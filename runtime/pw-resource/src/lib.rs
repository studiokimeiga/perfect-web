//! E4 — the local resource runtime.
//!
//! Charter §14 M4 task 3 lists what a resource runtime owes the language:
//! one in-flight request per key, request deduplication, reference-counted
//! subscribers, cancellation when unused, bounded retries with jitter, a
//! deterministic test clock, trace events, and private/public cache separation.
//!
//! These are **behaviour**, like E2A-R (ADR-0016). A passing test here does not
//! make misuse a compile error; the compiler-side half of E4 is the resource
//! manifest in `pw-core::hir`.
//!
//! # Why a test clock
//!
//! Freshness, timeouts and retry backoff are all time. A test that sleeps is
//! slow and flaky, and a flaky concurrency test gets muted rather than fixed.
//! [`Clock`] is advanced by the test, so every timing property here is exact.

pub mod entry;
pub use entry::{
    DeploymentIdentityKey, DevelopmentIdentityKey, EntryIdentity, ExplicitIdentityKey,
    IdentityKeyProvider, Partition, ResourceEntryId, Version,
};

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, ThreadId};
use std::time::Duration;

/// Milliseconds since the runtime started.
pub type Millis = u64;

/// A clock the tests own.
///
/// Real time appears only when a host advances this from a real source, which
/// keeps every timing rule in one place instead of scattered across `Instant`
/// calls that cannot be controlled.
#[derive(Clone, Default)]
pub struct Clock(Arc<AtomicU64>);

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn now(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
    pub fn advance(&self, ms: Millis) {
        let _ = self
            .0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                Some(now.saturating_add(ms))
            });
    }
}

/// Which cache a value may be stored in.
///
/// The separation is the point: charter §7.8 and the M4 gate both say private
/// state must never appear in public cache output. Both lookup and public
/// export enforce this classification; keys must still be scoped by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Privacy {
    /// Shareable between users.
    Public,
    /// Belongs to one session and must never be served to another.
    Private,
}

/// What a resource is asked for: its name plus the key that identifies the row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub resource: String,
    pub key: String,
}

impl Key {
    pub fn new(resource: &str, key: &str) -> Self {
        Self {
            resource: resource.to_string(),
            key: key.to_string(),
        }
    }
}

/// Something the runtime did, in order. Charter §14 M4 task 12 wants causal
/// traces; this is the record they are built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trace {
    RequestStarted {
        key: Key,
        attempt: u32,
    },
    RequestSucceeded {
        key: Key,
    },
    RequestFailed {
        key: Key,
        attempt: u32,
        error: String,
    },
    RequestDeduplicated {
        key: Key,
    },
    /// **A read made alone** ([`Resources::fetch_cancellable_as_of`]; NEXT
    /// 4c, track `render-merge`): its reader's snapshot is older than its
    /// resource's last invalidation, so it is read from no cache, joins no
    /// request and is joined by none, and its value is not kept.
    RequestAlone {
        key: Key,
    },
    ServedFromCache {
        key: Key,
    },
    /// **The last value kept, served because the origin failed** (ADR-0177):
    /// only for public data whose manifest declares `last_known_good`.
    ServedLastKnownGood {
        key: Key,
    },
    Cancelled {
        key: Key,
        reason: &'static str,
    },
    Subscribed {
        key: Key,
        subscribers: usize,
    },
    Unsubscribed {
        key: Key,
        subscribers: usize,
    },
    RetryScheduled {
        key: Key,
        attempt: u32,
        delay: Millis,
    },
    GaveUp {
        key: Key,
        attempts: u32,
    },
    RequestTimedOut {
        key: Key,
    },
    CommandOutcomeUnknown {
        idempotency_key: String,
    },
    CommandApplied {
        idempotency_key: String,
    },
    CommandDeduplicated {
        idempotency_key: String,
    },
}

