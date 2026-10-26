//! E7-P's development host.
//!
//! **Not E8's production host.** Its deletion condition is explicit: E8
//! replaces it with the capability-constrained production host while preserving
//! the `pw-protocol` browser boundary. Saying that plainly is what stops a
//! temporary adapter quietly becoming architecture.
//!
//! # What it exists to remove
//!
//! ```text
//! before                        after
//!
//! browser                       browser
//!   ↓                             ↓ pw-protocol
//! Node spike                    Rust dev server
//!   ↓                             ├── pw-resource
//! hand-built approximations       ├── pw-materialize
//!                                 ├── pw-render
//!                                 └── the compiler's emitted graph
//! ```
//!
//! The Node spike reconstructed E6's semantics in JavaScript — a cart counter,
//! an events array, a version integer. Every one of those was a second
//! implementation of something the Rust runtime already owned, and validating
//! an internal service boundary that exists to be deleted is wasted work.
//!
//! # What the browser knows
//!
//! `ResourceEntryId`, `Version`, `PartAddress`, `StreamFrame`, `Patch`. Not
//! `EntryKey`, not SQLite, not outbox rows, not compiler graph nodes. The
//! boundary is asserted, not assumed — see `e2e/protocol-boundary.spec.mjs`.
//!
//! # Its commands are compiled Pleris (E10-I)
//!
//! `add_to_cart` and `clear_cart` run as the components the compiler built
//! from `examples/store/app.pw` (`docs/evidence/E10/<component id>.wasm`),
//! admitted against each artifact's real imports and executed through the E8
//! host's engine. This server supplies only the DEPLOYMENT: the platform's
//! session operation, and `store:data/carts`, the data layer whose staged write
//! commits with its event only when the compiled command returns `Ok`. The Rust
//! closures that computed the commands are deleted, and
//! `the_commands_run_as_compiled_components` keeps them deleted.
//!
//! # Deliberately not
//!
//! A deployment abstraction, a production authentication system, a plugin host,
//! a Wasm executor of its own (the one it uses is `pw-host`'s), an HTTP/3
//! experiment, or a distributed materializer. This development deployment
//! explicitly models every local session as `SignedIn` so the store demo can
//! exercise command authorization; any other predicate is refused. Real identity
//! verification belongs to the deployment, not to this test server.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use pw_document::{IdentityDomain, LocalPartId, PartAddress, Partition, TemplateSchemaId};
use pw_host::engine::{HostFn, Val};
use pw_host::{Admission, ComponentContract, Granted, Limits, Node, Topology, admit};
use pw_materialize::{Clock, EntryKey, FragmentPolicy, Materializer};
use pw_protocol::{
    CURRENT, CausalBasis, Patch, PatchOp, PatchSet, Recovery, ResourceEntryId, StreamFrame,
    Targeted, Version,
};
use pw_render::{Env, PartId, Settled, Template, Value};
use pw_resource::{DevelopmentIdentityKey, EntryIdentity};

/// One subscriber's undelivered frames, and where its cursor is.
///
/// # Why frames are not removed when they are read
///
/// The first transport drained the queue on read and wrote the frames to the
/// socket. A page that reloaded left an in-flight long poll behind; that
/// request's thread woke, took the frames the reload had not yet asked for,
/// wrote them to a socket nobody was reading, and returned. The frames were
/// gone and the new page waited forever — reproducibly, and only when a
/// reload raced a change, which is why it looked like flakiness.
///
/// So delivery is acknowledged rather than assumed. Each frame carries a
/// sequence number; a subscriber asks for everything after the last sequence
/// it APPLIED; and the server drops a frame only once a later request proves
/// the client got past it. Writing to a dead socket now loses nothing.
///
/// # Why a subscriber is bounded, and forgotten
///
/// A frame leaves the queue only when the subscriber acknowledges it, and a
/// menu change is pushed to every subscriber. So a visitor who closed the tab
/// kept every later change forever: 1,000 visitors and 300 menu changes held
/// 600,000 frames (`docs/evidence/E10/load.txt`). A queue now holds at most
/// [`MAX_WAITING`] frames. A subscriber that falls further behind gets one
/// `Recovery::Reload` instead, the protocol's answer to "this document cannot
/// continue", and nothing more until it is re-rendered. A subscriber that has
/// not asked for [`IDLE`] is forgotten, and if it asks again it is told to
/// reload, because what it missed can no longer be said.
struct Subscriber {
    /// The last sequence assigned. Sequences start at ONE, so that the
    /// initial cursor — zero, meaning "nothing acknowledged yet" — is smaller
    /// than every frame. With zero-based sequences the very first frame a
    /// subscriber ever received was numbered zero, `since=0` read it as
    /// already acknowledged, and it was never delivered. It cost a real
    /// invalidation and looked like a transport that simply had nothing to say.
    ///
    /// Serving a document takes a sequence number too, so a served page's
    /// cursor is never zero. A poll with a nonzero cursor from a subscriber
    /// the server has forgotten is then known to be a page that missed
    /// frames, never mistaken for a new one.
    last_seq: u64,
    frames: Vec<(u64, StreamFrame)>,
    /// When it last asked for frames or was served a document.
    seen: std::time::Instant,
    /// It fell more than [`MAX_WAITING`] frames behind: its queue is one
    /// `Recovery::Reload`, and further frames are dropped until it is
    /// re-rendered.
    behind: bool,
    /// **Every frame pushed to it, kept or dropped** (ADR-0151). A document
    /// is read outside the table, and this says whether a change reached its
    /// session meanwhile. Not `last_seq`, which a document's cursor moves
    /// too: two documents read at once would each make the other read again.
    pushed: u64,
    /// **What happened to its frames, the latest [`TRAIL`] notes**: each
    /// pushed, each stream opened and how it ended, each batch written, and
    /// each pass that waited for the table. A failing browser test attaches
    /// it (`/bench/records`), beside the runtime's own record: WebKit's "Load
    /// more" kept its twenty rows on CI with the read applied and the stream
    /// never seen to end, and nothing said where the frames were.
    trail: std::collections::VecDeque<String>,
    /// **A telling passed it by** (ADR-0297): no page of it had asked for
    /// [`LIVE`], so what changed was not derived for it then; it is, when
    /// it next asks.
    stale: bool,
}

/// How many notes a document's trail keeps.
const TRAIL: usize = 400;

/// **How long since a document's page last asked, at most, for a telling
/// to derive it** (ADR-0297). A page asks without pause: a stream holds two
/// seconds and is opened again at once, a long poll one. Until ADR-0297 a
/// commit derived every document of every session that read what it
/// dropped, one session after another, those of pages closed up to
/// [`IDLE`] before among them: on CI a reader's change waited behind 47 of
/// them, five seconds, and "a follow reaches another reader" failed.
const LIVE: std::time::Duration = std::time::Duration::from_secs(3);

/// **How many sessions a commit tells at once** (ADR-0297), each in its own
/// hold: one reader's telling no longer waits for another's.
const TELLERS: usize = 8;

/// **A session, as a record names it**: a hash of its id, which is its
/// cookie and never written where another session can read it.
fn masked(session: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session.hash(&mut hasher);
    format!("s{:08x}", hasher.finish() as u32)
}

/// **Milliseconds since this server started**, the clock its records read.
fn uptime_ms() -> u128 {
    static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    STARTED
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

/// **A hold of the table, timed**: noted on [`Server::slow`] when it is
/// let go past 50 ms. Declared after the guard it times, so it is dropped
/// first.
struct Held<'a> {
    server: &'a Server,
    site: &'static str,
    at: std::time::Instant,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let ms = self.at.elapsed().as_millis();
        if ms < 50 {
            return;
        }
        let mut slow = self.server.slow.lock().expect("slow");
        if slow.len() >= 200 {
            slow.pop_front();
        }
        slow.push_back(format!(
            "{} ms {} held the table {ms} ms",
            uptime_ms(),
            self.site
        ));
    }
}

/// **Is a document still showing what a change was derived against?**
/// The same value, or none either time: each is replaced whole, never
/// changed in place.
fn same_shown(was: &Option<Arc<Shown>>, now: &Option<Arc<Shown>>) -> bool {
    match (was, now) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// A frame's kind, for a trail.
fn frame_kind(frame: &StreamFrame) -> &'static str {
    match frame {
        StreamFrame::ResourceChanged { .. } => "a change",
        StreamFrame::Patch(_) => "a patch",
        StreamFrame::PatchSet(_) => "a patch set",
        StreamFrame::Recovery { .. } => "a recovery",
        StreamFrame::EntryValue { .. } => "an entry's value",
    }
}

impl Default for Subscriber {
    fn default() -> Subscriber {
        Subscriber::at(0)
    }
}

impl Subscriber {
    /// **When a page that has just been served, or subscribed, asked**
    /// (ADR-0297): now. In a test build, an hour on: a test serves a document
    /// and then drives the server by calls, with no page left to ask again,
    /// so [`LIVE`] was three seconds of the machine's wall clock between its
    /// serve and its commit, and a slow machine (CI's, a loaded one) ran
    /// them out: the document was passed by, nothing was derived for it, and
    /// the test failed for its machine's speed (a burst of posts, a message,
    /// the page and command counts). A page "asks without pause"
    /// (ADR-0297), which is the premise of every such test. A test of the
    /// window itself sets `seen` to the instant it means
    /// (`a_document_no_page_asks_for_is_told_when_one_asks`), and one of
    /// the idle limit builds its subscribers (`Subscriber::at`) at the
    /// instant it means.
    fn asked_at() -> std::time::Instant {
        #[cfg(test)]
        {
            std::time::Instant::now() + std::time::Duration::from_secs(60 * 60)
        }
        #[cfg(not(test))]
        {
            std::time::Instant::now()
        }
    }

    /// A document's subscriber, its sequence starting at the document's
    /// cursor (ADR-0161): every frame for it comes after the document.
    fn at(cursor: u64) -> Subscriber {
        Subscriber {
            last_seq: cursor,
            frames: Vec::new(),
            seen: std::time::Instant::now(),
            behind: false,
            pushed: 0,
            trail: std::collections::VecDeque::from([format!(
                "{} ms subscribed at {cursor}",
                uptime_ms()
            )]),
            stale: false,
        }
    }

    /// **Has a page of it asked within [`LIVE`]?** (ADR-0297)
    fn live(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.seen) < LIVE
    }

    /// A note on its trail, the oldest let go past [`TRAIL`].
    fn note(&mut self, what: std::fmt::Arguments<'_>) {
        if self.trail.len() >= TRAIL {
            self.trail.pop_front();
        }
        self.trail.push_back(format!("{} ms {what}", uptime_ms()));
    }
}

/// How long the kitchen takes to answer (E14, T02): long enough that pages
/// opened together are asking at once.
const KITCHEN_MS: u64 = 300;

/// The most frames one subscriber may have waiting.
const MAX_WAITING: usize = 256;

/// **What a command answered** (ADR-0157): whether it committed, and its
/// value in the wire form a handler's compiled code decodes, `Ok` or a
/// declared `Err`. A command that trapped or was refused answered no value,
/// and `why` says what happened instead.
#[derive(Debug, Clone, PartialEq)]
struct Answered {
    committed: bool,
    result: Option<serde_json::Value>,
    why: Option<String>,
    /// **The predicate its `requires` refused it by** (ADR-0302), kept with
    /// the interaction, so a resent press is answered as the press was:
    /// until ADR-0302 the refusal lived only on the thread that ran
    /// `requires`, and a resend, answered from what was kept, was answered
    /// as a command that did not commit, with nothing to tell.
    refused: Option<String>,
}

impl Answered {
    /// The answer as an interaction keeps it, so a retried request is given
    /// the first one's (ADR-0121). `result` is written only when there is
    /// one: a command declaring no `Result` answers `null`, which is an
    /// answer, and a command that trapped answers none.
    fn kept(&self) -> String {
        let mut kept = serde_json::json!({
            "committed": self.committed,
            "why": self.why,
            "refused": self.refused,
        });
        if let Some(result) = &self.result {
            kept["result"] = result.clone();
        }
        kept.to_string()
    }

    fn from_kept(kept: &str) -> Answered {
        let v: serde_json::Value = serde_json::from_str(kept).unwrap_or_default();
        Answered {
            committed: v["committed"] == true,
            result: v.get("result").cloned(),
            why: v["why"].as_str().map(str::to_string),
            refused: v["refused"].as_str().map(str::to_string),
        }
    }
}

/// **A document's keyed reads** (ADR-0152, ADR-0161): each binding a signal
/// keys, by its name.
#[derive(Default)]
struct Keyed {
    reads: BTreeMap<String, KeyRead>,
}

/// **A `keep` read's turn on its binding** (ADR-0152), once the read before
/// it lets go. In a test's build a read that finds the turn held tells the
/// test first, if the test asked (`tell_when_a_turn_waits`), and then waits:
/// "waiting for the read before it" is a fact the turn can state, where "has
/// not returned yet" is a silence that a slow read makes too.
fn held_turn(turn: &Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    #[cfg(test)]
    if let Err(std::sync::TryLockError::WouldBlock) = turn.try_lock() {
        TURN_WAITING.with(|tell| {
            if let Some(tell) = &*tell.borrow() {
                tell();
            }
        });
    }
    turn.lock().expect("turn")
}

#[cfg(test)]
std::thread_local! {
    /// What this thread's test asked to be told when a `keep` read finds its
    /// binding's turn held (`tell_when_a_turn_waits`).
    static TURN_WAITING: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// **One binding's reads** (ADR-0152).
#[derive(Default)]
struct KeyRead {
    /// The latest read the browser asked for, by its number.
    latest: u64,
    /// The key the page shows: each signal's value in the last read applied.
    /// A signal absent here shows its first value.
    shown: BTreeMap<String, serde_json::Value>,
    /// A `cancel` read's hold on its key's flight, with the read's number.
    /// Let go when a newer read is asked or the browser leaves, so that
    /// `pw-resource` stops the flight when nobody else holds it.
    hold: Option<(u64, pw_resource::Subscription<Arc<Val>>)>,
    /// **One `keep` read at a time**: each waits for the one before it.
    turn: Arc<Mutex<()>>,
}

/// **Which key a binding a signal keys is read for** (ADR-0152).
enum Keys<'a> {
    /// A new document's: each signal's first value.
    First,
    /// What the session's page shows: the key last applied for each binding.
    Shown,
    /// A read the browser asked for: this binding's new key, the others as
    /// shown.
    Asked {
        binding: &'a str,
        key: &'a BTreeMap<String, serde_json::Value>,
    },
}

/// What came of a keyed read (ADR-0152).
#[derive(Debug, PartialEq, Eq)]
enum KeyOutcome {
    /// It was the latest, and its patch set is in the session's frames.
    Applied,
    /// A newer read was asked, its page was replaced, or it was let go:
    /// nothing it read is applied.
    Superseded,
}

/// Whether a read was stopped: a flight's cancellation, as a source sees it.
type Stopped = Arc<dyn Fn() -> bool + Send + Sync>;

/// How long the menu's slow category takes, and which it is (E14, T07): a
/// test sets them through `/bench/category`.
#[derive(Clone, Debug, Default)]
struct Categories {
    slow: Option<String>,
    delay_ms: u64,
    /// **Held until a test releases it**, or until the read is stopped, as
    /// the recommender is (ADR-0223): a test that needs the slow read in
    /// flight while it does something else holds it, rather than outwaiting
    /// a delay it guesses, which a slow machine outlasted.
    gate: Option<Arc<Gate>>,
}

/// How many times a document is read, at most (ADR-0151): each attempt but
/// the last outside the subscriber table, and one is repeated only when a
/// change reached its session while it was read.
const DOCUMENT_ATTEMPTS: u32 = 3;

/// How long a subscriber may go without asking before it is forgotten. The
/// long poll holds for one second and the stream for two, so a live page asks
/// far more often than this.
const IDLE: std::time::Duration = std::time::Duration::from_secs(120);

/// **A command whose page was shown to another reader** (track
/// `store-accounts`, Q3): the reader signed in or out in another tab, so the
/// session the browser now sends is not the one the page was served to.
/// What the 403 names, and what the reader is told.
const ANOTHER_READER: (&str, &str) = (
    "another-reader",
    "You signed in or out in another tab. Reload this page to go on.",
);

/// **A command whose page this host no longer holds** (track
/// `store-accounts`, Q3): forgotten while idle (`IDLE`), or served by a host
/// before this one. ADR-0302's words for a page out of date.
const PAGE_NOT_HELD: (&str, &str) = (
    "out-of-date",
    "This page is out of date. Reload it to go on.",
);

/// TRACK SEAM (second-host): **a command from a page another host served**
/// (ADR-0369, the integrator's ruling 1 of 2026-10-20): its number is in
/// another host's instance. A press that committed there is answered from
/// its record first ([`Server::kept_answer`]); any other is refused, as the
/// page's, never as one out of date: a deployment routes a page's requests
/// to the host that holds it (session affinity), and a reload asks a host
/// again.
const ANOTHER_HOST: (&str, &str) = (
    "another-host",
    "This page is served by another host. Reload it to go on.",
);

/// `{"cursor":..,"frames":[reload]}`: the batch a forgotten subscriber gets.
fn reload_batch(cursor: u64) -> String {
    let frame = StreamFrame::Recovery {
        protocol: CURRENT,
        recovery: pw_protocol::Recovery::Reload,
    };
    format!(
        "{{\"cursor\":{cursor},\"frames\":[{}]}}",
        serde_json::to_string(&frame).expect("frame")
    )
}

/// **A served document** (ADR-0161): its session, and its number, which is
/// also the cursor it was served at. Each is its own subscriber: what it
/// shows, its frames and its keyed reads are its own, so serving a session's
/// second tab leaves what its first was waiting for. Until 2026-10-03 a
/// session had one subscriber, and serving a document cleared it.
type Doc = (String, u64);

/// **What a commit's drops reached that another session may hold**
/// (ADR-0219): a query, by its path, and, where the entries dropped were a
/// user's, whose (track `store-accounts`, telling by principal, ADR-0318).
/// A private entry keyed by the reader's handle is that user's sessions'
/// alone to be told of; any other a commit reaches is every reader's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Reached {
    resource: String,
    user: Option<String>,
}

/// What a test does between a keyed read and its apply (ADR-0224).
#[cfg(test)]
type KeyedFetched = Box<dyn FnOnce(&Server) + Send>;

/// What a test does while a document's change is derived (ADR-0296).
#[cfg(test)]
type Deriving = Arc<dyn Fn(&Server, &Doc) + Send + Sync>;

/// What a test does after each binding a render reads (NEXT 4c).
#[cfg(test)]
type WhileReading = Arc<dyn Fn(&Server) + Send + Sync>;

/// **The documents a change is told to, and the interactions their values
/// include** ([`Server::to_tell`]; NEXT 4c).
struct Told {
    applied: Vec<String>,
    /// Those whose page asks: read, and told.
    live: Vec<Doc>,
    /// Those no page has asked for in [`LIVE`]: derived when one asks.
    passed: Vec<Doc>,
}

/// **A page's parameters**, as its address gives them (ADR-0162): `id`,
/// from `/stores/{id}`.
type Params = BTreeMap<String, String>;

/// **A speculated entry, by what names it** (ADR-0222, ADR-0236): the
/// session, the page, the binding, and the page's parameters its key reads.
type SpeculatedEntry = (String, String, String, Vec<String>);

/// Each of `session`'s documents in `map`, in the order they were served.
fn documents_of<V>(map: &BTreeMap<Doc, V>, session: &str) -> Vec<Doc> {
    map.range((session.to_string(), 0)..=(session.to_string(), u64::MAX))
        .map(|(doc, _)| doc.clone())
        .collect()
}

/// Forget every document that has not asked for [`IDLE`]. Says which, and
/// each session left with none, whose own state goes with it.
fn forget_idle(
    queue: &mut BTreeMap<Doc, Subscriber>,
    now: std::time::Instant,
) -> (Vec<Doc>, Vec<String>) {
    let idle: Vec<Doc> = queue
        .iter()
        .filter(|(_, w)| now.saturating_duration_since(w.seen) >= IDLE)
        .map(|(doc, _)| doc.clone())
        .collect();
    for doc in &idle {
        queue.remove(doc);
    }
    let mut gone: Vec<String> = idle
        .iter()
        .map(|(session, _)| session.clone())
        .filter(|session| documents_of(queue, session).is_empty())
        .collect();
    gone.dedup();
    (idle, gone)
}

impl Subscriber {
    fn push(&mut self, frame: StreamFrame) {
        self.pushed += 1;
        if self.behind {
            self.note(format_args!("dropped {}, behind", frame_kind(&frame)));
            return;
        }
        self.last_seq += 1;
        let seq = self.last_seq;
        self.note(format_args!("pushed {} as {seq}", frame_kind(&frame)));
        if self.frames.len() >= MAX_WAITING {
            self.frames.clear();
            self.frames.push((
                self.last_seq,
                StreamFrame::Recovery {
                    protocol: CURRENT,
                    recovery: pw_protocol::Recovery::Reload,
                },
            ));
            self.behind = true;
            return;
        }
        self.frames.push((self.last_seq, frame));
    }

    /// Everything after `since`, and the cursor a client that applies it all
    /// should report next time.
    fn after(&self, since: u64) -> (u64, Vec<&StreamFrame>) {
        let frames: Vec<&StreamFrame> = self
            .frames
            .iter()
            .filter(|(s, _)| *s > since)
            .map(|(_, f)| f)
            .collect();
        let cursor = self
            .frames
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| *s > since)
            .max()
            .unwrap_or(since);
        (cursor, frames)
    }

    /// Forget what the client has acknowledged.
    fn acknowledge(&mut self, through: u64) {
        self.frames.retain(|(s, _)| *s > through);
    }
}

/// The build this server serves. One value, used as the compatibility
/// generation everywhere it is needed, so nothing derives a second one.
/// **Entries' generation** in this host's materializer and resources. Not
/// the build's name (ADR-0300), which a host reads from its build: entries
/// named by it wait for the store's entries keyed by a user (W8's track),
/// which change the same functions.
const BUILD: &str = "B1";

/// The build id of the committed artifacts a test serves, which no `pw
/// build` wrote.
#[cfg(test)]
const COMMITTED_ARTIFACTS: &str = "b-committed-artifacts";

/// The deployment's PRF key, from a provider rather than a literal.
const IDENTITY: DevelopmentIdentityKey = DevelopmentIdentityKey;

/// **What a session's document shows** (ADR-0145): each part's text, by its
/// number, and each list its queries fill, item by item. A change is sent as
/// the difference from it, so a part that did not change is not patched and
/// an item whose key stayed keeps its nodes.
#[derive(Debug, Clone, Default)]
struct Shown {
    texts: BTreeMap<u32, String>,
    lists: BTreeMap<String, Vec<Value>>,
    /// Each block a query decides, as rendered (ADR-0146).
    blocks: BTreeMap<u32, String>,
    /// Each attribute at the top of the page that reads a query's value,
    /// by its part: its name, and its value as written, or none for a
    /// boolean one that is absent (ADR-0171).
    attributes: BTreeMap<u32, (String, Option<String>)>,
    /// What each handler at the top of the page captures of a query's value,
    /// by its element's first handler part, as the document writes it
    /// (ADR-0217).
    captures: BTreeMap<u32, Option<String>>,
    /// The page's title, by its part, as its text (ADR-0183).
    title: Option<(u32, String)>,
    /// **The values its blocks were rendered from** (ADR-0320), on a page
    /// with a block it tells part by part: what a block showed is read from
    /// them where the block changed, beside what it shows now. Only there:
    /// none else is read again.
    bindings: Option<Arc<BTreeMap<String, Val>>>,
    /// **Each value the page speculates on, but the store's cart**
    /// (ADR-0222), by its binding, as the page's module decodes it: what the
    /// browser holds, sent when it changes, from the read the page's patches
    /// are derived from.
    speculated: BTreeMap<String, serde_json::Value>,
}

/// **The recommender the store's data layer reaches** (ADR-0148): what
/// `store:data/recommendations#for-store` answers, after how long, and how it
/// fails. A test sets it through `/bench/recommendations`.
#[derive(Debug, Clone)]
struct Recommender {
    /// How long an answer takes. Charter §15.5: 1200 ms.
    delay_ms: u64,
    /// `declared`: the query's declared error, `NoneAvailable`. `host`: the
    /// call itself fails, as a source that is down does.
    fail: Option<String>,
    // What it recommends is the store's data (track `store-pg`): curated
    // through `store:host/recommendations#curate`.
    /// **Held until a test releases it** (ADR-0223), then answered after
    /// `delay_ms`. A test that needs its page seen before the answer holds
    /// the recommender rather than outwaiting a delay, which a loaded machine
    /// stretched past the query's own timeout.
    gate: Option<Arc<Gate>>,
}

impl Default for Recommender {
    fn default() -> Recommender {
        Recommender {
            delay_ms: 1200,
            fail: None,
            gate: None,
        }
    }
}

/// **A gate a held source waits at** (ADR-0223): closed until opened, and
/// waited at for ten seconds at most, so a test that never opens it holds no
/// thread for good.
///
/// A test that holds a source at a gate, rather than outwaiting a delay it
/// guesses, waits for the gate's own report that a read is held there
/// (`held`), and lets the source go when the event it waits for has come.
/// "A source that has not answered" is then a fact the test sets, where a
/// delay was a silence a slow machine made too.
#[derive(Debug, Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: std::sync::Condvar,
}

#[derive(Debug, Default)]
struct GateState {
    open: bool,
    /// How many reads wait at the gate now: what a test asks (`held`).
    #[cfg_attr(not(test), allow(dead_code))]
    waiting: usize,
    /// How many times a read waiting at the gate has asked whether it was
    /// stopped: what a test asks (`polled`), to know a read it let go of has
    /// been asked, and was not stopped.
    #[cfg_attr(not(test), allow(dead_code))]
    polls: u64,
}

impl Gate {
    fn open(&self) {
        self.state.lock().expect("gate").open = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        self.wait_for(None);
    }

    /// **Waits at the gate until it is opened, or until `stopped` says the
    /// read is wanted no more** (a read's cancellation is a predicate, not a
    /// signal, so it is asked every ten milliseconds), ten seconds at most.
    /// Whether it was stopped.
    fn wait_unless(&self, stopped: &dyn Fn() -> bool) -> bool {
        self.wait_for(Some(stopped))
    }

    fn wait_for(&self, stopped: Option<&dyn Fn() -> bool>) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut state = self.state.lock().expect("gate");
        state.waiting += 1;
        self.changed.notify_all();
        // A read that was stopped is stopped, whether or not the gate has
        // opened since: it is not read on.
        let was_stopped = loop {
            if let Some(stopped) = stopped {
                let is_stopped = stopped();
                state.polls += 1;
                self.changed.notify_all();
                if is_stopped {
                    break true;
                }
            }
            if state.open {
                break false;
            }
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break false;
            }
            let slice = match stopped {
                Some(_) => left.min(std::time::Duration::from_millis(10)),
                None => left,
            };
            state = self.changed.wait_timeout(state, slice).expect("gate").0;
        };
        state.waiting -= 1;
        self.changed.notify_all();
        was_stopped
    }

    /// **A test's: how many times a read waiting at the gate has asked
    /// whether it was stopped.**
    #[cfg(test)]
    fn polls(&self) -> u64 {
        self.state.lock().expect("gate").polls
    }

    /// **A test's: returns once the reads waiting at the gate have asked,
    /// `polls` times in all, whether they were stopped**, ten seconds at most
    /// (then it fails, saying so): a read let go of, asked twice since, and
    /// still waiting, was not stopped.
    #[cfg(test)]
    fn polled(&self, polls: u64) {
        let state = self.state.lock().expect("gate");
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, std::time::Duration::from_secs(10), |state| {
                state.polls < polls
            })
            .expect("gate");
        drop(state);
        assert!(!timeout.timed_out(), "the gate was not asked {polls} times");
    }

    /// **A test's: how many reads wait at the gate now.** For a test that has
    /// seen something end and asks whether a read it holds was still held
    /// when it did (`held` waits for one to be; this does not wait).
    #[cfg(test)]
    fn waiting_now(&self) -> usize {
        self.state.lock().expect("gate").waiting
    }

    /// **A test's: returns once `reads` reads wait at the gate**, ten seconds
    /// at most (then it fails, saying so).
    #[cfg(test)]
    fn held(&self, reads: usize) {
        let state = self.state.lock().expect("gate");
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, std::time::Duration::from_secs(10), |state| {
                state.waiting < reads
            })
            .expect("gate");
        drop(state);
        assert!(
            !timeout.timed_out(),
            "{reads} read(s) never reached the gate"
        );
    }
}

/// **What a store's recommender suggests from its menu** (ADR-0165): the
/// items after the first, two of them.
fn recommended_from(menu: &[(String, String)]) -> Vec<(String, String)> {
    menu.iter().skip(1).take(2).cloned().collect()
}

/// **What one session's cart meets at the database** (charter §15.5,
/// ADR-0174): how long a read of it takes, and whether its next write and its
/// next read fail, once each, as a database that is down fails.
#[derive(Debug, Clone, Default)]
struct CartFaults {
    delay_ms: u64,
    fail_write: bool,
    fail_read: bool,
}

/// **What one session's connections meet** (charter §15.5, ADR-0175).
#[derive(Debug, Clone, Default)]
struct ConnectionFaults {
    /// The next command's connection, closed with no answer: before the
    /// command runs, or after it has committed. Once.
    drop_command: Option<DropAt>,
    /// The next command's connection, held open with no answer, or with an
    /// answer begun and never finished, until its page gives up on it
    /// (ADR-0381). Once.
    hold_command: Option<HoldAt>,
    /// When the session's subscriptions were last cut off: each one open
    /// then ends.
    cut_from: Option<std::time::Instant>,
    /// Until when each new subscription is closed at once.
    cut_until: Option<std::time::Instant>,
}

/// Where a command's connection is dropped (ADR-0175).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropAt {
    /// The request read, and nothing run.
    Before,
    /// The command run, and committed if it did, and its answer not sent.
    After,
}

/// Where a command's connection is held open, unanswered (ADR-0381): a
/// network that neither answers nor fails, as a page meets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldAt {
    /// The request read, nothing run, and no answer begun.
    Before,
    /// The command run, and committed if it did, and no answer begun.
    After,
    /// The command run, and its answer's head and part of its body sent.
    Body,
}

/// **How long a held connection is held at most** (ADR-0381): a test's
/// fault, never a thread kept for a page that will not close it.
const HOLD_AT_MOST: std::time::Duration = std::time::Duration::from_secs(30);

/// **The wall clock, as an `Instant` holds it** (ADR-0180): the
/// milliseconds since 1970-01-01T00:00Z.
fn wall_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// **The estimator the store's data layer reaches, for one session** (E14,
/// T10): what `store:data/estimates#current` answers, after how long, and how
/// it fails. A test sets it through `/bench/estimate`.
#[derive(Debug, Clone)]
struct Estimator {
    /// How long an estimate takes. Charter §15.5: 400 ms.
    delay_ms: u64,
    /// `declared`: the query's declared error. Anything else: the call itself
    /// fails, as an estimator that is down does.
    fail: Option<String>,
    /// **Held until a test releases it**, as the recommender is (ADR-0223),
    /// then answered after `delay_ms`.
    gate: Option<Arc<Gate>>,
    // What it estimates, the least minutes a delivery takes and the most
    // (ADR-0180), is the store's data (track `store-pg`): set through
    // `store:host/estimates#set`.
}

impl Default for Estimator {
    fn default() -> Estimator {
        Estimator {
            delay_ms: 400,
            fail: None,
            gate: None,
        }
    }
}

// TRACK SEAM (uploads, ADR-0253): a deployment's blob storage.
mod blob;
mod data;
mod feed;
mod feed_pg;
// The parallel tracks' modules (ADR-0253, docs/PARALLEL.md): each reached
// from this file only at the seams marked `TRACK SEAM`.
mod identity;
// TRACK SEAM (identity): the development identity provider.
mod accounts;
mod store;
// TRACK SEAM (store-accounts): where a delivery goes, and whether a store
// reaches it.
mod places;
// Track `store-pg`: the store's data in PostgreSQL.
mod store_pg;
mod uploads;
// TRACK SEAM (notifications): the typed principal, and notifications.
mod notifications;
// ADR-0277: a materialization kept, and served.
mod materializations;
// TRACK SEAM (messages): direct messages.
mod messages;
// TRACK SEAM (kiokun): kiokun's dictionary, read (docs/PARALLEL.md, W6).
mod kiokun;
// TRACK SEAM (device-store): a document of a page that holds values the
// device keeps (ADR-0366).
mod device;
// ADR-0304: the platform's clock, and what a value read by it holds until.
mod clock;
// ADR-0305: a page's commands run in the order it sent them.
mod order;
// TRACK SEAM (store-search): the store's search, in its data layer (W9).
mod search;
// TRACK SEAM (store-hours): a store's opening hours (W9).
mod hours;
// TRACK SEAM (order-total): an order's total, the store's data for it.
mod checkout;
// NEXT 6c: a deployment's payments, a provider's webhooks held to their
// signature.
#[allow(dead_code)]
mod payments;
// ADR-0195's ruling 13: what a page this host serves may load and run.
mod csp;
// The scheduled-commands track: a command runs when the clock passes its
// time.
mod schedules;
// TRACK SEAM (reviews): a store's ratings and reviews, given through a
// delivered order.
mod reviews;
// TRACK SEAM (scheduled-orders): an order placed for a time its reader
// chooses, sent to its stores when it is due (W9).
mod scheduled_orders;
// TRACK SEAM (render-merge): one render's identical calls made once, in one
// snapshot (NEXT 4c).
mod render_merge;
// Track `second-host` (ADR-0369): hosts on one database hear each other's
// commits.
mod hosts;

struct Server {
    /// **The build this host serves, by its name** (ADR-0300): on each
    /// document's resume manifest, the handler table and the documents it
    /// answers (`pw-build`).
    build_id: String,
    /// The templates the compiler emitted, deserialized once.
    templates: Vec<Template>,
    /// **What the build's markup asks of its pages' policy** (`csp`), read
    /// from its templates once.
    needs: csp::Needs,
    /// **The program's data layer** (ADR-0218): what its contracts import,
    /// which the deployment supplies. A query reads through it and a command
    /// stages its writes in it.
    data: Arc<dyn data::DataLayer>,
    /// **The store's data layer** (ADR-0218, track `store-pg`): `data` when
    /// the program is the store, and an empty one otherwise. What changes
    /// its data is staged through `data`, as a command is; what reaches it
    /// directly is its test controls, its [`store::Faults`].
    store: Arc<dyn store::StoreLayer>,
    /// **Whom the program's cart and order are** (track `store-accounts`):
    /// `Some(true)` where it reads them by the reader's handle
    /// (`Cart(current_user())`), `Some(false)` by the session (the benchmark's
    /// copy of the store), and `None` where it reads no cart.
    cart_by_reader: Option<bool>,
    /// The materializer's clock, advanced once per regeneration.
    ///
    /// A version is `Entry.generated_at`, which is a clock reading — so a clock
    /// that never moves gives every entry version 0 and the browser's staleness
    /// comparison compares nothing. Advancing here rather than inventing a
    /// counter keeps the version the MATERIALIZER's, which is the gate.
    clock: Clock,
    /// The one materializer. Versions come from here and from nowhere else.
    materializer: Materializer,
    /// **Each kept materialization's entry, by its path and its key's text,
    /// and the arguments it was derived with** (ADR-0277): what an event
    /// may reach, and is made again with.
    kept: Mutex<materializations::Kept>,
    /// **Each entry an event made again, not yet told** (ADR-0277): by its
    /// path and its arguments, so each document reading it, and no other,
    /// is told.
    made: Mutex<Vec<(String, Vec<Val>)>>,
    /// **Each session's committed interactions**, most recent last
    /// (ADR-0172): what a value of its cart includes, which a page drops its
    /// speculations by. As many as are kept for idempotency.
    applied: Mutex<BTreeMap<String, std::collections::VecDeque<String>>>,
    /// **One change of a session at a time** (ADR-0172): a command's commit
    /// and the frames it fans out, so another command of the session cannot
    /// land between the version a change is sent at, its value and the
    /// interactions it names. Until 2026-10-03 two drains of one session
    /// interleaved, and two values were sent at one version: a page took the
    /// first, ignored the second, and lost a line until the next change.
    /// Taken in turn since ADR-0271.
    sessions: Mutex<BTreeMap<String, Arc<Turns>>>,
    /// The graph the compiler emitted: what each command emits and what each
    /// entry listens for. Read once; a test replaces it.
    graph: pw_materialize::Graph,
    /// The compiled components, by component id, read from
    /// `docs/evidence/E10/` as the compiler wrote them. Data, like the
    /// contracts: this server links no compiler. Compiled once, with the
    /// artifact's imports read once — both are properties of the bytes.
    components: BTreeMap<String, Loaded>,
    /// The document's static assets.
    dist: std::path::PathBuf,
    /// **Where the compiler's browser artifacts are** (ADR-0123): the
    /// handler modules and speculation modules. `pw build`'s output directory
    /// for a running server; the document directory for the unit tests,
    /// which `run.sh` fills the same way.
    artifacts: std::path::PathBuf,
    /// **Each page's speculation manifest** (ADR-0122, ADR-0191), by its
    /// page's path, as `pw build` wrote it to `speculations/`: which bindings
    /// the page speculates on, and so which entries' values it is sent.
    /// Until 2026-10-04 the store's alone was read, and no other page
    /// speculated, though the build wrote each its module.
    speculations: BTreeMap<String, serde_json::Value>,
    /// **What the store page shows, as reads of what its queries return**
    /// (ADR-0125): `pw build`'s `pages/store.page.StorePage.json`. Each
    /// binding names the query component it runs; each part, the steps from
    /// that binding's value. Until 2026-10-02 this server computed the page's
    /// values itself and ran none of the store's queries.
    plan: serde_json::Value,
    /// Every page's plan, by its template path (ADR-0130): what a page
    /// whose values are its signals alone renders from.
    plans: BTreeMap<String, serde_json::Value>,
    /// **The store's queries, run by their declared policies** (ADR-0127):
    /// freshness, cache partition, key, concurrency, timeout and retries are
    /// `pw-resource`'s to decide, on its own clock, which this server keeps at
    /// wall time. It caches each value itself, and hands it to the fetch.
    ///
    /// Until 2026-10-02 it cached a token, and the value was in a table here
    /// bounded to four per key. Eight threads committing on one session could
    /// invalidate more than four flights between a reader being handed the
    /// cached token and reading its value, and the reader found it gone
    /// (`concurrent_commands_on_one_session_all_commit`, 1 run in 3).
    queries: pw_resource::Resources<Arc<Val>>,
    query_clock: (
        pw_resource::Clock,
        std::time::Instant,
        std::sync::atomic::AtomicU64,
    ),
    /// A number for each `concurrency parallel` flight's own key.
    parallel: std::sync::atomic::AtomicU64,
    /// How many times each data-layer operation ran: what `/metrics` reports,
    /// and what shows a policy saved a request.
    calls: Arc<Mutex<BTreeMap<String, u64>>>,
    /// **Frames waiting for each document** (ADR-0161).
    pending: Table,
    /// **Each hold of `pending` past 50 ms, the latest 200**, at the places
    /// that render or derive while they hold it: what a failing browser test
    /// attaches beside a document's trail (`/bench/records`).
    slow: Mutex<std::collections::VecDeque<String>>,
    /// **What each telling did, the latest 300 notes** (ADR-0296): what a
    /// commit queued for other sessions, whom each drain chose, and how each
    /// telling of a session went. Sessions by a hash of their id, which is a
    /// cookie (`masked`). "a follow reaches another reader" failed on CI
    /// with its second reader never told, and nothing said why.
    tellings: Mutex<std::collections::VecDeque<String>>,
    /// **What each document shows** (ADR-0145, ADR-0161), as it was served
    /// and then patched: what a change is derived against. Locked after
    /// `pending` and `keyed`, never before. Each value is replaced whole, so
    /// a change derived outside the table against one is pushed only while
    /// it is still the one shown (`Arc::ptr_eq`, ADR-0296).
    shown: Mutex<BTreeMap<Doc, Arc<Shown>>>,
    /// **The next document's number** (ADR-0161), server-wide and from
    /// [`Server::first_page`], so a document's cursor is never zero.
    documents: std::sync::atomic::AtomicU64,
    /// TRACK SEAM (second-host): **this host's first page number**
    /// (ADR-0369, the integrator's ruling 1 of 2026-10-20): its instance on
    /// the database it shares in the high bits, so no two hosts number a
    /// page alike, and a page another host served is told apart from one
    /// this host no longer holds. One, its data in its memory.
    first_page: u64,
    /// **Each document's parameters** (ADR-0162), as its address gave them:
    /// which store it shows. Registered with its subscriber, before it is
    /// read, and forgotten with it.
    params: Mutex<BTreeMap<Doc, Params>>,
    /// **Each document's page** (ADR-0190), by its template path: which
    /// plan its values are read by and which template renders it.
    /// Registered and forgotten with its parameters; a document not in it
    /// is the store's.
    pages: Mutex<BTreeMap<Doc, String>>,
    /// **What each document's page asked of its reader, as answered when it
    /// was first read** (ADR-0307): the document's for its life, as the data
    /// it shows was read for its reader then. Forgotten with it.
    answers: Mutex<BTreeMap<Doc, BTreeMap<String, bool>>>,
    /// **What commits dropped that other sessions may hold** (ADR-0219),
    /// with the session that committed: told once the request that
    /// committed is answered (`tell_waiting`).
    telling: Mutex<Vec<(String, Vec<Reached>)>>,
    /// **The sessions being told, and whether a commit reached one again
    /// meanwhile** (ADR-0271): one telling of a session at a time, and one
    /// more after it for every commit that came while it ran.
    told: Mutex<BTreeMap<String, bool>>,
    /// **The words a program declares for its predicates** (ADR-0302,
    /// `predicates.json`): what a reader is told when one refuses, in place
    /// of the deployment's.
    words: BTreeMap<String, String>,
    /// **The version each speculated value was last sent at** (ADR-0222), by
    /// session, page, binding and the page's parameters its key reads
    /// (ADR-0236): what a commit's answer names, so the page keeps its
    /// speculation until the value that includes the commit arrives.
    speculated_versions: Mutex<BTreeMap<SpeculatedEntry, Version>>,
    /// **How long a streamed region's fill pauses half written**
    /// (ADR-0223), in milliseconds: none unless a test sets it through
    /// `/bench/split-fills`, as a network that delivers a response in parts
    /// would.
    split_fills: std::sync::atomic::AtomicU64,
    /// **What a test does between a keyed read and its apply** (ADR-0224),
    /// once: a commit there is the race the apply is held against.
    #[cfg(test)]
    keyed_fetched: Mutex<Option<KeyedFetched>>,
    /// **What a test does before a command commits** (ADR-0325), once: a
    /// commit of the same interaction there, by another host, is the race its
    /// record is held against.
    #[cfg(test)]
    before_commit: Mutex<Option<KeyedFetched>>,
    /// **What a test does while a change is derived outside the table**
    /// (ADR-0296), for each document derived: the table is free there, and
    /// a change that reaches the document there is derived against again.
    #[cfg(test)]
    deriving_hook: Mutex<Option<Deriving>>,
    /// **What a test does after a telling's render, before it lets go**
    /// (ADR-0271), once: a commit there is the one the telling must tell
    /// after it, and a panic there must not silence the session.
    #[cfg(test)]
    told_rendered: Mutex<Option<KeyedFetched>>,
    /// **What a test does after a render reads a binding** (NEXT 4c, track
    /// `render-merge`), once: a commit there lands between two bindings'
    /// reads, which the render's one snapshot is held against.
    #[cfg(test)]
    between_bindings: Mutex<Option<KeyedFetched>>,
    /// **What a test does after each binding a render reads**, for as long
    /// as it is set (NEXT 4c): renders held at their reads, each with its
    /// seat, while the test arranges what waits for one.
    #[cfg(test)]
    while_reading: Mutex<Option<WhileReading>>,
    /// **What a test does inside a query's read, once** (NEXT 4c): a read
    /// held in flight, its key's, while another render reads the key.
    #[cfg(test)]
    in_flight: Mutex<Option<KeyedFetched>>,
    /// **What each session's connections meet** (charter §15.5's one-shot
    /// network error and forced reconnect, ADR-0175): whether its next
    /// command's connection is dropped, and when its subscriptions are cut
    /// off. A test sets them through `/bench/drop` and `/bench/reconnect`.
    connection_faults: Mutex<BTreeMap<String, ConnectionFaults>>,
    /// **The sessions whose next regeneration fails** (charter §15.5's
    /// materializer failure, ADR-0176), once each. A test sets one through
    /// `/bench/materializer`.
    materializer_faults: Mutex<std::collections::BTreeSet<String>>,
    /// **Each document's keyed reads** (ADR-0152, ADR-0161).
    keyed: Mutex<BTreeMap<Doc, Keyed>>,
    /// **The items each store's menu fragment was last rendered from**: a
    /// fragment is rendered again when the `Menu` query's value is not what
    /// it shows.
    menu_rendered_from: Mutex<BTreeMap<String, Value>>,
    /// **The compiler's contracts, and the node this server is.**
    ///
    /// E8's last gate item asks for the command path to go through the host
    /// rather than a Rust closure. This is the half that is real today: the
    /// authority to run a command is DECIDED — `admit` against a topology,
    /// producing a `Granted` or a refusal — instead of being ambient because
    /// the function happens to be callable.
    ///
    /// The BODY is no longer a Rust closure (E10-I, 2026-09-24): each command
    /// runs as the component the compiler built from its `.pw` declaration,
    /// admitted against the imports the artifact actually has.
    contracts: Vec<ComponentContract>,
    topology: Topology,
    /// **Each idempotent command's outcome, by interaction** (ADR-0121).
    ///
    /// `pw-resource`'s reservation: a key is reserved before its command
    /// runs, a duplicate waits for and shares the first outcome, and an
    /// unknown outcome is never re-executed. The key binds the interaction to
    /// its session and its command; [`Server::interactions`] binds it to the
    /// arguments and bounds how many are kept.
    commands: pw_resource::Resources,
    /// Per session, the interaction keys held in `commands`, oldest first,
    /// each with the arguments it was first sent with.
    interactions: Mutex<BTreeMap<String, std::collections::VecDeque<(String, String)>>>,
    /// **The host's wall clock at start** (ADR-0304), in milliseconds since
    /// the epoch: [`Server::now`] is it and the query clock since.
    epoch: i64,
    /// **The host's tz database** (ADR-0304), for the platform's clock.
    zones: Arc<pw_time::Database>,
    /// **What each kept value holds until** (ADR-0304): the earliest instant
    /// its read compared the clock with, by its `pw-resource` key.
    held: Mutex<std::collections::HashMap<pw_resource::Key, i64>>,
    /// **Each live document, to read again when what it shows holds no
    /// longer** (ADR-0304).
    timers: clock::Timers,
    /// **Each page's commands, in the order it sent them** (ADR-0305): a
    /// command runs once the one its page sent before it has.
    order: order::Order,
    /// **Who a request is, and what `requires` is told**: the identity
    /// track's (ADR-0253).
    identity: identity::Identity,
    /// **A request whose body is a file**: the uploads track's (ADR-0253).
    uploads: uploads::Uploads,
    /// **How the scheduled runs ended** since the host started (the
    /// scheduled-commands track), which `/metrics` reports.
    counted_runs: CountedRuns,
    scheduled_locks: Mutex<BTreeMap<i64, std::sync::Weak<Mutex<()>>>>,
    /// **The deployment's public origin** (ADR-0311), `PW_ORIGIN`, as the
    /// identity reads it: what a page's canonical address is written
    /// absolute with, never a request's `Host`, which its client chooses.
    /// Set once, as the server starts; a page that states its canonical
    /// address cannot be served without it.
    origin: std::sync::OnceLock<String>,
    /// NEXT 6c: **the deployment's payments**, as its configuration states
    /// them (`PW_PAYMENTS`), set once as the server starts; none where it
    /// names none.
    payments: std::sync::OnceLock<Arc<payments::Payments>>,
}

/// How many interactions' outcomes one session keeps (ADR-0121). A retry of
/// an interaction older than this many later ones runs again: the bound's
/// cost, stated rather than unbounded memory.
const INTERACTIONS_PER_SESSION: usize = 64;

/// **How long an interaction's committed record answers a retry** (ADR-0325):
/// a day, as Stripe keeps an idempotency key at least 24 hours. A retry
/// later than that is a new request.
const INTERACTIONS_KEPT_MS: u64 = 24 * 60 * 60 * 1000;

/// The semantic identity of one session's cart.
///
/// Built from the shape the compiler's bridge produces. The server does not
/// invent it: `EntryIdentity` is the one answer to "which entry", and a second
/// construction here would be the divergence the split exists to prevent.
/// **A session's cart's entry** (ADR-0172): in the session's partition, and
/// keyed by the cart's owner as the program keys it (track `store-accounts`):
/// the reader's user where it reads a cart by the reader's handle, so that
/// the program's event naming the user reaches it, or the session.
fn cart_identity(owner: &str, session: &str) -> EntryIdentity {
    EntryIdentity::new(
        "store.page.Cart",
        &[owner],
        Partition::Session {
            id: session.to_string(),
        },
    )
    .generation(BUILD)
}

/// A store's menu's entry identity — PUBLIC, and carrying the same
/// generation a session-scoped entry does. Partition and compatibility are
/// orthogonal. One per store (ADR-0162): a change to one store's menu is not
/// another's.
fn menu_identity(store: &str) -> EntryIdentity {
    EntryIdentity::new("store.page.Menu", &[store], pw_resource::Partition::Public)
        .generation(BUILD)
}

/// The parameters of the store page for the store `id` (ADR-0162).
fn store_params(id: &str) -> Params {
    BTreeMap::from([("id".to_string(), id.to_string())])
}

fn cart_entry(owner: &str, session: &str) -> ResourceEntryId {
    ResourceEntryId::derive(&cart_identity(owner, session), &IDENTITY)
}

/// **A value a session's page speculates on, as the browser holds it**
/// (ADR-0222): one entry for each page's binding, which its `entry_value`
/// frames carry and a commit's answer names.
fn speculated_entry(session: &str, page: &str, binding: &str, route: &[String]) -> ResourceEntryId {
    // **And the page's parameters its key reads** (ADR-0236): two threads
    // one session has open are two entries.
    let key: Vec<&str> = std::iter::once(session)
        .chain(route.iter().map(String::as_str))
        .collect();
    ResourceEntryId::derive(
        &EntryIdentity::new(
            &format!("pw.speculated.{page}#{binding}"),
            &key,
            Partition::Session {
                id: session.to_string(),
            },
        )
        .generation(BUILD),
        &IDENTITY,
    )
}

/// **A session's documents' entry** (ADR-0218): what a commit's change is
/// sent against for a program whose data layer keeps no entry of its own for
/// the session.
/// **A lock taken in the order it is asked for** (ADR-0271): a ticket each,
/// served in turn. `std`'s mutex promises no order, and a session told of
/// every commit another made took its lock again and again while the
/// session's own read, and its subscription, waited seconds for it.
#[derive(Default)]
struct Turns {
    /// The next ticket, and the one being served.
    tickets: Mutex<(u64, u64)>,
    served: std::sync::Condvar,
}

/// A turn being served; the next is served when it ends, a panic included.
struct Turn<'a>(&'a Turns);

/// **The subscriber table**: the frames waiting for each document
/// (ADR-0161), behind a mutex whose `lock` checks, in a debug build (every
/// test's, and the development server's: the integrator's answer of
/// 2026-10-17), that no thread takes it in a render (TRACK SEAM
/// (render-merge), NEXT 4c, the integrator's ruling 2: a render never waits
/// for the table while it holds its seat; a document's last attempt
/// (ADR-0151) holds the table while its render waits for one).
#[derive(Default)]
struct Table(Mutex<BTreeMap<Doc, Subscriber>>);

impl Table {
    fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, BTreeMap<Doc, Subscriber>>> {
        #[cfg(debug_assertions)]
        assert!(
            !render_merge::in_a_render(),
            "the subscriber table taken in a render (NEXT 4c)"
        );
        self.0.lock()
    }

    #[cfg(test)]
    fn try_lock(
        &self,
    ) -> std::sync::TryLockResult<std::sync::MutexGuard<'_, BTreeMap<Doc, Subscriber>>> {
        self.0.try_lock()
    }
}

impl Turns {
    /// Wait for this caller's turn. A `LockResult`, as a mutex's is, though
    /// a turn is never poisoned: one that panics ends, and serves the next.
    fn lock(&self) -> std::sync::LockResult<Turn<'_>> {
        // TRACK SEAM (render-merge): never waited for in a render, which
        // would hold its seat meanwhile (NEXT 4c, the integrator's ruling 2),
        // checked in a debug build, a test's or the development server's
        // (its answer of 2026-10-17).
        #[cfg(debug_assertions)]
        assert!(
            !render_merge::in_a_render(),
            "a session's turn taken in a render (NEXT 4c)"
        );
        let mut tickets = self.tickets.lock().expect("tickets");
        let mine = tickets.0;
        tickets.0 += 1;
        while tickets.1 != mine {
            tickets = self.served.wait(tickets).expect("tickets");
        }
        Ok(Turn(self))
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.tickets.lock().expect("tickets").1 += 1;
        self.0.served.notify_all();
    }
}

fn session_documents(session: &str) -> ResourceEntryId {
    ResourceEntryId::derive(
        &EntryIdentity::new(
            "pw.session.documents",
            &[session],
            Partition::Session {
                id: session.to_string(),
            },
        )
        .generation(BUILD),
        &IDENTITY,
    )
}

/// **A session's order's entry** (ADR-0193): what a change the store makes
/// to it is sent against, as the cart's is.
fn order_entry(owner: &str, session: &str) -> ResourceEntryId {
    ResourceEntryId::derive(
        &EntryIdentity::new(
            "store.page.Order",
            &[owner],
            Partition::Session {
                id: session.to_string(),
            },
        )
        .generation(BUILD),
        &IDENTITY,
    )
}

/// **The compiler's contracts, as data.**
///
/// Read from `docs/evidence/E8/component-contracts.json`, which `just
/// e8-contracts` regenerates from the store demo. ADR-0018: neither side links
/// the other, so this is the artifact and not a call into the compiler.
///
/// Panics rather than defaulting to an empty list. An empty list means every
/// command has no contract and is refused, which looks like a security posture
/// and is actually a missing file.
/// **The compiled components**, as the compiler wrote them.
///
/// `just e10-component` writes `docs/evidence/E10/<component id>.wasm`, named
/// by the contract's own id so nothing here derives a name. Panics on a
/// missing artifact rather than running without it: a command with no
/// compiled body has no body at all now.
/// **A cart's line, as the data layer records it** (ADR-0172): its item, how
/// many, and the item's name and price when the line was made, which the
/// line shows from then on, as charter §15.1's `unit_price` says.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    item: String,
    quantity: i64,
    name: String,
    price: i64,
}

/// A cart's lines, as the data layer holds them.
type Lines = Vec<Line>;

/// One compiled component: ready to run, and what its artifact imports.
struct Loaded {
    prepared: pw_host::engine::Prepared,
    imports: Vec<String>,
}

#[cfg(test)]
fn components() -> BTreeMap<String, Loaded> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/evidence/E10");
    let mut out = BTreeMap::new();
    for id in [
        "store.page.add_to_cart",
        "store.page.clear_cart",
        "store.page.Store",
        "store.page.Menu",
        "store.page.Cart",
        // Whether the store reaches the reader's address (track
        // `store-accounts`).
        "store.page.Reach",
        // A user's cart's count (track `store-accounts`).
        "UserCarts.line_count",
        // What a menu's row reads its price through (ADR-0169).
        "domain.display",
        // A line's controls, and what the cart's lines read through
        // (ADR-0172).
        "store.page.increase_in_cart",
        "store.page.decrease_in_cart",
        "store.page.remove_from_cart",
        "domain.count",
        "domain.total",
        "UserCarts.subtotal",
        // The menu counted, and the line the store's page shows from it
        // (ADR-0277).
        "store.page.MenuSize",
        "store.page.MenuLine",
        // TRACK SEAM (store-hours): whether a store is open, every store's,
        // and what the cart says of its stores' hours.
        "store.hours.StoreStatus",
        "store.hours.StoresOpen",
        "store.hours.CartHours",
        // TRACK SEAM (reviews): what a store's ratings come to, and its
        // reviews, which its page reads.
        "store.reviews.StoreRatings",
    ] {
        let path = dir.join(format!("{id}.wasm"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}\nrun `just e10-component` to compile the store's commands",
                path.display()
            )
        });
        // Compiled once, its imports read from it (ADR-0317).
        let prepared = pw_host::engine::Prepared::compile(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let imports = prepared.imports();
        out.insert(id.to_string(), Loaded { prepared, imports });
    }
    out
}

/// **What one build gave the server** (ADR-0123, ADR-0125): where its
/// browser artifacts are, and what it runs the store by.
struct Built {
    artifacts: std::path::PathBuf,
    /// **The build's name** (ADR-0300), as `pw build` wrote it (`build-id`).
    build_id: String,
    templates: Vec<Template>,
    contracts: Vec<ComponentContract>,
    components: BTreeMap<String, Loaded>,
    graph: pw_materialize::Graph,
    plan: serde_json::Value,
    /// Every page's plan, by its template path (ADR-0130).
    plans: BTreeMap<String, serde_json::Value>,
}

/// Every component a build wrote, by its file name (ADR-0123).
fn components_in(dir: &std::path::Path) -> Result<BTreeMap<String, Loaded>, String> {
    let mut out = BTreeMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".wasm"))
        else {
            continue;
        };
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Compiled once, its imports read from it (ADR-0317).
        let prepared = pw_host::engine::Prepared::compile(&bytes)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let imports = prepared.imports();
        out.insert(id.to_string(), Loaded { prepared, imports });
    }
    Ok(out)
}

#[cfg(test)]
fn contracts() -> Vec<ComponentContract> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/evidence/E8/component-contracts.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}\nrun `just e8-contracts` to regenerate the contracts",
            path.display()
        )
    });
    ComponentContract::from_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// **The node this dev server is.**
///
/// An origin with the five capabilities the store demo's contracts require —
/// written out rather than derived from those contracts, which is the whole
/// point: a topology derived from what a program asks for grants everything
/// every program asks for, and admission becomes a formality.
///
/// `docs/evidence/E8/artifact-audit.txt` is the same shape against real
/// components.
#[cfg(test)]
fn dev_topology() -> Topology {
    topology_for(store::grants())
}

/// **The development origin, granting the platform's operations and a data
/// layer's** (ADR-0218).
fn topology_for(layer: &[&'static str]) -> Topology {
    Topology {
        nodes: vec![Node {
            name: "dev-origin".to_string(),
            world: "origin".to_string(),
            // The platform's grants, and its data layer's (ADR-0218).
            grants: [
                // 2026-08-10. The store gained the `import context.{
                // current_session }` it had been missing since E4, so the page
                // and both commands read the session. A dev origin that does
                // not publish it refuses all three — which is admission
                // working, and is what this list not being derived from the
                // contracts is for.
                "session.read",
                // A command's events, committed with its writes (ADR-0208).
                "outbox.write",
                // The platform's clock, compared (ADR-0304): this host
                // answers it, and tells what it held when time passes.
                "clock.compare",
                // And its tz database (ADR-0304), read from the host's own.
                "clock.zone",
                // NEXT 6c: a payment begun, at the origin alone (step seven):
                // the deployment's key is this host's, never the program's.
                // Where the deployment takes none, `begin` answers `NotTaken`.
                "payments.begin",
                // Step eight: a payment canceled, by its reader alone.
                "payments.cancel",
                // The scheduled-commands track: a run scheduled, or
                // cancelled, with a command's writes, which this host runs
                // when the clock passes its time.
                "clock.schedule",
            ]
            .iter()
            .chain(layer)
            .map(|s| s.to_string())
            .collect(),
        }],
    }
}

impl Server {
    /// **The server for what `pw build` built** (ADR-0123): its templates,
    /// contracts, components, graph, handlers and speculations, all from one
    /// build directory. Until 2026-10-02 the commands and contracts came from
    /// `docs/evidence/` and the graph was compiled into this binary, so a
    /// program changed and rebuilt kept running its old commands.
    #[cfg(test)]
    fn from_build(dist: std::path::PathBuf, build: std::path::PathBuf) -> Result<Server, String> {
        Server::from_build_with(dist, build, None)
    }

    /// [`Server::from_build`], the feed's data in the layer the deployment
    /// opened (ADR-0246): PostgreSQL where it names one, the in-memory layer
    /// otherwise. Either is held to what the program's sources state before
    /// anything is served.
    #[cfg(test)]
    fn from_build_with(
        dist: std::path::PathBuf,
        build: std::path::PathBuf,
        feed: Option<Arc<dyn data::DataLayer>>,
    ) -> Result<Server, String> {
        Server::from_build_layers(dist, build, feed, None)
    }

    /// [`Server::from_build_with`], and the store's data in the layer the
    /// deployment opened (track `store-pg`): PostgreSQL where it names one,
    /// the in-memory layer otherwise (and, for the server's tests, the layer
    /// [`store::layer_for_tests`] says).
    fn from_build_layers(
        dist: std::path::PathBuf,
        build: std::path::PathBuf,
        feed: Option<Arc<dyn data::DataLayer>>,
        store: Option<Arc<dyn store::StoreLayer>>,
    ) -> Result<Server, String> {
        let read = |rel: &str| {
            std::fs::read_to_string(build.join(rel))
                .map_err(|e| format!("{}: {e}", build.join(rel).display()))
        };
        let templates: Vec<Template> = serde_json::from_str(&read("templates.json")?)
            .map_err(|e| format!("templates.json: {e}"))?;
        // **The build's name** (ADR-0300): a build that names none was
        // written before builds were named, and is built again.
        let build_id = read("build-id")
            .map_err(|e| format!("{e}: the build names no build; run `pw build` again"))?
            .trim()
            .to_string();
        let contracts = ComponentContract::from_json(&read("contracts.json")?)
            .map_err(|e| format!("contracts.json: {e}"))?;
        let graph = pw_materialize::Graph::from_json(&read("graph.json")?)
            .map_err(|e| format!("graph.json: {e:?}"))?;
        let components = components_in(&build.join("components"))?;
        let mut plans = BTreeMap::new();
        for entry in std::fs::read_dir(build.join("pages")).map_err(|e| format!("pages: {e}"))? {
            let path = entry.map_err(|e| format!("pages: {e}"))?.path();
            let Some(page) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{page}: {e}"))?;
            plans.insert(
                page.to_string(),
                serde_json::from_str(&text).map_err(|e| format!("{page}: {e}"))?,
            );
        }
        // The page a document with none recorded is (ADR-0218): the store's,
        // where the program has it. A program without one has none.
        // **The data layer the program's contracts import** (ADR-0218): the
        // feed's where they import `feed:…`, the store's otherwise.
        // TRACK SEAM (uploads, ADR-0253): the uploads the program declares
        // (`uploads.json`), their blobs where the deployment keeps them.
        let uploads = uploads::Uploads::from_build(&build, &uploads::blob_root(&dist))?;
        let (store, data): (Arc<dyn store::StoreLayer>, Arc<dyn data::DataLayer>) = if contracts
            .iter()
            .flat_map(|c| &c.imports)
            .any(|i| i.interface.starts_with("feed:"))
        {
            (
                Arc::new(store::StoreData::new()),
                feed.unwrap_or_else(|| Arc::new(feed::FeedData::new())),
            )
        } else if contracts
            .iter()
            .flat_map(|c| &c.imports)
            .any(|i| i.interface.starts_with("kiokun:"))
        {
            // TRACK SEAM (kiokun): kiokun's files, read-only, where the
            // program imports `kiokun:data/…` (docs/PARALLEL.md, W6, Q2).
            // Unit tests assert the committed sample's answers. A developer's
            // deployment environment must not replace that test fixture.
            #[cfg(test)]
            let kiokun = kiokun::KiokunData::at(kiokun::sample(), None);
            #[cfg(not(test))]
            let kiokun = kiokun::KiokunData::new()?;
            (Arc::new(store::StoreData::new()), Arc::new(kiokun))
        } else {
            #[cfg(test)]
            let store = store.unwrap_or_else(store::layer_for_tests);
            #[cfg(not(test))]
            let store = store.unwrap_or_else(|| Arc::new(store::StoreData::new()));
            (store.clone(), store)
        };
        // TRACK SEAM (uploads, ADR-0253): the layer commits a post's image
        // from the uploads' leases, in the post's own transaction.
        if let Some(leases) = uploads.leases() {
            data.uploaded_by(leases);
            // And it says which blobs its committed rows name (ADR-0261):
            // the uploads serve no other. Held weakly, since the layer holds
            // the leases.
            let layer = Arc::downgrade(&data);
            uploads.named_by(Box::new(move |key: &str| {
                layer
                    .upgrade()
                    .ok_or_else(|| "the data layer is gone".to_string())?
                    .names_blob(key)
            }));
        }
        // **What the program's sources state, its database provides**
        // (ADR-0246): compared here, before anything is served, as an
        // operation no layer supplies is. ADR-0207 held a program to its
        // sources' statements; this holds the statements to the database. A
        // build from before `sources.json` states none.
        let declared: Vec<data::Declared> =
            match std::fs::read_to_string(build.join("sources.json")) {
                Ok(text) => {
                    serde_json::from_str(&text).map_err(|e| format!("sources.json: {e}"))?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(format!("sources.json: {e}")),
            };
        let short = data::held_to(&declared, &data.grants(), &data.provides()?);
        if !short.is_empty() {
            return Err(format!(
                "the data layer's database does not give what the program states: {}",
                short.join("; ")
            ));
        }
        // **Every predicate the program requires, this deployment
        // evaluates** (ADR-0302): one it cannot would answer each press of
        // its commands with nothing to tell, so it is refused here, before
        // anything is served. And the words the program declares for them.
        let words: BTreeMap<String, String> =
            match std::fs::read_to_string(build.join("predicates.json")) {
                Ok(text) => {
                    serde_json::from_str(&text).map_err(|e| format!("predicates.json: {e}"))?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(e) => return Err(format!("predicates.json: {e}")),
            };
        let unknown: std::collections::BTreeSet<&str> = contracts
            .iter()
            .flat_map(|c| c.exports.iter())
            .filter_map(|e| e.component.as_ref())
            .flat_map(|e| e.authorization.iter())
            .map(|a| a.predicate.as_str())
            .chain(words.keys().map(String::as_str))
            .filter(|p| identity::says(p).is_none())
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "this deployment cannot evaluate the predicate{} the program requires: {}",
                if unknown.len() == 1 { "" } else { "s" },
                unknown
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // The page a document with none recorded is, where the layer has one.
        let plan = data
            .default_page()
            .and_then(|page| plans.get(page))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let topology = topology_for(&data.grants());
        let mut server = Server::with_layer(
            dist,
            topology,
            Built {
                artifacts: build,
                build_id,
                templates,
                contracts,
                components,
                graph,
                plan,
                plans,
            },
            store,
            data,
        );
        // TRACK SEAM (uploads, ADR-0253): the uploads, and who uploads, as
        // the identity's principals say.
        uploads.identified_by(server.identity.principals());
        server.uploads = uploads;
        server.words = words;
        // **What the program imports, its data layer supplies** (ADR-0218):
        // refused here, as uncompiled handlers are, rather than when a press
        // first reaches the operation.
        let supplied = server.data.operations();
        for c in &server.contracts {
            for i in &c.imports {
                if i.kind != pw_host::ImportKind::HostCapability || i.interface.starts_with("pw:") {
                    continue;
                }
                // **The host's own operations are no program's** (track
                // `store-pg`): a route that changes the store's data stages
                // them, and no grant gives a program one.
                if i.key().starts_with(store::HOST_OPS) {
                    return Err(format!(
                        "`{}` imports `{}`, an operation the host keeps for its own routes",
                        c.component_id,
                        i.key()
                    ));
                }
                if !supplied.contains(&i.key()) {
                    return Err(format!(
                        "`{}` imports `{}`, which the data layer does not supply",
                        c.component_id,
                        i.key()
                    ));
                }
            }
        }
        Ok(server)
    }

    /// The server on the committed artifacts, on a topology the caller chooses.
    ///
    /// Exists so a test can run the command path against a node that grants
    /// nothing. A refusal that only ever happens in production is a refusal
    /// nobody has seen work.
    #[cfg(test)]
    fn on(dist: std::path::PathBuf, templates: Vec<Template>, topology: Topology) -> Server {
        Server::with(
            dist.clone(),
            topology,
            Built {
                artifacts: dist,
                // The committed artifacts are no `pw build`'s output.
                build_id: COMMITTED_ARTIFACTS.into(),
                templates,
                contracts: contracts(),
                components: components(),
                graph: pw_materialize::Graph::from_json(GRAPH).expect("the committed graph parses"),
                plan: serde_json::from_str(include_str!(
                    "../../../../docs/evidence/E10/pages/store.page.StorePage~ui.json"
                ))
                .expect("the committed page plan parses"),
                plans: BTreeMap::new(),
            },
        )
    }

    #[cfg(test)]
    fn with(dist: std::path::PathBuf, topology: Topology, built: Built) -> Server {
        let store = store::layer_for_tests();
        let data: Arc<dyn data::DataLayer> = store.clone();
        Server::with_layer(dist, topology, built, store, data)
    }

    /// [`Server::with`], its data layer given (ADR-0218): the store's own,
    /// or another program's, beside which the store's is empty.
    fn with_layer(
        dist: std::path::PathBuf,
        topology: Topology,
        Built {
            artifacts,
            build_id,
            templates,
            contracts,
            components,
            graph,
            plan,
            plans,
        }: Built,
        store: Arc<dyn store::StoreLayer>,
        data: Arc<dyn data::DataLayer>,
    ) -> Server {
        let speculations = speculation_manifests(&artifacts);
        let query_clock = pw_resource::Clock::new();
        let clock = Clock::new();
        let materializer = Materializer::new(clock.clone(), BUILD);
        materializer.declare("store.page.Cart", FragmentPolicy::default());
        materializer.declare("store.page.Menu", FragmentPolicy::default());
        // **Each materialization that derives its value** (ADR-0277), by the
        // policy it declares: what it serves where it could not be made, and
        // whether readers asking at once wait for one making.
        for node in graph.nodes.iter().filter(|n| n.node == "materialization") {
            materializer.declare(
                &node.path,
                FragmentPolicy {
                    fallback: match node.fallback.as_deref() {
                        Some("last_known_good") => pw_materialize::Fallback::LastKnownGood,
                        _ => pw_materialize::Fallback::None,
                    },
                    single_flight: node.stampede.as_deref() == Some("single_flight"),
                },
            );
        }
        // TRACK SEAM (identity): the layer maps a session to its principal's
        // user through the identity's principals.
        let identity = identity::Identity::default();
        data.identified_by(identity.principals());
        // TRACK SEAM (second-host): its sessions kept where the hosts share
        // them, where the layer's database is shared (ADR-0369).
        if let Some(kept) = data.sessions() {
            identity.principals().keep_in(kept);
        }
        // TRACK SEAM (store-accounts): whom the program's cart is, by the
        // key its pages read it with.
        let cart_by_reader = std::iter::once(&plan)
            .chain(plans.values())
            .flat_map(|plan| plan["bindings"].as_array().into_iter().flatten())
            .find(|b| b["resource"] == "store.page.Cart")
            .map(|b| {
                b["args"][0]
                    .as_str()
                    .is_some_and(notifications::is_current_user)
            });
        // TRACK SEAM (device-store): a page that holds a value the device
        // keeps hides what it decides until it is read, by a style its
        // policy allows by its hash (ADR-0366).
        let mut needs = csp::Needs::of(&templates);
        if std::iter::once(&plan)
            .chain(plans.values())
            .any(device::holds)
        {
            needs.styles.insert(device::READING.to_string());
            needs.styles.insert(device::UNSCRIPTED.to_string());
        }
        // TRACK SEAM (second-host): its pages numbered in its instance.
        let first_page = hosts::first_page(data.instance());
        Server {
            cart_by_reader,
            build_id,
            needs,
            templates,
            data,
            store,
            clock,
            materializer,
            kept: Mutex::new(BTreeMap::new()),
            made: Mutex::new(Vec::new()),
            graph,
            applied: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            components,
            dist,
            artifacts,
            pending: Table::default(),
            slow: Mutex::new(std::collections::VecDeque::new()),
            tellings: Mutex::new(std::collections::VecDeque::new()),
            origin: std::sync::OnceLock::new(),
            payments: std::sync::OnceLock::new(),
            shown: Mutex::new(BTreeMap::new()),
            documents: std::sync::atomic::AtomicU64::new(first_page),
            first_page,
            params: Mutex::new(BTreeMap::new()),
            pages: Mutex::new(BTreeMap::new()),
            answers: Mutex::new(BTreeMap::new()),
            telling: Mutex::new(Vec::new()),
            told: Mutex::new(BTreeMap::new()),
            words: BTreeMap::new(),
            speculated_versions: Mutex::new(BTreeMap::new()),
            split_fills: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            keyed_fetched: Mutex::new(None),
            #[cfg(test)]
            before_commit: Mutex::new(None),
            #[cfg(test)]
            deriving_hook: Mutex::new(None),
            #[cfg(test)]
            told_rendered: Mutex::new(None),
            #[cfg(test)]
            between_bindings: Mutex::new(None),
            #[cfg(test)]
            while_reading: Mutex::new(None),
            #[cfg(test)]
            in_flight: Mutex::new(None),
            connection_faults: Mutex::new(BTreeMap::new()),
            materializer_faults: Mutex::new(std::collections::BTreeSet::new()),
            keyed: Mutex::new(BTreeMap::new()),
            menu_rendered_from: Mutex::new(BTreeMap::new()),
            contracts,
            topology,
            speculations,
            plan,
            plans,
            queries: pw_resource::Resources::caching(query_clock.clone()),
            query_clock: (
                query_clock,
                std::time::Instant::now(),
                std::sync::atomic::AtomicU64::new(0),
            ),
            parallel: std::sync::atomic::AtomicU64::new(0),
            calls: Arc::default(),
            commands: pw_resource::Resources::new(pw_resource::Clock::new()),
            interactions: Mutex::new(BTreeMap::new()),
            epoch: wall_millis(),
            zones: Arc::new(pw_time::Database::system()),
            held: Mutex::new(std::collections::HashMap::new()),
            timers: clock::Timers::default(),
            order: order::Order::default(),
            // TRACK SEAM (identity): built above, its principals the layer's.
            identity,
            uploads: uploads::Uploads::default(),
            counted_runs: CountedRuns::default(),
            scheduled_locks: Mutex::new(BTreeMap::new()),
        }
    }

    /// **Whether the platform invokes `component_id`, and no reader** (NEXT
    /// 6c): a command `invoked_by Payments`. A press naming it is answered
    /// as a command that is not there; the host invokes it on its provider's
    /// word alone.
    fn invoked_by_platform(&self, component_id: &str) -> bool {
        payments::invoked_by_platform(&self.contracts, component_id)
    }

    /// **A payment the provider's word settled, acted on** (NEXT 6c, step
    /// seven): the command the program declares `invoked_by Payments`, run
    /// on behalf of the reader who began the payment, in a hold of its own
    /// never a browser's, with what the host has from the provider. Keyed by
    /// the payment and its status (ADR-0325), so an outcome heard twice, or
    /// by two hosts, commits once. A declared error is the program's answer,
    /// and acted on; a refusal or a trap is not, and the provider is answered
    /// so it sends the outcome again.
    fn settle_payment(&self, settled: &payments::Settled) -> Result<String, String> {
        let component_id = self
            .contracts
            .iter()
            .find(|c| {
                c.exports
                    .first()
                    .and_then(|e| e.component.as_ref())
                    .is_some_and(|e| e.invoked_by.as_deref() == Some("Payments"))
            })
            .map(|c| c.component_id.clone())
            .ok_or("the program declares no command invoked by Payments")?;
        let behalf = self.identity.principals().on_behalf(
            "payments",
            &format!("payment-{}", settled.intent),
            &settled.owner,
        );
        let session = behalf.session().to_string();
        let status = settled.status.replace('_', "-");
        let sent = serde_json::json!({
            "payment": settled.intent,
            "order": settled.order,
            "status": { "$case": status },
        });
        let interaction = format!("settle-{}-{status}", settled.intent);
        let answered = self.command_answer(&component_id, &session, &[sent], Some(&interaction))?;
        match (answered.committed, answered.why) {
            (true, _) => Ok(format!("{} is {}: settled", settled.intent, settled.status)),
            (false, Some(why)) => Err(why),
            (false, None) => Ok(format!(
                "{} is {}: the program answered {}",
                settled.intent,
                settled.status,
                answered.result.unwrap_or_default()
            )),
        }
    }

    /// **May this component run here, and with what?**
    ///
    /// One `admit` call, and no other source of truth. The command path calls
    /// this before doing anything, so authority is decided rather than assumed
    /// — the difference between a command that is callable and a command that
    /// is permitted.
    fn authorise(&self, component_id: &str) -> Result<Granted, String> {
        let Some(c) = self
            .contracts
            .iter()
            .find(|c| c.component_id == component_id)
        else {
            // A command with no contract is not a command this build produced.
            // Refused rather than run: "I have no record of this" and "this is
            // fine" must never be the same answer.
            return Err(format!("no contract for `{component_id}`"));
        };
        let node = &self.topology.nodes[0].name;
        // **The artifact's own imports**, read from the component the host is
        // about to run — not a list anyone wrote down. Until E10-I the body
        // was a Rust closure and the audit was given nothing.
        let actual = match self.components.get(component_id) {
            Some(loaded) => loaded.imports.clone(),
            None => return Err(format!("no compiled component for `{component_id}`")),
        };
        match admit(c, &self.topology, node, &actual) {
            Admission::Admit { .. } => {
                let a = admit(c, &self.topology, node, &actual);
                Granted::from(&a, &BTreeMap::new())
                    .ok_or_else(|| format!("`{component_id}` was admitted but granted nothing"))
            }
            Admission::Refuse(refusals) => Err(format!(
                "`{component_id}` may not run on `{node}`: {refusals:?}"
            )),
        }
    }

    /// The storage key, derived from the same identity the wire id is.
    fn cart_key(&self, session: &str) -> EntryKey {
        EntryKey::from_identity(&cart_identity(&self.owner(session), session))
    }

    /// **Whom `session`'s cart and order are** (track `store-accounts`): its
    /// reader's user, a signed-in principal's or the session's own guest
    /// (ADR-0270), where the program reads them by the reader's handle; the
    /// session itself where it reads them by the session.
    fn owner(&self, session: &str) -> String {
        match self.cart_by_reader {
            Some(true) => notifications::user_of(&self.identity.principals(), session),
            _ => session.to_string(),
        }
    }

    /// The session's cart's entry, as its pages are sent it.
    fn cart_entry(&self, session: &str) -> ResourceEntryId {
        cart_entry(&self.owner(session), session)
    }

    /// The session's order's entry.
    fn order_entry(&self, session: &str) -> ResourceEntryId {
        order_entry(&self.owner(session), session)
    }

    /// **The event the program declares for its cart, and for its order**
    /// (track `store-accounts`): a user's (`UserCartChanged`) where it reads
    /// them by the reader's handle, else a session's.
    fn cart_event(&self) -> &'static str {
        match self.cart_by_reader {
            Some(true) => "Events.UserCartChanged",
            _ => "Events.CartChanged",
        }
    }

    fn order_event(&self) -> &'static str {
        match self.cart_by_reader {
            Some(true) => "Events.UserOrderChanged",
            _ => "Events.OrderChanged",
        }
    }

    /// **A guest's cart joins its user's, at sign-in** (track
    /// `store-accounts`, ADR-0318; docs/PARALLEL.md, Q2): one transaction of
    /// the store's, the user's cart's event committed with it, and the user's
    /// open pages told. A join that fails leaves the sign-in done and the
    /// guest's lines where they were, and says so. A program that reads no
    /// cart has none to join.
    fn joined(&self, signed: &identity::SignedIn) {
        let Some(by_reader) = self.cart_by_reader else {
            return;
        };
        let guest = self.owner(signed.guest);
        let user = if by_reader {
            signed.principal.user.clone()
        } else {
            signed.session.to_string()
        };
        let event = (
            self.cart_event().to_string(),
            vec![Val::String(user.clone())],
        );
        let joined = self.store_write(
            signed.session,
            "store:host/carts#join",
            &[Val::String(guest), Val::String(user)],
            std::slice::from_ref(&event),
            |delivered| {
                let reached = self.invalidate_queries(signed.session, &[], delivered);
                if reached.is_empty() {
                    return;
                }
                self.telling
                    .lock()
                    .expect("telling")
                    .push((signed.session.to_string(), reached));
            },
        );
        if let Err(why) = joined {
            eprintln!(
                "pw dev server: a guest's cart did not join its user's at sign-in, and stays the guest's: {why}"
            );
        }
    }

    /// How many items the data layer holds for the session: the tests' view
    /// of the state. The page's count is `domain.line_count`'s (ADR-0125).
    /// Read through the layer, as a query reads it, so that a test reads
    /// the same on every layer (track `store-pg`).
    #[cfg(test)]
    fn cart_value(&self, session: &str) -> i64 {
        self.cart_lines(session).iter().map(|(_, q)| q).sum()
    }

    /// **The session's cart, read through the layer** (track `store-pg`):
    /// each line's item and quantity, in order.
    #[cfg(test)]
    fn cart_lines(&self, session: &str) -> Vec<(String, i64)> {
        let read = self.cart_read(session);
        match read {
            Val::Result(Ok(Some(cart))) => lines_of(&cart),
            other => panic!("carts#current answered {other:?}"),
        }
    }

    /// **Each line's name and price, as the layer answers them** (ADR-0172,
    /// track `store-pg`).
    #[cfg(test)]
    fn cart_priced(&self, session: &str) -> Vec<(String, i64)> {
        let Val::Result(Ok(Some(cart))) = self.cart_read(session) else {
            panic!("no cart");
        };
        let Some(Val::List(lines)) = val_at(&cart, &["lines"]) else {
            panic!("no lines: {cart:?}");
        };
        lines
            .iter()
            .map(
                |l| match (val_at(l, &["name"]), val_at(l, &["unit-price"])) {
                    (Some(Val::String(name)), Some(Val::S64(price))) => (name.clone(), *price),
                    other => panic!("no line: {other:?}"),
                },
            )
            .collect()
    }

    /// **The total the reader's cart page shows** (track `order-total`):
    /// what `place_order` is passed, as the page passes it, the
    /// `Checkout` query's total for the session's reader.
    #[cfg(test)]
    fn shown_total(&self, session: &str) -> Val {
        let owner = self.owner(session);
        let priced = self
            .query("store.page.Checkout", session, &[Val::String(owner)])
            .expect("the checkout");
        val_at(&priced, &["charges", "total"])
            .cloned()
            .unwrap_or_else(|| panic!("no total: {priced:?}"))
    }

    /// **The session's order, read through the layer** (ADR-0193, track
    /// `store-pg`): its status's case and the lines it was placed with, or
    /// none before one is placed.
    #[cfg(test)]
    fn order_of(&self, session: &str) -> Option<(String, Vec<(String, i64)>)> {
        let (current, placed) = match self.cart_by_reader {
            Some(true) => (
                "store:data/user-orders#current",
                "store:data/user-orders#placed",
            ),
            _ => ("store:data/orders#current", "store:data/orders#placed"),
        };
        let owner = self.owner(session);
        let status = match self.layer_read(session, current, &owner) {
            Val::Result(Ok(Some(v))) => match *v {
                Val::Option(None) => return None,
                Val::Option(Some(case)) => match *case {
                    Val::Variant(case, _) => case,
                    other => panic!("orders#current answered {other:?}"),
                },
                other => panic!("orders#current answered {other:?}"),
            },
            other => panic!("orders#current answered {other:?}"),
        };
        let lines = match self.layer_read(session, placed, &owner) {
            Val::Option(Some(cart)) => lines_of(&cart),
            Val::Option(None) => Vec::new(),
            other => panic!("orders#placed answered {other:?}"),
        };
        Some((status, lines))
    }

    /// **A change to the store's data at its source, untold** (track
    /// `store-pg`): the host's own operation `op`, written through the
    /// layer and committed with no event, as a change the program is not
    /// told of is. What a test arranges where it once wrote the in-memory
    /// layer's fields.
    #[cfg(test)]
    fn untold(&self, session: &str, op: &str, args: &[Val]) {
        self.store_write(session, op, args, &[], |_| {})
            .unwrap_or_else(|why| panic!("{op}: {why}"));
    }

    /// The session's order set at the source, untold (E14, T04).
    #[cfg(test)]
    fn order_set_untold(&self, session: &str, status: &str) {
        self.untold(
            session,
            "store:host/orders#set-status",
            &[
                Val::String(self.owner(session)),
                Val::Option(Some(Box::new(Val::String(status.into())))),
            ],
        );
    }

    /// An item sold out at the source, or back, untold (charter §15.5).
    #[cfg(test)]
    fn stock_untold(&self, item: &str, available: bool) {
        self.untold(
            "",
            "store:host/menus#stock",
            &[Val::String(item.into()), Val::Bool(available)],
        );
    }

    /// Store 47's menu changed at its source, untold (ADR-0178).
    #[cfg(test)]
    fn menu_untold(&self, op: MenuOp) {
        let mut args = vec![Val::String(STORE_ID.into())];
        args.extend(op.to_vals());
        self.untold("", "store:host/menus#change", &args);
    }

    /// **The session's cart, read through the layer as its pages read it**
    /// (track `store-accounts`): its reader's, by the reader's handle, or the
    /// session's.
    #[cfg(test)]
    fn cart_read(&self, session: &str) -> Val {
        let op = match self.cart_by_reader {
            Some(true) => "store:data/user-carts#current",
            _ => "store:data/carts#current",
        };
        self.layer_read(session, op, &self.owner(session))
    }

    /// One read of the data layer's, by its key, for `session`, passed
    /// `arg`.
    #[cfg(test)]
    fn layer_read(&self, session: &str, op: &str, arg: &str) -> Val {
        self.layer_read_args(session, op, &[Val::String(arg.to_string())])
    }

    /// [`layer_read`], with its arguments (track `order-total`).
    #[cfg(test)]
    fn layer_read_args(&self, session: &str, op: &str, args: &[Val]) -> Val {
        let ops = self.data.reads(session, None);
        let read = ops
            .get(op)
            .unwrap_or_else(|| panic!("the layer has no `{op}`"));
        let mut out = read(args).expect(op);
        assert_eq!(out.len(), 1, "{op}: {out:?}");
        out.remove(0)
    }

    /// **The platform's outbox, for one command** (ADR-0208, ADR-0209): each
    /// event and each invalidated entry its contract imports, linked to a
    /// function that stages what the command gives it, to be committed with
    /// its writes or not at all. The server never computes a key's values.
    fn outbox(
        &self,
        component_id: &str,
        host: &mut BTreeMap<String, HostFn>,
    ) -> Result<StagedEvents, String> {
        let contract = self
            .contracts
            .iter()
            .find(|c| c.component_id == component_id)
            .ok_or_else(|| format!("no contract for `{component_id}`"))?;
        let staged: StagedEvents = Arc::default();
        for import in &contract.imports {
            let into = staged.clone();
            // **A run it schedules, or cancels** (the scheduled-commands
            // track): the command's arguments, as a press sends them, and
            // when, staged with its writes.
            if let Some(scheduling) = import.schedules.clone() {
                let f: HostFn = Arc::new(move |values: &[Val]| {
                    let (given, when) = match (scheduling.how.as_str(), values.split_last()) {
                        ("at", Some((Val::S64(at), given))) => (given, schedules::When::At(*at)),
                        ("after", Some((Val::S64(ms), given))) => {
                            (given, schedules::When::After(*ms))
                        }
                        ("cancel", _) => (values, schedules::When::Cancel),
                        (how, _) => {
                            return Err(format!(
                                "a schedule `{how}` of `{}` was given {values:?}",
                                scheduling.command
                            ));
                        }
                    };
                    let args = serde_json::Value::Array(given.iter().map(val_to_json).collect())
                        .to_string();
                    into.lock()
                        .expect("staged runs")
                        .scheduled
                        .push(schedules::Scheduling {
                            key: schedules::key(&scheduling.command, &args),
                            command: scheduling.command.clone(),
                            args,
                            when,
                        });
                    Ok(Vec::new())
                });
                host.insert(import.key(), f);
                continue;
            }
            let f: HostFn = match (import.event.clone(), import.invalidates.clone()) {
                (Some(event), _) => Arc::new(move |values: &[Val]| {
                    into.lock()
                        .expect("staged events")
                        .events
                        .push((event.clone(), values.to_vec()));
                    Ok(Vec::new())
                }),
                (None, Some(query)) => {
                    // The value at each position, but those its key leaves
                    // to every value, `_`, which it is not given (ADR-0256).
                    let every = import.every.clone();
                    Arc::new(move |values: &[Val]| {
                        let mut given = values.iter();
                        let at: Vec<Option<Val>> = (0..values.len() + every.len())
                            .map(|i| match every.contains(&i) {
                                true => None,
                                false => given.next().cloned(),
                            })
                            .collect();
                        into.lock()
                            .expect("staged entries")
                            .invalidated
                            .push((query.clone(), at));
                        Ok(Vec::new())
                    })
                }
                (None, None) => continue,
            };
            host.insert(import.key(), f);
        }
        Ok(staged)
    }

    /// **A value a page speculates on, as its module reads it** (ADR-0233):
    /// by its query's declared result type, a value of a type that contains
    /// itself as its nodes. Until ADR-0233 it was written nested, knowing
    /// no type, and such a value was not speculated on (ADR-0205 §5).
    fn speculated_json(
        &self,
        plan: &serde_json::Value,
        binding: &str,
        value: &Val,
    ) -> Result<serde_json::Value, String> {
        let resource = plan["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| b["binding"] == binding)
            .and_then(|b| b["resource"].as_str())
            .ok_or_else(|| format!("no binding `{binding}`"))?;
        let export = self
            .contracts
            .iter()
            .find(|c| c.component_id == resource)
            .and_then(|c| c.exports[0].component.clone())
            .ok_or_else(|| format!("`{resource}`'s contract does not locate its export"))?;
        self.components
            .get(resource)
            .ok_or_else(|| format!("no compiled component `{resource}`"))?
            .prepared
            .browser_value(
                &[&export.interface, &export.function],
                value.clone(),
                &val_to_json,
            )
    }

    /// **Run one compiled command through the host.**
    ///
    /// Authority first — `admit` against the artifact's real imports — then
    /// the component, with the deployment's implementation of each granted
    /// operation. The command's result is whatever the COMPONENT returns.
    fn run(
        &self,
        component_id: &str,
        session: &str,
        host: &BTreeMap<String, HostFn>,
        args: &[Val],
    ) -> Result<Vec<Val>, String> {
        let granted = self.authorise(component_id)?;
        let contract = self
            .contracts
            .iter()
            .find(|c| c.component_id == component_id)
            .ok_or_else(|| format!("no contract for `{component_id}`"))?;
        let export = contract.exports[0]
            .component
            .clone()
            .ok_or_else(|| format!("`{component_id}`'s contract does not locate its export"))?;
        let prepared = &self
            .components
            .get(component_id)
            .ok_or_else(|| format!("no compiled component `{component_id}`"))?
            .prepared;
        let results = prepared.call_authorized_within(
            contract,
            &granted,
            &Limits {
                fuel: Some(50_000_000),
                memory_bytes: Some(16 * 1024 * 1024),
                table_elements: Some(1_000),
            },
            host,
            &[&export.interface, &export.function],
            args,
            // TRACK SEAM (identity, ADR-0253): what `requires` is told. The
            // important property is that each predicate is evaluated, and an
            // unknown one cannot run. A predicate over the command's
            // parameters reads through the command's own operations.
            |predicate, bound| self.identity.requires(predicate, bound, session, host),
        )?;
        // A type that contains itself arrives as its nodes, and a page reads
        // it nested (ADR-0194).
        prepared.untangled(&[&export.interface, &export.function], results)
    }

    /// The platform's session operation: the request's session, and nothing
    /// else the component could ask it for.
    fn session_operation(session: &str) -> HostFn {
        let session = session.to_string();
        Arc::new(move |_args: &[Val]| Ok(vec![Val::String(session.clone())]))
    }

    /// The materializer's version for this entry.
    ///
    /// Zero when nothing has been materialized yet. There is no counter here;
    /// asking the materializer is the only way the server learns a version.
    fn version(&self, session: &str) -> Version {
        let key = self.cart_key(session);
        Version(
            self.materializer
                .entry(&key)
                .map(|e| e.generated_at)
                .unwrap_or(0),
        )
    }

    /// **Run a command the compiler built, by its component id.**
    ///
    /// No code here knows which command it is running. The component runs
    /// its compiled body, for example
    ///
    /// ```text
    /// command add_to_cart(item: MenuItemId, quantity: PositiveInt) -> Result<Cart, CartError>
    /// {
    ///     Carts.add(current_session(), item, quantity)
    /// }
    /// ```
    ///
    /// and what this server supplies is the DEPLOYMENT: the platform's session
    /// operation, and [`Server::data_layer`], all of `store:data/carts`. The
    /// linker gives the component only what its contract was granted and its
    /// artifact imports. Every write STAGES; only if the component returns
    /// without an `Err` are the staged cart and its event committed together
    /// (ADR-0019), so a command that fails, or traps, commits nothing and the
    /// browser receives nothing.
    ///
    /// `fail` makes the data layer answer `cart-expired`, the declared
    /// refusal. A test's: a page's press reaches [`Server::command_answer`],
    /// and the browser meets a database's failure through `/bench/fail`
    /// (ADR-0174).
    #[cfg(test)]
    fn command(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        fail: bool,
    ) -> Result<(), String> {
        // The store's test fault, for this command alone (ADR-0174).
        self.store
            .faults()
            .fail_next
            .store(fail, std::sync::atomic::Ordering::SeqCst);
        let answered = self.command_answered(component_id, session, args, None)?;
        if answered.committed {
            return Ok(());
        }
        Err(format!(
            "{component_id} failed: {}",
            answered.result.unwrap_or_default()
        ))
    }

    /// [`Server::command`], and what the command answered (ADR-0157): its
    /// value, `Ok` or a declared `Err`, for the handler that called it. The
    /// outer error is a command that trapped or was refused, which answered
    /// no value.
    fn command_answered(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        interaction: Option<&str>,
    ) -> Result<Answered, String> {
        self.command_recorded(component_id, session, args, interaction, None)
    }

    /// [`Server::command_answered`], an idempotent command's answer committed
    /// with its writes by its interaction's key and the arguments it was sent
    /// with (ADR-0325).
    fn command_recorded(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        interaction: Option<&str>,
        record: Option<(&str, &str)>,
    ) -> Result<Answered, String> {
        self.command_recorded_for(component_id, session, args, interaction, record, None)?
            .into_answer()
    }

    fn command_recorded_for(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        interaction: Option<&str>,
        record: Option<(&str, &str)>,
        completing: Option<schedules::Completing<'_>>,
    ) -> Result<CommandOutcome, String> {
        let (answered, dropped) =
            self.command_held(component_id, session, args, interaction, record, completing)?;
        // Every other session holding what the commit dropped is told after
        // the author is answered (ADR-0219), so a post's answer does not
        // wait on every reader.
        if !dropped.is_empty() {
            self.telling_note(format_args!(
                "{} committed {component_id}: queued for others {dropped:?}",
                masked(session)
            ));
            self.telling
                .lock()
                .expect("telling")
                .push((session.to_string(), dropped));
        } else {
            self.telling_note(format_args!(
                "{} ran {component_id}: dropped nothing another session holds",
                masked(session)
            ));
        }
        Ok(answered)
    }

    /// [`Server::command_answered`] within the session's hold, and what its
    /// commit dropped that other sessions may hold (ADR-0219). `record` is an
    /// idempotent command's interaction, by its key and the arguments it was
    /// sent with: its answer is committed with its writes (ADR-0325).
    fn command_held(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        interaction: Option<&str>,
        record: Option<(&str, &str)>,
        completing: Option<schedules::Completing<'_>>,
    ) -> Result<(CommandOutcome, Vec<Reached>), String> {
        // The session's one change at a time, through its frames (ADR-0172).
        let session_lock = completing.is_none().then(|| self.one_at_a_time(session));
        let _one = session_lock
            .as_ref()
            .map(|lock| lock.lock().expect("one change of a session at a time"));
        // The data layer's writes, staged until the transaction commits
        // (ADR-0218): for the store, the session's cart, under one lock held
        // across the call and the commit — two presses in the same instant
        // both read the lines, and one of them would be lost
        // (`lazy-handler.spec.mjs` clicks twice concurrently to find that).
        {
            let mut staging = self.data.begin(session);
            let mut host = staging.ops();
            // The host's own operations are its routes', never a
            // component's (track `store-pg`).
            host.retain(|key, _| !key.starts_with(store::HOST_OPS));
            host.insert(
                "pw:host/session#read".to_string(),
                Self::session_operation(session),
            );
            // TRACK SEAM (notifications): the reader's user, `current_user()`.
            host.insert(
                notifications::PRINCIPAL_READ.to_string(),
                notifications::principal_operation(&self.identity.principals(), session),
            );
            // The platform's clock (ADR-0304), for the command's moment.
            host.extend(clock::operations(self.now(), &self.zones));
            // NEXT 6c: the deployment's payments, keyed by this interaction,
            // each begun by this session's reader, on whose behalf it is
            // settled (step seven).
            match self.payments.get() {
                Some(p) => {
                    let reader = self.identity.principals().user_of(session);
                    host.extend(payments::operations(
                        p.clone(),
                        interaction.map(str::to_string),
                        reader,
                    ));
                }
                // None taken: `begin` says so, and the order is placed as
                // before (step seven).
                None => host.extend(payments::not_taken()),
            }

            // What the command emits (ADR-0208): it computes each event's
            // values itself and hands them to the platform's outbox, which
            // stages them here with its writes.
            let staged_events = self.outbox(component_id, &mut host)?;
            let out = self.run(component_id, session, &host, args)?;
            let result = out.first().map(answer_json);
            if let [Val::Result(Err(_))] = out.as_slice() {
                // A declared error: nothing commits, and the handler is told
                // which (ADR-0157).
                let answered = Answered {
                    committed: false,
                    result,
                    why: None,
                    refused: None,
                };
                if let Some(completion) = completing {
                    // Release every old operation/claim and Writes before new staging.
                    drop(out);
                    drop(host);
                    drop(staged_events);
                    drop(staging);
                    return self
                        .complete_scheduled_error(session, record, answered, completion)
                        .map(|outcome| (outcome, Vec::new()));
                }
                return Ok((CommandOutcome::Answered(answered), Vec::new()));
            }
            let (emitted, invalidated, scheduling) = {
                let staged = staged_events.lock().expect("staged");
                (
                    staged.events.clone(),
                    staged.invalidated.clone(),
                    staged.scheduled.clone(),
                )
            };
            // **Its runs, as its commit writes them** (the scheduled-commands
            // track): a duration counted from now, each on behalf of this
            // session's user.
            let scheduler = self.identity.principals().user_of(session);
            let now_ms = self.now();
            let scheduled: Vec<schedules::Scheduled> = scheduling
                .iter()
                .map(|s| s.at_commit(now_ms, &scheduler))
                .collect();
            // **Its answer, committed with its writes** (ADR-0325): what a
            // retry of its interaction is answered, here after a restart or on
            // another host, kept a day.
            let answer = Answered {
                committed: true,
                result: result.clone(),
                why: None,
                refused: None,
            }
            .kept();
            let now = u64::try_from(self.now()).unwrap_or_default();
            let recorded = record.map(|(key, sent)| data::Recorded {
                key,
                sent,
                answer: &answer,
                at_ms: now,
                forget_before_ms: now.saturating_sub(INTERACTIONS_KEPT_MS),
            });
            #[cfg(test)]
            if let Some(hook) = self.before_commit.lock().expect("before commit").take() {
                hook(self);
            }
            let committed = match self.commit_staged(
                &mut *staging,
                &emitted,
                &invalidated,
                recorded.as_ref(),
                &scheduled,
                completing,
            ) {
                Ok(committed) => committed,
                // **Recorded first by another host** (ADR-0325): the same
                // interaction committed there, and its record holds the key.
                // A refusal rolled back or its acknowledgement was lost;
                // an exact authoritative receipt answers either case.
                Err(why) => match record {
                    Some((key, sent)) => match self.recalled_interaction(key)? {
                        Some((first, answer)) if first == sent => {
                            return Ok((
                                CommandOutcome::Answered(Answered::from_kept(&answer)),
                                Vec::new(),
                            ));
                        }
                        _ => return Err(why),
                    },
                    None => return Err(why),
                },
            };
            // A run it scheduled may be due before the next the host
            // waits for: the host looks again.
            if !scheduled.is_empty() {
                self.timers.changed.notify_all();
            }
            let committed = match committed {
                CommitOutcome::Applied(value) => Some(value),
                CommitOutcome::NoWrites => None,
                CommitOutcome::NotCurrent => return Ok((CommandOutcome::NotCurrent, Vec::new())),
            };
            let Some(((emitted, invalidated), ids)) = committed else {
                // Nothing was written, so there is nothing to commit.
                let answered = Answered {
                    committed: true,
                    result,
                    why: None,
                    refused: None,
                };
                return Ok((CommandOutcome::Answered(answered), Vec::new()));
            };
            // What the cart's values include from now on, by the interaction
            // that committed it (ADR-0172), recorded with the commit.
            if let Some(interaction) = interaction.filter(|_| completing.is_none()) {
                let mut applied = self.applied.lock().expect("applied");
                let mine = applied.entry(session.to_string()).or_default();
                mine.push_back(interaction.to_string());
                while mine.len() > INTERACTIONS_PER_SESSION {
                    mine.pop_front();
                }
            }
            let reached = self.invalidate_queries(session, &invalidated, &emitted);
            // The materializer drains the committed event and regenerates the
            // entry it invalidates. The version moves because the RESOURCE
            // moved.
            drop(staging);
            // The session's documents, at the version the commit made: by the
            // layer's own entry for the session (the store's cart), or on the
            // host's clock (ADR-0218).
            if self.data.session_entry() {
                if completing.is_none() {
                    self.drain_held(session);
                }
            } else {
                self.clock.advance(1);
                if completing.is_none() {
                    let version = Version(self.clock.now());
                    self.send_documents(session, &session_documents(session), version, false);
                }
            }
            // Delivered: every event the commit staged is consumed, the
            // order's among them, which no entry drained above names (track
            // `store-pg`). Until 2026-10-08 each order placed left its
            // `OrderChanged` in the materializer's outbox for good.
            self.materializer.delivered(&ids);
            let answered = Answered {
                committed: true,
                result,
                why: None,
                refused: None,
            };
            Ok((CommandOutcome::Answered(answered), reached))
        }
    }

    fn complete_scheduled_error(
        &self,
        session: &str,
        record: Option<(&str, &str)>,
        answer: Answered,
        completing: schedules::Completing<'_>,
    ) -> Result<CommandOutcome, String> {
        let mut empty = self.data.begin(session);
        let kept = answer.kept();
        let now = u64::try_from(self.now()).unwrap_or_default();
        let recorded = record.map(|(key, sent)| data::Recorded {
            key,
            sent,
            answer: &kept,
            at_ms: now,
            forget_before_ms: now.saturating_sub(INTERACTIONS_KEPT_MS),
        });
        match self.commit_staged(
            &mut *empty,
            &[],
            &[],
            recorded.as_ref(),
            &[],
            Some(completing),
        )? {
            CommitOutcome::Applied((_, ids)) => {
                self.materializer.delivered(&ids);
                Ok(CommandOutcome::Answered(answer))
            }
            CommitOutcome::NotCurrent => Ok(CommandOutcome::NotCurrent),
            CommitOutcome::NoWrites => Err("scheduled terminal completion did not commit".into()),
        }
    }

    /// **A command's writes and its events, committed** (ADR-0019, ADR-0208,
    /// ADR-0246; track `store-pg`): the one commit path, a program's
    /// command's and a route's that changes the store's data. `None` where
    /// nothing was written, and nothing commits.
    ///
    /// A layer that keeps its own outbox commits the writes and the events
    /// in its own transaction, and what it committed is what is delivered,
    /// read back from its outbox (ADR-0246); refused, nothing of the
    /// command is kept, and no one is told. A layer that keeps the session's
    /// entry (the store on PostgreSQL) then has its commit recorded in the
    /// materializer: the rows and the events it read back, in one
    /// materializer transaction, as the in-memory layer's commit is
    /// written, so the session's entry moves as it does in memory. The
    /// database stays the authority: a record that fails after it committed
    /// is said, and the command is answered committed.
    ///
    /// Returns what is delivered, and the materializer's ids for its events,
    /// which are consumed once delivered ([`Materializer::delivered`]).
    fn commit_staged(
        &self,
        staging: &mut dyn data::Staged,
        emitted: &[data::Handed],
        invalidated: &[data::Dropped],
        interaction: Option<&data::Recorded<'_>>,
        scheduled: &[schedules::Scheduled],
        completing: Option<schedules::Completing<'_>>,
    ) -> Result<CommitOutcome, String> {
        // A command that wrote nothing commits nothing, unless it scheduled
        // or cancelled a run (the scheduled-commands track), which is
        // written as its writes are.
        let mut rows = match staging.rows() {
            Some(rows) => rows,
            None if !scheduled.is_empty() || completing.is_some() => Vec::new(),
            None => return Ok(CommitOutcome::NoWrites),
        };
        if completing.is_some()
            && matches!(staging.projection_scope(), data::ProjectionScope::Session)
        {
            rows.clear();
        }

        let events = emitted
            .iter()
            .map(|(event, values)| outboxed(event, values))
            .collect::<Result<Vec<_>, String>>()?;
        let (delivered, ids) = match staging.commit(
            emitted,
            invalidated,
            interaction,
            scheduled,
            completing,
        )? {
            data::Commit::Applied(committed) => {
                let mut ids = Vec::new();
                if self.data.session_entry() {
                    let recorded = committed
                        .0
                        .iter()
                        .map(|(event, values)| outboxed(event, values))
                        .collect::<Result<Vec<_>, String>>()
                        .and_then(|events| {
                            self.materializer.command(|tx| {
                                for (key, value) in &rows {
                                    Materializer::set_state(tx, key, value);
                                }
                                Ok::<_, String>(events)
                            })
                        });
                    match recorded {
                        Ok(recorded) => ids = recorded,
                        Err(why) => eprintln!(
                            "pw dev server: a commit was not recorded in the materializer: {why}"
                        ),
                    }
                }
                (committed, ids)
            }
            data::Commit::NotCurrent => return Ok(CommitOutcome::NotCurrent),
            data::Commit::Materializer => {
                // The state the data layer staged and the outbox's events, in
                // one transaction (ADR-0019, ADR-0208); and the interaction's
                // answer, where the command has one (ADR-0325).
                let mut current = true;
                let ids = self.materializer.command(|tx| {
                    if let Some(completion) = completing {
                        if !completion.belongs_to(self.materializer.queue_authority()) {
                            return Err(
                                "scheduled completion belongs to another queue adapter".to_string()
                            );
                        }
                        if !Materializer::consume_run(tx, completion.run, completion.now_ms)? {
                            current = false;
                            return Ok(Vec::new());
                        }
                    }
                    staging.prepare_materialized()?;
                    for (key, value) in &rows {
                        Materializer::set_state(tx, key, value);
                    }
                    if let Some(r) = interaction {
                        Materializer::prune_interactions(tx, r.forget_before_ms);
                        Materializer::record_interaction(tx, r.key, r.sent, r.answer, r.at_ms)?;
                    }
                    // And the runs it scheduled, or cancelled (the
                    // scheduled-commands track).
                    schedules::write(tx, scheduled)?;
                    Ok::<_, String>(events)
                })?;
                if !current {
                    return Ok(CommitOutcome::NotCurrent);
                }
                ((emitted.to_vec(), invalidated.to_vec()), ids)
            }
        };
        // What was staged is the data layer's from now on.
        staging.publish();
        Ok(CommitOutcome::Applied((delivered, ids)))
    }

    /// **A route's change to the store's data, committed as a command's is**
    /// (track `store-pg`): the host's own operation `op`, staged through the
    /// layer in a command's transaction for `session`, and committed with
    /// `events` in it. `deliver` is handed what committed, once the staging
    /// is given up, and the events are consumed after it.
    ///
    /// The operation's answer, `Ok` with its value where it has one; `Err`
    /// where it refused, or the commit was, and nothing is kept.
    fn store_write(
        &self,
        session: &str,
        op: &str,
        args: &[Val],
        events: &[data::Handed],
        deliver: impl FnOnce(&[data::Handed]),
    ) -> Result<Option<Val>, String> {
        let mut staging = self.data.begin(session);
        let ops = staging.ops();
        let run = ops
            .get(op)
            .ok_or_else(|| format!("the data layer has no `{op}`"))?;
        let answer = match run(args)?.as_slice() {
            [Val::Result(Ok(answer))] => answer.as_deref().cloned(),
            [Val::Result(Err(why))] => {
                return Err(match why.as_deref() {
                    Some(Val::String(why)) => why.clone(),
                    other => format!("{op} refused: {other:?}"),
                });
            }
            other => return Err(format!("{op} answered {other:?}")),
        };
        let ((delivered, _), ids) =
            match self.commit_staged(&mut *staging, events, &[], None, &[], None)? {
                CommitOutcome::Applied(value) => value,
                CommitOutcome::NoWrites => return Ok(answer),
                CommitOutcome::NotCurrent => {
                    return Err("unexpected scheduled disposition on a platform route".into());
                }
            };
        drop(staging);
        deliver(&delivered);
        self.materializer.delivered(&ids);
        Ok(answer)
    }

    /// **What the recommender suggests, curated** (ADR-0148, track
    /// `store-pg`), for every store, or drawn from each store's menu again
    /// where `items` is none: the store's data, written through the layer.
    fn curate(&self, items: Option<&[(String, String)]>) -> Result<(), String> {
        let items = Val::Option(items.map(|items| {
            Box::new(Val::List(
                items
                    .iter()
                    .map(|(id, name)| {
                        Val::Record(vec![
                            ("id".into(), Val::String(id.clone())),
                            ("name".into(), Val::String(name.clone())),
                        ])
                    })
                    .collect(),
            ))
        }));
        self.store_write(
            "",
            "store:host/recommendations#curate",
            &[items],
            &[],
            |_| {},
        )
        .map(|_| ())
    }

    /// **What a session's estimator estimates** (E14, T10; ADR-0180, track
    /// `store-pg`): the least minutes and, where said, the most, written
    /// through the layer.
    fn set_estimate(&self, session: &str, minutes: i64, max: Option<i64>) -> Result<(), String> {
        self.store_write(
            session,
            "store:host/estimates#set",
            &[
                Val::String(session.to_string()),
                Val::S64(minutes),
                Val::Option(max.map(|m| Box::new(Val::S64(m)))),
            ],
            &[],
            |_| {},
        )
        .map(|_| ())
    }

    /// **A store's items' ids, in its order**, read through the layer
    /// (track `store-pg`).
    fn menu_ids(&self, store: &str) -> Result<Vec<String>, String> {
        let ops = self.data.reads("", None);
        let read = ops
            .get("store:data/menus#for-store")
            .ok_or("the data layer has no `menus#for-store`")?;
        match read(&[Val::String(store.to_string())])?.as_slice() {
            [Val::Result(Ok(Some(menu)))] => match menu.as_ref() {
                Val::List(items) => Ok(items
                    .iter()
                    .filter_map(|i| match val_at(i, &["id"]) {
                        Some(Val::String(id)) => Some(id.clone()),
                        _ => None,
                    })
                    .collect()),
                other => Err(format!("menus#for-store answered {other:?}")),
            },
            other => Err(format!("menus#for-store answered {other:?}")),
        }
    }

    /// **A command whose arguments arrived from a browser, as JSON.**
    ///
    /// A compiled handler runs in the user's browser, so its arguments are a
    /// claim. They are typed by the command component's OWN parameters, read
    /// from the artifact by the host, and refused otherwise, before anything
    /// runs. The outer error is a malformed request (the arguments); the inner
    /// one is a command that ran and did not commit.
    ///
    /// An idempotent command (its contract's `idempotent_by`, ADR-0121) runs
    /// at most once per `interaction`: a retried request gets the first
    /// request's outcome, and nothing runs again. Without an interaction it is
    /// refused before anything runs, and so is an interaction sent again with
    /// other arguments.
    #[cfg(test)]
    fn command_json(
        &self,
        component_id: &str,
        session: &str,
        json: &[serde_json::Value],
        interaction: Option<&str>,
    ) -> Result<Result<(), String>, String> {
        let answered = self.command_answer(component_id, session, json, interaction)?;
        Ok(match answered.committed {
            true => Ok(()),
            false => Err(answered.why.unwrap_or_else(|| {
                format!(
                    "{component_id} failed: {}",
                    answered.result.unwrap_or_default()
                )
            })),
        })
    }

    /// [`Server::command_json`], and what the command answered (ADR-0157).
    /// A retried interaction is given the first request's answer, its value
    /// included.
    fn command_arguments(
        &self,
        component_id: &str,
        json: &[serde_json::Value],
    ) -> Result<(pw_host::ComponentExport, Vec<Val>), String> {
        let loaded = self
            .components
            .get(component_id)
            .ok_or_else(|| format!("no compiled component for `{component_id}`"))?;
        let export = self
            .contracts
            .iter()
            .find(|c| c.component_id == component_id)
            .and_then(|c| c.exports.first())
            .and_then(|e| e.component.clone())
            .ok_or_else(|| format!("`{component_id}`'s contract does not locate its export"))?;
        // Typed by the export's parameters, and held to the invariants its
        // contract states (ADR-0179): a forged quantity of 0 is refused here.
        let args = loaded.prepared.arguments_for(&export, json)?;
        Ok((export, args))
    }

    fn command_answer(
        &self,
        component_id: &str,
        session: &str,
        json: &[serde_json::Value],
        interaction: Option<&str>,
    ) -> Result<Answered, String> {
        let (export, args) = self.command_arguments(component_id, json)?;
        // With the predicate `requires` refused it by, where one did
        // (ADR-0302): taken on the thread that ran it, as it ends, so it is
        // kept with the interaction.
        let answered = |run: Result<Answered, String>| {
            let mut answered = match run {
                Ok(answered) => answered,
                Err(why) => Answered {
                    committed: false,
                    result: None,
                    why: Some(why),
                    refused: None,
                },
            };
            answered.refused = identity::take_refusal();
            answered
        };
        let Some(key_type) = &export.idempotent_by else {
            return Ok(answered(self.command_answered(
                component_id,
                session,
                &args,
                interaction,
            )));
        };
        let id = interaction.ok_or_else(|| {
            format!(
                "`{component_id}` is idempotent_by {key_type}, and the request carries no interaction id"
            )
        })?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("`{id}` is not an interaction id"));
        }
        let key = format!("{session}\u{1f}{component_id}\u{1f}{id}");
        let sent = serde_json::Value::Array(json.to_vec()).to_string();
        // **A committed record answers it** (ADR-0325): sent again after the
        // host restarted, past the session's last 64, or to another host, the
        // interaction is answered as it was, and nothing runs.
        if let Some((first, answer)) = self.recalled_interaction(&key)? {
            if first != sent {
                return Err(format!(
                    "interaction `{id}` was first sent with other arguments"
                ));
            }
            return Ok(Answered::from_kept(&answer));
        }
        {
            let mut held = self.interactions.lock().expect("interactions");
            let queue = held.entry(session.to_string()).or_default();
            match queue.iter().find(|(k, _)| *k == key) {
                Some((_, first)) if *first != sent => {
                    return Err(format!(
                        "interaction `{id}` was first sent with other arguments"
                    ));
                }
                Some(_) => {}
                None => {
                    queue.push_back((key.clone(), sent.clone()));
                    while queue.len() > INTERACTIONS_PER_SESSION {
                        if let Some((oldest, _)) = queue.pop_front() {
                            self.commands.forget_command(&oldest);
                        }
                    }
                }
            }
        }
        let outcome = self
            .commands
            .try_command(&key, || {
                answered(self.command_recorded(
                    component_id,
                    session,
                    &args,
                    Some(id),
                    Some((&key, &sent)),
                ))
                .kept()
            })
            .map_err(|e| format!("interaction `{id}`'s outcome is unknown: {e:?}"))?;
        Ok(Answered::from_kept(&outcome))
    }

    /// **An interaction's committed record** (ADR-0325), kept a day on the
    /// host's clock: the arguments it was sent with and its answer, from the
    /// data layer that keeps it beside its writes, or the host's
    /// materializer. One older is a new request's, pruned or not.
    fn recalled_interaction(&self, key: &str) -> Result<Option<(String, String)>, String> {
        let since = u64::try_from(self.now())
            .unwrap_or_default()
            .saturating_sub(INTERACTIONS_KEPT_MS);
        if self.data.keeps_interactions() {
            self.data.recalled(key, since)
        } else {
            Ok(self.materializer.recalled(key, since))
        }
    }

    /// **Whether the session's next command connection is dropped, and
    /// where** (ADR-0175): taken, so it is dropped once.
    fn take_command_drop(&self, session: &str) -> Option<DropAt> {
        self.connection_faults
            .lock()
            .expect("connection faults")
            .get_mut(session)
            .and_then(|f| f.drop_command.take())
    }

    /// **Whether the session's next command connection is held open, and
    /// where** (ADR-0381): taken, so it is held once.
    fn take_command_hold(&self, session: &str) -> Option<HoldAt> {
        self.connection_faults
            .lock()
            .expect("connection faults")
            .get_mut(session)
            .and_then(|f| f.hold_command.take())
    }

    /// **Is a subscription of the session's, opened at `opened`, cut off
    /// now?** (ADR-0175): cut since it opened, or inside a window in which
    /// every new one is closed.
    fn cut_off(&self, session: &str, opened: std::time::Instant) -> bool {
        self.connection_faults
            .lock()
            .expect("connection faults")
            .get(session)
            .is_some_and(|f| {
                f.cut_from.is_some_and(|from| from > opened)
                    || f.cut_until
                        .is_some_and(|until| std::time::Instant::now() < until)
            })
    }

    /// **A session's resource changed outside a command** (ADR-0193): the
    /// store moved an order along. The change arrives as the event the
    /// program declares for it, `event` with the session as its key, and
    /// each of the session's documents is read again and sent what changed,
    /// against `entry` at its next version. Until 2026-10-04 only a command
    /// on the cart reached an open page: the store's order was read again
    /// when the page was.
    ///
    /// A session's query keeps no answer (PW0102), but a read of it may be in
    /// flight from before the change, and one `one_per_key` lets the read
    /// below join. The event drops it, as a command's commit drops the reads
    /// it makes stale (ADR-0127), so the read below starts after the change.
    fn session_changed(&self, session: &str, event: &str, entry: &ResourceEntryId) {
        let session_lock = self.one_at_a_time(session);
        let _one = session_lock
            .lock()
            .expect("one change of a session at a time");
        // The event names whom the change is (track `store-accounts`): the
        // session's owner, its user where the program reads by the reader.
        self.invalidate_queries(
            session,
            &[],
            &[(event.to_string(), vec![Val::String(self.owner(session))])],
        );
        self.clock.advance(1);
        let version = Version(self.clock.now());
        self.send_documents(session, entry, version, false);
    }

    /// **Forget the subscribers that stopped asking, and what was kept for
    /// them.** Their queues go, and so do their sessions' materialized cart
    /// entries: a returning page is served a new document, and `drain`
    /// regenerates a missing entry from state. 1,000 departed visitors had
    /// kept 1,001 entries (`docs/evidence/E10/load.txt`).
    ///
    /// In its own short hold of the subscriber table, released before any
    /// entry is evicted: `drain` takes the materializer and then the table,
    /// so evicting while holding the table would take them the other way.
    fn forget_idle_subscribers(&self) {
        let (documents, forgotten) = {
            let mut queue = self.pending.lock().expect("pending");
            forget_idle(&mut queue, std::time::Instant::now())
        };
        // What each forgotten document showed and read (ADR-0161), each in
        // its own hold: `read_keyed` takes `keyed` before `shown`.
        {
            let mut keyed = self.keyed.lock().expect("keyed");
            for doc in &documents {
                keyed.remove(doc);
            }
        }
        {
            let mut shown = self.shown.lock().expect("shown");
            for doc in &documents {
                shown.remove(doc);
            }
        }
        {
            let mut params = self.params.lock().expect("params");
            for doc in &documents {
                params.remove(doc);
            }
        }
        {
            let mut pages = self.pages.lock().expect("pages");
            for doc in &documents {
                pages.remove(doc);
            }
        }
        {
            let mut answers = self.answers.lock().expect("answers");
            for doc in &documents {
                answers.remove(doc);
            }
        }
        for session in forgotten {
            self.materializer.evict(&self.cart_key(&session));
            // And the query values kept for it alone (ADR-0127): a private
            // entry's key begins with its session.
            let own = format!("session={session}");
            let mine = |k: &str| k == own || k.starts_with(&format!("{own}\u{1f}"));
            self.queries.evict_where(|k| mine(&k.key));
            // And its interactions: a forgotten session's retry runs again,
            // which is the bound's stated cost (ADR-0121).
            let keys = self
                .interactions
                .lock()
                .expect("interactions")
                .remove(&session)
                .unwrap_or_default();
            for (key, _) in keys {
                self.commands.forget_command(&key);
            }
            // And what its kept values held until (ADR-0304).
            self.held.lock().expect("held").retain(|k, _| !mine(&k.key));
            // And its commands' order (ADR-0305).
            self.order.forget(&session);
            // And the versions its speculated values were sent at (ADR-0222).
            self.speculated_versions
                .lock()
                .expect("speculated versions")
                .retain(|(s, ..), _| *s != session);
        }
    }

    /// **Why a command from `document` is not `session`'s to send** (track
    /// `store-accounts`, Q3), or nothing where it is: a document this host
    /// served to another session, whose reader signed in or out in another
    /// tab since, or one it no longer holds, forgotten while idle; or one
    /// another host served (track `second-host`, its instance in the
    /// number's high bits). Document numbers are each host's own, one per
    /// document served and apart from every other host's on its database,
    /// so one names one session's document. The words are the platform's,
    /// as ADR-0302's for a page out of date.
    fn stale_document(&self, session: &str, document: u64) -> Option<(&'static str, &'static str)> {
        // No document: a page the host did not serve, or no page.
        if document == 0 {
            return None;
        }
        let pages = self.pages.lock().expect("pages");
        match pages.keys().find(|(_, n)| *n == document) {
            Some((shown_to, _)) if shown_to == session => None,
            Some(_) => Some(ANOTHER_READER),
            // TRACK SEAM (second-host): a page another host served.
            None if hosts::another_hosts(document, self.first_page) => Some(ANOTHER_HOST),
            None => Some(PAGE_NOT_HELD),
        }
    }

    /// **What the host kept for a press made on `document`** (track
    /// `store-accounts`, the integrator's finding): its interaction's
    /// outcome, kept under the session the page was shown to (the
    /// session's own where the host no longer holds the page), with the
    /// arguments it was first sent with. Nothing runs, and nothing waits: a
    /// press still running, or none kept, is `None`.
    fn kept_answer(
        &self,
        session: &str,
        document: u64,
        component_id: &str,
        json: &[serde_json::Value],
        interaction: Option<&str>,
    ) -> Option<Answered> {
        let id = interaction?;
        let shown_to = self
            .pages
            .lock()
            .expect("pages")
            .keys()
            .find(|(_, n)| *n == document)
            .map(|(shown_to, _)| shown_to.clone())
            .unwrap_or_else(|| session.to_string());
        let key = format!("{shown_to}\u{1f}{component_id}\u{1f}{id}");
        let sent = serde_json::Value::Array(json.to_vec()).to_string();
        let first_sent = self
            .interactions
            .lock()
            .expect("interactions")
            .get(&shown_to)
            .is_some_and(|queue| queue.iter().any(|(k, first)| *k == key && *first == sent));
        if !first_sent {
            // TRACK SEAM (second-host): **or what the deployment kept**
            // (ADR-0325, ADR-0369): a press this host never ran, made on a
            // page another host served (or this one, before it restarted),
            // which committed there, is answered from the record its commit
            // wrote with its writes, as this host's memory answers its own;
            // nothing runs. One never committed has no record, and is
            // refused as before. It is looked for under the page's session,
            // as above: the request's own where this host does not hold the
            // page (page numbers are apart on every host, so another host's
            // page is never one this host showed another session).
            return match self.recalled_interaction(&key) {
                Ok(Some((first, answer))) if first == sent => Some(Answered::from_kept(&answer)),
                Ok(_) => None,
                Err(why) => {
                    eprintln!("pw dev server: an interaction's record was not read: {why}");
                    None
                }
            };
        }
        self.commands
            .command_outcome(&key)
            .map(|kept| Answered::from_kept(&kept))
    }

    /// `pw-resource`'s clock, brought to wall time (ADR-0127).
    /// **The host's wall clock** (ADR-0304), in milliseconds since the
    /// epoch: the system's at start, moved since as the query clock is, by
    /// time and by a test (`/bench/clock`). One clock, so what is kept and
    /// what is compared move together.
    fn now(&self) -> i64 {
        self.sync_query_clock();
        self.epoch + i64::try_from(self.query_clock.0.now()).unwrap_or(i64::MAX)
    }

    /// **Where this host's scheduled runs are kept** (the scheduled-commands
    /// track): the layer's, where it writes them with its commands' writes;
    /// the materializer's otherwise.
    fn runs(&self) -> &dyn schedules::Runs {
        match self.data.runs() {
            Some(runs) => runs,
            None => &self.materializer,
        }
    }

    /// **Each scheduled run, run once the clock passes its time** (the
    /// scheduled-commands track), for as long as the host runs: woken when a
    /// commit schedules one or the clock is moved, and each second at least.
    fn run_when_due(&self) {
        self.drive_scheduled(|| true, |wait| self.timers.wait(wait));
    }

    fn drive_scheduled(&self, keep_running: impl Fn() -> bool, wait: impl Fn(u64)) {
        while keep_running() {
            let (_selected, delay) = self.run_due();
            wait(delay);
        }
    }

    /// Each run due now, one at a time, in the order of their instants: how
    /// many, and how long until the next, a second at most.
    fn run_due(&self) -> (usize, u64) {
        let now = self.now();
        let due = match schedules::select(self.runs(), now, 16) {
            Ok(due) => due,
            Err(why) => {
                eprintln!("pw dev server: the scheduled runs were not read: {why}");
                return (0, 1000);
            }
        };
        let mut blocked = false;
        for run in &due {
            blocked |= self.run_scheduled(run);
        }
        // What the runs' commits reached, told to each session reading it,
        // as a connection's command's is once it is answered (ADR-0219):
        // no connection made these.
        if !due.is_empty() {
            self.tell_waiting();
        }
        let next = self.runs().next_run().unwrap_or(None);
        let wait = if blocked {
            1000
        } else {
            next.map_or(1000, |at| (at - self.now()).clamp(1, 1000))
        };
        (due.len(), u64::try_from(wait).unwrap_or(1000))
    }

    /// **A scheduled run, run** (the scheduled-commands track): its command,
    /// on behalf of the user whose command scheduled it, in a session of its
    /// own never a browser's, as a press is run, keyed by its scheduling so
    /// that it commits once (ADR-0325): run again after a restart, or by a
    /// second host, it is answered as it was. One that committed, or
    /// answered a declared error, which is the program's answer, is done.
    /// One that trapped or was refused runs again, after 2 s doubling to a
    /// minute; after [`schedules::RETRIES`] runs again it is kept, failed,
    /// and said in the host's log and its `/metrics`.
    fn scheduled_lock(&self, id: i64) -> Arc<Mutex<()>> {
        let mut held = self.scheduled_locks.lock().expect("scheduled locks");
        held.retain(|_, weak| weak.strong_count() != 0);
        if let Some(lock) = held.get(&id).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        held.insert(id, Arc::downgrade(&lock));
        lock
    }

    fn run_scheduled(&self, run: &schedules::Candidate<'_>) -> bool {
        // Protect the principal lease on this host without permanent result memoization.
        let lock = self.scheduled_lock(run.id);
        let _one = lock.lock().expect("one scheduled generation on this host");
        let source = self.runs();
        if !run.belongs_to(source.authority()) {
            eprintln!("pw dev server: scheduled candidate belongs to another adapter");
            return false;
        }
        match source.current(run, self.now()) {
            Ok(true) => {}
            Ok(false) => {
                self.counted_runs
                    .stale
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return false;
            }
            Err(why) => {
                eprintln!("pw dev server: scheduled authority could not be checked: {why}");
                return true;
            }
        }
        if run.protocol != 1 {
            return match source.retry_current(
                run,
                run.attempts,
                run.due_ms,
                Some("legacy scheduled work requires reconciliation"),
            ) {
                Ok(true) => {
                    self.counted_runs
                        .failed
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    false
                }
                Ok(false) => self.scheduled_update_blocked(source, run),
                Err(why) => {
                    eprintln!("pw dev server: legacy scheduled quarantine failed: {why}");
                    true
                }
            };
        }
        let behalf = self.identity.principals().on_behalf(
            "clock",
            &format!("schedule-{}", run.id),
            &run.user,
        );
        let session = behalf.session().to_string();
        let result = self.command_scheduled(run, &session);
        let failed = match result {
            Ok(CommandOutcome::NotCurrent) => {
                self.counted_runs
                    .stale
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return false;
            }
            Ok(CommandOutcome::Answered(answer)) if answer.why.is_none() => {
                self.counted_runs
                    .ran
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return false; // Consumption already committed; never blindly delete a newer revision.
            }
            Ok(CommandOutcome::Answered(answer)) => answer
                .why
                .unwrap_or_else(|| "scheduled answer was refused".into()),
            Err(why) => why,
        };
        let failures = run.attempts.saturating_add(1);
        let terminal = run.attempts >= schedules::RETRIES;
        let due = if terminal {
            run.due_ms
        } else {
            self.now().saturating_add(schedules::backoff_ms(failures))
        };
        match source.retry_current(run, failures, due, terminal.then_some(failed.as_str())) {
            Ok(true) => {
                let counted = if terminal {
                    &self.counted_runs.failed
                } else {
                    &self.counted_runs.retried
                };
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if terminal {
                    eprintln!(
                        "pw dev server: scheduled command {} failed after {} attempts",
                        run.command, failures
                    );
                }
                false
            }
            Ok(false) => self.scheduled_update_blocked(source, run),
            Err(why) => {
                eprintln!("pw dev server: scheduled retry was not kept: {why}");
                true
            }
        }
    }

    /// Zero affected rows may mean a newer revision, or a suppressed write.
    /// Only an authoritative recheck can establish staleness; otherwise wait.
    fn scheduled_update_blocked(
        &self,
        source: &dyn schedules::Runs,
        run: &schedules::Candidate<'_>,
    ) -> bool {
        match source.current(run, self.now()) {
            Ok(false) => {
                self.counted_runs
                    .stale
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                false
            }
            Ok(true) => {
                eprintln!("pw dev server: scheduled persistence left its due generation unchanged");
                true
            }
            Err(why) => {
                eprintln!("pw dev server: scheduled update could not be reconciled: {why}");
                true
            }
        }
    }

    fn command_scheduled(
        &self,
        run: &schedules::Candidate<'_>,
        session: &str,
    ) -> Result<CommandOutcome, String> {
        let json = serde_json::from_str::<Vec<serde_json::Value>>(&run.args)
            .map_err(|_| "scheduled arguments do not read".to_string())?;
        let (export, args) = self.command_arguments(&run.command, &json)?;
        if export.invoked_by.as_deref() != Some("Clock") || export.idempotent_by.is_none() {
            return Err("scheduled target is not an idempotent Clock command".into());
        }
        let interaction = format!("schedule-{}", run.id);
        let key = format!("{session}\u{1f}{}\u{1f}{interaction}", run.command);
        let sent = serde_json::Value::Array(json).to_string();
        // One evaluator/commit core; no browser memoization/session history for Clock.
        let result = self.command_recorded_for(
            &run.command,
            session,
            &args,
            Some(&interaction),
            Some((&key, &sent)),
            Some(run.completing(self.now())),
        );
        let refused = identity::take_refusal();
        result.map(|outcome| match outcome {
            CommandOutcome::Answered(mut answer) => {
                answer.refused = refused;
                CommandOutcome::Answered(answer)
            }
            other => other,
        })
    }

    /// **Every other host's commits, heard and told** (ADR-0369, track
    /// `second-host`), for as long as the host runs: woken by a commit's
    /// notification, and each [`hosts::WAIT_MS`] without one. A layer in
    /// memory is one host by definition, and has nothing to hear.
    fn hear_other_hosts(&self) {
        let Some(tail) = self.data.tail() else {
            return;
        };
        loop {
            if !self.hear() {
                tail.trim();
                tail.wait(hosts::WAIT_MS);
            }
        }
    }

    /// **The other hosts' commits since this host's cursor, told** to each
    /// session of this host that reads what they dropped, as a commit of its
    /// own is told to the sessions other than its presser's (ADR-0219,
    /// ADR-0297): each commit in the order they were made, the cursor moved
    /// past it once it is told. Rows trimmed before this host read them tell
    /// each of its documents to reload: what they said can no longer be told.
    /// Whether more may be waiting.
    fn hear(&self) -> bool {
        let Some(tail) = self.data.tail() else {
            return false;
        };
        let heard = match tail.heard() {
            Ok(heard) => heard,
            Err(why) => {
                eprintln!("pw dev server: the other hosts' commits were not read: {why}");
                return false;
            }
        };
        if heard.missed {
            let documents: Vec<Doc> = self
                .pending
                .lock()
                .expect("pending")
                .keys()
                .cloned()
                .collect();
            for doc in &documents {
                self.unshowable(
                    doc,
                    "another host's commits were trimmed before this host read them",
                );
            }
        }
        for (last, (events, invalidated)) in &heard.commits {
            let stale = self.invalidate_queries("", invalidated, events);
            if !stale.is_empty() {
                self.telling
                    .lock()
                    .expect("telling")
                    .push((String::new(), stale));
            }
            tail.passed(*last);
        }
        tail.passed(heard.through);
        self.tell_waiting();
        heard.more
    }

    /// TRACK SEAM (second-host): **its pages numbered from `first_page`**,
    /// another host's, as a test that compares two hosts' pages to the byte
    /// has them: a page's number is its host's (ADR-0369), not its data's.
    /// Before it serves any.
    #[cfg(test)]
    fn numbered_from(&mut self, first_page: u64) {
        self.first_page = first_page;
        *self.documents.get_mut() = first_page;
    }

    /// **Each live document read again at the instant what it shows holds
    /// until** (ADR-0304), for as long as the host runs: woken when an
    /// instant is set or the clock is moved, and each second at least, as the
    /// clock moves with time.
    fn tell_when_time_passes(&self) {
        loop {
            let (told, wait) = self.tell_due();
            if told == 0 {
                self.timers.wait(wait);
            }
        }
    }

    /// Each document whose instant has come, read again and told: how many,
    /// and how long until the next, a second at most.
    fn tell_due(&self) -> (usize, u64) {
        let now = self.now();
        let (due, next) = self.timers.take_due(now);
        for doc in &due {
            self.time_passed(doc);
        }
        let wait = next.map_or(1000, |at| (at - now).clamp(1, 1000));
        (due.len(), u64::try_from(wait).unwrap_or(1000))
    }

    /// **`doc` shows what held until now**: read again, and told what
    /// changed, as a change of the session's would tell it, in the session's
    /// turn.
    fn time_passed(&self, doc: &Doc) {
        let session = doc.0.as_str();
        let session_lock = self.one_at_a_time(session);
        let _one = session_lock
            .lock()
            .expect("one change of a session at a time");
        self.clock.advance(1);
        let version = Version(self.clock.now());
        self.send_documents_where(
            session,
            |d| d == doc,
            &session_documents(session),
            version,
            false,
        );
    }

    fn sync_query_clock(&self) {
        let (clock, started, seen) = &self.query_clock;
        let now = started.elapsed().as_millis() as u64;
        let before = seen.swap(now, std::sync::atomic::Ordering::SeqCst);
        if now > before {
            clock.advance(now - before);
        }
    }

    /// **A binding's query, run by its declared policies** (ADR-0127). The
    /// key is the arguments its `key` names (all of them when it names none),
    /// and a private entry's key is scoped by the session, as `pw-resource`
    /// requires of its host.
    fn fetch_binding(
        &self,
        session: &str,
        binding: &serde_json::Value,
        args: &[Val],
    ) -> Result<Val, Unread> {
        let resource = binding["resource"].as_str().unwrap_or_default();
        // **A materialization's value, kept** (ADR-0277): read as a query's,
        // and answered from the materializer.
        if self.derives(resource) {
            return self.materialized(resource, args).map_err(Unread::Failed);
        }
        let answer = self.fetch_answer(session, binding, args)?;
        // A declared error the page says means it is not found (ADR-0163),
        // told from every other failure.
        if let Val::Result(Err(Some(e))) = &answer
            && let Val::Variant(case, _) = e.as_ref()
            && binding["not_found"]
                .as_array()
                .is_some_and(|cases| cases.iter().any(|c| c == case.as_str()))
        {
            return Err(Unread::NotFound(format!("{resource} answered {case}")));
        }
        // A declared error the page says means its address is another of
        // the page's (ADR-0295): what the case carries fills the route.
        if let Val::Result(Err(Some(e))) = &answer
            && let Val::Variant(case, carried) = e.as_ref()
            && binding["redirect"]["case"] == case.as_str()
        {
            let permanent = binding["redirect"]["permanent"] == true;
            return Err(match carried.as_deref() {
                Some(Val::String(to)) => Unread::Moved {
                    to: to.clone(),
                    permanent,
                },
                _ => Unread::Failed(format!("{resource} answered {case} with no address")),
            });
        }
        Ok(unwrapped(resource, answer)?)
    }

    /// **A query's answer, run by its declared policies** (ADR-0127): its
    /// whole result, `Ok` or `Err`, as a stream shows it (ADR-0148). A
    /// declared error is given to each reader waiting on the flight that
    /// answered it, and is not kept, as a host's failure is not: the next
    /// reader asks again.
    fn fetch_answer(
        &self,
        session: &str,
        binding: &serde_json::Value,
        args: &[Val],
    ) -> Result<Val, String> {
        let key = self.answer_key(session, binding, args)?;
        // Held while it runs (ADR-0152): `pw-resource` stops a flight when
        // its last holder lets go, and a keyed read's `cancel` is a holder
        // letting go. Every other reader holds it too, so a flight is stopped
        // only when nobody is waiting for it.
        let _held = self.queries.subscribe(&key);
        self.fetch_answer_at(session, binding, args, &key)
    }

    /// **The `pw-resource` key a binding's read of `args` runs under**: its
    /// entry's, or for `concurrency parallel` one of its own.
    fn answer_key(
        &self,
        session: &str,
        binding: &serde_json::Value,
        args: &[Val],
    ) -> Result<pw_resource::Key, String> {
        let resource = binding["resource"].as_str().unwrap_or_default();
        let policy = &binding["policy"];
        let mut key = entry_key(session, policy, args)
            .ok_or_else(|| format!("`{resource}`'s key names an argument it was not given"))?;
        if policy["parallel"] == true {
            // `concurrency parallel`: no shared flight, so a key of its own.
            let n = self
                .parallel
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            key.push_str(&format!("\u{1f}#{n}"));
        }
        Ok(pw_resource::Key::new(resource, &key))
    }

    /// [`Server::fetch_answer`], under a key the caller computed and holds.
    fn fetch_answer_at(
        &self,
        session: &str,
        binding: &serde_json::Value,
        args: &[Val],
        key: &pw_resource::Key,
    ) -> Result<Val, String> {
        let resource = binding["resource"].as_str().unwrap_or_default();
        let manifest = runtime_manifest(resource, &binding["policy"]);
        let key = key.clone();
        // **A value held until an instant now passed is read again**
        // (ADR-0304): what it compared the clock with answers otherwise.
        let passed = {
            let mut held = self.held.lock().expect("held");
            let passed = held.get(&key).is_some_and(|&until| until <= self.now());
            if passed {
                held.remove(&key);
            }
            passed
        };
        if passed {
            self.queries.invalidate_key(&key);
        }
        // A flight a commit invalidated is not an error: its result was about
        // to be stale, and a reader asks again (found 2026-10-02: two presses
        // at once made one command's re-read fail on the other's commit).
        //
        // Once more through `pw-resource`, then directly. Asking again
        // through it can be invalidated again by each commit that lands
        // during the read, and eight times was no bound on that: eight
        // threads committing on one session invalidated a reader eight times
        // in about one run of a hundred. A direct read starts after the
        // invalidation, so it is as new as the commit that caused it, and
        // nothing can take its result away.
        for _ in 0..2 {
            match self.fetch_once(session, resource, &manifest, &key, args) {
                pw_resource::Fetched::Cancelled(pw_resource::StopReason::Invalidated) => continue,
                pw_resource::Fetched::Fresh(v)
                | pw_resource::Fetched::FromCache(v)
                | pw_resource::Fetched::Deduplicated(v) => {
                    if matches!(*v, Val::Result(Err(_))) {
                        self.queries.invalidate_key(&key);
                    }
                    // What it holds until, for the read it is part of
                    // (ADR-0304): a kept value's, or another flight's.
                    if let Some(&until) = self.held.lock().expect("held").get(&key) {
                        clock::note(until);
                    }
                    return Ok(Val::clone(&v));
                }
                // The origin failed, and the query is public and declares
                // `fallback last_known_good` (ADR-0177): the last value kept.
                pw_resource::Fetched::LastKnownGood(v) => {
                    if std::env::var("PW_TRACE").is_ok() {
                        eprintln!("{resource}: its origin failed; its last known good value");
                    }
                    return Ok(Val::clone(&v));
                }
                pw_resource::Fetched::Failed(e) => return Err(format!("{resource}: {e}")),
                pw_resource::Fetched::Cancelled(r) => {
                    return Err(format!("{resource}: stopped, {r:?}"));
                }
                pw_resource::Fetched::TimedOut => return Err(format!("{resource}: timed out")),
            }
        }
        self.answer(resource, session, args)
    }

    /// One fetch of a binding's query through `pw-resource`.
    fn fetch_once(
        &self,
        session: &str,
        resource: &str,
        manifest: &pw_resource::Manifest,
        key: &pw_resource::Key,
        args: &[Val],
    ) -> pw_resource::Fetched<Arc<Val>> {
        self.sync_query_clock();
        let load = |_attempt, cancellation: &pw_resource::Cancellation<Arc<Val>>| {
            // What a slow source polls, to end early a read nobody is
            // waiting for any longer (ADR-0152).
            let cancellation = cancellation.clone();
            let stopped: Stopped = Arc::new(move || cancellation.is_cancelled());
            // What the value holds until (ADR-0304), kept with it before
            // it is published, so a reader its flight answers knows too.
            let (answer, until) =
                clock::noting(|| self.answer_within(resource, session, args, Some(stopped)));
            // A read held in flight once it has read, as a test asks (NEXT
            // 4c): taken first, so no other read waits on the hook while it
            // holds this one.
            #[cfg(test)]
            let hold = self.in_flight.lock().expect("in flight").take();
            #[cfg(test)]
            if let Some(hold) = hold {
                hold(self);
            }
            let mut held = self.held.lock().expect("held");
            match until {
                Some(until) => held.insert(key.clone(), until),
                None => held.remove(key),
            };
            answer.map(Arc::new)
        };
        // TRACK SEAM (render-merge): in a render, as of the invalidations
        // when it began (NEXT 4c, the integrator's ruling 3): where nothing
        // invalidated the query since, what is kept or in flight is what the
        // render's snapshot holds, as far as the program's invalidations say,
        // and is taken as the query's policy says; where something did, the
        // render reads it alone in its own snapshot, and never keeps or
        // hands another reader what may be older than the change.
        match render_merge::reading(&*self.data) {
            Some(render) => {
                self.queries
                    .fetch_cancellable_as_of(manifest, key, render.as_of(), load)
            }
            None => self.queries.fetch_cancellable(manifest, key, load),
        }
    }

    /// **Drop what a commit made stale** (ADR-0127): each entry a command
    /// invalidated, as it computed its key (ADR-0209), and each entry an
    /// emitted event reaches through a query's `invalidates_on`. A position
    /// a command writes `_`, or an event leaves unbound (ADR-0091), is every
    /// value there: the entries at the values given are dropped, whatever
    /// the rest (ADR-0256). Until ADR-0256 an event leaving one unbound
    /// dropped every entry of the query.
    ///
    /// **In every session's partition** (ADR-0256): a private entry is one
    /// session's copy, and what changed is every session's to read again.
    /// Until ADR-0256 the committing session's alone was dropped, and
    /// another's copy of a private query keyed by something else stayed.
    ///
    /// Returns **what it dropped that another session may hold** (ADR-0219):
    /// a shared query's entries, or a private query's that no key position
    /// pins to this session. Until ADR-0256 a private query's drop was told
    /// to no other session unless it was whole.
    fn invalidate_queries(
        &self,
        session: &str,
        invalidated: &[crate::data::Dropped],
        events: &[(String, Vec<Val>)],
    ) -> Vec<Reached> {
        // A query's policy, which keys its entries: read by a `let`, or by a
        // stream, whose answer is kept too (ADR-0148). Until 2026-10-03 only
        // a `let`'s was found, so an event never reached a stream's kept
        // answer (ADR-0165).
        // On any page: until 2026-10-04 only the store's was read, and a
        // query another page binds kept its stale answer (ADR-0190). A
        // query's policy is its own, wherever it is read. And the binding's
        // arguments, as its plan writes them (track `store-accounts`).
        let binding_of = |resource: &str| {
            std::iter::once(&self.plan)
                .chain(self.plans.values())
                .flat_map(|plan| {
                    ["bindings", "streams"]
                        .into_iter()
                        .flat_map(move |k| plan[k].as_array().into_iter().flatten())
                })
                .find(|b| b["resource"] == resource)
                .cloned()
        };
        // Each drop says whether another session may hold what it dropped:
        // a shared entry, or a private one no key position pins to this
        // session, by its id. An entry keyed by the session is its alone.
        // **And one keyed by a reader's handle is that user's** (track
        // `store-accounts`, telling by principal): the user at the key
        // position a binding fills with `current_user()`, where the drop
        // names one. ADR-0270's superset told every reader of the query.
        let drop_entries = |resource: &str, args: &[Option<Val>]| -> Option<Reached> {
            // No binding reads it, so nothing of it is kept.
            let binding = binding_of(resource)?;
            let policy = binding["policy"].clone();
            self.queries
                .invalidate_where(resource, |key| names_entry(&policy, args, key));
            let positions = key_positions(&policy);
            let pinned = positions
                .iter()
                .any(|i| matches!(args.get(*i), Some(Some(Val::String(s))) if s == session));
            if entry_is_private(&policy) && pinned {
                return None;
            }
            let user = positions
                .iter()
                .filter(|i| {
                    binding["args"][**i]
                        .as_str()
                        .is_some_and(notifications::is_current_user)
                })
                .find_map(|i| match args.get(*i) {
                    Some(Some(Val::String(user))) => Some(user.clone()),
                    _ => None,
                })
                .filter(|_| entry_is_private(&policy));
            Some(Reached {
                resource: resource.to_string(),
                user,
            })
        };
        let mut reached = Vec::new();
        // Until ADR-0209 the server read each key's text, `current_session()`
        // or else the whole query.
        for (query, values) in invalidated {
            reached.extend(drop_entries(query, values));
        }
        for (event, values) in events {
            for e in &self.graph.edges {
                if e.kind != pw_materialize::EdgeKind::InvalidatedBy || e.to != *event {
                    continue;
                }
                // The query's parameters, bound from the event's values by
                // the names its `invalidates_on` gives them: each as the
                // command computed it, so an `Int` keys as an `Int` does
                // (ADR-0208). Until then each was a `String`.
                let params = self
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.path == e.from)
                    .map(|n| n.params.clone())
                    .unwrap_or_default();
                let mut args = vec![None; params.len()];
                for (i, name) in e.key.iter().enumerate() {
                    if let (Some(p), Some(v)) =
                        (params.iter().position(|q| q == name), values.get(i))
                    {
                        args[p] = Some(v.clone());
                    }
                }
                reached.extend(drop_entries(&e.from, &args));
            }
        }
        // **And each kept materialization they reach, made again** (ADR-0277),
        // each document that reads it told when the telling runs.
        let made = self.regenerate_reached(events);
        self.made.lock().expect("made").extend(made);
        reached.sort();
        reached.dedup();
        reached
    }

    /// **Whether `resource` is a materialization that derives its value**
    /// (ADR-0277): one the graph names, whose body this build compiled.
    fn derives(&self, resource: &str) -> bool {
        self.graph
            .node(resource)
            .is_some_and(|n| n.node == "materialization")
            && self.components.contains_key(resource)
    }

    /// The materializer's key for `path`'s entry at `args`, each by its text
    /// in a key (ADR-0277).
    fn kept_key(path: &str, args: &[Val]) -> EntryKey {
        let texts: Vec<String> = args.iter().map(materializations::key_text).collect();
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        EntryKey::new(path, &texts)
    }

    /// **A materialization's value** (ADR-0277): its entry, current; made
    /// now where it has none or it is out of date, once for every reader
    /// asking at once; and where it could not be made, the last good one,
    /// where its `fallback` keeps it.
    fn materialized(&self, path: &str, args: &[Val]) -> Result<Val, String> {
        let key = Self::kept_key(path, args);
        if let pw_materialize::Read::Fresh(body) = self.materializer.read(&key) {
            return materializations::decode_text(&body);
        }
        self.regenerate_kept(path, args, None);
        match self.materializer.read(&key) {
            pw_materialize::Read::Fresh(body) | pw_materialize::Read::LastKnownGood(body) => {
                materializations::decode_text(&body)
            }
            _ => Err(format!(
                "`{path}` could not be derived, and keeps no last good value"
            )),
        }
    }

    /// **A materialization's entry made again** (ADR-0277): its value
    /// derived, encoded so it reads back whole, a new version. Kept among the
    /// entries an event may reach, by its arguments.
    fn regenerate_kept(
        &self,
        path: &str,
        args: &[Val],
        because: Option<i64>,
    ) -> pw_materialize::Regenerated {
        let key = Self::kept_key(path, args);
        let texts: Vec<String> = args.iter().map(materializations::key_text).collect();
        self.kept
            .lock()
            .expect("kept")
            .insert((path.to_string(), texts), args.to_vec());
        self.clock.advance(1);
        self.materializer.regenerate(&key, because, || {
            // A test's fault for the materialization (ADR-0277), by its path,
            // which holds until it is taken away: each making of it fails.
            if self
                .materializer_faults
                .lock()
                .expect("materializer faults")
                .contains(path)
            {
                return Err("the materializer failed".to_string());
            }
            let value = self.derive_value(path, args)?;
            materializations::encode_text(&value)
        })
    }

    /// **A materialization's value, derived now** (ADR-0277): its body run,
    /// each read it asks answered from what it reads. A body reads only by
    /// asking, and asks the same of the same answers, so where it asks one
    /// not yet answered the host answers it and runs the body again with what
    /// it has learned, until it asks nothing more: each read once, answered
    /// from what the resource is now.
    fn derive_value(&self, path: &str, args: &[Val]) -> Result<Val, String> {
        let contract = self
            .contracts
            .iter()
            .find(|c| c.component_id == path)
            .ok_or_else(|| format!("no contract for `{path}`"))?;
        // Each read the body may ask, by its import, and what it reads.
        let reads: BTreeMap<String, String> = contract
            .imports
            .iter()
            .filter_map(|i| Some((i.key(), i.reads.clone()?)))
            .collect();
        let answers: Arc<Mutex<BTreeMap<String, Val>>> = Arc::default();
        let asked: Arc<Mutex<materializations::Asked>> = Arc::default();
        // Each run learns one answer at least, and a body asks a bounded
        // number of questions: past this bound it is asking without end.
        for _ in 0..=64 {
            let mut host: BTreeMap<String, HostFn> = BTreeMap::new();
            for import in reads.keys() {
                let (answers, asked, import) = (answers.clone(), asked.clone(), import.clone());
                host.insert(
                    import.clone(),
                    Arc::new(move |given: &[Val]| {
                        let question = materializations::asked(&import, given)?;
                        if let Some(v) = answers.lock().expect("answers").get(&question) {
                            return Ok(vec![v.clone()]);
                        }
                        *asked.lock().expect("asked") = Some((import.clone(), given.to_vec()));
                        Err(format!("`{import}` is asked, and not yet answered"))
                    }),
                );
            }
            match self.run(path, "", &host, args) {
                Ok(out) => {
                    return out
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("`{path}` returned nothing"));
                }
                Err(e) => {
                    let Some((import, given)) = asked.lock().expect("asked").take() else {
                        return Err(e);
                    };
                    let resource = reads
                        .get(&import)
                        .ok_or_else(|| format!("`{import}` reads nothing this build knows"))?;
                    let value = self.read_value(resource, &given)?;
                    answers
                        .lock()
                        .expect("answers")
                        .insert(materializations::asked(&import, &given)?, value);
                }
            }
        }
        Err(format!("`{path}` asked more than 64 questions"))
    }

    /// **What a materialization's read answers** (ADR-0277): another's kept
    /// value, or a query's answer, its `Ok` value. A query's error answers
    /// none, and the regeneration fails, which `fallback` decides.
    fn read_value(&self, resource: &str, given: &[Val]) -> Result<Val, String> {
        if self.derives(resource) {
            return self.materialized(resource, given);
        }
        // Through the query's kept answer, where a page reads it and so its
        // policy is known: a change reads it once, for the page and for what
        // derives from it. One no page reads is read where it is.
        let policy = std::iter::once(&self.plan)
            .chain(self.plans.values())
            .flat_map(|plan| {
                ["bindings", "streams"]
                    .into_iter()
                    .flat_map(move |k| plan[k].as_array().into_iter().flatten())
            })
            .find(|b| b["resource"] == resource)
            .map(|b| b["policy"].clone());
        let answer = match policy {
            Some(policy) => self.fetch_answer(
                "",
                &serde_json::json!({ "resource": resource, "policy": policy }),
                given,
            )?,
            None => self.answer_within(resource, "", given, None)?,
        };
        match answer {
            Val::Result(Ok(Some(v))) => Ok(*v),
            Val::Result(Err(_)) => Err(format!("`{resource}` answered an error")),
            Val::Result(Ok(None)) => Err(format!("`{resource}` answered no value")),
            v => Ok(v),
        }
    }

    /// **Each kept materialization an event reaches, made again in its
    /// chain's order** (ADR-0277): by its own `invalidates_on`, or through
    /// what it reads (ADR-0102). Every one is out of date before any is
    /// made, and each is made after every one it reads, so none is built
    /// from one not yet made again. Each entry made with another value, in
    /// that order, for the documents that read it to be told.
    fn regenerate_reached(&self, events: &[(String, Vec<Val>)]) -> Vec<(String, Vec<Val>)> {
        let kept: Vec<(String, Vec<String>, Vec<Val>)> = self
            .kept
            .lock()
            .expect("kept")
            .iter()
            .map(|((path, key), args)| (path.clone(), key.clone(), args.clone()))
            .collect();
        let reached: Vec<(String, Vec<Val>)> = kept
            .into_iter()
            .filter(|(path, key, _)| {
                events.iter().any(|(event, values)| {
                    let values: Vec<String> =
                        values.iter().map(materializations::key_text).collect();
                    self.graph.reaches(path, event, &values, key)
                })
            })
            .map(|(path, _, args)| (path, args))
            .collect();
        for (path, args) in &reached {
            self.materializer.invalidate(&Self::kept_key(path, args), 0);
        }
        let paths: Vec<String> = reached.iter().map(|(p, _)| p.clone()).collect();
        let mut made = Vec::new();
        for path in self.graph.in_dependency_order(&paths) {
            for (_, args) in reached.iter().filter(|(p, _)| *p == path) {
                let key = Self::kept_key(&path, args);
                let was = self.materializer.entry(&key).map(|e| e.body);
                self.regenerate_kept(&path, args, None);
                // Told only where its value changed: a rename leaves the menu
                // counted as it was, and its readers are told nothing.
                if self.materializer.entry(&key).map(|e| e.body) != was {
                    made.push((path.clone(), args.clone()));
                }
            }
        }
        made
    }

    /// **Each open document that reads an entry made again, told**
    /// (ADR-0277): read again and sent what changed, it alone of its
    /// session's, since another of them may read the entry at another key,
    /// or none of it.
    fn tell_kept(&self, made: &[(String, Vec<Val>)]) {
        if made.is_empty() {
            return;
        }
        let open: Vec<Doc> = self
            .pending
            .lock()
            .expect("pending")
            .keys()
            .cloned()
            .collect();
        let mut readers: BTreeMap<String, std::collections::BTreeSet<u64>> = BTreeMap::new();
        for doc in open {
            let plan = self.plan_of(&self.page_of(&doc));
            let reads = plan["bindings"].as_array().into_iter().flatten().any(|b| {
                let resource = b["resource"].as_str().unwrap_or_default();
                made.iter().any(|(path, args)| {
                    path == resource
                        && self
                            .args_of(plan, &doc.0, Some(doc.1), b, &Keys::Shown)
                            .is_ok_and(|given| given == *args)
                })
            });
            if reads {
                readers.entry(doc.0.clone()).or_default().insert(doc.1);
            }
        }
        for (session, documents) in readers {
            let lock = self.one_at_a_time(&session);
            let _one = lock.lock().expect("one change of a session at a time");
            self.clock.advance(1);
            let version = Version(self.clock.now());
            self.send_documents_where(
                &session,
                |doc| documents.contains(&doc.1),
                &session_documents(&session),
                version,
                false,
            );
        }
    }

    /// **What commits dropped, told to every other session that reads it**
    /// (ADR-0219): run by a connection once its request is answered and
    /// closed. Whichever connection takes a commit's telling tells it; each
    /// session is read when it is told, so a session told late is told the
    /// latest. **Each session once**, for every commit that reached it
    /// (ADR-0271): until then each commit read every reader's documents
    /// again, so a burst of posts was a burst of renders for each reader,
    /// and a reader's own read waited behind them.
    fn tell_waiting(&self) {
        let waiting = std::mem::take(&mut *self.telling.lock().expect("telling"));
        let others: std::collections::BTreeSet<String> = waiting
            .iter()
            .flat_map(|(session, reached)| self.others_reading(session, reached))
            .collect();
        if !waiting.is_empty() {
            self.telling_note(format_args!(
                "took {} queued, from {:?}: telling {:?}",
                waiting.len(),
                waiting.iter().map(|(s, _)| masked(s)).collect::<Vec<_>>(),
                others.iter().map(|s| masked(s)).collect::<Vec<_>>()
            ));
        }
        // At once, each session in its own hold (ADR-0297): until then one
        // after another, and a reader whose session came late in the order
        // waited for every one before it.
        let others: Vec<String> = others.into_iter().collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let tellers = TELLERS.min(others.len());
        if tellers <= 1 {
            for other in &others {
                self.tell(other);
            }
        } else {
            std::thread::scope(|scope| {
                for _ in 0..tellers {
                    scope.spawn(|| {
                        while let Some(other) =
                            others.get(next.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
                        {
                            self.tell(other);
                        }
                    });
                }
            });
        }
        // And each document that reads an entry made again (ADR-0277).
        let made = std::mem::take(&mut *self.made.lock().expect("made"));
        self.tell_kept(&made);
    }

    /// **`other`'s open documents read again and sent what changed**
    /// (ADR-0219), in its own hold, taken after the committing session's is
    /// given up, so two sessions committing at once never wait on each
    /// other. **One telling of a session at a time** (ADR-0271): a commit
    /// that reaches it while one runs asks for one more after it, which
    /// reads the latest, and returns at once.
    fn tell(&self, other: &str) {
        {
            let mut told = self.told.lock().expect("told");
            if let Some(again) = told.get_mut(other) {
                *again = true;
                self.telling_note(format_args!(
                    "{}: a telling runs; one more after it",
                    masked(other)
                ));
                return;
            }
            told.insert(other.to_string(), false);
        }
        self.telling_note(format_args!("{}: telling", masked(other)));
        // A telling that panics must not silence the session for good: the
        // next commit finds it not being told. One that ends lets go of it
        // where it sees no commit came, in the same hold, so none is lost
        // between the two.
        struct Unwound<'a> {
            server: &'a Server,
            other: &'a str,
            ended: bool,
        }
        impl Drop for Unwound<'_> {
            fn drop(&mut self) {
                if !self.ended {
                    self.server.told.lock().expect("told").remove(self.other);
                }
            }
        }
        let mut unwound = Unwound {
            server: self,
            other,
            ended: false,
        };
        loop {
            {
                let lock = self.one_at_a_time(other);
                let _one = lock.lock().expect("one change of a session at a time");
                self.clock.advance(1);
                let version = Version(self.clock.now());
                self.send_documents(other, &session_documents(other), version, false);
            }
            // Taken out of its hold first: a hook that panics must not leave
            // it poisoned for the next telling.
            #[cfg(test)]
            {
                let then = self.told_rendered.lock().expect("told rendered").take();
                if let Some(then) = then {
                    then(self);
                }
            }
            let mut told = self.told.lock().expect("told");
            match told.get_mut(other) {
                Some(again) if *again => *again = false,
                _ => {
                    told.remove(other);
                    unwound.ended = true;
                    drop(told);
                    self.telling_note(format_args!("{}: told", masked(other)));
                    break;
                }
            }
        }
    }

    /// **The other sessions whose open documents read what a commit
    /// dropped** (ADR-0219): a post reaches every open timeline, not only
    /// its author's. Until ADR-0219 another reader saw a change when it
    /// next loaded the page.
    fn others_reading(&self, session: &str, reached: &[Reached]) -> Vec<String> {
        if reached.is_empty() {
            return Vec::new();
        }
        // **Telling by principal** (track `store-accounts`): what a private
        // entry keyed by a user reached is told to that user's sessions
        // alone; what any other reached, to every reader of it.
        let principals = self.identity.principals();
        let reaches = |other: &str| -> Vec<String> {
            reached
                .iter()
                .filter(|r| {
                    r.user
                        .as_deref()
                        .is_none_or(|user| notifications::user_of(&principals, other) == user)
                })
                .map(|r| r.resource.clone())
                .collect()
        };
        // The open documents, read before their pages: no two of these
        // tables are held at once.
        let open: Vec<Doc> = self
            .pending
            .lock()
            .expect("pending")
            .keys()
            .cloned()
            .collect();
        open.into_iter()
            .filter(|(s, _)| s != session)
            .filter(|doc| self.reads_any(&self.page_of(doc), &reaches(&doc.0)))
            .map(|(s, _)| s)
            .collect()
    }

    /// **Whether a page reads any of `resources`**, by a `let` or a stream
    /// its plan binds.
    fn reads_any(&self, page: &str, resources: &[String]) -> bool {
        let plan = self.plan_of(page);
        ["bindings", "streams"].into_iter().any(|k| {
            plan[k].as_array().into_iter().flatten().any(|b| {
                b["resource"]
                    .as_str()
                    .is_some_and(|r| resources.iter().any(|x| x == r))
            })
        })
    }

    /// **Run a query's compiled component** (ADR-0125), with the data layer,
    /// the session and the catalogue, and return its answer: its declared
    /// result, `Ok` or `Err`. Nothing a query does is committed: it reads.
    fn answer(&self, component_id: &str, session: &str, args: &[Val]) -> Result<Val, String> {
        self.answer_within(component_id, session, args, None)
    }

    /// [`Server::answer`], its slow sources told how to see that the read was
    /// stopped (ADR-0152).
    fn answer_within(
        &self,
        component_id: &str,
        session: &str,
        args: &[Val],
        stopped: Option<Stopped>,
    ) -> Result<Val, String> {
        // Each operation counted as it runs (`/metrics`).
        let counted = |name: &str, f: HostFn| -> HostFn {
            let (calls, name) = (self.calls.clone(), name.to_string());
            Arc::new(move |args: &[Val]| {
                *calls
                    .lock()
                    .expect("calls")
                    .entry(name.clone())
                    .or_default() += 1;
                f(args)
            })
        };
        // The store's reads (ADR-0218). TRACK SEAM (render-merge): in the
        // render this thread reads, where it reads one, every one reads the
        // render's snapshot, and a call the render made already is answered
        // as it was, and not run again (NEXT 4c).
        let render = render_merge::reading(&*self.data);
        let layer = match &render {
            Some(render) => render.reads(&*self.data, session, stopped),
            None => self.data.reads(session, stopped),
        };
        let mut host: BTreeMap<String, HostFn> = layer
            .into_iter()
            .map(|(name, f)| {
                let f = counted(&name, f);
                let f = match &render {
                    Some(render) => render.once(session, &name, f),
                    None => f,
                };
                (name, f)
            })
            .collect();
        // And the platform's session, each read its own: never merged.
        let mut platform: BTreeMap<String, HostFn> = BTreeMap::new();
        platform.insert(
            "pw:host/session#read".to_string(),
            Self::session_operation(session),
        );
        // TRACK SEAM (notifications): the reader's user, `current_user()`.
        platform.insert(
            notifications::PRINCIPAL_READ.to_string(),
            notifications::principal_operation(&self.identity.principals(), session),
        );
        // The platform's clock (ADR-0304): every comparison in one read
        // answers for one instant, and notes what the read holds until.
        platform.extend(clock::operations(self.now(), &self.zones));
        host.extend(platform.into_iter().map(|(name, f)| {
            let f = counted(&name, f);
            (name, f)
        }));
        let out = self.run(component_id, session, &host, args)?;
        out.into_iter()
            .next()
            .ok_or_else(|| format!("{component_id} returned nothing"))
    }

    /// **A query's value**: its answer's `Ok` value. A declared error is a
    /// failure to a reader that shows only a value (ADR-0147).
    fn query(&self, component_id: &str, session: &str, args: &[Val]) -> Result<Val, String> {
        unwrapped(component_id, self.answer(component_id, session, args)?)
    }

    /// **Each binding of the store page, by its query** (ADR-0125): a page
    /// parameter is what `document`'s address gave it (ADR-0162),
    /// `current_session()` is the request's session, and a signal's key is
    /// what `document` shows (ADR-0161).
    fn bindings(&self, session: &str, document: u64) -> Result<BTreeMap<String, Val>, String> {
        Ok(self.bindings_where(session, Some(document), |_| true, &Keys::Shown)?)
    }

    /// One binding's value, by its name, read for none of the session's
    /// documents: a binding no signal keys is every document's.
    fn binding(&self, session: &str, name: &str) -> Result<BTreeMap<String, Val>, String> {
        Ok(self.bindings_where(session, None, |b| b == name, &Keys::Shown)?)
    }

    /// The bindings whose names `wanted` takes, each run by its policies, a
    /// binding a signal keys read for the key `keys` says (ADR-0152), and
    /// what `document` shows (ADR-0161). The bindings are `document`'s page's
    /// (ADR-0190), or the store's for none.
    fn bindings_where(
        &self,
        session: &str,
        document: Option<u64>,
        wanted: impl Fn(&str) -> bool,
        keys: &Keys,
    ) -> Result<BTreeMap<String, Val>, Unread> {
        // TRACK SEAM (render-merge): one render, unless this thread reads one
        // already (NEXT 4c): its bindings read one snapshot, and each
        // identical call once.
        let _render = render_merge::begin(&*self.data, self.queries.epoch());
        let page = document.map_or_else(
            || self.store_page().to_string(),
            |d| self.page_of(&(session.to_string(), d)),
        );
        let plan = self.plan_of(&page);
        // What the document's values hold until (ADR-0304): the earliest
        // instant any of them compared the clock with, when it is read again.
        let (out, until) = clock::noting(|| {
            let mut out = BTreeMap::new();
            // **One query with one key is read once for its document**
            // (ADR-0303): a page and its layout that each bind `Cart` of the
            // session's read it once, and show one value of it.
            let mut read: Vec<(&str, Vec<Val>, Val)> = Vec::new();
            for b in plan["bindings"].as_array().into_iter().flatten() {
                if !wanted(b["binding"].as_str().unwrap_or_default()) {
                    continue;
                }
                let args = self.args_of(plan, session, document, b, keys)?;
                let resource = b["resource"].as_str().unwrap_or_default();
                let value = match read.iter().find(|(r, a, _)| *r == resource && *a == args) {
                    Some((_, _, v)) => v.clone(),
                    None => {
                        let v = self.fetch_binding(session, b, &args)?;
                        read.push((resource, args, v.clone()));
                        v
                    }
                };
                out.insert(b["binding"].as_str().unwrap_or_default().to_string(), value);
                #[cfg(test)]
                if let Some(between) = self.between_bindings.lock().expect("between").take() {
                    between(self);
                }
                #[cfg(test)]
                let reading = self.while_reading.lock().expect("while reading").clone();
                #[cfg(test)]
                if let Some(reading) = reading {
                    reading(self);
                }
            }
            Ok::<_, Unread>(out)
        });
        if let (Some(document), Some(until)) = (document, until) {
            self.timers.hold(&(session.to_string(), document), until);
        }
        // **What the page asks of its reader** (ADR-0307): each predicate's
        // answer for the document, as `requires` answered it when the
        // document was first read, read by the name its template reads it
        // by. The document's for its life, as the values it shows were read
        // for its reader then: a reader who signs out in another tab is told
        // at the next press (ADR-0302). Until 2026-10-11 each read answered
        // again, as `requires` would now, so a page re-rendered after a
        // sign-out elsewhere lost the controls of the regions that changed
        // and kept the rest, and a page of one reader's data could be given
        // another's answers.
        let mut out = out?;
        let asked: Vec<&str> = plan["predicates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str())
            .collect();
        if !asked.is_empty() {
            let kept = document.and_then(|n| {
                let answers = self.answers.lock().expect("answers");
                answers.get(&(session.to_string(), n)).cloned()
            });
            let held = match kept {
                Some(held) => held,
                None => {
                    let mut held = BTreeMap::new();
                    for predicate in &asked {
                        held.insert(
                            predicate.to_string(),
                            self.identity.answers(predicate, session)?,
                        );
                    }
                    if let Some(n) = document {
                        self.answers
                            .lock()
                            .expect("answers")
                            .insert((session.to_string(), n), held.clone());
                    }
                    held
                }
            };
            for (predicate, holds) in held {
                out.insert(asked_name(&predicate), Val::Bool(holds));
            }
        }
        Ok(out)
    }

    /// **A binding's arguments**, as a host computes them: a page parameter
    /// is what `document`'s address gave it (ADR-0162), `current_session()`
    /// the request's session, and a page signal the key `keys` says
    /// (ADR-0152), as `document` shows it (ADR-0161).
    fn args_of(
        &self,
        plan: &serde_json::Value,
        session: &str,
        document: Option<u64>,
        b: &serde_json::Value,
        keys: &Keys,
    ) -> Result<Vec<Val>, String> {
        let binding = b["binding"].as_str().unwrap_or_default();
        let signals: Vec<&str> = b["signals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str())
            .collect();
        b["args"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| match a.as_str() {
                // A page parameter, as the document's address gave it
                // (ADR-0162).
                Some(name)
                    if plan["params"]
                        .as_array()
                        .is_some_and(|ps| ps.iter().any(|p| p == name)) =>
                {
                    document
                        .and_then(|d| {
                            self.params
                                .lock()
                                .expect("params")
                                .get(&(session.to_string(), d))
                                .and_then(|p| param_val(plan, name, p))
                        })
                        .ok_or_else(|| format!("the page's `{name}` is given no value here"))
                }
                Some("current_session()") => Ok(Val::String(session.into())),
                // TRACK SEAM (notifications): the reader's user, from the
                // session's principal: never a value the browser sends.
                Some(arg) if notifications::is_current_user(arg) => Ok(Val::String(
                    notifications::user_of(&self.identity.principals(), session),
                )),
                Some(signal) if signals.contains(&signal) => {
                    let value = match keys {
                        Keys::Asked {
                            binding: asked,
                            key,
                        } if *asked == binding => key.get(signal).cloned(),
                        Keys::First => None,
                        Keys::Shown | Keys::Asked { .. } => document.and_then(|d| {
                            self.keyed
                                .lock()
                                .expect("keyed")
                                .get(&(session.to_string(), d))
                                .and_then(|k| k.reads.get(binding))
                                .and_then(|r| r.shown.get(signal).cloned())
                        }),
                    }
                    .unwrap_or_else(|| first_value(plan, signal));
                    key_val(&value).ok_or_else(|| format!("`{signal}` is no key: {value}"))
                }
                other => Err(format!("an argument this server cannot compute: {other:?}")),
            })
            .collect()
    }

    /// **A part's value**, by the steps the compiler planned: a record field,
    /// or a member function run as its own component. `None` where a case's
    /// payload is read from a value of another case: its arm is not shown.
    fn read_part(
        &self,
        bindings: &BTreeMap<String, Val>,
        part: &serde_json::Value,
    ) -> Result<Option<Val>, String> {
        let binding = part["binding"].as_str().unwrap_or_default();
        let value = bindings
            .get(binding)
            .cloned()
            .ok_or_else(|| format!("no binding `{binding}`"))?;
        self.follow(value, &part["steps"])
    }

    /// A part outside any arm: it always has a value.
    fn read_shown_part(
        &self,
        bindings: &BTreeMap<String, Val>,
        part: &serde_json::Value,
    ) -> Result<Val, String> {
        self.read_part(bindings, part)?
            .ok_or_else(|| format!("part {} reads an arm's payload", part["part"]))
    }

    /// **The value a part's steps lead to from `value`**: each a record field,
    /// a member function run as its own component (ADR-0125), or a case's
    /// payload (the integrator's, 2026-10-13), which a value of another case
    /// has none of.
    fn follow(&self, mut value: Val, steps: &serde_json::Value) -> Result<Option<Val>, String> {
        for step in steps.as_array().into_iter().flatten() {
            value = if let Some(field) = step["field"].as_str() {
                let wit = field.replace('_', "-");
                match &value {
                    Val::Record(fields) => fields
                        .iter()
                        .find(|(n, _)| *n == wit)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| format!("no field `{field}`"))?,
                    other => return Err(format!("`.{field}` read from {other:?}")),
                }
            } else if let Some(member) = step["member"].as_str() {
                let out = self.run(member, "", &BTreeMap::new(), &[value])?;
                out.into_iter()
                    .next()
                    .ok_or_else(|| format!("{member} returned nothing"))?
            } else if let Some(derived) = step["derived"].as_str() {
                // A computed part's function (ADR-0226), run as a member is:
                // pure, with the value it reads.
                let out = self.run(derived, "", &BTreeMap::new(), &[value])?;
                out.into_iter()
                    .next()
                    .ok_or_else(|| format!("{derived} returned nothing"))?
            } else if let Some(payload) = step.get("payload") {
                let case = payload["case"].as_str().unwrap_or_default();
                let field = payload["field"].as_u64().map(|i| i as usize);
                let Some(carried) = payload_of(&value, case, field)? else {
                    return Ok(None);
                };
                carried
            } else {
                return Err(format!("a step this server does not know: {step}"));
            };
        }
        Ok(Some(value))
    }

    /// **A binding's value as the renderer reads it** (ADR-0169): each row
    /// of a list with what it reads of its item through a member, which the
    /// member's component computes. The renderer reads the rest by field.
    fn rendered_value(
        &self,
        plan: &serde_json::Value,
        binding: &str,
        value: &Val,
    ) -> Result<Value, String> {
        let mut out = val_to_value(value);
        // Each list in the binding's value whose rows read through a member:
        // the value itself, or a list inside it, `cart.lines` (ADR-0170).
        let mut lists: BTreeMap<&str, Vec<&serde_json::Value>> = BTreeMap::new();
        for read in plan["rows"].as_array().into_iter().flatten() {
            let Some(collection) = read["collection"].as_str() else {
                continue;
            };
            if collection.split('.').next() == Some(binding) {
                lists.entry(collection).or_default().push(read);
            }
        }
        for (collection, reads) in lists {
            let fields: Vec<&str> = collection.split('.').skip(1).collect();
            self.rows_at(value, &mut out, &fields, &reads, collection)?;
        }
        Ok(out)
    }

    /// **Each row of the list at `fields`, with what its reads compute**
    /// (ADR-0170): `*` is each item of a list on the way, so a loop inside
    /// a loop, `menu.*.items`, has each of its rows' reads computed in each
    /// outer item (ADR-0181).
    fn rows_at(
        &self,
        value: &Val,
        out: &mut Value,
        fields: &[&str],
        reads: &[&serde_json::Value],
        collection: &str,
    ) -> Result<(), String> {
        let unlisted = || format!("`{collection}`'s rows are read, and it is no list");
        if let Some(at) = fields.iter().position(|f| *f == "*") {
            let (before, after) = (&fields[..at], &fields[at + 1..]);
            let (Some(Val::List(items)), Some(Value::List(rows))) =
                (val_at(value, before), value_at_mut(out, before))
            else {
                return Err(unlisted());
            };
            for (item, row) in items.iter().zip(rows.iter_mut()) {
                self.rows_at(item, row, after, reads, collection)?;
            }
            return Ok(());
        }
        let (Some(Val::List(items)), Some(Value::List(rows))) =
            (val_at(value, fields), value_at_mut(out, fields))
        else {
            return Err(unlisted());
        };
        for (item, row) in items.iter().zip(rows.iter_mut()) {
            let Value::Record(fields) = row else {
                return Err(format!("a row of `{collection}` is no record"));
            };
            for read in reads {
                let read_value = self.follow(item.clone(), &read["steps"])?.ok_or_else(|| {
                    format!("a row's read {} reads an arm's payload", read["path"])
                })?;
                // Set whole, by its path from the item's name:
                // `item.price.display` is `price.display` in the row, which
                // the renderer reads as one (ADR-0170). Until 2026-10-03 it
                // was set inside `price`, and a member of a number,
                // `quantity.count`, had nowhere to go.
                let path = read["path"].as_str().unwrap_or_default();
                let within = path.split_once('.').map_or("", |(_, rest)| rest);
                fields.insert(within.to_string(), val_to_value(&read_value));
            }
        }
        Ok(())
    }

    /// The text a part shows, found by its template path.
    fn part_text(&self, session: &str, path: &str) -> Result<String, String> {
        let part = self.plan["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["path"] == path)
            .ok_or_else(|| format!("the plan has no part `{path}`"))?
            .clone();
        // The part's own binding, and no other (ADR-0151): every binding
        // was read here, so a command asked each of the page's queries for
        // the cart's count, and a slow one held up every command.
        let bindings = self.binding(session, part["binding"].as_str().unwrap_or_default())?;
        Ok(
            match &val_to_value(&self.read_shown_part(&bindings, &part)?) {
                Value::Text(t) => t.clone(),
                Value::Int(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => return Err(format!("`{path}` has no text form: {other:?}")),
            },
        )
    }

    /// The lock a session's one change at a time holds (ADR-0172).
    fn one_at_a_time(&self, session: &str) -> Arc<Turns> {
        self.sessions
            .lock()
            .expect("sessions")
            .entry(session.to_string())
            .or_default()
            .clone()
    }

    /// Consume committed events and regenerate what they invalidate, as the
    /// session's one change at the time (ADR-0172).
    fn drain(&self, session: &str) {
        let session_lock = self.one_at_a_time(session);
        let _one = session_lock
            .lock()
            .expect("one change of a session at a time");
        self.drain_held(session);
    }

    /// [`Server::drain`], by a caller that holds the session's lock.
    fn drain_held(&self, session: &str) {
        // A data layer that keeps no entry for the session has none to
        // regenerate (ADR-0220): its commits send the session's documents at
        // the host's clock. Until ADR-0220 the store's cart was regenerated
        // for every program, and a feed's page, which has no cart, was told
        // to reload whenever it opened its stream.
        if !self.data.session_entry() {
            return;
        }
        let key = self.cart_key(session);
        let invalidated = self
            .materializer
            .drain(&self.graph, std::slice::from_ref(&key));
        // Nothing changed since the session's entry was made, and it is
        // current: nothing to read (ADR-0174). Until 2026-10-04 every
        // document read the cart here and then again to render it, and a
        // one-shot read error met this read, which no one sees, rather than
        // the page's. An entry whose regeneration failed is stale, and is
        // tried again here (ADR-0176): the materializer reports an entry as
        // invalidated only when it was current, so until then a session
        // whose regeneration failed was sent nothing again.
        let entry = self.materializer.entry(&key);
        let existing = entry.is_some();
        let stale = entry.as_ref().is_some_and(|e| e.stale);
        if existing && !stale && invalidated.is_empty() {
            return;
        }
        // TRACK SEAM (render-merge): one render, the count read here and the
        // session's documents read again below (NEXT 4c): the entry's value
        // and what its documents show are of one snapshot, and the cart
        // every page shows is read once. The documents it tells are taken
        // before it begins, and it ends once they are read, before they are
        // told: a render never waits for the table while it holds its seat
        // (the integrator's ruling 2).
        let told = existing.then(|| self.to_tell(session, |_| true));
        let render = render_merge::begin(&*self.data, self.queries.epoch());
        // The count the page shows, as the page computes it: the cart query's
        // value, read through `domain.line_count` (ADR-0125).
        let value = match self.part_text(session, "cart.line_count") {
            Ok(value) => value,
            Err(why) => {
                drop(render);
                return self.unshowable_session(session, &why);
            }
        };
        if std::env::var("PW_TRACE").is_ok() {
            eprintln!(
                "drain session={session} invalidated={} value={value} existing={existing}",
                invalidated.len(),
            );
        }

        // First materialization, or a regeneration the drain asked for.
        //
        // The clock advances first, because a version IS a clock reading:
        // `Entry.generated_at`. A clock that never moves gives every entry
        // version 0, and then the browser's staleness comparison compares
        // nothing while looking exactly like it works. Advancing here rather
        // than keeping a counter is what keeps the version the MATERIALIZER's.
        let because = invalidated.first().map(|(_, id)| *id);
        self.clock.advance(1);
        // Charter §15.5's materializer failure (ADR-0176), as a test arms it.
        let failing = self
            .materializer_faults
            .lock()
            .expect("materializer faults")
            .remove(session);
        let regenerated = self.materializer.regenerate(&key, because, || {
            if failing {
                Err("the materializer failed".to_string())
            } else {
                Ok(value.to_string())
            }
        });
        // A regeneration that failed sends nothing (ADR-0176). The entry
        // stays stale, the page keeps what it shows, and the session's next
        // drain, which its next subscription request makes, tries again.
        // Until 2026-10-04 its frames were sent at the version that had not
        // moved: the page ignored them as advancing nothing, while the
        // server took it to show them, and its next patch left it a line
        // short.
        if regenerated == pw_materialize::Regenerated::Failed {
            if std::env::var("PW_TRACE").is_ok() {
                eprintln!("drain session={session}: the regeneration failed; tried again next");
            }
            return;
        }

        // Frames only for an INVALIDATION, now or one whose regeneration
        // failed before (ADR-0176). The first materialization is the value
        // the document was rendered with, and a frame for it would tell the
        // browser to replace a part with what it already shows — harmless
        // here, and wrong in general because it spends a version the page then
        // treats as the newest it has seen.
        let Some(told) = told else {
            return;
        };
        self.tell_in(
            session,
            told,
            render,
            &self.cart_entry(session),
            self.version(session),
            true,
        );
    }

    /// **Each of a session's documents, read again and sent what changed**
    /// (ADR-0145, ADR-0161), against `entry` at `version`: the cart's, after
    /// a command (`drain_held`), or another resource's, after the store
    /// changed it (ADR-0193). The cart's value goes with it to a page that
    /// speculates on it, when the change is the cart's (`speculated`).
    fn send_documents(
        &self,
        session: &str,
        entry: &ResourceEntryId,
        version: Version,
        speculated: bool,
    ) {
        self.send_documents_where(session, |_| true, entry, version, speculated);
    }

    /// [`Server::send_documents`], of the session's documents `which` takes
    /// (ADR-0277).
    fn send_documents_where(
        &self,
        session: &str,
        which: impl Fn(&Doc) -> bool,
        entry: &ResourceEntryId,
        version: Version,
        speculated: bool,
    ) {
        // TRACK SEAM (render-merge): the documents told, taken first; then
        // read in one render, which ends before they are told (NEXT 4c).
        let told = self.to_tell(session, which);
        let render = render_merge::begin(&*self.data, self.queries.epoch());
        self.tell_in(session, told, render, entry, version, speculated);
    }

    /// **The documents a change is told to** (ADR-0297): of the session's,
    /// those `which` takes whose page asks, and those it passes by, which no
    /// page has asked for in [`LIVE`] and are derived when one asks; and the
    /// interactions the values read next include (ADR-0172), read before
    /// them: one committed in between is named by its own change, which
    /// follows at once, and never by a value it is not in. Taken before the
    /// render that reads them begins (NEXT 4c): a render never waits for the
    /// table while it holds its seat.
    fn to_tell(&self, session: &str, which: impl Fn(&Doc) -> bool) -> Told {
        let applied: Vec<String> = self
            .applied
            .lock()
            .expect("applied")
            .get(session)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default();
        let queue = self.pending.lock().expect("pending");
        let now = std::time::Instant::now();
        let (live, passed): (Vec<Doc>, Vec<Doc>) = documents_of(&queue, session)
            .into_iter()
            .filter(|d| which(d) && queue.contains_key(d))
            .partition(|d| queue.get(d).is_some_and(|waiting| waiting.live(now)));
        Told {
            applied,
            live,
            passed,
        }
    }

    /// **`told`'s documents read in `render`, and told what changed**
    /// (ADR-0145, ADR-0161), against `entry` at `version`: what each of the
    /// session's pages shows now, from its queries, each read for its own
    /// document, outside the table. `render` ends once they are read,
    /// whoever began it, before its thread waits for the table (NEXT 4c, the
    /// integrator's ruling 2); a document that could not be read is said so
    /// after it.
    fn tell_in(
        &self,
        session: &str,
        told: Told,
        render: render_merge::Rendering,
        entry: &ResourceEntryId,
        version: Version,
        speculated: bool,
    ) {
        let Told {
            applied,
            live,
            passed,
        } = told;
        let mut read = Vec::new();
        let mut unread = Vec::new();
        for doc in live {
            let params = self.params_of(&doc);
            // Each by its own page's plan (ADR-0190): the store's, or a cart
            // page's, which reads the same cart.
            let page = self.page_of(&doc);
            let now = self.bindings(session, doc.1).and_then(|bindings| {
                Ok((self.showing(&page, session, &params, &bindings)?, bindings))
            });
            match now {
                Ok((now, bindings)) => read.push((doc, page, params, bindings, now)),
                Err(why) => unread.push((doc, why)),
            }
        }
        drop(render);
        for (doc, why) in unread {
            self.unshowable(&doc, &why);
        }
        // Only a document whose page asks is derived now (ADR-0297); one no
        // page has asked for in [`LIVE`] is derived when one asks.
        if !passed.is_empty() {
            let mut queue = self.pending.lock().expect("pending");
            for doc in &passed {
                if let Some(waiting) = queue.get_mut(doc)
                    && !waiting.stale
                {
                    waiting.stale = true;
                    waiting.note(format_args!(
                        "passed by, no page asking: derived when one asks"
                    ));
                }
            }
        }

        // Two frames per change: the entry advanced, and every place in the
        // document the server DERIVED from it, as one set (ADR-0145). A
        // subscription and a patch are logically separate — a server may
        // derive a patch from a change rather than must.
        let mut failed = Vec::new();
        for (doc, page, params, bindings, now) in read {
            // And the value itself, to a page that speculates on it
            // (ADR-0122), from the snapshot the patches are derived from.
            // To each page that speculates on it, by its own manifest
            // (ADR-0191).
            // Until 2026-10-03 it was read again after the table was let go,
            // and pushed in a second hold: a page could be sent one change in
            // two batches, and a command committed in between gave the frame
            // a value later than its version.
            let value = self
                .speculates_on_cart(&page)
                .filter(|_| speculated)
                .and_then(|binding| bindings.get(&binding))
                .map(val_to_json);
            let now = Arc::new(now);
            // **Derived outside the table** (ADR-0296): against what the
            // document shows, read in a moment of its own, and pushed only
            // if the document still shows it; else derived again. Until
            // ADR-0296 every document of the session was derived inside one
            // hold of the table, 50 to 200 ms a telling under parallel load,
            // back to back, and every stream and read on the host waited for
            // it: WebKit's "Load more" saw its stream's passes wait up to 1.6 s
            // each, and its read's rows never came. The last attempt is
            // derived inside, so a document every telling changes is still
            // told.
            let mut attempt = 1;
            loop {
                let last = attempt >= DOCUMENT_ATTEMPTS;
                let deriving = std::time::Instant::now();
                let ahead = (!last).then(|| {
                    let was = self.shown.lock().expect("shown").get(&doc).cloned();
                    let derived = was.as_ref().map(|was| {
                        #[cfg(test)]
                        self.deriving(&doc);
                        self.derive(&page, session, &params, &bindings, was, &now)
                    });
                    (was, derived)
                });
                let derived_ms = deriving.elapsed().as_millis();
                let asked = std::time::Instant::now();
                let mut queue = self.pending.lock().expect("pending");
                let waited_ms = asked.elapsed().as_millis();
                let _held = self.held("telling a session's documents");
                // Against what the document shows. With none served, none
                // shows. And what changed of the values it speculates on
                // (ADR-0222), in this hold.
                let mut shown = self.shown.lock().expect("shown");
                let current = shown.get(&doc).cloned();
                let derived = match ahead {
                    Some((was, derived)) if same_shown(&was, &current) => derived,
                    // Something reached the document while this was derived.
                    Some(_) => {
                        drop(shown);
                        drop(queue);
                        attempt += 1;
                        continue;
                    }
                    None => current
                        .as_ref()
                        .map(|was| self.derive(&page, session, &params, &bindings, was, &now)),
                };
                let (patches, speculated) = match (derived, &current) {
                    (Some(Ok(patches)), Some(was)) => {
                        let speculated = self.speculated_frames(&doc, &page, was, &now, &applied);
                        shown.insert(doc.clone(), now.clone());
                        (patches, speculated)
                    }
                    (Some(Err(why)), _) => {
                        failed.push((doc.clone(), why));
                        break;
                    }
                    _ => (Vec::new(), Vec::new()),
                };
                drop(shown);
                // A document forgotten while it was read is told nothing.
                let Some(waiting) = queue.get_mut(&doc) else {
                    break;
                };
                waiting.note(format_args!(
                    "told at {}: derived in {derived_ms} ms, waited {waited_ms} ms for the table, attempt {attempt}",
                    version.0
                ));
                waiting.push(StreamFrame::ResourceChanged {
                    protocol: CURRENT,
                    entry: entry.clone(),
                    version,
                });
                // Sent with no patches too: the document reflects the new
                // version when nothing it shows changed.
                waiting.push(StreamFrame::PatchSet(PatchSet {
                    protocol: CURRENT,
                    basis: CausalBasis::of(entry.clone(), version),
                    patches,
                }));
                // In the same hold, so the value a page holds is always the
                // one the patches it was sent were derived from.
                for frame in speculated {
                    waiting.push(frame);
                }
                // In the same hold, so one change reaches a page whole.
                if let Some(value) = value {
                    waiting.push(StreamFrame::EntryValue {
                        protocol: CURRENT,
                        entry: entry.clone(),
                        version,
                        value,
                        applied: applied.clone(),
                    });
                }
                break;
            }
        }
        for (doc, why) in failed {
            self.unshowable(&doc, &why);
        }
    }

    // --- E7-P: keyed collections -------------------------------------
    //
    // Architect ruling, 2026-08-07:
    //
    // > DOM identity should survive moves. A `MoveInstance` that deletes and
    // > recreates the node is not equivalent.
    //
    // Everything below therefore derives ONE thing on the server — which
    // instance, and what its markup is — and sends it. The browser does not
    // re-render, and it does not diff: it has no template and no previous
    // model to diff against.

    /// The menu's loop part, from the compiler's manifest.
    fn menu_part(&self) -> (&Template, LocalPartId) {
        let template = self
            .templates
            .iter()
            .find(|t| t.name == "StorePage")
            .expect("StorePage");
        let part = template
            .manifest()
            .into_iter()
            .find(|p| p.kind == "each")
            .expect("the store page has a keyed loop");
        (template, LocalPartId(part.id.0))
    }

    fn menu_address(&self) -> PartAddress {
        let (template, part) = self.menu_part();
        PartAddress::new(&TemplateSchemaId(template.schema.clone()), part)
    }

    /// The identity domain a session's document was rendered in.
    ///
    /// Derived HERE and used by both the full render and every patch, so a
    /// token in a patch is the token already in the page. Two derivations
    /// would agree until one of them changed.
    fn domain(&self, session: &str) -> IdentityDomain {
        IdentityDomain::document(
            "StorePage(47)",
            Partition::Session {
                id: session.to_string(),
            },
            BUILD,
        )
        .keyed("own-renderer-spike-key")
    }

    /// **The identity domain a session's document of `page` is rendered in**
    /// (ADR-0190): the store's as before, and any other page's by its path,
    /// as a page of signals is (ADR-0130).
    fn domain_of(&self, page: &str, session: &str) -> IdentityDomain {
        if page == self.store_page() {
            return self.domain(session);
        }
        IdentityDomain::document(
            page,
            Partition::Session {
                id: session.to_string(),
            },
            BUILD,
        )
        .keyed("own-renderer-spike-key")
    }

    /// The menu fragment's OWN identity domain — public, and the same for
    /// every reader.
    ///
    /// Not the document's. `store.page.Menu` is declared `public … cache
    /// shared`: one answer for everybody. If its instance tokens came from the
    /// enclosing document they would be per-session, and two readers of one
    /// shared cache entry would get different bytes — which is the exact
    /// composition `IdentityDomain` exists to make unrepresentable, arriving
    /// through the back door of "the page renders the fragment".
    fn fragment_domain(&self, store: &str) -> IdentityDomain {
        IdentityDomain::document(
            &format!("store.page.Menu({store})"),
            Partition::Public,
            BUILD,
        )
        .keyed("own-renderer-spike-key")
    }

    /// The fragment's render environment.
    ///
    /// Takes the items rather than locking for them. Locking here put the menu
    /// mutex on two points of one call path — the page render and the fragment
    /// render beneath it — and a non-reentrant mutex against itself is a hang,
    /// not an error. The lock is taken once, by whoever is about to use the
    /// list, and passed down.
    ///
    /// It must NOT set the materialized fragment either: that is what the PAGE
    /// splices in, and putting it here made `menu_env` call `menu_fragment`
    /// call `menu_env` — unbounded recursion that presented as a server which
    /// accepted connections and answered none.
    fn menu_env(&self, store: &str, rows: &Value) -> Env {
        Env::new()
            .set("menu", rows.clone())
            .in_domain(self.fragment_domain(store))
    }

    /// **A store's menu rows, as its page renders them** (ADR-0169): the
    /// `Menu` query's value, run through its component, each row with what it
    /// reads through a member. Until 2026-10-03 E7-P built each row from an
    /// id and a name, and a row read nothing else of its item.
    fn menu_rows(&self, store: &str) -> Result<Value, String> {
        // Read as the page reads it, through the query's kept answer: what
        // a page shows is what it holds.
        let binding = self.plan["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| b["binding"] == "menu")
            .cloned()
            .ok_or_else(|| "the store's page binds no menu".to_string())?;
        let menu = self.fetch_binding("", &binding, &[Val::String(store.to_string())])?;
        self.rendered_value(&self.plan, "menu", &menu)
    }

    /// The menu fragment's bytes, materialized once and reused, and the rows
    /// they show.
    ///
    /// Stored in the materializer under the PUBLIC entry key. Two readers get
    /// the same bytes because they are the same bytes — the entry is read,
    /// not re-rendered — and that is what makes `cache shared` a fact about
    /// the system rather than a claim about two renders agreeing.
    ///
    /// What a new document shows of the menu is what the store's open pages
    /// show. A change to it reaches them first (`refresh_menu`, ADR-0178),
    /// and is never made here, where they would not hear of it: until
    /// 2026-10-04 a document whose `Menu` read differed rendered the
    /// fragment again, and the pages open kept the version before (ADR-0150).
    /// `rows` is the query's value, which the fragment is first rendered
    /// from.
    fn menu_fragment(&self, store: &str, rows: &Value) -> (String, Value) {
        let key = self.menu_key(store);
        let mut rendered_from = self.menu_rendered_from.lock().expect("rendered from");
        let from = rendered_from.get(store).cloned();
        if let Some(entry) = self.materializer.entry(&key)
            && !entry.stale
            && let Some(from) = &from
        {
            return (entry.body, from.clone());
        }
        // Rendered first, or again where its entry was dropped: from what the
        // pages show, where a page shows it.
        let from = from.unwrap_or_else(|| rows.clone());
        let (template, part) = self.menu_part();
        let env = self.menu_env(store, &from);
        let html = pw_render::render_part(template, part, &env, &self.templates)
            .expect("the menu fragment renders");
        self.clock.advance(1);
        self.materializer.invalidate(&key, 0);
        self.materializer
            .regenerate(&key, None, || Ok(html.clone()));
        rendered_from.insert(store.to_string(), from.clone());
        (html, from)
    }

    /// The public entry whose version every patch to a store's menu is
    /// caused by.
    fn menu_key(&self, store: &str) -> EntryKey {
        EntryKey::from_identity(&menu_identity(store))
    }

    fn menu_version(&self, store: &str) -> Version {
        Version(
            self.materializer
                .entry(&self.menu_key(store))
                .map(|e| e.generated_at)
                .unwrap_or(0),
        )
    }

    /// **A change to store 47's menu, told to every page that shows it**
    /// (E7-P): the change applied, the fragment rendered again as a new
    /// version, and one patch set for every reader, since a public fragment
    /// has one identity and so one set of instance tokens.
    ///
    /// The patches are the whole difference between what the open pages show
    /// and the menu now (ADR-0178): the change's own structural patch, where
    /// it moves the list, as E7-P makes it; then every row that differs from
    /// what the pages show, set where it is or rendered again where it is,
    /// as a session's list is (ADR-0145, ADR-0168). Until 2026-10-04 a change
    /// sent its own patch alone, and anything that had changed at the
    /// source unannounced was drawn into the fragment and never reached a
    /// page already open.
    fn broadcast_menu(&self, op: MenuOp) -> Result<(), String> {
        // A departed visitor is forgotten before the fan-out, so a change is
        // not queued for pages nobody is reading.
        self.forget_idle_subscribers();
        // The subscriber table is taken FIRST and held throughout.
        //
        // It is what serializes a change against a document being served: a
        // page rendered from the new list must not also receive the patch that
        // produced it, and a page rendered from the old list must receive it.
        // Without the interlock both orders are possible and the second one
        // applies a change twice — an item inserted, then inserted again.
        let mut queue = self.pending.lock().expect("pending");
        // The menu's rows before the change, as the open pages show them:
        // what the fragment was rendered from (ADR-0169), so what a renamed
        // item was (ADR-0168). With no fragment yet, as the query holds them.
        let shown = self
            .menu_rendered_from
            .lock()
            .expect("rendered from")
            .get(STORE_ID)
            .cloned();
        let before = match shown {
            Some(rows) => rows,
            None => self.menu_rows(STORE_ID)?,
        };
        // The deployment's menu changed, which is `MenuChanged(47)`: it drops
        // what the program says depends on it, the entries of each query
        // that declares `invalidates_on MenuChanged(id)` for this store, and
        // no other store's (§15.6 test 11, ADR-0164). Until 2026-10-03 every
        // store's kept `Menu` was dropped here, by the query's name.
        // A stock change is `InventoryChanged(47, item)` (ADR-0178), which
        // the store's `Menu` and its fragment listen for.
        // The store's ids are `String`s (`domain.pw`), as they key here.
        let event = match &op {
            MenuOp::Stock { id, .. } => (
                "Events.InventoryChanged".to_string(),
                vec![Val::String(STORE_ID.into()), Val::String(id.clone())],
            ),
            _ => (
                "Events.MenuChanged".to_string(),
                vec![Val::String(STORE_ID.into())],
            ),
        };
        // **The change, written through the store's data layer with its
        // event, in one transaction** (track `store-pg`), as a command's
        // writes are. Refusals happen before anything is regenerated: a
        // rejected operation commits nothing and leaves no version behind,
        // or the page would be told to catch up to a change that did not
        // happen. Until 2026-10-08 the change was made to the server's own
        // list, and its event was committed nowhere.
        let mut args = vec![Val::String(STORE_ID.into())];
        args.extend(op.to_vals());
        self.store_write(
            "",
            "store:host/menus#change",
            &args,
            std::slice::from_ref(&event),
            |delivered| {
                let reached = self.invalidate_queries("", &[], delivered);
                // TRACK SEAM (store-search), found by W9, a host fix: what the
                // change dropped is told to each session reading it (ADR-0219).
                // The store's pages are told by broadcast_menu's own patches;
                // reading them again finds nothing changed; excluded to save
                // that read, not for correctness
                // (a_menu_change_tells_the_stores_page_once holds with or
                // without it).
                self.telling.lock().expect("telling").push((
                    String::new(),
                    reached
                        .into_iter()
                        .filter(|r| {
                            !self.reads_any(self.store_page(), std::slice::from_ref(&r.resource))
                        })
                        .collect(),
                ));
            },
        )?;

        // The menu E7-P changes is store 47's (ADR-0162), read again with
        // the change.
        let items = self.menu_rows(STORE_ID)?;
        let (template, _) = self.menu_part();
        let env = self.menu_env(STORE_ID, &items);
        // The whole difference between what the open pages show and the menu
        // now (ADR-0178), derived from the two values as a session's list's
        // is (ADR-0145): an insert, a removal, a move, a rename, a stock
        // change, and anything changed at the source unannounced, each in
        // its category's list, every row whose key stayed keeping its nodes
        // (ADR-0181). Until 2026-10-04 an insert, a removal and a move sent a
        // patch E7-P made for its own operation, which a menu grouped by
        // category has nowhere to address.
        let patches = list_patches(
            template,
            &self.menu_address().template,
            "menu",
            rows(&before),
            rows(&items),
            &env,
            &self.templates,
        )?;
        if std::env::var("PW_TRACE").is_ok() {
            eprintln!("broadcast {:?} to {} document(s)", op, queue.len());
        }
        let told = self.tell_menu(&mut queue, STORE_ID, &items, patches);
        // **Each document that reads an entry the change made again**
        // (ADR-0277), once the subscriber table is given up: the menu's own
        // pages are sent its patches above, as ever.
        drop(queue);
        let made = std::mem::take(&mut *self.made.lock().expect("made"));
        self.tell_kept(&made);
        told
    }

    /// **A store's menu fragment rendered again from `items`, and the pages
    /// that show it told**, in the subscriber table the caller holds: the
    /// fragment, regenerated once in its own domain, is a new version, and
    /// `patches` take each page of the store from what it showed to it.
    /// Nothing is sent where nothing a page shows changed.
    fn tell_menu(
        &self,
        queue: &mut BTreeMap<Doc, Subscriber>,
        store: &str,
        items: &Value,
        patches: Vec<Targeted>,
    ) -> Result<(), String> {
        let (template, part) = self.menu_part();
        let env = self.menu_env(store, items);
        let html = pw_render::render_part(template, part, &env, &self.templates)
            .map_err(|b| format!("{b:?}"))?;
        self.clock.advance(1);
        // Invalidated first: `regenerate` leaves a current entry alone, so
        // without this the fragment kept its first rendering and every
        // document served after a change showed the old menu, while pages
        // already open were patched (found 2026-10-02, ADR-0127).
        self.materializer.invalidate(&self.menu_key(store), 0);
        self.materializer
            .regenerate(&self.menu_key(store), None, || Ok(html));
        self.menu_rendered_from
            .lock()
            .expect("rendered from")
            .insert(store.to_string(), items.clone());
        if patches.is_empty() {
            return Ok(());
        }
        let entry = ResourceEntryId::derive(&menu_identity(store), &IDENTITY);
        let version = self.menu_version(store);
        // To each document that shows the store (§15.6 test 11): another
        // store's menu is not this one's.
        let params = self.params.lock().expect("params");
        // A document of another page, an order's with `id` 47, shows no menu
        // (ADR-0190).
        let pages = self.pages.lock().expect("pages");
        let readers = |doc: &Doc| {
            pages
                .get(doc)
                .is_none_or(|page| page.as_str() == self.store_page())
                && params
                    .get(doc)
                    .is_some_and(|p| p.get("id").map(String::as_str) == Some(store))
        };
        for (_, waiting) in queue.iter_mut().filter(|(doc, _)| readers(doc)) {
            waiting.push(StreamFrame::ResourceChanged {
                protocol: CURRENT,
                entry: entry.clone(),
                version,
            });
            // One patch alone, or the set that applies whole (ADR-0145).
            waiting.push(match patches.as_slice() {
                [only] => StreamFrame::Patch(Patch {
                    protocol: CURRENT,
                    basis: CausalBasis::of(entry.clone(), version),
                    target: only.target.clone(),
                    operation: only.operation.clone(),
                }),
                _ => StreamFrame::PatchSet(PatchSet {
                    protocol: CURRENT,
                    basis: CausalBasis::of(entry.clone(), version),
                    patches: patches.clone(),
                }),
            });
        }
        Ok(())
    }

    /// **A store's menu fragment brought to its query's value, and the pages
    /// that show it told** (ADR-0178), before a document of the store is
    /// read, so the document shows what they do.
    ///
    /// ADR-0150 rendered the fragment again for a new document whose `Menu`
    /// read differed from what it was rendered from, and told no page open:
    /// no event had announced the change. But the fragment is one version
    /// for every reader, and the pages kept the one before, while every
    /// change after was derived from the new one. A rename then left an
    /// item's Add with its old name on those pages, and a sold-out item's
    /// Add on them for good.
    ///
    /// The query's value is read outside the subscriber table, as a
    /// document's is (ADR-0151). A change told meanwhile was read after this
    /// one, and is what the pages show then; this one stands down.
    fn refresh_menu(&self, store: &str) -> Result<(), String> {
        let seen = self
            .menu_rendered_from
            .lock()
            .expect("rendered from")
            .get(store)
            .cloned();
        // Never rendered: no page shows it, and the document renders it.
        let Some(seen) = seen else { return Ok(()) };
        let items = self.menu_rows(store)?;
        if items == seen {
            return Ok(());
        }
        self.forget_idle_subscribers();
        let mut queue = self.pending.lock().expect("pending");
        let now_shown = self
            .menu_rendered_from
            .lock()
            .expect("rendered from")
            .get(store)
            .cloned();
        if now_shown.as_ref() != Some(&seen) {
            return Ok(());
        }
        let (template, _) = self.menu_part();
        let env = self.menu_env(store, &items);
        let patches = list_patches(
            template,
            &self.menu_address().template,
            "menu",
            rows(&seen),
            rows(&items),
            &env,
            &self.templates,
        )?;
        self.tell_menu(&mut queue, store, &items, patches)
    }

    /// The document, and an empty queue for whoever receives it.
    ///
    /// A freshly rendered document already reflects every change committed
    /// before it was rendered. Delivering the patches for those changes as
    /// well would apply them twice — and the second application is not a
    /// no-op, because inserting an item is not idempotent. A reload is the
    /// ordinary way to reach that state, so this is not an edge case.
    ///
    /// Clearing rather than replaying is right because a patch is a
    /// TRANSITION. It is only meaningful against the state it was derived
    /// from, and this document was not derived from that state.
    ///
    /// Returns the cursor the document should subscribe from, so a reloaded
    /// page does not ask for frames the reload already made meaningless.
    #[cfg(test)]
    fn serve_document(&self, session: &str) -> (String, u64) {
        let (rendered, cursor, _) = self
            .serve_document_with_entries(session)
            .unwrap_or_else(|e| panic!("{e}"));
        (rendered, cursor)
    }

    /// The document, its cursor, and the values of the entries it speculates
    /// on (ADR-0122), all read under one hold of the subscriber table, so the
    /// values are the ones the rendered parts show.
    #[cfg(test)]
    fn serve_document_with_entries(
        &self,
        session: &str,
    ) -> Result<(String, u64, serde_json::Value), String> {
        self.serve_document_settled(session, self.store_page(), &store_params(STORE_ID), &[])
            .map(|(html, cursor, entries, _)| (html, cursor, entries))
            .map_err(String::from)
    }

    /// [`Server::serve_document`], for the store `id` (ADR-0162), or why it
    /// could not be read (ADR-0163).
    #[cfg(test)]
    fn serve_store_document(&self, session: &str, id: &str) -> Result<(String, u64), Unread> {
        self.serve_document_settled(session, self.store_page(), &store_params(id), &[])
            .map(|(html, cursor, _, _)| (html, cursor))
    }

    /// The document as [`Server::serve_document_with_entries`] serves it,
    /// with what its streams' queries settled to before it was written
    /// (ADR-0148), and the environment it was rendered in, which renders
    /// each streamed region's arm when its query settles.
    fn serve_document_settled(
        &self,
        session: &str,
        page: &str,
        params: &Params,
        settled: &[(u32, Settled)],
    ) -> Result<(String, u64, serde_json::Value, Env), Unread> {
        self.drain(session);
        self.forget_idle_subscribers();
        // The store's menu brought to its query's value first, and the pages
        // that show it told (ADR-0178), before this document is a reader:
        // it is then rendered from what they show. Another page shows no
        // menu fragment (ADR-0190).
        if page == self.store_page()
            && let Some(store) = params.get("id")
            && let Err(why) = self.refresh_menu(store)
            && std::env::var("PW_TRACE").is_ok()
        {
            eprintln!("the menu of {store} was not brought up to date: {why}");
        }
        // The document's values are read, and it is rendered, OUTSIDE the
        // subscriber table (ADR-0151). Until 2026-10-03 the table was held
        // throughout, so a page waiting for a slow query kept every other
        // page waiting, and every command's frames; and two pages read at
        // once never shared a query's flight, whatever its `concurrency`
        // said. The table is held only to install the document.
        // The document's number first (ADR-0161), and its subscriber with
        // it, so a change that reaches the session while it is read reaches
        // it too.
        let doc: Doc = (
            session.to_string(),
            self.documents
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        // Its parameters with it (ADR-0162): which store it reads. And its
        // page (ADR-0190).
        self.params
            .lock()
            .expect("params")
            .insert(doc.clone(), params.clone());
        self.pages
            .lock()
            .expect("pages")
            .insert(doc.clone(), page.to_string());
        let served = (|| {
            for _ in 1..DOCUMENT_ATTEMPTS {
                let pushed = self.subscribed(&doc);
                let read = self.render_document(&doc, settled)?;
                let mut queue = self.pending.lock().expect("pending");
                if let Some(served) = self.installed(&mut queue, &doc, read, Some(pushed)) {
                    return Ok(served);
                }
            }
            // The last attempt is read inside the table, as every document
            // was before: nothing can reach the session between the read and
            // the install, so a page is always served.
            let mut queue = self.pending.lock().expect("pending");
            let _held = self.held("a document's last attempt, read in the table");
            let read = self.render_document(&doc, settled)?;
            Ok(self
                .installed(&mut queue, &doc, read, None)
                .expect("nothing reaches a session inside the table"))
        })();
        // A document that could not be read is no page, and waits for
        // nothing.
        if served.is_err() {
            self.pending.lock().expect("pending").remove(&doc);
            self.params.lock().expect("params").remove(&doc);
            self.pages.lock().expect("pages").remove(&doc);
        }
        served
    }

    /// **The document's subscriber, before it is read** (ADR-0151,
    /// ADR-0161), and how many frames have reached it. Registered first, so a
    /// change that reaches the session while its document is read is pushed
    /// to it and counted, even for a session's first page.
    /// What a test does while `doc`'s change is derived outside the table.
    #[cfg(test)]
    fn deriving(&self, doc: &Doc) {
        let hook = self.deriving_hook.lock().expect("deriving").clone();
        if let Some(hook) = hook {
            hook(self, doc);
        }
    }

    /// **A document a telling passed by, told as its page asks**
    /// (ADR-0297): derived against what it shows, in the session's hold, as
    /// a telling would have, before the page is given its frames.
    fn told_as_it_asks(&self, doc: &Doc) {
        let stale = self
            .pending
            .lock()
            .expect("pending")
            .get_mut(doc)
            .is_some_and(|waiting| {
                // Asking now: live for the telling below.
                waiting.seen = std::time::Instant::now();
                std::mem::take(&mut waiting.stale)
            });
        if !stale {
            return;
        }
        let session = &doc.0;
        let lock = self.one_at_a_time(session);
        let _one = lock.lock().expect("one change of a session at a time");
        self.clock.advance(1);
        let version = Version(self.clock.now());
        self.send_documents_where(
            session,
            |d| d == doc,
            &session_documents(session),
            version,
            false,
        );
    }

    /// A note on [`Server::tellings`], the oldest let go past 300.
    fn telling_note(&self, what: std::fmt::Arguments<'_>) {
        let mut tellings = self.tellings.lock().expect("tellings");
        if tellings.len() >= 300 {
            tellings.pop_front();
        }
        tellings.push_back(format!("{} ms {what}", uptime_ms()));
    }

    /// A hold of the table at `site`, from now, timed ([`Held`]).
    fn held(&self, site: &'static str) -> Held<'_> {
        Held {
            server: self,
            site,
            at: std::time::Instant::now(),
        }
    }

    fn subscribed(&self, doc: &Doc) -> u64 {
        let mut queue = self.pending.lock().expect("pending");
        let waiting = queue
            .entry(doc.clone())
            .or_insert_with(|| Subscriber::at(doc.1));
        waiting.seen = Subscriber::asked_at();
        waiting.pushed
    }

    /// **A document read, installed** in the table the caller holds: what
    /// was waiting is cleared, the document takes its cursor, and what it
    /// shows is recorded with it, so a change is sent as the difference
    /// (ADR-0145). `None` when a frame reached the session after `pushed`:
    /// the document may show a value from before that change, and clearing
    /// the change's frames would lose it, so it is read again.
    fn installed(
        &self,
        queue: &mut BTreeMap<Doc, Subscriber>,
        doc: &Doc,
        (html, shown, env): (String, Shown, Env),
        pushed: Option<u64>,
    ) -> Option<(String, u64, serde_json::Value, Env)> {
        let waiting = queue
            .entry(doc.clone())
            .or_insert_with(|| Subscriber::at(doc.1));
        if pushed.is_some_and(|pushed| waiting.pushed != pushed) {
            return None;
        }
        // What reached it while an attempt before this one was read: this
        // read includes it.
        waiting.frames.clear();
        waiting.behind = false;
        waiting.seen = Subscriber::asked_at();
        // What it speculates on, as it was read (ADR-0222).
        let entries = self.speculated_entries(doc, &self.page_of(doc), &shown);
        self.shown
            .lock()
            .expect("shown")
            .insert(doc.clone(), Arc::new(shown));
        // Its keyed reads start with it (ADR-0152).
        self.keyed
            .lock()
            .expect("keyed")
            .insert(doc.clone(), Keyed::default());
        // Its cursor is its number, never zero.
        Some((html, doc.1, entries, env))
    }

    /// **The cart, as the page's speculation module decodes it** (ADR-0122):
    /// the WIT `domain-cart` record by its Pleris field names, as
    /// `cart_value` builds it for a component. Each line is priced at its
    /// item's price, as the data layer prices it (ADR-0169).
    fn cart_json(&self, session: &str) -> serde_json::Value {
        // The cart query's value, by its Pleris field names (ADR-0125).
        let cart = self
            .query(
                "store.page.Cart",
                session,
                &[Val::String(self.owner(session))],
            )
            .unwrap_or_else(|e| panic!("the cart query failed: {e}"));
        val_to_json(&cart)
    }

    /// Does `page` speculate on the session's cart? Read from its manifest:
    /// a binding of `store.page.Cart` keyed by `current_session()`, the one
    /// key this server computes, as it computes an event's (ADR-0104).
    fn speculates_on_cart(&self, page: &str) -> Option<String> {
        self.speculations.get(page)?["bindings"]
            .as_array()?
            .iter()
            .find(|b| {
                b["resource"] == "store.page.Cart"
                    && (b["key"] == serde_json::json!(["current_session()"])
                        // Track `store-accounts`: or by its reader's handle.
                        || b["key"] == serde_json::json!(["current_user()"]))
            })
            .and_then(|b| b["binding"].as_str().map(str::to_string))
    }

    /// **Each binding `page` speculates on, but the store's cart**
    /// (ADR-0222), by its manifest: a value of any query the page shows by
    /// the speculation's key. The cart's is the materializer's entry, kept
    /// as before. Until ADR-0222 no other was sent, and a page that
    /// speculated on one could not show it.
    fn speculated_bindings(&self, page: &str) -> Vec<String> {
        let cart = self.speculates_on_cart(page);
        self.speculations
            .get(page)
            .and_then(|m| m["bindings"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|b| b["binding"].as_str().map(str::to_string))
            .filter(|b| cart.as_deref() != Some(b.as_str()))
            .collect()
    }

    /// **The frames that tell a document what changed of the values its page
    /// speculates on** (ADR-0222), from `was` to `now`: each at a version
    /// read from the clock in the caller's hold of the subscriber table, so
    /// a document is sent its values in the order its versions say. Each
    /// one sent is recorded, for a commit's answer to name.
    fn speculated_frames(
        &self,
        doc: &Doc,
        page: &str,
        was: &Shown,
        now: &Shown,
        applied: &[String],
    ) -> Vec<StreamFrame> {
        let mut frames = Vec::new();
        let params = self.params_of(doc);
        for (binding, value) in &now.speculated {
            if was.speculated.get(binding) == Some(value) {
                continue;
            }
            self.clock.advance(1);
            let version = Version(self.clock.now());
            let route = self.speculated_route(page, binding, &params);
            self.speculated_versions
                .lock()
                .expect("speculated versions")
                .insert(
                    (
                        doc.0.clone(),
                        page.to_string(),
                        binding.clone(),
                        route.clone(),
                    ),
                    version,
                );
            frames.push(StreamFrame::EntryValue {
                protocol: CURRENT,
                entry: speculated_entry(&doc.0, page, binding, &route),
                version,
                value: value.clone(),
                applied: applied.to_vec(),
            });
        }
        frames
    }

    /// **The page's parameters a speculated binding's key reads** (ADR-0236),
    /// by their values in a document: what names its entry beside the
    /// session. None for a key of invocation-context calls and signals.
    fn speculated_route(&self, page: &str, binding: &str, params: &Params) -> Vec<String> {
        self.speculations
            .get(page)
            .and_then(|m| m["bindings"].as_array())
            .into_iter()
            .flatten()
            .find(|b| b["binding"] == binding)
            .and_then(|b| b["key"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|k| params.get(k.as_str()?).cloned())
            .collect()
    }

    /// Each speculated binding's entry, version and value, for the document
    /// `doc` of `page` that shows `shown`. Read in the subscriber table's
    /// hold.
    fn speculated_entries(&self, doc: &Doc, page: &str, shown: &Shown) -> serde_json::Value {
        let session = doc.0.as_str();
        let params = self.params_of(doc);
        let mut out = serde_json::Map::new();
        for (binding, value) in &shown.speculated {
            let route = self.speculated_route(page, binding, &params);
            out.insert(
                binding.clone(),
                serde_json::json!({
                    "entry": speculated_entry(session, page, binding, &route),
                    "version": Version(self.clock.now()),
                    "value": value,
                }),
            );
        }
        if let Some(binding) = self.speculates_on_cart(page) {
            out.insert(
                binding,
                serde_json::json!({
                    "entry": self.cart_entry(session),
                    "version": self.version(session),
                    "value": self.cart_json(session),
                }),
            );
        }
        serde_json::Value::Object(out)
    }

    /// The versions a committed command's writes produced (ADR-0122): what a
    /// page reconciles a speculation against.
    fn committed_basis(&self, session: &str) -> serde_json::Value {
        // A layer with no session entry (ADR-0222): each value the session's
        // pages speculate on, at the version it was last sent. A commit sends
        // its value before it is answered, so the page waits for that one.
        if !self.data.session_entry() {
            let sent = self
                .speculated_versions
                .lock()
                .expect("speculated versions");
            let basis: Vec<serde_json::Value> = sent
                .iter()
                .filter(|((s, ..), _)| s == session)
                .map(|((s, page, binding, route), version)| {
                    serde_json::json!({
                        "entry": speculated_entry(s, page, binding, route),
                        "version": version,
                    })
                })
                .collect();
            return serde_json::Value::Array(basis);
        }
        // Where the entry's regeneration failed (ADR-0176), the version it
        // was tried at, which any regeneration after it passes: the page
        // keeps its speculation until the value that includes the commit
        // comes. The version that had not moved would tell it that the value
        // it holds includes the commit, which it does not.
        let stale = self
            .materializer
            .entry(&self.cart_key(session))
            .is_some_and(|e| e.stale);
        let version = if stale {
            Version(self.clock.now())
        } else {
            self.version(session)
        };
        serde_json::json!([{ "entry": self.cart_entry(session), "version": version }])
    }

    /// Every handler identity this build's templates name.
    ///
    /// Answered from the compiler's template IR rather than from a list here.
    /// A list would be a second answer to "which handlers exist", and the
    /// first thing it would do is disagree.
    fn handler_identities(&self) -> std::collections::BTreeSet<String> {
        self.templates
            .iter()
            .flat_map(|t| t.manifest())
            .filter(|p| p.kind == "event" && !p.value.is_empty())
            .map(|p| p.value.clone())
            .collect()
    }

    /// **Each handler this build compiled, with the capture schema its
    /// document presents** (ADR-0132): the identity, and the paths it
    /// captures joined, which is empty for a handler that captures nothing.
    /// What `/pw-handlers` tells the browser's resume decision.
    fn handler_table(&self) -> BTreeMap<String, String> {
        handler_table(&self.templates)
    }

    /// **A page's parameters, as its address gives them** (ADR-0130): each
    /// one text. A page whose parameter is not given is not rendered with a
    /// guess.
    fn page_params(&self, path: &str, query: &str) -> Result<BTreeMap<String, String>, String> {
        let plan = self
            .plans
            .get(path)
            .ok_or_else(|| format!("no page `{path}` in this build"))?;
        let mut out = BTreeMap::new();
        for p in plan["params"].as_array().into_iter().flatten() {
            let name = p.as_str().unwrap_or_default();
            let given = query
                .split('&')
                .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
                .and_then(percent_decoded)
                .ok_or_else(|| format!("`{path}` is given no `{name}`"))?;
            out.insert(name.to_string(), given);
        }
        Ok(out)
    }

    /// **A page whose values are its signals, its parameters and its
    /// streams** (ADR-0130, ADR-0136, ADR-0148): rendered at each signal's
    /// first value, as `pw build`'s plan states it, each parameter as the
    /// address gives it, `?item=cortado`, and each stream as its query settled
    /// or pending. The document, and the environment it was rendered in. A
    /// page that binds a query with `let` is the store's route's.
    fn render_page(
        &self,
        path: &str,
        params: &BTreeMap<String, String>,
        session: &str,
        settled: &[(u32, Settled)],
    ) -> Result<(String, Env, &Template), String> {
        let plan = self
            .plans
            .get(path)
            .ok_or_else(|| format!("no page `{path}` in this build"))?;
        if plan["bindings"].as_array().is_some_and(|b| !b.is_empty()) {
            return Err(format!(
                "`{path}` binds a query; this route renders a page's signals and streams"
            ));
        }
        let template = self
            .templates
            .iter()
            .find(|t| t.path == path)
            .ok_or_else(|| format!("no template `{path}`"))?;
        let mut env = Env::new();
        // Each as its address gave it: a query's `Some` or `None`
        // (ADR-0309), a path's its text.
        for name in plan["params"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str())
        {
            if let Some(v) = param_val(plan, name, params) {
                env = env.set(name, val_to_value(&v));
            }
        }
        for (part, outcome) in settled {
            env = env.settle(PartId(*part), outcome.clone());
        }
        // What it computes from its signals, at their first values (ADR-0227).
        let env = self.with_computed_signals(env, plan, template)?;
        let env = with_signals(env, plan).in_domain(
            IdentityDomain::document(
                path,
                Partition::Session {
                    id: session.to_string(),
                },
                BUILD,
            )
            .keyed("own-renderer-spike-key"),
        );
        let body = pw_render::render(template, &env, &self.templates)
            .map_err(|e| format!("`{path}` does not render: {e:?}"))?;
        // The page's own title, or its name where it states none (ADR-0183),
        // and its metadata (ADR-0186).
        let title = pw_render::title_text(template, &env)
            .map_err(|e| format!("`{path}`'s title does not render: {e:?}"))?
            .unwrap_or_else(|| template.name.clone());
        let metadata = pw_render::head_metadata(template, &env)
            .map_err(|e| format!("`{path}`'s metadata does not render: {e:?}"))?;
        // And the address it names its own (ADR-0311).
        let metadata = metadata
            + &pw_render::head_canonical(template, &env, self.origin.get().map(String::as_str))
                .map_err(|e| format!("`{path}`'s canonical address does not render: {e:?}"))?;
        Ok((
            signal_document(
                &body,
                &title,
                &metadata,
                template,
                plan,
                &self.templates,
                &self.build_id,
            ),
            env,
            template,
        ))
    }

    /// Where `pw emit-handlers` wrote the module for a handler identity.
    fn handler_module(&self, identity: &str) -> std::path::PathBuf {
        self.artifacts
            .join("handlers")
            .join(format!("{identity}.mjs"))
    }

    /// Handler identities the document names and no compiled module exists
    /// for. The server refuses to start while there are any: a document whose
    /// buttons cannot load their code is a build that did not finish.
    fn uncompiled_handlers(&self) -> Vec<String> {
        self.handler_identities()
            .into_iter()
            .filter(|id| !self.handler_module(id).is_file())
            .collect()
    }

    #[cfg(test)]
    fn render_store(&self, session: &str) -> String {
        self.render_store_showing(session)
            .unwrap_or_else(|e| panic!("{e}"))
            .0
    }

    /// **The store's document, and what it shows** (ADR-0145), or why it
    /// cannot be rendered: a query that failed is a page that cannot be
    /// shown, answered as such, never a server that stops (E14, T04).
    #[cfg(test)]
    fn render_store_showing(&self, session: &str) -> Result<(String, Shown), String> {
        // A document of the default store, read and not served.
        let doc: Doc = (
            session.to_string(),
            self.documents
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        self.params
            .lock()
            .expect("params")
            .insert(doc.clone(), store_params(STORE_ID));
        let read = self.render_document(&doc, &[]);
        self.params.lock().expect("params").remove(&doc);
        read.map(|(html, shown, _)| (html, shown))
            .map_err(String::from)
    }

    /// **The page a path names by its route**, and the parameters the path
    /// gives it (ADR-0162): among the pages this build planned with one.
    /// One route is one page's (PW0341), so the first is the only one.
    fn routed(&self, path: &str, query: &str) -> Option<Result<(String, Params), String>> {
        // ADR-0309: malformed path text is a bad request before matching,
        // so a failed decode cannot masquerade as a missing page.
        if path
            .split('/')
            .any(|segment| path_decoded(segment).is_none())
        {
            return Some(Err(format!(
                "`{path}`'s path is no text its route can read"
            )));
        }
        self.plans.iter().find_map(|(page, plan)| {
            let route = plan["route"].as_str()?;
            let mut params = route_params(route_path(route), path)?;
            // And the names its query part gives (ADR-0309), as the address
            // says them.
            Some(match query_params(route, query) {
                Some(given) => {
                    params.extend(given);
                    // TRACK SEAM (argument-invariants): each held to what
                    // its type states, before anything reads it.
                    held_params(plan, &params).map(|()| (page.clone(), params))
                }
                None => Err(format!("`{path}`'s query is no text its route can read")),
            })
        })
    }

    /// **Whether a page this build serves states its canonical address**
    /// (ADR-0311): such a host is not started without a public origin.
    fn states_canonical(&self) -> bool {
        self.templates.iter().any(|t| {
            t.chunks.iter().any(|c| {
                matches!(
                    c,
                    pw_render::Chunk::Dynamic(pw_render::Part::Canonical { .. })
                )
            })
        })
    }

    /// **The store's page's path**, the page this server was built around:
    /// what a document not given another page shows (ADR-0190).
    fn store_page(&self) -> &str {
        self.plan["page"].as_str().unwrap_or_default()
    }

    /// **A page's plan, by its path** (ADR-0190): the store's where this
    /// build planned no such page, as a server made for one page's tests
    /// plans no other.
    fn plan_of(&self, page: &str) -> &serde_json::Value {
        self.plans.get(page).unwrap_or(&self.plan)
    }

    /// **What a page computes from its signals, at their first values**
    /// (ADR-0227): each by the path the compiler names it, computed by its
    /// component from the signal's first value. The browser computes it from
    /// then on, with the same function compiled for it.
    fn with_computed_signals(
        &self,
        mut env: Env,
        plan: &serde_json::Value,
        template: &Template,
    ) -> Result<Env, String> {
        for live in plan["live"].as_array().into_iter().flatten() {
            let Some(id) = live["derived"].as_str() else {
                continue;
            };
            let part = live["part"].as_u64().unwrap_or_default() as u32;
            // A text part, an attribute, or a block's subject (ADR-0229); or
            // a list the browser computes (track `device-store`, ADR-0366).
            let path = match find_part(&template.chunks, part) {
                Some(
                    pw_render::ir::Part::Text { value, .. }
                    | pw_render::ir::Part::Attribute { value, .. }
                    | pw_render::ir::Part::BooleanAttribute { value, .. }
                    | pw_render::ir::Part::Conditional { value, .. }
                    | pw_render::ir::Part::Match { value, .. },
                ) => value.clone(),
                Some(pw_render::ir::Part::Each { collection, .. }) => collection.clone(),
                _ => return Err(format!("part {part} reads no value")),
            };
            // Each value it reads, at its first: one at `path`, or several
            // (track `device-store`), in order.
            let paths: Vec<&str> = match live["paths"].as_array() {
                Some(several) if !several.is_empty() => {
                    several.iter().filter_map(|p| p.as_str()).collect()
                }
                _ => vec![live["path"].as_str().unwrap_or_default()],
            };
            let firsts: Vec<serde_json::Value> = paths
                .iter()
                .map(|p| {
                    let mut at = p.split('.');
                    let mut first = first_value(plan, at.next().unwrap_or_default());
                    for field in at {
                        first = first[field].clone();
                    }
                    first
                })
                .collect();
            let value = self.computed_from(id, &firsts)?;
            env = env.set(&path, val_to_value(&value));
        }
        Ok(env)
    }

    /// **A computed value's component, run with `json`** (ADR-0227), typed
    /// by its parameter as a command's argument is: pure, with what it reads.
    fn computed_from(&self, id: &str, inputs: &[serde_json::Value]) -> Result<Val, String> {
        let loaded = self
            .components
            .get(id)
            .ok_or_else(|| format!("no compiled component for `{id}`"))?;
        let export = self
            .contracts
            .iter()
            .find(|c| c.component_id == id)
            .and_then(|c| c.exports.first())
            .and_then(|e| e.component.clone())
            .ok_or_else(|| format!("`{id}`'s contract does not locate its export"))?;
        let args = loaded.prepared.arguments_for(&export, inputs)?;
        self.run(id, "", &BTreeMap::new(), &args)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("{id} returned nothing"))
    }

    /// **A page's template, by its path** (ADR-0190): the store's where this
    /// build has no such template.
    fn template_of(&self, page: &str) -> &Template {
        self.templates
            .iter()
            .find(|t| t.path == page)
            .unwrap_or_else(|| self.store_template())
    }

    /// **The page a document shows** (ADR-0190): as it was served, or the
    /// store's.
    fn page_of(&self, doc: &Doc) -> String {
        self.pages
            .lock()
            .expect("pages")
            .get(doc)
            .cloned()
            .unwrap_or_else(|| self.store_page().to_string())
    }

    /// **Whether a page binds a query** with `let` (ADR-0190): its values are
    /// then read, kept current and patched as the store's page's are. One
    /// that binds none is a page of signals and streams (ADR-0130).
    fn binds_a_query(&self, page: &str) -> bool {
        self.plans
            .get(page)
            .and_then(|plan| plan["bindings"].as_array())
            .is_some_and(|b| !b.is_empty())
    }

    /// **A document's parameters** (ADR-0162), as its address gave them:
    /// what each render of it reads (ADR-0231), and which store the store's
    /// page shows.
    fn params_of(&self, doc: &Doc) -> Params {
        self.params
            .lock()
            .expect("params")
            .get(doc)
            .cloned()
            .unwrap_or_default()
    }

    /// A document, what it shows, and the environment it was rendered in,
    /// with what its streams' queries settled to (ADR-0148): the store's, or
    /// any page that binds a query, by its own plan and template (ADR-0190).
    /// A streamed region not given is rendered pending, with its placeholder.
    fn render_document(
        &self,
        doc: &Doc,
        settled: &[(u32, Settled)],
    ) -> Result<(String, Shown, Env), Unread> {
        let session = doc.0.as_str();
        let params = self.params_of(doc);
        let page = self.page_of(doc);
        let template = self.template_of(&page);
        // Every value is a query's (ADR-0125): each binding runs its compiled
        // component, and each part reads the binding's value by the steps the
        // compiler planned. The store's menu's items are the `Menu` query's.
        // A new document's keys are its signals' first values, as the
        // browser holds them when it loads (ADR-0152).
        let bindings = self
            .bindings_where(session, Some(doc.1), |_| true, &Keys::First)
            .map_err(|e| e.of(&format!("`{}`'s queries", template.name)))?;
        let shown = self
            .showing(&page, session, &params, &bindings)
            .map_err(|e| format!("`{}`'s values: {e}", template.name))?;
        let mut env = self.document_env(&page, session, &params, &bindings);
        for (part, outcome) in settled {
            env = env.settle(PartId(*part), outcome.clone());
        }
        let html = pw_render::render(template, &env, &self.templates)
            .map_err(|b| format!("`{}` does not render: {b:?}", template.name))?;
        Ok((html, shown, env))
    }

    /// **A binding read again, for the key the browser asks for**
    /// (ADR-0152). `seq` numbers the browser's reads of the binding, and
    /// `document` is the cursor of the page that asks. The read is applied
    /// only if it is the latest for a page that is still the session's:
    /// what its page shows is derived from it, and sent as one patch set in
    /// the session's frames, in order with every other change. Under
    /// `on_key_change cancel` a newer read lets go of this one's flight, and
    /// so does `left`, which says whether the browser has gone.
    fn read_keyed(
        &self,
        session: &str,
        binding: &str,
        seq: u64,
        document: u64,
        key: &BTreeMap<String, serde_json::Value>,
        left: &(dyn Fn() -> bool + Sync),
    ) -> Result<KeyOutcome, String> {
        // The binding of the document's page (ADR-0190).
        let plan = self.plan_of(&self.page_of(&(session.to_string(), document)));
        let b = plan["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| b["binding"] == binding)
            .ok_or_else(|| format!("no binding `{binding}`"))?;
        let signals: std::collections::BTreeSet<String> = b["signals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
        if signals.is_empty() {
            return Err(format!("`{binding}` is keyed by no signal"));
        }
        if key
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != signals
            || key.values().any(|v| key_val(v).is_none())
        {
            return Err(format!(
                "`{binding}`'s key is its signals' values: {signals:?}"
            ));
        }
        let doc: Doc = (session.to_string(), document);
        // A read for a document the server does not hold is no page's, and
        // takes nothing (ADR-0161): not even its arguments, which are the
        // document's (ADR-0162).
        if !self.keyed.lock().expect("keyed").contains_key(&doc) {
            return Ok(KeyOutcome::Superseded);
        }
        let begun = std::time::Instant::now();
        let asked = Keys::Asked { binding, key };
        let args = self.args_of(plan, session, Some(document), b, &asked)?;
        let flight = self.answer_key(session, b, &args)?;
        let cancel = b["policy"]["on_key_change"] == "cancel";
        let keep = b["policy"]["on_key_change"] == "keep";

        // The latest, for this page? A `cancel` read takes its hold here, and
        // the one before it lets go.
        let hold = self.queries.subscribe(&flight);
        let (hold, let_go, turn) = {
            let mut keyed = self.keyed.lock().expect("keyed");
            let Some(page) = keyed.get_mut(&doc) else {
                return Ok(KeyOutcome::Superseded);
            };
            let read = page.reads.entry(binding.to_string()).or_default();
            if seq <= read.latest {
                return Ok(KeyOutcome::Superseded);
            }
            read.latest = seq;
            let turn = read.turn.clone();
            if cancel {
                let before = read.hold.replace((seq, hold));
                (None, before, turn)
            } else {
                (Some(hold), None, turn)
            }
        };
        // Outside the table: letting go may stop a flight.
        drop(let_go);
        // Under `keep`, one read of the binding at a time: a read waits for
        // the one before it, which runs to its end, and one that is no longer
        // the latest when its turn comes is dropped unread. The browser
        // sends each read at once, so the server knows the latest while the
        // one before it still runs, and never applies that one.
        let _turn = keep.then(|| held_turn(&turn));
        if keep && self.superseded(session, binding, seq, document, false) {
            return Ok(KeyOutcome::Superseded);
        }

        // **Applied in the session's hold, and read again if a change reached
        // the document while it read** (ADR-0224). A commit holds the session
        // from its commit to what it sends, so none comes between the check
        // and what is sent below. Read outside the hold, so a slow read keeps
        // no command of the session waiting, but for the last attempt, read
        // inside it, as a document's last is read in the table (ADR-0151).
        // Until ADR-0224 a value read before a commit could be applied after
        // the commit's change was sent, and the page showed the list without
        // it until the next change.
        let session_lock = self.one_at_a_time(session);
        let sent = |s: &Self| {
            s.pending
                .lock()
                .expect("pending")
                .get(&doc)
                .map(|w| w.pushed)
        };
        let mut attempt = 1;
        let (value, _one) = loop {
            let last = attempt >= DOCUMENT_ATTEMPTS;
            let held = last.then(|| {
                session_lock
                    .lock()
                    .expect("one change of a session at a time")
            });
            let before = sent(self);
            // While it reads, a browser that leaves lets go of it too.
            let done = std::sync::atomic::AtomicBool::new(false);
            let mine = &doc;
            let value = std::thread::scope(|scope| {
                if cancel {
                    let done = &done;
                    scope.spawn(move || {
                        while !done.load(std::sync::atomic::Ordering::SeqCst) {
                            if left() {
                                self.let_go(mine, binding, seq);
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                    });
                }
                let value = self.fetch_answer_at(session, b, &args, &flight);
                done.store(true, std::sync::atomic::Ordering::SeqCst);
                value
            });
            let value = match value {
                Ok(value) => unwrapped(b["resource"].as_str().unwrap_or_default(), value)?,
                // Stopped: let go of, by a newer read or a browser that left.
                Err(_) if self.superseded(session, binding, seq, document, cancel) => {
                    return Ok(KeyOutcome::Superseded);
                }
                Err(why) => return Err(why),
            };
            #[cfg(test)]
            if let Some(between) = self.keyed_fetched.lock().expect("between").take() {
                between(self);
            }
            let held = match held {
                Some(held) => held,
                None => session_lock
                    .lock()
                    .expect("one change of a session at a time"),
            };
            // Newer frames may be the newer key's own result. A request
            // its page no longer awaits must not start another flight to
            // catch up with those frames, especially after losing its hold.
            if self.superseded(session, binding, seq, document, cancel) {
                return Ok(KeyOutcome::Superseded);
            }
            if !last && sent(self) != before {
                drop(held);
                attempt += 1;
                continue;
            }
            break (value, held);
        };
        drop(hold);
        let fetched_ms = begun.elapsed().as_millis();

        // What the page shows for the new key: this binding's value, and the
        // others as they are.
        let mut bindings =
            self.bindings_where(session, Some(document), |n| n != binding, &Keys::Shown)?;
        bindings.insert(binding.to_string(), value);
        let params = self.params_of(&doc);
        let page = self.page_of(&doc);
        let now = self.showing(&page, session, &params, &bindings)?;
        // The presses its values include, read before the table (ADR-0172).
        let applied: Vec<String> = self
            .applied
            .lock()
            .expect("applied")
            .get(session)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default();
        let now = Arc::new(now);
        // **Derived outside the table** (ADR-0296), as a telling is: against
        // what the document shows, read in a moment of its own, and applied
        // only if it still shows it; the last attempt is derived inside.
        let mut attempt = 1;
        let (mut queue, mut keyed, patches, speculated) = loop {
            let last = attempt >= DOCUMENT_ATTEMPTS;
            let ahead = (!last).then(|| {
                let was = self.shown.lock().expect("shown").get(&doc).cloned();
                let derived = was.as_ref().map(|was| {
                    #[cfg(test)]
                    self.deriving(&doc);
                    self.derive(&page, session, &params, &bindings, was, &now)
                });
                (was, derived)
            });
            let queue = self.pending.lock().expect("pending");
            let keyed = self.keyed.lock().expect("keyed");
            if !keyed
                .get(&doc)
                .and_then(|k| k.reads.get(binding))
                .is_some_and(|r| r.latest == seq)
            {
                return Ok(KeyOutcome::Superseded);
            }
            // And what the page speculates on, at the new key (ADR-0222): a
            // speculation after it is shown over the list the page shows.
            let mut shown = self.shown.lock().expect("shown");
            let current = shown.get(&doc).cloned();
            let Some(was) = current.clone() else {
                return Ok(KeyOutcome::Superseded);
            };
            let patches = match ahead {
                Some((before, Some(derived))) if same_shown(&before, &current) => derived?,
                // Something reached the document while this was derived.
                Some(_) => {
                    drop(shown);
                    drop(keyed);
                    drop(queue);
                    attempt += 1;
                    continue;
                }
                None => self.derive(&page, session, &params, &bindings, &was, &now)?,
            };
            let speculated = self.speculated_frames(&doc, &page, &was, &now, &applied);
            shown.insert(doc.clone(), now.clone());
            break (queue, keyed, patches, speculated);
        };
        let _held = self.held("a keyed read, applied");
        let read = keyed
            .get_mut(&doc)
            .and_then(|k| k.reads.get_mut(binding))
            .expect("the latest read, as checked in this hold");
        read.shown = key.clone();
        if read.hold.as_ref().is_some_and(|(n, _)| *n == seq) {
            read.hold = None;
        }
        let entry = keyed_entry(session, binding);
        let Some(waiting) = queue.get_mut(&doc) else {
            return Ok(KeyOutcome::Superseded);
        };
        waiting.note(format_args!(
            "read {binding} #{seq} applied {} ms after it was asked, its value read in {fetched_ms} ms, attempt {attempt}",
            begun.elapsed().as_millis()
        ));
        waiting.push(StreamFrame::ResourceChanged {
            protocol: CURRENT,
            entry: entry.clone(),
            version: Version(seq),
        });
        waiting.push(StreamFrame::PatchSet(PatchSet {
            protocol: CURRENT,
            basis: CausalBasis::of(entry, Version(seq)),
            patches,
        }));
        for frame in speculated {
            waiting.push(frame);
        }
        Ok(KeyOutcome::Applied)
    }

    /// **Let go of a `cancel` read's flight** (ADR-0152), if it is still the
    /// read holding it: a browser that left.
    fn let_go(&self, doc: &Doc, binding: &str, seq: u64) {
        let hold = {
            let mut keyed = self.keyed.lock().expect("keyed");
            keyed
                .get_mut(doc)
                .and_then(|k| k.reads.get_mut(binding))
                .filter(|r| r.hold.as_ref().is_some_and(|(n, _)| *n == seq))
                .and_then(|r| r.hold.take())
        };
        drop(hold);
    }

    /// Whether a read is no longer the one its page waits for: a newer one
    /// was asked, its page was replaced, or, under `cancel`, it was let go.
    fn superseded(
        &self,
        session: &str,
        binding: &str,
        seq: u64,
        document: u64,
        cancel: bool,
    ) -> bool {
        let keyed = self.keyed.lock().expect("keyed");
        match keyed.get(&(session.to_string(), document)) {
            None => true,
            Some(k) => k.reads.get(binding).is_none_or(|r| {
                r.latest != seq || (cancel && r.hold.as_ref().is_none_or(|(n, _)| *n != seq))
            }),
        }
    }

    /// **A session's page whose values cannot be read** (E14, T04): its
    /// document cannot be kept current, and is told to read itself again,
    /// which answers why. Never a panic: one failed query is one page that
    /// cannot be shown, not a server that stops for everyone.
    fn unshowable(&self, doc: &Doc, why: &str) {
        if std::env::var("PW_TRACE").is_ok() {
            eprintln!("unshowable document={doc:?}: {why}");
        }
        let mut queue = self.pending.lock().expect("pending");
        if self.shown.lock().expect("shown").remove(doc).is_some()
            && let Some(waiting) = queue.get_mut(doc)
        {
            waiting.push(StreamFrame::Recovery {
                protocol: CURRENT,
                recovery: Recovery::Reload,
            });
        }
    }

    /// [`Server::unshowable`], for every document of a session: what none of
    /// them can show.
    fn unshowable_session(&self, session: &str, why: &str) {
        let documents = documents_of(&self.pending.lock().expect("pending"), session);
        for doc in documents {
            self.unshowable(&doc, why);
        }
    }

    /// **What a document this host serves may load and run** (`csp`): the
    /// script that starts the runtime and the data layer's style, by their
    /// hashes; what the build's markup asks; and the deployment's payment
    /// provider and identity provider, where it has them at origins of
    /// their own.
    fn policy(&self) -> String {
        let deployment = csp::Deployment {
            frames: self.payments.get().map(|p| p.provider.clone()),
            forms: self.identity.sign_in_origin(),
        };
        csp::document(RUNTIME_SOURCE, self.data.style(), &self.needs, &deployment)
    }

    /// **A document, and each streamed region's arm as its query settles**
    /// (ADR-0148). With nothing left to settle it is one response of known
    /// length, as every document was. Otherwise all but the document's end is
    /// written at once; each region's patch follows, in the order the queries
    /// settle; and the response ends with the last, closing the connection,
    /// which is what tells the browser the document is complete.
    #[allow(clippy::too_many_arguments)]
    fn respond_streaming(
        &self,
        stream: &mut TcpStream,
        session: &str,
        fresh: bool,
        page: &str,
        template: &Template,
        env: &Env,
        settling: &mut Settling,
    ) {
        let cookie = if fresh {
            // TRACK SEAM (identity): the session cookie, HttpOnly and Secure
            // outside this machine.
            identity::session_cookie(session)
        } else {
            String::new()
        };
        // **The build that served it** (ADR-0300), which a page that keeps
        // its parts across a navigation holds the next document to.
        let build = format!("pw-build: {}\r\n", self.build_id);
        // What it may load and run (`csp`).
        let policy = format!("content-security-policy: {}\r\n", self.policy());
        if settling.waiting.is_empty() && settling.settled.is_empty() {
            write_response(
                stream,
                200,
                "text/html; charset=utf-8",
                &format!("{PRIVATE}{cookie}{build}{policy}"),
                page.as_bytes(),
            );
            return;
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\n{NOSNIFF}{PRIVATE}{cookie}\
             {build}{policy}connection: close\r\n\r\n"
        );
        let shell = page.strip_suffix(DOCUMENT_END).unwrap_or(page);
        if stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(shell.as_bytes()))
            .and_then(|_| stream.flush())
            .is_err()
        {
            return;
        }
        while let Some((part, outcome)) = settling.next() {
            let patch = |outcome: Settled| {
                pw_render::settled_patch(
                    template,
                    PartId(part),
                    &env.clone().settle(PartId(part), outcome),
                    &self.templates,
                )
            };
            // An arm that does not render is a region whose query failed, as
            // far as the page can say: never a placeholder forever.
            let written = patch(outcome).or_else(|why| {
                if std::env::var("PW_TRACE").is_ok() {
                    eprintln!("stream {part} did not render: {why:?}");
                }
                patch(Settled::Failed(None))
            });
            let Ok(written) = written else { continue };
            // In two parts, a pause between them, where a test asks for it
            // (ADR-0223): the browser then parses a template it has only part
            // of.
            let pause = self.split_fills.load(std::sync::atomic::Ordering::SeqCst);
            let mut half = if pause > 0 {
                written.len() / 2
            } else {
                written.len()
            };
            while !written.is_char_boundary(half) {
                half -= 1;
            }
            let (first, rest) = written.split_at(half);
            if stream
                .write_all(first.as_bytes())
                .and_then(|_| stream.flush())
                .is_err()
            {
                return;
            }
            if !rest.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(pause));
                if stream
                    .write_all(rest.as_bytes())
                    .and_then(|_| stream.flush())
                    .is_err()
                {
                    return;
                }
            }
        }
        let _ = stream.write_all(DOCUMENT_END.as_bytes());
        let _ = stream.flush();
        let _ = stream.shutdown(std::net::Shutdown::Write);
    }

    /// The store page's template.
    fn store_template(&self) -> &Template {
        self.templates
            .iter()
            .find(|t| t.name == "StorePage")
            .expect("StorePage")
    }

    /// **What a document of `page` renders from** (ADR-0190): each binding's
    /// whole value, its parts' values, and its signals at their first values,
    /// in the session's identity domain; and on the store's page, the menu's
    /// public fragment. A list a session's query fills is its binding's
    /// value: until 2026-10-03 it was set a second time, from what the
    /// document shows, which since ADR-0146 is the same value under the same
    /// name.
    fn document_env(
        &self,
        page: &str,
        session: &str,
        params: &Params,
        bindings: &BTreeMap<String, Val>,
    ) -> Env {
        let plan = self.plan_of(page);
        // The store a document of the store's page shows (ADR-0162): its
        // `id`, or the benchmark's.
        let store = params.get("id").map_or(STORE_ID, String::as_str);
        // Both bound BEFORE the chain. A `MutexGuard` produced inside a method
        // argument lives until the end of the whole STATEMENT, so locking the
        // menu inside the chain and locking it again inside `menu_fragment`
        // deadlocked a non-reentrant mutex against itself — presenting as a
        // request that simply never returned.
        let menu = (page == self.store_page()).then(|| {
            let rows = self
                .rendered_value(plan, "menu", &bindings["menu"])
                .unwrap_or_else(|e| panic!("the menu's rows: {e}"));
            // The rows the fragment shows, which the open pages do
            // (ADR-0178).
            self.menu_fragment(store, &rows)
        });
        // **Its parameters, as its address gave them** (ADR-0231): each one
        // text, as a page that binds no query is rendered with them
        // (ADR-0130). Until 2026-10-05 none was here, and a title, an
        // attribute or a handler's captures that read one built, then every
        // request for the page was answered 503.
        let mut env = Env::new();
        // Each as its address gave it: a query's `Some` or `None`
        // (ADR-0309), a path's its text.
        for name in plan["params"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str())
        {
            if let Some(v) = param_val(plan, name, params) {
                env = env.set(name, val_to_value(&v));
            }
        }
        // Each binding's whole value, so a block a query decides, and what is
        // inside it, reads any field of it (ADR-0146), and each row what it
        // reads through a member (ADR-0169).
        for (name, value) in bindings {
            let rendered = self
                .rendered_value(plan, name, value)
                .unwrap_or_else(|e| panic!("`{name}`: {e}"));
            env = env.set(name, rendered);
        }
        if let Some((fragment, rows)) = menu {
            env = env
                .set("menu", rows)
                // The public fragment, EMITTED rather than rendered. Its
                // instance tokens are the fragment's own, so every reader's
                // document contains the same bytes and one patch addresses
                // all of them.
                .materialized(self.menu_part().1, &fragment);
        }
        // Each text part a host computes, and each computed attribute's value
        // (ADR-0226), by the path the compiler names it.
        for part in plan["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(plan["derived"].as_array().into_iter().flatten())
        {
            let path = part["path"].as_str().unwrap_or_default();
            // An arm's payload, of a value of another case, is set nowhere:
            // its arm is not shown (the integrator's, 2026-10-13).
            if let Some(value) = self
                .read_part(bindings, part)
                .unwrap_or_else(|e| panic!("part `{path}`: {e}"))
            {
                env = env.set(path, val_to_value(&value));
            }
        }
        // What it computes from its signals, at their first values (ADR-0227).
        let env = self
            .with_computed_signals(env, plan, self.template_of(page))
            .unwrap_or_else(|e| panic!("`{page}`'s computed values: {e}"));
        // Its signals, at their first values (ADR-0140).
        with_signals(env, plan)
            // A page, not a materialization: its domain is the route identity
            // and its partition. The generation is carried whatever the
            // partition is — the two are orthogonal.
            .in_domain(self.domain_of(page, session))
    }

    /// **The lists a document of `page` keeps current itself** (ADR-0145):
    /// each collection the page iterates, but on the store's page one whose
    /// binding is cached shared, a fragment every reader shares, patched once
    /// for all of them (the menu, E7-P). No other page has a fragment
    /// (ADR-0190).
    fn own_lists(&self, page: &str) -> Vec<String> {
        let plan = self.plan_of(page);
        let fragments = page == self.store_page();
        let shared = |name: &str| {
            fragments
                && plan["bindings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|b| b["binding"] == name && b["policy"]["cache"] == "shared")
        };
        // A list by its path, whose binding is its first name (ADR-0170).
        plan["collections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str())
            .filter(|c| !shared(c.split('.').next().unwrap_or_default()))
            .map(str::to_string)
            .collect()
    }

    /// **What a session's document shows, from its queries' values**
    /// (ADR-0145): each part's text, and each list a session's query fills.
    fn showing(
        &self,
        page: &str,
        session: &str,
        params: &Params,
        bindings: &BTreeMap<String, Val>,
    ) -> Result<Shown, String> {
        let plan = self.plan_of(page);
        let mut shown = Shown::default();
        for part in plan["parts"].as_array().into_iter().flatten() {
            let id = part["part"].as_u64().unwrap_or_default() as u32;
            let value = val_to_value(&self.read_shown_part(bindings, part)?);
            let text = text_of(&value).ok_or_else(|| format!("part {id} has no text form"))?;
            shown.texts.insert(id, text);
        }
        // Each list by its path: a binding's value, or a list inside it,
        // `cart.lines` (ADR-0170). Until 2026-10-03 the binding was asked
        // for, and `cart` is no list.
        for list in self.own_lists(page) {
            let mut path = list.split('.');
            let binding = path.next().unwrap_or_default();
            let fields: Vec<&str> = path.collect();
            let value = bindings
                .get(binding)
                .ok_or_else(|| format!("no binding `{binding}`"))?;
            let mut rendered = self.rendered_value(plan, binding, value)?;
            let Some(Value::List(items)) = value_at_mut(&mut rendered, &fields) else {
                return Err(format!("`{list}` is not a list"));
            };
            shown.lists.insert(list.clone(), std::mem::take(items));
        }
        // Each block that renders a query's value, as it renders now
        // (ADR-0146), and what it was rendered from, where one is told part
        // by part (ADR-0320).
        let env = self.document_env(page, session, params, bindings);
        let template = self.template_of(page);
        for block in plan["blocks"].as_array().into_iter().flatten() {
            let id = block.as_u64().unwrap_or_default() as u32;
            let html = pw_render::render_part(template, PartId(id), &env, &self.templates)
                .map_err(|b| format!("block {id}: {b:?}"))?;
            shown.blocks.insert(id, html);
        }
        if shown
            .blocks
            .keys()
            .any(|id| !self.speculation_renders(page, *id))
        {
            shown.bindings = Some(Arc::new(bindings.clone()));
        }
        // Each attribute at the top of the page that reads a query's value,
        // written by the function the page's render writes it with
        // (ADR-0171).
        for attribute in plan["attributes"].as_array().into_iter().flatten() {
            let id = attribute.as_u64().unwrap_or_default() as u32;
            let part = find_part(&template.chunks, id)
                .ok_or_else(|| format!("the plan's attribute {id} is no part"))?;
            let written = pw_render::attribute_value(part, &env)
                .map_err(|b| format!("attribute {id}: {b:?}"))?
                .ok_or_else(|| format!("part {id} is no attribute"))?;
            shown.attributes.insert(id, written);
        }
        // What each handler at the top of the page captures of a query's
        // value (ADR-0217), as the page's render writes it.
        for handler in plan["captures"].as_array().into_iter().flatten() {
            let id = handler.as_u64().unwrap_or_default() as u32;
            let written = pw_render::captures_at(template, PartId(id), &env)
                .map_err(|b| format!("the captures of part {id}: {b:?}"))?;
            shown.captures.insert(id, written);
        }
        // The page's title, as the document's head writes it (ADR-0183).
        if let Some(id) = plan["title"].as_u64() {
            let title = pw_render::title_text(template, &env)
                .map_err(|b| format!("the title: {b:?}"))?
                .ok_or_else(|| format!("the plan's title {id} is no part"))?;
            shown.title = Some((id as u32, title));
        }
        // Each value it speculates on (ADR-0222), from the same read, as
        // the page's module reads it (ADR-0233).
        for binding in self.speculated_bindings(page) {
            if let Some(value) = bindings.get(&binding) {
                let written = self.speculated_json(plan, &binding, value)?;
                shown.speculated.insert(binding, written);
            }
        }
        Ok(shown)
    }

    /// **The patches that turn what a document shows into what it should**
    /// (ADR-0145): each part whose text changed, and each list's change as
    /// keyed operations.
    fn derive(
        &self,
        page: &str,
        session: &str,
        params: &Params,
        bindings: &BTreeMap<String, Val>,
        was: &Shown,
        now: &Shown,
    ) -> Result<Vec<Targeted>, String> {
        let template = self.template_of(page);
        let schema = TemplateSchemaId(template.schema.clone());
        let mut out = Vec::new();
        for (id, text) in &now.texts {
            if was.texts.get(id) != Some(text) {
                out.push(Targeted {
                    target: PartAddress::new(&schema, LocalPartId(*id)),
                    operation: PatchOp::ReplaceText { text: text.clone() },
                });
            }
        }
        // The page's title, set as text is (ADR-0183): the browser sets it
        // as `document.title`.
        if let Some((id, title)) = &now.title
            && was.title.as_ref().map(|(_, t)| t) != Some(title)
        {
            out.push(Targeted {
                target: PartAddress::new(&schema, LocalPartId(*id)),
                operation: PatchOp::ReplaceText {
                    text: title.clone(),
                },
            });
        }
        let env = self.document_env(page, session, params, bindings);
        for (list, items) in &now.lists {
            // A list at the top of the page only. One inside a block would
            // have no range while the block is not shown, and patching it
            // would read the page again at every change. No page served here
            // has one: a block a query decides is not rendered by this server
            // yet, and one a signal decides reads no query (ADR-0137).
            let top = template.chunks.iter().any(|c| {
                matches!(c, pw_render::Chunk::Dynamic(pw_render::Part::Each { collection, .. })
                    if collection == list)
            });
            if !top {
                continue;
            }
            let old = was.lists.get(list).map(Vec::as_slice).unwrap_or(&[]);
            out.extend(list_patches(
                template,
                &schema,
                list,
                old,
                items,
                &env,
                &self.templates,
            )?);
        }
        // Each attribute whose value changed, set where it is (ADR-0171).
        for (id, written) in &now.attributes {
            if was.attributes.get(id) == Some(written) {
                continue;
            }
            let (name, value) = written.clone();
            out.push(Targeted {
                target: PartAddress::new(&schema, LocalPartId(*id)),
                operation: match value {
                    Some(value) => PatchOp::SetAttribute { name, value },
                    None => PatchOp::RemoveAttribute { name },
                },
            });
        }
        // Each handler's captures that changed, set where its element is
        // (ADR-0217). Until then they were what the page was first rendered
        // with, and a press after a commit sent that.
        for (id, written) in &now.captures {
            if was.captures.get(id) == Some(written) {
                continue;
            }
            let name = "data-pw-captures".to_string();
            let operation = match written {
                Some(value) => PatchOp::SetAttribute {
                    name,
                    value: value.clone(),
                },
                None => PatchOp::RemoveAttribute { name },
            };
            out.push(Targeted {
                target: PartAddress::new(&schema, LocalPartId(*id)),
                operation,
            });
        }
        // Each block whose rendering changed (ADR-0146): told part by part
        // where it shows the arm it showed, as a row is (ADR-0320), so a
        // field in it keeps its focus and a live region in it is told its
        // new text; rendered again where it is otherwise. Until 2026-10-12
        // every one was rendered again, whole.
        let before = was
            .bindings
            .as_ref()
            .map(|b| self.document_env(page, session, params, b));
        for (id, html) in &now.blocks {
            if was.blocks.get(id) == Some(html) {
                continue;
            }
            let in_place = match &before {
                // **Not one a speculation renders** (ADR-0172): the browser
                // may show it as it speculated, and only the whole block finds
                // it as it was derived.
                Some(before)
                    if was.blocks.contains_key(id) && !self.speculation_renders(page, *id) =>
                {
                    pw_render::block_changes(template, PartId(*id), before, &env, &self.templates)
                        .map_err(|b| format!("block {id}: {b:?}"))?
                }
                _ => None,
            };
            match in_place {
                Some(changes) => out.extend(instance_patches(&schema, &[], changes)),
                None => out.push(Targeted {
                    target: PartAddress::new(&schema, LocalPartId(*id)),
                    operation: PatchOp::ReplaceRange { html: html.clone() },
                }),
            }
        }
        Ok(out)
    }

    /// **Does a speculation on `page` render the part `id` again?**
    /// (ADR-0172): the browser renders it from the value it speculates on,
    /// whole, so a host tells it whole too.
    fn speculation_renders(&self, page: &str, id: u32) -> bool {
        self.speculations
            .get(page)
            .and_then(|m| m["regions"].as_array())
            .into_iter()
            .flatten()
            .any(|r| r.as_u64() == Some(u64::from(id)))
    }
}

/// **What a command handed the platform** (ADR-0208, ADR-0209): its events
/// and the entries it invalidates, each by its declaration's path with the
/// values it computed, and the runs it scheduled or cancelled (the
/// scheduled-commands track), staged until its writes commit.
#[derive(Default)]
struct Staged {
    events: Vec<(String, Vec<Val>)>,
    invalidated: Vec<crate::data::Dropped>,
    scheduled: Vec<schedules::Scheduling>,
}

type StagedEvents = Arc<Mutex<Staged>>;

/// **How a host's scheduled runs ended** (the scheduled-commands track):
/// done, to run again, or kept, failed for good.
#[derive(Default)]
struct CountedRuns {
    stale: std::sync::atomic::AtomicU64,
    ran: std::sync::atomic::AtomicU64,
    retried: std::sync::atomic::AtomicU64,
    failed: std::sync::atomic::AtomicU64,
}

/// **A region a document leaves for its query to fill** (ADR-0148): which
/// part, the plan's entry for it (its query's component and policies), and
/// what this request gives the query.
struct StreamRun {
    part: u32,
    stream: serde_json::Value,
    args: Vec<Val>,
    streamed: bool,
}

/// **Each stream a plan names, with what this request gives its query**
/// (ADR-0148): a page parameter as `params` holds it, `current_session()`
/// as the request's session, and `current_user()` as its reader's user
/// (track `store-accounts`: a store's estimate to the reader's address;
/// ADR-0270 left a stream's `current_user()` unclaimed).
fn stream_runs(
    plan: &serde_json::Value,
    session: &str,
    user: &str,
    params: &BTreeMap<String, String>,
) -> Result<Vec<StreamRun>, String> {
    let mut out = Vec::new();
    for s in plan["streams"].as_array().into_iter().flatten() {
        let args = s["args"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| match a.as_str() {
                Some("current_session()") => Ok(Val::String(session.into())),
                Some(arg) if notifications::is_current_user(arg) => Ok(Val::String(user.into())),
                Some(name) => param_val(plan, name, params).ok_or_else(|| {
                    format!("a stream's query is given `{name}`, which is not given")
                }),
                None => Err(format!(
                    "a stream's argument this server cannot compute: {a}"
                )),
            })
            .collect::<Result<Vec<Val>, String>>()?;
        out.push(StreamRun {
            part: s["part"].as_u64().unwrap_or_default() as u32,
            stream: s.clone(),
            args,
            streamed: s["streamed"] == true,
        });
    }
    Ok(out)
}

/// **A document's streams, their queries running** (ADR-0148): each on a
/// thread of its own, started before the document is rendered, and each
/// given its `timeout` from when it started. A stream whose budget is spent
/// is given the host's failure; its answer, when it comes, is kept as its
/// policy says, for the next reader.
struct Settling {
    answers: std::sync::mpsc::Receiver<(u32, Result<Val, String>)>,
    /// Each stream not settled yet: whether it is streamed, and when its
    /// budget is spent.
    waiting: BTreeMap<u32, (bool, Option<std::time::Instant>)>,
    /// Settled and not taken yet, in the order they settled.
    settled: std::collections::VecDeque<(u32, Settled)>,
}

impl Settling {
    fn start<'scope, 'env>(
        scope: &'scope std::thread::Scope<'scope, 'env>,
        server: &'env Server,
        session: &'env str,
        runs: Vec<StreamRun>,
    ) -> Settling {
        let (tx, answers) = std::sync::mpsc::channel();
        let mut waiting = BTreeMap::new();
        let started = std::time::Instant::now();
        for run in runs {
            let budget = run.stream["policy"]["timeout_ms"]
                .as_u64()
                .map(|ms| started + std::time::Duration::from_millis(ms));
            waiting.insert(run.part, (run.streamed, budget));
            let tx = tx.clone();
            scope.spawn(move || {
                let answer = server.fetch_answer(session, &run.stream, &run.args);
                let _ = tx.send((run.part, answer));
            });
        }
        Settling {
            answers,
            waiting,
            settled: std::collections::VecDeque::new(),
        }
    }

    /// One answer, or each budget spent by now, or nothing left to come.
    fn receive(&mut self) {
        use std::sync::mpsc::RecvTimeoutError;
        let next = self.waiting.values().filter_map(|(_, at)| *at).min();
        let got = match next {
            Some(at) => self
                .answers
                .recv_timeout(at.saturating_duration_since(std::time::Instant::now())),
            None => self
                .answers
                .recv()
                .map_err(|_| RecvTimeoutError::Disconnected),
        };
        match got {
            Ok((part, answer)) => {
                if self.waiting.remove(&part).is_some() {
                    self.settled.push_back((part, settled_of(answer)));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = std::time::Instant::now();
                let spent: Vec<u32> = self
                    .waiting
                    .iter()
                    .filter(|(_, (_, at))| at.is_some_and(|at| at <= now))
                    .map(|(part, _)| *part)
                    .collect();
                for part in spent {
                    self.waiting.remove(&part);
                    self.settled.push_back((part, Settled::Failed(None)));
                }
            }
            // Every query answered or is gone: what still waits has nothing
            // coming.
            Err(RecvTimeoutError::Disconnected) => {
                for part in std::mem::take(&mut self.waiting).into_keys() {
                    self.settled.push_back((part, Settled::Failed(None)));
                }
            }
        }
    }

    /// **What the document is rendered with**: each stream the page waits
    /// for, settled, and each streamed one that has settled already, which
    /// the document shows at once rather than as a placeholder.
    fn for_document(&mut self) -> Vec<(u32, Settled)> {
        while self.waiting.values().any(|(streamed, _)| !streamed) {
            self.receive();
        }
        while let Ok((part, answer)) = self.answers.try_recv() {
            if self.waiting.remove(&part).is_some() {
                self.settled.push_back((part, settled_of(answer)));
            }
        }
        self.settled.drain(..).collect()
    }

    /// The next streamed region to settle, as it does; `None` when all have.
    fn next(&mut self) -> Option<(u32, Settled)> {
        while self.settled.is_empty() && !self.waiting.is_empty() {
            self.receive();
        }
        self.settled.pop_front()
    }
}

/// What every document ends with, written last: after each streamed
/// region's patch, when there are any (ADR-0148).
const DOCUMENT_END: &str = "</body>\n</html>\n";

/// A structural change to a keyed collection.
///
/// One type, holding both halves of the change: what it does to the server's
/// list, and what patch describes it. Splitting them is how a list and its
/// patches drift — the patch generator gets an off-by-one the state does not
/// have, and the page ends up with an order the server never held.
#[derive(Debug, Clone)]
enum MenuOp {
    /// Insert `(id, name)` before or after `at`.
    Insert {
        id: String,
        name: String,
        /// The instance to anchor to, or the head/tail of the collection.
        at: Option<String>,
        before: bool,
    },
    Remove {
        id: String,
    },
    /// Move `id` after `after`, or to the front when `after` is `None`.
    Move {
        id: String,
        after: Option<String>,
    },
    /// Change one item's ordinary field. Not structural — and included here to
    /// prove that it does NOT disturb its siblings.
    Rename {
        id: String,
        name: String,
    },
    /// Whether one item can be ordered changed (ADR-0178): its row is shown
    /// again, as the data layer now answers for it. The menu's items are
    /// as they were.
    Stock {
        id: String,
        /// Whether it can be ordered now, written with the change (track
        /// `store-pg`).
        available: bool,
    },
}

impl MenuOp {
    fn position(items: &[(String, String)], id: &str) -> Result<usize, String> {
        items
            .iter()
            .position(|(k, _)| k == id)
            .ok_or_else(|| format!("no item `{id}`"))
    }

    fn apply(&self, items: &mut Vec<(String, String)>) -> Result<(), String> {
        match self {
            MenuOp::Insert {
                id,
                name,
                at,
                before,
            } => {
                // A duplicate key is REFUSED, not disambiguated. Two instances
                // with one key means one address for two places, which is
                // `docs/RISK_QUEUE.md` 25 arriving through the front door.
                if items.iter().any(|(k, _)| k == id) {
                    return Err(format!("duplicate key `{id}`"));
                }
                if id.is_empty() {
                    return Err("an item with no key has no address".into());
                }
                let at = match at {
                    Some(a) => {
                        let p = Self::position(items, a)?;
                        if *before { p } else { p + 1 }
                    }
                    // No anchor: the head for `before`, the tail for `after`.
                    // This is the only way an item can enter an empty
                    // collection without re-rendering the document.
                    None if *before => 0,
                    None => items.len(),
                };
                items.insert(at, (id.clone(), name.clone()));
            }
            MenuOp::Remove { id } => {
                let at = Self::position(items, id)?;
                items.remove(at);
            }
            MenuOp::Move { id, after } => {
                let from = Self::position(items, id)?;
                let item = items.remove(from);
                let to = match after {
                    None => 0,
                    Some(a) => Self::position(items, a)? + 1,
                };
                items.insert(to, item);
            }
            MenuOp::Rename { id, name } => {
                let at = Self::position(items, id)?;
                items[at].1 = name.clone();
            }
            MenuOp::Stock { id, .. } => {
                Self::position(items, id)?;
            }
        }
        Ok(())
    }
}

/// A cart as the WIT's `domain-cart` record: its lines, each with an item,
/// a quantity and a unit price.
fn cart_value(lines: &[Line]) -> Val {
    Val::Record(vec![(
        "lines".into(),
        Val::List(
            lines
                .iter()
                .map(|line| {
                    Val::Record(vec![
                        ("item-id".into(), Val::String(line.item.clone())),
                        // As the line recorded it (ADR-0172). A program whose
                        // `CartLine` declares no name is not passed one
                        // (ADR-0166).
                        ("name".into(), Val::String(line.name.clone())),
                        ("quantity".into(), Val::S64(line.quantity)),
                        // The item's price when the line was made (ADR-0169,
                        // ADR-0172). Until 2026-10-03 every line was priced
                        // 450, whatever its item.
                        ("unit-price".into(), Val::S64(line.price)),
                    ])
                })
                .collect(),
        ),
    )])
}

/// **A cart's lines, as `(item, quantity)`**, from a cart's value: what a
/// test compares (track `store-pg`).
#[cfg(test)]
fn lines_of(cart: &Val) -> Vec<(String, i64)> {
    let Some(Val::List(lines)) = val_at(cart, &["lines"]) else {
        panic!("no cart: {cart:?}");
    };
    lines
        .iter()
        .map(
            |line| match (val_at(line, &["item-id"]), val_at(line, &["quantity"])) {
                (Some(Val::String(item)), Some(Val::S64(q))) => (item.clone(), *q),
                other => panic!("no line: {other:?}"),
            },
        )
        .collect()
}

/// **A binding's policy, as `pw-resource`'s manifest** (ADR-0127).
/// **A query's value, from its answer** (ADR-0147): the `Ok` value, and a
/// declared error as a failure.
fn unwrapped(component_id: &str, answer: Val) -> Result<Val, String> {
    match answer {
        Val::Result(Ok(Some(v))) => Ok(*v),
        Val::Result(Err(e)) => Err(format!("{component_id} answered {e:?}")),
        v => Ok(v),
    }
}

/// **What a stream's query settled to, from its answer** (ADR-0148): its
/// `Ok` value; `Some` of its declared error; or `None`, for the host's
/// failure, a spent budget or a trap.
fn settled_of(answer: Result<Val, String>) -> Settled {
    match answer {
        Ok(Val::Result(Ok(Some(v)))) => Settled::Ready(val_to_value(&v)),
        Ok(Val::Result(Ok(None))) => Settled::Ready(Value::Record(BTreeMap::new())),
        Ok(Val::Result(Err(Some(e)))) => Settled::Failed(Some(val_to_value(&e))),
        Ok(Val::Result(Err(None))) => Settled::Failed(Some(Value::Record(BTreeMap::new()))),
        Ok(v) => Settled::Ready(val_to_value(&v)),
        Err(_) => Settled::Failed(None),
    }
}

fn runtime_manifest(resource: &str, policy: &serde_json::Value) -> pw_resource::Manifest {
    let mut m = pw_resource::Manifest::new(resource)
        .freshness(policy["freshness_ms"].as_u64().unwrap_or(0))
        .attempts(policy["attempts"].as_u64().unwrap_or(1) as u32)
        // Whether its delays vary, as its `retry` says (ADR-0215).
        .jitter(policy["jitter"] == true);
    if let Some(t) = policy["timeout_ms"].as_u64() {
        m.timeout = t;
    }
    if policy["cache"] == "private" || policy["privacy"] != "public" {
        m = m.private();
    }
    if policy["cache"] == "none" {
        m = m.uncached();
    }
    // Public data alone (ADR-0177): the runtime serves no private value from
    // its fallback, whatever this says, and PW0343 refuses one declared.
    if policy["fallback"] == "last_known_good" {
        m = m.last_known_good();
    }
    m
}

/// **An entry's key** (ADR-0127): the arguments the policy's `key` names, by
/// position, and for a private entry the session first. `None` when an
/// argument the key names was not given.
fn entry_key(session: &str, policy: &serde_json::Value, args: &[Val]) -> Option<String> {
    let mut parts = Vec::new();
    if entry_is_private(policy) {
        parts.push(format!("session={session}"));
    }
    for i in policy["key"].as_array().into_iter().flatten() {
        let v = args.get(i.as_u64()? as usize)?;
        parts.push(val_to_json(v).to_string());
    }
    Some(parts.join("\u{1f}"))
}

/// **Whether each of a query's entries is one session's**, as `entry_key`
/// keys it: a private cache, or a value that is not public.
fn entry_is_private(policy: &serde_json::Value) -> bool {
    policy["cache"] == "private" || policy["privacy"] != "public"
}

/// The parameters a query's entries are keyed by, by position.
fn key_positions(policy: &serde_json::Value) -> Vec<usize> {
    policy["key"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i.as_u64())
        .map(|i| i as usize)
        .collect()
}

/// **Whether the entry kept at `key` is one `args` names** (ADR-0256): the
/// value at each of its query's key positions where `args` gives one,
/// whatever the rest, in any session's partition. `args` is the value at
/// each parameter, `None` where a command wrote `_` or an event leaves it
/// unbound.
fn names_entry(policy: &serde_json::Value, args: &[Option<Val>], key: &str) -> bool {
    let mut parts: Vec<&str> = match key.is_empty() {
        true => Vec::new(),
        false => key.split('\u{1f}').collect(),
    };
    if entry_is_private(policy) {
        match parts.first() {
            Some(p) if p.starts_with("session=") => {
                parts.remove(0);
            }
            _ => return false,
        }
    }
    let positions = key_positions(policy);
    parts.len() == positions.len()
        && positions
            .iter()
            .zip(&parts)
            .all(|(i, part)| match args.get(*i) {
                Some(Some(v)) => serde_json::from_str::<serde_json::Value>(part)
                    .is_ok_and(|p| p == val_to_json(v)),
                _ => true,
            })
}

/// **A key's value, as a component is given it** (ADR-0152): a `String`, an
/// `Int` or a `Bool`, which PW5308 holds a key to.
fn key_val(v: &serde_json::Value) -> Option<Val> {
    match v {
        serde_json::Value::String(s) => Some(Val::String(s.clone())),
        serde_json::Value::Bool(b) => Some(Val::Bool(*b)),
        serde_json::Value::Number(n) => n.as_i64().map(Val::S64),
        _ => None,
    }
}

/// **A keyed binding's entry, for one session** (ADR-0152): its version is
/// the number of the read its page last applied.
fn keyed_entry(session: &str, binding: &str) -> ResourceEntryId {
    ResourceEntryId::derive(
        &EntryIdentity::new(
            &format!("store.page.{binding}#key"),
            &[session],
            Partition::Session {
                id: session.to_string(),
            },
        )
        .generation(BUILD),
        &IDENTITY,
    )
}

/// **A menu item's category** (E14, T07): hot or cold, by the item.
fn category_of(item: &str) -> &'static str {
    match item {
        "cold-brew" => "cold",
        _ => "hot",
    }
}

/// The store `/StorePage.html` and `/` show (ADR-0125), the first of the two
/// this server holds (ADR-0162).
const STORE_ID: &str = "47";
const STORE_NAME: &str = "Blue Bottle";

/// **The name a template reads a predicate's answer by** (ADR-0307):
/// `SignedIn~holds`, which no source writes. The compiler's convention
/// (`pw_core::template_ir::asked_name`), which the plan's `predicates` and
/// the template's paths follow; a test holds the two the same.
fn asked_name(predicate: &str) -> String {
    format!("{predicate}~holds")
}

/// **Why a page's values could not be read** (ADR-0163): its address names
/// nothing, as the page declares, or it cannot be shown now. Told apart by
/// type, so no failure's text can make it the other.
#[derive(Debug)]
enum Unread {
    /// A binding's query answered the case the page's `not_found_on` names:
    /// answered 404.
    NotFound(String),
    /// A binding's query answered the case the page's `redirect_on` names
    /// (ADR-0295): answered 308, or 307, to the page's route at `to`.
    Moved { to: String, permanent: bool },
    /// Any other failure (ADR-0147): answered 503.
    Failed(String),
}

impl Unread {
    /// The same failure, said of `what`.
    fn of(self, what: &str) -> Unread {
        match self {
            Unread::NotFound(why) => Unread::NotFound(format!("{what}: {why}")),
            moved @ Unread::Moved { .. } => moved,
            Unread::Failed(why) => Unread::Failed(format!("{what}: {why}")),
        }
    }
}

impl From<String> for Unread {
    fn from(why: String) -> Unread {
        Unread::Failed(why)
    }
}

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unread::NotFound(why) => write!(f, "not found: {why}"),
            Unread::Moved { to, .. } => write!(f, "moved to `{to}`"),
            Unread::Failed(why) => f.write_str(why),
        }
    }
}

impl From<Unread> for String {
    fn from(unread: Unread) -> String {
        unread.to_string()
    }
}

/// **What a page whose address names nothing is answered** (ADR-0163), with
/// 404: accessible, laid out at a phone's width (ADR-0182), and nothing of
/// the page's.
const NOT_FOUND_PAGE: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
     <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
     <title>Not found</title>\n</head>\n<body>\n<main>\n<h1>Not found</h1>\n\
     <p>Nothing is at this address.</p>\n</main>\n</body>\n</html>\n";

/// **The second store** (ADR-0162): its id and name. Its menu is its own,
/// and nothing changes it, so a change to store 47's is seen not to reach
/// it (§15.6 test 11).
const SECOND_STORE: (&str, &str) = ("48", "Harbor Coffee");

/// The second store's menu.
fn second_menu() -> Vec<(String, String)> {
    [
        ("drip", "Drip Coffee"),
        ("matcha", "Matcha Latte"),
        ("scone", "Blueberry Scone"),
    ]
    .map(|(id, name)| (id.to_string(), name.to_string()))
    .to_vec()
}

/// **An item's category in its store** (ADR-0181), by its id and its name:
/// store 47 lists every item, E7-P's too, under Coffee, so its moves stay
/// within one list; store 48's drinks and its bakery are two.
fn item_category(store: &str, id: &str) -> (&'static str, &'static str) {
    match (store, id) {
        (STORE_ID, _) => ("coffee", "Coffee"),
        (_, "scone") => ("bakery", "Bakery"),
        _ => ("drinks", "Drinks"),
    }
}

/// A store's name, by its id: the stores this server holds, as they are
/// first (`store.rs`'s seed).
fn store_named(id: &str) -> Option<&'static str> {
    match id {
        STORE_ID => Some(STORE_NAME),
        _ if id == SECOND_STORE.0 => Some(SECOND_STORE.1),
        _ => None,
    }
}

/// The value at `fields` in what a component returned, each a record's
/// field by its WIT name (`unit_price` is `unit-price`).
fn val_at<'a>(value: &'a Val, fields: &[&str]) -> Option<&'a Val> {
    fields.iter().try_fold(value, |v, field| match v {
        Val::Record(fs) => {
            let wit = field.replace('_', "-");
            fs.iter().find(|(n, _)| *n == wit).map(|(_, v)| v)
        }
        _ => None,
    })
}

/// The value at `fields` in what the renderer reads.
fn value_at_mut<'a>(value: &'a mut Value, fields: &[&str]) -> Option<&'a mut Value> {
    fields.iter().try_fold(value, |v, field| match v {
        Value::Record(fs) => fs.get_mut(*field),
        _ => None,
    })
}

/// A component value as the renderer reads it: a record by its Pleris field
/// names (the WIT's kebab case undone), an `Int` as an `Int`.
/// **What a `{#match}` arm binds of `value`, for the case `case`** (the
/// integrator's, 2026-10-13), by its WIT name as the plan names every case:
/// `some`'s, `ok`'s and `err`'s payload, or a declared case's, and of a case
/// of several fields the one at `field`; `None` for a value of another case,
/// whose arm is not shown. A value that is no case is the plan's fault.
fn payload_of(value: &Val, case: &str, field: Option<usize>) -> Result<Option<Val>, String> {
    let carried = match (value, case) {
        (Val::Option(Some(v)), "some") => Some((**v).clone()),
        (Val::Option(_), _) => None,
        (Val::Result(Ok(v)), "ok") | (Val::Result(Err(v)), "err") => v.as_deref().cloned(),
        (Val::Result(_), _) => None,
        (Val::Variant(name, v), _) if name == case => v.as_deref().cloned(),
        (Val::Variant(..), _) => None,
        (other, _) => return Err(format!("the case `{case}` read from {other:?}")),
    };
    Ok(match (field, carried) {
        (_, None) => None,
        (None, carried) => carried,
        (Some(i), Some(Val::Tuple(fields))) => Some(
            fields
                .get(i)
                .cloned()
                .ok_or_else(|| format!("`{case}` has no field {i}"))?,
        ),
        (Some(i), Some(other)) => {
            return Err(format!("field {i} of `{case}` read from {other:?}"));
        }
    })
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    /// **An arm's payload is its case's**, and a value of another case has
    /// none: the arm is not shown, and nothing is set for it.
    #[test]
    fn an_arm_reads_its_cases_payload_and_another_cases_value_has_none() {
        let some = Val::Option(Some(Box::new(Val::S64(1234))));
        assert_eq!(payload_of(&some, "some", None), Ok(Some(Val::S64(1234))));
        assert_eq!(payload_of(&some, "none", None), Ok(None));
        assert_eq!(payload_of(&Val::Option(None), "some", None), Ok(None));
        let ok = Val::Result(Ok(Some(Box::new(Val::String("paid".into())))));
        assert_eq!(
            payload_of(&ok, "ok", None),
            Ok(Some(Val::String("paid".into())))
        );
        assert_eq!(payload_of(&ok, "err", None), Ok(None));
        let err = Val::Result(Err(Some(Box::new(Val::S64(7)))));
        assert_eq!(payload_of(&err, "err", None), Ok(Some(Val::S64(7))));
        assert_eq!(payload_of(&err, "ok", None), Ok(None));
        let circle = Val::Variant("circle".into(), Some(Box::new(Val::Float64(2.0))));
        assert_eq!(
            payload_of(&circle, "circle", None),
            Ok(Some(Val::Float64(2.0)))
        );
        assert_eq!(payload_of(&circle, "square", None), Ok(None));
        // A case of several fields: the one at its place.
        let between = Val::Variant(
            "between".into(),
            Some(Box::new(Val::Tuple(vec![Val::S64(100), Val::S64(900)]))),
        );
        assert_eq!(
            payload_of(&between, "between", Some(1)),
            Ok(Some(Val::S64(900)))
        );
        assert_eq!(
            payload_of(&between, "between", Some(0)),
            Ok(Some(Val::S64(100)))
        );
        assert_eq!(payload_of(&between, "fixed", Some(1)), Ok(None));
        assert!(payload_of(&between, "between", Some(2)).is_err());
        // A value that is no case is the plan's fault, said.
        assert!(payload_of(&Val::S64(1), "some", None).is_err());
    }
}

fn val_to_value(v: &Val) -> Value {
    match v {
        Val::String(s) => Value::Text(s.clone()),
        Val::Bool(b) => Value::Bool(*b),
        Val::S64(n) => Value::Int(*n),
        Val::S32(n) => Value::Int(i64::from(*n)),
        Val::List(items) => Value::List(items.iter().map(val_to_value).collect()),
        Val::Record(fields) => Value::Record(
            fields
                .iter()
                .map(|(n, v)| (n.replace('-', "_"), val_to_value(v)))
                .collect(),
        ),
        // A case, as a `{#match}` arm names it (ADR-0146): `Some`, `None`,
        // `Ok` and `Err` as Pleris writes them, and a declared case by its
        // WIT name (ADR-0061). A payload of several fields is a list.
        Val::Variant(case, payload) => Value::Variant {
            case: case.clone(),
            payload: payload.as_deref().map(|v| Box::new(val_to_value(v))),
        },
        Val::Enum(case) => Value::Variant {
            case: case.clone(),
            payload: None,
        },
        Val::Option(v) => Value::Variant {
            case: if v.is_some() { "some" } else { "none" }.to_string(),
            payload: v.as_deref().map(|v| Box::new(val_to_value(v))),
        },
        Val::Result(r) => {
            let (case, v) = match r {
                Ok(v) => ("ok", v),
                Err(v) => ("err", v),
            };
            Value::Variant {
                case: case.to_string(),
                payload: v.as_deref().map(|v| Box::new(val_to_value(v))),
            }
        }
        Val::Tuple(items) => Value::List(items.iter().map(val_to_value).collect()),
        other => Value::Text(format!("{other:?}")),
    }
}

/// A value as a text part shows it, as the renderer writes it: `None` for a
/// list or a record, which a text part does not show.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t.clone()),
        Value::Int(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// **A part's address inside `frames`**, the loop instances around it,
/// outermost first (ADR-0181).
fn address_in(
    schema: &TemplateSchemaId,
    frames: &[(PartId, pw_document::InstanceToken)],
    part: PartId,
) -> PartAddress {
    frames.iter().fold(
        PartAddress::new(schema, LocalPartId(part.0)),
        |at, (each, instance)| at.within(LocalPartId(each.0), instance.clone()),
    )
}

/// **The patches that set an instance's changed parts where they are**
/// (ADR-0168): a text part's text, an attribute's value, and a list inside
/// the instance, each addressed inside `frames` (ADR-0181).
fn instance_patches(
    schema: &TemplateSchemaId,
    frames: &[(PartId, pw_document::InstanceToken)],
    changes: Vec<(PartId, pw_render::InstanceChange)>,
) -> Vec<Targeted> {
    let mut out = Vec::new();
    for (id, change) in changes {
        let target = address_in(schema, frames, id);
        match change {
            pw_render::InstanceChange::Text(text) => out.push(Targeted {
                target,
                operation: PatchOp::ReplaceText { text },
            }),
            pw_render::InstanceChange::Attribute {
                name,
                value: Some(value),
            } => out.push(Targeted {
                target,
                operation: PatchOp::SetAttribute { name, value },
            }),
            pw_render::InstanceChange::Attribute { name, value: None } => out.push(Targeted {
                target,
                operation: PatchOp::RemoveAttribute { name },
            }),
            pw_render::InstanceChange::List(changes) => {
                out.extend(list_change_patches(schema, frames, id, changes));
            }
        }
    }
    out
}

/// **The patches a keyed list's changes are** (ADR-0145, ADR-0181), the
/// list's own operations at its address inside `frames`, and an instance's
/// changes inside it.
fn list_change_patches(
    schema: &TemplateSchemaId,
    frames: &[(PartId, pw_document::InstanceToken)],
    each: PartId,
    changes: Vec<pw_render::ListChange>,
) -> Vec<Targeted> {
    let target = address_in(schema, frames, each);
    let at = |operation: PatchOp| Targeted {
        target: target.clone(),
        operation,
    };
    let mut out = Vec::new();
    for change in changes {
        match change {
            pw_render::ListChange::Remove(instance) => {
                out.push(at(PatchOp::RemoveInstance { instance }))
            }
            pw_render::ListChange::InsertAfter { after, html } => {
                out.push(at(PatchOp::InsertAfter {
                    instance: Some(after),
                    html,
                }))
            }
            pw_render::ListChange::InsertBefore { before, html } => {
                out.push(at(PatchOp::InsertBefore {
                    instance: before,
                    html,
                }))
            }
            pw_render::ListChange::Move { instance, after } => {
                out.push(at(PatchOp::MoveInstance { instance, after }))
            }
            pw_render::ListChange::Set { instance, changes } => {
                let mut inside = frames.to_vec();
                inside.push((each, instance));
                out.extend(instance_patches(schema, &inside, changes));
            }
        }
    }
    out
}

/// **A list's change, as keyed operations** (ADR-0145): the list the
/// template iterates over `list` becomes `new`, as the renderer derives it
/// (`pw_render::list_changes`), a list inside a row included (ADR-0181).
///
/// An item whose key stayed keeps its nodes, as E7-P keeps the menu's: a
/// focus, a scroll, and anything a test marked on it stay.
fn list_patches(
    template: &Template,
    schema: &TemplateSchemaId,
    list: &str,
    old: &[Value],
    new: &[Value],
    env: &Env,
    others: &[Template],
) -> Result<Vec<Targeted>, String> {
    let Some((each, _)) = each_over(&template.chunks, list) else {
        return Err(format!("`{}` iterates no list `{list}`", template.path));
    };
    let changes = pw_render::list_changes(template, each, old, new, env, others)
        .map_err(|b| format!("{b:?}"))?;
    Ok(list_change_patches(schema, &[], each, changes))
}

/// The `{#each}` over `list`: its part, and its key's path. Searched through
/// every block, as the renderer's parts are.
fn each_over(chunks: &[pw_render::Chunk], list: &str) -> Option<(PartId, String)> {
    for c in chunks {
        let pw_render::Chunk::Dynamic(p) = c else {
            continue;
        };
        if let pw_render::Part::Each {
            id,
            collection,
            key,
            ..
        } = p
            && collection == list
        {
            return Some((*id, key.clone().unwrap_or_default()));
        }
        for region in p.nested() {
            if let Some(found) = each_over(region, list) {
                return Some(found);
            }
        }
    }
    None
}

/// A component value as a speculation module decodes it (ADR-0122).
fn val_to_json(v: &Val) -> serde_json::Value {
    match v {
        Val::String(s) => serde_json::json!(s),
        Val::Bool(b) => serde_json::json!(b),
        Val::S64(n) => serde_json::json!(n),
        Val::S32(n) => serde_json::json!(n),
        Val::List(items) => serde_json::Value::Array(items.iter().map(val_to_json).collect()),
        Val::Record(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(n, v)| (n.replace('-', "_"), val_to_json(v)))
                .collect(),
        ),
        // A case as a handler's compiled code reads one (ADR-0157, `js_pure`'s
        // `decode`): its name as the WIT spells it, and its payload, several
        // fields as an array.
        Val::Result(r) => {
            let (case, payload) = match r {
                Ok(v) => ("ok", v),
                Err(v) => ("err", v),
            };
            case_json(case, payload.as_deref())
        }
        Val::Option(o) => match o {
            Some(v) => case_json("some", Some(v)),
            None => case_json("none", None),
        },
        Val::Variant(name, payload) => case_json(name, payload.as_deref()),
        Val::Enum(name) => case_json(name, None),
        Val::Tuple(items) => serde_json::Value::Array(items.iter().map(val_to_json).collect()),
        other => serde_json::json!(format!("{other:?}")),
    }
}

/// **A command's answer, for the handler that called it** (ADR-0157): `Ok`
/// without its value, which reaches the page from the resource, so the page
/// has one source for what it shows; or the declared `Err`, whole. A result
/// that is not a `Result` answers nothing.
fn answer_json(v: &Val) -> serde_json::Value {
    match v {
        Val::Result(Ok(_)) => serde_json::json!({ "$case": "ok" }),
        Val::Result(Err(e)) => case_json("err", e.as_deref()),
        _ => serde_json::Value::Null,
    }
}

/// A case on the wire: `{ "$case": name }`, and its payload as `value`.
fn case_json(name: &str, payload: Option<&Val>) -> serde_json::Value {
    match payload {
        Some(v) => serde_json::json!({ "$case": name, "value": val_to_json(v) }),
        None => serde_json::json!({ "$case": name }),
    }
}

fn default_menu() -> Vec<(String, String)> {
    [
        ("espresso", "Espresso"),
        ("cortado", "Cortado"),
        ("cold-brew", "Cold Brew"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

/// A list's rows, or none where the value is no list.
fn rows(v: &Value) -> &[Value] {
    match v {
        Value::List(rows) => rows,
        _ => &[],
    }
}

/// A row with its key alone: what an instance's token is derived from.
#[cfg(test)]
fn key_row(id: &str) -> Value {
    Value::Record([("id".to_string(), Value::Text(id.into()))].into())
}

/// **What each item is** (ADR-0166, charter §15.1), by its id: what the data
/// layer answers with an item, and what a menu's row shows under its name. An
/// item the table does not know, such as one E7-P inserts, is described by
/// nothing.
fn item_description(id: &str) -> &'static str {
    match id {
        "espresso" => "A double shot, pulled short.",
        "cortado" => "Espresso cut with an equal part of warm milk.",
        "cold-brew" => "Steeped for eighteen hours and served over ice.",
        "drip" => "Brewed to order, one cup at a time.",
        "matcha" => "Ceremonial matcha whisked with steamed milk.",
        "scone" => "Baked each morning with wild blueberries.",
        _ => "",
    }
}

/// **What each item costs, in cents** (ADR-0169, charter §15.1), by its id.
/// An item the table does not know, such as one E7-P inserts, costs $4.00.
fn item_price(id: &str) -> i64 {
    match id {
        "espresso" => 350,
        "cortado" => 425,
        "cold-brew" => 475,
        "drip" => 300,
        "matcha" => 525,
        "scone" => 375,
        _ => 400,
    }
}

/// **What each store says of itself** (ADR-0166, charter §15.1), by its id.
fn store_description(id: &str) -> &'static str {
    match id {
        STORE_ID => {
            "Small-batch coffee, served at the bar or carried out. The espresso changes \
             with the season, and the pastries come in every morning."
        }
        _ if id == SECOND_STORE.0 => {
            "A neighborhood cafe by the water, pouring drip coffee brewed to order and \
             matcha whisked by hand."
        }
        _ => "",
    }
}

/// The containment the large-menu case relies on (E7 gate item 10).
///
/// `content-visibility: auto` rather than virtualization: a menu is
/// semantically a list of independent items, so every item stays in the
/// document, findable and in the accessibility tree, and the ones off screen
/// are simply not laid out until they approach the viewport. Virtualization
/// removes items from the document, which is a correctness cost that has to be
/// justified per case rather than adopted as a default.
///
/// **Only an item that cannot be on screen as the page is first laid out**
/// (ADR-0187): one with 28 items before it in its list, or in a list with 16
/// lists before it. That is at least 2,400 CSS pixels down at 16 px text, and
/// leaves at most 448 items uncontained, whatever the menu. The browser
/// renders a contained item on screen in its first frame anyway, and what
/// follows the item then moves: that is reported as a layout shift though
/// it is never painted. Until 2026-10-04 every item was contained, and the
/// cart moved 249 px on a phone.
///
/// `contain-intrinsic-size` is not optional with it: without a placeholder
/// size the scrollbar jumps as items are realized, which is the visible defect
/// that makes people abandon containment and reach for virtualization. It is
/// an item's height. Until 2026-10-04 it was 42 px, and items had grown to
/// 125.
const STYLE: &str = "#menu > ul > li:nth-child(n+29), #menu > ul:nth-of-type(n+17) > li \
                     { content-visibility: auto; contain-intrinsic-size: auto 7.75em; }";

/// **An event as the outbox keeps it** (ADR-0208): its declaration's path,
/// and each value the command computed, as a key's text. A key is a
/// `String`, an `Int` or a `Bool` (PW5308); a value of another type keys no
/// entry, and is refused rather than written.
fn outboxed(event: &str, values: &[Val]) -> Result<pw_materialize::Event, String> {
    let texts = values
        .iter()
        .map(|v| match v {
            Val::String(s) => Ok(s.clone()),
            Val::S64(n) => Ok(n.to_string()),
            Val::Bool(b) => Ok(b.to_string()),
            other => Err(format!("`{event}` carries {other:?}, which keys no entry")),
        })
        .collect::<Result<Vec<String>, String>>()?;
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    Ok(pw_materialize::Event::new(event, &texts))
}

/// The graph, as `pw emit-graph` produced it. Read at build time so the server
/// cannot drift from the compiler's answer between runs.
#[cfg(test)]
const GRAPH: &str = include_str!("../../../../runtime/pw-materialize/tests/store-graph.json");

fn main() {
    let dist = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dist".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3143);

    // What `pw build` wrote (ADR-0123): the second argument, or `build/`
    // beside the documents, where `run.sh` writes it.
    let build = std::env::args()
        .nth(2)
        .unwrap_or_else(|| format!("{dist}/build"));
    // The feed's data in PostgreSQL, where the deployment names a database
    // (ADR-0246); in memory otherwise. Its URL is the environment's, never
    // the repository's. `PW_FEED_TRANSACTIONS=connection` leaves a command's
    // isolation to the database's default, which the host then measures.
    let isolation = match std::env::var("PW_FEED_TRANSACTIONS").as_deref() {
        Ok("connection") => feed_pg::Isolation::ConnectionDefault,
        _ => feed_pg::Isolation::Serializable,
    };
    // TRACK SEAM (render-merge): **how many renders read a layer's
    // database at once, and how long one waits for its seat**, the
    // deployment's to say (`PW_RENDER_SEATS`, `PW_RENDER_SEAT_WAIT` in
    // seconds; eight and thirty where it says nothing: the integrator's
    // answer of 2026-10-17), for each layer on PostgreSQL, and refused at
    // start where it cannot be read.
    let seats = feed_pg::RenderSeats::stated(
        std::env::var("PW_RENDER_SEATS").ok().as_deref(),
        std::env::var("PW_RENDER_SEAT_WAIT").ok().as_deref(),
    )
    .unwrap_or_else(|e| {
        eprintln!("pw dev server: {e}");
        std::process::exit(1);
    });
    // TRACK SEAM (second-host): the schema a test run's hosts keep the
    // feed's tables in (`PW_FEED_SCHEMA`, ADR-0369): a test's setting, not a
    // deployment's (the integrator's ruling 4 of 2026-10-20), honored only
    // for a development deployment on this machine and refused at start for
    // any other; the connection's own search path otherwise, as before.
    let feed_schema = identity::Deployment::from_env(port)
        .and_then(|d| hosts::test_schema(std::env::var("PW_FEED_SCHEMA").ok().as_deref(), &d))
        .unwrap_or_else(|e| {
            eprintln!("pw dev server: {e}");
            std::process::exit(1);
        });
    let feed = match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => {
            match feed_pg::FeedPg::open_with(&url, feed_schema.as_deref(), isolation, seats) {
                Ok(layer) => Some(Arc::new(layer) as Arc<dyn data::DataLayer>),
                Err(e) => {
                    eprintln!("pw dev server: PW_FEED_DATABASE_URL: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => None,
    };
    // **The store's data in PostgreSQL** (track `store-pg`), where the
    // deployment names a database, in its own schema, `pw_store`; in memory
    // otherwise. Its command's isolation is set as the feed's is.
    let store = match std::env::var("PW_STORE_DATABASE_URL") {
        Ok(url) if !url.is_empty() => {
            match store_pg::StorePg::open_with(&url, store_pg::SCHEMA, isolation, seats) {
                Ok(layer) => Some(Arc::new(layer) as Arc<dyn store::StoreLayer>),
                Err(e) => {
                    eprintln!("pw dev server: PW_STORE_DATABASE_URL: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => None,
    };
    let server = match Server::from_build_layers(dist.into(), build.into(), feed, store) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("pw dev server: {e}\nrun spikes/own-renderer/run.sh, which runs `pw build`");
            std::process::exit(1);
        }
    };
    let missing = server.uncompiled_handlers();
    if !missing.is_empty() {
        eprintln!(
            "pw dev server: no compiled module for handler(s) {} under {}/handlers; \
             run spikes/own-renderer/run.sh, which runs `pw emit-handlers`",
            missing.join(", "),
            server.dist.display()
        );
        std::process::exit(1);
    }
    // TRACK SEAM (identity): the deployment's identity, as its configuration
    // states it, refused at start where it may not run.
    let deployment = identity::Deployment::from_env(port)
        .and_then(|d| {
            identity::configure_cookies(&d);
            let choice = std::env::var("PW_IDENTITY").ok();
            server.identity.configure(&d, choice.as_deref()).map(|_| d)
        })
        .unwrap_or_else(|e| {
            eprintln!("pw dev server: {e}");
            std::process::exit(1);
        });
    // TRACK SEAM (second-host): **how long a session lives** (ADR-0369, the
    // integrator's ruling 2 of 2026-10-20), the deployment's to say
    // (`PW_SESSION_IDLE`, `PW_SESSION_LIFETIME`, in seconds; fourteen days
    // and sixty where it says nothing), refused at start where it cannot be
    // read.
    match identity::SessionLifetimes::stated(
        std::env::var("PW_SESSION_IDLE").ok().as_deref(),
        std::env::var("PW_SESSION_LIFETIME").ok().as_deref(),
    ) {
        Ok(lifetimes) => server.identity.principals().expire_after(lifetimes),
        Err(e) => {
            eprintln!("pw dev server: {e}");
            std::process::exit(1);
        }
    }
    // NEXT 6c: the deployment's payments, as its configuration states them,
    // refused at start where they may not run.
    match payments::configure(
        deployment.development && deployment.loopback(),
        &deployment.origin,
        port,
        std::env::var("PW_PAYMENTS").ok().as_deref(),
        std::env::var("PW_PAYMENTS_PORT").ok().as_deref(),
    ) {
        Ok(Some(p)) => {
            println!("pw dev server payments: {}", p.describe());
            let _ = server.payments.set(p);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("pw dev server: {e}");
            std::process::exit(1);
        }
    }
    // A page's canonical address is written with the public origin
    // (ADR-0311), or the server does not start.
    let stated = std::env::var("PW_ORIGIN").is_ok_and(|o| !o.is_empty());
    match canonical_origin(&server, &deployment, stated) {
        Ok(origin) => {
            let _ = server.origin.set(origin);
        }
        Err(e) => {
            eprintln!("pw dev server: {e}");
            std::process::exit(1);
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("pw dev server on {port}");
    // TRACK SEAM (identity): which identity model runs.
    println!(
        "pw dev server identity: {} ({})",
        server.identity.describe(),
        deployment.origin
    );
    // TRACK SEAM (uploads, ADR-0253): what it holds a browser's file to.
    if let Some(uploads) = server.uploads.describe() {
        println!("pw dev server uploads: {uploads}");
    }

    // **Each live document read again when what it shows holds no longer**
    // (ADR-0304): the host's timer.
    {
        let server = server.clone();
        std::thread::spawn(move || server.tell_when_time_passes());
    }
    // **Each scheduled run, run once the clock passes its time** (the
    // scheduled-commands track).
    {
        let server = server.clone();
        std::thread::spawn(move || server.run_when_due());
    }
    // **Every other host's commits, told to this host's pages** (ADR-0369,
    // track `second-host`), where its data layer's database is shared.
    match server.data.tail() {
        Some(tail) => {
            println!(
                "pw dev server hosts: {} on a shared database, schema {}, hearing every \
                 host's commits",
                tail.host(),
                tail.outbox().schema
            );
            let server = server.clone();
            std::thread::spawn(move || server.hear_other_hosts());
        }
        None => println!("pw dev server hosts: one; its data layer shares no database"),
    }

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = server.clone();
        // A thread per connection. The subscription blocks, and a
        // single-threaded loop would make the first subscriber the last.
        std::thread::spawn(move || handle(&server, stream));
    }
}

/// **A connection's request answered, and then what its commit dropped told
/// to every other session that reads it** (ADR-0219): after the connection
/// is closed, so the author is answered without waiting for every reader.
fn handle(server: &Server, stream: TcpStream) {
    answer_connection(server, stream);
    server.tell_waiting();
}

fn answer_connection(server: &Server, mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut parts = request.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));

    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    let (session, fresh) = session_of(&headers);
    // TRACK SEAM (second-host): the session as the hosts keep it, read once
    // for the request (ADR-0369): signed in or out on another host, it is so
    // here too.
    if !fresh {
        server.identity.principals().refresh(&session);
    }

    // TRACK SEAM (uploads, ADR-0253): an upload reads its own body, within
    // its own limits, before the bound a command's body is held to.
    let route = path.split('?').next().unwrap_or("/");
    if server.uploads.claims(method, route) {
        server.uploads.answer(
            method,
            route,
            &headers,
            &session,
            fresh,
            &mut reader,
            &mut stream,
        );
        return;
    }

    // The body, when the request declares one. Bounded: a command's arguments
    // are a few values, and a length nothing checks is an allocation anyone
    // can ask for.
    const MAX_BODY: usize = 64 * 1024;
    let length = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    if length > MAX_BODY {
        respond(
            &mut stream,
            413,
            "text/plain; charset=utf-8",
            &session,
            fresh,
            b"request body too large",
        );
        return;
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }

    // NEXT 6c: the provider's webhook, held to its signature. It comes from
    // the provider, not this origin, and carries no session.
    if method == "POST" && route == "/payments/webhook" {
        let (code, said) = match server.payments.get() {
            Some(p) => p.heard(
                payments::header_value(&headers, "stripe-signature"),
                &body,
                &|settled| server.settle_payment(settled),
            ),
            None => (404, "this deployment takes no payments".to_string()),
        };
        respond_build(
            &mut stream,
            code,
            "text/plain; charset=utf-8",
            said.as_bytes(),
        );
        return;
    }

    // TRACK SEAM (identity, ADR-0253): the identity track's routes, before
    // any other, and a request that changes something refused unless it
    // comes from this origin.
    let asked = path.split_once('?').map_or("", |(_, q)| q);
    if server.identity.answer(
        method,
        route,
        &headers,
        &session,
        fresh,
        &body,
        &mut stream,
        asked,
        // TRACK SEAM (store-accounts): a guest signed in, its cart joining
        // its user's before the sign-in is answered.
        &|signed| server.joined(signed),
    ) {
        return;
    }
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    // The page a path names by its route, and the parameters it gives
    // (ADR-0162).
    let routed = match method {
        "GET" => server.routed(route, query),
        _ => None,
    };
    // A query a page's route reads that is no text, as the address says it,
    // is refused, never replaced (ADR-0309).
    let routed = match routed {
        Some(Err(why)) => {
            respond(
                &mut stream,
                400,
                "text/plain; charset=utf-8",
                &session,
                fresh,
                why.as_bytes(),
            );
            return;
        }
        Some(Ok(matched)) => Some(matched),
        None => None,
    };

    match (method, route) {
        // **A command the compiler built, by its component id**: what a
        // compiled handler calls (`context.command(id, args)` in
        // `dist/handlers/<identity>.mjs`). The body is the arguments, as a JSON
        // array, typed here by the component's own parameters; the guard admits
        // only components this server hosts, so an unknown id falls through to
        // the 404 below rather than to a default. A command the platform
        // invokes (NEXT 6c, `invoked_by Payments`) is not a reader's to press,
        // and falls through with them, as a Convex client's call of an
        // internal function finds no public one.
        ("POST", route)
            if route.strip_prefix("/command/").is_some_and(|id| {
                server.components.contains_key(id) && !server.invoked_by_platform(id)
            }) =>
        {
            let id = &route["/command/".len()..];
            let args = match serde_json::from_slice::<Vec<serde_json::Value>>(&body) {
                Ok(args) => args,
                Err(e) => {
                    let why =
                        serde_json::Value::String(format!("the body is not a JSON array: {e}"));
                    respond_json(
                        &mut stream,
                        400,
                        &session,
                        fresh,
                        &format!("{{\"committed\":false,\"error\":{why}}}"),
                    );
                    return;
                }
            };
            // The interaction this request belongs to, sent by the runtime
            // once per press and kept by a retry (ADR-0121).
            let header = |name: &str| {
                headers.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim()
                        .eq_ignore_ascii_case(name)
                        .then(|| v.trim().to_string())
                })
            };
            let interaction = header("pw-interaction");
            // Here for the order (ADR-0305), whatever its answer: a command
            // refused below is answered, and what follows it may run.
            let arrival = interaction
                .as_deref()
                .map(|i| server.order.arrive(&session, i));
            // TRACK SEAM (store-accounts): **a command is answered for the
            // reader its page was shown to** (Q3, a rule of the platform).
            // The runtime names its document (`pw-document`); one this host
            // served to another session, the reader having signed in or out
            // in another tab, or one it no longer holds, is refused before
            // the command runs, in the platform's words, as a `requires`
            // refusal is told (ADR-0302). A command from no page names none
            // and is answered as before.
            // **A press the host already answered is answered from what it
            // kept, first** (the integrator's finding of 2026-10-11): a retry
            // of a press whose answer was lost after it committed (ADR-0175)
            // is told it committed, though the page went stale between the
            // two attempts. The rule governs only commands not yet run, and
            // the order's wait (ADR-0305) only those that pass it.
            let stale = header("pw-document")
                .and_then(|v| v.parse::<u64>().ok())
                .and_then(|document| {
                    server
                        .stale_document(&session, document)
                        .map(|why| (document, why))
                });
            let kept = stale.as_ref().and_then(|(document, _)| {
                server.kept_answer(&session, *document, id, &args, interaction.as_deref())
            });
            if kept.is_none()
                && let Some((_, (refused, words))) = stale
            {
                let refused = serde_json::json!({
                    "committed": false,
                    "refused": refused,
                    "says": words,
                });
                respond_json(&mut stream, 403, &session, fresh, &refused.to_string());
                return;
            }
            // **After the one its page sent before it** (ADR-0305): run once
            // that one has, whichever request the network brought first;
            // answered early, running nothing, where it does not come.
            if kept.is_none()
                && let Some(after) = header("pw-after")
                && server.order.follow(&session, &after) == order::Turn::Early
            {
                if let Some(arrival) = arrival {
                    arrival.early();
                }
                let early =
                    serde_json::json!({ "committed": false, "early": true, "after": after });
                respond_json(&mut stream, 409, &session, fresh, &early.to_string());
                return;
            }
            // Charter §15.5's one-shot network error (ADR-0175): the
            // connection closed with no answer, before the command runs, or
            // after it has committed.
            let drop_at = server.take_command_drop(&session);
            if drop_at == Some(DropAt::Before) {
                // Never run: what follows it waits for it to be sent again.
                if let Some(arrival) = arrival {
                    arrival.vanish();
                }
                return;
            }
            // ADR-0381's fault: the connection held open with no answer, or
            // with an answer begun and never finished, until its page gives
            // up on it.
            let hold_at = server.take_command_hold(&session);
            if hold_at == Some(HoldAt::Before) {
                // Never run, as a request that never came: what follows it
                // waits for it to be sent again.
                if let Some(arrival) = arrival {
                    arrival.vanish();
                }
                hold_open(&mut stream);
                return;
            }
            // TRACK SEAM (identity): a `requires` refusal is answered as its
            // own case, 403 and the predicate, never shaped like a declared
            // error. Taken before the command, so none is left from another.
            let _ = identity::take_refusal();
            let answer = match kept {
                Some(kept) => Ok(kept),
                None => server.command_answer(id, &session, &args, interaction.as_deref()),
            };
            if drop_at == Some(DropAt::After) {
                return;
            }
            if hold_at == Some(HoldAt::After) {
                // Run, and what follows it may run; its answer never begun.
                drop(arrival);
                hold_open(&mut stream);
                return;
            }
            // TRACK SEAM (identity): the refusal, where `requires` refused,
            // kept with the interaction (ADR-0302), and what a reader is
            // told of it: the program's words for the predicate where it
            // declares them, the deployment's where it does not. Never
            // what the predicate read.
            if let Ok(Answered {
                refused: Some(predicate),
                ..
            }) = &answer
            {
                let says = server
                    .words
                    .get(predicate)
                    .map(String::as_str)
                    .or_else(|| identity::says(predicate))
                    .unwrap_or_default();
                let refused = serde_json::json!({
                    "committed": false,
                    "refused": predicate,
                    "says": says,
                });
                respond_json(&mut stream, 403, &session, fresh, &refused.to_string());
                return;
            }
            match answer {
                // A malformed request: nothing ran.
                Err(e) => {
                    let why = serde_json::Value::String(e);
                    respond_json(
                        &mut stream,
                        400,
                        &session,
                        fresh,
                        &format!("{{\"committed\":false,\"error\":{why}}}"),
                    );
                }
                Ok(answered) => {
                    if let Some(why) = &answered.why
                        && std::env::var("PW_TRACE").is_ok()
                    {
                        eprintln!("{id}: {why}");
                    }
                    // A commit says which versions it produced (ADR-0122), so a
                    // page can tell when the value it is sent includes it. A
                    // version, not a value: the value reaches the page from the
                    // resource, as every change does.
                    let mut body = serde_json::json!({ "committed": answered.committed });
                    if answered.committed {
                        body["basis"] = server.committed_basis(&session);
                    }
                    // And what the command answered, for the handler that
                    // called it (ADR-0157): `Ok`, or the declared `Err` it can
                    // tell the customer about.
                    if let Some(result) = answered.result {
                        body["result"] = result;
                    }
                    if hold_at == Some(HoldAt::Body) {
                        // Run, and its answer begun and never finished.
                        drop(arrival);
                        respond_in_part(&mut stream, 202, &session, fresh, &body.to_string());
                        hold_open(&mut stream);
                        return;
                    }
                    respond_json(&mut stream, 202, &session, fresh, &body.to_string());
                }
            }
        }
        ("POST", "/command/menu_changed") => {
            // An event nothing about the cart listens for. It reaches the
            // materializer and selects no cart entry.
            let _ = server.materializer.command::<String>(|_tx| {
                Ok(vec![pw_materialize::Event::new(
                    "Events.MenuChanged",
                    &["47"],
                )])
            });
            server.drain(&session);
            respond_json(&mut stream, 202, &session, fresh, "{}");
        }
        // **The kitchen sets a session's order** (E14, T04): a benchmark's
        // hook, as every stack in it has one. `?status=preparing` sets the
        // status's case; no status removes the order. The page reads it again
        // when it is next served.
        // **The host's clock, as a test moves it** (ADR-0304): forward
        // only, as time goes, to the instant `at` or `advance` milliseconds
        // on. Every document that shows what holds no longer is told.
        ("GET", "/bench/clock") => {
            let now = serde_json::json!({ "now": server.now() });
            respond_json(&mut stream, 200, &session, fresh, &now.to_string());
        }
        ("POST", "/bench/clock") => {
            let read = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(k)?.strip_prefix('='))
                    .and_then(|v| v.parse::<i64>().ok())
            };
            let now = server.now();
            let ahead = match (read("at"), read("advance")) {
                (Some(at), None) => Some(at - now),
                (None, Some(ms)) => Some(ms),
                _ => None,
            };
            match ahead.and_then(|ms| u64::try_from(ms).ok()) {
                Some(ms) => {
                    server.query_clock.0.advance(ms);
                    server.timers.changed.notify_all();
                    let now = serde_json::json!({ "now": server.now() });
                    respond_json(&mut stream, 200, &session, fresh, &now.to_string());
                }
                None => respond_json(
                    &mut stream,
                    400,
                    &session,
                    fresh,
                    "{\"error\":\"`at` (an instant not passed) or `advance` (milliseconds not below 0)\"}",
                ),
            }
        }
        ("POST", "/bench/order") => {
            let status = query
                .split('&')
                .find_map(|p| p.strip_prefix("status="))
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            // Through the store's data layer, with its event, in one
            // transaction (track `store-pg`). The order moved along keeps
            // its lines (ADR-0193); one the kitchen sets with none placed
            // has none.
            // The order the session's reader reads (track `store-accounts`):
            // its user's, where the program reads it by the reader's handle.
            let owner = server.owner(&session);
            let event = (
                server.order_event().to_string(),
                vec![Val::String(owner.clone())],
            );
            let status = Val::Option(status.map(|s| Box::new(Val::String(s))));
            let written = server.store_write(
                &session,
                "store:host/orders#set-status",
                &[Val::String(owner), status],
                std::slice::from_ref(&event),
                // And the session's open pages told (ADR-0193).
                |_| {
                    server.session_changed(
                        &session,
                        server.order_event(),
                        &server.order_entry(&session),
                    )
                },
            );
            match written {
                Ok(_) => respond_json(&mut stream, 200, &session, fresh, "{}"),
                Err(why) => refused_json(&mut stream, &session, fresh, why),
            }
        }
        // The recommender a test sets (ADR-0148): how long it takes, how it
        // fails (`declared` or `host`), and what it recommends, `id:Name`
        // pairs. Nothing kept from before is served after: what a page reads
        // next is what the recommender now says.
        ("POST", "/bench/recommendations") => {
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
                    .and_then(percent_decoded)
            };
            let mut set = Recommender::default();
            if let Some(ms) = q("delay").and_then(|v| v.parse().ok()) {
                set.delay_ms = ms;
            }
            if q("hold").as_deref() == Some("1") {
                set.gate = Some(Arc::new(Gate::default()));
            }
            set.fail = q("fail").filter(|f| !f.is_empty());
            // What it recommends is the store's data, curated through the
            // layer (track `store-pg`); none curated draws from each store's
            // menu again.
            let items: Option<Vec<(String, String)>> = q("items").map(|items| {
                items
                    .split(',')
                    .filter_map(|pair| pair.split_once(':'))
                    .map(|(id, name)| (id.to_string(), name.to_string()))
                    .collect()
            });
            if let Err(why) = server.curate(items.as_deref()) {
                return refused_json(&mut stream, &session, fresh, why);
            }
            // A read held as it was is let go (ADR-0223): a test that set the
            // recommender again has done with it.
            let was = std::mem::replace(
                &mut *server
                    .store
                    .faults()
                    .recommender
                    .lock()
                    .expect("recommender"),
                set,
            );
            if let Some(gate) = was.gate {
                gate.open();
            }
            // Every query a page of this build reads: what it kept, and what
            // is still running with the recommender as it was.
            let resources: std::collections::BTreeSet<String> = server
                .plans
                .values()
                .flat_map(|plan| {
                    ["bindings", "streams"].into_iter().flat_map(move |k| {
                        plan[k]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|b| b["resource"].as_str().map(str::to_string))
                    })
                })
                .collect();
            for resource in resources {
                server.queries.invalidate(&resource);
            }
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **Each streamed region's fill written in two parts** (ADR-0223),
        // `?ms=` apart; `0` writes each whole again.
        ("POST", "/bench/split-fills") => {
            let ms = query
                .split('&')
                .find_map(|p| p.strip_prefix("ms="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            server
                .split_fills
                .store(ms, std::sync::atomic::Ordering::SeqCst);
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **The recommender's held reads, let go** (ADR-0223), its settings
        // kept.
        ("POST", "/bench/recommendations/release") => {
            if let Some(gate) = &server
                .store
                .faults()
                .recommender
                .lock()
                .expect("recommender")
                .gate
            {
                gate.open();
            }
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **What happened to a document's frames** (the WebKit "Load more"
        // flake, NEXT): its trail and the table's slow holds, for a failing
        // browser test to attach beside the runtime's record. Only the
        // session's own documents.
        ("GET", "/bench/records") => {
            let document = query
                .split('&')
                .find_map(|p| p.strip_prefix("doc="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let doc: Doc = (session.clone(), document);
            let trail: Vec<String> = server
                .pending
                .lock()
                .expect("pending")
                .get(&doc)
                .map(|w| w.trail.iter().cloned().collect())
                .unwrap_or_default();
            let slow: Vec<String> = server.slow.lock().expect("slow").iter().cloned().collect();
            let tellings: Vec<String> = server
                .tellings
                .lock()
                .expect("tellings")
                .iter()
                .cloned()
                .collect();
            let body = serde_json::json!({
                "at": uptime_ms() as u64,
                "doc": document,
                "you": masked(&session),
                "trail": trail,
                "slow": slow,
                "tellings": tellings,
            });
            respond_json(&mut stream, 200, &session, fresh, &body.to_string());
        }
        // The store's staff post a notice (E14, T09): at the source, and
        // nothing is told. What a page shows of it is the `Notice` query's
        // to keep, for as long as its freshness says.
        // TRACK SEAM (store-hours): a store's hours, every day alike (a test's).
        ("POST", "/bench/hours") => match crate::hours::bench(server, query) {
            Ok(()) => respond_json(&mut stream, 200, &session, fresh, "{}"),
            Err(why) => refused_json(&mut stream, &session, fresh, why),
        },
        ("POST", "/bench/notice") => {
            let text = query
                .split('&')
                .find_map(|p| p.strip_prefix("text="))
                .and_then(percent_decoded)
                .unwrap_or_default();
            // Through the layer (track `store-pg`), and still nothing told.
            match server.store_write(
                &session,
                "store:host/notices#post",
                &[Val::String(STORE_ID.into()), Val::String(text)],
                &[],
                |_| {},
            ) {
                Ok(_) => respond_json(&mut stream, 200, &session, fresh, "{}"),
                Err(why) => refused_json(&mut stream, &session, fresh, why),
            }
        }
        // **A binding read again, for the key the browser asks for**
        // (ADR-0152): `?binding=..&seq=..&doc=..&key=<JSON>`. The patch set
        // comes in the session's frames; this says whether it was applied.
        ("GET", "/pw-read") => {
            let param = |name: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{name}=") as &str))
                    .and_then(percent_decoded)
            };
            let number = |name: &str| param(name).and_then(|v| v.parse::<u64>().ok());
            let key: Option<BTreeMap<String, serde_json::Value>> =
                param("key").and_then(|k| serde_json::from_str(&k).ok());
            let (Some(binding), Some(seq), Some(document), Some(key)) =
                (param("binding"), number("seq"), number("doc"), key)
            else {
                return respond_json(&mut stream, 400, &session, fresh, "{}");
            };
            // The browser has left when its connection is closed: a read
            // finds nothing more to read, and nothing waiting.
            let watched = stream.try_clone().ok();
            if let Some(w) = &watched {
                let _ = w.set_nonblocking(true);
            }
            let left = move || {
                watched
                    .as_ref()
                    .is_some_and(|w| matches!(w.peek(&mut [0u8; 1]), Ok(0)))
            };
            let outcome = server.read_keyed(&session, &binding, seq, document, &key, &left);
            // The watch made the connection nonblocking; the answer is
            // written as every other is.
            let _ = stream.set_nonblocking(false);
            match outcome {
                Ok(outcome) => {
                    let body = match outcome {
                        KeyOutcome::Applied => serde_json::json!({ "applied": seq }),
                        KeyOutcome::Superseded => serde_json::json!({ "superseded": seq }),
                    };
                    respond_json(&mut stream, 200, &session, fresh, &body.to_string());
                }
                Err(why) => {
                    if std::env::var("PW_TRACE").is_ok() {
                        eprintln!("pw-read {binding}: {why}");
                    }
                    respond_json(&mut stream, 400, &session, fresh, "{}");
                }
            }
        }
        // An item sold out at the source, or back in stock (charter §15.5's
        // forced stale item): `?item=cortado&available=false`. A page that
        // shows it is not told; the next add is refused by name.
        ("POST", "/bench/stock") => {
            let param = |name: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{name}=") as &str))
                    .and_then(percent_decoded)
            };
            let Some(item) = param("item") else {
                return respond_json(&mut stream, 400, &session, fresh, "{}");
            };
            let available = param("available").as_deref() != Some("false");
            // Told (ADR-0178): `InventoryChanged(47, item)`, written with
            // the change, and every page that shows the store sees the item
            // as it is now. Untold, it is §15.5's forced stale item: a page
            // shows what it was, and the command that adds it refuses it
            // (ADR-0157). Either way through the layer (track `store-pg`).
            let changed = if param("tell").as_deref() == Some("true") {
                server.broadcast_menu(MenuOp::Stock {
                    id: item,
                    available,
                })
            } else {
                server
                    .store_write(
                        &session,
                        "store:host/menus#stock",
                        &[Val::String(item), Val::Bool(available)],
                        &[],
                        |_| {},
                    )
                    .map(|_| ())
            };
            if let Err(e) = changed {
                let why = serde_json::Value::String(e);
                respond_json(
                    &mut stream,
                    409,
                    &session,
                    fresh,
                    &format!("{{\"refused\":{why}}}"),
                );
                return;
            }
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // Which of the menu's categories is slow, and how slow (E14, T07):
        // `?slow=hot&delay=1500`. Nothing is told.
        ("POST", "/bench/category") => {
            let param = |name: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{name}=") as &str))
                    .and_then(percent_decoded)
            };
            *server.store.faults().categories.lock().expect("categories") = Categories {
                slow: param("slow"),
                delay_ms: param("delay").and_then(|d| d.parse().ok()).unwrap_or(0),
                gate: None,
            };
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // The kitchen's prep time changes (E14, T02): at the source, and
        // nothing is told. A page reads it as its query's policies say.
        ("POST", "/bench/prep") => {
            match query
                .split('&')
                .find_map(|p| p.strip_prefix("minutes="))
                .and_then(|m| m.parse::<i64>().ok())
            {
                // Through the layer (track `store-pg`), and nothing told.
                Some(minutes) => match server.store_write(
                    &session,
                    "store:host/kitchen#set-prep",
                    &[Val::String(STORE_ID.into()), Val::S64(minutes)],
                    &[],
                    |_| {},
                ) {
                    Ok(_) => respond_json(&mut stream, 200, &session, fresh, "{}"),
                    Err(why) => refused_json(&mut stream, &session, fresh, why),
                },
                None => respond_json(&mut stream, 400, &session, fresh, "{}"),
            }
        }
        // How many times each slow source was asked (E14, T02 and T09), as
        // every stack in the benchmark reports it.
        ("GET", "/bench/calls") => {
            let calls = server.calls.lock().expect("calls");
            let count = |op: &str| calls.get(op).copied().unwrap_or(0);
            let body = serde_json::json!({
                "notice": count("store:data/notices#current"),
                "prep": count("store:data/kitchen#prep-minutes"),
                "category": count("store:data/menus#in-category"),
                "category_stopped": server.store
                    .faults()
                    .category_stopped
                    .load(std::sync::atomic::Ordering::SeqCst),
                // The store's origin, asked (ADR-0177).
                "store": count("store:data/stores#get"),
            });
            respond_json(&mut stream, 200, &session, fresh, &body.to_string());
        }
        // How the estimator behaves for the request's session (E14, T10): how
        // long it takes, how it fails, and what it estimates. What was kept
        // for the session is not served after.
        // **The store's delay** (charter §15.5, ADR-0174): how long the data
        // layer takes to answer for a store, for every reader, as the store is
        // one value for all of them. What any page kept of a store is read
        // again, so the next reader waits.
        ("POST", "/bench/store") => {
            // **The store's origin, failing once** (ADR-0177): its next read
            // fails, for every reader, and what was kept of it is expired
            // and kept, so the next reader asks the origin, and a fallback
            // has the last value to answer with.
            if query.split('&').any(|p| p == "fail=next") {
                server
                    .store
                    .faults()
                    .store_fails_next
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                // The store page's plan and every page's: a server built
                // from templates alone has the first.
                for resource in std::iter::once(&server.plan)
                    .chain(server.plans.values())
                    .flat_map(|plan| {
                        plan["bindings"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|b| b["resource"].as_str().map(str::to_string))
                    })
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    server.queries.expire(&resource);
                }
                respond_json(&mut stream, 200, &session, fresh, "{}");
                return;
            }
            let delay = query
                .split('&')
                .find_map(|p| p.strip_prefix("delay="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            server
                .store
                .faults()
                .store_delay_ms
                .store(delay, std::sync::atomic::Ordering::SeqCst);
            let resources: std::collections::BTreeSet<String> = std::iter::once(&server.plan)
                .chain(server.plans.values())
                .flat_map(|plan| {
                    plan["bindings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|b| b["resource"].as_str().map(str::to_string))
                })
                .collect();
            for resource in resources {
                server.queries.invalidate(&resource);
            }
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **This session's cart delay** (charter §15.5, ADR-0174): how long a
        // read of its cart takes. Another session's does not wait, and what
        // this session's pages kept is read again.
        ("POST", "/bench/cart") => {
            let delay = query
                .split('&')
                .find_map(|p| p.strip_prefix("delay="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            server
                .store
                .faults()
                .cart_faults
                .lock()
                .expect("cart faults")
                .entry(session.clone())
                .or_default()
                .delay_ms = delay;
            let own = format!("session={session}");
            server
                .queries
                .evict_where(|k| k.key == own || k.key.starts_with(&format!("{own}\u{1f}")));
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **A one-shot database error** (charter §15.5, ADR-0174): this
        // session's next cart write, or its next cart read, fails as a
        // database that is down does. Once: the one after it succeeds. Until
        // 2026-10-04 `/command/add_and_fail` ran an add of its own, which no
        // page's press met.
        ("POST", "/bench/fail") => {
            let next = query.split('&').find_map(|p| p.strip_prefix("next="));
            let mut faults = server
                .store
                .faults()
                .cart_faults
                .lock()
                .expect("cart faults");
            let mine = faults.entry(session.clone()).or_default();
            match next {
                Some("write") => mine.fail_write = true,
                Some("read") => mine.fail_read = true,
                _ => {
                    drop(faults);
                    respond_json(
                        &mut stream,
                        400,
                        &session,
                        fresh,
                        "{\"error\":\"next is `write` or `read`\"}",
                    );
                    return;
                }
            }
            drop(faults);
            // A read the page kept would not reach the database.
            let own = format!("session={session}");
            server
                .queries
                .evict_where(|k| k.key == own || k.key.starts_with(&format!("{own}\u{1f}")));
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **A one-shot network error** (charter §15.5, ADR-0175): this
        // session's next command connection is closed with no answer,
        // before the command runs (`at=before`) or after it has committed
        // (`at=after`). Once.
        ("POST", "/bench/drop") => {
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
            };
            let at = match (q("next"), q("at")) {
                (Some("command"), Some("before")) => DropAt::Before,
                (Some("command"), Some("after")) => DropAt::After,
                _ => {
                    respond_json(
                        &mut stream,
                        400,
                        &session,
                        fresh,
                        "{\"error\":\"next is `command`, and at is `before` or `after`\"}",
                    );
                    return;
                }
            };
            server
                .connection_faults
                .lock()
                .expect("connection faults")
                .entry(session.clone())
                .or_default()
                .drop_command = Some(at);
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **A connection that neither answers nor fails** (ADR-0381): this
        // session's next command connection is held open with no answer,
        // before the command runs (`at=before`) or after it has committed
        // (`at=after`), or with its answer's head and part of its body sent
        // (`at=body`), until its page gives up on it. Once.
        ("POST", "/bench/hold") => {
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
            };
            let at = match (q("next"), q("at")) {
                (Some("command"), Some("before")) => HoldAt::Before,
                (Some("command"), Some("after")) => HoldAt::After,
                (Some("command"), Some("body")) => HoldAt::Body,
                _ => {
                    respond_json(
                        &mut stream,
                        400,
                        &session,
                        fresh,
                        "{\"error\":\"next is `command`, and at is `before`, `after` or `body`\"}",
                    );
                    return;
                }
            };
            server
                .connection_faults
                .lock()
                .expect("connection faults")
                .entry(session.clone())
                .or_default()
                .hold_command = Some(at);
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **A forced reconnect** (charter §15.5, ADR-0175): this session's
        // open subscriptions end now, with no more bytes, and each new one
        // is closed at once for `for` milliseconds. What is queued meanwhile
        // is the page's when it subscribes again.
        ("POST", "/bench/reconnect") => {
            let window = query
                .split('&')
                .find_map(|p| p.strip_prefix("for="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let now = std::time::Instant::now();
            let mut faults = server.connection_faults.lock().expect("connection faults");
            let mine = faults.entry(session.clone()).or_default();
            mine.cut_from = Some(now);
            mine.cut_until = Some(now + std::time::Duration::from_millis(window));
            drop(faults);
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // **A materializer failure** (charter §15.5, ADR-0176): this
        // session's next regeneration of its cart's entry fails. Once.
        ("POST", "/bench/materializer") => {
            if !query.split('&').any(|p| p == "fail=next") {
                respond_json(
                    &mut stream,
                    400,
                    &session,
                    fresh,
                    "{\"error\":\"fail is `next`\"}",
                );
                return;
            }
            server
                .materializer_faults
                .lock()
                .expect("materializer faults")
                .insert(session.clone());
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        ("POST", "/bench/estimate") => {
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
                    .and_then(percent_decoded)
            };
            let mut set = Estimator::default();
            if let Some(ms) = q("delay").and_then(|v| v.parse().ok()) {
                set.delay_ms = ms;
            }
            set.fail = q("fail").filter(|f| !f.is_empty());
            // What it estimates is the session's data, through the layer
            // (track `store-pg`); its delay and failure are test controls.
            let minutes = q("minutes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(store::DEFAULT_ESTIMATE.0);
            let max = q("max").and_then(|v| v.parse().ok());
            if let Err(why) = server.set_estimate(&session, minutes, max) {
                return refused_json(&mut stream, &session, fresh, why);
            }
            server
                .store
                .faults()
                .estimators
                .lock()
                .expect("estimators")
                .insert(session.clone(), set);
            let own = format!("session={session}");
            server
                .queries
                .evict_where(|k| k.key == own || k.key.starts_with(&format!("{own}\u{1f}")));
            respond_json(&mut stream, 200, &session, fresh, "{}");
        }
        // E7-P's structural commands. Each mutates the keyed collection and
        // broadcasts the patch that describes the mutation. A refusal returns
        // 409 and changes nothing — including the version.
        ("POST", "/command/menu") => {
            // Each value decoded as a form's is, as the other controls' are.
            // Until 2026-10-03 only `%20` was, and `%26` stayed in a name.
            let q = |k: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
                    .and_then(percent_decoded)
            };
            let id = q("id").unwrap_or_default();
            let op = match q("op").as_deref() {
                Some("insert_before") | Some("insert_after") => MenuOp::Insert {
                    name: q("name").unwrap_or_else(|| id.clone()),
                    id,
                    at: q("at"),
                    before: q("op").as_deref() == Some("insert_before"),
                },
                Some("remove") => MenuOp::Remove { id },
                Some("move") => MenuOp::Move {
                    id,
                    after: q("after"),
                },
                Some("rename") => MenuOp::Rename {
                    name: q("name").unwrap_or_default(),
                    id,
                },
                _ => {
                    respond_json(&mut stream, 400, &session, fresh, "{\"error\":\"no op\"}");
                    return;
                }
            };
            match server.broadcast_menu(op) {
                Ok(()) => respond_json(&mut stream, 202, &session, fresh, "{\"committed\":true}"),
                Err(e) => respond_json(
                    &mut stream,
                    409,
                    &session,
                    fresh,
                    &format!("{{\"committed\":false,\"refused\":\"{e}\"}}"),
                ),
            }
        }
        // A patch addressing an instance that does not exist. Not reachable
        // through any command — injected, because the property under test is
        // what the BROWSER does when a server and a page disagree, and a
        // correct server never produces the disagreement.
        ("POST", "/command/menu_ghost") => {
            let entry = ResourceEntryId::derive(&menu_identity(STORE_ID), &IDENTITY);
            let version = server.menu_version(STORE_ID);
            let mut queue = server.pending.lock().expect("pending");
            for doc in documents_of(&queue, &session) {
                if let Some(waiting) = queue.get_mut(&doc) {
                    waiting.push(StreamFrame::Patch(Patch {
                        protocol: CURRENT,
                        basis: CausalBasis::of(entry.clone(), Version(version.0 + 1)),
                        target: server.menu_address(),
                        operation: PatchOp::RemoveInstance {
                            instance: pw_document::InstanceToken::from_wire("nosuchinstance"),
                        },
                    }));
                }
            }
            drop(queue);
            respond_json(&mut stream, 202, &session, fresh, "{}");
        }
        // What the materializer holds for the PUBLIC menu entry. Enough to
        // show that a second reader reads rather than regenerates, and no
        // more: the storage key never appears, because the browser has no
        // business knowing how an entry is stored.
        ("GET", "/menu-entry") => {
            let identity = menu_identity(STORE_ID);
            let entry = ResourceEntryId::derive(&identity, &IDENTITY);
            let version = server.menu_version(STORE_ID);
            respond_json(
                &mut stream,
                200,
                &session,
                fresh,
                &format!(
                    "{{\"entry\":\"{entry}\",\"version\":{},\"partition\":\"{}\"}}",
                    version.0,
                    identity.partition_text()
                ),
            );
        }
        // E7-L — the handler's code, fetched on first interaction and not
        // before.
        //
        // Keyed by handler IDENTITY, not by name: a changed implementation is
        // a different identity, so it is a different URL and no cache can
        // serve yesterday's behaviour to today's document. The name is what
        // the module DOES; the identity is which version of it this document
        // was rendered against.
        //
        // The module is the COMPILER's (E10, 2026-09-25): `pw emit-handlers`
        // compiled the handler's body to `dist/handlers/<identity>.mjs`, and
        // this route serves that file unchanged. Until then the server wrote a
        // module here itself. An identity this build's templates do not name
        // is refused before the file system is asked, so a request can only
        // ever reach a file the compiler named.
        // A page's speculation module (ADR-0122), as `pw build` wrote it.
        // Only one a page's manifest names (ADR-0191).
        ("GET", route)
            if route.strip_prefix("/speculation/").is_some_and(|m| {
                let m = m.split('?').next().unwrap_or(m);
                server
                    .speculations
                    .values()
                    .any(|s| s["module"].as_str() == Some(m))
            }) =>
        {
            let module = route.trim_start_matches("/speculation/");
            let module = module.split('?').next().unwrap_or(module);
            match std::fs::read(server.artifacts.join("speculations").join(module)) {
                Ok(bytes) => {
                    respond_build(&mut stream, 200, "text/javascript; charset=utf-8", &bytes)
                }
                Err(_) => respond_build(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    b"this speculation was not compiled",
                ),
            }
        }
        // What a page computes from its signals (ADR-0227), as `pw build`
        // wrote it: only a module a page's plan names.
        ("GET", route)
            if route.split('?').next().is_some_and(|r| {
                server
                    .plans
                    .values()
                    .any(|p| computed_module(p).as_deref() == Some(r))
            }) =>
        {
            let module = route.split('?').next().unwrap_or(route);
            let file = module.trim_start_matches("/computed/");
            match std::fs::read(server.artifacts.join("computed").join(file)) {
                Ok(bytes) => {
                    respond_build(&mut stream, 200, "text/javascript; charset=utf-8", &bytes)
                }
                Err(_) => respond_build(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    b"this module was not compiled",
                ),
            }
        }
        ("GET", route) if route.starts_with("/handler/") => {
            let id = route
                .trim_start_matches("/handler/")
                .trim_end_matches(".mjs");
            // The query is the browser's, not ours: a retry appends one so the
            // module map treats it as a new specifier, because a failed load
            // is cached forever otherwise. The identity is still the path.
            let id = id.split('?').next().unwrap_or(id);
            if !server.handler_identities().contains(id) {
                // An identity this build does not know. Refused rather than
                // guessed: serving *some* handler for an unknown identity is
                // how a stale document ends up running new code.
                respond_build(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    b"no such handler",
                );
                return;
            }
            match std::fs::read(server.handler_module(id)) {
                Ok(module) => {
                    respond_build(&mut stream, 200, "text/javascript; charset=utf-8", &module)
                }
                Err(_) => respond_build(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    b"this handler was not compiled",
                ),
            }
        }
        // A page whose values are its signals and its streams (ADR-0130,
        // ADR-0148).
        ("GET", route) if route.starts_with("/page/") => {
            let path = route.trim_start_matches("/page/");
            // TRACK SEAM (argument-invariants): a parameter its type refuses
            // is a request refused, 400 and why, as a routed page's is
            // (ADR-0346); no such page, or one not given, is not found.
            let params = server.page_params(path, query).map(|params| {
                let held = held_params(server.plan_of(path), &params);
                (params, held)
            });
            if let Ok((_, Err(why))) = &params {
                respond(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    &session,
                    fresh,
                    why.as_bytes(),
                );
                return;
            }
            match params.map(|(params, _)| params) {
                // One that binds a query as the store's is (ADR-0190).
                Ok(params) if server.binds_a_query(path) => {
                    serve_bound(server, &mut stream, &session, fresh, path, params, query)
                }
                Ok(params) => serve_page(server, &mut stream, &session, fresh, path, params),
                Err(why) => respond(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    &session,
                    fresh,
                    why.as_bytes(),
                ),
            }
        }
        // What this build compiled, for the browser's resume decision
        // (ADR-0132): one `identity|capture` line per handler. Served with
        // the build, so a document cached from another build cannot vouch
        // for a handler this one lacks.
        ("GET", "/pw-handlers") => {
            // The build's name, and each page's document: its schema and its
            // scope (ADR-0300), which the browser's decision holds a document
            // of that page to. `#` lines, as no handler's identity begins.
            let mut table = format!("#build|{}\n", server.build_id);
            for (page, plan) in &server.plans {
                if let Some(template) = server.templates.iter().find(|t| &t.path == page) {
                    table.push_str(&format!(
                        "#page|{page}|{}|{}\n",
                        template.schema,
                        plan["scope"].as_str().unwrap_or("public")
                    ));
                }
            }
            for (identity, capture) in server.handler_table() {
                table.push_str(&format!("{identity}|{capture}\n"));
            }
            write_response(
                &mut stream,
                200,
                "text/plain; charset=utf-8",
                &format!("pw-build: {}\r\n", server.build_id),
                table.as_bytes(),
            );
        }
        // What the server did (ADR-0127, charter §10.5): how many times each
        // data-layer operation ran, and how many query values are kept.
        ("GET", "/metrics") => {
            let calls = server.calls.lock().expect("calls").clone();
            let runs = &server.counted_runs;
            let body = serde_json::json!({
                "calls": calls,
                "kept": server.queries.stored(),
                // The scheduled-commands track: how its runs ended.
                "runs": {
                    "ran": runs.ran.load(std::sync::atomic::Ordering::SeqCst),
                    "retried": runs.retried.load(std::sync::atomic::Ordering::SeqCst),
                    "stale": runs.stale.load(std::sync::atomic::Ordering::SeqCst),
                    "failed": runs.failed.load(std::sync::atomic::Ordering::SeqCst),
                },
            });
            respond_json(&mut stream, 200, &session, fresh, &body.to_string());
        }
        ("GET", "/menu") => {
            // Store 47's menu, read through the layer (track `store-pg`).
            let ids: Vec<String> = match server.menu_ids(STORE_ID) {
                Ok(ids) => ids.iter().map(|k| format!("\"{k}\"")).collect(),
                Err(why) => return refused_json(&mut stream, &session, fresh, why),
            };
            respond_json(
                &mut stream,
                200,
                &session,
                fresh,
                &format!("{{\"items\":[{}]}}", ids.join(",")),
            );
        }
        ("GET", "/stream") => stream_frames(server, &mut stream, &session, fresh, query),
        // Store 47's page, for E7's measurements. The root is the page that
        // declares it, the store list (ADR-0192).
        ("GET", "/StorePage.html") => {
            // The large-menu case, for E7 gate item 10. A query parameter
            // rather than a second route: the same page, the same renderer,
            // the same runtime — only more items, which is what makes the
            // measurement about SIZE rather than about a different page.
            let items: usize = query
                .split('&')
                .find_map(|p| p.strip_prefix("items="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if items > 0 {
                // Through the layer (track `store-pg`): a menu that long
                // already is not written.
                let replaced = server.store_write(
                    &session,
                    "store:host/menus#replace",
                    &[Val::String(STORE_ID.into()), Val::S64(items as i64)],
                    &[],
                    |_| {},
                );
                match replaced {
                    Ok(Some(Val::Bool(true))) => {
                        server
                            .materializer
                            .invalidate(&server.menu_key(STORE_ID), 0);
                        server.queries.invalidate("store.page.Menu");
                    }
                    Ok(_) => {}
                    Err(why) => return refused_json(&mut stream, &session, fresh, why),
                }
            }
            let page = server.store_page().to_string();
            serve_bound(
                server,
                &mut stream,
                &session,
                fresh,
                &page,
                store_params(STORE_ID),
                query,
            );
        }
        // A command this server does not host, including the address-resolving
        // `/command/add_to_cart?instance=..` route E10 deleted. Named, rather
        // than left to the 405 below: the method is right and the command is
        // not there.
        ("POST", route) if route.starts_with("/command/") => respond(
            &mut stream,
            404,
            "text/plain; charset=utf-8",
            &session,
            fresh,
            b"no such command",
        ),
        // **A page at its route** (ADR-0162): the store at `/stores/{id}`,
        // its parameters as the address gives them, and any other page that
        // declares one; one that binds a query as the store's is (ADR-0190).
        ("GET", _)
            if routed
                .as_ref()
                .is_some_and(|(page, _)| server.binds_a_query(page)) =>
        {
            let (page, params) = routed.expect("matched");
            serve_bound(server, &mut stream, &session, fresh, &page, params, query)
        }
        ("GET", _) if routed.is_some() => {
            let (page, params) = routed.expect("matched");
            serve_page(server, &mut stream, &session, fresh, &page, params)
        }
        ("GET", _) => serve_file(server, &mut stream, route),
        _ => respond(&mut stream, 405, "text/plain", &session, fresh, b"method"),
    }
}

/// Long-poll: hand over whatever frames are waiting, or wait briefly for one.
///
/// The transport, and only the transport. `pw-protocol` names none of this, so
/// E7-P can replace it with a streaming fetch without a frame changing.
/// The streaming adapter: one connection, frames written as they arrive.
///
/// # Why two adapters exist
///
/// Architect ruling, 2026-08-07: a multiplexed `StreamFrame` transport, *with
/// long-poll as a fallback adapter*. Two of them is the point. A protocol with
/// exactly one transport is indistinguishable from a protocol that IS its
/// transport, and the difference only becomes visible when a second one has to
/// carry the same frames without changing them.
///
/// Both adapters deliver identical frames, in order, under the same cursor
/// discipline. What differs is only when the connection ends: this one holds
/// it open and writes batch after batch; the long poll returns after the first
/// batch and is asked again.
///
/// No `content-length`, and the body is terminated by the close. That is the
/// oldest streaming mechanism HTTP has and needs no chunked framing to be
/// written by hand — which matters here, because a bug in hand-rolled chunked
/// encoding would look exactly like a transport that drops frames.
fn stream_open(
    server: &Server,
    stream: &mut TcpStream,
    session: &str,
    fresh: bool,
    document: u64,
    since: u64,
) {
    let doc: Doc = (session.to_string(), document);
    let cookie = if fresh {
        // TRACK SEAM (identity): the session cookie, HttpOnly and Secure
        // outside this machine.
        identity::session_cookie(session)
    } else {
        String::new()
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\n\
         {PRIVATE}{cookie}connection: close\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }

    // A page with a cursor the server has no subscriber for was forgotten
    // while idle: what it missed cannot be said, so it is told to reload.
    // So is a page that names no document (ADR-0161).
    if since > 0 && !server.pending.lock().expect("pending").contains_key(&doc) {
        let _ = stream.write_all(format!("{}\n", reload_batch(since)).as_bytes());
        return;
    }

    // What the page says it applied, once: a frame is forgotten when a page
    // acknowledges it, never because it was written (ADR-0139). Until
    // 2026-10-02 each pass below acknowledged what the pass before had
    // written, so a stream held for a page that had gone, reloaded or closed,
    // dropped what the page that replaced it was waiting for.
    {
        let mut queue = server.pending.lock().expect("pending");
        let waiting = queue
            .entry(doc.clone())
            .or_insert_with(|| Subscriber::at(document));
        waiting.acknowledge(since);
        waiting.note(format_args!("stream opened since {since}"));
    }
    let mut written = since;
    // How it ended, on its trail.
    let ended = |why: std::fmt::Arguments<'_>| {
        if let Some(waiting) = server.pending.lock().expect("pending").get_mut(&doc) {
            waiting.note(why);
        }
    };

    // Bounded, like the long poll: a held connection is a held thread, and
    // three engine families times six workers is eighteen of them.
    let opened = std::time::Instant::now();
    for pass in 0..80 {
        // Cut off since it opened (ADR-0175): it ends, with no more bytes.
        if server.cut_off(session, opened) {
            ended(format_args!("stream cut off at pass {pass}"));
            return;
        }
        let batch = {
            let asked = std::time::Instant::now();
            let mut queue = server.pending.lock().expect("pending");
            let waited = asked.elapsed().as_millis();
            let Some(waiting) = queue.get_mut(&doc) else {
                // Forgotten while the stream was held: the next request
                // is told to reload.
                return;
            };
            if waited >= 50 {
                waiting.note(format_args!("pass {pass} waited {waited} ms for the table"));
            }
            waiting.seen = std::time::Instant::now();
            let (cursor, frames) = waiting.after(written);
            if frames.is_empty() {
                None
            } else {
                let body: Vec<serde_json::Value> = frames
                    .into_iter()
                    .map(|f| serde_json::to_value(f).expect("frame"))
                    .collect();
                Some((cursor, serde_json::to_string(&body).expect("frames")))
            }
        };

        if let Some((cursor, frames)) = batch {
            // Written, but NOT acknowledged. The cursor advances here only for
            // this connection's own reads; the subscriber's copy is dropped on
            // the next request, which is the client saying it applied them.
            // A write that never arrives therefore costs nothing.
            let line = format!("{{\"cursor\":{cursor},\"frames\":{frames}}}\n");
            if stream.write_all(line.as_bytes()).is_err() || stream.flush().is_err() {
                ended(format_args!("stream write failed through {cursor}"));
                return;
            }
            ended(format_args!("wrote through {cursor}, {} bytes", line.len()));
            written = cursor;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    ended(format_args!(
        "stream ended after its passes, {} ms",
        opened.elapsed().as_millis()
    ));
}

fn stream_frames(
    server: &Server,
    stream: &mut TcpStream,
    session: &str,
    fresh: bool,
    _query: &str,
) {
    // A bounded hold, deliberately short. Each waiting subscriber occupies a
    // connection and a thread, and three engine families times six workers is
    // eighteen pages each holding one — a longer hold made the suite flaky
    // under parallelism while every test passed alone, which is a harness
    // failure that reads exactly like a runtime failure.
    let number = |name: &str| -> Option<u64> {
        _query
            .split('&')
            .find_map(|p| p.strip_prefix(name)?.strip_prefix('='))
            .and_then(|v| v.parse().ok())
    };
    let since = number("since").unwrap_or(0);
    // The document the page is (ADR-0161): its number, which was its first
    // cursor. A page that names none is no document this server served.
    let document = number("doc").unwrap_or(0);
    // Charter §15.5's forced reconnect (ADR-0175): cut off, the
    // subscription is closed at once, with no answer. What was queued
    // meanwhile is the page's when it subscribes again.
    let opened = std::time::Instant::now();
    if server.cut_off(session, opened) {
        return;
    }
    // A regeneration that failed is tried again (ADR-0176) when the page
    // next asks: its change reaches it within one subscription.
    server.drain(session);
    let doc: Doc = (session.to_string(), document);
    // What a telling passed by while no page of it asked (ADR-0297).
    server.told_as_it_asks(&doc);

    // One route, two adapters. The frames are the same either way — see
    // `e2e/transport.spec.mjs`, which runs the whole subscription twice.
    if _query.split('&').any(|p| p == "mode=stream") {
        stream_open(server, stream, session, fresh, document, since);
        return;
    }

    // Forgotten while idle: see `stream_open`.
    if since > 0 && !server.pending.lock().expect("pending").contains_key(&doc) {
        respond_json(stream, 200, session, fresh, &reload_batch(since));
        return;
    }

    for _ in 0..40 {
        // Cut off since it opened (ADR-0175): no answer.
        if server.cut_off(session, opened) {
            return;
        }
        {
            let mut queue = server.pending.lock().expect("pending");
            let waiting = queue
                .entry(doc.clone())
                .or_insert_with(|| Subscriber::at(document));
            waiting.seen = std::time::Instant::now();
            // This request is the client's acknowledgement of everything up to
            // `since`: it applied those frames and asked for what follows.
            waiting.acknowledge(since);
            let (cursor, frames) = waiting.after(since);
            if !frames.is_empty() {
                let body: Vec<serde_json::Value> = frames
                    .into_iter()
                    .map(|f| serde_json::to_value(f).expect("frame"))
                    .collect();
                let json = format!(
                    "{{\"cursor\":{cursor},\"frames\":{}}}",
                    serde_json::to_string(&body).expect("frames")
                );
                // Left in place until the next request proves they arrived.
                respond_json(stream, 200, session, fresh, &json);
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    respond_json(
        stream,
        200,
        session,
        fresh,
        &format!("{{\"cursor\":{since},\"frames\":[]}}"),
    );
}

/// **A page that binds a query, at the parameters `params` give it**
/// (ADR-0162, ADR-0190): the store's, at `/StorePage.html` for store 47 and
/// `/stores/{id}` for any, and any other at its route, the store list's at
/// `/` (ADR-0192). Until
/// 2026-10-04 only the store's page was served so: another page that bound
/// a query was refused, and read queries through streams alone.
fn serve_bound(
    server: &Server,
    stream: &mut TcpStream,
    session: &str,
    fresh: bool,
    page: &str,
    params: Params,
    query: &str,
) {
    let template = server.template_of(page);
    let unavailable = |stream: &mut TcpStream, why: Unread| match why {
        // A page whose address is another of the page's, as the page
        // declares it (ADR-0295), is moved there.
        Unread::Moved { to, permanent } => moved(
            server, stream, session, fresh, page, &params, query, &to, permanent,
        ),
        // A page whose address names nothing, as the page declares it
        // (ADR-0163), is not found.
        Unread::NotFound(_) => respond(
            stream,
            404,
            "text/html; charset=utf-8",
            session,
            fresh,
            NOT_FOUND_PAGE.as_bytes(),
        ),
        // A page whose values could not be read is answered as
        // unavailable, and the server goes on (E14, T04).
        Unread::Failed(why) => respond(
            stream,
            503,
            "text/plain; charset=utf-8",
            session,
            fresh,
            format!("`{}` cannot be shown: {why}", template.name).as_bytes(),
        ),
    };
    // Each stream's query, started before the document is rendered
    // (ADR-0148): the page waits for the ones it is declared to wait
    // for, and the rest fill their regions in the same response.
    let reader = notifications::user_of(&server.identity.principals(), session);
    let runs = match stream_runs(server.plan_of(page), session, &reader, &params) {
        Ok(runs) => runs,
        Err(why) => return unavailable(stream, Unread::Failed(why)),
    };
    std::thread::scope(|scope| {
        let mut settling = Settling::start(scope, server, session, runs);
        let settled = settling.for_document();
        // Registering the subscriber HERE, when the document is
        // served, rather than when it first subscribes: the page
        // holds instances from this moment on, so a structural change
        // after this moment has an address in it. A registration
        // deferred to the first poll would silently drop every change
        // that raced it.
        let (rendered, cursor, entries, env) =
            match server.serve_document_settled(session, page, &params, &settled) {
                Ok(served) => served,
                Err(why) => return unavailable(stream, why),
            };
        // Its page's speculation, by its own manifest (ADR-0191).
        let speculation = server.speculations.get(page).and_then(|m| {
            let module = m["module"].as_str()?;
            // Each part the browser renders again with a speculation
            // (ADR-0172), whose template the document carries.
            let regions: Vec<u32> = m["regions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r.as_u64().map(|r| r as u32))
                .collect();
            Some((
                format!("/speculation/{module}"),
                entries,
                regions,
                params.clone(),
            ))
        });
        // The page's title, from the values it was rendered with
        // (ADR-0183). A store's program that states none, the benchmark's
        // copy, is titled as every store's page was, and another page that
        // states none by its name, as a page of signals is.
        let title = match pw_render::title_text(template, &env) {
            Ok(title) => title.unwrap_or_else(|| {
                if page == server.store_page() {
                    "Store".to_string()
                } else {
                    template.name.clone()
                }
            }),
            Err(why) => return unavailable(stream, Unread::Failed(format!("its title: {why:?}"))),
        };
        // And what it says of itself to what reads it unshown (ADR-0186).
        let metadata = match pw_render::head_metadata(template, &env) {
            Ok(metadata) => metadata,
            Err(why) => {
                return unavailable(stream, Unread::Failed(format!("its metadata: {why:?}")));
            }
        };
        // And the address it names its own (ADR-0311).
        let origin = server.origin.get().map(String::as_str);
        let metadata = match pw_render::head_canonical(template, &env, origin) {
            Ok(canonical) => metadata + &canonical,
            Err(why) => {
                return unavailable(
                    stream,
                    Unread::Failed(format!("its canonical address: {why:?}")),
                );
            }
        };
        // What it asked of its reader, as the host answered for it (ADR-0307).
        let holds: BTreeMap<String, bool> = server.plan_of(page)["predicates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str())
            .filter_map(|p| {
                let name = asked_name(p);
                match env.value(&name) {
                    Some(Value::Bool(b)) => Some((name, *b)),
                    _ => None,
                }
            })
            .collect();
        let html = document(
            &rendered,
            &title,
            &metadata,
            server.data.style(),
            template,
            &server.templates,
            server.plan_of(page),
            cursor,
            speculation,
            &holds,
            &server.build_id,
        );
        server.respond_streaming(stream, session, fresh, &html, template, &env, &mut settling);
    });
}

/// **A page moved** (ADR-0295): 308 where the move is permanent and 307
/// where it is not, to the page's route with its one parameter `to`,
/// encoded as a link's hole is, and the address's query kept (RFC 9110:
/// a `Location` may be a path, resolved against the address asked for).
/// Kept by no cache, as every answer to a session is: a heuristically
/// cacheable 308 would outlive the data that decided it. A value no
/// segment can carry, or the address asked for itself, is the program's
/// fault, answered 500 and said.
#[allow(clippy::too_many_arguments)]
fn moved(
    server: &Server,
    stream: &mut TcpStream,
    session: &str,
    fresh: bool,
    page: &str,
    params: &Params,
    query: &str,
    to: &str,
    permanent: bool,
) {
    let plan = server.plan_of(page);
    // The route's path: its query part (ADR-0309) names no segment, and the
    // address's own query is kept below.
    let route = route_path(plan["route"].as_str().unwrap_or_default());
    let holes: Vec<&str> = route
        .split('/')
        .filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .collect();
    let fault = |stream: &mut TcpStream, why: String| {
        eprintln!("pw-dev-server: `{page}` cannot be moved: {why}");
        respond(
            stream,
            500,
            "text/plain; charset=utf-8",
            session,
            fresh,
            format!("`{page}` cannot be moved: {why}").as_bytes(),
        );
    };
    let [hole] = holes.as_slice() else {
        return fault(stream, format!("its route `{route}` has no one parameter"));
    };
    if to.is_empty() || to == "." || to == ".." {
        return fault(stream, format!("`{to}` is no segment of an address"));
    }
    if params.get(*hole).map(String::as_str) == Some(to) {
        return fault(stream, format!("it moves `{to}` to itself"));
    }
    let mut location = route.replace(
        &format!("{{{hole}}}"),
        &pw_render::escape::url_component(to),
    );
    if !query.is_empty() {
        location.push('?');
        location.push_str(query);
    }
    let shown = pw_render::escape::text(&location);
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Moved</title>\n</head>\n<body>\n<main>\n<h1>Moved</h1>\n\
         <p>This page is at <a href=\"{}\">{shown}</a>.</p>\n</main>\n</body>\n</html>\n",
        pw_render::escape::attribute(&location)
    );
    let cookie = if fresh {
        identity::session_cookie(session)
    } else {
        String::new()
    };
    write_response(
        stream,
        if permanent { 308 } else { 307 },
        "text/html; charset=utf-8",
        &format!(
            "{PRIVATE}{cookie}location: {location}\r\n{}",
            runs_nothing("text/html")
        ),
        body.as_bytes(),
    );
}

/// **The origin a page's canonical address is written with** (ADR-0311):
/// the deployment's public origin, `stated` where `PW_ORIGIN` names it. A
/// production deployment whose pages state one, and that states no origin,
/// is refused at start, rather than given this machine's: every canonical
/// address it wrote would name a loopback address.
fn canonical_origin(
    server: &Server,
    deployment: &identity::Deployment,
    stated: bool,
) -> Result<String, String> {
    if server.states_canonical() && !deployment.development && !stated {
        return Err(
            "its pages state their canonical address, and PW_ORIGIN states no \
                    public origin to write it with"
                .to_string(),
        );
    }
    Ok(deployment.origin.clone())
}

/// **A page that binds no query, at the parameters `params` give it**
/// (ADR-0130, ADR-0162): from `/page/<path>?..`, or its route.
fn serve_page(
    server: &Server,
    stream: &mut TcpStream,
    session: &str,
    fresh: bool,
    path: &str,
    params: Params,
) {
    let not_found = |stream: &mut TcpStream, why: String| {
        respond(
            stream,
            404,
            "text/plain; charset=utf-8",
            session,
            fresh,
            why.as_bytes(),
        );
    };
    let runs = match server
        .plans
        .get(path)
        .ok_or_else(|| format!("no page `{path}` in this build"))
        .and_then(|plan| {
            let reader = notifications::user_of(&server.identity.principals(), session);
            stream_runs(plan, session, &reader, &params)
        }) {
        Ok(runs) => runs,
        Err(why) => return not_found(stream, why),
    };
    std::thread::scope(|scope| {
        let mut settling = Settling::start(scope, server, session, runs);
        let settled = settling.for_document();
        match server.render_page(path, &params, session, &settled) {
            Ok((page, env, template)) => server.respond_streaming(
                stream,
                session,
                fresh,
                &page,
                template,
                &env,
                &mut settling,
            ),
            Err(why) => not_found(stream, why),
        }
    });
}

/// **The parameters a path gives a route** (ADR-0162), or `None` when it is
/// not the route's: one segment each, as the compiler's link check matches
/// a link to a route (`routes::matches`), each decoded as a path is.
/// **The names a route's query part gives** (ADR-0309): `"/search{?q}"`
/// gives `q`, RFC 6570's form-style expansion at the route's end, as
/// `pw_core::routes` reads it.
fn route_query_names(route: &str) -> Vec<&str> {
    match route.find("{?") {
        Some(at) if route.ends_with('}') => route[at + 2..route.len() - 1]
            .split(',')
            .map(str::trim)
            .collect(),
        _ => Vec::new(),
    }
}

/// A route's path: the route without its query part.
fn route_path(route: &str) -> &str {
    route.find("{?").map_or(route, |at| &route[..at])
}

/// **What a request's query gives a route's query names** (ADR-0309), read
/// as the URL Standard reads application/x-www-form-urlencoded (§5.1): split
/// on `&`, a `+` a space, then percent-decoded; the first occurrence of a
/// name taken, and a name the route does not give ignored, as an address
/// may carry one from elsewhere (`utm_source`). `None` where a value the
/// route gives is no UTF-8, or has an escape that is not one: refused,
/// where the URL Standard would replace it.
fn query_params(route: &str, query: &str) -> Option<Params> {
    let names = route_query_names(route);
    let mut out = Params::new();
    if names.is_empty() {
        return Some(out);
    }
    let decoded = |v: &str| path_decoded(&v.replace('+', " "));
    for piece in query.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = piece.split_once('=').unwrap_or((piece, ""));
        let Some(name) = decoded(name) else { continue };
        if !names.contains(&name.as_str()) || out.contains_key(&name) {
            continue;
        }
        out.insert(name, decoded(value)?);
    }
    Some(out)
}

/// TRACK SEAM (argument-invariants): **a page's parameters, held to the
/// invariants their types state** (ADR-0346). An address is a browser's, so
/// each parameter, as [`param_val`] gives it to the page, is decoded as an
/// argument is (ADR-0179): one that breaks its type's invariant is refused,
/// by name, before the page is rendered or a query reads it, never shown or
/// read with. The plan states where (`bounded`, by each parameter's position
/// in `params`); a page whose parameters' types state none has nothing to
/// hold.
fn held_params(plan: &serde_json::Value, params: &Params) -> Result<(), String> {
    let Some(bounded) = plan.get("bounded") else {
        return Ok(());
    };
    let bounded: Vec<pw_host::Bounded> = serde_json::from_value(bounded.clone()).map_err(|e| {
        format!("the page's plan states its parameters' invariants unreadably: {e}")
    })?;
    let values = plan["params"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let name = p.as_str().unwrap_or_default();
            param_val(plan, name, params)
                .ok_or_else(|| format!("the page's `{name}` is given no value"))
        })
        .collect::<Result<Vec<Val>, String>>()?;
    pw_host::engine::holds(&bounded, &values).map_err(|why| format!("parameter refused: {why}"))
}

/// **A page parameter's value, as its document's address gave it**
/// (ADR-0162, ADR-0309): a path's is its text; a query's is `Some` of what
/// the address said, or `None` where it said nothing. `None` for a path's
/// that was not given.
fn param_val(plan: &serde_json::Value, name: &str, params: &Params) -> Option<Val> {
    let route = plan["route"].as_str().unwrap_or_default();
    if route_query_names(route).contains(&name) {
        return Some(Val::Option(
            params.get(name).map(|v| Box::new(Val::String(v.clone()))),
        ));
    }
    params.get(name).map(|v| Val::String(v.clone()))
}

fn route_params(route: &str, path: &str) -> Option<Params> {
    let segments = |s: &str| -> Vec<String> {
        match s.trim_matches('/') {
            "" => Vec::new(),
            t => t.split('/').map(str::to_string).collect(),
        }
    };
    let (route, path) = (segments(route), segments(path));
    if route.len() != path.len() {
        return None;
    }
    let mut out = Params::new();
    for (r, p) in route.iter().zip(&path) {
        match r.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            Some(name) => {
                let value = path_decoded(p).filter(|v| !v.is_empty())?;
                out.insert(name.to_string(), value);
            }
            None if r == p => {}
            None => return None,
        }
    }
    Some(out)
}

/// A path segment, decoded: `%XX` as the byte it escapes. A `+` is itself,
/// as RFC 3986 has it in a path; only a form's query makes it a space.
/// `None` for an escape that is not one, or bytes that are not UTF-8.
fn path_decoded(v: &str) -> Option<String> {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn serve_file(server: &Server, stream: &mut TcpStream, route: &str) {
    let name = route.trim_start_matches('/');
    // No traversal: a request names a file in `dist` and nothing above it.
    if name.contains("..") || name.contains('/') {
        respond_build(stream, 404, "text/plain", b"not found");
        return;
    }
    let path = server.dist.join(name);
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("mjs") | Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        // A stylesheet the build ships, which a page's policy allows from
        // this origin, and which a browser applies only as `text/css`.
        Some("css") => "text/css; charset=utf-8",
        // TRACK SEAM (todomvc): a drawing the build ships, which the page's
        // stylesheet draws a checkbox with (`img-src 'self'`, ADR-0333): a
        // browser draws an image only as `image/svg+xml`, under `nosniff`.
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    match std::fs::read(&path) {
        Ok(bytes) => respond_build(stream, 200, mime, &bytes),
        Err(_) => respond_build(stream, 404, "text/plain", b"not found"),
    }
}

/// A query string's value, decoded: `%XX` as the byte it escapes, and `+`
/// as a space, as a form encodes one. `None` for an escape that is not one,
/// or bytes that are not UTF-8.
fn percent_decoded(v: &str) -> Option<String> {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// **What a document says about its page's signals** (ADR-0130, ADR-0140):
/// each signal's first value, the parts they decide, and each block's
/// template, which the browser renders again. Empty for a page with none.
fn signal_manifest(
    plan: &serde_json::Value,
    template: &Template,
) -> (
    serde_json::Map<String, serde_json::Value>,
    serde_json::Value,
    serde_json::Map<String, serde_json::Value>,
) {
    let signals: serde_json::Map<String, serde_json::Value> = plan["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or_default().to_string(),
                s["initial"].clone(),
            )
        })
        .collect();
    let mut blocks = serde_json::Map::new();
    for live in plan["live"].as_array().into_iter().flatten() {
        // A block, or a view's instance (ADR-0203): rendered again whole.
        if !matches!(
            live["kind"].as_str(),
            Some("conditional" | "match" | "instance")
        ) {
            continue;
        }
        let id = live["part"].as_u64().unwrap_or_default() as u32;
        if let Some(part) = find_part(&template.chunks, id) {
            blocks.insert(
                id.to_string(),
                serde_json::to_value(part).unwrap_or_default(),
            );
        }
    }
    let live = plan["live"].clone();
    (signals, live, blocks)
}

/// **The signal a page binds to its address's fragment** (track `todomvc`,
/// ADR-0367), as its plan states it: the signal and each form the fragment
/// takes with the value the signal takes then, carried in the document's
/// manifest. The browser keeps the signal in step with `location.hash`; this
/// host renders the signal's first value, since a request carries no
/// fragment.
fn with_fragment(manifest: &mut serde_json::Value, plan: &serde_json::Value) {
    if let Some(binding) = plan.get("fragment").filter(|f| !f.is_null()) {
        manifest["fragment"] = binding.clone();
    }
}

/// Each signal's first value, set into a rendering environment (ADR-0130).
fn with_signals(mut env: Env, plan: &serde_json::Value) -> Env {
    for s in plan["signals"].as_array().into_iter().flatten() {
        let name = s["name"].as_str().unwrap_or_default();
        let first = Value::from_wire(&s["initial"])
            .unwrap_or_else(|e| panic!("the plan's first value of `{name}`: {e}"));
        env = env.set(name, first);
    }
    env
}

/// A page signal's first value, as the build computed it (ADR-0130), by the
/// page's plan (ADR-0190).
fn first_value(plan: &serde_json::Value, signal: &str) -> serde_json::Value {
    plan["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"] == signal)
        .map(|s| s["initial"].clone())
        .unwrap_or(serde_json::Value::Null)
}

/// **The module a page's browser computes its signals' values with**
/// (ADR-0227), as `pw build` wrote it: one a page computes any from.
fn computed_module(plan: &serde_json::Value) -> Option<String> {
    let computes = plan["live"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|l| l["derived"].is_string())
        // TRACK SEAM (device-store): or the steps of a value the device
        // keeps from an earlier version to its own.
        || device::migrates(plan);
    computes.then(|| {
        format!(
            "/computed/{}.mjs",
            plan["page"].as_str().unwrap_or_default()
        )
    })
}

/// **A page that binds no query, as a document** (ADR-0130, ADR-0148): its
/// values are its signals and its streams' parts.
///
/// Its parts manifest carries what the browser holds and renders again:
/// each signal's first value, each part a signal decides, and the template
/// of each block one decides, which the browser's copy of the renderer
/// renders when it changes.
fn signal_document(
    body: &str,
    title: &str,
    metadata: &str,
    template: &Template,
    plan: &serde_json::Value,
    templates: &[Template],
    build: &str,
) -> String {
    let (signals, _, blocks) = signal_manifest(plan, template);
    let manifest = serde_json::json!({
        "template": template.path,
        "schema": template.schema,
        "cursor": 0,
        "parts": template.manifest(),
        "resume": resume_manifest(templates, build, template, plan),
        "signals": signals,
        "live": plan["live"].clone(),
        "blocks": blocks,
        // Nothing on this page is a resource's, so nothing is listened for.
        "listens": false,
    });
    let mut manifest = manifest;
    // The signal it binds to its address's fragment (track `todomvc`).
    with_fragment(&mut manifest, plan);
    // TRACK SEAM (device-store): what the device keeps that it holds.
    device::with_device(&mut manifest, plan);
    // What it computes from its signals (ADR-0227).
    if let Some(module) = computed_module(plan) {
        manifest["computed"] = serde_json::Value::String(module);
    }
    with_instance_templates(&mut manifest, template, templates);
    if let Some(layout) = plan.get("layout").filter(|l| !l.is_null()) {
        manifest["layout"] = layout.clone();
    }
    // No `<` in a script element's text (ADR-0097).
    let json =
        pw_render::escape::json_in_script(&serde_json::to_string(&manifest).unwrap_or_default());
    // Where a failed press is said (ADR-0302), from the first byte.
    let announcer = pw_render::ANNOUNCER;
    // TRACK SEAM (device-store): its root reading the device, and the style
    // that hides what the device decides until it has (ADR-0366).
    let (root, device) = (device::root(plan), device::head(plan));
    format!(
        "<!doctype html>\n<html{root}>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n{metadata}{device}</head>\n<body>\n{body}\n\
         {announcer}\n<script type=\"application/json\" id=\"pw-parts\">{json}</script>\n\
         {RUNTIME}\n{DOCUMENT_END}",
        pw_render::escape::text(title)
    )
}

/// The part numbered `id`, wherever it is in `chunks`.
fn find_part(chunks: &[pw_render::ir::Chunk], id: u32) -> Option<&pw_render::ir::Part> {
    use pw_render::ir::Chunk;
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        if p.id().is_some_and(|i| i.0 == id) {
            return Some(p);
        }
        // Through every region a part has, as the IR's own walks go: a walk
        // that names the kinds it descends into misses the next one, as this
        // missed a stream's arms (ADR-0148).
        if let Some(inner) = p.nested().into_iter().find_map(|r| find_part(r, id)) {
            return Some(inner);
        }
    }
    None
}

/// Each handler `templates` hold, by identity, with the paths it captures
/// joined (ADR-0132).
fn handler_table(templates: &[Template]) -> BTreeMap<String, String> {
    fn walk(chunks: &[pw_render::ir::Chunk], out: &mut BTreeMap<String, String>) {
        for c in chunks {
            let pw_render::ir::Chunk::Dynamic(p) = c else {
                continue;
            };
            if let pw_render::ir::Part::Event {
                handler, captures, ..
            } = p
                && !handler.is_empty()
            {
                out.insert(handler.clone(), captures.join(","));
            }
            // Every region, as `find_part` walks them. Until 2026-10-03 this
            // named the three block kinds, so a handler in a stream's arm was
            // in no table, and its Pick refused (ADR-0148).
            for region in p.nested() {
                walk(region, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    for t in templates {
        walk(&t.chunks, &mut out);
    }
    out
}

/// The document's resume manifest (ADR-0132): one base, and one entry per
/// handler, keyed by its identity, presenting its capture schema. The base
/// names no handler, so a part with no entry of its own authorises nothing.
/// **The document's resume manifest** (ADR-0132, ADR-0300): its base names
/// the build that served it, its page's document schema (the template's,
/// which closes over each template its instances reach, ADR-0203) and its
/// page's scope (`resume::page_scope`, carried in its plan), each of which
/// the browser's decision holds a handler to.
fn resume_manifest(
    templates: &[Template],
    build: &str,
    template: &Template,
    plan: &serde_json::Value,
) -> serde_json::Value {
    let handlers: serde_json::Map<String, serde_json::Value> = handler_table(templates)
        .into_iter()
        .map(|(identity, capture)| {
            let bytes = if capture.is_empty() { "" } else { "x" };
            (
                identity.clone(),
                serde_json::json!({ "handler": identity, "capture": capture, "captures": bytes }),
            )
        })
        .collect();
    serde_json::json!({
        "scheme": "2", "abi": "1", "build": build, "handler": "",
        "capture": "", "document": template.schema,
        "scope": plan["scope"].as_str().unwrap_or("public"),
        "captures": "", "construct": "region",
        "handlers": handlers,
    })
}

/// **Each view's template the page's instances reach** (ADR-0203), by its
/// path: its parts, which the browser reads an instance's in, and the
/// template, which the browser's renderer renders an instance in a block
/// with. Nothing for a page that renders no view that contains itself.
fn with_instance_templates(
    manifest: &mut serde_json::Value,
    template: &Template,
    templates: &[Template],
) {
    let reached: serde_json::Map<String, serde_json::Value> =
        pw_render::instances_reached(template, templates)
            .into_iter()
            .map(|t| {
                (
                    t.path.clone(),
                    serde_json::json!({ "parts": t.manifest(), "template": t }),
                )
            })
            .collect();
    if !reached.is_empty() {
        manifest["templates"] = serde_json::Value::Object(reached);
    }
}

/// The document shell, with the parts manifest and the runtime: titled as
/// the page states (ADR-0183), and styled as its data layer styles its pages
/// (ADR-0220).
#[allow(clippy::too_many_arguments)]
fn document(
    body: &str,
    title: &str,
    metadata: &str,
    style: &str,
    template: &Template,
    templates: &[Template],
    plan: &serde_json::Value,
    cursor: u64,
    speculation: Option<(String, serde_json::Value, Vec<u32>, Params)>,
    holds: &BTreeMap<String, bool>,
    build: &str,
) -> String {
    let manifest = serde_json::json!({
        "template": template.path,
        "schema": template.schema,
        // Where this document starts listening. A page carries its own
        // position rather than starting at zero, because starting at zero
        // would replay changes this document already contains.
        "cursor": cursor,
        "parts": template.manifest(),
        // One base manifest and a per-handler override.
        //
        // Per handler, because `decide` answers about ONE handler and a page
        // with two would otherwise authorise both on the strength of whichever
        // one the page-wide manifest described. `clear_cart` captures nothing,
        // so its capture schema is the schema of nothing — which is still a
        // schema, and still has to match.
        "resume": resume_manifest(templates, build, template, plan),
    });
    // The page's speculation module and the values it starts from (ADR-0122).
    // A private page's own session's values: this document is `cache private`.
    let mut manifest = manifest;
    with_instance_templates(&mut manifest, template, templates);
    // **The layout it is shown in** (ADR-0303): what a
    // navigation compares, to keep the layout in place.
    if let Some(layout) = plan.get("layout").filter(|l| !l.is_null()) {
        manifest["layout"] = layout.clone();
    }
    // And its signals (ADR-0140): a page that reads queries holds UI state
    // too, and the browser renders what they decide, as on a page of
    // signals alone.
    let (signals, live, blocks) = signal_manifest(plan, template);
    if !signals.is_empty() {
        manifest["signals"] = serde_json::Value::Object(signals);
        manifest["live"] = live;
        manifest["blocks"] = serde_json::Value::Object(blocks);
    }
    // The signal it binds to its address's fragment (track `todomvc`).
    with_fragment(&mut manifest, plan);
    // TRACK SEAM (device-store): what the device keeps that it holds.
    device::with_device(&mut manifest, plan);
    // What it computes from them (ADR-0227).
    if let Some(module) = computed_module(plan) {
        manifest["computed"] = serde_json::Value::String(module);
    }
    // Each binding a signal keys (ADR-0152): the browser reads it again, for
    // the new key, when one of its signals changes, and does what its stale
    // work says.
    let keyed: Vec<serde_json::Value> = plan["bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["signals"].as_array().is_some_and(|s| !s.is_empty()))
        .map(|b| {
            serde_json::json!({
                "binding": b["binding"],
                "signals": b["signals"],
                "on_key_change": b["policy"]["on_key_change"],
            })
        })
        .collect();
    if !keyed.is_empty() {
        manifest["keyed"] = serde_json::Value::Array(keyed);
    }
    if let Some((module, entries, regions, params)) = speculation {
        manifest["speculation"] = serde_json::Value::String(module);
        manifest["entries"] = entries;
        // And the page's parameters (ADR-0236), as its address gives them:
        // a region a speculation renders again reads them, as the host does.
        manifest["params"] = serde_json::json!(params);
        // And what the page asked of its reader (ADR-0307), as the host
        // answered it for the document: a region rendered again reads it.
        if !holds.is_empty() {
            manifest["holds"] = serde_json::json!(holds);
        }
        // Each part a speculation renders again, as its template writes it
        // (ADR-0172): the browser's copy of the renderer renders it from the
        // speculated value.
        // A handler's region is its element's handlers, whose captures are
        // one attribute (ADR-0217).
        let regions: serde_json::Map<String, serde_json::Value> = regions
            .iter()
            .filter_map(|id| {
                let part = find_part(&template.chunks, *id)?;
                let value = match part {
                    pw_render::ir::Part::Event { owner, .. } => serde_json::to_value(
                        template
                            .chunks
                            .iter()
                            .filter_map(|c| match c {
                                pw_render::ir::Chunk::Dynamic(
                                    p @ pw_render::ir::Part::Event { owner: o, .. },
                                ) if o == owner => Some(p),
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                    )
                    .ok()?,
                    _ => serde_json::to_value(part).ok()?,
                };
                Some((id.to_string(), value))
            })
            .collect();
        if !regions.is_empty() {
            manifest["regions"] = serde_json::Value::Object(regions);
        }
    }
    // No `<` in a script element's text (ADR-0097).
    let json =
        pw_render::escape::json_in_script(&serde_json::to_string(&manifest).unwrap_or_default());
    // Laid out at a phone's width, as every page this host serves is
    // (ADR-0182). Its style in its head, where HTML puts it and where it is
    // read before the body is laid out (ADR-0187): its data layer's, and none
    // where the layer has none. Until ADR-0220 every program's page carried
    // the store's menu's.
    let style = match style {
        "" => String::new(),
        css => format!("<style>{css}</style>\n"),
    };
    // Where a failed press is said (ADR-0302), from the first byte.
    let announcer = pw_render::ANNOUNCER;
    // TRACK SEAM (device-store): its root reading the device, and the style
    // that hides what the device decides until it has (ADR-0366).
    let (root, device) = (device::root(plan), device::head(plan));
    format!(
        "<!doctype html>\n<html{root}>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n{metadata}{style}{device}</head>\n<body>\n{body}\n\
         {announcer}\n<script type=\"application/json\" id=\"pw-parts\">{json}</script>\n\
         {RUNTIME}\n{DOCUMENT_END}",
        title = pw_render::escape::text(title)
    )
}

/// **How a document starts its runtime** (ADR-0148): a classic script that
/// imports it, which runs while the response is still open, so a page whose
/// streamed regions have not settled is interactive. Measured on 2026-10-03
/// (`spikes/own-renderer/probes/streaming.mjs`): a deferred module starts
/// only once the whole response has arrived, in every engine, and an `async`
/// one does in WebKit.
const RUNTIME: &str = "<script>import(\"/pw-runtime.mjs\")</script>";

/// **Its text**, which a document's policy allows by its hash (`csp`).
/// `RUNTIME`'s, as a test holds it.
const RUNTIME_SOURCE: &str = "import(\"/pw-runtime.mjs\")";

/// The store page's speculation manifest, if the build wrote one (ADR-0122).
/// **Every page's speculation manifest** (ADR-0191), by the page it names.
fn speculation_manifests(dist: &std::path::Path) -> BTreeMap<String, serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(dist.join("speculations")) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let m: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
            Some((m["page"].as_str()?.to_string(), m))
        })
        .collect()
}

/// **The request's session, and whether it is new**: its cookie's, or a
/// new one where it carries none. A cookie naming a session the platform
/// opens on a reader's behalf (NEXT 6c, `identity::ON_BEHALF`) is a new
/// session too: those are never a browser's to present, though their names
/// are no secret.
fn session_of(headers: &str) -> (String, bool) {
    match headers
        .split("pw-session=")
        .nth(1)
        .and_then(|rest| rest.split([';', '\r', '\n']).next())
    {
        Some(id) if !id.starts_with(identity::ON_BEHALF) => (id.to_string(), false),
        // TRACK SEAM (identity): a fresh session's id, 128 random bits.
        _ => (identity::new_session_id(), true),
    }
}

/// **A change to the store's data refused** (track `store-pg`): 409, and
/// why. Nothing of it was kept.
fn refused_json(stream: &mut TcpStream, session: &str, fresh: bool, why: String) {
    let why = serde_json::Value::String(why);
    respond_json(
        stream,
        409,
        session,
        fresh,
        &format!("{{\"refused\":{why}}}"),
    );
}

fn respond_json(stream: &mut TcpStream, code: u16, session: &str, fresh: bool, body: &str) {
    respond(
        stream,
        code,
        "application/json; charset=utf-8",
        session,
        fresh,
        body.as_bytes(),
    );
}

/// **An answer begun and never finished** (ADR-0381's fault): its head, whose
/// `content-length` promises the whole body, and the body's first half.
fn respond_in_part(stream: &mut TcpStream, code: u16, session: &str, fresh: bool, body: &str) {
    let cookie = if fresh {
        identity::session_cookie(session)
    } else {
        String::new()
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json; charset=utf-8\r\n\
         content-length: {length}\r\n{NOSNIFF}{PRIVATE}{cookie}connection: close\r\n\r\n",
        reason = reason(code),
        length = body.len(),
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body.as_bytes()[..body.len() / 2]);
    let _ = stream.flush();
}

/// **A connection held open** (ADR-0381's fault): nothing more is written to
/// it, and it is read until its client closes it, or for [`HOLD_AT_MOST`], so
/// the hold ends with the page that gave up on it.
fn hold_open(stream: &mut TcpStream) {
    let until = std::time::Instant::now() + HOLD_AT_MOST;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(50)));
    let mut read = [0u8; 64];
    while std::time::Instant::now() < until {
        match stream.read(&mut read) {
            // Closed by its client.
            Ok(0) => return,
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            // Reset by its client.
            Err(_) => return,
        }
    }
}

/// **A status code's reason phrase** (RFC 9110 §15). A client ignores it
/// (RFC 9112 §4); a person reading the response does not. Until 2026-10-03
/// every response said `OK`, a 404 and a 503 among them.
fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// **How a response to a session's request may be kept** (ADR-0184): by
/// no cache. What it holds is the session's, a page, its cart, an answer to
/// its command, or the cookie that names it; and a cache that kept it would
/// give it to whoever asked next. Charter §15.6 tests 2, 12 and 13: until
/// 2026-10-04 the store's page said nothing of how it may be kept.
const PRIVATE: &str = "cache-control: private, no-store\r\n";

/// **A response to a session's request**: kept by no cache, and with the
/// cookie that names a fresh session (ADR-0184).
fn respond(stream: &mut TcpStream, code: u16, mime: &str, session: &str, fresh: bool, body: &[u8]) {
    let cookie = if fresh {
        // TRACK SEAM (identity): the session cookie, HttpOnly and Secure
        // outside this machine.
        identity::session_cookie(session)
    } else {
        String::new()
    };
    write_response(
        stream,
        code,
        mime,
        &format!("{PRIVATE}{cookie}{}", runs_nothing(mime)),
        body,
    );
}

/// **The policy of an HTML answer that is no document the runtime starts
/// in** (`csp::BARE`): a page not found, a page moved, a page the build
/// rendered with no runtime. None for any other type.
fn runs_nothing(mime: &str) -> String {
    if mime.starts_with("text/html") {
        format!("content-security-policy: {}\r\n", csp::BARE)
    } else {
        String::new()
    }
}

/// **A file of the build** (ADR-0184): the runtime, the renderer, a
/// handler's module. The same for every reader, and holding nothing of
/// one, so any cache may keep it. It names no session: until 2026-10-04 a
/// fresh session's cookie went on whatever it asked for first, and a cache
/// that kept that file would have given the session to everyone it served
/// the file to, who would then all share one cart.
fn respond_build(stream: &mut TcpStream, code: u16, mime: &str, body: &[u8]) {
    write_response(stream, code, mime, &runs_nothing(mime), body);
}

/// **Every answer's type is its type** (`csp`): `nosniff`, so a browser
/// runs as a script only what is served as one, and applies as a
/// stylesheet only what is served as one (Fetch, "Should response to
/// request be blocked due to nosniff?"). A page's policy allows this
/// origin's scripts (`'self'`): this is what makes that its JavaScript.
const NOSNIFF: &str = "x-content-type-options: nosniff\r\n";

fn write_response(stream: &mut TcpStream, code: u16, mime: &str, headers: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code} {}\r\ncontent-type: {mime}\r\ncontent-length: {}\r\n{NOSNIFF}{headers}connection: close\r\n\r\n",
        reason(code),
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    let mut sink = Vec::new();
    let _ = stream.take(0).read_to_end(&mut sink);
}

enum CommandOutcome {
    Answered(Answered),
    NotCurrent,
}
impl CommandOutcome {
    fn into_answer(self) -> Result<Answered, String> {
        match self {
            Self::Answered(a) => Ok(a),
            Self::NotCurrent => Err("unexpected scheduled disposition on a client command".into()),
        }
    }
}
enum CommitOutcome {
    Applied((data::Outboxed, Vec<i64>)),
    NoWrites,
    NotCurrent,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store's `app.pw`, with the cart's lines listed below the menu from
    /// a private query of their own: what T03's reference adds (ADR-0145).
    fn with_lines(app: &str) -> String {
        let query = "
// What the cart section shows of each line: its item's name, and how many.
type CartEntry = CartEntry {
    item_id: MenuItemId,
    name: String,
    quantity: Int,
}

// The name a menu gives an item.
fn name_in(menu: List<MenuItem>, item: MenuItemId) -> String !{} {
    List.fold(menu, \"\", fn(found, entry) if same_item(entry.id, item) { entry.name } else { found })
}

session query CartLines(id: StoreId, session: Session<SessionId>) -> Result<List<CartEntry>, CartError>
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            id, session
    invalidates_on CartChanged(session)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    let cart = Carts.current(session)?
    let menu = match Menus.for_store(id) {
        Ok(items) => items,
        Err(_) => [],
    }
    Ok(List.map(cart.lines, fn(line) CartEntry { item_id: line.item_id, name: name_in(menu, line.item_id), quantity: line.quantity.count }))
}
";
        let out = app
            .replacen("import context.{ current_session }", "import context.{ current_session }\nimport List", 1)
            .replacen("    MenuItemId, PositiveInt }", "    MenuItemId, PositiveInt, same_item }", 1)
            .replacen("\ncommand add_to_cart(", &format!("{query}\ncommand add_to_cart("), 1)
            .replacen(
                "    let cart = query Cart(current_session())\n",
                "    let cart = query Cart(current_session())\n    let lines = query CartLines(id, current_session())\n",
                1,
            )
            .replacen(
                "                <p id=\"cart-count\">{cart.line_count}</p>",
                "                <p id=\"cart-count\">{cart.line_count}</p>\n                <ul id=\"cart-lines\">\n                    {#each lines as line (line.item_id)}\n                        <li>{line.name} × {line.quantity}</li>\n                    {/each}\n                </ul>",
                1,
            );
        assert!(out.contains("CartLines(id, current_session())") && out.contains("cart-lines"));
        out
    }

    /// **That store, built by the compiler as `pw build` builds it, and
    /// served from what was written** (ADR-0145).
    fn lines_server() -> Served {
        lines_server_with(with_lines)
    }

    /// The store, its `app.pw` changed by `change`, built and served.
    fn lines_server_with(change: fn(&str) -> String) -> Served {
        served_from(change, None)
    }

    /// **The store with T04's setup**: a session's order, which the kitchen
    /// sets (E14, T04). The benchmark's own patch, applied as its harness
    /// applies it, so this and the task cannot drift apart.
    fn orders_server() -> Served {
        served_from(
            |app| app.to_string(),
            Some(include_str!(
                "../../../../benchmarks/tasks/T04-order-ready/setup/pleris.patch"
            )),
        )
    }

    /// The store's sources copied, `app.pw` changed by `change` and `patch`
    /// applied, built by the compiler as `pw build` builds them, and served
    /// from what was written.
    fn served_from(change: fn(&str) -> String, patch: Option<&str>) -> Served {
        served_from_patches(change, patch.as_slice())
    }

    /// The store, changed by `change` and then by each patch in order: a
    /// task's setup, and its reference after it. The benchmark's store
    /// (ADR-0156), which every task's patches are written against.
    fn served_from_patches(change: fn(&str) -> String, patches: &[&str]) -> Served {
        served_from_patches_in("benchmarks/baselines/pleris", change, patches)
    }

    /// [`served_from_patches`], the store's sources read from `base`, under
    /// the repository's root: the benchmark's store, or the canonical one,
    /// `examples`.
    fn served_from_patches_in(base: &str, change: fn(&str) -> String, patches: &[&str]) -> Served {
        let (dir, out) = built_from_patches_in(base, change, patches);
        Served {
            server: Server::from_build(out.clone(), out).expect("served"),
            _dir: dir,
        }
    }

    /// [`served_from_patches_in`]'s build, not yet served: its directory, and
    /// the build in it.
    fn built_from_patches_in(
        base: &str,
        change: fn(&str) -> String,
        patches: &[&str],
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let base = root.join(base);
        let dir = tempfile::TempDir::with_prefix("pw-served-").expect("a temporary directory");
        let work = dir.path().to_path_buf();
        let examples = work.join("examples");
        let pw_files = |dir: &std::path::Path| -> Vec<std::path::PathBuf> {
            let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
                .map(|e| e.expect("entry").path())
                .filter(|p| p.extension().is_some_and(|x| x == "pw"))
                .collect();
            paths.sort();
            paths
        };
        for rel in ["lib", "store"] {
            std::fs::create_dir_all(examples.join(rel)).expect("dir");
            for p in pw_files(&base.join(rel)) {
                let to = examples.join(rel).join(p.file_name().expect("name"));
                std::fs::copy(&p, &to).expect("copy");
            }
        }
        std::fs::copy(base.join("domain.pw"), examples.join("domain.pw")).expect("copy");
        let app = examples.join("store/app.pw");
        let changed = change(&std::fs::read_to_string(&app).expect("app"));
        std::fs::write(&app, changed).expect("app");
        for (i, patch) in patches.iter().enumerate() {
            let file = work.join(format!("{i}.patch"));
            std::fs::write(&file, patch).expect("patch");
            let applied = std::process::Command::new("git")
                .arg("apply")
                .arg(&file)
                .current_dir(&examples)
                .output()
                .expect("git runs");
            assert!(
                applied.status.success(),
                "the patch applies: {}",
                String::from_utf8_lossy(&applied.stderr)
            );
        }
        let mut sources: Vec<(String, String)> = Vec::new();
        let mut read = |p: std::path::PathBuf| {
            let src = std::fs::read_to_string(&p).expect("read");
            sources.push((p.display().to_string(), src));
        };
        for p in pw_files(&root.join("packages/pw-std")) {
            read(p);
        }
        for p in pw_files(&root.join("packages/pw-platform-web")) {
            read(p);
        }
        read(examples.join("domain.pw"));
        for p in pw_files(&examples.join("lib")) {
            read(p);
        }
        for p in pw_files(&examples.join("store")) {
            read(p);
        }
        let units: Vec<pw_core::check::Unit> = sources
            .into_iter()
            .map(|(path, src)| pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path,
                src,
            })
            .collect();
        let build = pw_core::build::build(&units).expect("the store builds");
        assert!(build.refusals().is_empty(), "{:?}", build.refusals());
        let out = work.join("build");
        build.write(&out).expect("the build is written");
        (dir, out)
    }

    /// **A build served from a directory of its own, removed when the test
    /// is done with it** (ADR-0158). The server reads handler modules from
    /// the directory on request, so the directory lives as long as it does.
    struct Served {
        server: Server,
        _dir: tempfile::TempDir,
    }

    impl std::ops::Deref for Served {
        type Target = Server;
        fn deref(&self) -> &Server {
            &self.server
        }
    }

    impl std::ops::DerefMut for Served {
        fn deref_mut(&mut self) -> &mut Server {
            &mut self.server
        }
    }

    #[test]
    fn an_order_the_kitchen_sets_is_the_order_query_s_value() {
        let s = orders_server();
        let (none, _) = s.serve_document("a");
        assert!(visible(&none).contains("No order yet."), "{none}");
        s.order_set_untold("a", "preparing");
        let (preparing, _) = s.serve_document("a");
        assert!(
            visible(&preparing).contains("Your order is being prepared."),
            "{preparing}"
        );
        // Another session has none.
        let (other, _) = s.serve_document("b");
        assert!(visible(&other).contains("No order yet."), "{other}");
    }

    #[test]
    fn a_page_whose_values_cannot_be_read_is_answered_and_the_server_goes_on() {
        let s = orders_server();
        // A status the program's type does not have: the component's types
        // refuse what the host gives, and the query fails.
        s.order_set_untold("a", "ready");
        let refused = s
            .serve_document_with_entries("a")
            .expect_err("a page that cannot be shown");
        assert!(refused.contains("`StorePage`'s queries"), "{refused}");
        // Nothing was left held: another session's page is served.
        let (other, _) = s.serve_document("b");
        assert!(visible(&other).contains("No order yet."), "{other}");
    }

    #[test]
    fn a_served_page_whose_values_fail_is_told_to_read_itself_again() {
        let s = orders_server();
        s.serve_document("a");
        s.order_set_untold("a", "ready");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("the command commits");
        let queue = s.pending.lock().expect("pending");
        assert!(
            queue[&latest(&queue, "a")]
                .frames
                .iter()
                .any(|(_, f)| matches!(
                    f,
                    StreamFrame::Recovery {
                        recovery: Recovery::Reload,
                        ..
                    }
                )),
            "{:?}",
            queue[&latest(&queue, "a")].frames
        );
    }

    /// **The store with recommendations streamed below its cart** (ADR-0148),
    /// their failed arm telling a declared error from the host's.
    fn with_recommendations(app: &str) -> String {
        app.replacen("import Menus\n", "import Menus\nimport Recommender\n", 1)
            .replacen(
                "MenuItemId, PositiveInt }",
                "MenuItemId, PositiveInt, Recommendation, RecommendationError }",
                1,
            )
            .replacen(
                "public query Store(",
                "public query Recommendations(id: StoreId) -> Result<List<Recommendation>, RecommendationError>
    freshness     0.seconds
    consistency   eventual
    cache         shared
    key           id
    concurrency   one_per_key
    on_key_change cancel
    delivery      streamed
    timeout       1.seconds
{
    Recommender.for_store(id)
}

public query Store(",
                1,
            )
            .replacen(
                "\n        </main>",
                "
            <section aria-label=\"Recommendations\">
                <stream query={Recommendations(id)}>
                    <placeholder><p>Finding recommendations</p></placeholder>
                    <ready as={items}>
                        <ul>{#each items as item (item.id)}<li>{item.name}</li>{/each}</ul>
                    </ready>
                    <failed as={why}>{#match why}{:Some(e)}<p>Declined</p>{:None}<p>Unavailable</p>{/match}</failed>
                </stream>
            </section>
        </main>",
                1,
            )
    }

    /// What one request to `s` answers, chunk by chunk, each with when it
    /// arrived: the connection served by `handle`, as a browser's is.
    fn fetched(s: &Server, path: &str) -> Vec<(std::time::Duration, String)> {
        fetched_as(s, path, None)
    }

    /// [`fetched`], the request carrying `session`'s cookie when given.
    fn fetched_as(
        s: &Server,
        path: &str,
        session: Option<&str>,
    ) -> Vec<(std::time::Duration, String)> {
        fetched_stepwise(s, path, session, |_, _| {})
    }

    /// [`fetched_as`], **telling `heard` all the response has carried so far
    /// each time a chunk of it arrives**, before the next is read: a test
    /// that holds a source at a gate lets it go when the response shows what
    /// it waits for (an event), where a delay it guessed was a silence a slow
    /// machine made too. And once more, last, with `over` true, when the
    /// connection closes: the response is over, and what stood then is asked
    /// there. (This returns when the server's handler does, which may be long
    /// after: a source it gave up on is waited for, so a test that holds one
    /// lets it go when the response is over.)
    fn fetched_stepwise(
        s: &Server,
        path: &str,
        session: Option<&str>,
        mut heard: impl FnMut(&str, bool),
    ) -> Vec<(std::time::Duration, String)> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let at = listener.local_addr().expect("address");
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let (stream, _) = listener.accept().expect("accept");
                handle(s, stream);
            });
            let mut client = TcpStream::connect(at).expect("connect");
            let cookie = session
                .map(|id| format!("Cookie: pw-session={id}\r\n"))
                .unwrap_or_default();
            // Started before the request is written: the server starts
            // nothing before it reads the request, so a part it sends after
            // a delay is never measured sooner than the delay. Started after,
            // a recommendation delayed 600 ms was measured at 599.75 ms on CI
            // (run 37899457744), the server's clock ahead of this one.
            let started = std::time::Instant::now();
            client
                .write_all(format!("GET {path} HTTP/1.1\r\nHost: t\r\n{cookie}\r\n").as_bytes())
                .expect("request");
            let mut chunks = Vec::new();
            let mut buf = [0u8; 65536];
            // A character a read ends inside is told with the read that
            // completes it. Decoded alone, its first bytes were U+FFFD, and a
            // test comparing the page's text failed by where the reads fell:
            // under load, 煮's "にしめる" read as "に\u{fffd}\u{fffd}める" (W6's
            // finding, 2026-10-10). Bytes that are no UTF-8 are refused, not
            // replaced.
            let mut pending: Vec<u8> = Vec::new();
            loop {
                let n = client.read(&mut buf).expect("read");
                if n == 0 {
                    assert!(pending.is_empty(), "the response ends inside a character");
                    // The close, which is what tells a browser the document
                    // is complete: the last chunk, empty, when it came. Told
                    // too, as the last of what is heard: a test asks what
                    // stood when the response was over.
                    chunks.push((started.elapsed(), String::new()));
                    let so_far: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
                    heard(&so_far, true);
                    break;
                }
                pending.extend_from_slice(&buf[..n]);
                let whole = match std::str::from_utf8(&pending) {
                    Ok(text) => text.len(),
                    // Ends inside a character: the rest comes next.
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    Err(e) => panic!("the response is no UTF-8: {e}"),
                };
                if whole > 0 {
                    let text =
                        String::from_utf8(pending.drain(..whole).collect()).expect("decoded above");
                    chunks.push((started.elapsed(), text));
                    let so_far: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
                    heard(&so_far, false);
                }
            }
            chunks
        })
    }

    /// **The recommender, held at a gate** a test lets it go through
    /// (ADR-0223): the gate.
    fn hold_recommender(s: &Server) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *s.store.faults().recommender.lock().expect("recommender") = Recommender {
            delay_ms: 0,
            fail: None,
            gate: Some(gate.clone()),
        };
        gate
    }

    /// **A program whose budgets are a minute**, for a test whose subject is
    /// not a budget: a source held at a gate for the test to hear what comes
    /// first must not have its wait ended by a budget a slow machine ran out
    /// before the test let it go.
    fn patient(app: &str) -> String {
        let mut out: Vec<String> = app
            .lines()
            .map(|line| match line.trim_start().strip_prefix("timeout") {
                Some(rest) if rest.trim_end().ends_with(".seconds") => {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    format!("{indent}timeout       60.seconds")
                }
                _ => line.to_string(),
            })
            .collect();
        if app.ends_with('\n') {
            out.push(String::new());
        }
        out.join("\n")
    }

    fn recommend(s: &Server, delay_ms: u64, fail: Option<&str>) {
        *s.store.faults().recommender.lock().expect("recommender") = Recommender {
            delay_ms,
            fail: fail.map(str::to_string),
            ..Recommender::default()
        };
    }

    /// What the recommendations' region shows: the arm the document carries
    /// for it, in place or as its patch. Not the page: the menu has a Cold
    /// Brew too, so a page holds it whatever the region shows.
    fn region(page: &str) -> String {
        let at = page
            .find("aria-label=\"Recommendations\"")
            .expect("the region");
        let rest = &page[at..];
        match rest.find("<?start") {
            // Pending: the patch, after the document.
            Some(_) => page[page.find("<template for=").expect("the patch")..].to_string(),
            None => rest[..rest.find("</section>").expect("its end")].to_string(),
        }
    }

    #[test]
    fn a_streamed_region_follows_its_document_in_the_same_response() {
        // The recommender is held at a gate, and let go when the response
        // shows the document, its region pending, and its runtime started:
        // what had come by then is a fact the test sets, where a delay of 400
        // ms, and bounds of 300 ms on the document and 350 on the arm, were
        // silences a slow machine made too. (The budget is a minute: it is
        // not the subject.)
        let s = served_from(|app| patient(&with_recommendations(app)), None);
        let recommender = hold_recommender(&s);
        let mut document: Option<String> = None;
        let chunks = fetched_stepwise(&s, "/StorePage.html", None, |so_far, _| {
            if document.is_none()
                && so_far.contains("<?start name=")
                && so_far.contains("import(\"/pw-runtime.mjs\")")
            {
                document = Some(so_far.to_string());
                recommender.open();
            }
        });
        let whole: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        // The document first, its region pending, and its runtime started:
        // the source was held until they were heard, so the region's arm,
        // which it fills, could not have come with them.
        let shell = document.expect("a pending region and the runtime came");
        assert!(
            !shell.contains("<template for="),
            "the region's arm came with the document: {shell}"
        );
        assert!(
            !whole.contains("content-length"),
            "a streamed document's length is not known first"
        );
        // The arm, the comment that says it has all arrived (ADR-0223), and
        // then the document's end, and nothing after it.
        let arm = &whole[whole.find("<template for=").expect("arm")..];
        assert!(
            arm.contains("Cortado") && arm.contains("Cold Brew"),
            "{arm}"
        );
        let name = arm["<template for=\"".len()..]
            .split('"')
            .next()
            .expect("its region");
        assert!(
            whole.ends_with(&format!("</template><!--/{name}--></body>\n</html>\n")),
            "{whole}"
        );
    }

    #[test]
    fn a_region_past_its_budget_is_given_the_host_s_failure_when_its_budget_is_spent() {
        let s = served_from(with_recommendations, None);
        // The recommender is held at a gate for the whole response, past a
        // one-second budget: the response ends because the budget was spent,
        // not because the source answered (it cannot have), which a bound of
        // 2.5 s on a source of 3 s measured with the machine's speed.
        let recommender = hold_recommender(&s);
        let mut at_its_gate = false;
        let mut held_at_the_close = None;
        let chunks = fetched_stepwise(&s, "/StorePage.html", None, |_, over| {
            // The source is at its gate before the response is read on: that
            // it is still there when the response ends is then a fact, not a
            // thread that was slow to start.
            if !std::mem::replace(&mut at_its_gate, true) {
                recommender.held(1);
            }
            // **The response is over, its connection closed with its last
            // region, while the source it gave up on is held still**: the
            // server does not wait for an abandoned source to end the
            // response. Asked as the close is heard, and the source let go
            // then, for the handler waits for it. (Ended when the source's
            // thread was, the connection closes after the gate's ten seconds,
            // and the source has left by then. A bound of 2.5 s on a source
            // of 3 s said the same, by the machine's speed.)
            if over {
                held_at_the_close = Some(recommender.waiting_now());
                recommender.open();
            }
        });
        let whole: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        let arm = &whole[whole.find("<template for=").expect("arm")..];
        assert!(arm.contains("Unavailable"), "{arm}");
        assert!(
            !arm.contains("Cortado"),
            "the held recommender answered: {arm}"
        );
        assert_eq!(
            held_at_the_close,
            Some(1),
            "the response ended only when the abandoned source did"
        );
    }

    #[test]
    fn a_declared_error_and_a_host_s_failure_are_shown_apart() {
        let s = served_from(with_recommendations, None);
        recommend(&s, 0, Some("declared"));
        let declared: String = fetched(&s, "/StorePage.html")
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert!(declared.contains("<p>Declined</p>"), "{declared}");
        recommend(&s, 0, Some("host"));
        let host: String = fetched(&s, "/StorePage.html")
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert!(host.contains("<p>Unavailable</p>"), "{host}");
    }

    /// The same, with the recommendations kept five minutes.
    fn with_kept_recommendations(app: &str) -> String {
        with_recommendations(app).replacen(
            "    freshness     0.seconds\n    consistency   eventual",
            "    freshness     5.minutes\n    consistency   eventual",
            1,
        )
    }

    #[test]
    fn a_declared_error_is_given_and_not_kept() {
        let s = served_from(with_kept_recommendations, None);
        recommend(&s, 0, Some("declared"));
        let page = |s: &Server| -> String {
            fetched(s, "/StorePage.html")
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        assert!(region(&page(&s)).contains("<p>Declined</p>"));
        // The recommender answers again, and the next page asks it: what it
        // declined is not served for five minutes.
        recommend(&s, 0, None);
        let answered = region(&page(&s));
        assert!(
            answered.contains("Cortado") && !answered.contains("Declined"),
            "{answered}"
        );
        // An answer is kept, as its policy says.
        recommend(&s, 0, Some("declared"));
        let kept = region(&page(&s));
        assert!(
            kept.contains("Cortado") && !kept.contains("Declined"),
            "{kept}"
        );
    }

    /// T10's store: a session's delivery estimate, which the page waits for.
    const ESTIMATE_SETUP: &str =
        include_str!("../../../../benchmarks/tasks/T10-estimate-states/setup/pleris.patch");
    /// And T10's reference after it: the estimate streamed, failure and all.
    const ESTIMATE_STREAMED: &str =
        include_str!("../../../../benchmarks/tasks/T10-estimate-states/reference/pleris.patch");

    fn estimate(s: &Server, session: &str, fail: Option<&str>, minutes: i64) {
        s.store
            .faults()
            .estimators
            .lock()
            .expect("estimators")
            .insert(
                session.to_string(),
                Estimator {
                    delay_ms: 0,
                    fail: fail.map(str::to_string),
                    ..Estimator::default()
                },
            );
        // What it estimates is the session's data (track `store-pg`).
        s.set_estimate(session, minutes, None).expect("estimated");
    }

    fn page_as(s: &Server, session: &str) -> String {
        fetched_as(s, "/StorePage.html", Some(session))
            .into_iter()
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn a_session_s_estimate_is_its_estimator_s() {
        let s = served_from_patches(|app| app.to_string(), &[ESTIMATE_SETUP]);
        estimate(&s, "a", None, 35);
        assert!(visible(&page_as(&s, "a")).contains("Delivery in 35 min"));
        // Another session's estimator is its own.
        assert!(visible(&page_as(&s, "b")).contains("Delivery in 25 min"));
        // A page that waits for an estimate that fails is unavailable, as
        // any page whose values cannot be read is (ADR-0147): what T10 fixes.
        estimate(&s, "a", Some("down"), 35);
        assert!(page_as(&s, "a").starts_with("HTTP/1.1 503"));
    }

    #[test]
    fn a_failed_estimate_fills_its_region_and_the_page_is_served() {
        let s = served_from_patches(|app| app.to_string(), &[ESTIMATE_SETUP, ESTIMATE_STREAMED]);
        estimate(&s, "a", Some("down"), 35);
        let failed = page_as(&s, "a");
        assert!(failed.starts_with("HTTP/1.1 200"), "{failed}");
        assert!(visible(&failed).contains("Espresso"), "{failed}");
        // In place or as its patch: an estimator that fails at once can
        // answer before the document is rendered, and what has settled then
        // is rendered in place (`Settling::for_document`). Until 2026-10-07
        // this read the patch alone, and failed on CI when the estimate won.
        let shown = slot(&failed, "Delivery");
        assert!(
            shown.contains("Delivery estimate unavailable"),
            "{shown}: {failed}"
        );
        // And a declared error is a failure too, where the arm binds none.
        estimate(&s, "b", Some("declared"), 35);
        let declared = page_as(&s, "b");
        assert!(
            visible(&declared).contains("Delivery estimate unavailable"),
            "{declared}"
        );
        estimate(&s, "c", None, 35);
        assert!(visible(&page_as(&s, "c")).contains("Delivery in 35 min"));
    }

    #[test]
    fn a_menu_changed_at_its_source_shows_once_its_value_is_read_again() {
        // A materialized fragment is kept while it shows the `Menu` query's
        // value. Until 2026-10-03 it was kept until a command invalidated it,
        // so a menu changed at its source, read again once its freshness was
        // spent, was not what a new document showed.
        let s = served_from(|app| app.to_string(), None);
        let (before, _) = s.serve_document("a");
        assert!(visible(&before).contains("Cortado"), "{before}");
        s.menu_untold(MenuOp::Remove {
            id: "cortado".into(),
        });
        // The query's freshness spent: its value is read again.
        s.queries.invalidate("store.page.Menu");
        let (after, _) = s.serve_document("b");
        assert!(!visible(&after).contains("Cortado"), "{after}");
        assert!(visible(&after).contains("Espresso"), "{after}");
        // And the page open is told first, the item removed where it is
        // (ADR-0178). Until 2026-10-04 it kept the fragment's version before.
        let queue = s.pending.lock().expect("pending");
        let (_, waiting) = queue
            .iter()
            .find(|((session, _), _)| session == "a")
            .expect("the page open");
        assert!(
            waiting.frames.iter().any(|(_, f)| matches!(
                f,
                StreamFrame::Patch(p) if matches!(p.operation, PatchOp::RemoveInstance { .. })
            )),
            "{:?}",
            waiting.frames
        );
    }

    /// T09's store: a notice board the page reads, kept five minutes.
    const NOTICE_SETUP: &str =
        include_str!("../../../../benchmarks/tasks/T09-notice-freshness/setup/pleris.patch");

    fn notice_calls(s: &Server) -> u64 {
        s.calls
            .lock()
            .expect("calls")
            .get("store:data/notices#current")
            .copied()
            .unwrap_or(0)
    }

    #[test]
    fn a_notice_is_kept_as_its_freshness_says_and_each_ask_is_counted() {
        let s = served_from_patches(|app| app.to_string(), &[NOTICE_SETUP]);
        let (first, _) = s.serve_document("a");
        assert!(visible(&first).contains("Open until 7 pm"), "{first}");
        assert_eq!(notice_calls(&s), 1);
        // Posted at the source, and nothing is told: the kept answer is
        // served, and the board is not asked again.
        s.untold(
            "",
            "store:host/notices#post",
            &[
                Val::String(STORE_ID.into()),
                Val::String("Closing early".into()),
            ],
        );
        let (kept, _) = s.serve_document("b");
        assert!(visible(&kept).contains("Open until 7 pm"), "{kept}");
        assert_eq!(notice_calls(&s), 1);
        // Once its freshness is spent, the board is asked, once.
        s.queries.invalidate("store.page.Notice");
        let (fresh, _) = s.serve_document("c");
        assert!(visible(&fresh).contains("Closing early"), "{fresh}");
        assert_eq!(notice_calls(&s), 2);
    }

    /// **A quantity that is no `PositiveInt` is refused at the boundary**
    /// (ADR-0179, charter §7.1's "explicit decoding at every external
    /// boundary"). A browser's request is a claim, and one forged with a
    /// quantity of 0 or less is refused before the command runs: nothing is
    /// written, and the cart is as it was. Until 2026-10-04 it was accepted
    /// as an `s64`, and the data layer added it to the line.
    #[test]
    fn a_quantity_that_is_no_positive_int_is_refused_at_the_boundary() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_document("a");
        for forged in [0, -3] {
            let refused = s.command_answer(
                ADD,
                "a",
                &[shown("espresso"), serde_json::json!(forged)],
                Some(&format!("forged-{forged}")),
            );
            let why = refused.expect_err("refused before the command runs");
            assert!(
                why.contains("`domain.PositiveInt`") && why.contains("value >= 1"),
                "{why}"
            );
            assert!(why.contains(&format!("{forged}")), "{why}");
        }
        let lines = s.cart_lines("a");
        assert!(lines.is_empty(), "the cart holds {:?}", lines.len());
        // A quantity the type holds is added, as before.
        let added = s
            .command_answer(
                ADD,
                "a",
                &[shown("espresso"), serde_json::json!(2)],
                Some("press-1"),
            )
            .expect("a well-formed request");
        assert!(added.committed);
        assert_eq!(s.cart_value("a"), 2);
    }

    /// **A count the data layer holds that is no `PositiveInt` is never
    /// shown** (ADR-0179): a line of 0 written around the program, as a
    /// database's row can be, makes the cart's read fail, by name, and the
    /// page is not served from it.
    #[test]
    fn a_stored_line_of_nothing_is_a_failed_read() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_document("a");
        let added = s
            .command_answer(
                ADD,
                "a",
                &[shown("espresso"), serde_json::json!(1)],
                Some("press-1"),
            )
            .expect("a well-formed request");
        assert!(added.committed);
        // Around the program, as a row can be. A database that holds the
        // invariant itself refuses the row (PostgreSQL's
        // `cart_lines_quantity_check`), and there is nothing to read.
        if let Err(refused) = s.store.line_around(&s.owner("a"), 0, 0) {
            assert!(refused.contains("cart_lines_quantity_check"), "{refused}");
            return;
        }
        let why = match s.serve_store_document("a", STORE_ID) {
            Err(Unread::Failed(why)) => why,
            other => panic!("served from a line of 0: {other:?}"),
        };
        assert!(
            why.contains("answered what breaks an invariant")
                && why.contains("`ok.lines[0].quantity` is 0"),
            "{why}"
        );
    }

    #[test]
    fn an_item_sold_out_since_the_page_is_refused_by_name() {
        // Charter §15.4 and §15.6 test 10, on the canonical store: the add
        // reads the item's availability inside the command, and a sold-out
        // item is refused with its typed error, which the handler is
        // answered (ADR-0157). Nothing commits, and a retried interaction is
        // given the same answer.
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_document("a");
        s.stock_untold("cortado", false);
        let cortado = [shown("cortado"), serde_json::json!(1)];
        let refused = s
            .command_answer(ADD, "a", &cortado, Some("press-1"))
            .expect("a well-formed request");
        assert!(!refused.committed);
        assert_eq!(
            refused.result,
            Some(serde_json::json!({
                "$case": "err",
                "value": { "$case": "item-unavailable", "value": "cortado" }
            }))
        );
        assert_eq!(
            s.cart_value("a"),
            0,
            "the refused add left the cart as it was"
        );
        let again = s
            .command_answer(ADD, "a", &cortado, Some("press-1"))
            .expect("a well-formed request");
        assert_eq!(again, refused, "a retry is given the first answer");
        // Back in stock, the next press adds.
        s.stock_untold("cortado", true);
        let added = s
            .command_answer(ADD, "a", &cortado, Some("press-2"))
            .expect("a well-formed request");
        assert!(added.committed);
        // Answered `Ok`, without the cart: the page learns that from the
        // resource.
        assert_eq!(added.result, Some(serde_json::json!({ "$case": "ok" })));
        assert_eq!(s.cart_value("a"), 1);
    }

    /// T07's store: the menu by category, a binding a signal keys, its
    /// stale work `keep` (ADR-0152).
    const BROWSE_SETUP: &str =
        include_str!("../../../../benchmarks/tasks/T07-stale-category/setup/pleris.patch");
    /// T07's reference, `cancel`, and its unsafe patch, `supersede`.
    const BROWSE_CANCEL: &str =
        include_str!("../../../../benchmarks/tasks/T07-stale-category/reference/pleris.patch");
    const BROWSE_SUPERSEDE: &str =
        include_str!("../../../../benchmarks/tasks/T07-stale-category/unsafe/pleris.patch");

    /// The names the page shows of the binding `browsing`, as recorded.
    fn browsed(s: &Server, session: &str) -> Vec<String> {
        let document = latest(&s.shown.lock().expect("shown"), session).1;
        browsed_in(s, session, document)
    }

    /// [`browsed`], for one document of the session (ADR-0161).
    fn browsed_in(s: &Server, session: &str, document: u64) -> Vec<String> {
        let shown = s.shown.lock().expect("shown");
        shown[&(session.to_string(), document)].lists["browsing"]
            .iter()
            .filter_map(|item| match item {
                Value::Record(fields) => match fields.get("name") {
                    Some(Value::Text(name)) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn category(name: &str) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([("category".to_string(), serde_json::json!(name))])
    }

    const STAYED: &(dyn Fn() -> bool + Sync) = &|| false;

    #[test]
    fn a_keyed_read_is_applied_as_one_patch_set() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_CANCEL]);
        let (page, cursor) = s.serve_document("a");
        // The document shows the signal's first key: every item.
        assert!(visible(&page).contains("Cold Brew"), "{page}");
        assert_eq!(browsed(&s, "a"), ["Espresso", "Cortado", "Cold Brew"]);
        let read = s.read_keyed("a", "browsing", 1, cursor, &category("cold"), STAYED);
        assert_eq!(read, Ok(KeyOutcome::Applied));
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
        // One patch set, in the session's frames, after the document.
        let queue = s.pending.lock().expect("pending");
        let sets: Vec<&PatchSet> = queue[&latest(&queue, "a")]
            .frames
            .iter()
            .filter_map(|(_, f)| match f {
                StreamFrame::PatchSet(set) => Some(set),
                _ => None,
            })
            .collect();
        assert_eq!(sets.len(), 1);
        assert!(!sets[0].patches.is_empty());
        drop(queue);
        // A command re-reads the binding for the key the page shows.
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("the command commits");
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
    }

    #[test]
    fn a_read_older_than_the_latest_is_not_applied() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_CANCEL]);
        let (_, cursor) = s.serve_document("a");
        assert_eq!(
            s.read_keyed("a", "browsing", 2, cursor, &category("cold"), STAYED),
            Ok(KeyOutcome::Applied)
        );
        // The first read, arriving after the second.
        assert_eq!(
            s.read_keyed("a", "browsing", 1, cursor, &category("hot"), STAYED),
            Ok(KeyOutcome::Superseded)
        );
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
        // A second page of the session, as a second tab: each document is its
        // own (ADR-0161). Until 2026-10-03 it replaced the first, and a read
        // the first asked for was dropped.
        let (_, newer) = s.serve_document("a");
        // The new page shows the first key, as its browser holds it.
        assert_eq!(browsed(&s, "a"), ["Espresso", "Cortado", "Cold Brew"]);
        // A read the first page asks for is the first page's ...
        assert_eq!(
            s.read_keyed("a", "browsing", 3, cursor, &category("hot"), STAYED),
            Ok(KeyOutcome::Applied)
        );
        assert_eq!(browsed_in(&s, "a", cursor), ["Espresso", "Cortado"]);
        // ... and leaves the second as it was.
        assert_eq!(browsed(&s, "a"), ["Espresso", "Cortado", "Cold Brew"]);
        assert_eq!(
            s.read_keyed("a", "browsing", 1, newer, &category("cold"), STAYED),
            Ok(KeyOutcome::Applied)
        );
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
        // A read for a document the server does not hold is no page's, and
        // takes nothing: no reads kept for it, no flight held.
        assert_eq!(
            s.read_keyed("a", "browsing", 9, newer + 1_000, &category("hot"), STAYED),
            Ok(KeyOutcome::Superseded)
        );
        assert!(
            !s.keyed
                .lock()
                .expect("keyed")
                .contains_key(&("a".to_string(), newer + 1_000))
        );
    }

    /// **The menu's `hot` category, held at a gate** (ADR-0223): a read of it
    /// stays in flight until a test lets it go, or until it is stopped. A
    /// test that needs a slow read in flight while it does something else
    /// holds it, rather than outwaiting a delay it guesses: Hot's read took
    /// a second, and what the test did meanwhile took as long as the machine
    /// made it.
    fn hold_hot(s: &Server) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *s.store.faults().categories.lock().expect("categories") = Categories {
            slow: Some("hot".to_string()),
            delay_ms: 0,
            gate: Some(gate.clone()),
        };
        gate
    }

    /// How many reads of a category were stopped part way.
    fn stopped_reads(s: &Server) -> u64 {
        s.store
            .faults()
            .category_stopped
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// **A test's: `tell` is called on this thread each time a `keep` read
    /// finds its binding's turn held**, just before it waits for the read
    /// before it, until the returned guard is dropped.
    fn tell_when_a_turn_waits(tell: impl Fn() + 'static) -> TurnTelling {
        TURN_WAITING.with(|waiting| *waiting.borrow_mut() = Some(Box::new(tell)));
        TurnTelling
    }

    struct TurnTelling;

    impl Drop for TurnTelling {
        fn drop(&mut self) {
            TURN_WAITING.with(|waiting| *waiting.borrow_mut() = None);
        }
    }

    #[test]
    fn under_cancel_a_newer_key_stops_the_old_keys_read() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_CANCEL]);
        // Hot's read is held at a gate that is never opened here: it ends
        // only by being stopped, which is what is asserted.
        let hot_gate = hold_hot(&s);
        let (_, cursor) = s.serve_document("a");
        let (hot, cold) = std::thread::scope(|scope| {
            let hot =
                scope.spawn(|| s.read_keyed("a", "browsing", 1, cursor, &category("hot"), STAYED));
            // Hot's read is in flight, held, before Cold is asked for.
            hot_gate.held(1);
            let cold = s.read_keyed("a", "browsing", 2, cursor, &category("cold"), STAYED);
            (hot.join().expect("hot"), cold)
        });
        assert_eq!(hot, Ok(KeyOutcome::Superseded));
        assert_eq!(cold, Ok(KeyOutcome::Applied));
        // Stopped part way, not read to its end.
        assert_eq!(stopped_reads(&s), 1);
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
    }

    #[test]
    fn under_supersede_the_old_keys_read_runs_on_unshown() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_SUPERSEDE]);
        let hot_gate = hold_hot(&s);
        let (_, cursor) = s.serve_document("a");
        let (hot, cold) = std::thread::scope(|scope| {
            let hot =
                scope.spawn(|| s.read_keyed("a", "browsing", 1, cursor, &category("hot"), STAYED));
            hot_gate.held(1);
            // Cold is not kept waiting for Hot, whose read is still held.
            let cold = s.read_keyed("a", "browsing", 2, cursor, &category("cold"), STAYED);
            // Hot's read is let go of, and runs to its end, unshown.
            hot_gate.open();
            (hot.join().expect("hot"), cold)
        });
        assert_eq!(hot, Ok(KeyOutcome::Superseded));
        assert_eq!(cold, Ok(KeyOutcome::Applied));
        assert_eq!(stopped_reads(&s), 0);
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
    }

    #[test]
    fn under_keep_the_new_key_is_read_after_the_old_one_ends() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP]);
        let hot_gate = hold_hot(&s);
        let (_, cursor) = s.serve_document("a");
        let (waits, waiting) = std::sync::mpsc::channel();
        let (hot, cold) = std::thread::scope(|scope| {
            let hot =
                scope.spawn(|| s.read_keyed("a", "browsing", 1, cursor, &category("hot"), STAYED));
            hot_gate.held(1);
            let s = &s;
            let cold = scope.spawn(move || {
                let _telling = tell_when_a_turn_waits(move || {
                    let _ = waits.send(());
                });
                s.read_keyed("a", "browsing", 2, cursor, &category("cold"), STAYED)
            });
            // Cold's read found Hot's turn held, and waits for Hot's read,
            // which is held: it cannot end before Hot's does.
            waiting
                .recv_timeout(MUST_HAPPEN)
                .expect("Cold's read never waited for Hot's");
            assert!(!cold.is_finished(), "Cold's read ended before Hot's");
            hot_gate.open();
            (hot.join().expect("hot"), cold.join().expect("cold"))
        });
        // Hot's read ends, and is not shown: Cold was asked for meanwhile.
        assert_eq!(hot, Ok(KeyOutcome::Superseded));
        assert_eq!(cold, Ok(KeyOutcome::Applied));
        assert_eq!(stopped_reads(&s), 0);
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
    }

    /// How long a test waits for an event that must come, at most: a bound
    /// on a hang, never on the speed of the machine.
    const MUST_HAPPEN: std::time::Duration = std::time::Duration::from_secs(10);

    #[test]
    fn a_flight_another_reader_waits_for_is_not_stopped() {
        // ADR-0152: `cancel` lets go of a read's flight, and `pw-resource`
        // stops a flight only when nobody holds it. Here session `a` reads
        // Hot, and session `b`, whose page shows Hot, reads it again for a
        // command while `a`'s read is in flight, sharing it. `a` then moves
        // to Cold: the flight goes on for `b`.
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_CANCEL]);
        let (_, b) = s.serve_document("b");
        assert_eq!(
            s.read_keyed("b", "browsing", 1, b, &category("hot"), STAYED),
            Ok(KeyOutcome::Applied)
        );
        // Hot is held at a gate, so its flight is in flight for as long as
        // the steps below take, however long that is. (Hot's read took a
        // second: the nightly of 2026-10-11 saw `a` move on before `b` had
        // joined, and train-3's, train-11's and bounded-race's `just ci`
        // saw the flight stopped, at loads of 20 to 50, when the steps took
        // longer than that second and the command's read began a flight of
        // its own, which nobody held when it was let go.)
        let hot_gate = hold_hot(&s);
        let (_, a) = s.serve_document("a");
        // Each read that joined another's flight, as the query runtime traces it.
        let joined = || {
            s.queries
                .trace()
                .iter()
                .filter(|t| matches!(t, pw_resource::Trace::RequestDeduplicated { .. }))
                .count()
        };
        let before = joined();
        let hot_started = || {
            s.queries
                .trace()
                .iter()
                .filter(|t| {
                    matches!(t, pw_resource::Trace::RequestStarted { key, .. }
                    if key.resource == "store.page.MenuIn" && key.key == "\"47\"\u{1f}\"hot\"")
                })
                .count()
        };
        let hot_before = hot_started();
        std::thread::scope(|scope| {
            let hot = scope.spawn(|| s.read_keyed("a", "browsing", 1, a, &category("hot"), STAYED));
            // `a`'s read is in flight, held at the gate, before `b`'s
            // command asks for the same one.
            hot_gate.held(1);
            let command = scope.spawn(|| s.command(ADD, "b", &add("espresso", 1), false));
            // Until `b`'s read has joined `a`'s flight, whichever began it.
            let until = std::time::Instant::now() + MUST_HAPPEN;
            while joined() == before {
                assert!(
                    std::time::Instant::now() < until,
                    "`b`'s read never joined the flight"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(
                s.read_keyed("a", "browsing", 2, a, &category("cold"), STAYED),
                Ok(KeyOutcome::Applied)
            );
            // `a` has let go; the flight is `b`'s still. With the gate
            // closed, Hot's source must observe twice that it is not stopped.
            // This proves cancellation protection before the shared flight
            // completes; the flight count below also excludes a later retry.
            let asked = hot_gate.polls();
            hot_gate.polled(asked + 2);
            assert_eq!(
                stopped_reads(&s),
                0,
                "the flight was stopped; the runtime traced {:#?}",
                s.queries.trace()
            );
            hot_gate.open();
            assert_eq!(hot.join().expect("hot"), Ok(KeyOutcome::Superseded));
            command
                .join()
                .expect("command")
                .expect("the command commits");
        });
        // Cold's frames superseded Hot while it was in flight. The old
        // read must not treat those frames as a reason to fetch Hot again:
        // its first flight ran for `b`, and neither page needs another.
        assert_eq!(
            hot_started() - hot_before,
            1,
            "the superseded Hot read started another flight: {:#?}",
            s.queries.trace()
        );
        // Read to its end, for `b`: its page was not told to read itself
        // again.
        let queue = s.pending.lock().expect("pending");
        assert!(
            !queue[&latest(&queue, "b")]
                .frames
                .iter()
                .any(|(_, f)| matches!(f, StreamFrame::Recovery { .. })),
            "b was told to reload"
        );
        drop(queue);
        assert_eq!(browsed(&s, "b"), ["Espresso", "Cortado"]);
        assert_eq!(browsed(&s, "a"), ["Cold Brew"]);
    }

    #[test]
    fn a_browser_that_leaves_lets_go_of_its_read() {
        let s = served_from_patches(|app| app.to_string(), &[BROWSE_SETUP, BROWSE_CANCEL]);
        let hot_gate = hold_hot(&s);
        let (_, cursor) = s.serve_document("a");
        // The browser leaves once its read is in flight, held at the gate.
        let gone = std::sync::atomic::AtomicBool::new(false);
        let left = || gone.load(std::sync::atomic::Ordering::SeqCst);
        let read = std::thread::scope(|scope| {
            let read =
                scope.spawn(|| s.read_keyed("a", "browsing", 1, cursor, &category("hot"), &left));
            hot_gate.held(1);
            gone.store(true, std::sync::atomic::Ordering::SeqCst);
            read.join().expect("read")
        });
        // The read ended with the gate still closed: stopped part way.
        assert_eq!(read, Ok(KeyOutcome::Superseded));
        assert_eq!(stopped_reads(&s), 1);
        // Nothing it read is shown.
        assert_eq!(browsed(&s, "a"), ["Espresso", "Cortado", "Cold Brew"]);
    }

    /// T02's store: the kitchen's prep time, which every page asks for.
    const PREP_SETUP: &str =
        include_str!("../../../../benchmarks/tasks/T02-request-storm/setup/pleris.patch");
    /// T02's reference: pages that ask while an ask is under way share it.
    const PREP_SHARED: &str =
        include_str!("../../../../benchmarks/tasks/T02-request-storm/reference/pleris.patch");

    fn prep_calls(s: &Server) -> u64 {
        s.calls
            .lock()
            .expect("calls")
            .get("store:data/kitchen#prep-minutes")
            .copied()
            .unwrap_or(0)
    }

    /// **The kitchen, held at a gate** (ADR-0223): a read of its prep time
    /// stays in the kitchen until a test lets it go.
    fn hold_kitchen(s: &Server) -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        *s.store.faults().kitchen_gate.lock().expect("kitchen") = Some(gate.clone());
        gate
    }

    /// Eight documents served at once, each to its own session, with the
    /// kitchen held at `kitchen`: `heard` is called with the pages in flight,
    /// and says when the test has heard what it waits for, whereupon the
    /// kitchen is let go. How many times the kitchen was asked.
    fn eight_at_once(s: &Server, kitchen: &Gate, heard: impl FnOnce()) -> u64 {
        let before = prep_calls(s);
        std::thread::scope(|scope| {
            for i in 0..8 {
                scope.spawn(move || {
                    let (page, _) = s.serve_document(&format!("burst-{i}"));
                    assert!(visible(&page).contains("Ready in 12 min"), "{page}");
                });
            }
            heard();
            kitchen.open();
        });
        prep_calls(s) - before
    }

    #[test]
    fn pages_read_at_once_share_an_ask_their_query_says_is_one_per_key() {
        // ADR-0151: a document is read outside the subscriber table, so
        // eight pages are read at once, and share the kitchen's answer. The
        // kitchen is held at a gate until the first ask is in it and the
        // other seven have joined it: shared is what the trace says, where
        // a bound of 1.2 s on the eight pages measured the machine.
        let s = served_from_patches(|app| app.to_string(), &[PREP_SETUP, PREP_SHARED]);
        let kitchen = hold_kitchen(&s);
        let joined = || {
            s.queries
                .trace()
                .iter()
                .filter(|t| matches!(t, pw_resource::Trace::RequestDeduplicated { .. }))
                .count()
        };
        let asked = eight_at_once(&s, &kitchen, || {
            kitchen.held(1);
            until("seven pages joined the ask", || joined() >= 7);
        });
        assert!(asked <= 2, "the kitchen was asked {asked} times");
    }

    #[test]
    fn pages_read_at_once_each_ask_when_their_query_says_parallel() {
        let s = served_from_patches(|app| app.to_string(), &[PREP_SETUP]);
        let kitchen = hold_kitchen(&s);
        // Asked at once, all the same: the eight reads are in the kitchen
        // together, which one after another could not be.
        let asked = eight_at_once(&s, &kitchen, || kitchen.held(8));
        assert_eq!(asked, 8);
    }

    #[test]
    fn a_change_that_reaches_a_session_while_its_page_is_read_is_not_lost() {
        // ADR-0151: a document read before a change, whose frames reached
        // the session while it was read, is not installed; it is read again.
        let s = served_from_patches(|app| app.to_string(), &[]);
        s.serve_document("a");
        let doc: Doc = (
            "a".to_string(),
            s.documents
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        s.params
            .lock()
            .expect("params")
            .insert(doc.clone(), store_params(STORE_ID));
        let pushed = s.subscribed(&doc);
        let before = s.render_document(&doc, &[]).expect("the page reads");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("the command commits");
        let mut queue = s.pending.lock().expect("pending");
        assert!(
            s.installed(&mut queue, &doc, before, Some(pushed))
                .is_none()
        );
        drop(queue);
        // Served whole, it shows the change.
        let (page, _) = s.serve_document("a");
        let at = page.find("id=\"cart-count\"").expect("the count");
        let rest = &page[at..];
        let count = visible(&rest[rest.find('>').expect("its tag") + 1..]);
        assert!(count.starts_with('1'), "{page}");
    }

    #[test]
    fn a_command_asks_only_the_queries_its_page_shows_once() {
        // ADR-0151: the cart's count is read from the cart's binding alone.
        // A command asked every query the page reads for it, and then again
        // for the patches.
        let s = served_from_patches(|app| app.to_string(), &[PREP_SETUP]);
        s.serve_document("a");
        assert_eq!(prep_calls(&s), 1);
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("the command commits");
        // Once, for what the page shows after it.
        assert_eq!(prep_calls(&s), 2);
    }

    #[test]
    fn a_page_with_nothing_streamed_is_one_response_of_known_length() {
        let s = served_from(|app| app.to_string(), None);
        let whole: String = fetched(&s, "/StorePage.html")
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert!(whole.contains("content-length:"), "{whole}");
        assert!(!whole.contains("<?start"), "{whole}");
    }

    /// **A session's latest document** (ADR-0161): what a test that serves a
    /// session one page means by its subscriber.
    fn latest<V>(map: &BTreeMap<Doc, V>, session: &str) -> Doc {
        documents_of(map, session)
            .pop()
            .unwrap_or_else(|| panic!("no document for `{session}`"))
    }

    /// Each patch set queued for a session's latest document, in order.
    fn patch_sets(s: &Server, session: &str) -> Vec<PatchSet> {
        let queue = s.pending.lock().expect("pending");
        documents_of(&queue, session)
            .pop()
            .and_then(|doc| queue.get(&doc))
            .map(|w| {
                w.frames
                    .iter()
                    .filter_map(|(_, f)| match f {
                        StreamFrame::PatchSet(p) => Some(p.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// **An item as the canonical store's menu row shows it** (ADR-0172):
    /// what its Add sends `add_to_cart`.
    fn shown(id: &str) -> serde_json::Value {
        let name = default_menu()
            .into_iter()
            .chain(second_menu())
            .find(|(i, _)| i == id)
            .map(|(_, name)| name)
            .unwrap_or_else(|| panic!("no item `{id}`"));
        // Its store and its category (ADR-0181), as the data layer says.
        let store = match default_menu().iter().any(|(i, _)| i == id) {
            true => STORE_ID,
            false => SECOND_STORE.0,
        };
        let (category, category_name) = item_category(store, id);
        serde_json::json!({
            "description": item_description(id),
            "id": id,
            "store_id": store,
            "name": name,
            "price": item_price(id),
            "available": true,
            "category": { "id": category, "name": category_name },
        })
    }

    /// One patch, as `operation@part[:instance frames]`, for an assertion.
    fn said(t: &Targeted) -> String {
        let op = match &t.operation {
            PatchOp::ReplaceText { text } => format!("text {text:?}"),
            PatchOp::InsertBefore { instance, html } => {
                format!("insert before {} {}", instance.is_some(), visible(html))
            }
            PatchOp::InsertAfter { instance, html } => {
                format!("insert after {} {}", instance.is_some(), visible(html))
            }
            PatchOp::RemoveInstance { .. } => "remove".to_string(),
            PatchOp::MoveInstance { after, .. } => format!("move after {}", after.is_some()),
            PatchOp::ReplaceRange { html } => format!("range {:?}", visible(html)),
            other => format!("{other:?}"),
        };
        format!(
            "{op} @{}{}",
            t.target.part.0,
            if t.target.instances.is_empty() {
                ""
            } else {
                " in an instance"
            }
        )
    }

    /// Markup's text, its comments and tags dropped.
    fn visible(html: &str) -> String {
        // A style's or a script's text is not shown, and CSS's `>` is not a
        // tag's end (ADR-0187 moved the store's style into its head).
        let mut text = String::new();
        let mut rest = html;
        while let Some((at, open)) = ["<style", "<script"]
            .into_iter()
            .filter_map(|open| rest.find(open).map(|at| (at, open)))
            .min()
        {
            text.push_str(&rest[..at]);
            let close = if open == "<style" {
                "</style>"
            } else {
                "</script>"
            };
            rest = rest[at..]
                .find(close)
                .map_or("", |end| &rest[at + end + close.len()..]);
        }
        text.push_str(rest);
        let mut out = String::new();
        let mut depth = 0;
        for c in text.chars() {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                c if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out.trim().to_string()
    }

    #[test]
    fn a_list_a_session_s_query_fills_is_rendered() {
        let s = lines_server();
        s.command(ADD, "a", &add("espresso", 2), false)
            .expect("runs");
        let (page, _) = s.serve_document("a");
        let page = visible(&page);
        assert!(page.contains("Espresso × 2"), "the session's lines: {page}");
        // Another session's document lists none of them.
        let (other, _) = s.serve_document("b");
        let other = visible(&other);
        assert!(!other.contains("× 2"), "another session's: {other}");
    }

    #[test]
    fn a_change_patches_every_place_it_reaches_in_one_frame() {
        let s = lines_server();
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        let sets = patch_sets(&s, "a");
        assert_eq!(sets.len(), 1, "one frame for the change: {sets:?}");
        let got: Vec<String> = sets[0].patches.iter().map(said).collect();
        // The count, and the first line, at the head of the list.
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].starts_with("text \"1\""), "{got:?}");
        assert!(
            got[1].starts_with("insert before false Espresso × 1"),
            "{got:?}"
        );
    }

    #[test]
    fn an_item_that_stays_is_set_in_place() {
        let s = lines_server();
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        let got: Vec<String> = patch_sets(&s, "a")[0].patches.iter().map(said).collect();
        // The count, and the line's quantity inside its instance: its nodes
        // stay, nothing is removed or inserted.
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].starts_with("text \"2\""), "{got:?}");
        assert!(
            got[1].starts_with("text \"2\"") && got[1].ends_with("in an instance"),
            "{got:?}"
        );
    }

    #[test]
    fn a_new_item_goes_after_the_one_before_it() {
        let s = lines_server();
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.serve_document("a");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        let got: Vec<String> = patch_sets(&s, "a")[0].patches.iter().map(said).collect();
        assert!(
            got.iter()
                .any(|p| p.starts_with("insert after true Cortado × 1")),
            "{got:?}"
        );
    }

    #[test]
    fn an_item_that_leaves_is_removed() {
        let s = lines_server();
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        s.serve_document("a");
        s.command(CLEAR, "a", &[], false).expect("runs");
        let got: Vec<String> = patch_sets(&s, "a")[0].patches.iter().map(said).collect();
        assert_eq!(
            got.iter().filter(|p| p.starts_with("remove")).count(),
            2,
            "{got:?}"
        );
        assert!(got[0].starts_with("text \"0\""), "{got:?}");
    }

    #[test]
    fn each_change_is_derived_from_the_last_one_sent() {
        let s = lines_server();
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        let sets = patch_sets(&s, "a");
        assert_eq!(sets.len(), 2, "{sets:?}");
        // The second set inserts the cortado after the espresso the first
        // one inserted, and inserts nothing else.
        let got: Vec<String> = sets[1].patches.iter().map(said).collect();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].starts_with("text \"2\""), "{got:?}");
        assert!(
            got[1].starts_with("insert after true Cortado × 1"),
            "{got:?}"
        );
    }

    /// The store, with a line its cart decides: a block a query decides
    /// (ADR-0146). Without `add_to_cart`'s speculation, which would not
    /// reach the block, and is refused beside one (ADR-0170).
    fn with_block(app: &str) -> String {
        let speculation = "    optimistic    Cart(current_session()) as cart => Carts.with_line(cart, item, quantity)\n";
        assert_eq!(
            app.matches(speculation).count(),
            1,
            "the speculation's anchor"
        );
        let out = app.replace(speculation, "").replacen(
            "                <p id=\"cart-count\">{cart.line_count}</p>",
            "                <p id=\"cart-count\">{cart.line_count}</p>\n                {#if cart.lines}<p id=\"ready\">Ready when you are.</p>{/if}",
            1,
        );
        assert!(out.contains("{#if cart.lines}"));
        out
    }

    #[test]
    fn a_block_a_query_decides_is_rendered() {
        let s = lines_server_with(with_block);
        let (empty, _) = s.serve_document("a");
        assert!(!visible(&empty).contains("Ready"), "{empty}");
        s.command(ADD, "b", &add("espresso", 1), false)
            .expect("runs");
        let (held, _) = s.serve_document("b");
        assert!(visible(&held).contains("Ready when you are."), "{held}");
    }

    #[test]
    fn a_block_whose_rendering_changed_is_rendered_again() {
        let s = lines_server_with(with_block);
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(CLEAR, "a", &[], false).expect("runs");
        let sets: Vec<Vec<String>> = patch_sets(&s, "a")
            .iter()
            .map(|p| p.patches.iter().map(said).collect())
            .collect();
        assert_eq!(sets.len(), 3, "{sets:?}");
        // Shown, with the first line: the count, and the block.
        assert_eq!(sets[0].len(), 2, "{sets:?}");
        assert!(
            sets[0][1].starts_with("range \"Ready when you are.\""),
            "{sets:?}"
        );
        // Still shown: the count alone.
        assert_eq!(sets[1].len(), 1, "{sets:?}");
        assert!(sets[1][0].starts_with("text \"2\""), "{sets:?}");
        // Gone, with the last line.
        assert_eq!(sets[2].len(), 2, "{sets:?}");
        assert!(sets[2][1].starts_with("range \"\""), "{sets:?}");
    }

    /// The store, with two blocks its cart decides, one that lists the
    /// cart's lines and one that shows text, and one its menu decides. With
    /// `add_to_cart`'s speculation, which renders the cart's again in the
    /// browser (ADR-0172), or without it. The menu's no speculation renders,
    /// so the host keeps what the page's blocks were rendered from either
    /// way.
    fn with_listed_blocks(app: &str, speculated: bool) -> String {
        let speculation = "    optimistic    Cart(current_session()) as cart => Carts.with_line(cart, item, quantity)\n";
        assert_eq!(
            app.matches(speculation).count(),
            1,
            "the speculation's anchor"
        );
        let app = if speculated {
            app.to_string()
        } else {
            app.replace(speculation, "")
        };
        let out = app.replacen(
            "                <p id=\"cart-count\">{cart.line_count}</p>",
            "                <p id=\"cart-count\">{cart.line_count}</p>\n                \
             {#if cart.lines}<ul id=\"ready\">{#each cart.lines as l (l.item_id)}<li>{l.item_id}</li>{/each}</ul>{/if}\n                \
             {#if cart.lines}<p id=\"fees\">Fees at checkout.</p>{/if}\n                \
             {#if menu}<p id=\"menu-shown\">The menu.</p>{/if}",
            1,
        );
        assert!(out.contains("{#if cart.lines}"));
        out
    }

    /// The patches each change sent document `doc`, as `said` writes them.
    fn told(s: &Server, doc: &str) -> Vec<Vec<String>> {
        patch_sets(s, doc)
            .iter()
            .map(|p| p.patches.iter().map(said).collect())
            .collect()
    }

    /// **A block that shows the arm it showed is told part by part**
    /// (ADR-0320): the line a change adds is inserted into its list where it
    /// is, and the block is not rendered again. Until 2026-10-12 it was,
    /// whole, at every change inside it (ADR-0146).
    #[test]
    fn a_block_that_shows_the_arm_it_showed_is_told_part_by_part() {
        let s = lines_server_with(|app| with_listed_blocks(app, false));
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        let sets = told(&s, "a");
        assert_eq!(sets.len(), 2, "{sets:?}");
        // Shown, with the first line: each block, whole.
        assert!(
            sets[0].iter().any(|p| p.starts_with("range \"espresso\"")),
            "{sets:?}"
        );
        assert!(
            sets[0]
                .iter()
                .any(|p| p.starts_with("range \"Fees at checkout.\"")),
            "{sets:?}"
        );
        // Still shown: the line inserted where it is. The other block renders
        // as it did, and is not sent.
        assert!(
            sets[1]
                .iter()
                .any(|p| p.starts_with("insert after true cortado")),
            "{sets:?}"
        );
        assert!(!sets[1].iter().any(|p| p.starts_with("range")), "{sets:?}");
    }

    /// **A block a speculation renders is rendered again whole** (ADR-0320,
    /// ADR-0172): the browser may show it as it speculated, and only the
    /// whole block finds it as it was derived.
    #[test]
    fn a_block_a_speculation_renders_is_rendered_again_whole() {
        let s = lines_server_with(|app| with_listed_blocks(app, true));
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        let sets = told(&s, "a");
        assert_eq!(sets.len(), 2, "{sets:?}");
        assert!(
            sets[1]
                .iter()
                .any(|p| p.starts_with("range \"espressocortado\"")),
            "{sets:?}"
        );
        // One that renders as it did is not sent (ADR-0146).
        assert!(
            !sets[1].iter().any(|p| p.contains("Fees at checkout.")),
            "{sets:?}"
        );
    }

    /// The store, with a block its cart decides that shows the cart's count
    /// through a member, `{cart.line_count}` (ADR-0323). With `add_to_cart`'s
    /// speculation, which renders the block again in the browser (ADR-0172),
    /// or without it.
    fn with_count_in_a_block(app: &str, speculated: bool) -> String {
        let speculation = "    optimistic    Cart(current_session()) as cart => Carts.with_line(cart, item, quantity)\n";
        let count = "                <p id=\"cart-count\">{cart.line_count}</p>";
        assert_eq!(
            app.matches(speculation).count(),
            1,
            "the speculation's anchor"
        );
        assert_eq!(app.matches(count).count(), 1, "the count's anchor");
        let app = if speculated {
            app.to_string()
        } else {
            app.replace(speculation, "")
        };
        app.replace(
            count,
            &format!(
                "{count}\n                {{#if cart.lines}}<p id=\"inner\">{{cart.line_count}} in \
                 the cart</p>{{/if}}"
            ),
        )
    }

    /// **A member read inside a block is computed once a change, and told**
    /// (ADR-0323): the count inside `{#if cart.lines}` is shown with the
    /// block at the first line, set where it is at the second, as the count
    /// at the top is, and gone with the block when the cart is cleared.
    /// Until 2026-10-12 the page did not build: "which a host would call per
    /// instance".
    #[test]
    fn a_member_read_inside_a_block_is_computed_and_told_where_it_is() {
        let s = lines_server_with(|app| with_count_in_a_block(app, false));
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        s.command(CLEAR, "a", &[], false).expect("runs");
        let sets = told(&s, "a");
        assert_eq!(sets.len(), 3, "{sets:?}");
        assert!(
            sets[0]
                .iter()
                .any(|p| p.starts_with("range \"1 in the cart\"")),
            "{sets:?}"
        );
        // The block still shown: its count set where it is, beside the top's.
        assert_eq!(
            sets[1]
                .iter()
                .filter(|p| p.starts_with("text \"2\""))
                .count(),
            2,
            "{sets:?}"
        );
        assert!(!sets[1].iter().any(|p| p.starts_with("range")), "{sets:?}");
        assert!(
            sets[2].iter().any(|p| p.starts_with("range \"\"")),
            "{sets:?}"
        );
    }

    /// **And in a block a speculation renders** (ADR-0172): rendered again
    /// whole, its count the host's.
    #[test]
    fn a_member_read_inside_a_speculated_block_is_rendered_with_it() {
        let s = lines_server_with(|app| with_count_in_a_block(app, true));
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("cortado", 1), false)
            .expect("runs");
        let sets = told(&s, "a");
        assert_eq!(sets.len(), 2, "{sets:?}");
        assert!(
            sets[1]
                .iter()
                .any(|p| p.starts_with("range \"2 in the cart\"")),
            "{sets:?}"
        );
    }

    /// The store, with a message its cart's lines decide by an attribute:
    /// `hidden` while the cart holds a line (ADR-0171). Without
    /// `add_to_cart`'s speculation, which would not reach it (ADR-0170).
    fn with_empty_message(app: &str) -> String {
        let speculation = "    optimistic    Cart(current_session()) as cart => Carts.with_line(cart, item, quantity)\n";
        let count = "                <p id=\"cart-count\">{cart.line_count}</p>";
        assert_eq!(
            app.matches(speculation).count(),
            1,
            "the speculation's anchor"
        );
        assert_eq!(app.matches(count).count(), 1, "the count's anchor");
        app.replace(speculation, "").replace(
            count,
            &format!(
                "{count}\n                <p id=\"empty\" hidden={{cart.lines}}>Your cart is \
                 empty.</p>"
            ),
        )
    }

    /// **An attribute at the top of the page that reads a query's value is
    /// set again when it changes** (ADR-0171): the message is hidden once
    /// the cart holds a line, and shown when it is cleared. Until 2026-10-03
    /// it kept the value its document was rendered with.
    #[test]
    fn an_attribute_that_reads_a_query_is_set_again_when_it_changes() {
        let s = lines_server_with(with_empty_message);
        let (page, _) = s.serve_document("a");
        assert!(
            !page.contains("hidden"),
            "shown while the cart is empty: {page}"
        );
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(CLEAR, "a", &[], false).expect("runs");
        let sets: Vec<Vec<String>> = patch_sets(&s, "a")
            .iter()
            .map(|p| p.patches.iter().map(said).collect())
            .collect();
        assert_eq!(sets.len(), 3, "{sets:?}");
        let hidden = |set: &[String]| {
            set.iter()
                .find(|p| p.contains("\"hidden\""))
                .cloned()
                .unwrap_or_default()
        };
        // Hidden with the first line, at the top of the page ...
        assert!(
            hidden(&sets[0]).starts_with("SetAttribute { name: \"hidden\", value: \"\" }")
                && !hidden(&sets[0]).ends_with("in an instance"),
            "{sets:?}"
        );
        // ... not set again while it stays hidden ...
        assert_eq!(hidden(&sets[1]), "", "{sets:?}");
        // ... and shown again with the last line gone.
        assert!(
            hidden(&sets[2]).starts_with("RemoveAttribute { name: \"hidden\" }"),
            "{sets:?}"
        );
        let (page, _) = s.serve_document("a");
        assert!(!page.contains("hidden"), "{page}");
    }

    /// The store, listing its cart's lines: a loop over a list inside a
    /// query's value, each row reading a member of its line, `count`
    /// (ADR-0170). Without `add_to_cart`'s speculation, which would not reach
    /// the list, and is refused beside one.
    fn with_cart_lines(app: &str) -> String {
        let speculation = "    optimistic    Cart(current_session()) as cart => Carts.with_line(cart, item, quantity)\n";
        let count = "                <p id=\"cart-count\">{cart.line_count}</p>";
        assert_eq!(
            app.matches(speculation).count(),
            1,
            "the speculation's anchor"
        );
        assert_eq!(app.matches(count).count(), 1, "the count's anchor");
        app.replace(speculation, "").replace(
            count,
            &format!(
                "{count}\n                <ul id=\"lines\">{{#each cart.lines as line \
                 (line.item_id)}}<li>× {{line.quantity.count}}</li>{{/each}}</ul>"
            ),
        )
    }

    /// **A list inside a query's value is rendered, and patched where it
    /// is** (ADR-0170): `cart.lines`, each row's count computed for it.
    /// Until 2026-10-03 the server asked for the binding, `cart`, as the
    /// list, and failed at the cart's first change.
    #[test]
    fn a_list_inside_a_querys_value_is_rendered_and_patched_where_it_is() {
        let s = lines_server_with(with_cart_lines);
        s.serve_document("a");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        let sets: Vec<Vec<String>> = patch_sets(&s, "a")
            .iter()
            .map(|p| p.patches.iter().map(said).collect())
            .collect();
        assert_eq!(sets.len(), 2, "{sets:?}");
        // The count, and the line inserted at the head of the list ...
        assert_eq!(sets[0].len(), 2, "{sets:?}");
        assert!(sets[0][0].starts_with("text \"1\""), "{sets:?}");
        assert!(
            sets[0][1].starts_with("insert before false × 1"),
            "{sets:?}"
        );
        // ... then its count set where it is, its nodes kept.
        assert_eq!(sets[1].len(), 2, "{sets:?}");
        assert!(sets[1][0].starts_with("text \"2\""), "{sets:?}");
        assert!(
            sets[1][1].starts_with("text \"2\"") && sets[1][1].ends_with("in an instance"),
            "{sets:?}"
        );
        // And a document served now shows it.
        let (page, _) = s.serve_document("a");
        assert!(visible(&page).contains("× 2"), "{page}");
    }

    #[test]
    fn a_case_is_a_case() {
        // As a `{#match}` arm names it (ADR-0146): `Some`, `None`, `Ok` and
        // `Err` as Pleris writes them, and a declared case by its WIT name.
        assert_eq!(
            val_to_value(&Val::Enum("preparing".into())),
            Value::Variant {
                case: "preparing".into(),
                payload: None
            }
        );
        assert_eq!(
            val_to_value(&Val::Option(Some(Box::new(Val::S64(3))))),
            Value::Variant {
                case: "some".into(),
                payload: Some(Box::new(Value::Int(3)))
            }
        );
        assert_eq!(
            val_to_value(&Val::Result(Err(Some(Box::new(Val::String("x".into())))))),
            Value::Variant {
                case: "err".into(),
                payload: Some(Box::new(Value::Text("x".into())))
            }
        );
    }

    #[test]
    fn the_menu_is_no_session_s_list() {
        // A shared list is one fragment for every reader, patched once for
        // all of them (E7-P); a session's lists are the private ones.
        let s = lines_server();
        assert_eq!(s.own_lists(s.store_page()), ["lines".to_string()]);
    }

    #[test]
    fn a_session_s_change_reaches_its_own_document_alone() {
        let s = lines_server();
        s.serve_document("a");
        s.serve_document("b");
        s.command(ADD, "a", &add("espresso", 1), false)
            .expect("runs");
        assert_eq!(patch_sets(&s, "a").len(), 1);
        assert!(
            patch_sets(&s, "b").is_empty(),
            "b's document changed nothing"
        );
    }

    #[test]
    fn a_list_moves_what_moved_and_sets_what_changed_where_it_is() {
        let s = lines_server();
        let template = s.store_template().clone();
        let schema = TemplateSchemaId(template.schema.clone());
        let env = Env::new().in_domain(s.domain("a"));
        let line = |id: &str, n: i64| {
            Value::Record(
                [
                    ("item_id".to_string(), Value::Text(id.into())),
                    ("name".to_string(), Value::Text(id.to_uppercase())),
                    ("quantity".to_string(), Value::Int(n)),
                ]
                .into(),
            )
        };
        // A move keeps the nodes: no removal, no insertion.
        let moved = list_patches(
            &template,
            &schema,
            "lines",
            &[line("a", 1), line("b", 1), line("c", 1)],
            &[line("c", 1), line("a", 1), line("b", 1)],
            &env,
            &s.templates,
        )
        .expect("derived");
        let got: Vec<String> = moved.iter().map(said).collect();
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].starts_with("move after false"), "{got:?}");
        // A menu item's markup holds a button as well as text. A rename
        // changes neither the button nor what it captures, so the name is set
        // where it is, and the item's nodes stay (ADR-0168). Until 2026-10-03
        // an item that held more than text was rendered again.
        let item = |id: &str, name: &str| {
            Value::Record(
                [
                    ("id".to_string(), Value::Text(id.into())),
                    ("name".to_string(), Value::Text(name.into())),
                ]
                .into(),
            )
        };
        let renamed = list_patches(
            &template,
            &schema,
            "menu",
            &[item("a", "A"), item("b", "B")],
            &[item("a", "A"), item("b", "Bee")],
            &env,
            &s.templates,
        )
        .expect("derived");
        let got: Vec<String> = renamed.iter().map(said).collect();
        assert_eq!(got, ["text \"Bee\" @2 in an instance"]);
    }

    /// A node that grants nothing at all.
    fn barren() -> Topology {
        Topology {
            nodes: vec![Node {
                name: "barren".to_string(),
                world: "origin".to_string(),
                grants: Default::default(),
            }],
        }
    }

    fn server(topology: Topology) -> Server {
        Server::on(std::path::PathBuf::from("."), Vec::new(), topology)
    }

    const ADD: &str = "store.page.add_to_cart";
    const CLEAR: &str = "store.page.clear_cart";
    const INCREASE: &str = "store.page.increase_in_cart";
    const DECREASE: &str = "store.page.decrease_in_cart";
    const REMOVE: &str = "store.page.remove_from_cart";

    /// `add_to_cart`'s arguments, as the component takes them.
    fn add(item: &str, quantity: i64) -> [Val; 2] {
        [Val::String(item.into()), Val::S64(quantity)]
    }

    /// `add_to_cart`'s arguments on the canonical store, which takes the
    /// item as its menu row shows it (ADR-0172).
    fn add_shown(item: &str, quantity: i64) -> [Val; 2] {
        let shown = shown(item);
        let text = |v: &serde_json::Value| Val::String(v.as_str().unwrap_or_default().into());
        [
            // In the order `MenuItem` declares its fields, as a component's
            // record takes them.
            Val::Record(vec![
                ("id".into(), Val::String(item.into())),
                // Its store (ADR-0181).
                ("store-id".into(), text(&shown["store_id"])),
                ("name".into(), text(&shown["name"])),
                ("description".into(), text(&shown["description"])),
                ("price".into(), Val::S64(item_price(item))),
                // As the page showed it: one it could order (ADR-0178).
                ("available".into(), Val::Bool(true)),
                // Its category (ADR-0181).
                (
                    "category".into(),
                    Val::Record(vec![
                        ("id".into(), text(&shown["category"]["id"])),
                        ("name".into(), text(&shown["category"]["name"])),
                    ]),
                ),
            ]),
            Val::S64(quantity),
        ]
    }

    /// With the compiler's template IR, for a test whose second command
    /// regenerates the fragment the first one materialized.
    fn rendering_server() -> Server {
        let templates: Vec<Template> =
            serde_json::from_str(include_str!("../../store-ir.json")).expect("the template IR");
        Server::on(std::path::PathBuf::from("."), templates, dev_topology())
    }

    /// **The command path asks, and a refusal is a refusal.**
    ///
    /// E8's last gate item asks for the dev server's command path to go through
    /// the host. This is the half that is real: `add_to_cart` requires
    /// `database.write<Carts>` by its CONTRACT, and a node without it cannot
    /// run the command — the state does not move and no event is queued.
    ///
    /// The body is the compiled component (E10-I); the refusal comes first.
    #[test]
    fn a_command_is_refused_on_a_node_that_does_not_grant_its_capability() {
        let s = server(barren());
        let err = s
            .command(ADD, "session-1", &add("espresso", 1), false)
            .expect_err("a barren node cannot host a write");
        assert!(
            err.contains("add_to_cart") && err.contains("barren"),
            "and it says which command and which node: {err}"
        );
        assert_eq!(s.cart_value("session-1"), 0, "the state did not move");
    }

    /// The discriminating half. Without it the test above passes for a command
    /// path that refuses everything, which is not a capability system.
    #[test]
    fn the_same_command_runs_where_the_capability_exists() {
        let s = server(dev_topology());
        s.command(ADD, "session-1", &add_shown("espresso", 1), false)
            .expect("the dev origin grants the write");
        assert_eq!(s.cart_value("session-1"), 1);
    }

    /// **The command's arguments reach the data layer through the compiled
    /// body**, and its result is the component's. `cortado` and `2` go in as
    /// the command's arguments; the session is the host's; the lines the
    /// data layer holds afterwards are what `Carts.add` was called with.
    #[test]
    fn the_compiled_command_carries_its_arguments_to_the_data_layer() {
        let s = rendering_server();
        s.command(ADD, "session-9", &add_shown("cortado", 2), false)
            .expect("runs");
        s.command(ADD, "session-9", &add_shown("espresso", 1), false)
            .expect("runs");
        s.command(ADD, "session-9", &add_shown("cortado", 1), false)
            .expect("runs");
        assert_eq!(
            s.cart_lines("session-9"),
            [("cortado".to_string(), 3), ("espresso".to_string(), 1)]
        );
        // Each recorded with its item's name and price when it was made
        // (ADR-0172), as the layer answers it.
        assert_eq!(
            s.cart_priced("session-9"),
            [("Cortado".to_string(), 425), ("Espresso".to_string(), 350)]
        );
        assert_eq!(s.cart_value("session-9"), 4);
        assert_eq!(
            s.cart_value("another-session"),
            0,
            "one session's cart only"
        );
    }

    /// A session's lines as the data layer holds them: each item and how
    /// many.
    fn quantities(s: &Server, session: &str) -> Vec<(String, i64)> {
        s.cart_lines(session)
    }

    /// **A line's −, + and Remove reach the data layer through their
    /// compiled bodies** (ADR-0172): one more; one fewer, and at one the
    /// line goes; the line gone. One fewer of a line that is no longer there
    /// changes nothing: another page took it away first.
    #[test]
    fn a_lines_steps_reach_the_data_layer() {
        let s = rendering_server();
        let item = |id: &str| [Val::String(id.into())];
        let at = |s: &Server| quantities(s, "session-9");
        let owned = |want: &[(&str, i64)]| {
            want.iter()
                .map(|(i, q)| (i.to_string(), *q))
                .collect::<Vec<_>>()
        };
        s.command(ADD, "session-9", &add_shown("cortado", 2), false)
            .expect("runs");
        s.command(ADD, "session-9", &add_shown("espresso", 1), false)
            .expect("runs");
        s.command(INCREASE, "session-9", &item("cortado"), false)
            .expect("runs");
        assert_eq!(at(&s), owned(&[("cortado", 3), ("espresso", 1)]));
        s.command(DECREASE, "session-9", &item("cortado"), false)
            .expect("runs");
        assert_eq!(at(&s), owned(&[("cortado", 2), ("espresso", 1)]));
        s.command(DECREASE, "session-9", &item("espresso"), false)
            .expect("runs");
        assert_eq!(at(&s), owned(&[("cortado", 2)]), "at one, the line goes");
        s.command(DECREASE, "session-9", &item("espresso"), false)
            .expect("a line no longer there is not an error");
        assert_eq!(at(&s), owned(&[("cortado", 2)]));
        s.command(REMOVE, "session-9", &item("cortado"), false)
            .expect("runs");
        assert_eq!(at(&s), owned(&[]));
        // One more of an item that sold out since it was added is refused
        // by name, as an add is, and nothing is written.
        s.command(ADD, "session-9", &add_shown("cortado", 1), false)
            .expect("runs");
        s.stock_untold("cortado", false);
        let refused = s
            .command(INCREASE, "session-9", &item("cortado"), false)
            .expect_err("sold out");
        assert!(refused.contains("item-unavailable"), "{refused}");
        assert_eq!(at(&s), owned(&[("cortado", 1)]));
    }

    /// **A session's changes come one at a time, each through its frames**
    /// (ADR-0172). A command commits and its change is derived and queued
    /// before the session's next change begins, and a document is served
    /// after the change in progress. Until 2026-10-03 two drains of one
    /// session could derive two patch sets against one document, and a
    /// line the page showed was lost.
    #[test]
    fn a_sessions_changes_wait_for_the_one_in_progress() {
        let s = rendering_server();
        s.serve_document("session-9");
        let lock = s.one_at_a_time("session-9");
        let wait = std::time::Duration::from_millis(500);
        std::thread::scope(|scope| {
            let held = lock.lock().unwrap();
            let command =
                scope.spawn(|| s.command(ADD, "session-9", &add_shown("cortado", 1), false));
            let served = scope.spawn(|| s.serve_document("session-9"));
            std::thread::sleep(wait);
            assert!(!command.is_finished(), "the command waits");
            assert!(!served.is_finished(), "and so does the document");
            assert!(quantities(&s, "session-9").is_empty(), "nothing committed");
            // Another session's change does not wait for this one.
            s.command(ADD, "session-10", &add_shown("cortado", 1), false)
                .expect("runs");
            drop(held);
            command.join().unwrap().expect("runs");
            served.join().unwrap();
        });
        assert_eq!(quantities(&s, "session-9"), [("cortado".to_string(), 1)]);
    }

    /// **A one-shot database error** (charter §15.5, ADR-0174): the
    /// session's next cart write fails as a database that is down does, and
    /// nothing is committed; the one after succeeds, and another session's
    /// is not touched.
    #[test]
    fn a_one_shot_write_error_fails_the_next_write_once() {
        let s = rendering_server();
        s.store
            .faults()
            .cart_faults
            .lock()
            .unwrap()
            .entry("session-9".into())
            .or_default()
            .fail_write = true;
        s.command(ADD, "session-10", &add_shown("espresso", 1), false)
            .expect("another session's write");
        let failed = s
            .command(ADD, "session-9", &add_shown("cortado", 1), false)
            .expect_err("the database is down");
        assert!(failed.contains("the database is unavailable"), "{failed}");
        assert_eq!(quantities(&s, "session-9"), []);
        s.command(ADD, "session-9", &add_shown("cortado", 1), false)
            .expect("the next write succeeds");
        assert_eq!(quantities(&s, "session-9"), [("cortado".to_string(), 1)]);
        assert_eq!(quantities(&s, "session-10"), [("espresso".to_string(), 1)]);
    }

    /// And its next cart read: the session's page cannot be shown, once
    /// (ADR-0174), as a page whose query failed cannot be (ADR-0147).
    #[test]
    fn a_one_shot_read_error_fails_the_next_read_once() {
        let s = rendering_server();
        // A page of the session's, served: its entry is made.
        s.serve_document_with_entries("session-9").expect("served");
        s.store
            .faults()
            .cart_faults
            .lock()
            .unwrap()
            .entry("session-9".into())
            .or_default()
            .fail_read = true;
        s.render_store("session-10");
        let failed = s
            .serve_document_with_entries("session-9")
            .expect_err("the database is down");
        assert!(failed.contains("the database is unavailable"), "{failed}");
        s.serve_document_with_entries("session-9")
            .expect("the next read succeeds");
    }

    /// **A delay slows what it names** (charter §15.5, ADR-0174): a cart
    /// delay its own session's reads of the cart, and no other session's;
    /// the store's delay every reader's store, when the store is read. What
    /// a delay slows is what takes it: the delays taken are counted, and a
    /// render that waited no delay is one that took none, where "took less
    /// than a second" was a speed the machine set.
    #[test]
    fn a_delay_slows_what_it_names_and_nothing_else() {
        let s = rendering_server();
        let second = std::time::Duration::from_millis(1000);
        let timed = |session: &str| {
            let started = std::time::Instant::now();
            s.render_store(session);
            started.elapsed()
        };
        let taken = || {
            s.store
                .faults()
                .delays_taken
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        // The shared queries, read and kept.
        timed("warm");
        s.store
            .faults()
            .cart_faults
            .lock()
            .unwrap()
            .entry("slow".into())
            .or_default()
            .delay_ms = 1000;
        let before = taken();
        assert!(timed("slow") >= second, "its own session's cart");
        assert!(taken() > before, "its own session's cart took the delay");
        let before = taken();
        timed("quick");
        assert_eq!(taken(), before, "not another's");

        s.store
            .faults()
            .store_delay_ms
            .store(1000, std::sync::atomic::Ordering::SeqCst);
        timed("quick");
        assert_eq!(
            taken(),
            before,
            "a store kept within its freshness is not read"
        );
        s.query_clock.0.advance(30_001);
        assert!(timed("quick") >= second, "read again, and slow");
        assert!(taken() > before, "read again, and took the delay");
        s.store
            .faults()
            .store_delay_ms
            .store(0, std::sync::atomic::Ordering::SeqCst);
        s.query_clock.0.advance(30_001);
        let before = taken();
        timed("quick");
        assert_eq!(taken(), before, "cleared");
    }

    /// A request through [`handle`], as a page sends one: what came back,
    /// and nothing when the connection was closed with no answer.
    fn posted(s: &Server, path: &str, session: &str, interaction: &str, body: &str) -> String {
        posted_with(
            s,
            path,
            session,
            &format!("pw-interaction: {interaction}\r\n"),
            body,
        )
    }

    /// [`posted`], naming the command its page sent before it (ADR-0305).
    fn posted_after(
        s: &Server,
        path: &str,
        session: &str,
        interaction: &str,
        after: &str,
        body: &str,
    ) -> String {
        let headers = format!("pw-interaction: {interaction}\r\npw-after: {after}\r\n");
        posted_with(s, path, session, &headers, body)
    }

    /// [`posted`], given up on as a page gives up on an answer that does not
    /// come (ADR-0381): what came back within `wait` of the last byte, and
    /// whether the request's thread ended within a second of the connection
    /// being closed.
    fn given_up(
        s: &Server,
        path: &str,
        session: &str,
        interaction: &str,
        body: &str,
        wait: std::time::Duration,
    ) -> (String, bool) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let at = listener.local_addr().expect("address");
        let (tx, ended) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let (stream, _) = listener.accept().expect("accept");
                handle(s, stream);
                let _ = tx.send(());
            });
            let mut client = TcpStream::connect(at).expect("connect");
            client
                .write_all(
                    format!(
                        "POST {path} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
                         pw-interaction: {interaction}\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("request");
            client.set_read_timeout(Some(wait)).expect("a read timeout");
            let mut answer = Vec::new();
            let mut chunk = [0u8; 4096];
            // Until it is closed, or nothing comes within `wait`.
            while let Ok(n) = client.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                answer.extend_from_slice(&chunk[..n]);
            }
            drop(client);
            let thread_ended = ended
                .recv_timeout(std::time::Duration::from_secs(1))
                .is_ok();
            (String::from_utf8_lossy(&answer).into_owned(), thread_ended)
        })
    }

    /// [`posted`], with `headers`, each line ending in CRLF.
    fn posted_with(s: &Server, path: &str, session: &str, headers: &str, body: &str) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let at = listener.local_addr().expect("address");
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let (stream, _) = listener.accept().expect("accept");
                handle(s, stream);
            });
            let mut client = TcpStream::connect(at).expect("connect");
            client
                .write_all(
                    format!(
                        "POST {path} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
                         {headers}content-type: application/json\r\n\
                         content-length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("request");
            let mut answer = String::new();
            let _ = client.read_to_string(&mut answer);
            answer
        })
    }

    /// **A one-shot network error** (charter §15.5, ADR-0175): the session's
    /// next command connection is closed with no answer, before the command
    /// runs or after it has committed. Sent again with the same interaction,
    /// the request is answered with what happened, once.
    #[test]
    fn a_dropped_command_connection_is_answered_by_nothing() {
        let s = rendering_server();
        let body = serde_json::json!([shown("cortado"), 1]).to_string();
        let path = format!("/command/{ADD}");
        let drop = |at: DropAt| {
            s.connection_faults
                .lock()
                .unwrap()
                .entry("session-9".into())
                .or_default()
                .drop_command = Some(at);
        };
        drop(DropAt::Before);
        assert_eq!(
            posted(&s, &path, "session-9", "press-1", &body),
            "",
            "no answer"
        );
        assert_eq!(quantities(&s, "session-9"), [], "and nothing ran");
        let again = posted(&s, &path, "session-9", "press-1", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(quantities(&s, "session-9"), [("cortado".to_string(), 1)]);

        drop(DropAt::After);
        assert_eq!(
            posted(&s, &path, "session-9", "press-2", &body),
            "",
            "no answer"
        );
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 2)],
            "but it committed"
        );
        let again = posted(&s, &path, "session-9", "press-2", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 2)],
            "once"
        );
    }

    /// **A connection that neither answers nor fails** (ADR-0381): the
    /// session's next command connection is held open with no answer, before
    /// the command runs or after it has committed, or with its answer begun
    /// and never finished, until its page gives up on it. Sent again with
    /// the same interaction, the request is answered with what happened,
    /// once, and the hold is taken: the command after it is answered.
    #[test]
    fn a_held_command_connection_is_answered_by_nothing_until_it_is_given_up() {
        let s = rendering_server();
        let body = serde_json::json!([shown("cortado"), 1]).to_string();
        let path = format!("/command/{ADD}");
        let hold = |at: HoldAt| {
            s.connection_faults
                .lock()
                .unwrap()
                .entry("session-9".into())
                .or_default()
                .hold_command = Some(at);
        };
        // Long enough for the command to run first on a loaded machine: it is
        // also how long the client waits for bytes that do not come.
        let wait = std::time::Duration::from_millis(1000);

        hold(HoldAt::Before);
        let (answer, ended) = given_up(&s, &path, "session-9", "press-1", &body, wait);
        assert_eq!(answer, "", "no answer");
        assert!(ended, "the hold ends with its connection");
        assert_eq!(quantities(&s, "session-9"), [], "and nothing ran");
        let again = posted(&s, &path, "session-9", "press-1", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(quantities(&s, "session-9"), [("cortado".to_string(), 1)]);

        hold(HoldAt::After);
        let (answer, ended) = given_up(&s, &path, "session-9", "press-2", &body, wait);
        assert_eq!(answer, "", "no answer");
        assert!(ended, "the hold ends with its connection");
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 2)],
            "but it committed"
        );
        let again = posted(&s, &path, "session-9", "press-2", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 2)],
            "once"
        );

        hold(HoldAt::Body);
        let (answer, ended) = given_up(&s, &path, "session-9", "press-3", &body, wait);
        assert!(ended, "the hold ends with its connection");
        // Its head, whose length promises a body, and less than that body.
        let (head, part) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
        assert!(head.starts_with("HTTP/1.1 202"), "{answer}");
        let promised = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length: "))
            .and_then(|n| n.trim().parse::<usize>().ok())
            .unwrap_or(0);
        assert!(
            !part.is_empty() && part.len() < promised,
            "{} of {promised} bytes: {part}",
            part.len()
        );
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 3)],
            "it committed"
        );
        let again = posted(&s, &path, "session-9", "press-3", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(
            quantities(&s, "session-9"),
            [("cortado".to_string(), 3)],
            "once"
        );

        // Held once: the next command is answered.
        let next = posted(&s, &path, "session-9", "press-4", &body);
        assert!(next.contains("\"committed\":true"), "{next}");
        assert_eq!(quantities(&s, "session-9"), [("cortado".to_string(), 4)]);
    }

    /// **A page that shows what the clock decides is told when the clock
    /// passes it** (ADR-0304): the home page asks whether today's 21:00, in
    /// UTC, has passed. Read at 20:59:30 it says "On sale", held until 21:00;
    /// at 20:59:59 nothing is told, and past 21:00 the page is sent "Sale
    /// over", once.
    #[test]
    fn a_page_is_told_when_the_instant_it_compared_passes() {
        let s = on_sale_until_nine();
        // 20:59:30 UTC, on a day ahead of the host's.
        let now = s.now();
        let day = now.div_euclid(86_400_000) + 1;
        let nine = day * 86_400_000 + 21 * 3_600_000;
        s.query_clock
            .0
            .advance(u64::try_from(nine - 30_000 - now).unwrap());
        let (html, _, _, _) = s
            .serve_document_settled("a", "store.page.HomePage~ui", &Params::new(), &[])
            .expect("served");
        assert!(visible(&html).contains("On sale"), "{html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        assert_eq!(s.timers.at(&doc), Some(nine), "held until 21:00");
        let before = sets_of(&s, &doc).len();
        s.query_clock.0.advance(29_000);
        assert_eq!(s.tell_due().0, 0, "20:59:59: nothing is due");
        assert_eq!(sets_of(&s, &doc).len(), before);
        s.query_clock.0.advance(2_000);
        assert_eq!(s.tell_due().0, 1, "21:00:01: the page is due");
        let sets = sets_of(&s, &doc);
        assert_eq!(sets.len(), before + 1, "told once");
        assert!(
            written(sets.last().expect("a set")).contains("Sale over"),
            "{:?}",
            sets.last()
        );
        // Read again past 21:00, it holds until the day ends: today's date
        // was compared too, and tomorrow's sale is on again.
        assert_eq!(
            s.timers.at(&doc),
            Some((day + 1) * 86_400_000),
            "held until the next day begins"
        );
    }

    /// The store's home page with a sale on until 21:00 UTC each day
    /// (ADR-0304): a shared query, kept ten minutes, so only the instant it
    /// compared makes it be read again sooner.
    fn on_sale_until_nine() -> Served {
        served_from_patches_in(
            "examples",
            |app| {
                // The reader's cart is the user's since track `store-accounts`;
                // the stores, each with whether it is open, since track
                // `store-hours`.
                let page = "    let stores = query StoresOpen()\n    let cart = query Cart(current_user())\n";
                let shown = "            <h1>Stores</h1>\n";
                assert!(app.contains(page) && app.contains(shown), "the home page");
                app.replacen("import List\n", "import List\nimport clock\n", 1)
                    .replacen(
                        "public query StoreList() -> List<Store>\n",
                        "public query Sale() -> String\n    freshness   10.minutes\n    consistency snapshot\n    cache       shared\n    concurrency one_per_key\n    timeout     2.seconds\n{\n    let utc = clock.zone(\"UTC\")\n    match clock.time(21, 0) {\n        Some(nine) => if clock.passed(clock.at(clock.today_in(utc), nine, utc)) { \"Sale over\" } else { \"On sale\" },\n        None => \"No sale\",\n    }\n}\n\npublic query StoreList() -> List<Store>\n",
                        1,
                    )
                    .replacen(page, &format!("{page}    let sale = query Sale()\n"), 1)
                    .replacen(shown, &format!("{shown}            <p id=\"sale\">{{sale}}</p>\n"), 1)
            },
            &[],
        )
    }

    /// **A value kept past the instant it compared is read again**
    /// (ADR-0304): the sale's entry is kept ten minutes, and a page served a
    /// second after 21:00 is not served the value from 20:59:30. A second
    /// session's page, served from the kept entry, is held to its instant
    /// too.
    #[test]
    fn a_kept_value_past_its_instant_is_read_again() {
        let s = on_sale_until_nine();
        let now = s.now();
        let day = now.div_euclid(86_400_000) + 1;
        let nine = day * 86_400_000 + 21 * 3_600_000;
        s.query_clock
            .0
            .advance(u64::try_from(nine - 30_000 - now).unwrap());
        let home = |session: &str| {
            let (html, _, _, _) = s
                .serve_document_settled(session, "store.page.HomePage~ui", &Params::new(), &[])
                .expect("served");
            visible(&html)
        };
        assert!(home("a").contains("On sale"));
        // From the kept entry, and held to 21:00 as the first was.
        assert!(home("b").contains("On sale"));
        let b = latest(&s.pending.lock().expect("pending"), "b");
        assert_eq!(s.timers.at(&b), Some(nine), "a kept value's instant");
        s.query_clock.0.advance(31_000);
        assert!(
            home("c").contains("Sale over"),
            "not the value from before 21:00"
        );
    }

    /// **A page that compares no clock is held by nothing** (ADR-0304): the
    /// store's order page is read again only when something changes. (The
    /// home page compared none until track `store-hours`, whose stores say
    /// whether they are open.)
    #[test]
    fn a_page_that_compares_no_clock_holds_nothing() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let _ = s
            .serve_document_settled("a", "store.page.OrderPage~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        assert_eq!(s.timers.at(&doc), None);
    }

    /// **The host's clock, as a test moves it** (ADR-0304): the system's,
    /// moved forward by `advance` or to `at`, never back.
    #[test]
    fn the_hosts_clock_moves_forward_as_a_test_says() {
        let s = rendering_server();
        let now = s.now();
        assert!((now - wall_millis()).abs() < 5_000, "the system's: {now}");
        let moved = posted(&s, "/bench/clock?advance=60000", "clock", "c-1", "");
        assert!(moved.starts_with("HTTP/1.1 200"), "{moved}");
        assert!(s.now() >= now + 60_000);
        let to = s.now() + 3_600_000;
        let moved = posted(&s, &format!("/bench/clock?at={to}"), "clock", "c-2", "");
        assert!(moved.starts_with("HTTP/1.1 200"), "{moved}");
        assert!(s.now() >= to && s.now() < to + 5_000);
        let back = posted(&s, &format!("/bench/clock?at={now}"), "clock", "c-3", "");
        assert!(back.starts_with("HTTP/1.1 400"), "never back: {back}");
        assert!(s.now() >= to, "and not moved");
    }

    /// **A page's command, here before the one it follows, runs after it**
    /// (ADR-0305): "Clear", pressed after "Place order", names it, and the
    /// network brings it first. It waits; the order is placed with the cart's
    /// line, and the cart is cleared after. Until 2026-10-10 the clear ran
    /// first, and the order was refused as nothing to order.
    #[test]
    fn a_pages_command_here_before_the_one_it_follows_runs_after_it() {
        let mut s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        // The clear waits for the order, which the test lets arrive once it
        // has heard the clear wait: for a minute, not the two seconds a slow
        // machine outwaited (the clear was then answered early, unrun).
        s.order = order::Order::waiting(std::time::Duration::from_secs(60));
        let session = "ordered";
        let body = serde_json::json!([shown("espresso"), 1]).to_string();
        let added = posted(&s, &format!("/command/{ADD}"), session, "press-0", &body);
        assert!(added.contains("\"committed\":true"), "{added}");
        // The total the cart page shows, which the press passes (track
        // `order-total`).
        let Val::S64(total) = s.shown_total(session) else {
            panic!("a total")
        };
        let to_place = format!("[{total}]");
        std::thread::scope(|scope| {
            let clear = scope.spawn(|| {
                let path = format!("/command/{CLEAR}");
                posted_after(&s, &path, session, "press-2", "press-1", "[]")
            });
            until("the clear waiting for the order", || s.order.waits() >= 1);
            assert!(!clear.is_finished(), "the clear waits for the order");
            assert_eq!(
                quantities(&s, session),
                [("espresso".to_string(), 1)],
                "and has not run"
            );
            let placed = posted(
                &s,
                &format!("/command/{PLACE}"),
                session,
                "press-1",
                &to_place,
            );
            assert!(placed.contains("\"$case\":\"ok\""), "placed: {placed}");
            let cleared = clear.join().expect("answered");
            assert!(cleared.starts_with("HTTP/1.1 202"), "{cleared}");
        });
        assert_eq!(quantities(&s, session), [], "cleared after");
    }

    /// **One whose first never comes is answered early, and runs nothing**
    /// (ADR-0305): sent again by its page, naming none, it runs.
    #[test]
    fn a_pages_command_whose_first_never_comes_is_answered_early_and_runs_nothing() {
        let mut s = rendering_server();
        s.order = order::Order::waiting(std::time::Duration::from_millis(200));
        let session = "early";
        let path = format!("/command/{ADD}");
        let body = serde_json::json!([shown("cortado"), 1]).to_string();
        let early = posted_after(&s, &path, session, "press-2", "press-1", &body);
        assert!(early.starts_with("HTTP/1.1 409"), "{early}");
        assert!(early.contains("\"early\":true"), "{early}");
        assert_eq!(quantities(&s, session), [], "nothing ran");
        // What follows one answered early is answered early, at once: it
        // does not wait (a bound of 150 ms on the answer was a speed the
        // machine set).
        let waited = s.order.waits();
        let after = posted_after(&s, &path, session, "press-3", "press-2", &body);
        assert!(after.starts_with("HTTP/1.1 409"), "{after}");
        assert_eq!(s.order.waits(), waited, "it did not wait");
        assert_eq!(quantities(&s, session), [], "nothing ran");
        let again = posted(&s, &path, session, "press-2", &body);
        assert!(again.contains("\"committed\":true"), "{again}");
        assert_eq!(quantities(&s, session), [("cortado".to_string(), 1)]);
    }

    /// **A session forgotten for idleness takes its commands' order**
    /// (ADR-0305, ADR-0161), as it takes its interactions.
    #[test]
    fn a_session_forgotten_takes_its_commands_order() {
        let mut s = rendering_server();
        s.order = order::Order::waiting(std::time::Duration::from_millis(100));
        let session = "idle-order";
        let _ = s.serve_document(session);
        let body = serde_json::json!([shown("cortado"), 1]).to_string();
        let added = posted(&s, &format!("/command/{ADD}"), session, "press-1", &body);
        assert!(added.contains("\"committed\":true"), "{added}");
        assert_eq!(s.order.follow(session, "press-1"), order::Turn::Run);
        let long_ago = std::time::Instant::now() - IDLE - std::time::Duration::from_secs(1);
        for w in s.pending.lock().unwrap().values_mut() {
            w.seen = long_ago;
        }
        s.forget_idle_subscribers();
        assert_eq!(s.order.follow(session, "press-1"), order::Turn::Early);
    }

    /// **One that follows a command whose connection closed before it ran
    /// waits for it to be sent again** (ADR-0305, ADR-0175): that one never
    /// ran, so it is not yet the one before.
    #[test]
    fn a_pages_command_after_one_closed_unrun_waits_for_it_sent_again() {
        let mut s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        // For a minute (see `a_pages_command_here_before_the_one_it_follows_runs_after_it`).
        s.order = order::Order::waiting(std::time::Duration::from_secs(60));
        let session = "closed";
        let body = serde_json::json!([shown("espresso"), 1]).to_string();
        let added = posted(&s, &format!("/command/{ADD}"), session, "press-0", &body);
        assert!(added.contains("\"committed\":true"), "{added}");
        // The total the cart page shows, which the press passes (track
        // `order-total`).
        let Val::S64(total) = s.shown_total(session) else {
            panic!("a total")
        };
        let to_place = format!("[{total}]");
        s.connection_faults
            .lock()
            .unwrap()
            .entry(session.into())
            .or_default()
            .drop_command = Some(DropAt::Before);
        let place = format!("/command/{PLACE}");
        assert_eq!(
            posted(&s, &place, session, "press-1", &to_place),
            "",
            "no answer"
        );
        std::thread::scope(|scope| {
            let clear = scope.spawn(|| {
                let path = format!("/command/{CLEAR}");
                posted_after(&s, &path, session, "press-2", "press-1", "[]")
            });
            until("the clear waiting for the order", || s.order.waits() >= 1);
            assert!(!clear.is_finished(), "the clear waits for the order");
            let placed = posted(&s, &place, session, "press-1", &to_place);
            assert!(placed.contains("\"$case\":\"ok\""), "placed: {placed}");
            let cleared = clear.join().expect("answered");
            assert!(cleared.starts_with("HTTP/1.1 202"), "{cleared}");
        });
    }

    /// Waits until `ready` holds, ten seconds at most (then it fails, saying
    /// `what` never came): for an event a test's own thread cannot be told
    /// of, as a subscriber's trail records it.
    fn until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + MUST_HAPPEN;
        while !ready() {
            assert!(std::time::Instant::now() < deadline, "never: {what}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Whether `doc`'s trail says `what` yet.
    fn trail_says(s: &Server, doc: &Doc, what: &str) -> bool {
        s.pending
            .lock()
            .expect("pending")
            .get(doc)
            .is_some_and(|waiting| waiting.trail.iter().any(|note| note.contains(what)))
    }

    /// **A forced reconnect** (charter §15.5, ADR-0175): the session's open
    /// subscription ends at once; a new one is closed with no answer for the
    /// cut's window; and one after it is sent what was queued meanwhile.
    ///
    /// The test waits for what its steps need to have happened, not for
    /// time: the stream is open when its trail says so, and it ended because
    /// it was cut when its trail says that (a stream that was not cut ends by
    /// itself after its passes, two seconds on). The window is an hour, which
    /// the test ends itself: a window of 800 ms ran out between the steps
    /// inside it on a slow machine.
    #[test]
    fn a_forced_reconnect_ends_a_subscription_and_refuses_new_ones_for_its_window() {
        let s = rendering_server();
        let session = "cut";
        let (_, document) = s.serve_document(session);
        let doc: Doc = (session.to_string(), document);
        let ms = std::time::Duration::from_millis;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let mut page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (mut held, _) = listener.accept().expect("accept");
        std::thread::scope(|scope| {
            let open =
                scope.spawn(|| stream_open(&s, &mut held, session, false, document, document));
            until("the stream open", || {
                trail_says(&s, &doc, "stream opened since")
            });
            // Through the control, as a test arms it.
            let armed = posted(&s, "/bench/reconnect?for=3600000", session, "cut-1", "");
            assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
            open.join().expect("ended");
        });
        assert!(
            trail_says(&s, &doc, "stream cut off at pass"),
            "it ended at once, cut off, not after its passes: {:?}",
            s.pending.lock().expect("pending")[&doc].trail
        );
        // What the server wrote before it ended: its end of the connection
        // closed, as `handle` closes it when the subscription returns.
        drop(held);
        page.set_read_timeout(Some(ms(10_000))).expect("timeout");
        let mut written = String::new();
        let _ = page.read_to_string(&mut written);
        assert!(written.starts_with("HTTP/1.1 200"), "{written}");
        assert!(
            !written.contains("\"frames\""),
            "and nothing after: {written}"
        );

        // A change made while the page is cut off.
        s.command(ADD, session, &add_shown("espresso", 1), false)
            .expect("runs");
        let poll = format!("/stream?doc={document}&since={document}");
        let refused = fetched_as(&s, &poll, Some(session));
        assert_eq!(
            refused.iter().map(|(_, c)| c.as_str()).collect::<String>(),
            "",
            "a new one, inside the window: no answer"
        );
        // A stream too, which would otherwise be sent its head before its
        // first look, and be asked for again at once.
        let streamed = fetched_as(
            &s,
            &format!("/stream?mode=stream&doc={document}&since={document}"),
            Some(session),
        );
        assert_eq!(
            streamed.iter().map(|(_, c)| c.as_str()).collect::<String>(),
            "",
            "a new stream, inside the window: not even its head"
        );
        // The window ends.
        s.connection_faults
            .lock()
            .unwrap()
            .get_mut(session)
            .expect("the session's faults")
            .cut_until = None;
        let served = fetched_as(&s, &poll, Some(session))
            .into_iter()
            .map(|(_, c)| c)
            .collect::<String>();
        assert!(
            served.starts_with("HTTP/1.1 200") && served.contains("\"frames\""),
            "after it, what was queued: {served}"
        );

        // With no window, a blip: what is open ends at once, and a page that
        // asks again is served at once.
        let session = "blip";
        let (_, document) = s.serve_document(session);
        let doc: Doc = (session.to_string(), document);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let _page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (mut held, _) = listener.accept().expect("accept");
        std::thread::scope(|scope| {
            let open =
                scope.spawn(|| stream_open(&s, &mut held, session, false, document, document));
            until("the stream open", || {
                trail_says(&s, &doc, "stream opened since")
            });
            let armed = posted(&s, "/bench/reconnect?for=0", session, "blip-1", "");
            assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
            open.join().expect("ended");
        });
        assert!(
            trail_says(&s, &doc, "stream cut off at pass"),
            "it ended at once, cut off: {:?}",
            s.pending.lock().expect("pending")[&doc].trail
        );
        let again = fetched_as(
            &s,
            &format!("/stream?doc={document}&since={document}"),
            Some(session),
        )
        .into_iter()
        .map(|(_, c)| c)
        .collect::<String>();
        assert!(again.starts_with("HTTP/1.1 200"), "served at once: {again}");

        // A long poll held when the cut comes ends at once too, unanswered.
        let session = "cut-poll";
        let (_, document) = s.serve_document(session);
        let doc: Doc = (session.to_string(), document);
        let poll = format!("/stream?doc={document}&since={document}");
        // A pass of the poll marks its page as asking (`seen`): the poll is
        // held in its loop once that has changed.
        let asked = s.pending.lock().expect("pending")[&doc].seen;
        std::thread::scope(|scope| {
            let held = scope.spawn(|| fetched_as(&s, &poll, Some(session)));
            until("the poll held", || {
                s.pending.lock().expect("pending")[&doc].seen != asked
            });
            s.connection_faults
                .lock()
                .unwrap()
                .entry(session.into())
                .or_default()
                .cut_from = Some(std::time::Instant::now());
            let answered = held.join().expect("ended");
            // A poll that ran its passes out answers an empty batch; this
            // one ended with no answer at all.
            assert_eq!(
                answered.iter().map(|(_, c)| c.as_str()).collect::<String>(),
                "",
                "unanswered"
            );
        });
    }

    /// **A materializer failure** (charter §15.5, ADR-0176): a regeneration
    /// that fails sends nothing at the version that did not move, and the
    /// commit's answer names a version a later one passes. The session's
    /// next drain tries again, and sends the change.
    #[test]
    fn a_regeneration_that_fails_sends_nothing_and_is_tried_again() {
        let s = speculating_server();
        let session = "failing";
        s.serve_document_with_entries(session).expect("served");
        let values = |s: &Server| -> Vec<(u64, serde_json::Value)> {
            let queue = s.pending.lock().expect("pending");
            queue[&latest(&queue, session)]
                .frames
                .iter()
                .filter_map(|(_, f)| match f {
                    StreamFrame::EntryValue { version, value, .. } => {
                        Some((version.0, value.clone()))
                    }
                    _ => None,
                })
                .collect()
        };
        let before = s.version(session);
        // Through the control, as a test arms it.
        let armed = posted(&s, "/bench/materializer?fail=next", session, "arm-1", "");
        assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
        s.command_json(
            ADD,
            session,
            &[shown("espresso"), serde_json::json!(1)],
            Some("press-1"),
        )
        .expect("well-formed")
        .expect("committed");
        assert_eq!(values(&s), [], "nothing sent");
        assert_eq!(s.version(session), before, "the version did not move");
        let promised = s.committed_basis(session)[0]["version"]
            .as_u64()
            .expect("a version");
        assert!(promised > before.0, "the answer names one a later passes");

        // Tried again, as the page's next subscription request does.
        s.drain(session);
        let sent = values(&s);
        let [(version, value)] = sent.as_slice() else {
            panic!("one value, got {sent:?}");
        };
        assert!(*version >= promised, "{version} >= {promised}");
        assert_eq!(value["lines"][0]["item_id"], "espresso");
        // And once: a drain with nothing changed sends nothing more.
        s.drain(session);
        assert_eq!(values(&s).len(), 1);
    }

    /// **Last-known-good, for public data alone** (charter §15.6 test 18,
    /// ADR-0177): while the store's origin fails, a page is served the last
    /// store kept; while a session's cart's read fails, its page is not.
    #[test]
    fn a_failed_origin_is_answered_with_the_last_public_value_kept() {
        let s = rendering_server();
        // The store read, and kept.
        assert!(visible(&s.render_store("first")).contains("Blue Bottle"));
        let reads = calls(&s, "store:data/stores#get");
        // Its origin fails once, and what was kept is expired.
        let armed = posted(&s, "/bench/store?fail=next", "first", "arm-1", "");
        assert!(armed.starts_with("HTTP/1.1 200"), "{armed}");
        let page = s.render_store("second");
        assert!(
            visible(&page).contains("Blue Bottle"),
            "the last store kept"
        );
        assert_eq!(
            calls(&s, "store:data/stores#get"),
            reads + 1,
            "its origin was asked"
        );
        assert!(
            s.queries
                .trace()
                .iter()
                .any(|t| matches!(t, pw_resource::Trace::ServedLastKnownGood { .. })),
            "and failed, and what was kept answered"
        );
        // The origin back, the store is read again.
        s.queries.expire("store.page.Store");
        assert!(visible(&s.render_store("third")).contains("Blue Bottle"));
        assert_eq!(calls(&s, "store:data/stores#get"), reads + 2);

        // A session's cart, whose read fails: not served from anything kept.
        s.serve_document_with_entries("fourth").expect("served");
        s.store
            .faults()
            .cart_faults
            .lock()
            .unwrap()
            .entry("fourth".into())
            .or_default()
            .fail_read = true;
        let refused = s
            .serve_document_with_entries("fourth")
            .expect_err("a private read that failed");
        assert!(refused.contains("the database is unavailable"), "{refused}");
    }

    /// **What a line records is the store's, not the request's** (ADR-0172).
    /// A page sends the item it showed, and a request can send any name and
    /// any price for it, or an item no store has.
    #[test]
    fn a_line_records_the_stores_name_and_price_not_the_requests() {
        let s = rendering_server();
        let [_, quantity] = add_shown("cortado", 1);
        let sent = |id: &str| {
            Val::Record(vec![
                ("id".into(), Val::String(id.into())),
                ("store-id".into(), Val::String(STORE_ID.into())),
                ("name".into(), Val::String("Free coffee".into())),
                ("description".into(), Val::String(String::new())),
                ("price".into(), Val::S64(1)),
                ("available".into(), Val::Bool(true)),
                (
                    "category".into(),
                    Val::Record(vec![
                        ("id".into(), Val::String("free".into())),
                        ("name".into(), Val::String("Free".into())),
                    ]),
                ),
            ])
        };
        s.command(
            ADD,
            "session-9",
            &[sent("cortado"), quantity.clone()],
            false,
        )
        .expect("runs");
        let recorded = |s: &Server| s.cart_priced("session-9");
        assert_eq!(recorded(&s), [("Cortado".to_string(), 425)]);
        // An item no store has is not available: refused by name, before
        // anything is written.
        let refused = s
            .command(ADD, "session-9", &[sent("free-coffee"), quantity], false)
            .expect_err("no store has it");
        assert!(refused.contains("item-unavailable"), "{refused}");
        assert_eq!(recorded(&s), [("Cortado".to_string(), 425)]);
    }

    /// **A failed command commits nothing.** The data layer answers
    /// `cart-expired`, the COMPONENT returns it, and neither the staged write
    /// nor its event survives — ADR-0019, now decided by what the compiled
    /// command returned rather than by a closure's control flow.
    #[test]
    fn a_failing_command_commits_neither_state_nor_event() {
        let s = rendering_server();
        s.command(ADD, "session-3", &add_shown("espresso", 1), false)
            .expect("runs");
        let err = s
            .command(ADD, "session-3", &add_shown("cortado", 5), true)
            .expect_err("the data layer refuses");
        assert!(
            err.contains("cart-expired"),
            "the component's own result: {err}"
        );
        assert_eq!(s.cart_value("session-3"), 1, "the failed add left nothing");
    }

    /// **No Rust closure computes a command, and no Rust writes a handler.**
    /// Structural, because the failure it guards against — a body quietly
    /// reimplemented beside the component, or a handler module written here
    /// instead of compiled — would pass every behavioural test above.
    ///
    /// The needles are assembled, so this test's own text cannot match them.
    #[test]
    fn the_commands_and_handlers_are_the_compilers() {
        let src = include_str!("main.rs");
        // The test module, not the first test-only item: `Server::on` and the
        // committed-artifact loaders are test-only too (ADR-0123).
        let server_code = &src[..src
            .find(&["#[cfg(test)]\nmod ", "tests"].concat())
            .expect("the tests")];
        // Where a command's component runs: `command_held`, in the session's
        // hold, which `command_answered` answers through since ADR-0219, and
        // `command` since ADR-0157.
        let start = server_code
            .find("fn command_held(")
            .expect("the command path");
        let body =
            &server_code[start..start + server_code[start..].find("\n    }\n").expect("its end")];
        assert!(
            body.contains("self.run(component_id, session, &host, args)"),
            "every command runs its compiled component"
        );
        assert!(
            !body.contains(&["\"Events", "."].concat()),
            "a command's events are the ones it declares (ADR-0104), never named here"
        );
        for gone in [
            ["fn add_to_", "cart("].concat(),
            ["fn clear_", "cart("].concat(),
            ["fn menu_item", "_at("].concat(),
            ["export async ", "function run"].concat(),
        ] {
            assert!(
                !server_code.contains(&gone),
                "`{gone}` is back: per-command Rust, or a handler written by the server"
            );
        }
    }

    /// **A command's arguments from a browser are typed by the component.**
    ///
    /// What `dist/handlers/<identity>.mjs` sends: `context.command(id, args)`
    /// posts the arguments as JSON, and they are converted by the parameter
    /// types the ARTIFACT declares before anything runs.
    #[test]
    fn a_browsers_arguments_are_typed_by_the_components_own_parameters() {
        let s = rendering_server();
        // The item as its row shows it, a record (ADR-0172): a field the type
        // does not declare is not passed in.
        let cortado = shown("cortado");
        let mut with_more = cortado.clone();
        with_more["calories"] = serde_json::json!(5);
        let ran = s
            .command_json(
                ADD,
                "session-5",
                &[with_more, serde_json::json!(2)],
                Some("press-1"),
            )
            .expect("well-formed");
        ran.expect("and it committed");
        assert_eq!(s.cart_value("session-5"), 2);

        // Refused before anything runs, each for its own reason.
        let mut without_name = cortado.clone();
        without_name
            .as_object_mut()
            .expect("a record")
            .remove("name");
        let mut priced_as_text = cortado.clone();
        priced_as_text["price"] = serde_json::json!("425");
        for (args, why) in [
            (vec![cortado.clone()], "takes 2 argument(s); 1 were sent"),
            (
                vec![serde_json::json!("cortado"), serde_json::json!(2)],
                "expected an object",
            ),
            (
                vec![without_name, serde_json::json!(2)],
                "the record has no field `name`",
            ),
            (
                vec![priced_as_text, serde_json::json!(2)],
                "argument 1 (`arg0`).price: expected an integer",
            ),
            (
                vec![cortado.clone(), serde_json::json!(1.5)],
                "expected an integer",
            ),
            (
                vec![cortado.clone(), serde_json::json!("2")],
                "expected an integer",
            ),
            (
                vec![cortado.clone(), serde_json::json!(u64::MAX)],
                "is outside",
            ),
        ] {
            let err = s
                .command_json(ADD, "session-5", &args, Some("press-2"))
                .expect_err("malformed");
            assert!(err.contains(why), "{args:?}: {err}");
        }
        assert_eq!(
            s.cart_value("session-5"),
            2,
            "no refused request moved the state"
        );

        let err = s
            .command_json("store.page.nothing_declares_this", "session-5", &[], None)
            .expect_err("not hosted");
        assert!(err.contains("no compiled component"), "{err}");
    }

    /// **`clear_cart` commits its state as well as its event.** Until the
    /// command path was one path, `clear_cart` committed only the event, and
    /// the materializer's `cart:<session>` state kept the old total.
    /// The manifest `pw emit-speculations` writes for the store page.
    fn speculating_server() -> Server {
        let mut s = rendering_server();
        s.speculations.insert(
            "store.page.StorePage~ui".to_string(),
            serde_json::json!({
                "page": "store.page.StorePage~ui",
                "module": "store.page.StorePage~ui.mjs",
                "bindings": [{ "binding": "cart", "resource": "store.page.Cart", "key": ["current_session()"] }],
                "commands": ["store.page.add_to_cart"],
            }),
        );
        s
    }

    /// **A page that speculates is sent the value it speculates on**
    /// (ADR-0122): in its document, and as an `entry_value` frame each time
    /// the entry advances, with the version a commit produced in the
    /// command's answer. A page that does not speculate is sent none of it.
    #[test]
    fn a_speculating_page_is_sent_its_carts_value() {
        let s = speculating_server();
        let (_, _, entries) = s.serve_document_with_entries("session-v").expect("served");
        assert_eq!(entries["cart"]["value"], serde_json::json!({ "lines": [] }));
        let before = entries["cart"]["version"].as_u64().expect("a version");

        s.command_json(
            ADD,
            "session-v",
            &[shown("cortado"), serde_json::json!(2)],
            Some("press-v"),
        )
        .expect("well-formed")
        .expect("committed");
        let basis = s.committed_basis("session-v");
        let version = basis[0]["version"].as_u64().expect("a version");
        assert!(version > before, "the commit produced a newer version");
        assert_eq!(
            basis[0]["entry"], entries["cart"]["entry"],
            "of the same entry"
        );

        let queue = s.pending.lock().expect("pending");
        let values: Vec<&StreamFrame> = queue[&latest(&queue, "session-v")]
            .frames
            .iter()
            .map(|(_, f)| f)
            .filter(|f| matches!(f, StreamFrame::EntryValue { .. }))
            .collect();
        let [
            StreamFrame::EntryValue {
                version: v,
                value,
                applied,
                ..
            },
        ] = values.as_slice()
        else {
            panic!("one entry_value frame, got {values:?}");
        };
        assert_eq!(v.0, version, "at the version the commit produced");
        assert_eq!(
            *value,
            serde_json::json!({ "lines": [
                { "item_id": "cortado", "name": "Cortado", "quantity": 2, "unit_price": 425 }
            ] })
        );
        // The presses the value includes (ADR-0172): the page drops their
        // speculations, which it would otherwise show again over it.
        assert_eq!(*applied, ["press-v"]);
        drop(queue);
        s.command_json(
            INCREASE,
            "session-v",
            &[serde_json::json!("cortado")],
            Some("press-v2"),
        )
        .expect("well-formed")
        .expect("committed");
        let queue = s.pending.lock().expect("pending");
        let Some(StreamFrame::EntryValue { applied, value, .. }) = queue
            [&latest(&queue, "session-v")]
            .frames
            .iter()
            .map(|(_, f)| f)
            .rfind(|f| matches!(f, StreamFrame::EntryValue { .. }))
        else {
            panic!("a second entry_value frame");
        };
        assert_eq!(*applied, ["press-v", "press-v2"]);
        assert_eq!(value["lines"][0]["quantity"], 3);
        drop(queue);

        // The control: the same page, built without speculations.
        let plain = rendering_server();
        let (_, _, entries) = plain
            .serve_document_with_entries("session-w")
            .expect("served");
        assert_eq!(entries, serde_json::json!({}));
        plain
            .command_json(
                ADD,
                "session-w",
                &[shown("cortado"), serde_json::json!(1)],
                Some("press-w"),
            )
            .expect("well-formed")
            .expect("committed");
        let queue = plain.pending.lock().expect("pending");
        assert!(
            !queue[&latest(&queue, "session-w")]
                .frames
                .iter()
                .any(|(_, f)| matches!(f, StreamFrame::EntryValue { .. })),
            "no value to a page that does not speculate"
        );
    }

    /// **What the page shows is its queries'** (ADR-0125). Structural, as
    /// `the_commands_and_handlers_are_the_compilers` is: a value written here
    /// in Rust would render correctly and pass every behavioural test while
    /// the compiled queries ran for nothing. The needles are assembled.
    #[test]
    fn the_pages_values_are_its_queries() {
        let src = include_str!("main.rs");
        let server_code = &src[..src
            .find(&["#[cfg(test)]\nmod ", "tests"].concat())
            .expect("the tests")];
        for gone in [
            [".set(\"store", ".name\""].concat(),
            [".set(\"cart", ".line_count\""].concat(),
            ["Value::Text(\"Blue", " Bottle\""].concat(),
        ] {
            assert!(
                !server_code.contains(&gone),
                "`{gone}` is back: a page value computed by the server, not its query"
            );
        }
        // And the render goes through the plan.
        let s = rendering_server();
        let html = s.render_store("session-q");
        assert!(
            html.contains(STORE_NAME),
            "the Store query's name is rendered"
        );
        s.command(ADD, "session-q", &add_shown("cortado", 3), false)
            .expect("runs");
        assert_eq!(
            s.part_text("session-q", "cart.line_count").as_deref(),
            Ok("3")
        );
    }

    fn calls(s: &Server, op: &str) -> u64 {
        s.calls.lock().expect("calls").get(op).copied().unwrap_or(0)
    }

    /// **A shared query runs once for every reader, within its freshness**
    /// (ADR-0127). `Store` is shared for 30 seconds and `Menu` for five
    /// minutes; `Cart` is private and kept for no time, so each render runs it.
    #[test]
    fn a_shared_query_is_run_once_for_its_readers_and_a_private_one_for_each() {
        let s = rendering_server();
        s.render_store("reader-a");
        s.render_store("reader-b");
        assert_eq!(
            calls(&s, "store:data/stores#get"),
            1,
            "one Store for both readers"
        );
        assert_eq!(
            calls(&s, "store:data/menus#sections"),
            1,
            "one Menu for both"
        );
        assert_eq!(
            calls(&s, "store:data/user-carts#current"),
            2,
            "a Cart for each reader"
        );
        // And `Cart`, which the page and its layout both bind, asked once for
        // each reader's document (ADR-0303). Its operation's calls no longer
        // show that alone: one render makes an identical call once (NEXT 4c's
        // render merge), so they stay at two where the query is asked twice.
        let carts = s
            .queries
            .trace()
            .iter()
            .filter(|t| {
                matches!(t, pw_resource::Trace::RequestStarted { key, .. }
                    if key.resource == "store.page.Cart")
            })
            .count();
        assert_eq!(carts, 2, "Cart asked once for each reader's document");

        // Past Store's 30 seconds and inside Menu's five minutes.
        s.query_clock.0.advance(30_001);
        s.render_store("reader-a");
        assert_eq!(calls(&s, "store:data/stores#get"), 2, "Store expired");
        assert_eq!(calls(&s, "store:data/menus#sections"), 1, "Menu did not");
    }

    /// **A command drops exactly the entry it invalidates** (ADR-0127): the
    /// session's cart, and not another session's. Cart is kept for no time in
    /// the store, so the test keeps it for a minute to see what is dropped.
    #[test]
    fn a_command_drops_the_entry_it_invalidates_and_no_other() {
        let mut s = rendering_server();
        // The cart's every binding, the page's and its layout's (ADR-0303):
        // a query's policy is its declaration's.
        for b in s.plan["bindings"].as_array_mut().expect("bindings") {
            if b["resource"] == "store.page.Cart" {
                b["policy"]["freshness_ms"] = serde_json::json!(60_000);
            }
        }
        s.render_store("one");
        s.render_store("two");
        assert_eq!(calls(&s, "store:data/user-carts#current"), 2);
        s.render_store("one");
        assert_eq!(calls(&s, "store:data/user-carts#current"), 2, "kept");

        s.command(ADD, "one", &add_shown("espresso", 1), false)
            .expect("runs");
        assert!(
            s.render_store("one").contains(">1<"),
            "one's own add is shown"
        );
        assert_eq!(
            calls(&s, "store:data/user-carts#current"),
            3,
            "one's entry was dropped"
        );
        s.render_store("two");
        assert_eq!(
            calls(&s, "store:data/user-carts#current"),
            3,
            "two's was not"
        );
    }

    /// **Commands that invalidate each other's reads all commit** (ADR-0127).
    /// Each commit drops the session's cart entry while another command may
    /// be re-reading it; an invalidated read is read again rather than failed.
    /// Two presses at once failed both before this.
    #[test]
    fn concurrent_commands_on_one_session_all_commit() {
        let s = std::sync::Arc::new(rendering_server());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for _ in 0..5 {
                        s.command(ADD, "busy", &add_shown("espresso", 1), false)
                            .expect("commits");
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("no command failed");
        }
        assert_eq!(s.cart_value("busy"), 40);
        assert_eq!(s.part_text("busy", "cart.line_count").as_deref(), Ok("40"));
    }

    /// **A change to the menu drops the Menu query's values** (ADR-0127).
    #[test]
    fn a_menu_change_drops_the_menus_kept_values() {
        let s = rendering_server();
        s.render_store("reader");
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".into(),
            name: "Ristretto".into(),
        })
        .expect("renamed");
        let html = s.render_store("reader");
        assert!(
            html.contains("Ristretto"),
            "the page shows the changed menu: {html}"
        );
        assert_eq!(calls(&s, "store:data/menus#sections"), 2);
    }

    /// **A private entry is keyed by its session; a shared one by its
    /// arguments alone** (ADR-0127).
    #[test]
    fn an_entry_key_is_scoped_by_session_only_when_private() {
        let shared = serde_json::json!({ "cache": "shared", "privacy": "public", "key": [0] });
        let private = serde_json::json!({ "cache": "private", "privacy": "session", "key": [0] });
        let args = [Val::String("47".into())];
        assert_eq!(
            entry_key("a", &shared, &args),
            entry_key("b", &shared, &args)
        );
        assert_ne!(
            entry_key("a", &private, &args),
            entry_key("b", &private, &args)
        );
        let none = serde_json::json!({ "cache": "shared", "privacy": "public", "key": [1] });
        assert_eq!(
            entry_key("a", &none, &args),
            None,
            "a key naming an argument not given"
        );
    }

    /// **An idempotent command runs once per interaction** (ADR-0121).
    /// `add_to_cart` declares `idempotent_by InteractionId`, and until
    /// 2026-10-02 nothing read it: a retried request added twice.
    #[test]
    fn a_retried_interaction_runs_its_command_once() {
        let s = rendering_server();
        let espresso = [shown("espresso"), serde_json::json!(1)];
        for _ in 0..3 {
            s.command_json(ADD, "session-r", &espresso, Some("press-a"))
                .expect("well-formed")
                .expect("committed, or the first commit's outcome");
        }
        assert_eq!(s.cart_value("session-r"), 1, "three sends, one interaction");

        // Another interaction is another add, and another session's same
        // interaction id is its own.
        s.command_json(ADD, "session-r", &espresso, Some("press-b"))
            .expect("well-formed")
            .expect("committed");
        assert_eq!(s.cart_value("session-r"), 2);
        s.command_json(ADD, "session-q", &espresso, Some("press-a"))
            .expect("well-formed")
            .expect("committed");
        assert_eq!(s.cart_value("session-q"), 1);

        // Refused before anything runs: no interaction, a malformed one, and
        // one reused with other arguments.
        let err = s
            .command_json(ADD, "session-r", &espresso, None)
            .expect_err("an idempotent command needs its key");
        assert!(err.contains("carries no interaction id"), "{err}");
        let err = s
            .command_json(ADD, "session-r", &espresso, Some("a b"))
            .expect_err("malformed");
        assert!(err.contains("is not an interaction id"), "{err}");
        let other = [shown("cortado"), serde_json::json!(1)];
        let err = s
            .command_json(ADD, "session-r", &other, Some("press-a"))
            .expect_err("the same interaction is the same request");
        assert!(err.contains("first sent with other arguments"), "{err}");
        assert_eq!(s.cart_value("session-r"), 2, "no refusal moved the state");

        // **One that committed nothing is held by the session's memory
        // alone** (ADR-0325): a declared error records nothing, so the
        // interaction sent again with other arguments is refused by what the
        // session kept, as before a record was committed (ADR-0121), and is
        // not answered the first's outcome.
        s.stock_untold("cortado", false);
        let declared = s
            .command_json(ADD, "session-r", &other, Some("press-c"))
            .expect("well-formed")
            .expect_err("sold out");
        assert!(declared.contains("item-unavailable"), "{declared}");
        let err = s
            .command_json(ADD, "session-r", &espresso, Some("press-c"))
            .expect_err("the same interaction is the same request");
        assert!(err.contains("first sent with other arguments"), "{err}");
        assert_eq!(s.cart_value("session-r"), 2, "nothing moved the state");
        s.stock_untold("cortado", true);
    }

    /// **What an idempotent command keeps is bounded** (ADR-0121, and E10
    /// gate item 3's property): per session, the last
    /// `INTERACTIONS_PER_SESSION` interactions.
    #[test]
    fn interactions_kept_are_bounded_per_session() {
        let s = rendering_server();
        let espresso = [shown("espresso"), serde_json::json!(1)];
        let presses = INTERACTIONS_PER_SESSION + 40;
        for i in 0..presses {
            s.command_json(ADD, "session-b", &espresso, Some(&format!("p{i}")))
                .expect("well-formed")
                .expect("committed");
        }
        assert_eq!(s.cart_value("session-b"), presses as i64);
        assert_eq!(s.commands.commands_held(), INTERACTIONS_PER_SESSION);
        // The newest is still recognised in memory.
        s.command_json(
            ADD,
            "session-b",
            &espresso,
            Some(&format!("p{}", presses - 1)),
        )
        .expect("well-formed")
        .expect("its first outcome");
        assert_eq!(s.cart_value("session-b"), presses as i64);
        // **And the oldest, by its record committed with its writes**
        // (ADR-0325): the bound is the reservations' alone. Until ADR-0325 it
        // was the bound's cost, and the cart took a line more.
        s.command_json(ADD, "session-b", &espresso, Some("p0"))
            .expect("well-formed")
            .expect("its first outcome, from its record");
        assert_eq!(s.cart_value("session-b"), presses as i64);
        // Sent with other arguments, it is refused, as in memory (ADR-0121).
        let other = [shown("cortado"), serde_json::json!(1)];
        let refused = s
            .command_json(ADD, "session-b", &other, Some("p0"))
            .expect_err("another request under the same interaction");
        assert!(
            refused.contains("first sent with other arguments"),
            "{refused}"
        );
        assert_eq!(s.cart_value("session-b"), presses as i64);
    }

    /// **An interaction's record answers it for a day** (ADR-0325): past the
    /// session's 64 in memory, the oldest is answered by its record within
    /// the day, and is a new request a day and a minute later, as a key
    /// Stripe has pruned is.
    #[test]
    fn an_interactions_record_answers_it_for_a_day() {
        let s = rendering_server();
        let espresso = [shown("espresso"), serde_json::json!(1)];
        let presses = INTERACTIONS_PER_SESSION + 1;
        for i in 0..presses {
            s.command_json(ADD, "session-d", &espresso, Some(&format!("d{i}")))
                .expect("well-formed")
                .expect("committed");
        }
        // Within the day: its record answers, and nothing runs again.
        posted(&s, "/bench/clock?advance=82800000", "clock", "c-1", "");
        s.command_json(ADD, "session-d", &espresso, Some("d0"))
            .expect("well-formed")
            .expect("its first outcome, from its record");
        assert_eq!(s.cart_value("session-d"), presses as i64);
        // A day and a minute after it was recorded: a new request, which runs.
        posted(&s, "/bench/clock?advance=3660000", "clock", "c-2", "");
        s.command_json(ADD, "session-d", &espresso, Some("d0"))
            .expect("well-formed")
            .expect("committed again");
        assert_eq!(s.cart_value("session-d"), presses as i64 + 1);
    }

    #[test]
    fn every_command_commits_its_state_and_its_event_together() {
        let s = rendering_server();
        s.command(ADD, "session-6", &add_shown("espresso", 3), false)
            .expect("runs");
        assert_eq!(s.materializer.state("cart:session-6").as_deref(), Some("3"));
        s.command_json(CLEAR, "session-6", &[], Some("clear-1"))
            .expect("well-formed")
            .expect("and it committed");
        assert_eq!(s.cart_value("session-6"), 0);
        assert_eq!(
            s.materializer.state("cart:session-6").as_deref(),
            Some("0"),
            "the cleared cart's state, not the old total"
        );
    }

    /// **A query's `retry` reaches its cache as declared** (ADR-0215): its
    /// attempts, and whether its delays vary. Until then every query
    /// jittered, whatever it declared.
    #[test]
    fn a_querys_retry_reaches_its_cache_as_declared() {
        let exact = runtime_manifest("r", &serde_json::json!({ "attempts": 3 }));
        assert_eq!((exact.max_attempts, exact.jitter), (3, false));
        let varied = runtime_manifest("r", &serde_json::json!({ "attempts": 3, "jitter": true }));
        assert_eq!((varied.max_attempts, varied.jitter), (3, true));
    }

    /// **The feed reference app, built and served** (ADR-0218): its data the
    /// feed's layer, chosen because its contracts import `feed:data/…`.
    fn served_feed() -> Served {
        served_feed_with(|app| app.to_string())
    }

    /// [`served_feed`], its `app.pw` changed by `change`.
    fn served_feed_with(change: fn(&str) -> String) -> Served {
        let (dir, out) = built_feed_with(change);
        Served {
            server: Server::from_build(out.clone(), out).expect("served"),
            _dir: dir,
        }
    }

    /// The feed, its `app.pw` changed by `change`, built as `pw build` builds
    /// it and not yet served: its directory, and the build in it.
    fn built_feed_with(change: fn(&str) -> String) -> (tempfile::TempDir, std::path::PathBuf) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let dir = tempfile::TempDir::with_prefix("pw-feed-").expect("a temporary directory");
        let mut units = Vec::new();
        for d in [
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/feed",
        ] {
            let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
                .unwrap_or_else(|e| panic!("{d}: {e}"))
                .map(|e| e.expect("entry").path())
                .filter(|p| p.extension().is_some_and(|x| x == "pw"))
                .collect();
            paths.sort();
            for p in paths {
                let mut src = std::fs::read_to_string(&p).expect("read");
                if d == "examples/feed" && p.ends_with("app.pw") {
                    src = change(&src);
                }
                units.push(pw_core::check::Unit {
                    hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                    path: p.display().to_string(),
                    src,
                });
            }
        }
        let build = pw_core::build::build(&units).expect("the feed builds");
        assert!(build.refusals().is_empty(), "{:?}", build.refusals());
        let out = dir.path().join("build");
        build.write(&out).expect("the build is written");
        (dir, out)
    }

    /// The feed on PostgreSQL (ADR-0246): its tests, which skip without a
    /// database.
    mod feed_pg;

    /// An entry written `_` is every entry at the rest, in every session's
    /// partition (ADR-0256).
    mod every_entry;

    /// The follows timeline (ADR-0257): its tests, in memory and on
    /// PostgreSQL where a database is named.
    mod follows;

    /// TRACK SEAM (identity): accounts and sign-in, served.
    mod sign_in;

    // TRACK SEAM (uploads, ADR-0253): an image on a post, in memory and on
    // PostgreSQL where a database is named.
    mod uploads;

    // TRACK SEAM (notifications): notifications, in memory and on
    // PostgreSQL where a database is named.
    mod notifications;

    /// A materialization kept, and served (ADR-0277).
    mod materializations;

    /// A page at another address of the page, moved there (ADR-0295).
    mod redirects;

    // TRACK SEAM (messages): direct messages, in memory and on PostgreSQL
    // where a database is named.
    mod messages;

    /// Track `store-pg`: the store on PostgreSQL, the tests that look at the
    /// database itself, which skip without one.
    mod store_pg;

    // TRACK SEAM (store-accounts): a cart and an order a user's, on the
    // layer the server's tests serve the store on.
    mod store_accounts;

    // TRACK SEAM (store-accounts): delivery addresses, and a store's reach.
    mod addresses;

    // TRACK SEAM (kiokun): kiokun.com's word page, served from kiokun's
    // files (docs/PARALLEL.md, W6).
    mod kiokun;

    // TRACK SEAM (todomvc): a page's fragment binding reaches its document
    // (docs/PARALLEL.md, W7).
    mod address_fragment;

    // TRACK SEAM (todomvc): TodoMVC, built and served (docs/PARALLEL.md, W7).
    mod todomvc;

    // TRACK SEAM (device-store): a page that holds a value the device keeps,
    // served (ADR-0366).
    mod device_store;

    // TRACK SEAM (store-search): the store's search page, served (W9).
    mod store_search;

    // TRACK SEAM (store-hours): a store's opening hours, served (W9).
    mod store_hours;
    // TRACK SEAM (order-total): an order's total.
    mod order_total;
    // TRACK SEAM (bounded-statements): a read is a bounded number of
    // statements on PostgreSQL (NEXT 4b).
    mod bounded_statements;
    /// What a page this host serves may load and run (`csp`).
    mod page_policy;
    // TRACK SEAM (render-merge): one render's identical calls made once, in
    // one snapshot (NEXT 4c).
    mod render_merge;
    // TRACK SEAM (payments): an order awaiting its payment, and settled.
    mod store_payments;
    // The scheduled-commands track: a command run when the clock passes its
    // time, and the store's sweep.
    mod scheduled_memory;
    mod scheduled_probe;
    #[cfg(feature = "scheduler-postgres-tests")]
    mod scheduled_replay;
    mod store_scheduled;
    // TRACK SEAM (reviews): a store's ratings and reviews, in memory and on
    // PostgreSQL where a database is named.
    mod reviews;
    // TRACK SEAM (argument-invariants): a value from a browser held to its
    // type's invariant where it arrives (ADR-0346).
    mod argument_invariants;
    // ADR-0351: the actual member receiver survives component lowering.
    mod receiver_identity;
    // TRACK SEAM (scheduled-orders): an order placed for a time its reader
    // chooses, and sent to its stores when it is due.
    mod scheduled_orders;
    // Track `second-host` (ADR-0369): hosts sharing one database.
    mod second_host;

    /// **A second program is served by the same host** (ADR-0218): the
    /// feed's timeline from its data layer, and a post committed and sent
    /// to the session's document. Until ADR-0218 the host served the store
    /// alone, and the feed's build failed at start.
    #[test]
    fn the_feed_is_served_by_the_host_its_data_the_deployments() {
        let s = served_feed();
        let (html, _, _, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        assert!(html.contains("Hello, feed."), "{html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let answered = s
            .command_answered(
                "feed.app.post",
                "a",
                &[Val::String("A first post".into())],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        let set = sets_of(&s, &doc).pop().expect("a patch set");
        assert!(
            format!("{set:?}").contains("A first post"),
            "the timeline shows the post: {set:?}"
        );
    }

    /// **A post reaches every open timeline** (ADR-0219): `Timeline` listens
    /// for `Posted(_)`, so the post drops every session's answer, and each
    /// other session's open home page is read again and sent the post. Until
    /// ADR-0219 only the author's page was told, and another reader saw the
    /// post when the page was next loaded.
    #[test]
    fn a_post_reaches_every_open_timeline() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let (mine, theirs) = {
            let pending = s.pending.lock().expect("pending");
            (latest(&pending, "a"), latest(&pending, "b"))
        };
        let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
        let before = frames(&theirs);
        let answered = s
            .command_answered(
                "feed.app.post",
                "a",
                &[Val::String("Seen by everyone".into())],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        assert_eq!(frames(&theirs), before, "not before the author is answered");
        let told = frames(&mine);
        s.tell_waiting();
        assert_eq!(
            frames(&mine),
            told,
            "the author's page was told with the commit"
        );
        let set = sets_of(&s, &theirs).pop().expect("a patch set");
        assert!(
            format!("{set:?}").contains("Seen by everyone"),
            "the other reader's timeline shows the post: {set:?}"
        );
    }

    /// **A burst of posts tells another reader once** (ADR-0271): every
    /// commit waiting when a connection tells is told together, and the
    /// other session's open timeline is read again once, showing each post.
    /// Until ADR-0271 it was read again for each: a render per post per
    /// reader, which under a burst kept the reader's own read waiting.
    #[test]
    fn a_burst_of_posts_tells_another_reader_once() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        let before = sets_of(&s, &theirs).len();
        for i in 0..5 {
            let answered = s
                .command_answered(
                    "feed.app.post",
                    "a",
                    &[Val::String(format!("Burst {i}"))],
                    Some(&format!("i-{i}")),
                )
                .expect("runs");
            assert!(answered.committed, "{:?}", answered.result);
        }
        s.tell_waiting();
        assert_eq!(
            sets_of(&s, &theirs).len(),
            before + 1,
            "one patch set, for five posts"
        );
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        for i in 0..5 {
            assert!(set.contains(&format!("Burst {i}")), "post {i}: {set}");
        }
        // And the reader is told of the next, its telling let go of.
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("After the burst".into())],
            Some("i-after"),
        )
        .expect("runs");
        s.tell_waiting();
        assert_eq!(sets_of(&s, &theirs).len(), before + 2);
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("After the burst"), "{set}");
    }

    /// **A commit that comes while a reader is told is told after it**
    /// (ADR-0271): its telling, finding the reader being told, asks for one
    /// more and returns; the one more reads the latest. Without it, the post
    /// would wait for the next commit to be seen.
    #[test]
    fn a_commit_while_a_reader_is_told_is_told_after_it() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        *s.told_rendered.lock().expect("told rendered") = Some(Box::new(|s: &Server| {
            s.command_answered(
                "feed.app.post",
                "a",
                &[Val::String("Late".into())],
                Some("i-2"),
            )
            .expect("runs");
            // `b` is being told: this asks for one more, and returns.
            s.tell_waiting();
        }));
        s.tell_waiting();
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("Late"), "the late post is told: {set}");
    }

    /// The table's freedom while each derive ran, as a hook records it.
    fn record_freedom(s: &Server) -> Arc<Mutex<Vec<bool>>> {
        let free = Arc::new(Mutex::new(Vec::new()));
        let seen = free.clone();
        *s.deriving_hook.lock().expect("deriving") = Some(Arc::new(move |s: &Server, _: &Doc| {
            seen.lock()
                .expect("seen")
                .push(s.pending.try_lock().is_ok());
        }));
        free
    }

    /// **The table is free while a telling derives** (ADR-0296). Each
    /// commit's telling derived every document of a session inside the table,
    /// 50 to 200 ms a telling under parallel load, and every stream and read
    /// on the host waited for it: WebKit's "Load more" never got its rows.
    #[test]
    fn the_table_is_free_while_a_telling_derives() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let free = record_freedom(&s);
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let free = free.lock().expect("free").clone();
        assert!(!free.is_empty() && free.iter().all(|f| *f), "{free:?}");
    }

    /// **The table is free while a keyed read derives** (ADR-0296): "Load
    /// more"'s own read, applied.
    #[test]
    fn the_table_is_free_while_a_keyed_read_derives() {
        let s = served_feed();
        for i in 0..21 {
            s.command_answered(
                "feed.app.post",
                "b",
                &[Val::String(format!("Post {i}"))],
                Some(&format!("i-{i}")),
            )
            .expect("runs");
        }
        let (_, cursor, _, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let free = record_freedom(&s);
        let more = BTreeMap::from([("shown".to_string(), serde_json::json!(40))]);
        let read = s.read_keyed("a", "feed", 1, cursor, &more, STAYED);
        assert!(matches!(read, Ok(KeyOutcome::Applied)), "applied");
        let free = free.lock().expect("free").clone();
        assert!(!free.is_empty() && free.iter().all(|f| *f), "{free:?}");
    }

    /// Replaces what `target` shows, whole, at each of its first `times`
    /// derives, as a change applied meanwhile would; counts its derives.
    fn replace_while_derived(
        s: &Server,
        target: &Doc,
        times: usize,
    ) -> Arc<std::sync::atomic::AtomicUsize> {
        let derived = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = derived.clone();
        let target = target.clone();
        *s.deriving_hook.lock().expect("deriving") =
            Some(Arc::new(move |s: &Server, doc: &Doc| {
                if *doc != target {
                    return;
                }
                if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < times {
                    let mut shown = s.shown.lock().expect("shown");
                    let was = shown.get(doc).cloned().expect("shown");
                    shown.insert(doc.clone(), Arc::new((*was).clone()));
                }
            }));
        derived
    }

    /// **A document changed while its change is derived is derived against
    /// again** (ADR-0296): a change derived against what it no longer shows
    /// is not pushed.
    #[test]
    fn a_document_changed_while_its_change_is_derived_is_derived_against_again() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        let derived = replace_while_derived(&s, &theirs, 1);
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        assert_eq!(derived.load(std::sync::atomic::Ordering::SeqCst), 2);
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("First"), "told: {set}");
    }

    /// **A keyed read whose document changed while it derived is derived
    /// against again** (ADR-0296): "Load more"'s rows are applied over what
    /// the page shows, never over what it showed.
    #[test]
    fn a_keyed_read_whose_document_changed_while_it_derived_is_derived_again() {
        let s = served_feed();
        for i in 0..21 {
            s.command_answered(
                "feed.app.post",
                "b",
                &[Val::String(format!("Post {i}"))],
                Some(&format!("i-{i}")),
            )
            .expect("runs");
        }
        let (_, cursor, _, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let derived = replace_while_derived(&s, &doc, 1);
        let more = BTreeMap::from([("shown".to_string(), serde_json::json!(40))]);
        let read = s.read_keyed("a", "feed", 1, cursor, &more, STAYED);
        assert!(matches!(read, Ok(KeyOutcome::Applied)), "applied");
        assert_eq!(
            derived.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "derived against what it shows, again"
        );
    }

    /// **A document changed at every attempt is still told** (ADR-0296): the
    /// last attempt is derived inside the table, where nothing reaches it.
    #[test]
    fn a_document_changed_at_every_attempt_is_still_told() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        let derived = replace_while_derived(&s, &theirs, usize::MAX);
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        assert_eq!(
            derived.load(std::sync::atomic::Ordering::SeqCst),
            DOCUMENT_ATTEMPTS as usize - 1,
            "each attempt but the last derived outside"
        );
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("First"), "told: {set}");
    }

    /// When a document was told, from its trail: the milliseconds of its
    /// first telling's note.
    fn told_ms(s: &Server, doc: &Doc) -> u128 {
        s.pending.lock().expect("pending")[doc]
            .trail
            .iter()
            .find(|note| note.contains(" told at "))
            .and_then(|note| note.split(" ms ").next()?.parse().ok())
            .unwrap_or_else(|| panic!("{doc:?} was not told"))
    }

    /// **A reader is told while another's telling takes its time**
    /// (ADR-0297): a commit tells the sessions that read what it dropped at
    /// once. One after another, the second waited for the first: on CI a
    /// reader's change waited behind 47 tellings.
    #[test]
    fn a_reader_is_told_while_another_readers_telling_takes_its_time() {
        let s = served_feed();
        for session in ["a", "b", "c"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let (slow, quick) = {
            let pending = s.pending.lock().expect("pending");
            (latest(&pending, "b"), latest(&pending, "c"))
        };
        // "b" takes its time: its document is derived only once "c" has been
        // told (the trail says so), or ten seconds have passed. A delay of
        // 400 ms was a silence "c"'s telling could be outwaited by on a slow
        // machine, and the order then said nothing of the tellers.
        let (target, quick_doc) = (slow.clone(), quick.clone());
        *s.deriving_hook.lock().expect("deriving") =
            Some(Arc::new(move |s: &Server, doc: &Doc| {
                if *doc == target {
                    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while std::time::Instant::now() < until
                        && !s.pending.lock().expect("pending")[&quick_doc]
                            .trail
                            .iter()
                            .any(|note| note.contains(" told at "))
                    {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }));
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        // "b" comes first in the order, and takes its time: "c" was told
        // while it did (the milliseconds a note carries may tie).
        assert!(
            told_ms(&s, &quick) <= told_ms(&s, &slow),
            "c told at {} ms, b at {} ms",
            told_ms(&s, &quick),
            told_ms(&s, &slow)
        );
        let set = format!("{:?}", sets_of(&s, &quick).pop().expect("a patch set"));
        assert!(set.contains("First"), "told: {set}");
    }

    /// **A document no page asks for is told when one asks** (ADR-0297):
    /// a telling passes it by, and its page, asking again, is given what
    /// changed, derived against what it shows.
    #[test]
    fn a_document_no_page_asks_for_is_told_when_one_asks() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        s.pending
            .lock()
            .expect("pending")
            .get_mut(&theirs)
            .expect("subscribed")
            .seen = std::time::Instant::now()
            .checked_sub(LIVE + std::time::Duration::from_millis(1))
            .expect("an instant");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        assert!(
            sets_of(&s, &theirs).is_empty(),
            "passed by: nothing derived for it"
        );
        // Its page asks, as a page does, and is answered what changed.
        let asked = get_as(&s, "b", &format!("/stream?doc={0}&since={0}", theirs.1));
        assert!(asked.contains("First"), "told as it asks: {asked}");
        // Asked again, nothing more: it was told once.
        s.told_as_it_asks(&theirs);
        assert_eq!(sets_of(&s, &theirs).len(), 1);
    }

    /// **A test's page keeps asking, however long the test takes** (the
    /// premise of every test that serves a document and later commits):
    /// [`LIVE`] is three seconds of the machine's wall clock, which a test
    /// of a slow machine outlasts between its serve and its commit, so a
    /// test build dates the ask an hour on (`Subscriber::asked_at`). The
    /// window itself is tested by setting `seen` to the instant meant.
    #[test]
    fn a_page_served_is_asking_for_as_long_as_a_test_takes() {
        let s = served_feed();
        s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let later = std::time::Instant::now() + 100 * LIVE;
        assert!(
            s.pending.lock().expect("pending")[&doc].live(later),
            "a page served, {:?} on",
            100 * LIVE
        );
        // As a page subscribed is.
        let subscribed: Doc = ("a".to_string(), 9_999);
        s.subscribed(&subscribed);
        assert!(
            s.pending.lock().expect("pending")[&subscribed].live(later),
            "a page subscribed"
        );
        // And one aged to the instant the window is tested at is not.
        let mut aged = Subscriber::at(1);
        aged.seen = later - LIVE - std::time::Duration::from_millis(1);
        assert!(!aged.live(later), "a page that has not asked for LIVE");
    }

    /// **A host serves the build it was given, by its name** (ADR-0300): on
    /// each document it answers (`pw-build`) and in the document's resume
    /// manifest, beside its page's document schema and scope; and in its
    /// handler table, with each page's.
    #[test]
    fn a_host_serves_its_build_by_its_name() {
        let s = served_feed();
        assert!(
            s.build_id.len() == 17 && s.build_id.starts_with('b'),
            "{}",
            s.build_id
        );
        let home = s
            .templates
            .iter()
            .find(|t| t.path == "feed.app.Home~ui")
            .expect("the home page");
        let table = get_as(&s, "a", "/pw-handlers");
        assert!(
            table.contains(&format!("pw-build: {}\r\n", s.build_id)),
            "{table}"
        );
        assert!(
            table.contains(&format!("#build|{}\n", s.build_id)),
            "{table}"
        );
        assert!(
            table.contains(&format!(
                "#page|feed.app.Home~ui|{}|session:\n",
                home.schema
            )),
            "{table}"
        );
        let page = get_as(&s, "a", "/");
        assert!(
            page.contains(&format!("pw-build: {}\r\n", s.build_id)),
            "{page}"
        );
        let manifest = manifest_of(page.split_once("\r\n\r\n").map_or("", |(_, b)| b));
        assert_eq!(manifest["resume"]["build"], s.build_id.as_str());
        assert_eq!(manifest["resume"]["document"], home.schema.as_str());
        assert_eq!(manifest["resume"]["scope"], "session:");
    }

    /// **A build that names no build is not served** (ADR-0300): it was
    /// written before builds were named, and is built again.
    #[test]
    fn a_build_that_names_no_build_is_not_served() {
        let (_dir, out) = built_feed_with(|app| app.to_string());
        std::fs::remove_file(out.join("build-id")).expect("named");
        match Server::from_build(out.clone(), out) {
            Ok(_) => panic!("served a build that names none"),
            Err(why) => assert!(why.contains("names no build"), "{why}"),
        }
    }

    /// One GET of `path` by `session`, through the server's own handling,
    /// and the whole answer.
    fn get_as(s: &Server, session: &str, path: &str) -> String {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let at = listener.local_addr().expect("address");
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let (stream, _) = listener.accept().expect("accept");
                handle(s, stream);
            });
            let mut client = std::net::TcpStream::connect(at).expect("connect");
            client
                .write_all(
                    format!(
                        "GET {path} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .expect("request");
            let mut answer = String::new();
            let _ = std::io::Read::read_to_string(&mut client, &mut answer);
            answer
        })
    }

    /// **A document's trail says what happened to its frames** (ADR-0296):
    /// each frame pushed, and the telling that pushed it, kept with the
    /// document and answered to its own session alone (`/bench/records`).
    #[test]
    fn a_documents_trail_says_what_happened_to_its_frames() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let trail = s.pending.lock().expect("pending")[&theirs]
            .trail
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        for said in [
            "subscribed at",
            "told at",
            "pushed a change as",
            "pushed a patch set as",
        ] {
            assert!(trail.contains(said), "{said}: {trail}");
        }
        // Over HTTP: the session's own document's, and no other's.
        let asked = |session: &str| {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
            let at = listener.local_addr().expect("address");
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let (stream, _) = listener.accept().expect("accept");
                    handle(&s, stream);
                });
                let mut client = std::net::TcpStream::connect(at).expect("connect");
                client
                    .write_all(
                        format!(
                            "GET /bench/records?doc={} HTTP/1.1\r\nHost: t\r\n\
                             Cookie: pw-session={session}\r\n\r\n",
                            theirs.1
                        )
                        .as_bytes(),
                    )
                    .expect("request");
                let mut answer = String::new();
                let _ = std::io::Read::read_to_string(&mut client, &mut answer);
                answer
            })
        };
        assert!(
            asked("b").contains("pushed a patch set as"),
            "{}",
            asked("b")
        );
        assert!(!asked("a").contains("pushed"), "{}", asked("a"));
    }

    /// **A telling that panics does not silence the reader** (ADR-0271): the
    /// next commit tells it, as one more would have.
    #[test]
    fn a_telling_that_panics_does_not_silence_the_reader() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("First".into())],
            Some("i-1"),
        )
        .expect("runs");
        *s.told_rendered.lock().expect("told rendered") =
            Some(Box::new(|_: &Server| panic!("a telling that fails")));
        let told = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.tell_waiting()));
        assert!(told.is_err(), "the telling panicked");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("Next".into())],
            Some("i-2"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("Next"), "told after the panic: {set}");
    }

    /// **A session's turns are served in the order they are asked**
    /// (ADR-0271): each waits for those asked before it, and none asked
    /// after it comes first. `std`'s mutex promises no order, and a session
    /// told again and again took its lock before the session's own read.
    #[test]
    fn a_sessions_turns_are_served_in_the_order_asked() {
        let turns = Arc::new(Turns::default());
        let asked = |n: u64| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while turns.tickets.lock().expect("tickets").0 < n {
                assert!(
                    std::time::Instant::now() < deadline,
                    "ticket {n} never asked"
                );
                std::thread::yield_now();
            }
        };
        let served = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            let first = turns.lock().unwrap();
            for i in 0..8u64 {
                let (turns, served) = (&turns, &served);
                scope.spawn(move || {
                    let _turn = turns.lock().unwrap();
                    served.lock().expect("served").push(i);
                });
                // The next asks only once this one has its ticket.
                asked(i + 2);
            }
            drop(first);
        });
        assert_eq!(*served.lock().expect("served"), (0..8).collect::<Vec<_>>());
        // And a turn that panics serves the next. Each on a thread of its
        // own, waited for no longer than it should take: a turn that served
        // no one would leave the next waiting for ever.
        let panicking = Arc::clone(&turns);
        let held = std::thread::spawn(move || {
            let _turn = panicking.lock().unwrap();
            panic!("a telling that fails");
        })
        .join();
        assert!(held.is_err());
        let (sent, served_next) = std::sync::mpsc::channel();
        let next = Arc::clone(&turns);
        std::thread::spawn(move || {
            let _turn = next.lock().unwrap();
            let _ = sent.send(());
        });
        assert!(
            served_next
                .recv_timeout(std::time::Duration::from_secs(10))
                .is_ok(),
            "the next is served after a panic"
        );
    }

    /// **The feed's page, opening its stream, is not told to reload**
    /// (ADR-0220). A stream's request drains the session first, and the
    /// drain regenerated the store's cart for every program: on the feed's
    /// page, which has no cart, it read the store's `cart.line_count`, found
    /// no such part, and told the page to read itself again, which it did,
    /// for ever.
    #[test]
    fn the_feeds_page_opening_its_stream_is_not_told_to_reload() {
        let s = served_feed();
        s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let polled = fetched_as(
            &s,
            &format!("/stream?doc={}&since={}", doc.1, doc.1),
            Some("a"),
        )
        .into_iter()
        .map(|(_, c)| c)
        .collect::<String>();
        assert!(polled.starts_with("HTTP/1.1 200"), "{polled}");
        assert!(!polled.contains("reload"), "{polled}");
    }

    /// **A page carries its data layer's style, and no other's** (ADR-0220):
    /// the store's menu's containment (ADR-0187) on the store's page, and
    /// nothing on the feed's, which has no menu.
    #[test]
    fn a_page_carries_its_data_layers_style_and_no_other() {
        let feed = served_feed();
        let home = fetched_as(&feed, "/", Some("a"))
            .into_iter()
            .map(|(_, c)| c)
            .collect::<String>();
        assert!(home.contains("<title>Home</title>"), "{home}");
        assert!(!home.contains("<style>"), "{home}");
        let store = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = fetched_as(&store, "/StorePage.html", Some("a"))
            .into_iter()
            .map(|(_, c)| c)
            .collect::<String>();
        assert!(page.contains(&format!("<style>{STYLE}</style>")), "{page}");
    }

    /// **A guest's post names the guest to every reader** (ADR-0220): a
    /// session that signed up as no one posts as a guest named for it. Until
    /// ADR-0220 every reader read "You".
    #[test]
    fn a_guests_post_names_the_guest_to_every_reader() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("From a guest".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
        assert!(set.contains("Guest a"), "{set}");
        assert!(!set.contains("You"), "{set}");
    }

    /// **A post sent over HTTP reaches another reader once it is answered**
    /// (ADR-0219): the connection that committed tells the others after its
    /// answer is written and it is closed.
    #[test]
    fn a_post_over_http_reaches_another_reader_after_its_answer() {
        let s = served_feed();
        for session in ["a", "b"] {
            s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
        }
        let theirs = latest(&s.pending.lock().expect("pending"), "b");
        let answer = posted(
            &s,
            "/command/feed.app.post",
            "a",
            "i-1",
            "[\"Over the wire\"]",
        );
        assert!(answer.starts_with("HTTP/1.1 202"), "{answer}");
        assert!(answer.contains("\"committed\":true"), "{answer}");
        let set = sets_of(&s, &theirs).pop().expect("a patch set");
        assert!(
            format!("{set:?}").contains("Over the wire"),
            "the other reader's timeline shows the post: {set:?}"
        );
    }

    /// The `entry_value` frames a document was sent: each entry, value,
    /// version and the presses it names.
    fn entry_values(
        s: &Server,
        doc: &Doc,
    ) -> Vec<(ResourceEntryId, serde_json::Value, Version, Vec<String>)> {
        s.pending.lock().expect("pending")[doc]
            .frames
            .iter()
            .filter_map(|(_, f)| match f {
                StreamFrame::EntryValue {
                    entry,
                    version,
                    value,
                    applied,
                    ..
                } => Some((entry.clone(), value.clone(), *version, applied.clone())),
                _ => None,
            })
            .collect()
    }

    /// **A thread a page speculates on is an entry of its own** (ADR-0236,
    /// ruling 0122-d): the thread page binds `Thread(id)`, and its form passes
    /// `id` to `reply`, so the page speculates on the thread it shows. Two
    /// threads one session has open are two entries, each named by its `id`.
    /// A reply sends its thread's new value, as its nodes, to that thread's
    /// entry alone. Until ADR-0236 an entry was named by the session, the page
    /// and the binding, and two open threads would have been one.
    #[test]
    fn a_speculated_thread_is_named_by_its_page_parameter() {
        let s = served_feed();
        let open = |id: &str| {
            let thread = Params::from([("id".to_string(), id.to_string())]);
            let (_, _, entries, _) = s
                .serve_document_settled("a", "feed.app.PostPage~ui", &thread, &[])
                .expect("served");
            let doc = latest(&s.pending.lock().expect("pending"), "a");
            (entries["thread"]["entry"].clone(), doc)
        };
        let (p1, p1_doc) = open("p1");
        let (p3, p3_doc) = open("p3");
        assert_eq!(
            p1,
            serde_json::json!(speculated_entry(
                "a",
                "feed.app.PostPage~ui",
                "thread",
                &["p1".to_string()]
            ))
        );
        assert_ne!(p1, p3, "two threads, two entries");
        s.command_answered(
            "feed.app.reply",
            "a",
            &[Val::String("p3".into()), Val::String("Mine".into())],
            Some("i-1"),
        )
        .expect("runs");
        // p3's document is sent p3's new value, at p3's entry, as its nodes.
        let sent = entry_values(&s, &p3_doc);
        let Some((_, _, version, _)) = sent.iter().find(|(entry, value, ..)| {
            serde_json::json!(entry) == p3 && value["$graph"].to_string().contains("Mine")
        }) else {
            panic!("{sent:?}");
        };
        // And the commit's answer names that entry at that version, which the
        // page waits for before it lets the speculation go.
        let basis = s.committed_basis("a");
        assert!(
            basis
                .as_array()
                .into_iter()
                .flatten()
                .any(|b| b["entry"] == p3 && b["version"] == serde_json::json!(version)),
            "{basis}"
        );
        // p1's is sent nothing at p3's entry.
        assert!(
            entry_values(&s, &p1_doc)
                .iter()
                .all(|(entry, ..)| serde_json::json!(entry) != p3),
            "p1's document"
        );
    }

    /// **A document that speculates carries its page's parameters**
    /// (ADR-0236): a region a speculation renders again reads them, as the
    /// host does. The thread page's carries its `id`; the home page's, which
    /// takes none, carries none.
    #[test]
    fn a_speculating_document_carries_its_parameters() {
        let s = served_feed();
        let manifest = |path: &str| {
            manifest_of(
                &fetched_as(&s, path, Some("a"))
                    .into_iter()
                    .map(|(_, c)| c)
                    .collect::<String>(),
            )
        };
        assert_eq!(
            manifest("/post/p1")["params"],
            serde_json::json!({ "id": "p1" })
        );
        assert_eq!(manifest("/")["params"], serde_json::json!({}));
    }

    /// **A post is shown before the server answers, and the page is told
    /// when the value it holds includes it** (ADR-0222). The feed's home page
    /// speculates on its timeline: it holds the timeline's value, a post
    /// sends the new one with the press it includes, and the post's answer
    /// names that value's version. Until ADR-0222 a page held the store's
    /// cart alone, and the feed's could not speculate.
    #[test]
    fn a_post_is_shown_before_the_server_answers_and_the_page_told_when_it_holds_it() {
        let s = served_feed();
        let (_, _, entries, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let feed = &entries["feed"];
        assert_eq!(
            feed["entry"],
            serde_json::json!(speculated_entry("a", "feed.app.Home~ui", "feed", &[]))
        );
        assert_eq!(feed["value"].as_array().map(Vec::len), Some(1), "{feed}");
        let before = feed["version"].as_u64().expect("a version");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command_answered(
            "feed.app.post",
            "a",
            &[Val::String("Before the answer".into())],
            Some("i-7"),
        )
        .expect("runs");
        let sent = entry_values(&s, &doc);
        let [(entry, value, version, applied)] = sent.as_slice() else {
            panic!("one value sent: {sent:?}");
        };
        assert_eq!(serde_json::json!(entry), feed["entry"]);
        assert!(version.0 > before, "a newer version than the page holds");
        assert_eq!(value[0]["text"], "Before the answer", "{value}");
        assert!(
            applied.contains(&"i-7".to_string()),
            "it names the press it includes: {applied:?}"
        );
        assert_eq!(
            s.committed_basis("a"),
            serde_json::json!([{ "entry": entry, "version": version }]),
            "the answer names the version that includes it"
        );
    }

    /// **A value a page speculates on is sent when it changes, and only
    /// then** (ADR-0222): a like of a reply, which no timeline shows, reads
    /// the timeline again and finds it as it was.
    #[test]
    fn a_speculated_value_that_did_not_change_is_not_sent_again() {
        let s = served_feed();
        s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command_answered(
            "feed.app.like",
            "a",
            &[Val::String("p2".into())],
            Some("i-1"),
        )
        .expect("runs");
        assert_eq!(entry_values(&s, &doc).len(), 0, "the timeline is as it was");
        // Control: a like of a post the timeline shows.
        s.command_answered(
            "feed.app.like",
            "a",
            &[Val::String("p1".into())],
            Some("i-2"),
        )
        .expect("runs");
        assert_eq!(entry_values(&s, &doc).len(), 1, "the timeline changed");
    }

    /// **A page that reads more holds what it shows** (ADR-0222): "Load
    /// more" reads the timeline again for a longer page, and the value the
    /// page speculates on is the longer one, or a post pressed after it would
    /// be shown over the shorter list.
    #[test]
    fn a_page_that_reads_more_holds_what_it_shows() {
        let s = served_feed();
        // One more than a page shows, by another session.
        for i in 0..21 {
            s.command_answered(
                "feed.app.post",
                "b",
                &[Val::String(format!("Post {i}"))],
                Some(&format!("i-{i}")),
            )
            .expect("runs");
        }
        let (_, cursor, entries, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        assert_eq!(entries["feed"]["value"].as_array().map(Vec::len), Some(20));
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let more = BTreeMap::from([("shown".to_string(), serde_json::json!(40))]);
        let read = s.read_keyed("a", "feed", 1, cursor, &more, STAYED);
        assert!(matches!(read, Ok(KeyOutcome::Applied)), "applied");
        let sent = entry_values(&s, &doc);
        let [(_, value, ..)] = sent.as_slice() else {
            panic!("one value sent: {sent:?}");
        };
        assert_eq!(value.as_array().map(Vec::len), Some(22), "{value}");
    }

    /// **A longer read is not applied over a commit it did not see**
    /// (ADR-0224). "Load more" reads the timeline for a longer page; the
    /// session posts between that read and its apply, and its change is sent
    /// first. Applied as read, the longer page would be the one without the
    /// post, and the page would show it so until the next change.
    #[test]
    fn a_longer_read_is_not_applied_over_a_commit_it_did_not_see() {
        let s = served_feed();
        for i in 0..21 {
            s.command_answered(
                "feed.app.post",
                "b",
                &[Val::String(format!("Post {i}"))],
                Some(&format!("i-{i}")),
            )
            .expect("runs");
        }
        let (_, cursor, _, _) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        *s.keyed_fetched.lock().expect("between") = Some(Box::new(|s: &Server| {
            s.command_answered(
                "feed.app.post",
                "a",
                &[Val::String("Between the read and its apply".into())],
                Some("i-between"),
            )
            .expect("runs");
        }));
        let more = BTreeMap::from([("shown".to_string(), serde_json::json!(40))]);
        let read = s.read_keyed("a", "feed", 1, cursor, &more, STAYED);
        assert!(matches!(read, Ok(KeyOutcome::Applied)), "applied");
        let shown = s.shown.lock().expect("shown")[&doc].speculated["feed"].clone();
        assert_eq!(shown.as_array().map(Vec::len), Some(23), "{shown}");
        assert_eq!(
            shown[0]["text"], "Between the read and its apply",
            "{shown}"
        );
    }

    /// **A post's text is held to its length where it arrives** (ADR-0225):
    /// `PostText` is from 1 to 280 code points, and a browser's request is a
    /// claim, so the host refuses an empty one and a long one before the
    /// command runs, by name. 280 emoji are 280 code points, whatever their
    /// bytes, and are taken.
    #[test]
    fn a_posts_text_is_held_to_its_length_where_it_arrives() {
        let s = served_feed();
        let posts = |s: &Server| {
            s.answer(
                "feed.app.Timeline",
                "a",
                &[Val::String("a".into()), Val::S64(100)],
            )
        };
        let before = format!("{:?}", posts(&s));
        for (text, said) in [
            (String::new(), "0 code points long"),
            ("x".repeat(281), "281 code points long"),
        ] {
            let refused = s
                .command_json(
                    "feed.app.post",
                    "a",
                    &[serde_json::json!(text)],
                    Some("i-no"),
                )
                .expect_err("refused before it runs");
            assert!(
                refused.contains(said) && refused.contains("feed.app.PostText"),
                "{refused}"
            );
        }
        assert_eq!(format!("{:?}", posts(&s)), before, "nothing was posted");
        s.command_json(
            "feed.app.post",
            "a",
            &[serde_json::json!("\u{1F600}".repeat(280))],
            Some("i-yes"),
        )
        .expect("well-formed")
        .expect("committed");
    }

    /// **A page that reads nothing a commit dropped is not read again**
    /// (ADR-0219): with `Thread` listening for likes alone, a post leaves
    /// another session's open thread as it was, and it is sent nothing.
    #[test]
    fn a_page_reading_nothing_dropped_is_told_nothing() {
        // And with no reply speculated on it, which PW5107 would refuse of a
        // `Thread` that does not listen for posts (ADR-0236).
        let s = served_feed_with(|app| {
            app.replace(
                "invalidates_on Liked(id), Posted(_)",
                "invalidates_on Liked(id)",
            )
            .replace(
                "    optimistic    Thread(to) as thread => replied(thread, text)\n",
                "",
            )
        });
        let thread = Params::from([("id".to_string(), "p1".to_string())]);
        s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        s.serve_document_settled("c", "feed.app.PostPage~ui", &thread, &[])
            .expect("served");
        let theirs = latest(&s.pending.lock().expect("pending"), "c");
        let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
        let before = frames(&theirs);
        let answered = s
            .command_answered(
                "feed.app.post",
                "a",
                &[Val::String("Not in the thread".into())],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        s.tell_waiting();
        assert_eq!(frames(&theirs), before, "the thread's page is sent nothing");
    }

    /// **A value the template computes is the host's, and sent when it
    /// changes** (ADR-0226). The thread page's counts are
    /// `counted(List.length(thread.replies), ..)` and `counted(thread.likes,
    /// ..)`: each a function the compiler lifts out of the template, which
    /// the host runs with the thread. A like of the post is a new thread, so
    /// a new count of likes, and the page is sent that text alone. Until
    /// ADR-0226 a computed hole was refused at build.
    #[test]
    fn a_value_the_template_computes_is_the_hosts_and_sent_when_it_changes() {
        let s = served_feed();
        let shown = |html: &str| {
            let at = html.find("<p id=\"counts\">").expect("the counts");
            let end = at + html[at..].find("</p>").expect("their end");
            let mut text = String::new();
            let mut rest = &html[at + "<p id=\"counts\">".len()..end];
            while let Some(open) = rest.find("<!--") {
                text.push_str(&rest[..open]);
                rest = &rest[open + rest[open..].find("-->").expect("a comment's end") + 3..];
            }
            text + rest
        };
        for (id, counts) in [
            ("p1", "1 reply · 2 likes"),
            ("p2", "1 reply · 0 likes"),
            ("p3", "0 replies · 1 like"),
        ] {
            let thread = Params::from([("id".to_string(), id.to_string())]);
            let (html, ..) = s
                .serve_document_settled("a", "feed.app.PostPage~ui", &thread, &[])
                .expect("served");
            assert_eq!(shown(&html), counts, "{id}: {html}");
        }
        let thread = Params::from([("id".to_string(), "p1".to_string())]);
        s.serve_document_settled("c", "feed.app.PostPage~ui", &thread, &[])
            .expect("served");
        let theirs = latest(&s.pending.lock().expect("pending"), "c");
        let answered = s
            .command_answered(
                "feed.app.like",
                "b",
                &[Val::String("p1".into())],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        s.tell_waiting();
        let set = sets_of(&s, &theirs).pop().expect("a patch set");
        let texts: Vec<String> = set
            .patches
            .iter()
            .filter_map(|p| match &p.operation {
                PatchOp::ReplaceText { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["3 likes"], "the count that changed, alone: {set:?}");
    }

    /// **An attribute's computed value is the host's too** (ADR-0226): the
    /// thread page's counts given a `title` computed from the thread, and
    /// a boolean attribute. Each is written when the page renders, and set
    /// again when a like makes the thread a new value.
    #[test]
    fn an_attributes_computed_value_is_the_hosts_and_set_again() {
        // With nothing speculated on the thread, neither a reply nor a like
        // (ADR-0238): an attribute computed from a speculated value is
        // refused (ADR-0235), and this is the host's.
        let s = served_feed_with(|app| {
            app.replace(
                "<p id=\"counts\">",
                "<p id=\"counts\" title={counted(thread.likes, \"like\", \"likes\")} \
                 hidden={List.length(thread.replies) == 0}>",
            )
            .replace(
                "    optimistic    Thread(to) as thread => replied(thread, text)\n",
                "",
            )
            .replace(
                "liked(feed, post),\n        Thread(post) as thread => liked_thread(thread, post)\n",
                "liked(feed, post)\n",
            )
        });
        let thread = |id: &str| Params::from([("id".to_string(), id.to_string())]);
        let (html, ..) = s
            .serve_document_settled("a", "feed.app.PostPage~ui", &thread("p1"), &[])
            .expect("served");
        assert!(html.contains("title=\"2 likes\""), "{html}");
        assert!(!html.contains(" hidden"), "a thread with a reply: {html}");
        let (html, ..) = s
            .serve_document_settled("a", "feed.app.PostPage~ui", &thread("p3"), &[])
            .expect("served");
        assert!(html.contains("title=\"1 like\""), "{html}");
        assert!(html.contains(" hidden"), "a thread with no reply: {html}");
        s.serve_document_settled("c", "feed.app.PostPage~ui", &thread("p1"), &[])
            .expect("served");
        let theirs = latest(&s.pending.lock().expect("pending"), "c");
        s.command_answered(
            "feed.app.like",
            "b",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = sets_of(&s, &theirs).pop().expect("a patch set");
        let titles: Vec<String> = set
            .patches
            .iter()
            .filter_map(|p| match &p.operation {
                PatchOp::SetAttribute { name, value } if name == "title" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["3 likes"], "{set:?}");
    }

    /// The text of the element `open` starts, its comments left out.
    fn text_in(html: &str, open: &str) -> String {
        let at = html
            .find(open)
            .unwrap_or_else(|| panic!("no `{open}` in {html}"));
        let rest = &html[at + open.len()..];
        let mut rest = &rest[..rest.find("</").expect("its end")];
        let mut text = String::new();
        while let Some(start) = rest.find("<!--") {
            text.push_str(&rest[..start]);
            rest = &rest[start + rest[start..].find("-->").expect("a comment's end") + 3..];
        }
        text + rest
    }

    /// **What a page computes from a signal is rendered at its first value**
    /// (ADR-0227). The feed's draft says what is left of it, and its post
    /// button is disabled while there is no post to send: the host renders
    /// each from the draft's first value, by the component the browser's
    /// module was compiled beside, and the browser computes each from then
    /// on. The module is served to a page that names it, and no other.
    #[test]
    fn what_a_page_computes_from_a_signal_is_rendered_at_its_first_value() {
        let home = |s: &Server| {
            let computed = s.with_computed_signals(
                Env::new(),
                s.plan_of("feed.app.Home~ui"),
                s.template_of("feed.app.Home~ui"),
            );
            assert!(
                computed.is_ok(),
                "each live subject has a computed first value: {:?}",
                computed.as_ref().err()
            );
            let result = s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[]);
            assert!(
                result.is_ok(),
                "initial computed values render: {:?}",
                result.as_ref().err()
            );
            result.unwrap().0
        };
        let s = served_feed();
        let html = home(&s);
        assert_eq!(text_in(&html, "<p id=\"left\">"), "280 left");
        assert!(html.contains("type=\"submit\" disabled>Post"), "{html}");
        let body = |path: &str| -> String {
            fetched_as(&s, path, Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        // The document names its module, which the browser loads when the
        // draft first changes.
        let document = body("/");
        assert!(
            document.contains("\"computed\":\"/computed/feed.app.Home~ui.mjs\""),
            "{document}"
        );
        let module = body("/computed/feed.app.Home~ui.mjs");
        assert!(
            module.starts_with("HTTP/1.1 200") && module.contains("export const parts"),
            "{module}"
        );
        // And the thread page's, since its reply form (ADR-0231). No path a
        // plan does not name is read, one that leaves the directory least of
        // all.
        assert!(
            body("/computed/feed.app.PostPage~ui.mjs").contains("export const parts"),
            "the thread page's module"
        );
        assert!(
            !body("/computed/feed.app.Nowhere.mjs").contains("export const parts"),
            "a module no page names"
        );
        assert!(
            !body("/computed/../contracts.json").contains("component_id"),
            "a file outside the modules"
        );
        // From another first value, another first render.
        let s = served_feed_with(|app| {
            app.replace(
                "signal draft: String = \"\"",
                "signal draft: String = \"Hello\"",
            )
        });
        let html = home(&s);
        assert_eq!(text_in(&html, "<p id=\"left\">"), "275 left");
        assert!(html.contains("type=\"submit\">Post"), "{html}");
    }

    /// **A page of signals alone computes from them too** (ADR-0227), at
    /// their first values, where the route that renders such a page renders
    /// it: an `Int`'s, and a view's given a field of a record's.
    #[test]
    fn a_page_of_signals_alone_computes_at_their_first_values() {
        let s = served_feed_with(|app| {
            format!(
                "{app}\ntype Panel = Panel {{ open: Bool }}\n\n\
                 fn said(open: Bool) -> String !{{}} {{\n    if open {{\n        \"open\"\n    }} else {{\n        \"shut\"\n    }}\n}}\n\n\
                 view Opened(open: Bool) !{{}} {{\n    <p id=\"opened\">{{said(open)}}</p>\n}}\n\n\
                 page Count() {{\n    route \"/count\"\n    cache private\n\n    \
                 signal n: Int = 3\n    signal panel: Panel = Panel {{ open: true }}\n\n    \
                 view {{\n        <title>Count</title>\n        <main>\n            \
                 <p id=\"twice\">{{n * 2}}</p>\n            <Opened open={{panel.open}} />\n            \
                 <button type=\"button\" on:press={{() => n = n + 1}}>More</button>\n            \
                 <button type=\"button\" on:press={{() => panel = Panel {{ open: false }}}}>Close</button>\n        \
                 </main>\n    }}\n}}\n"
            )
        });
        let page: String = fetched_as(&s, "/count", Some("a"))
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert_eq!(text_in(&page, "<p id=\"twice\">"), "6");
        assert_eq!(text_in(&page, "<p id=\"opened\">"), "open");
        assert!(
            page.contains("\"computed\":\"/computed/feed.app.Count~ui.mjs\""),
            "{page}"
        );
    }

    /// **A value computed in a row is each row's** (ADR-0228): the feed's
    /// timeline says each post's likes in words, computed by the host for
    /// each row from its item. Another session's like is a new row, sent with
    /// its new count.
    #[test]
    fn a_value_computed_in_a_row_is_each_rows_and_sent_with_it() {
        let s = served_feed();
        let (html, ..) = s
            .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        assert_eq!(text_in(&html, "<span class=\"likes\">"), "2 likes");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command_answered(
            "feed.app.like",
            "b",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = sets_of(&s, &doc).pop().expect("a patch set");
        let sent = written(&set);
        assert!(
            sent.contains("3 likes") && !sent.contains("2 likes"),
            "the row again, with its count: {set:?}"
        );
    }

    /// **An opaque value compares as its representation** (ADR-0228), in a
    /// component a host runs: `thread.id == PostId("p1")`, two `PostId`s, as
    /// the feed's own like transition compares them in the browser. Until
    /// ADR-0228 it checked, and did not build.
    #[test]
    fn an_opaque_value_compares_as_its_representation() {
        let s = served_feed_with(|app| {
            app.replace(
                "            <Replies post={thread} />\n",
                "            <Replies post={thread} />\n            \
                 <p id=\"which\">{said(thread.id == PostId(\"p1\"))}</p>\n",
            ) + "\nfn said(first: Bool) -> String !{} {\n    if first {\n        \"the first\"\n    } else {\n        \"a reply\"\n    }\n}\n"
        });
        for (id, said) in [("p1", "the first"), ("p2", "a reply")] {
            let thread = Params::from([("id".to_string(), id.to_string())]);
            let (html, ..) = s
                .serve_document_settled("a", "feed.app.PostPage~ui", &thread, &[])
                .expect("served");
            assert_eq!(text_in(&html, "<p id=\"which\">"), said, "{id}");
        }
    }

    /// **A condition a host computes decides its block, and again when what
    /// it reads changes** (ADR-0229): the thread page's `{#if
    /// List.length(thread.replies) == 0}`, and `{#if thread.likes > 2}`,
    /// which a like makes true, and the block is sent rendered again.
    #[test]
    fn a_condition_a_host_computes_decides_its_block_and_again_when_it_changes() {
        let s = served_feed_with(|app| {
            app.replace(
                "            <Replies post={thread} />\n",
                "            <Replies post={thread} />\n            \
                 {#if thread.likes > 2}<p id=\"popular\">Popular</p>{/if}\n",
            )
        });
        let thread = |id: &str| Params::from([("id".to_string(), id.to_string())]);
        let page = |id: &str| {
            let result = s.serve_document_settled("a", "feed.app.PostPage~ui", &thread(id), &[]);
            assert!(
                result.is_ok(),
                "initial computed subject renders: {:?}",
                result.as_ref().err()
            );
            result.unwrap().0
        };
        assert!(page("p3").contains("<p id=\"quiet\">No replies yet.</p>"));
        let html = page("p1");
        assert!(!html.contains("id=\"quiet\""), "{html}");
        assert!(!html.contains("Popular"), "two likes: {html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command_answered(
            "feed.app.like",
            "b",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = sets_of(&s, &doc).pop().expect("a patch set");
        assert!(
            written(&set).contains("<p id=\"popular\">Popular</p>"),
            "the block rendered again: {set:?}"
        );
    }

    /// **A condition the browser computes is rendered at the signal's first
    /// value** (ADR-0229): the home page's `{#if String.length(draft) > 280}`,
    /// false for an empty draft, and true for one too long from the first.
    #[test]
    fn a_condition_the_browser_computes_is_rendered_at_its_first_value() {
        let home = |s: &Server| {
            let computed = s.with_computed_signals(
                Env::new(),
                s.plan_of("feed.app.Home~ui"),
                s.template_of("feed.app.Home~ui"),
            );
            assert!(
                computed.is_ok(),
                "the browser-computed subject has a first value: {:?}",
                computed.as_ref().err()
            );
            let result = s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[]);
            assert!(
                result.is_ok(),
                "initial browser-computed subject renders: {:?}",
                result.as_ref().err()
            );
            result.unwrap().0
        };
        assert!(!home(&served_feed()).contains("id=\"over\""));
        let s = served_feed_with(|app| {
            app.replace(
                "signal draft: String = \"\"",
                &format!("signal draft: String = \"{}\"", "x".repeat(281)),
            )
        });
        assert!(
            home(&s).contains("<p id=\"over\" role=\"alert\">Too long to post.</p>"),
            "a draft too long from the first"
        );
    }

    /// **A page that binds a query is rendered with its parameters**
    /// (ADR-0231), as one that binds none is (ADR-0130). The thread page's
    /// title, an attribute, a link and a text part read its `id`, and its
    /// reply form's handler captures it. Until 2026-10-05 all but the text
    /// part built, then every request was answered 503, `MissingValue { path:
    /// "id" }`, and the text part was refused at build. A block a like
    /// renders again reads it too.
    #[test]
    fn a_page_that_binds_a_query_is_rendered_with_its_parameters() {
        let s = served_feed_with(|app| {
            app.replace("<title>Post</title>", "<title>Post {id}</title>")
                .replace(
                    "            <Replies post={thread} />\n",
                    "            <Replies post={thread} />\n            \
                     <p id=\"which\" title=\"{id}\"><a id=\"self\" href=\"/post/{id}\">{id}</a></p>\n            \
                     {#if thread.likes > 2}<p id=\"popular\">{id} is popular</p>{/if}\n",
                )
        });
        // As a browser asks for it, the title in its head.
        let html = fetched_as(&s, "/post/p1", Some("a"))
            .into_iter()
            .map(|(_, c)| c)
            .collect::<String>();
        assert!(html.starts_with("HTTP/1.1 200"), "{html}");
        assert!(html.contains("<title>Post p1</title>"), "{html}");
        assert!(html.contains("id=\"which\" title=\"p1\">"), "{html}");
        assert!(html.contains("id=\"self\" href=\"/post/p1\">"), "{html}");
        let which = html
            .split("id=\"which\"")
            .nth(1)
            .and_then(|rest| rest.split("</p>").next())
            .expect("the paragraph");
        assert_eq!(visible(&format!("<p {which}")), "p1");
        assert!(
            html.contains("data-pw-captures=\"{&quot;id&quot;:&quot;p1&quot;}\""),
            "the reply form captures the thread's `id`: {html}"
        );
        // Shown, not merely carried: the block's template is in the
        // document's manifest since the thread is speculated on (ADR-0236).
        assert!(!visible(&html).contains("is popular"), "two likes: {html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command_answered(
            "feed.app.like",
            "b",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
        s.tell_waiting();
        let set = sets_of(&s, &doc).pop().expect("a patch set");
        assert!(
            visible(&written(&set)).contains("p1 is popular"),
            "the block rendered again with the parameter: {set:?}"
        );
    }

    /// **A row a change renders reads the page's parameters** (ADR-0231):
    /// a list of the thread's replies at the top of its page, each row
    /// naming the thread by its `id`, is sent the reply's row.
    #[test]
    fn a_row_a_change_renders_reads_the_pages_parameters() {
        let s = served_feed_with(|app| {
            app.replace(
                "            <Replies post={thread} />\n",
                "            <Replies post={thread} />\n            \
                 <ul id=\"listed\">{#each thread.replies as r (r.id)}<li>{r.text} under {id}</li>{/each}</ul>\n",
            )
        });
        let p3 = Params::from([("id".to_string(), "p3".to_string())]);
        s.serve_document_settled("b", "feed.app.PostPage~ui", &p3, &[])
            .expect("served");
        let reader = latest(&s.pending.lock().expect("pending"), "b");
        let answered = s
            .command_answered(
                "feed.app.reply",
                "a",
                &[Val::String("p3".into()), Val::String("Listed".into())],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        s.tell_waiting();
        let set = format!("{:?}", sets_of(&s, &reader).pop().expect("a patch set"));
        assert!(
            set.contains("Listed") && set.contains("under <!--pw:") && set.contains("-->p3<!--"),
            "the row, rendered with the thread's `id`: {set}"
        );
    }

    /// **An opaque value's representation is rendered** (ADR-0232):
    /// `{p.id.value}` in the home page's rows, and on the thread page its
    /// parameter's, its query's, a `{#match}` arm's and its view's. Until
    /// 2026-10-06 each built, then every request for its page was answered
    /// 503, `MissingValue { path: "p.id.value" }`.
    #[test]
    fn an_opaque_values_representation_is_rendered() {
        let s = served_feed_with(|app| {
            app.replace(
                "                        <p>{p.text}</p>\n",
                "                        <p>{p.text}</p>\n                        \
                 <p class=\"pid\" title=\"{p.id.value}\"><a href=\"/post/{p.id.value}\">{p.id.value}</a></p>\n",
            )
            .replace(
                "            <Replies post={thread} />\n",
                "            <Replies post={thread} />\n            \
                 <p id=\"which\">{id.value} {thread.id.value}</p>\n            \
                 {#match List.get(thread.replies, 0)}{:Some(r)}<p id=\"first\">{r.id.value}</p>\
                 {:None}<p id=\"first\">none</p>{/match}\n",
            )
            .replace(
                "        <p>{post.text}</p>\n",
                "        <p>{post.text}</p>\n        <p class=\"vid\">{post.id.value}</p>\n",
            )
        });
        let page = |path: &str| {
            fetched_as(&s, path, Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect::<String>()
        };
        let home = page("/");
        assert!(home.starts_with("HTTP/1.1 200"), "{home}");
        assert!(home.contains("class=\"pid\" title=\"p1\">"), "{home}");
        assert!(home.contains("href=\"/post/p1\">"), "{home}");
        let thread = page("/post/p1");
        assert!(thread.starts_with("HTTP/1.1 200"), "{thread}");
        assert_eq!(text_in(&thread, "<p id=\"which\">"), "p1 p1");
        assert_eq!(text_in(&thread, "<p id=\"first\">"), "p2");
        assert_eq!(text_in(&thread, "<p class=\"vid\">"), "p1");
    }

    /// **A value of a type that contains itself is written for a page's
    /// module as its nodes** (ADR-0233), by its query's type: the thread
    /// page's `thread`, `Thread`'s `Ok`, in level order. Until ADR-0233
    /// every value a page speculates on was written nested, knowing no type,
    /// and such a value was not speculated on (ADR-0205 §5). A value of no
    /// such type is written as before.
    #[test]
    fn a_value_that_contains_itself_is_written_as_its_nodes() {
        let s = served_feed();
        let p1 = Params::from([("id".to_string(), "p1".to_string())]);
        s.serve_document_settled("a", "feed.app.PostPage~ui", &p1, &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let bindings = s.bindings("a", doc.1).expect("read");
        let written = s
            .speculated_json(
                s.plan_of("feed.app.PostPage~ui"),
                "thread",
                &bindings["thread"],
            )
            .expect("written");
        let nodes = written["$graph"].as_array().expect("a graph");
        // The post, its reply, and the reply to that, each node's replies
        // the next run.
        let ids: Vec<&str> = nodes.iter().filter_map(|n| n["id"].as_str()).collect();
        assert_eq!(ids, ["p1", "p2", "p3"], "{written}");
        assert_eq!(nodes[0]["replies"], serde_json::json!([{ "$node": 1 }]));
        assert_eq!(nodes[1]["replies"], serde_json::json!([{ "$node": 2 }]));
        assert_eq!(nodes[2]["replies"], serde_json::json!([]));
        assert_eq!(nodes[0]["author"]["name"], "Ada", "{written}");
        // The timeline's rows, of no such type: as they were written.
        s.serve_document_settled("b", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let home = latest(&s.pending.lock().expect("pending"), "b");
        let bindings = s.bindings("b", home.1).expect("read");
        assert_eq!(
            s.speculated_json(s.plan_of("feed.app.Home~ui"), "feed", &bindings["feed"]),
            Ok(val_to_json(&bindings["feed"]))
        );
    }

    /// **A reply is its thread's, to every reader, and no timeline's**
    /// (ADR-0231): `reply` stages a post that replies to `to`, and emits
    /// `Posted`, so each open thread page is read again and sent it, with its
    /// counts. A reply to a post that is not there is answered not found,
    /// and nothing commits.
    #[test]
    fn a_reply_is_its_threads_to_every_reader_and_no_timelines() {
        let s = served_feed();
        let p3 = Params::from([("id".to_string(), "p3".to_string())]);
        s.serve_document_settled("b", "feed.app.PostPage~ui", &p3, &[])
            .expect("served");
        let reader = latest(&s.pending.lock().expect("pending"), "b");
        let answered = s
            .command_answered(
                "feed.app.reply",
                "a",
                &[
                    Val::String("p3".into()),
                    Val::String("A reply to p3".into()),
                ],
                Some("i-1"),
            )
            .expect("runs");
        assert!(answered.committed, "{:?}", answered.result);
        s.tell_waiting();
        let sent = visible(&written(&sets_of(&s, &reader).pop().expect("a patch set")));
        assert!(
            sent.contains("A reply to p3") && sent.contains("1 reply"),
            "the reader's thread and its count: {sent}"
        );
        // In its thread, and in no timeline.
        let page = |page: &str, params: &Params| {
            visible(
                &s.serve_document_settled("c", page, params, &[])
                    .expect("served")
                    .0,
            )
        };
        let thread = page("feed.app.PostPage~ui", &p3);
        assert!(thread.contains("A reply to p3"), "{thread}");
        assert!(!thread.contains("No replies yet."), "{thread}");
        let home = page("feed.app.Home~ui", &Params::new());
        assert!(home.contains("Hello, feed."), "{home}");
        assert!(!home.contains("A reply to p3"), "{home}");
        // To a post that is not there: not found, and nothing commits.
        let missing = s
            .command_answered(
                "feed.app.reply",
                "a",
                &[Val::String("none".into()), Val::String("Lost".into())],
                Some("i-2"),
            )
            .expect("runs");
        assert!(!missing.committed, "{:?}", missing.result);
        assert!(
            missing
                .result
                .as_ref()
                .is_some_and(|r| r.to_string().contains("not-found")),
            "{:?}",
            missing.result
        );
    }

    /// **What a commit drops of the session's own reaches no other
    /// session** (ADR-0219): `add_to_cart` drops the cart the session's
    /// `Cart` keys by it, and another session's open cart page is sent
    /// nothing.
    #[test]
    fn a_sessions_own_change_reaches_no_other_session() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        for session in ["a", "b"] {
            s.serve_document_settled(session, CART_PAGE, &Params::new(), &[])
                .expect("served");
        }
        let (mine, theirs) = {
            let pending = s.pending.lock().expect("pending");
            (latest(&pending, "a"), latest(&pending, "b"))
        };
        let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
        let (mine_before, theirs_before) = (frames(&mine), frames(&theirs));
        s.command(ADD, "a", &add_shown("espresso", 1), false)
            .expect("added");
        s.tell_waiting();
        assert!(
            frames(&mine) > mine_before,
            "the session's own page is told"
        );
        assert_eq!(frames(&theirs), theirs_before, "another's is not");
    }

    /// **A build that imports what its data layer does not supply is refused
    /// at start** (ADR-0218), as one whose handlers were not compiled is,
    /// rather than when a request first reaches the operation.
    #[test]
    fn a_build_importing_what_no_data_layer_supplies_is_refused() {
        let (_dir, out) = built_from_patches_in(
            "examples",
            |app| {
                format!(
                    "{app}\nfn nowhere(id: StoreId) -> Int !{{ database.read<Stores> }}\n    \
                     host \"store:data/stores#nowhere\"\n\npublic query Nowhere(id: StoreId) -> \
                     Int\n    freshness 0.seconds\n    cache shared\n    concurrency one_per_key\n    \
                     on_key_change cancel\n    timeout 2.seconds\n{{\n    nowhere(id)\n}}\n"
                )
            },
            &[],
        );
        let Err(why) = Server::from_build(out.clone(), out) else {
            panic!("served");
        };
        assert!(why.contains("store:data/stores#nowhere"), "{why}");
    }

    /// **A command commits the events it computes, and no others**
    /// (ADR-0104, ADR-0208). `add_to_cart` declares `emits
    /// UserCartChanged(current_user())`: its component evaluates the key and
    /// hands the event to the platform's outbox, the one function of it its
    /// contract imports, and its cart's entry moves. The server reads no key.
    #[test]
    fn a_command_commits_the_events_it_computes() {
        let s = rendering_server();
        let imported: Vec<&str> = s
            .contracts
            .iter()
            .find(|c| c.component_id == ADD)
            .expect("its contract")
            .imports
            .iter()
            .filter_map(|i| i.event.as_deref())
            .collect();
        assert_eq!(
            imported,
            ["Events.UserCartChanged"],
            "exactly the one it declares"
        );
        s.drain("session-7");
        let before = s.version("session-7");
        s.command(ADD, "session-7", &add_shown("espresso", 1), false)
            .expect("runs");
        assert_ne!(s.version("session-7"), before, "the event it computed");
        assert_eq!(
            outboxed("Events.Moved", &[Val::S64(-3), Val::Bool(true)]),
            Ok(pw_materialize::Event::new("Events.Moved", &["-3", "true"])),
            "an `Int` and a `Bool` key as their text"
        );
    }

    /// **A command that emits nothing tells nothing** (ADR-0104,
    /// ADR-0208). Its write commits, and no entry hears of it: the events
    /// committed are the ones the command computed, and a command built
    /// without `emits` computes none. Until 2026-09-26 this server committed
    /// `CartChanged` whatever a command declared.
    #[test]
    fn a_command_that_emits_nothing_tells_nothing() {
        let s = served_from_patches_in(
            "examples",
            |app| {
                // `add_to_cart`'s, the first command's. And what a cart's
                // change would drop that it then says nothing of: the cart's
                // hours (track `store-hours`), its checkout (track
                // `order-total`) and the times its stores offer (track
                // `scheduled-orders`), which read the cart too.
                let emits = "    emits         UserCartChanged(current_user())\n";
                assert!(app.contains(emits), "the store's add_to_cart emits");
                app.replacen(emits, "", 1).replacen(
                    "    invalidates   Cart(current_user())\n",
                    "    invalidates   Cart(current_user()), CartHours(current_user()), Checkout(current_user()), TimesOffered(current_user(), _)\n",
                    1,
                )
            },
            &[],
        );
        s.drain("session-7");
        let before = s.version("session-7");
        s.command(ADD, "session-7", &add_shown("espresso", 1), false)
            .expect("runs");
        assert_eq!(s.cart_value("session-7"), 1, "the write committed");
        assert_eq!(
            s.version("session-7"),
            before,
            "and no event reached the cart"
        );
    }

    /// **A command whose events a node cannot keep does not run there**
    /// (ADR-0208). Without `outbox.write`, its component's outbox is not
    /// linked: it is refused, and nothing is written, rather than its writes
    /// committed and its events lost.
    #[test]
    fn a_command_whose_events_cannot_be_kept_is_not_run() {
        let mut s = rendering_server();
        s.topology.nodes[0].grants.retain(|g| g != "outbox.write");
        let err = s
            .command(ADD, "session-8", &add("espresso", 1), false)
            .expect_err("no outbox here");
        assert!(err.contains("outbox.write"), "{err}");
        assert_eq!(s.cart_value("session-8"), 0, "nothing was written");
    }

    /// **The server will not start without the compiler's handler modules.**
    /// It reads the identities from the template IR and looks for each module
    /// where `pw emit-handlers` writes it.
    #[test]
    fn a_document_whose_handlers_were_not_compiled_is_refused() {
        let s = rendering_server();
        let named = s.handler_identities();
        // The cart's own page's handlers are the store's, as their code is
        // (ADR-0190); placing an order is one more (ADR-0193); and the
        // address page's five, an address chosen, renamed, removed and saved
        // (track `store-accounts`); and the cart page's checkout, a code
        // applied and taken off, four tips by percent, one by amount, and the
        // code's and the typed tip's bindings (track `order-total`); and the
        // order page's, an order awaiting its payment taken back (NEXT 6c);
        // and the rate page's, five stars chosen, the comment's binding and
        // the rating sent (track `reviews`), plus the page of times' two.
        assert_eq!(
            named.len(),
            30,
            "add_to_cart, clear_cart, a line's three (ADR-0172), place_order and an address's five: {named:?}"
        );
        assert_eq!(
            s.uncompiled_handlers(),
            named.iter().cloned().collect::<Vec<_>>(),
            "`.` has no handlers directory, so every one is missing"
        );

        let dist = std::env::temp_dir().join(format!("pw-dev-handlers-{}", std::process::id()));
        std::fs::create_dir_all(dist.join("handlers")).expect("a scratch dist");
        for id in &named {
            std::fs::write(dist.join("handlers").join(format!("{id}.mjs")), "").expect("a module");
        }
        let templates: Vec<Template> =
            serde_json::from_str(include_str!("../../store-ir.json")).expect("the template IR");
        let built = Server::on(dist.clone(), templates, dev_topology());
        assert!(built.uncompiled_handlers().is_empty());
        let _ = std::fs::remove_dir_all(&dist);
    }

    /// Each command is authorised on its own contract.
    ///
    /// Asking once for the whole program would make one command's authority
    /// every command's, which is the aggregation `contracts()` emits one
    /// contract per declaration to avoid.
    #[test]
    fn clear_cart_is_authorised_separately() {
        // The authorisation only. Running `clear_cart` to completion
        // regenerates the fragment it invalidates, which needs the compiled
        // template IR — and what this test is about is which contract decides,
        // not what the renderer does afterwards.
        let s = server(dev_topology());
        assert!(s.authorise("store.page.clear_cart").is_ok());

        // The refusal reaches the caller before any state moves, which is why
        // the barren case CAN run the whole command.
        let barren = server(barren());
        let err = barren
            .command(CLEAR, "session-1", &[], false)
            .expect_err("a barren node cannot host a write");
        assert!(err.contains("clear_cart"), "{err}");
        assert_eq!(barren.cart_value("session-1"), 0);
    }

    /// Everything the server holds per subscriber, summed: frames not yet
    /// acknowledged, and subscribers.
    fn held(s: &Server) -> (usize, usize) {
        let queue = s.pending.lock().unwrap();
        (queue.values().map(|w| w.frames.len()).sum(), queue.len())
    }

    /// **Under sustained load, what the server holds stays bounded** (E10
    /// gate item 3). Each number below was measured before the bound existed,
    /// and is in `docs/evidence/E10/load.txt`: 3,000 outbox rows after 3,000
    /// commands, and 600,000 frames for 1,000 departed visitors.
    #[test]
    fn sustained_load_leaves_the_server_bounded() {
        // One session with a subscriber that applies every batch.
        let s = rendering_server();
        s.serve_document("steady");
        for _ in 0..3_000 {
            s.command(ADD, "steady", &add_shown("espresso", 1), false)
                .expect("runs");
            let mut queue = s.pending.lock().unwrap();
            let doc = latest(&queue, "steady");
            let w = queue.get_mut(&doc).unwrap();
            let last = w.last_seq;
            w.acknowledge(last);
        }
        let (frames, subscribers) = held(&s);
        let rows = s.materializer.retained_events();
        println!(
            "steady: 3000 commands -> frames {frames}, subscribers {subscribers}, outbox rows {rows}"
        );
        assert_eq!((frames, subscribers, rows), (0, 1, 0));
        assert_eq!(s.cart_value("steady"), 3_000, "and every command committed");

        // Visitors who load the page once and leave, then changes to the menu.
        let s = rendering_server();
        for i in 0..1_000 {
            s.serve_document(&format!("visitor-{i}"));
        }
        for i in 0..300 {
            let name = if i % 2 == 0 { "Gibraltar" } else { "Cortado" };
            s.broadcast_menu(MenuOp::Rename {
                id: "cortado".into(),
                name: name.into(),
            })
            .expect("renames");
        }
        let (frames, subscribers) = held(&s);
        println!(
            "churn: 1000 visitors, 300 menu changes -> frames {frames}, subscribers {subscribers}, \
             materialized entries {}",
            s.materializer.entry_count()
        );
        assert_eq!(subscribers, 1_000, "none idle yet");
        assert_eq!(frames, 1_000, "one reload each: every visitor fell behind");
        for w in s.pending.lock().unwrap().values() {
            assert!(w.behind);
            assert!(matches!(
                w.frames.as_slice(),
                [(
                    _,
                    StreamFrame::Recovery {
                        recovery: pw_protocol::Recovery::Reload,
                        ..
                    }
                )]
            ));
        }

        // Past the idle limit, the next change forgets them all.
        let long_ago = std::time::Instant::now() - IDLE - std::time::Duration::from_secs(1);
        for w in s.pending.lock().unwrap().values_mut() {
            w.seen = long_ago;
        }
        s.broadcast_menu(MenuOp::Rename {
            id: "cortado".into(),
            name: "Cortado".into(),
        })
        .expect("renames");
        let (frames, subscribers) = held(&s);
        let entries = s.materializer.entry_count();
        println!(
            "idle: after {}s -> frames {frames}, subscribers {subscribers}, materialized entries {entries}",
            IDLE.as_secs()
        );
        assert_eq!((frames, subscribers), (0, 0));
        // The menu counted and its line (ADR-0277) are every reader's, kept
        // as the menu is.
        assert_eq!(
            entries, 3,
            "the shared menu, its count and its line, and no visitor's cart"
        );

        // A visitor who comes back is served a whole document: their cart
        // entry is regenerated from state, not lost.
        s.command(ADD, "visitor-7", &add_shown("espresso", 2), false)
            .expect("runs");
        let (html, cursor) = s.serve_document("visitor-7");
        assert!(cursor > 0);
        assert!(html.contains(">2<"), "the returning visitor's cart: {html}");
    }

    /// **A queue that reaches its bound becomes one reload, and stays one.**
    #[test]
    fn a_subscriber_that_falls_behind_is_told_to_reload() {
        let mut w = Subscriber::default();
        let notice = |n: u64| StreamFrame::ResourceChanged {
            protocol: CURRENT,
            entry: cart_entry(&format!("session-{n}"), &format!("session-{n}")),
            version: Version(n),
        };
        for n in 0..MAX_WAITING as u64 {
            w.push(notice(n));
        }
        assert_eq!(w.frames.len(), MAX_WAITING, "at the bound, not past it");
        assert!(!w.behind);
        w.push(notice(999));
        w.push(notice(1000));
        assert!(w.behind);
        assert_eq!(w.frames.len(), 1);
        let (cursor, frames) = w.after(0);
        assert!(matches!(
            frames.as_slice(),
            [StreamFrame::Recovery {
                recovery: pw_protocol::Recovery::Reload,
                ..
            }]
        ));
        assert_eq!(
            cursor,
            MAX_WAITING as u64 + 1,
            "the reload took the next sequence"
        );
    }

    /// **A page the server has forgotten is told to reload**, through the
    /// real route: a poll with a cursor for a session with no subscriber.
    #[test]
    fn a_forgotten_page_that_polls_is_told_to_reload() {
        let s = server(dev_topology());
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let at = listener.local_addr().expect("addr");
        let poll = |since: u64| -> String {
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let (stream, _) = listener.accept().expect("accept");
                    handle(&s, stream);
                });
                let mut c = TcpStream::connect(at).expect("connect");
                write!(
                    c,
                    "GET /stream?since={since} HTTP/1.1\r\ncookie: pw-session=gone\r\n\r\n"
                )
                .expect("request");
                let mut out = String::new();
                c.read_to_string(&mut out).expect("response");
                out
            })
        };
        let reply = poll(7);
        println!("{}", reply.lines().last().unwrap_or_default());
        assert!(reply.contains(r#""recovery":"reload""#), "{reply}");

        // The control: a page that has never been served a document, cursor
        // zero, is subscribed rather than told to reload.
        let reply = poll(0);
        assert!(!reply.contains("recovery"), "{reply}");
        assert!(
            s.pending
                .lock()
                .unwrap()
                .contains_key(&("gone".to_string(), 0))
        );
    }

    #[test]
    fn store_commands_carry_and_enforce_their_signed_in_precondition() {
        let mut s = server(dev_topology());
        // Track `store-accounts` (ADR-0318): anyone fills a cart, a guest its
        // session's guest's, so the cart's commands require no one; placing
        // an order requires a reader signed in (`place_order`, whose
        // component the committed evidence does not hold).
        for id in [ADD, CLEAR] {
            let contract = s
                .contracts
                .iter()
                .find(|c| c.component_id == id)
                .expect("command contract");
            let requirements = &contract.exports[0]
                .component
                .as_ref()
                .expect("compiled export")
                .authorization;
            assert!(requirements.is_empty(), "{id}: {requirements:?}");
        }
        let place = s
            .contracts
            .iter()
            .find(|c| c.component_id == "store.page.place_order")
            .expect("place_order's contract");
        let requirements = &place.exports[0]
            .component
            .as_ref()
            .expect("compiled export")
            .authorization;
        assert_eq!(requirements.len(), 1, "{requirements:?}");
        assert_eq!(requirements[0].predicate, "SignedIn");
        assert!(requirements[0].arguments.is_empty());

        s.command(ADD, "signed-in-demo", &add_shown("espresso", 1), false)
            .expect("anyone fills a cart");

        // A predicate the deployment does not know is denied before the
        // component or staged data layer can change state.
        let mut requirement = requirements[0].clone();
        requirement.predicate = "OwnsOrder".to_string();
        s.contracts
            .iter_mut()
            .find(|c| c.component_id == ADD)
            .expect("command contract")
            .exports[0]
            .component
            .as_mut()
            .expect("compiled export")
            .authorization
            .push(requirement);
        let before = s.cart_value("signed-in-demo");
        let err = s
            .command(ADD, "signed-in-demo", &add_shown("cortado", 1), false)
            .expect_err("unknown authorization must fail closed");
        assert!(err.contains("no authorization predicate"), "{err}");
        assert_eq!(s.cart_value("signed-in-demo"), before);
    }

    /// A component the build has no contract for is refused, not run.
    ///
    /// "I have no record of this" and "this is fine" must never be the same
    /// answer — the same fail-closed direction as the artifact audit's
    /// unparseable import.
    #[test]
    fn an_unknown_component_is_refused_rather_than_defaulted() {
        let s = server(dev_topology());
        let err = s
            .authorise("store.page.nothing_declares_this")
            .expect_err("no contract");
        assert!(err.contains("no contract"), "{err}");

        // The control: a component that IS in the contracts is authorised, so
        // the refusal above is about the missing record and not about the
        // lookup being broken.
        assert!(s.authorise("store.page.add_to_cart").is_ok());
    }

    /// **A frame is forgotten when the page says it applied it** (ADR-0139).
    ///
    /// A page's stream can be held on the server after its connection is
    /// gone, by a reload or a network that dropped. Until 2026-10-02 the
    /// stream dropped each frame 25 ms after writing it, so a change written
    /// to a connection nobody read never reached the page's next request: the
    /// keyed-list suite's intermittent failure, found building ADR-0138.
    /// Since ADR-0161 a page that replaces another is another document, so
    /// this is the same document asking again.
    #[test]
    fn a_frame_a_stream_wrote_to_a_dropped_connection_still_reaches_its_page() {
        let s = rendering_server();
        let session = "dropped";
        let (_, document) = s.serve_document(session);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let mut gone = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (mut held, _) = listener.accept().expect("accept");
        std::thread::scope(|scope| {
            scope.spawn(|| stream_open(&s, &mut held, session, false, document, document));
            s.command(ADD, session, &add_shown("espresso", 1), false)
                .expect("runs");

            // The stream wrote the change to the connection ... (read until
            // it has: the bytes are the event, and ten seconds the most that
            // is waited for them. A sleep of 300 ms and a read of 500 ms were
            // a silence a slow machine made too.)
            gone.set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("timeout");
            let mut written = String::new();
            let mut buf = vec![0; 1 << 16];
            while !written.contains("\"frames\"") {
                let n = match gone.read(&mut buf) {
                    // A stopped and continued process is interrupted in a read
                    // that has a timeout: read on.
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    read => read.expect("the stream wrote the change"),
                };
                assert!(n > 0, "the stream ended without the change: {written}");
                written.push_str(&String::from_utf8_lossy(&buf[..n]));
            }

            // ... and has gone round again since: each pass marks the
            // subscriber seen as it begins, in the one hold of the table that
            // looks at what the subscriber holds, so a pass begun after the
            // write is a pass that has done with what it wrote whatever it
            // does with it (acknowledges it, if it is wrong). A sleep of 300
            // ms stood for this.
            let key = (session.to_string(), document);
            let seen = |s: &Server| s.pending.lock().expect("pending")[&key].seen;
            let written_by = seen(&s);
            until("the stream's pass after its write", || {
                seen(&s) > written_by
            });

            // ... which the page never read: its next request, from where it
            // was, is still sent it.
            let mut queue = s.pending.lock().expect("pending");
            let waiting = queue.get_mut(&key).expect("its subscriber");
            let (cursor, frames) = waiting.after(document);
            assert!(
                !frames.is_empty(),
                "the change was written to a connection that dropped, and dropped"
            );
            // Control: what a page says it applied is dropped.
            waiting.acknowledge(cursor);
            assert!(waiting.after(document).1.is_empty());
        });
    }

    /// **A path gives a route its parameters** (ADR-0162): one segment each,
    /// as the compiler's link check matches a link, decoded as a path is.
    #[test]
    fn a_path_gives_a_route_its_parameters() {
        let id = |path: &str| route_params("/stores/{id}", path).map(|p| p["id"].clone());
        assert_eq!(id("/stores/48").as_deref(), Some("48"));
        assert_eq!(id("/stores/48/").as_deref(), Some("48"));
        assert_eq!(id("/stores/flat%20white").as_deref(), Some("flat white"));
        // A `+` in a path is itself; a form's query makes it a space.
        assert_eq!(id("/stores/a+b").as_deref(), Some("a+b"));
        // Not the route: another word, another length, or no segment.
        for other in ["/shops/48", "/stores", "/stores/48/menu", "/stores//", "/"] {
            assert_eq!(id(other), None, "{other}");
        }
        // Not text: refused rather than passed through.
        assert_eq!(id("/stores/%zz"), None);
        assert_eq!(id("/stores/%FF"), None);
        // A route of words alone, and the root.
        assert_eq!(route_params("/about", "/about"), Some(Params::new()));
        assert_eq!(route_params("/", "/"), Some(Params::new()));
    }

    /// **A route's query part, read as a browser's form writes it**
    /// (ADR-0309): the URL Standard's form-urlencoded parsing, `+` a space and
    /// `%20` one, the first of a name taken, a name the route does not give
    /// ignored, and bytes that are no UTF-8 refused.
    #[test]
    fn a_query_is_read_as_a_browsers_form_writes_it() {
        assert_eq!(route_query_names("/search{?q,page}"), ["q", "page"]);
        assert_eq!(route_query_names("/stores/{id}"), Vec::<&str>::new());
        assert_eq!(route_path("/search{?q}"), "/search");
        assert_eq!(route_path("/stores/{id}"), "/stores/{id}");
        let q = |query: &str| query_params("/search{?q}", query).map(|p| p.get("q").cloned());
        assert_eq!(q("q=flat+white"), Some(Some("flat white".to_string())));
        assert_eq!(q("q=flat%20white"), Some(Some("flat white".to_string())));
        assert_eq!(q("q=a%2Bb"), Some(Some("a+b".to_string())));
        assert_eq!(q("q=%E3%81%B2"), Some(Some("\u{3072}".to_string())));
        assert_eq!(q("q="), Some(Some(String::new())));
        assert_eq!(q("q"), Some(Some(String::new())));
        assert_eq!(q(""), Some(None));
        assert_eq!(
            q("utm_source=x&q=pie&q=cake"),
            Some(Some("pie".to_string()))
        );
        // Not text: refused, where the URL Standard would put U+FFFD.
        assert_eq!(q("q=%FF"), None);
        assert_eq!(q("q=%zz"), None);
        // A name the route does not give is ignored, whatever it holds.
        assert_eq!(q("x=%FF&q=pie"), Some(Some("pie".to_string())));
        // What the page is given: a query's `Some` or `None`, a path's text.
        let plan = serde_json::json!({ "route": "/stores/{id}{?tab}" });
        let given = Params::from([("id".to_string(), "48".to_string())]);
        assert_eq!(
            param_val(&plan, "id", &given),
            Some(Val::String("48".into()))
        );
        assert_eq!(param_val(&plan, "tab", &given), Some(Val::Option(None)));
        let given = Params::from([("tab".to_string(), "menu".to_string())]);
        assert_eq!(
            param_val(&plan, "tab", &given),
            Some(Val::Option(Some(Box::new(Val::String("menu".into())))))
        );
        assert_eq!(param_val(&plan, "id", &given), None);
    }

    /// **The store at its route, and a second store at its own** (ADR-0162,
    /// charter §15.3): `/stores/{id}` names the store page, and `48` is a
    /// store with its own name and menu.
    #[test]
    fn each_store_is_served_at_its_route() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (page, params) = s
            .routed("/stores/48", "")
            .expect("the store's route")
            .expect("its query read");
        assert_eq!(page, "store.page.StorePage~ui");
        assert_eq!(params, store_params("48"));
        assert_eq!(s.routed("/stores", ""), None);
        let (harbor, _) = s.serve_store_document("a", "48").expect("served");
        let harbor = visible(&harbor);
        assert!(harbor.contains("Harbor Coffee"), "{harbor}");
        assert!(harbor.contains("Matcha Latte"), "{harbor}");
        assert!(!harbor.contains("Blue Bottle"), "{harbor}");
        assert!(!harbor.contains("Espresso"), "{harbor}");
        // Store 47 at its route is the page `/StorePage.html` serves.
        let (blue, _) = s.serve_store_document("b", STORE_ID).expect("served");
        assert!(visible(&blue).contains("Blue Bottle"), "{blue}");
        // And store 48 again, after 47: each store's menu is its own
        // fragment, not the one rendered last.
        let (again, _) = s.serve_store_document("d", "48").expect("served");
        let again = visible(&again);
        assert!(again.contains("Matcha Latte"), "{again}");
        assert!(!again.contains("Espresso"), "{again}");
        // A store this server does not hold is not found, as the page
        // declares (ADR-0163).
        let absent = s
            .serve_store_document("c", "999")
            .expect_err("no store 999");
        assert!(matches!(absent, Unread::NotFound(_)), "{absent:?}");
    }

    /// **A store that is not there is not found** (ADR-0163): the page
    /// declares `not_found_on StoreError.NotFound`, and a store the data
    /// layer does not hold is answered 404, a page that is nothing of the
    /// store's, where any other failure is answered 503 (ADR-0147).
    #[test]
    fn a_store_that_is_not_there_is_not_found() {
        let fetched = |s: &Server, path: &str, session: &str| -> String {
            fetched_as(s, path, Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let absent = fetched(&s, "/stores/999", "a");
        // With its reason, as every response has it.
        assert!(absent.starts_with("HTTP/1.1 404 Not Found\r\n"), "{absent}");
        let page = visible(&absent);
        assert!(page.contains("Not found"), "{page}");
        for of_the_store in ["Blue Bottle", "Harbor Coffee", "Espresso", "Add"] {
            assert!(!page.contains(of_the_store), "{page}");
        }
        // And it is no document the server holds.
        assert!(
            !s.pending
                .lock()
                .expect("pending")
                .keys()
                .any(|(session, _)| session == "a"),
            "a page that was not found waits for changes"
        );
        // Control: a store the server holds is served.
        assert!(fetched(&s, "/stores/48", "b").starts_with("HTTP/1.1 200 OK\r\n"));
        // Control: the clause names the case that means absent. Naming
        // another, the store's absence is a failure like any other.
        let other = served_from_patches_in(
            "examples",
            |app| {
                app.replace(
                    "not_found_on StoreError.NotFound",
                    "not_found_on StoreError.Unavailable",
                )
            },
            &[],
        );
        let failed = fetched(&other, "/stores/999", "a");
        assert!(
            failed.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
            "{failed}"
        );
        // Control: a page that names none is answered 503 for it.
        let none = served_from_patches_in(
            "examples",
            |app| app.replace("    not_found_on StoreError.NotFound\n", ""),
            &[],
        );
        assert!(!none.plan.to_string().contains("not_found"));
        let failed = fetched(&none, "/stores/999", "a");
        assert!(
            failed.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
            "{failed}"
        );
    }

    /// **A change to one store's menu reaches that store's pages only**
    /// (§15.6 test 11, ADR-0162), and a change to the session's cart reaches
    /// both its stores' pages.
    #[test]
    fn a_change_to_one_stores_menu_reaches_that_stores_pages_only() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, blue) = s.serve_store_document("a", STORE_ID).expect("served");
        let (_, harbor) = s.serve_store_document("a", "48").expect("served");
        let doc = |d: u64| ("a".to_string(), d);
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".to_string(),
            name: "Espresso Doppio".to_string(),
        })
        .expect("the menu changes");
        // A rename sets its item's parts, one patch or a set (ADR-0168).
        let patched = |d: u64| {
            s.pending.lock().expect("pending")[&doc(d)]
                .frames
                .iter()
                .any(|(_, f)| matches!(f, StreamFrame::Patch(_) | StreamFrame::PatchSet(_)))
        };
        assert!(
            patched(blue),
            "store 47's page was not told its menu changed"
        );
        assert!(!patched(harbor), "store 48's page was told of 47's menu");
        // The cart is the session's: an add reaches both.
        let sets = |d: u64| {
            s.pending.lock().expect("pending")[&doc(d)]
                .frames
                .iter()
                .filter(|(_, f)| matches!(f, StreamFrame::PatchSet(set) if !set.patches.is_empty()))
                .count()
        };
        let before = (sets(blue), sets(harbor));
        s.command(ADD, "a", &add_shown("drip", 1), false)
            .expect("runs");
        assert_eq!((sets(blue) - before.0, sets(harbor) - before.1), (1, 1));
    }

    /// The cart page's path (ADR-0190).
    const CART_PAGE: &str = "store.page.CartPage~ui";

    /// **A second page that binds a query is served at its route** (ADR-0190),
    /// by its own template and plan: the cart page reads the session's cart.
    /// Until 2026-10-04 it was refused, as only the store's page bound one.
    #[test]
    fn a_page_that_binds_a_query_is_served_at_its_route() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |path: &str, session: &str| -> String {
            fetched_as(&s, path, Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        for path in ["/cart", "/page/store.page.CartPage~ui"] {
            let cart = page(path, "a");
            assert!(cart.starts_with("HTTP/1.1 200"), "{path}: {cart}");
            assert!(cart.contains("<title>Your cart</title>"), "{path}: {cart}");
            assert!(
                cart.contains("\"template\":\"store.page.CartPage~ui\""),
                "{cart}"
            );
            assert!(visible(&cart).contains("Your cart is empty."), "{cart}");
            // It speculates from its own module (ADR-0191).
            assert!(
                cart.contains("\"speculation\":\"/speculation/store.page.CartPage~ui.mjs\""),
                "{cart}"
            );
        }
        // Which is served, and a module no page's manifest names is not.
        let module = page("/speculation/store.page.CartPage~ui.mjs", "a");
        assert!(
            module.starts_with("HTTP/1.1 200") && module.contains("text/javascript"),
            "{module}"
        );
        let none = page("/speculation/store.page.Nothing.mjs", "a");
        assert!(!none.starts_with("HTTP/1.1 200"), "{none}");
        // The session's cart, and not another's.
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("the command commits");
        let a = visible(&page("/cart", "a"));
        assert!(
            a.contains("Espresso") && a.contains("Items in cart: 2"),
            "{a}"
        );
        let b = visible(&page("/cart", "b"));
        assert!(b.contains("Your cart is empty."), "{b}");
    }

    /// The order's page, and the command that places an order (ADR-0193).
    const ORDER_PAGE: &str = "store.page.OrderPage~ui";
    const PLACE: &str = "store.page.place_order";

    /// Every patch set queued for one document, in order.
    fn sets_of(s: &Server, doc: &Doc) -> Vec<PatchSet> {
        s.pending.lock().expect("pending")[doc]
            .frames
            .iter()
            .filter_map(|(_, f)| match f {
                StreamFrame::PatchSet(set) => Some(set.clone()),
                _ => None,
            })
            .collect()
    }

    /// **What each change set the text part of `page` that reads `value`
    /// to**, where it is (ADR-0320): a block told part by part sets its
    /// parts as text, and does not write the block again.
    fn texts_told(s: &Server, doc: &Doc, page: &str, value: &str) -> Vec<String> {
        let parts: Vec<u32> = s
            .template_of(page)
            .manifest()
            .into_iter()
            .filter(|p| p.kind == "text" && p.value == value)
            .map(|p| p.id.0)
            .collect();
        assert!(
            !parts.is_empty(),
            "`{page}` has no text part that reads `{value}`"
        );
        sets_of(s, doc)
            .iter()
            .flat_map(|set| set.patches.iter())
            .filter(|p| p.target.instances.is_empty() && parts.contains(&p.target.part.0))
            .filter_map(|p| match &p.operation {
                PatchOp::ReplaceText { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The HTML a patch set writes, its blocks rendered again.
    fn written(set: &PatchSet) -> String {
        set.patches
            .iter()
            .map(|p| match &p.operation {
                PatchOp::ReplaceRange { html } => html.clone(),
                PatchOp::ReplaceText { text } => text.clone(),
                _ => String::new(),
            })
            .collect()
    }

    /// **A page is shown in its layout** (ADR-0303): the store's four pages
    /// in `StoreLayout`, its header the same markup on each, the page's own
    /// in its slot; and its count told as the cart changes on a page that
    /// reads no cart of its own, the order's, as any part's value is
    /// (ADR-0219).
    #[test]
    fn a_store_page_is_shown_in_its_layout_and_its_count_is_told() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |path: &str| -> String {
            fetched_as(&s, path, Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let header = |html: &str| -> String {
            html.split_once("<header>")
                .and_then(|(_, rest)| rest.split_once("</header>"))
                .map(|(h, _)| h.to_string())
                .unwrap_or_default()
        };
        let shared = header(&page("/"));
        assert!(shared.contains("id=\"header-cart-count\""), "{shared}");
        for path in ["/", "/stores/47", "/cart", "/order"] {
            let html = page(path);
            assert!(html.starts_with("HTTP/1.1 200"), "{path}: {html}");
            assert_eq!(header(&html), shared, "{path}");
            // The page's own markup is in the slot, after the header.
            let (before, rest) = html.split_once("<!--pw-slot-->").expect("a slot");
            let (inside, _) = rest.split_once("<!--/pw-slot-->").expect("closed");
            assert!(before.contains("</header>"), "{path}: {before}");
            assert!(inside.contains("<main"), "{path}: {inside}");
        }
        // The order's page reads only its order; its layout reads the cart.
        let _ = s
            .serve_document_settled("a", ORDER_PAGE, &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("added");
        let told: String = sets_of(&s, &doc).iter().map(written).collect();
        assert_eq!(told, "2", "the header's count, and nothing else");
    }

    /// **One entry, one value** (ADR-0312): the store's page and its layout
    /// both show the session's cart, and its document carries it once, by
    /// the page's name, the layout's count rendered from it. Until then it
    /// carried two entries for the one cart, `cart` and `cart~StoreLayout`,
    /// and a change sent the value twice, each at its own version.
    #[test]
    fn a_page_and_its_layout_show_one_entry_by_one_value() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let html: String = fetched_as(&s, &format!("/stores/{STORE_ID}"), Some("a"))
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        let manifest = manifest_of(&html);
        let entries: Vec<&String> = manifest["entries"]
            .as_object()
            .expect("entries")
            .keys()
            .collect();
        assert_eq!(entries, ["cart"], "{manifest}");
        // Nothing is sent by the layout's name: the cart's entry is the
        // materializer's, and no other value is speculated on.
        assert_eq!(
            s.speculates_on_cart("store.page.StorePage~ui").as_deref(),
            Some("cart")
        );
        assert!(s.speculated_bindings("store.page.StorePage~ui").is_empty());
    }

    /// **What a handler at the top of the page captures is set again when it
    /// changes** (ADR-0217, ADR-0210's urgent defect 2). A button on the
    /// cart's page that captures the cart: after a line is added, the patch
    /// sets its captures to the cart with the line. Until then it kept the
    /// cart the page was first rendered with, and a press sent that.
    #[test]
    fn a_handlers_captures_at_the_top_of_the_page_are_set_again() {
        let s = served_from_patches_in(
            "examples",
            |app| {
                // Track `order-total`: after the promo code's and the tip's.
                let page = "    signal tip_typed: String = \"\"\n\n    view {\n        <title>Your cart</title>\n        <main>\n";
                assert!(app.contains(page), "the cart's page");
                app.replacen("import domain.{ ", "import domain.{ CartLine, ", 1)
                    .replacen(
                    page,
                    "    signal tip_typed: String = \"\"\n    signal kept: List<CartLine> = []\n\n    view {\n        <title>Your cart</title>\n        <main>\n            <button type=\"button\" id=\"keep\" on:press={resumable(captures = { cart }) => kept = cart.lines}>Keep</button>\n",
                    1,
                )
            },
            &[],
        );
        let (html, _, _, _) = s
            .serve_document_settled("a", CART_PAGE, &Params::new(), &[])
            .expect("served");
        assert!(html.contains("id=\"keep\""), "{html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("added");
        let set = sets_of(&s, &doc).pop().expect("a patch set");
        let captures: Vec<&str> = set
            .patches
            .iter()
            .filter_map(|p| match &p.operation {
                PatchOp::SetAttribute { name, value } if name == "data-pw-captures" => {
                    Some(value.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(
            captures.iter().any(|v| v.contains("espresso")),
            "the button captures the cart with its line: {captures:?}"
        );
    }

    /// **An order is placed from the cart, and its page shows it**
    /// (ADR-0193): the cart's lines become the session's order, the cart is
    /// empty after, and a page open on the order is sent the change.
    #[test]
    fn an_order_is_placed_from_the_cart_and_reaches_its_open_page() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (html, _, _, _) = s
            .serve_document_settled("a", ORDER_PAGE, &Params::new(), &[])
            .expect("served");
        assert!(visible(&html).contains("You have no order yet."), "{html}");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("added");
        s.command(ADD, "a", &add_shown("cortado", 1), false)
            .expect("added");
        s.command(PLACE, "a", &[s.shown_total("a")], false)
            .expect("placed");
        // The order is the cart's lines and a status (ADR-0193). Until
        // 2026-10-08 it kept the status alone, and its lines went with the
        // emptied cart.
        assert_eq!(
            s.order_of("a"),
            Some((
                "placed".to_string(),
                vec![("espresso".to_string(), 2), ("cortado".to_string(), 1)]
            ))
        );
        assert!(s.cart_lines("a").is_empty(), "the cart is empty after");
        let last = sets_of(&s, &doc).pop().expect("a patch set");
        assert!(
            written(&last).contains("Placed: the store has your order."),
            "{last:?}"
        );
        // Another session has none.
        let (other, _, _, _) = s
            .serve_document_settled("b", ORDER_PAGE, &Params::new(), &[])
            .expect("served");
        assert!(
            visible(&other).contains("You have no order yet."),
            "{other}"
        );
    }

    /// **An empty cart places no order** (ADR-0193), and says so: the
    /// command is answered with its declared error, and nothing commits.
    #[test]
    fn an_empty_cart_places_no_order() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let answered = s
            .command_answered(PLACE, "a", &[s.shown_total("a")], None)
            .expect("answered");
        assert!(!answered.committed, "{:?}", answered.result);
        assert!(
            answered
                .result
                .as_ref()
                .is_some_and(|r| r.to_string().contains("nothing-to-order")),
            "{:?}",
            answered.result
        );
        assert_eq!(s.order_of("a"), None);
    }

    /// **The store moving an order along reaches the page open on it**
    /// (ADR-0193), through the kitchen's own address: not a command, so not
    /// the cart's change. Sent against the order's own entry, at a version
    /// that advances, and with no speculated value to a cart page open
    /// beside it.
    fn moved_along(s: &Served) {
        s.command(ADD, "a", &add_shown("espresso", 1), false)
            .expect("added");
        s.command(PLACE, "a", &[s.shown_total("a")], false)
            .expect("placed");
        s.serve_document_settled("a", ORDER_PAGE, &Params::new(), &[])
            .expect("served");
        let order = latest(&s.pending.lock().expect("pending"), "a");
        s.serve_document_settled("a", CART_PAGE, &Params::new(), &[])
            .expect("served");
        let cart = latest(&s.pending.lock().expect("pending"), "a");
        let mut versions = Vec::new();
        for (status, said) in [
            ("preparing", "Preparing: the store is making it."),
            ("on-the-way", "On its way to you."),
            ("delivered", "Delivered."),
        ] {
            let answer = posted(s, &format!("/bench/order?status={status}"), "a", "", "");
            assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
            let last = sets_of(s, &order).pop().expect("a patch set");
            assert!(written(&last).contains(said), "{status}: {last:?}");
            let basis = &last.basis.resources;
            assert_eq!(basis.len(), 1);
            assert_eq!(basis[0].entry, s.order_entry("a"));
            versions.push(basis[0].version);
        }
        assert!(versions.windows(2).all(|w| w[0] < w[1]), "{versions:?}");
        let values = s.pending.lock().expect("pending")[&cart]
            .frames
            .iter()
            .filter(|(_, f)| matches!(f, StreamFrame::EntryValue { .. }))
            .count();
        assert_eq!(values, 0, "the cart's value went with the order's change");
        // Moved along, it keeps the lines it was placed with (ADR-0193).
        assert_eq!(
            s.order_of("a"),
            Some(("delivered".to_string(), vec![("espresso".to_string(), 1)]))
        );
    }

    #[test]
    fn the_store_moving_an_order_along_reaches_its_open_page() {
        moved_along(&served_from_patches_in(
            "examples",
            |app| app.to_string(),
            &[],
        ));
    }

    /// **Every event a commit stages is consumed once it is delivered**
    /// (track `store-pg`), on whichever layer the store is: after orders are
    /// placed and moved along, the materializer's outbox holds nothing.
    /// Until 2026-10-08 each `OrderChanged` was kept for good, since no entry
    /// a drain names reaches it.
    #[test]
    fn every_event_a_commit_stages_is_consumed_once_delivered() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_document_settled("a", ORDER_PAGE, &Params::new(), &[])
            .expect("served");
        for round in 0..3 {
            s.command(ADD, "a", &add_shown("espresso", 1), false)
                .expect("added");
            s.command(PLACE, "a", &[s.shown_total("a")], false)
                .expect("placed");
            let answer = posted(&s, "/bench/order?status=preparing", "a", "", "");
            assert!(answer.starts_with("HTTP/1.1 200"), "{round}: {answer}");
        }
        s.broadcast_menu(MenuOp::Rename {
            id: "cortado".into(),
            name: "Gibraltar".into(),
        })
        .expect("renamed");
        assert_eq!(s.order_of("a").map(|o| o.0).as_deref(), Some("preparing"));
        assert_eq!(
            s.materializer.retained_events(),
            0,
            "the outbox holds nothing"
        );
    }

    /// **A program that names one of the host's own operations is refused**
    /// (track `store-pg`): the routes that change the store's data stage
    /// `store:host/…` through the layer, and no grant gives a program one,
    /// whatever capability it declares for it.
    #[test]
    fn a_program_naming_a_host_operation_is_refused() {
        let (_dir, out) = built_from_patches_in(
            "examples",
            |app| {
                format!(
                    "{app}\nfn post_notice(id: StoreId) -> Int !{{ database.read<Stores> }}\n    \
                     host \"store:host/notices#post\"\n\npublic query Posting(id: StoreId) -> \
                     Int\n    freshness 0.seconds\n    cache shared\n    concurrency one_per_key\n    \
                     on_key_change cancel\n    timeout 2.seconds\n{{\n    post_notice(id)\n}}\n"
                )
            },
            &[],
        );
        let Err(why) = Server::from_build(out.clone(), out) else {
            panic!("served");
        };
        assert!(
            why.contains("store:host/notices#post")
                && why.contains("an operation the host keeps for its own routes"),
            "{why}"
        );
    }

    /// **The stores, as the home page** (ADR-0192): at `/`, each store this
    /// server holds by its name and description, linked to its page, and the
    /// session's cart beside them, kept current as it changes. Until
    /// 2026-10-04 `/` was store 47's page.
    #[test]
    fn the_stores_are_the_home_page() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |path: &str, session: &str| -> String {
            fetched_as(&s, path, Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let home = page("/", "a");
        assert!(home.starts_with("HTTP/1.1 200"), "{home}");
        assert!(home.contains("<title>Stores</title>"), "{home}");
        for (id, name) in [(STORE_ID, STORE_NAME), SECOND_STORE] {
            assert!(
                home.contains(&format!("href=\"/stores/{id}\"")) && visible(&home).contains(name),
                "{id}: {home}"
            );
            assert!(
                visible(&home).contains(store_description(id)),
                "{id}: {home}"
            );
        }
        assert!(visible(&home).contains("Your cart: 0 items"), "{home}");
        // The cart beside them, as the session's: its count is patched there.
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("the command commits");
        let count = s
            .template_of("store.page.HomePage~ui")
            .manifest()
            .into_iter()
            .find(|e| e.value == "cart.line_count")
            .expect("the home page's count")
            .id;
        assert!(
            patch_sets(&s, "a").iter().any(|set| set
                .patches
                .iter()
                .any(|p| p.target.part == count
                    && matches!(&p.operation, PatchOp::ReplaceText { text } if text == "2"))),
            "the home page's count was not patched"
        );
        assert!(visible(&page("/", "a")).contains("Your cart: 2 items"));
    }

    /// **A change reaches every page of the session that reads it**
    /// (ADR-0190): a press on the store's page sends the cart page the
    /// difference, derived from the cart page's own plan and addressed to its
    /// own template, and the store's page its own.
    #[test]
    fn a_change_reaches_each_page_that_reads_it_by_its_own_plan() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_store_document("a", STORE_ID).expect("served");
        s.serve_document_settled("a", CART_PAGE, &Params::new(), &[])
            .expect("served");
        let docs = documents_of(&s.pending.lock().expect("pending"), "a");
        let (store, cart) = (docs[0].clone(), docs[1].clone());
        s.command(ADD, "a", &add_shown("espresso", 2), false)
            .expect("the command commits");
        let set = |doc: &Doc| -> PatchSet {
            s.pending.lock().expect("pending")[doc]
                .frames
                .iter()
                .rev()
                .find_map(|(_, f)| match f {
                    StreamFrame::PatchSet(set) if !set.patches.is_empty() => Some(set.clone()),
                    _ => None,
                })
                .expect("a patch set")
        };
        let schema = |page: &str| TemplateSchemaId(s.template_of(page).schema.clone());
        let (to_store, to_cart) = (set(&store), set(&cart));
        assert!(
            to_cart
                .patches
                .iter()
                .all(|p| p.target.template == schema(CART_PAGE)),
            "{to_cart:?}"
        );
        assert!(
            to_store
                .patches
                .iter()
                .all(|p| p.target.template == schema(s.store_page())),
            "{to_store:?}"
        );
        // The count, at the cart page's own part.
        let count = s
            .template_of(CART_PAGE)
            .manifest()
            .into_iter()
            .find(|e| e.value == "cart.line_count")
            .expect("the cart page's count")
            .id;
        assert!(
            to_cart.patches.iter().any(|p| p.target.part == count
                && matches!(&p.operation, PatchOp::ReplaceText { text } if text == "2")),
            "{to_cart:?}"
        );
        // And the speculated value to each, as each speculates (ADR-0191):
        // at the cart's own entry, the one its patches are derived from.
        // Since ADR-0303 each page speculates on its layout's binding of the
        // cart too, sent at an entry of its own (ADR-0222), so a value at any
        // entry did not say the page was sent its own: until 2026-10-11 a
        // host that sent the cart's to the store's documents alone passed.
        let values = |doc: &Doc| {
            let own = set(doc).basis.resources[0].entry.clone();
            s.pending.lock().expect("pending")[doc]
                .frames
                .iter()
                .filter(
                    |(_, f)| matches!(f, StreamFrame::EntryValue { entry, .. } if *entry == own),
                )
                .count()
        };
        assert_eq!((values(&store) > 0, values(&cart) > 0), (true, true));
    }

    /// **A page other than the store's is told nothing of its menu**
    /// (ADR-0190): a cart page's document has no menu, though its session
    /// reads store 47's.
    #[test]
    fn a_page_without_the_menu_is_told_nothing_of_it() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.serve_store_document("a", STORE_ID).expect("served");
        s.serve_document_settled("a", CART_PAGE, &store_params(STORE_ID), &[])
            .expect("served");
        let docs = documents_of(&s.pending.lock().expect("pending"), "a");
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".to_string(),
            name: "Espresso Doppio".to_string(),
        })
        .expect("the menu changes");
        let patched = |doc: &Doc| {
            s.pending.lock().expect("pending")[doc]
                .frames
                .iter()
                .any(|(_, f)| matches!(f, StreamFrame::Patch(_) | StreamFrame::PatchSet(_)))
        };
        assert!(patched(&docs[0]), "the store's page was not told");
        assert!(!patched(&docs[1]), "the cart page was told of the menu");
    }

    /// **A change to store 47's menu drops store 47's kept menu, and only
    /// it** (§15.6 test 11, ADR-0164). The change is `MenuChanged(47)`, and
    /// it reaches the entries whose queries declare `invalidates_on
    /// MenuChanged(id)`, for that id. Until 2026-10-03 the server dropped
    /// every store's kept menu, by the query's name.
    #[test]
    fn a_menu_change_drops_that_stores_kept_menu_only() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let reads = || calls(&s, "store:data/menus#sections");
        s.serve_store_document("a", STORE_ID).expect("served");
        s.serve_store_document("a", "48").expect("served");
        let read = reads();
        // Control: each is kept, so serving them again reads neither.
        s.serve_store_document("b", STORE_ID).expect("served");
        s.serve_store_document("b", "48").expect("served");
        assert_eq!(reads(), read);
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".to_string(),
            name: "Espresso Doppio".to_string(),
        })
        .expect("the menu changes");
        // Store 47's menu is read again, once, with the change: its fragment
        // is rendered from the query's answer, as a page's is (ADR-0169).
        assert_eq!(reads(), read + 1, "store 47's menu was not read again");
        // Store 48's menu is still kept ...
        s.serve_store_document("c", "48").expect("served");
        assert_eq!(
            reads(),
            read + 1,
            "store 47's change dropped store 48's menu"
        );
        // ... and so is store 47's new one, which shows the change.
        let (blue, _) = s.serve_store_document("c", STORE_ID).expect("served");
        assert_eq!(reads(), read + 1, "store 47's new menu was not kept");
        assert!(visible(&blue).contains("Espresso Doppio"), "{blue}");
    }

    /// **An entry a command invalidates is dropped by the key the command
    /// computed, and no other** (ADR-0209). Store 47's `Menu`, as a command
    /// hands it over: store 48's stays kept. Until ADR-0209 the server read a
    /// key's text, and anything but `current_session()` dropped every store's
    /// menu.
    #[test]
    fn an_invalidated_entry_is_dropped_by_the_key_the_command_computed() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let reads = || calls(&s, "store:data/menus#sections");
        s.serve_store_document("a", STORE_ID).expect("served");
        s.serve_store_document("a", "48").expect("served");
        let read = reads();
        s.invalidate_queries(
            "a",
            &[(
                "store.page.Menu".to_string(),
                vec![Some(Val::String(STORE_ID.into()))],
            )],
            &[],
        );
        s.serve_store_document("b", "48").expect("served");
        assert_eq!(reads(), read, "store 48's menu is kept");
        s.serve_store_document("b", STORE_ID).expect("served");
        assert_eq!(reads(), read + 1, "store 47's was dropped, and read again");
    }

    /// **A change reaches every part that reads it** (ADR-0168): renaming
    /// store 47's espresso sets its name's text and its Add button's name,
    /// where they are, in one patch set. Until 2026-10-03 a rename set the
    /// name's text alone, and the button stayed "Add Espresso".
    #[test]
    fn a_rename_sets_every_part_that_reads_the_name() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".to_string(),
            name: "Espresso Doppio".to_string(),
        })
        .expect("renamed");
        let queue = s.pending.lock().expect("pending");
        let set = queue[&("a".to_string(), document)]
            .frames
            .iter()
            .find_map(|(_, f)| match f {
                StreamFrame::PatchSet(set) => Some(set.clone()),
                _ => None,
            })
            .expect("one patch set for the rename");
        let operations: Vec<PatchOp> = set.patches.iter().map(|p| p.operation.clone()).collect();
        assert_eq!(
            operations,
            [
                PatchOp::ReplaceText {
                    text: "Espresso Doppio".to_string()
                },
                PatchOp::SetAttribute {
                    name: "aria-label".to_string(),
                    value: "Add Espresso Doppio".to_string()
                },
                // And what its Add captures, the item the page shows, which
                // `add_to_cart` makes its line from (ADR-0172): set where it
                // is, as the document writes it.
                PatchOp::SetAttribute {
                    name: "data-pw-captures".to_string(),
                    value: pw_render::escape::attribute(
                        &serde_json::json!({ "item": shown("espresso") })
                            .to_string()
                            .replace("\"Espresso\"", "\"Espresso Doppio\"")
                    ),
                },
            ]
        );
        // Each inside the espresso's own instance, which keeps its nodes,
        // inside its category's (ADR-0181).
        let espresso = menu_instance(&s, "espresso");
        assert!(
            set.patches.iter().all(
                |p| p.target.instances.len() == 2 && p.target.instances[1].instance == espresso
            ),
            "{:?}",
            set.patches
        );
    }

    /// Every patch operation queued for one document, in order: each patch
    /// alone and each set's.
    fn operations_for(s: &Server, session: &str, document: u64) -> Vec<Targeted> {
        let queue = s.pending.lock().expect("pending");
        queue[&(session.to_string(), document)]
            .frames
            .iter()
            .flat_map(|(_, f)| match f {
                StreamFrame::Patch(p) => vec![Targeted {
                    target: p.target.clone(),
                    operation: p.operation.clone(),
                }],
                StreamFrame::PatchSet(set) => set.patches.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// An item's instance in store 47's menu fragment, as the open pages
    /// address it.
    /// An item's instance in store 47's menu, inside its category's, as the
    /// open pages address it (ADR-0181): every item of store 47's is a
    /// coffee.
    fn menu_instance(s: &Server, id: &str) -> pw_document::InstanceToken {
        let (template, _) = s.menu_part();
        let (items, _) = each_over(&template.chunks, "section.items").expect("the items' loop");
        pw_render::instance_token_of(items, &key_row(id), "id", &coffee_env(s))
    }

    /// Inside store 47's coffee category: what its items' tokens are derived
    /// in.
    fn coffee_env(s: &Server) -> Env {
        let (template, part) = s.menu_part();
        let env = s.menu_env(STORE_ID, &Value::List(Vec::new()));
        let coffee = Value::Record(
            [(
                "category".to_string(),
                Value::Record([("id".to_string(), Value::Text("coffee".into()))].into()),
            )]
            .into(),
        );
        pw_render::instance_env(template, part, &coffee, &env).expect("a category's instance")
    }

    /// Where store 47's coffees are: the items' loop, inside the coffee
    /// category's instance (ADR-0181).
    fn coffees_at(s: &Server) -> PartAddress {
        let (template, part) = s.menu_part();
        let (items, _) = each_over(&template.chunks, "section.items").expect("the items' loop");
        let env = s.menu_env(STORE_ID, &Value::List(Vec::new()));
        let coffee = Value::Record(
            [(
                "category".to_string(),
                Value::Record([("id".to_string(), Value::Text("coffee".into()))].into()),
            )]
            .into(),
        );
        let token = pw_render::instance_token_of(part, &coffee, "category.id", &env);
        PartAddress::new(&s.menu_address().template, LocalPartId(items.0)).within(part, token)
    }

    /// The row an operation renders again, where it is one.
    fn rendered_row(op: &PatchOp) -> Option<&str> {
        match op {
            PatchOp::InsertAfter { html, .. } | PatchOp::InsertBefore { html, .. } => Some(html),
            _ => None,
        }
    }

    /// **A sold-out item is shown so, before the press** (ADR-0178, charter
    /// §15.1's `available`): its row says it is sold out, and has no Add to
    /// press. The others are as they were.
    #[test]
    fn a_sold_out_item_is_shown_so_and_has_no_add() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        s.stock_untold("cortado", false);
        let (html, _) = s.serve_store_document("a", STORE_ID).expect("served");
        let said = visible(&html)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            said.contains("Cortado Espresso cut with an equal part of warm milk. $4.25 Sold out"),
            "{said}"
        );
        assert!(!html.contains("aria-label=\"Add Cortado\""), "{html}");
        assert!(html.contains("aria-label=\"Add Espresso\""), "{html}");
        assert!(html.contains("aria-label=\"Add Cold Brew\""), "{html}");
        assert_eq!(said.matches("Sold out").count(), 1, "{said}");
    }

    /// **A stock change, told, reaches every page that shows the store**
    /// (ADR-0178, charter §15.2: availability is event invalidated). The
    /// item's row is rendered again where it is, since the block it shows
    /// decides otherwise, and no other row is touched. Back in stock, its
    /// Add comes back the same way.
    #[test]
    fn a_stock_change_told_renders_its_row_again_where_it_is() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        let (_, other) = s.serve_store_document("b", "48").expect("served");
        s.broadcast_menu(MenuOp::Stock {
            id: "cortado".to_string(),
            available: false,
        })
        .expect("told");
        let ops = operations_for(&s, "a", document);
        let (cortado, espresso) = (menu_instance(&s, "cortado"), menu_instance(&s, "espresso"));
        assert_eq!(ops.len(), 2, "{ops:?}");
        assert_eq!(
            ops[0].operation,
            PatchOp::RemoveInstance {
                instance: cortado.clone()
            }
        );
        let PatchOp::InsertAfter { instance, html } = &ops[1].operation else {
            panic!("rendered again after espresso: {ops:?}");
        };
        assert_eq!(instance.as_ref(), Some(&espresso));
        assert!(
            html.contains("Sold out") && !html.contains("<button"),
            "{html}"
        );
        // Both in the coffees' list, inside the coffee category (ADR-0181).
        assert!(ops.iter().all(|o| o.target == coffees_at(&s)), "{ops:?}");
        // Another store's page is not told of store 47's stock.
        assert!(operations_for(&s, "b", other).is_empty());
        // A document served now shows it, with no frame of its own.
        let (now, later) = s.serve_store_document("c", STORE_ID).expect("served");
        assert!(visible(&now).contains("Sold out"), "{now}");
        assert!(operations_for(&s, "c", later).is_empty());

        s.broadcast_menu(MenuOp::Stock {
            id: "cortado".to_string(),
            available: true,
        })
        .expect("told");
        let back = operations_for(&s, "c", later);
        assert_eq!(back.len(), 2, "{back:?}");
        assert_eq!(
            back[0].operation,
            PatchOp::RemoveInstance { instance: cortado }
        );
        let row = rendered_row(&back[1].operation).expect("rendered again");
        assert!(row.contains("aria-label=\"Add Cortado\""), "{row}");
        // An item no store has is refused, and nothing is sent.
        assert!(
            s.broadcast_menu(MenuOp::Stock {
                id: "matcha".to_string(),
                available: false,
            })
            .is_err()
        );
        assert_eq!(operations_for(&s, "c", later).len(), 2);
    }

    /// **Untold, a stock change is §15.5's forced stale item** (ADR-0157):
    /// a page shows the item as it was, and so does a document served while
    /// the menu kept is fresh. Once it is read again, a new document shows
    /// it, and the pages open are told first (ADR-0178): until then they
    /// kept the fragment's version before, while every change after was
    /// derived from the new one (ADR-0150).
    #[test]
    fn an_untold_stock_change_reaches_the_open_pages_when_the_menu_is_read_again() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        s.stock_untold("cortado", false);
        let (stale, _) = s.serve_store_document("b", STORE_ID).expect("served");
        assert!(stale.contains("aria-label=\"Add Cortado\""), "{stale}");
        assert!(operations_for(&s, "a", document).is_empty());
        // The menu's freshness spent: it is read again.
        s.queries.invalidate("store.page.Menu");
        let (fresh, _) = s.serve_store_document("c", STORE_ID).expect("served");
        assert!(!fresh.contains("aria-label=\"Add Cortado\""), "{fresh}");
        assert!(visible(&fresh).contains("Sold out"), "{fresh}");
        let told = operations_for(&s, "a", document);
        assert_eq!(told.len(), 2, "{told:?}");
        assert_eq!(
            told[0].operation,
            PatchOp::RemoveInstance {
                instance: menu_instance(&s, "cortado")
            }
        );
        assert!(rendered_row(&told[1].operation).is_some_and(|r| r.contains("Sold out")));
    }

    /// **A change tells the pages the whole difference** (ADR-0178): what
    /// changed at the source unannounced, and was read again with it, is
    /// sent with it. Until 2026-10-04 a rename sent its own row's parts, and
    /// a sold-out item drawn into the fragment with it kept its Add on the
    /// pages open.
    #[test]
    fn a_change_sends_what_changed_unannounced_with_it() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        s.stock_untold("cold-brew", false);
        s.broadcast_menu(MenuOp::Rename {
            id: "espresso".to_string(),
            name: "Espresso Doppio".to_string(),
        })
        .expect("renamed");
        let ops = operations_for(&s, "a", document);
        let espresso = menu_instance(&s, "espresso");
        let renamed: Vec<&Targeted> = ops
            .iter()
            .filter(|t| t.target.instances.last().map(|f| &f.instance) == Some(&espresso))
            .collect();
        assert!(
            renamed.iter().any(|t| t.operation
                == PatchOp::ReplaceText {
                    text: "Espresso Doppio".to_string()
                }),
            "{ops:?}"
        );
        assert!(
            ops.iter().any(|t| t.operation
                == PatchOp::RemoveInstance {
                    instance: menu_instance(&s, "cold-brew")
                }),
            "{ops:?}"
        );
        assert!(
            ops.iter()
                .filter_map(|t| rendered_row(&t.operation))
                .any(|r| r.contains("Cold Brew") && r.contains("Sold out")),
            "{ops:?}"
        );
    }

    /// **A structural change, and what changed unannounced with it, derived
    /// from the menu's values** (ADR-0178, ADR-0181): E7-P's insert is the
    /// difference between what the pages show and the menu now, as the
    /// sold-out espresso's row is, each in the coffees' list. Until
    /// 2026-10-04 an insert sent a patch E7-P made at its anchor, which a menu
    /// grouped by category has nowhere to address.
    #[test]
    fn an_insert_and_what_changed_with_it_are_derived_from_the_menu() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        s.stock_untold("espresso", false);
        s.broadcast_menu(MenuOp::Insert {
            id: "flat-white".to_string(),
            name: "Flat White".to_string(),
            at: Some("cortado".to_string()),
            before: true,
        })
        .expect("inserted");
        let ops = operations_for(&s, "a", document);
        assert_eq!(ops.len(), 4, "{ops:?}");
        // The espresso, sold out, rendered again where it is: first.
        assert_eq!(
            ops[0].operation,
            PatchOp::RemoveInstance {
                instance: menu_instance(&s, "espresso")
            }
        );
        let PatchOp::InsertBefore {
            instance: None,
            html,
        } = &ops[1].operation
        else {
            panic!("the espresso again, at the head: {ops:?}");
        };
        assert!(html.contains("Sold out"), "{html}");
        // Then the flat white, after it, which puts it before the cortado.
        let PatchOp::InsertAfter { instance, html } = &ops[2].operation else {
            panic!("the insert after the espresso: {ops:?}");
        };
        assert_eq!(instance.as_ref(), Some(&menu_instance(&s, "espresso")));
        assert!(html.contains("Flat White"), "{html}");
        assert!(
            ops[..3].iter().all(|o| o.target == coffees_at(&s)),
            "{ops:?}"
        );
        // And the menu counted, made again with the insert, after its list:
        // the page that reads it is told its text (ADR-0277).
        assert_eq!(
            ops[3].operation,
            PatchOp::ReplaceText {
                text: "4 items in 1 section".to_string()
            },
            "{ops:?}"
        );
    }

    /// **A stock change is `InventoryChanged`, not `MenuChanged`** (ADR-0178):
    /// it drops store 47's kept menu, which listens for it, and not its
    /// recommendations, which listen for the menu's own changes (ADR-0165)
    /// and are slow to ask for.
    #[test]
    fn a_stock_change_drops_the_menu_and_not_the_recommendations() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        recommend(&s, 0, None);
        for session in ["a", "b"] {
            estimate(&s, session, None, 25);
        }
        let page = |session: &str| -> String {
            fetched_as(&s, "/stores/47", Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let asks = || calls(&s, "store:data/recommendations#for-store");
        let menus = || calls(&s, "store:data/menus#sections");
        page("a");
        let (asked, read) = (asks(), menus());
        s.broadcast_menu(MenuOp::Stock {
            id: "cortado".to_string(),
            available: false,
        })
        .expect("told");
        let shown = page("b");
        assert!(visible(&shown).contains("Sold out"), "{shown}");
        assert_eq!(menus(), read + 1, "the menu was read again, once");
        assert_eq!(asks(), asked, "a stock change asked the recommender again");
    }

    /// **A document shows what the open pages show** (ADR-0178): read
    /// without the menu first brought to its query's value, as one read in
    /// the moment between the two would be, it is rendered from what they
    /// show, and the fragment is not rendered again behind their backs.
    /// Until 2026-10-04 it was (ADR-0150), and they kept the version before.
    #[test]
    fn a_document_read_behind_a_change_shows_what_the_open_pages_show() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        let version = s.menu_version(STORE_ID);
        s.stock_untold("cortado", false);
        s.queries.invalidate("store.page.Menu");
        // Read, not served: no refresh before it.
        let read = s.render_store("b");
        assert!(read.contains("aria-label=\"Add Cortado\""), "{read}");
        assert_eq!(s.menu_version(STORE_ID), version, "rendered again, untold");
        assert!(operations_for(&s, "a", document).is_empty());
        // Served, the pages are told first, and it shows the change.
        let (served, _) = s.serve_store_document("c", STORE_ID).expect("served");
        assert!(!served.contains("aria-label=\"Add Cortado\""), "{served}");
        assert!(s.menu_version(STORE_ID) > version);
        assert_eq!(operations_for(&s, "a", document).len(), 2);
    }

    /// **A store's menu is grouped by its category** (ADR-0181, charter
    /// §15.1): each category a heading and its items, in the order the store
    /// lists them. Store 47's coffees are one; store 48's drinks and its
    /// bakery are two, each item under its own.
    #[test]
    fn a_menu_is_grouped_by_its_category() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |id: &str| {
            let (html, _) = s.serve_store_document("a", id).expect("served");
            (
                visible(&html)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                html,
            )
        };
        let (blue, html) = page(STORE_ID);
        assert!(blue.contains("Coffee Espresso A double shot"), "{blue}");
        // One category's heading; the cart's has attributes of its own.
        assert_eq!(html.matches("<h2>").count(), 1, "one category");
        let (harbor, html) = page("48");
        assert_eq!(
            html.matches("<h2>").count(),
            2,
            "Drinks and Bakery: {harbor}"
        );
        let at = |text: &str| {
            harbor
                .find(text)
                .unwrap_or_else(|| panic!("no {text}: {harbor}"))
        };
        assert!(at("Drinks Drip Coffee") < at("Matcha Latte"));
        assert!(at("Matcha Latte") < at("Bakery Blueberry Scone"));
    }

    /// **Each menu row shows its price** (ADR-0169, charter §15.1): what the
    /// row reads of its item through `display`, which the member's own
    /// component computes for each row, each store's menu its own. Until
    /// 2026-10-03 a row could read no member of its item.
    #[test]
    fn each_menu_row_shows_its_price() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |id: &str| {
            let (html, _) = s.serve_store_document("a", id).expect("served");
            visible(&html)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let blue = page(STORE_ID);
        for said in [
            "Espresso A double shot, pulled short. $3.50",
            "Cortado Espresso cut with an equal part of warm milk. $4.25",
            "Cold Brew Steeped for eighteen hours and served over ice. $4.75",
        ] {
            assert!(blue.contains(said), "{said}: {blue}");
        }
        let harbor = page("48");
        assert!(
            harbor.contains("Drip Coffee Brewed to order, one cup at a time. $3.00"),
            "{harbor}"
        );
    }

    /// **An item E7-P inserts arrives with its price** (ADR-0169): its row
    /// is the `Menu` query's, read again after the change, with what the row
    /// reads through `display` computed for it.
    #[test]
    fn an_inserted_item_arrives_with_its_price() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let (_, document) = s.serve_store_document("a", STORE_ID).expect("served");
        s.broadcast_menu(MenuOp::Insert {
            id: "flat-white".to_string(),
            name: "Flat White".to_string(),
            at: None,
            before: false,
        })
        .expect("inserted");
        let queue = s.pending.lock().expect("pending");
        let html = queue[&("a".to_string(), document)]
            .frames
            .iter()
            .find_map(|(_, f)| match f {
                StreamFrame::Patch(p) => match &p.operation {
                    PatchOp::InsertAfter { html, .. } => Some(html.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("one insert");
        let shown = visible(&html)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(shown.starts_with("Flat White"), "{shown}");
        assert!(shown.contains("$4.00"), "{shown}");
    }

    /// **The store and each item say what they are** (ADR-0166, charter
    /// §15.1): the data layer's descriptions, read through the store's
    /// `Store` and `MenuItem`, each store its own. The benchmark's store,
    /// whose types declare none, is served from the same rows: its tests
    /// would fail if a host answer were not read through its types.
    #[test]
    fn the_store_and_each_item_say_what_they_are() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let page = |id: &str| {
            let (html, _) = s.serve_store_document("a", id).expect("served");
            visible(&html)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let blue = page(STORE_ID);
        for said in [
            "Small-batch coffee, served at the bar or carried out.",
            "Espresso A double shot, pulled short.",
            "Cortado Espresso cut with an equal part of warm milk.",
            "Cold Brew Steeped for eighteen hours and served over ice.",
        ] {
            assert!(blue.contains(said), "{said}: {blue}");
        }
        let harbor = page("48");
        for said in [
            "A neighborhood cafe by the water",
            "Drip Coffee Brewed to order, one cup at a time.",
        ] {
            assert!(harbor.contains(said), "{said}: {harbor}");
        }
        assert!(!harbor.contains("Small-batch"), "{harbor}");
    }

    /// [`slot_patch`], for a page not yet whole: `None` while the slot's
    /// section has not come to its end, and when the slot is not pending.
    fn pending_patch(page: &str, label: &str) -> Option<String> {
        let at = page.find(&format!("aria-label=\"{label}\""))?;
        let section = &page[at..at + page[at..].find("</section>")?];
        let name = &section[section.find("<?start name=\"")? + "<?start name=\"".len()..];
        Some(format!("<template for=\"{}\">", &name[..name.find('"')?]))
    }

    /// **The patch that fills a slot of the store** (ADR-0165): the opening
    /// of the `<template for>` its pending range names, if it is pending.
    fn slot_patch(page: &str, label: &str) -> Option<String> {
        let at = page
            .find(&format!("aria-label=\"{label}\""))
            .unwrap_or_else(|| panic!("no {label} slot: {page}"));
        let section = &page[at..at + page[at..].find("</section>").expect("its end")];
        let name = &section[section.find("<?start name=\"")? + "<?start name=\"".len()..];
        Some(format!("<template for=\"{}\">", &name[..name.find('"')?]))
    }

    /// **What a slot of the store shows** (ADR-0165), as a reader sees it:
    /// the arm its section holds in place, or, pending there, the arm its
    /// patch carries after the document.
    fn slot(page: &str, label: &str) -> String {
        let at = page
            .find(&format!("aria-label=\"{label}\""))
            .unwrap_or_else(|| panic!("no {label} slot: {page}"));
        let shown = match slot_patch(page, label) {
            Some(patch) => {
                let arm = &page[page.find(&patch).unwrap_or_else(|| panic!("no {patch}"))..];
                &arm[..arm.find("</template>").expect("the patch's end")]
            }
            None => {
                let open = page[..at].rfind("<section").expect("its section");
                &page[open..at + page[at..].find("</section>").expect("its end")]
            }
        };
        // Whatever is not a tag, its spaces made one.
        visible(shown)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// **A delivery estimate is a range, said in words** (ADR-0180, charter
    /// §15.1): its least and most minutes, as the estimator answers them, 10
    /// apart unless it says. Each is a `PositiveInt`, so an estimator's answer
    /// of 0 minutes breaks its invariant, which the host refuses (ADR-0179):
    /// the slot says the estimate is unavailable, and the page is served. The
    /// answer must carry when it was made, `generated_at`, or the read fails
    /// by name (ADR-0166).
    #[test]
    fn a_delivery_estimate_is_a_range() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        recommend(&s, 0, None);
        let page = |session: &str| -> String {
            fetched_as(&s, "/stores/47", Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        estimate(&s, "a", None, 20);
        assert_eq!(slot(&page("a"), "Delivery"), "Delivery in 20 to 30 min");
        s.store
            .faults()
            .estimators
            .lock()
            .expect("estimators")
            .insert(
                "b".to_string(),
                Estimator {
                    delay_ms: 0,
                    fail: None,
                    ..Estimator::default()
                },
            );
        s.set_estimate("b", 15, Some(45)).expect("estimated");
        assert_eq!(slot(&page("b"), "Delivery"), "Delivery in 15 to 45 min");
        estimate(&s, "c", None, 0);
        let refused = page("c");
        assert!(refused.starts_with("HTTP/1.1 200"), "{refused}");
        assert_eq!(slot(&refused, "Delivery"), "Delivery estimate unavailable");
    }

    /// **A session's response is kept by no cache, and a build's names no
    /// session** (ADR-0184, charter §15.6 tests 2 and 12). Until 2026-10-04
    /// the store's page said nothing of how it may be kept, and a fresh
    /// session's cookie went on whatever it asked for first: a file of the
    /// build a cache kept would have given that session to everyone it was
    /// served to.
    #[test]
    fn a_session_s_response_is_kept_by_no_cache_and_a_build_s_names_no_session() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        // The head of a response to a fresh session: one with no cookie.
        let head = |path: &str| -> String {
            let whole: String = fetched_as(&s, path, None)
                .into_iter()
                .map(|(_, c)| c)
                .collect();
            whole
                .split("\r\n\r\n")
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
        // The store's page, whole and streamed; the page that is not found.
        recommend(&s, 0, None);
        let whole = head("/stores/47");
        recommend(&s, 300, None);
        let streamed = head("/stores/47");
        let absent = head("/stores/999");
        for (what, head) in [
            ("whole", &whole),
            ("streamed", &streamed),
            ("absent", &absent),
        ] {
            assert!(
                head.contains("cache-control: private, no-store"),
                "{what}: {head}"
            );
            assert!(head.contains("set-cookie: pw-session="), "{what}: {head}");
        }
        // Files of the build, each asked for first by a fresh session.
        let handler = s
            .handler_identities()
            .into_iter()
            .next()
            .expect("a handler");
        for path in [
            "/templates.json".to_string(),
            format!("/handler/{handler}.mjs"),
        ] {
            let head = head(&path);
            assert!(head.starts_with("http/1.1 200"), "{path}: {head}");
            assert!(
                !head.contains("set-cookie"),
                "{path} names a session: {head}"
            );
            assert!(!head.contains("private"), "{path}: {head}");
        }
    }

    /// **What a shared cache keeps holds nothing a session put in it**
    /// (ADR-0184, charter §15.6 tests 2 and 13). Two sessions fill their
    /// carts and read their pages. What the query runtime keeps for every
    /// reader, and each fragment the materializer keeps in its public
    /// partition, is then what it is when nobody pressed anything: it names
    /// no session, holds no line of a cart, and holds no deployment key.
    #[test]
    fn what_a_shared_cache_keeps_holds_nothing_a_session_put_in_it() {
        let (alice, bob) = ("session-alice-shared-output", "session-bob-shared-output");
        let kept = |pressed: bool| -> (Vec<String>, Vec<String>) {
            let mut s = served_from_patches_in("examples", |app| app.to_string(), &[]);
            // Both hosts keep the same instant, noon in the stores' zone
            // (America/Los_Angeles) on 2026-10-21, where each took the wall
            // clock when it was built: minutes apart, and a store's status
            // ("open until ..") is the next local midnight's, so a pair that
            // straddled midnight there kept two values (found by this
            // track's run at 07:00Z, 2026-10-20).
            s.epoch = 1_792_609_200_000;
            recommend(&s, 0, None);
            for session in [alice, bob] {
                for store in [STORE_ID, "48"] {
                    s.serve_store_document(session, store).expect("served");
                }
            }
            if pressed {
                s.command(ADD, alice, &add_shown("espresso", 2), false)
                    .expect("added");
                s.command(ADD, bob, &add_shown("cold-brew", 1), false)
                    .expect("added");
                for session in [alice, bob] {
                    s.serve_store_document(session, STORE_ID).expect("served");
                }
            }
            let mut values: Vec<String> = s
                .queries
                .public_cache_contents()
                .into_iter()
                .map(|(key, value)| format!("{key:?} = {value:?}"))
                .collect();
            values.sort();
            let mut fragments: Vec<String> = s
                .materializer
                .all_entries()
                .into_iter()
                .filter(|(key, _)| key.contains("partition=public"))
                .map(|(key, entry)| format!("{key} = {}", entry.body))
                .collect();
            fragments.sort();
            (values, fragments)
        };
        let (values, fragments) = kept(true);
        assert!(
            !values.is_empty(),
            "the query runtime keeps the store's values"
        );
        assert!(!fragments.is_empty(), "the materializer keeps the menu");
        assert_eq!(kept(false), (values.clone(), fragments.clone()));
        let deployment_key = pw_resource::entry::DEVELOPMENT_KEY;
        for kept in values.iter().chain(&fragments) {
            for secret in [alice, bob, deployment_key] {
                assert!(
                    !kept.contains(secret),
                    "{secret} is kept for every reader: {kept}"
                );
            }
        }
    }

    /// **A store's page is titled by its name** (ADR-0183): the page states
    /// its title, `<title>{store.name}</title>`, and the host writes it into
    /// the document's head from the page's values. Until 2026-10-04 every
    /// store's page was "Store" (WCAG 2.4.2, F25).
    #[test]
    fn a_store_s_page_is_titled_by_its_name() {
        let title_of = |s: &Server, path: &str| -> String {
            let whole: String = fetched_as(s, path, Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect();
            whole
                .split("<title>")
                .nth(1)
                .and_then(|t| t.split("</title>").next())
                .unwrap_or_else(|| panic!("no title: {whole}"))
                .to_string()
        };
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        recommend(&s, 0, None);
        assert_eq!(title_of(&s, "/stores/47"), "Blue Bottle");
        assert_eq!(title_of(&s, "/stores/48"), "Harbor Coffee");
        // Escaped where it is written: a title is text.
        let named = served_from_patches_in(
            "examples",
            |app| {
                app.replace(
                    "<title>{store.name}</title>",
                    "<title>{store.name} & more</title>",
                )
            },
            &[],
        );
        recommend(&named, 0, None);
        assert_eq!(title_of(&named, "/stores/47"), "Blue Bottle &amp; more");
        // Control: a store's program that states none, the benchmark's copy,
        // is titled as every store's page was.
        let untitled = served_from(|app| app.to_string(), None);
        assert_eq!(title_of(&untitled, "/StorePage.html"), "Store");
    }

    /// **A store's page describes itself** (ADR-0186): its description, from
    /// its values, in the document's head, for a search engine's result and a
    /// link's preview, and none of it in the body.
    #[test]
    fn a_store_s_page_describes_itself_in_its_head() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        recommend(&s, 0, None);
        let page = |path: &str| -> String {
            fetched_as(&s, path, Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        for store in [STORE_ID, SECOND_STORE.0] {
            let whole = page(&format!("/stores/{store}"));
            let (head, body) = whole.split_once("</head>").expect("a head");
            let meta = format!(
                "<meta name=\"description\" content=\"{}\">",
                pw_render::escape::attribute(store_description(store))
            );
            assert!(head.contains(&meta), "{store}: {head}");
            assert!(!body.contains("<meta"), "{store}: {body}");
        }
    }

    /// **A page states its canonical address** (ADR-0311): the store's
    /// page, given `<link rel="canonical" href={StorePage(id)} />`, is served
    /// with it in its head, absolute with the deployment's origin and never
    /// the request's `Host`, Open Graph's `og:url` from it, and nothing of
    /// either in its body. A host with no origin cannot write it, and does
    /// not serve the page.
    #[test]
    fn a_page_s_canonical_address_is_written_with_the_deployment_s_origin() {
        fn stated(app: &str) -> String {
            const DESCRIBED: &str =
                "        <meta name=\"description\" content={store.description} />\n";
            assert_eq!(app.matches(DESCRIBED).count(), 1, "the page's description");
            app.replace(
                DESCRIBED,
                &format!("{DESCRIBED}        <link rel=\"canonical\" href={{StorePage(id)}} />\n"),
            )
        }
        let page = |s: &Server| -> String {
            fetched_as(s, &format!("/stores/{STORE_ID}"), Some("a"))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let s = served_from_patches_in("examples", stated, &[]);
        assert!(s.states_canonical());
        recommend(&s, 0, None);
        s.origin
            .set("https://store.example".to_string())
            .expect("set once");
        let whole = page(&s);
        let (head, body) = whole.split_once("</head>").expect("a head");
        let address = format!("https://store.example/stores/{STORE_ID}");
        assert!(
            head.contains(&format!(
                "<link rel=\"canonical\" href=\"{address}\">\n\
                 <meta property=\"og:url\" content=\"{address}\">\n"
            )),
            "{head}"
        );
        // The request said `Host: t`, which its client chose.
        assert!(!whole.contains("//t/"), "{head}");
        // Neither element in the body, where only the parts manifest names
        // the part.
        assert!(
            !body.contains("<link rel=\"canonical\"") && !body.contains("property=\"og:url\""),
            "{body}"
        );
        // With no origin, the page is not shown rather than shown with an
        // address made up.
        let without = served_from_patches_in("examples", stated, &[]);
        recommend(&without, 0, None);
        let refused = page(&without);
        assert!(refused.starts_with("HTTP/1.1 503"), "{refused}");
        assert!(refused.contains("its canonical address"), "{refused}");
        // The control: the store as it is states none.
        let plain = served_from_patches_in("examples", |app| app.to_string(), &[]);
        assert!(!plain.states_canonical());
        // At start: a production deployment whose pages state one is given
        // its stated origin, and refused without one; a development one has
        // this machine's; and one whose pages state none needs none.
        let at = |development: bool| identity::Deployment {
            development,
            origin: "https://store.example".to_string(),
        };
        assert_eq!(
            canonical_origin(&s, &at(false), true).as_deref(),
            Ok("https://store.example")
        );
        assert!(
            canonical_origin(&s, &at(false), false)
                .is_err_and(|e| e.contains("PW_ORIGIN states no public origin"))
        );
        assert!(canonical_origin(&s, &at(true), false).is_ok());
        assert!(canonical_origin(&plain, &at(false), false).is_ok());
    }

    /// **A page that binds no query describes itself in its head too**
    /// (ADR-0186): its title and its metadata, escaped as its renderer
    /// wrote them, and nothing of either in its body.
    #[test]
    fn a_signal_page_s_head_holds_its_title_and_metadata() {
        let template = Template {
            path: "t.P".into(),
            name: "P".into(),
            params: vec![],
            schema: "s".into(),
            chunks: vec![],
        };
        let page = signal_document(
            "<main><h1>Panel</h1></main>",
            "Panel & co",
            "<meta name=\"description\" content=\"A panel.\">\n",
            &template,
            &serde_json::json!({}),
            &[],
            COMMITTED_ARTIFACTS,
        );
        let (head, body) = page.split_once("</head>").expect("a head");
        assert!(
            head.ends_with(
                "<title>Panel &amp; co</title>\n<meta name=\"description\" content=\"A panel.\">\n"
            ),
            "{head}"
        );
        assert!(
            !body.contains("<meta") && !body.contains("<title"),
            "{body}"
        );
    }

    /// **A page that binds no query holds the announcer too** (ADR-0302):
    /// once, empty, before the runtime that says what a failed press is told.
    #[test]
    fn a_signal_page_holds_its_announcer() {
        let template = Template {
            path: "t.P".into(),
            name: "P".into(),
            params: vec![],
            schema: "s".into(),
            chunks: vec![],
        };
        let page = signal_document(
            "<main></main>",
            "P",
            "",
            &template,
            &serde_json::json!({}),
            &[],
            COMMITTED_ARTIFACTS,
        );
        assert_eq!(page.matches(pw_render::ANNOUNCER).count(), 1, "{page}");
        let (before, _) = page
            .split_once("id=\"pw-parts\"")
            .expect("a parts manifest");
        assert!(before.contains(pw_render::ANNOUNCER), "{page}");
    }

    /// **A title that changed is set as text** (ADR-0183), at the title's
    /// address, which the browser sets as `document.title`. One that did not
    /// change is not set.
    #[test]
    fn a_title_that_changed_is_set_as_its_text() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        let id = s.plan["title"]
            .as_u64()
            .expect("the plan names the page's title") as u32;
        let shown = |title: &str| Shown {
            title: Some((id, title.to_string())),
            ..Shown::default()
        };
        // A document of the store's, which gives its page an `id`.
        s.serve_store_document("a", STORE_ID).expect("served");
        let doc = documents_of(&s.pending.lock().expect("pending"), "a")
            .pop()
            .expect("a document");
        let bindings = s.bindings("a", doc.1).expect("read");
        let changed = s
            .derive(
                s.store_page(),
                "a",
                &store_params(STORE_ID),
                &bindings,
                &shown("Blue Bottle"),
                &shown("Blue Bottle Coffee"),
            )
            .expect("derived");
        assert_eq!(
            changed,
            [Targeted {
                target: PartAddress::new(
                    &TemplateSchemaId(s.store_template().schema.clone()),
                    LocalPartId(id)
                ),
                operation: PatchOp::ReplaceText {
                    text: "Blue Bottle Coffee".to_string()
                },
            }]
        );
        let same = s
            .derive(
                s.store_page(),
                "a",
                &store_params(STORE_ID),
                &bindings,
                &shown("Blue Bottle"),
                &shown("Blue Bottle"),
            )
            .expect("derived");
        assert!(same.is_empty(), "{same:?}");
    }

    /// **The store's slots come after its own content, in the same
    /// response** (charter §15.3, §15.6 test 3, ADR-0165): its name, menu
    /// and cart, with each slot pending, and then each slot's arm as its
    /// query answers, the estimate's first.
    #[test]
    fn the_store_sends_its_slots_after_its_own_content() {
        // Each slot's source is held at a gate of its own, and let go when the
        // response shows what it waits for: the document first, the estimate
        // after it, the recommendations after that. What the response carries
        // at each step is a fact the test sets, where delays of 200 and 600
        // ms, and a bound of 150 on the document, were silences a slow
        // machine made too (the budgets are a minute: they are not the
        // subject).
        let s = served_from_patches_in("examples", patient, &[]);
        let recommendations = hold_recommender(&s);
        let estimate_gate = Arc::new(Gate::default());
        s.store
            .faults()
            .estimators
            .lock()
            .expect("estimators")
            .insert(
                "a".to_string(),
                Estimator {
                    delay_ms: 0,
                    fail: None,
                    gate: Some(estimate_gate.clone()),
                },
            );
        s.set_estimate("a", 30, None).expect("estimated");
        let own = [
            "Blue Bottle",
            "Cold Brew",
            "id=\"cart-count\"",
            "Estimating delivery",
            "Finding recommendations",
        ];
        // What had come when each source was let go, and each arm's patch.
        let mut document: Option<(String, String, String)> = None;
        let mut estimate_came: Option<String> = None;
        let chunks = fetched_stepwise(&s, "/stores/47", Some("a"), |so_far, _| {
            if document.is_none()
                && own.iter().all(|text| so_far.contains(text))
                && let (Some(delivery), Some(recs)) = (
                    pending_patch(so_far, "Delivery"),
                    pending_patch(so_far, "Recommendations"),
                )
            {
                document = Some((so_far.to_string(), delivery, recs));
                estimate_gate.open();
            }
            if let Some((_, delivery, _)) = &document
                && estimate_came.is_none()
                && so_far.contains(delivery)
            {
                estimate_came = Some(so_far.to_string());
                recommendations.open();
            }
        });
        let whole: String = chunks.iter().map(|(_, c)| c.as_str()).collect();
        // The store's own content, and both slots pending, at once: with the
        // document heard, neither source had been let go, so neither arm
        // could have come with it.
        let (shell, delivery, recommended) = document.expect("the document came");
        assert!(
            !shell.contains(&delivery) && !shell.contains(&recommended),
            "an arm came with the document: {shell}"
        );
        // Then each arm, as its query answers: the estimate, with the
        // recommendations still held, and then the recommendations.
        let estimate_came = estimate_came.expect("the estimate's arm came");
        assert!(
            !estimate_came.contains(&recommended),
            "the recommendations came with the estimate, held"
        );
        assert!(
            whole.contains(&recommended),
            "the recommendations' arm came after"
        );
        // A range, in words (ADR-0180).
        assert_eq!(slot(&whole, "Delivery"), "Delivery in 30 to 40 min");
        assert_eq!(slot(&whole, "Recommendations"), "Cortado Cold Brew");
        // Each arm with the comment after it (ADR-0223).
        assert!(
            // ADR-0277: the menu counted is a part of the page's, before
            // (ADR-0307's four cart blocks are not this store's: its cart
            // controls ask nothing, track `store-accounts`.)
            // them; and ADR-0303: the layout's count is part 0, before all.
            // Track `store-accounts`: and where deliveries go, before them.
            // ADR-0323: and the cart's link, a block and the subtotal it
            // says while the cart holds a line. ADR-0308, amended: and the
            // store's cuisine; track `store-hours`: and whether it is open;
            // track `reviews`: and what its ratings come to.
            whole.ends_with("</template><!--/pw-43--></body>\n</html>\n"),
            "{whole}"
        );
        // An estimator that is down fills its slot with the failure, and the
        // page is served.
        estimate(&s, "b", Some("down"), 0);
        let failed: String = fetched_as(&s, "/stores/47", Some("b"))
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert!(failed.starts_with("HTTP/1.1 200 OK\r\n"), "{failed}");
        assert_eq!(slot(&failed, "Delivery"), "Delivery estimate unavailable");
    }

    /// **A store's recommendations are its own, kept ten minutes, and
    /// dropped when its menu changes** (ADR-0165, ADR-0164). The change is
    /// `MenuChanged(47)`, and it reaches the kept answer of a stream's query
    /// as it reaches a `let`'s: store 47's recommendations are asked again,
    /// and store 48's are kept.
    #[test]
    fn a_menu_change_drops_that_stores_recommendations_only() {
        let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
        recommend(&s, 0, None);
        for session in ["a", "b", "c"] {
            estimate(&s, session, None, 25);
        }
        let page = |path: &str, session: &str| -> String {
            fetched_as(&s, path, Some(session))
                .into_iter()
                .map(|(_, c)| c)
                .collect()
        };
        let asks = || calls(&s, "store:data/recommendations#for-store");
        // Each store's recommendations are drawn from its own menu.
        assert_eq!(
            slot(&page("/stores/47", "a"), "Recommendations"),
            "Cortado Cold Brew"
        );
        assert_eq!(
            slot(&page("/stores/48", "a"), "Recommendations"),
            "Matcha Latte Blueberry Scone"
        );
        let asked = asks();
        assert_eq!(asked, 2);
        // Control: both are kept, so neither is asked again.
        page("/stores/47", "b");
        page("/stores/48", "b");
        assert_eq!(asks(), asked);
        s.broadcast_menu(MenuOp::Rename {
            id: "cortado".to_string(),
            name: "Cortado Grande".to_string(),
        })
        .expect("the menu changes");
        // Store 48's are still kept ...
        page("/stores/48", "c");
        assert_eq!(
            asks(),
            asked,
            "store 47's change dropped store 48's recommendations"
        );
        // ... and store 47's are asked again, and name the change.
        let again = slot(&page("/stores/47", "c"), "Recommendations");
        assert_eq!(
            asks(),
            asked + 1,
            "store 47's recommendations were not asked again"
        );
        assert_eq!(again, "Cortado Grande Cold Brew");
    }

    /// **A document is its own subscriber** (ADR-0161). Two tabs in one
    /// session: the first is served, a change reaches it, and before its page
    /// asks for it the second is served. The first page is still sent the
    /// change.
    #[test]
    fn a_change_waiting_for_one_tab_survives_another_tab_being_served() {
        let s = rendering_server();
        let session = "two-tabs";
        let (_, first) = s.serve_document(session);
        s.command(ADD, session, &add_shown("espresso", 1), false)
            .expect("runs");
        let (_, _second) = s.serve_document(session);
        let queue = s.pending.lock().expect("pending");
        let (_, frames) = queue[&(session.to_string(), first)].after(first);
        assert!(!frames.is_empty(), "the first tab was never sent the add");
    }

    /// A change to the session's cart reaches every document of the session
    /// (ADR-0161), each patched against what it shows.
    #[test]
    fn a_change_reaches_every_document_of_its_session() {
        let s = rendering_server();
        let session = "both-tabs";
        let (_, first) = s.serve_document(session);
        let (_, second) = s.serve_document(session);
        s.command(ADD, session, &add_shown("espresso", 1), false)
            .expect("runs");
        let queue = s.pending.lock().expect("pending");
        for document in [first, second] {
            let frames = &queue[&(session.to_string(), document)].frames;
            assert!(
                frames.iter().any(
                    |(_, f)| matches!(f, StreamFrame::PatchSet(set) if !set.patches.is_empty())
                ),
                "document {document} was not patched: {frames:?}"
            );
        }
        // Control: another session's page hears nothing.
        drop(queue);
        let (_, other) = s.serve_document("elsewhere");
        s.command(ADD, session, &add_shown("espresso", 1), false)
            .expect("runs");
        assert!(
            s.pending.lock().expect("pending")[&("elsewhere".to_string(), other)]
                .frames
                .is_empty()
        );
    }

    /// **A session is forgotten with its last document** (ADR-0161): its cart
    /// entry and its interactions stay while another of its pages is open.
    #[test]
    fn a_session_is_forgotten_with_its_last_document() {
        let now = std::time::Instant::now();
        let idle = Subscriber {
            seen: now - IDLE - std::time::Duration::from_secs(1),
            ..Subscriber::at(1)
        };
        let mut queue: BTreeMap<Doc, Subscriber> = BTreeMap::new();
        queue.insert(("s".to_string(), 1), idle);
        queue.insert(("s".to_string(), 2), Subscriber::at(2));
        let (forgotten, gone) = forget_idle(&mut queue, now);
        assert_eq!(forgotten, [("s".to_string(), 1)]);
        assert!(gone.is_empty(), "{gone:?}");
        // Its last page idle too, the session goes with it.
        let later = now + IDLE + std::time::Duration::from_secs(1);
        let (forgotten, gone) = forget_idle(&mut queue, later);
        assert_eq!(forgotten, [("s".to_string(), 2)]);
        assert_eq!(gone, ["s"]);
    }

    /// The other half (ADR-0139): what a page's stream request says it
    /// applied is dropped, so a live page's queue does not grow.
    #[test]
    fn a_stream_drops_what_its_page_says_it_applied() {
        let s = rendering_server();
        let session = "applied";
        let (_, document) = s.serve_document(session);
        let doc: Doc = (session.to_string(), document);
        s.command(ADD, session, &add_shown("espresso", 1), false)
            .expect("runs");
        let applied = s.pending.lock().expect("pending")[&doc].last_seq;
        assert!(
            s.pending.lock().expect("pending")[&doc]
                .frames
                .iter()
                .any(|(n, _)| *n <= applied),
            "the change queued frames"
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let _page = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (mut held, _) = listener.accept().expect("accept");
        std::thread::scope(|scope| {
            scope.spawn(|| stream_open(&s, &mut held, session, false, document, applied));
            // The stream has opened, and acknowledged what the page applied,
            // when its trail says so.
            until("the stream open", || {
                trail_says(&s, &doc, "stream opened since")
            });
            let queue = s.pending.lock().expect("pending");
            assert!(
                !queue[&doc].frames.iter().any(|(n, _)| *n <= applied),
                "what the page applied is still queued"
            );
        });
    }

    /// The parts manifest a document carries.
    fn manifest_of(html: &str) -> serde_json::Value {
        let json = html
            .split("<script type=\"application/json\" id=\"pw-parts\">")
            .nth(1)
            .and_then(|r| r.split("</script>").next())
            .expect("a manifest");
        serde_json::from_str(json).expect("JSON")
    }

    /// **A page that reads queries holds signals too** (ADR-0140). The
    /// store's document said nothing of signals until 2026-10-02, so a
    /// signal on the store's page rendered nowhere the browser could change
    /// it: T11's dialog could not be written on the store.
    #[test]
    fn the_store_s_document_carries_its_signals() {
        let template: Template = serde_json::from_value(serde_json::json!({
            "path": "store.page.StorePage~ui",
            "name": "StorePage",
            "params": [],
            "schema": "s",
            "chunks": [
                { "chunk": "static", "value": "<main>" },
                { "chunk": "dynamic", "value": {
                    "part": "conditional", "id": 0, "value": "open",
                    "then": [{ "chunk": "static", "value": "<p id=\"shown\">open</p>" }],
                    "otherwise": []
                } },
                { "chunk": "static", "value": "</main>" }
            ]
        }))
        .expect("a template");
        let plan = serde_json::json!({
            "signals": [{ "name": "open", "initial": true }],
            "live": [{ "part": 0, "signal": "open", "path": "open", "kind": "conditional",
                       "reads": ["open"] }]
        });
        // Rendered at its first value ...
        let body =
            pw_render::render(&template, &with_signals(Env::new(), &plan), &[]).expect("renders");
        assert!(body.contains("<p id=\"shown\">open</p>"), "{body}");
        // ... and the document says what the browser holds and renders again.
        let templates = vec![template];
        let manifest = manifest_of(&document(
            &body,
            "Store",
            "",
            "",
            &templates[0],
            &templates,
            &plan,
            3,
            None,
            &BTreeMap::new(),
            COMMITTED_ARTIFACTS,
        ));
        assert_eq!(manifest["signals"], serde_json::json!({ "open": true }));
        assert_eq!(manifest["live"][0]["part"], 0);
        assert!(manifest["blocks"]["0"]["then"].is_array(), "{manifest}");
        // Control: a page with no signals carries none of it.
        let manifest = manifest_of(&document(
            &body,
            "Store",
            "",
            "",
            &templates[0],
            &templates,
            &serde_json::json!({}),
            3,
            None,
            &BTreeMap::new(),
            COMMITTED_ARTIFACTS,
        ));
        assert!(manifest.get("signals").is_none(), "{manifest}");
    }

    /// The store's page renders a block its signal decides at the signal's
    /// first value (ADR-0140): until 2026-10-02 nothing gave the renderer a
    /// signal's value on the store's route, and the page did not render.
    #[test]
    fn the_store_renders_its_signals_at_their_first_values() {
        let mut templates: Vec<Template> =
            serde_json::from_str(include_str!("../../store-ir.json")).expect("the template IR");
        // The store's, by its name: the IR holds the cart's page too
        // (ADR-0190).
        let store = templates
            .iter()
            .position(|t| t.name == "StorePage")
            .expect("the store's template");
        templates[store].chunks.push(
            serde_json::from_value(serde_json::json!({ "chunk": "dynamic", "value": {
                "part": "conditional", "id": 99, "value": "open",
                "then": [{ "chunk": "static", "value": "<p id=\"shown\">open</p>" }],
                "otherwise": []
            } }))
            .expect("a chunk"),
        );
        let mut s = Server::on(std::path::PathBuf::from("."), templates, dev_topology());
        // The store's own signals, and one more the added part reads.
        let with_open = |s: &mut Server, initial: bool| {
            let mut signals = s.plan["signals"].as_array().cloned().unwrap_or_default();
            signals.retain(|signal| signal["name"] != "open");
            signals.push(serde_json::json!({ "name": "open", "initial": initial }));
            s.plan["signals"] = serde_json::Value::Array(signals);
        };
        with_open(&mut s, true);
        let html = s.render_store("first-shown");
        assert!(html.contains("<p id=\"shown\">open</p>"), "{html}");
        // Control: shut at its first value, and not shown.
        with_open(&mut s, false);
        assert!(!s.render_store("first-shut").contains("id=\"shown\""));
    }

    /// **A kept answer is the answer** (ADR-0121, ADR-0157): a retried
    /// request is given exactly what the first was, including the
    /// difference between answering nothing, `null`, and answering no value.
    #[test]
    fn a_kept_answer_is_the_answer_it_kept() {
        for answered in [
            Answered {
                committed: true,
                result: Some(serde_json::json!({ "$case": "ok" })),
                why: None,
                refused: None,
            },
            Answered {
                committed: false,
                result: Some(serde_json::json!({
                    "$case": "err",
                    "value": { "$case": "item-unavailable", "value": "cortado" }
                })),
                why: None,
                refused: None,
            },
            // A command declaring no `Result`: answered, with nothing.
            Answered {
                committed: true,
                result: Some(serde_json::Value::Null),
                why: None,
                refused: None,
            },
            // A command that trapped: no answer at all.
            Answered {
                committed: false,
                result: None,
                why: Some("trapped".to_string()),
                refused: None,
            },
        ] {
            assert_eq!(Answered::from_kept(&answered.kept()), answered);
        }
    }

    /// A page's parameter, as the address carries it (ADR-0136).
    #[test]
    fn a_parameter_is_the_text_the_address_carries() {
        assert_eq!(percent_decoded("cold-brew").as_deref(), Some("cold-brew"));
        assert_eq!(
            percent_decoded("flat%20white").as_deref(),
            Some("flat white")
        );
        assert_eq!(percent_decoded("flat+white").as_deref(), Some("flat white"));
        assert_eq!(percent_decoded("%E2%82%AC").as_deref(), Some("\u{20ac}"));
        assert_eq!(percent_decoded("%3Cb%3E").as_deref(), Some("<b>"));
        // Not an escape, and not text: refused rather than passed through.
        assert_eq!(percent_decoded("%zz"), None);
        assert_eq!(percent_decoded("%4"), None);
        assert_eq!(percent_decoded("%FF"), None);
    }
}