/// How a resource behaves. The runtime half of the manifest the compiler
/// records in `pw-core::hir::Policy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub resource: String,
    pub privacy: Privacy,
    /// How long a cached value stays usable.
    pub freshness: Millis,
    /// Attempts in total, not retries after the first. 1 means "do not retry".
    pub max_attempts: u32,
    /// Base backoff; attempt *n* waits `base * 2^(n-1)`, plus deterministic
    /// jitter derived from the key rather than from a random source — a test
    /// that cannot predict the delay cannot assert on it.
    pub retry_base: Millis,
    /// **Whether the delays vary**, as the declaration's `retry` says
    /// (ADR-0215): `jitter = false` waits exactly `base * 2^(n-1)`. Until
    /// ADR-0215 every query jittered, whatever it declared.
    pub jitter: bool,
    /// Whole-flight budget, including retries, measured by the injected clock.
    pub timeout: Millis,
    /// Whether a result is kept at all (ADR-0127). A declaration with no
    /// `cache` clause is not cached: concurrent requests still share one
    /// flight, and nothing outlives it.
    pub cacheable: bool,
    /// **What a read that fails at its origin is answered with**
    /// (ADR-0177): nothing, or for public data the last value kept.
    pub fallback: Fallback,
}

/// **A read's fallback when its origin fails** (ADR-0177, charter §15.6
/// test 18).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Fallback {
    /// The failure is the reader's.
    #[default]
    None,
    /// The last value kept, past its freshness, for public data alone: a
    /// session's own value is never served from a copy kept for it.
    LastKnownGood,
}

impl Manifest {
    pub fn new(resource: &str) -> Self {
        Self {
            resource: resource.to_string(),
            privacy: Privacy::Public,
            freshness: 30_000,
            max_attempts: 1,
            retry_base: 100,
            jitter: true,
            timeout: 2_000,
            cacheable: true,
            fallback: Fallback::None,
        }
    }
    pub fn uncached(mut self) -> Self {
        self.cacheable = false;
        self
    }
    pub fn private(mut self) -> Self {
        self.privacy = Privacy::Private;
        self
    }
    /// The last value kept, when the origin fails (ADR-0177). Public data
    /// alone: for private data it is never served.
    pub fn last_known_good(mut self) -> Self {
        self.fallback = Fallback::LastKnownGood;
        self
    }
    pub fn freshness(mut self, ms: Millis) -> Self {
        self.freshness = ms;
        self
    }
    pub fn attempts(mut self, n: u32) -> Self {
        self.max_attempts = n;
        self
    }
    /// Whether the delays vary (ADR-0215).
    pub fn jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// Backoff for `attempt`, doubling, with jitter that depends on the key.
    ///
    /// Jitter exists so a thundering herd does not retry in lockstep. Deriving
    /// it from the key rather than from randomness keeps it spread *across
    /// keys* while staying exactly predictable in a test.
    pub fn backoff(&self, attempt: u32, key: &str) -> Millis {
        let base = self
            .retry_base
            .saturating_mul(2u64.pow(attempt.saturating_sub(1).min(16)));
        let spread = base / 4;
        if spread == 0 || !self.jitter {
            return base;
        }
        let h = key
            .bytes()
            .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
        base.saturating_add(h % spread)
    }
}

#[derive(Debug, Clone)]
struct Entry<V> {
    value: V,
    stored_at: Millis,
    privacy: Privacy,
    /// Past its freshness whatever the clock says, and kept (ADR-0177).
    expired: bool,
}

/// Why a request lost permission to publish a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    NoSubscribers,
    Invalidated,
    TimedOut,
}

#[derive(Debug, Clone)]
enum QueryError {
    Load(String),
    Stopped(StopReason),
}

type QueryResult<V> = Result<V, QueryError>;

/// The result and its wakeup predicate have one mutex. No user code runs here.
struct Completion<T> {
    owner: ThreadId,
    result: Mutex<Option<T>>,
    changed: Condvar,
}

impl<T: Clone> Completion<T> {
    fn new() -> Self {
        Self {
            owner: thread::current().id(),
            result: Mutex::new(None),
            changed: Condvar::new(),
        }
    }

    fn get(&self) -> Option<T> {
        self.result.lock().expect("completion").clone()
    }

    /// First terminal outcome wins, including cancellation racing completion.
    fn finish(&self, value: T) -> T {
        let mut result = self.result.lock().expect("completion");
        let terminal = result.get_or_insert(value).clone();
        self.changed.notify_all();
        terminal
    }

    fn wait(&self) -> T {
        let mut result = self.result.lock().expect("completion");
        loop {
            if let Some(value) = &*result {
                return value.clone();
            }
            result = self.changed.wait(result).expect("completion");
        }
    }
}

struct Flight<V> {
    manifest: Manifest,
    started: Millis,
    completion: Completion<QueryResult<V>>,
}

impl<V> Flight<V> {
    fn expired(&self, now: Millis) -> bool {
        now >= self.started.saturating_add(self.manifest.timeout)
    }
}

/// A synchronous adapter may inspect this between interruptible operations.
/// Signalling cancellation fences publication; stopping foreign I/O is the
/// adapter's responsibility. A cancelled request cannot be made valid again.
pub struct Cancellation<V = String> {
    flight: Arc<Flight<V>>,
    clock: Clock,
}

impl<V> Clone for Cancellation<V> {
    fn clone(&self) -> Self {
        Self {
            flight: self.flight.clone(),
            clock: self.clock.clone(),
        }
    }
}

impl<V: Clone> Cancellation<V> {
    pub fn reason(&self) -> Option<StopReason> {
        match self.flight.completion.get() {
            Some(Err(QueryError::Stopped(reason))) => Some(reason),
            Some(_) => None,
            None if self.flight.expired(self.clock.now()) => Some(StopReason::TimedOut),
            None => None,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.reason().is_some()
    }
}

/// An uncertain command must not be silently retried as a new execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    OutcomeUnknown,
    Reentrant,
}

type CommandResult = Result<String, CommandError>;

struct State<V> {
    cache: HashMap<Key, Entry<V>>,
    in_flight: HashMap<Key, Arc<Flight<V>>>,
    subscribers: HashMap<Key, usize>,
    /// Reservations and results are kept for this runtime's lifetime.
    applied: HashMap<String, Arc<Completion<CommandResult>>>,
    trace: Vec<Trace>,
    /// **How many invalidations so far** (NEXT 4c, track `render-merge`):
    /// what a reader of a snapshot takes as its [`Resources::epoch`].
    epoch: u64,
    /// **Each resource's last invalidation**, by the epoch it made: one
    /// entry per resource, never per key, so as many as the program has.
    invalidated: HashMap<String, u64>,
}

impl<V> Default for State<V> {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            in_flight: HashMap::new(),
            subscribers: HashMap::new(),
            applied: HashMap::new(),
            trace: Vec::new(),
            epoch: 0,
            invalidated: HashMap::new(),
        }
    }
}

impl<V> State<V> {
    /// `resource` invalidated now: a reader whose snapshot is older reads it
    /// alone ([`Resources::fetch_cancellable_as_of`]).
    fn stamp(&mut self, resource: &str) {
        self.epoch += 1;
        self.invalidated.insert(resource.to_string(), self.epoch);
    }
}

/// The runtime. Cheap to clone; every clone shares one state.
///
/// **Generic over the value it caches** (2026-10-02): a host that holds
/// values of its own type is handed them by the fetch that cached them,
/// with no side table to keep in step. ADR-0127's server kept values by a
/// token in one, bounded by count per key, and under contention a reader
/// could be handed a token whose value the bound had already dropped.
pub struct Resources<V = String> {
    state: Arc<Mutex<State<V>>>,
    clock: Clock,
}

impl<V> Clone for Resources<V> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            clock: self.clock.clone(),
        }
    }
}

/// What a fetch did, so a caller can tell a cache hit from real work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched<V = String> {
    FromCache(V),
    Fresh(V),
    /// The origin failed, and the last value kept was served (ADR-0177).
    LastKnownGood(V),
    /// Another caller's request was already running; this one joined it.
    Deduplicated(V),
    Failed(String),
    Cancelled(StopReason),
    TimedOut,
}

impl Resources<String> {
    /// A runtime caching text, as every caller before 2026-10-02 did.
    pub fn new(clock: Clock) -> Self {
        Self::caching(clock)
    }
}

impl<V: Clone> Resources<V> {
    /// A runtime caching values of type `V`.
    pub fn caching(clock: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            clock,
        }
    }

    pub fn trace(&self) -> Vec<Trace> {
        self.state.lock().expect("state").trace.clone()
    }

    pub fn in_flight(&self, key: &Key) -> bool {
        self.state
            .lock()
            .expect("state")
            .in_flight
            .contains_key(key)
    }

    pub fn subscribers(&self, key: &Key) -> usize {
        self.state
            .lock()
            .expect("state")
            .subscribers
            .get(key)
            .copied()
            .unwrap_or(0)
    }

    /// Everything a public cache would serve. Used to assert that private
    /// values are not in it.
    pub fn public_cache_contents(&self) -> Vec<(Key, V)> {
        self.state
            .lock()
            .expect("state")
            .cache
            .iter()
            .filter(|(_, e)| e.privacy == Privacy::Public)
            .map(|(k, e)| (k.clone(), e.value.clone()))
            .collect()
    }

    /// Take a reference to a key. The value is kept while anyone holds one.
    pub fn subscribe(&self, key: &Key) -> Subscription<V> {
        let mut st = self.state.lock().expect("state");
        let n = st.subscribers.entry(key.clone()).or_insert(0);
        *n += 1;
        let count = *n;
        st.trace.push(Trace::Subscribed {
            key: key.clone(),
            subscribers: count,
        });
        Subscription {
            rt: self.clone(),
            key: key.clone(),
            released: false,
        }
    }

    /// Fetch using a synchronous callback. See `fetch_cancellable` for adapters
    /// that can stop work after the initiating owner leaves.
    pub fn fetch(
        &self,
        manifest: &Manifest,
        key: &Key,
        mut load: impl FnMut(u32) -> Result<V, String>,
    ) -> Fetched<V> {
        self.fetch_cancellable(manifest, key, |attempt, _| load(attempt))
    }

    /// Coalesce matching reads onto one owned flight and share its real result.
    /// User code runs without runtime locks. Deadlines use the injected clock,
    /// not a wall-clock preemption of an arbitrary synchronous callback.
    pub fn fetch_cancellable(
        &self,
        manifest: &Manifest,
        key: &Key,
        load: impl FnMut(u32, &Cancellation<V>) -> Result<V, String>,
    ) -> Fetched<V> {
        let fetched = self.fetch_from_origin(manifest, key, load, None);
        self.fallen_back(manifest, key, fetched)
    }

    /// **The invalidations so far** (NEXT 4c, track `render-merge`): what a
    /// reader whose data is a snapshot takes before it takes the snapshot,
    /// and fetches as of ([`Resources::fetch_cancellable_as_of`]).
    pub fn epoch(&self) -> u64 {
        self.state.lock().expect("state").epoch
    }

    /// **A fetch for a reader of a snapshot taken after `as_of`** (NEXT 4c,
    /// track `render-merge`: a host's render). Where nothing invalidated
    /// `key`'s resource since `as_of`, as [`Resources::fetch_cancellable`]:
    /// the cache, a request running, or the origin, kept and shared; what
    /// it hands the reader is then what its own snapshot holds, as far as
    /// invalidations say. Where something did, the reader's snapshot may be
    /// older than the change, and what the cache or a request running holds
    /// newer: `load` runs here, for this reader alone, its value neither
    /// taken from the cache nor shared nor kept, nothing but its timeout
    /// stops it, and a request running goes on as it was. So a value read in
    /// a snapshot older than an invalidation is never kept or handed to
    /// another reader after it.
    pub fn fetch_cancellable_as_of(
        &self,
        manifest: &Manifest,
        key: &Key,
        as_of: u64,
        load: impl FnMut(u32, &Cancellation<V>) -> Result<V, String>,
    ) -> Fetched<V> {
        let fetched = self.fetch_from_origin(manifest, key, load, Some(as_of));
        self.fallen_back(manifest, key, fetched)
    }

    /// `fetched`, or where its origin failed, the fallback its manifest
    /// declares.
    fn fallen_back(&self, manifest: &Manifest, key: &Key, fetched: Fetched<V>) -> Fetched<V> {
        // **The last value kept, where the origin failed** (ADR-0177): for
        // public data whose manifest declares it, and never for private
        // data, whatever a manifest says. A stopped read is not a failure.
        if matches!(fetched, Fetched::Failed(_) | Fetched::TimedOut)
            && manifest.fallback == Fallback::LastKnownGood
            && manifest.privacy == Privacy::Public
        {
            let mut st = self.state.lock().expect("state");
            if let Some(value) = st
                .cache
                .get(key)
                .filter(|e| e.privacy == Privacy::Public)
                .map(|e| e.value.clone())
            {
                st.trace
                    .push(Trace::ServedLastKnownGood { key: key.clone() });
                return Fetched::LastKnownGood(value);
            }
        }
        fetched
    }

    /// [`Resources::fetch_cancellable`], without a fallback: the cache, a
    /// flight, or the origin; or, for a reader of a snapshot older than its
    /// resource's last invalidation (`as_of`), the origin alone
    /// ([`Resources::fetch_cancellable_as_of`]).
    fn fetch_from_origin(
        &self,
        manifest: &Manifest,
        key: &Key,
        mut load: impl FnMut(u32, &Cancellation<V>) -> Result<V, String>,
        as_of: Option<u64>,
    ) -> Fetched<V> {
        if manifest.resource != key.resource {
            return Fetched::Failed("manifest resource does not match the key".into());
        }
        let flight;
        // A request of this reader's alone: from no cache, joining none and
        // joined by none, and its value not kept.
        let alone;
        {
            let mut st = self.state.lock().expect("state");
            let now = self.clock.now();
            alone = as_of.is_some_and(|as_of| {
                st.invalidated
                    .get(&key.resource)
                    .is_some_and(|&at| at > as_of)
            });
            if alone {
                st.trace.push(Trace::RequestAlone { key: key.clone() });
            } else {
                if let Some(entry) = st.cache.get(key) {
                    if entry.privacy != manifest.privacy {
                        return Fetched::Failed("cache privacy does not match the manifest".into());
                    }
                    if !entry.expired && now.saturating_sub(entry.stored_at) < manifest.freshness {
                        let value = entry.value.clone();
                        st.trace.push(Trace::ServedFromCache { key: key.clone() });
                        return Fetched::FromCache(value);
                    }
                }
                if let Some(existing) = st.in_flight.get(key).cloned() {
                    if existing.manifest != *manifest {
                        return Fetched::Failed(
                            "in-flight policy does not match the manifest".into(),
                        );
                    }
                    if existing.completion.owner == thread::current().id() {
                        return Fetched::Failed(
                            "a request cannot synchronously join itself".into(),
                        );
                    }
                    st.trace
                        .push(Trace::RequestDeduplicated { key: key.clone() });
                    drop(st);
                    return Self::fetched(self.join_query(key, &existing), true);
                }
            }
            flight = Arc::new(Flight {
                manifest: manifest.clone(),
                started: now,
                completion: Completion::new(),
            });
            // A request alone is not the key's: the one running stays it.
            if !alone {
                st.in_flight.insert(key.clone(), flight.clone());
            }
        }
        let cancellation = Cancellation {
            flight: flight.clone(),
            clock: self.clock.clone(),
        };
        let attempts = manifest.max_attempts.max(1);
        for attempt in 1..=attempts {
            {
                let mut st = self.state.lock().expect("state");
                if let Some(outcome) = Self::stopped(&mut st, key, &flight, self.clock.now()) {
                    return Self::fetched(outcome, false);
                }
                st.trace.push(Trace::RequestStarted {
                    key: key.clone(),
                    attempt,
                });
            }
            let result = catch_unwind(AssertUnwindSafe(|| load(attempt, &cancellation)));
            let result = match result {
                Ok(result) => result,
                Err(payload) => {
                    let _ = self.finish_query(
                        key,
                        &flight,
                        Err(QueryError::Load("loader panicked".into())),
                    );
                    resume_unwind(payload);
                }
            };
            let mut st = self.state.lock().expect("state");
            if let Some(outcome) = Self::stopped(&mut st, key, &flight, self.clock.now()) {
                return Self::fetched(outcome, false);
            }
            match result {
                Ok(value) => {
                    if manifest.cacheable && !alone {
                        st.cache.insert(
                            key.clone(),
                            Entry {
                                value: value.clone(),
                                stored_at: self.clock.now(),
                                privacy: manifest.privacy,
                                expired: false,
                            },
                        );
                    }
                    st.trace.push(Trace::RequestSucceeded { key: key.clone() });
                    let outcome = Self::finish_locked(&mut st, key, &flight, Ok(value));
                    return Self::fetched(outcome, false);
                }
                Err(error) => {
                    st.trace.push(Trace::RequestFailed {
                        key: key.clone(),
                        attempt,
                        error: error.clone(),
                    });
                    if attempt == attempts {
                        st.trace.push(Trace::GaveUp {
                            key: key.clone(),
                            attempts,
                        });
                        let outcome = Self::finish_locked(
                            &mut st,
                            key,
                            &flight,
                            Err(QueryError::Load(error)),
                        );
                        return Self::fetched(outcome, false);
                    }
                    let delay = manifest.backoff(attempt, &key.key);
                    st.trace.push(Trace::RetryScheduled {
                        key: key.clone(),
                        attempt,
                        delay,
                    });
                    // This existing local-clock runtime simulates backoff. Do
                    // not mistake advancing logical time for sleeping real I/O.
                    let remaining = manifest
                        .timeout
                        .saturating_sub(self.clock.now().saturating_sub(flight.started));
                    self.clock.advance(delay.min(remaining));
                }
            }
        }
        unreachable!("at least one attempt; final attempt returns")
    }

    fn fetched(result: QueryResult<V>, joined: bool) -> Fetched<V> {
        match result {
            Ok(value) if joined => Fetched::Deduplicated(value),
            Ok(value) => Fetched::Fresh(value),
            Err(QueryError::Load(error)) => Fetched::Failed(error),
            Err(QueryError::Stopped(StopReason::TimedOut)) => Fetched::TimedOut,
            Err(QueryError::Stopped(reason)) => Fetched::Cancelled(reason),
        }
    }

    /// Lock order is always state -> completion. Joiners release completion
    /// before asking the runtime to expire a flight, preventing lock inversion.
    fn join_query(&self, key: &Key, flight: &Arc<Flight<V>>) -> QueryResult<V> {
        let mut result = flight.completion.result.lock().expect("completion");
        loop {
            if let Some(value) = &*result {
                return value.clone();
            }
            if flight.expired(self.clock.now()) {
                drop(result);
                return self.finish_query(
                    key,
                    flight,
                    Err(QueryError::Stopped(StopReason::TimedOut)),
                );
            }
            let (next, _) = flight
                .completion
                .changed
                .wait_timeout(result, Duration::from_millis(10))
                .expect("completion");
            result = next;
        }
    }

    fn stopped(
        st: &mut State<V>,
        key: &Key,
        flight: &Arc<Flight<V>>,
        now: Millis,
    ) -> Option<QueryResult<V>> {
        if let Some(outcome) = flight.completion.get() {
            return Some(outcome);
        }
        if flight.expired(now) {
            return Some(Self::finish_locked(
                st,
                key,
                flight,
                Err(QueryError::Stopped(StopReason::TimedOut)),
            ));
        }
        None
    }

    fn finish_query(
        &self,
        key: &Key,
        flight: &Arc<Flight<V>>,
        result: QueryResult<V>,
    ) -> QueryResult<V> {
        let mut st = self.state.lock().expect("state");
        Self::finish_locked(&mut st, key, flight, result)
    }

    fn finish_locked(
        st: &mut State<V>,
        key: &Key,
        flight: &Arc<Flight<V>>,
        result: QueryResult<V>,
    ) -> QueryResult<V> {
        if let Some(previous) = flight.completion.get() {
            return previous;
        }
        match &result {
            Err(QueryError::Stopped(StopReason::TimedOut)) => {
                st.trace.push(Trace::RequestTimedOut { key: key.clone() });
            }
            Err(QueryError::Stopped(reason)) => {
                st.trace.push(Trace::Cancelled {
                    key: key.clone(),
                    reason: match reason {
                        StopReason::NoSubscribers => "no subscribers remain",
                        StopReason::Invalidated => "resource invalidated",
                        StopReason::TimedOut => unreachable!(),
                    },
                });
            }
            _ => {}
        }
        let terminal = flight.completion.finish(result);
        // Revoked work must never delete a newer request for the same key.
        if st
            .in_flight
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, flight))
        {
            st.in_flight.remove(key);
        }
        terminal
    }

    /// Compatibility wrapper. An unknown outcome is not a successful string.
    /// Use `try_command` to handle that state explicitly.
    pub fn command(&self, idempotency_key: &str, apply: impl FnOnce() -> String) -> String {
        self.try_command(idempotency_key, apply)
            .expect("command outcome is not known to be successful")
    }

    /// Reserve before execution, share completion, and never re-execute after
    /// an unwinding callback. This is process-local, not durable exactly-once.
    /// The host must bind identity to authority, operation and payload.
    pub fn try_command(
        &self,
        idempotency_key: &str,
        apply: impl FnOnce() -> String,
    ) -> CommandResult {
        self.try_command_result(idempotency_key, || Ok(apply()))
    }

    /// A fallible host application can report an uncertain acknowledgement
    /// without inventing a completed rollback answer. Unknown completions are
    /// retained and cannot be forgotten/re-executed by reservation eviction.
    pub fn try_command_result(
        &self,
        idempotency_key: &str,
        apply: impl FnOnce() -> CommandResult,
    ) -> CommandResult {
        let completion;
        {
            let mut st = self.state.lock().expect("state");
            if let Some(existing) = st.applied.get(idempotency_key).cloned() {
                if existing.owner == thread::current().id() && existing.get().is_none() {
                    return Err(CommandError::Reentrant);
                }
                st.trace.push(Trace::CommandDeduplicated {
                    idempotency_key: idempotency_key.into(),
                });
                drop(st);
                return existing.wait();
            }
            completion = Arc::new(Completion::new());
            st.applied
                .insert(idempotency_key.into(), completion.clone());
        }
        match catch_unwind(AssertUnwindSafe(apply)) {
            Ok(value) => {
                let mut st = self.state.lock().expect("state");
                let value = value.map_err(|_| CommandError::OutcomeUnknown);
                st.trace.push(if value.is_ok() {
                    Trace::CommandApplied {
                        idempotency_key: idempotency_key.into(),
                    }
                } else {
                    Trace::CommandOutcomeUnknown {
                        idempotency_key: idempotency_key.into(),
                    }
                });
                completion.finish(value)
            }
            Err(payload) => {
                {
                    let mut st = self.state.lock().expect("state");
                    st.trace.push(Trace::CommandOutcomeUnknown {
                        idempotency_key: idempotency_key.into(),
                    });
                    let _ = completion.finish(Err(CommandError::OutcomeUnknown));
                }
                resume_unwind(payload);
            }
        }
    }

    /// **Forget a finished command's outcome**, so the host can bound what it
    /// keeps (ADR-0121). A key invoked again after this runs again: that is
    /// the bound's cost, and the host chooses it. Two outcomes are kept:
    /// a reservation still running, because forgetting it would let a
    /// duplicate start beside it; and an unknown outcome, because forgetting
    /// that would re-execute a command that may have taken effect, which is
    /// what `try_command` exists to prevent. Returns whether one was forgotten.
    pub fn forget_command(&self, idempotency_key: &str) -> bool {
        let mut st = self.state.lock().expect("state");
        let finished = st
            .applied
            .get(idempotency_key)
            .is_some_and(|c| matches!(c.get(), Some(Ok(_))));
        if finished {
            st.applied.remove(idempotency_key);
        }
        finished
    }

    /// **A finished command's kept outcome**, read without running or
    /// waiting for anything: `None` where the key has none, where its
    /// command is still running, or where its outcome is unknown. A host
    /// reads it to answer a retry it would not run again (track
    /// `store-accounts`).
    /// A completed reservation, preserving an uncertain outcome for recovery.
    /// Pending/absent outcomes are None; a caller must never turn Err into rollback.
    pub fn command_result(&self, idempotency_key: &str) -> Option<CommandResult> {
        let st = self.state.lock().expect("state");
        st.applied
            .get(idempotency_key)
            .and_then(|completion| completion.get())
    }

    pub fn command_outcome(&self, idempotency_key: &str) -> Option<String> {
        let st = self.state.lock().expect("state");
        match st.applied.get(idempotency_key)?.get() {
            Some(Ok(value)) => Some(value),
            _ => None,
        }
    }

    /// How many command outcomes are held: what a host bounds.
    pub fn commands_held(&self) -> usize {
        self.state.lock().expect("state").applied.len()
    }

    /// **Invalidate one entry** (ADR-0127): what a command's `invalidates
    /// Cart(current_session())` names, and not every session's cart. Its stored
    /// value goes, and a flight for it loses the right to publish.
    pub fn invalidate_key(&self, key: &Key) {
        let mut st = self.state.lock().expect("state");
        // Kept or not: a reader of an older snapshot may be about to fill it
        // (NEXT 4c).
        st.stamp(&key.resource);
        st.cache.remove(key);
        if let Some(flight) = st.in_flight.get(key).cloned() {
            let _ = Self::finish_locked(
                &mut st,
                key,
                &flight,
                Err(QueryError::Stopped(StopReason::Invalidated)),
            );
        }
    }

    /// **Forget stored values a host no longer needs** (ADR-0127): a forgotten
    /// session's private entries. Flights are left to finish.
    pub fn evict_where(&self, mut forget: impl FnMut(&Key) -> bool) {
        self.state
            .lock()
            .expect("state")
            .cache
            .retain(|key, _| !forget(key));
    }

    /// How many values are stored: what a host bounds.
    pub fn stored(&self) -> usize {
        self.state.lock().expect("state").cache.len()
    }

    /// Invalidate both stored values and publication rights of outstanding work.
    /// **Each value kept of `resource`, expired and kept** (ADR-0177): the
    /// next reader asks its origin, and a public value declared
    /// `last_known_good` is still there to answer with if the origin fails.
    pub fn expire(&self, resource: &str) {
        let mut st = self.state.lock().expect("state");
        st.stamp(resource);
        for (key, entry) in st.cache.iter_mut() {
            if key.resource == resource {
                entry.expired = true;
            }
        }
    }

    /// **Drop every entry of `resource` whose key `names`** (ADR-0256), as
    /// [`Resources::invalidate_key`] drops one: a flight for one ends
    /// invalidated. Answers what it dropped, a flight's key among them, by
    /// key.
    pub fn invalidate_where(
        &self,
        resource: &str,
        mut names: impl FnMut(&str) -> bool,
    ) -> Vec<Key> {
        let mut st = self.state.lock().expect("state");
        // Whatever it drops, none included: a reader of an older snapshot
        // may be about to fill one (NEXT 4c).
        st.stamp(resource);
        let mut dropped: Vec<Key> = st
            .cache
            .keys()
            .filter(|k| k.resource == resource && names(&k.key))
            .cloned()
            .collect();
        for key in &dropped {
            st.cache.remove(key);
        }
        let flights: Vec<_> = st
            .in_flight
            .iter()
            .filter(|(k, _)| k.resource == resource && names(&k.key))
            .map(|(k, f)| (k.clone(), f.clone()))
            .collect();
        for (key, flight) in flights {
            let _ = Self::finish_locked(
                &mut st,
                &key,
                &flight,
                Err(QueryError::Stopped(StopReason::Invalidated)),
            );
            if !dropped.contains(&key) {
                dropped.push(key);
            }
        }
        dropped.sort_by(|a, b| a.key.cmp(&b.key));
        dropped
    }

    pub fn invalidate(&self, resource: &str) {
        let mut st = self.state.lock().expect("state");
        st.stamp(resource);
        st.cache.retain(|key, _| key.resource != resource);
        let obsolete: Vec<_> = st
            .in_flight
            .iter()
            .filter(|(key, _)| key.resource == resource)
            .map(|(key, flight)| (key.clone(), flight.clone()))
            .collect();
        for (key, flight) in obsolete {
            let _ = Self::finish_locked(
                &mut st,
                &key,
                &flight,
                Err(QueryError::Stopped(StopReason::Invalidated)),
            );
        }
    }

    fn release(&self, key: &Key) {
        let mut st = self.state.lock().expect("state");
        let Some(count) = st.subscribers.get_mut(key) else {
            return;
        };
        *count -= 1;
        let count = *count;
        if count == 0 {
            st.subscribers.remove(key);
        }
        st.trace.push(Trace::Unsubscribed {
            key: key.clone(),
            subscribers: count,
        });
        if count == 0
            && let Some(flight) = st.in_flight.get(key).cloned()
        {
            let _ = Self::finish_locked(
                &mut st,
                key,
                &flight,
                Err(QueryError::Stopped(StopReason::NoSubscribers)),
            );
        }
    }
}

/// A reference to a key. Dropping it releases the reference, and the last one
/// out cancels work nobody is waiting for.
pub struct Subscription<V: Clone = String> {
    rt: Resources<V>,
    key: Key,
    released: bool,
}

impl<V: Clone> Subscription<V> {
    pub fn key(&self) -> &Key {
        &self.key
    }
    /// Release early, rather than at end of scope.
    pub fn release(mut self) {
        self.rt.release(&self.key);
        self.released = true;
    }
}

impl<V: Clone> Drop for Subscription<V> {
    fn drop(&mut self) {
        if !self.released {
            self.rt.release(&self.key);
        }
    }
}

#[cfg(test)]
mod fallible_command_tests {
    use super::*;

    #[test]
    fn uncertain_host_acknowledgements_are_never_memoized_as_rollback_or_reexecuted() {
        let resources = Resources::<String>::new(Clock::new());
        let first = resources.try_command_result("unknown", || Err(CommandError::OutcomeUnknown));
        assert_eq!(first, Err(CommandError::OutcomeUnknown));
        assert!(resources.command_outcome("unknown").is_none());
        assert!(
            !resources.forget_command("unknown"),
            "unknown reservations are retained"
        );
        let retried = resources.try_command_result("unknown", || {
            panic!("unknown operation must never run twice")
        });
        assert_eq!(retried, Err(CommandError::OutcomeUnknown));
        assert_eq!(
            resources.try_command_result("known", || Ok("committed".into())),
            Ok("committed".into())
        );
        assert_eq!(resources.command_outcome("known"), Some("committed".into()));
        assert!(
            resources.forget_command("known"),
            "known outcomes keep their existing bounded eviction contract"
        );
    }
}
