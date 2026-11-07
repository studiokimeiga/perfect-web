//! ADR-0389, amended 2026-11-02: **two requests of one session whose
//! admissions overlap are each answered**. A browser asks for a page, its
//! runtime, its handler manifest and its handler modules at once, and sends a
//! command while they load; each request is admitted, reading its session as
//! the hosts keep it. Until then the second admission of one session to end
//! was refused `Superseded`: a page or a module was answered 503 "The session
//! could not be confirmed", and a command 403 `session-changed`.
//!
//! Three sources of sessions: this host's memory, which reads nothing outside
//! its lock (a guard); a source in this process whose reads meet in pairs, so
//! that two admissions overlap whatever the scheduler does; and PostgreSQL,
//! where the two reads meet after they read (a test's step in
//! `SessionsPg::session`), so that both read before either ends.
//!
//! And, from the repair's review: **on PostgreSQL an admission takes no
//! session lock**. A command holds its session's advisory lock while it
//! runs, a payment's outbound call included; until then each admission of
//! that session took the same lock, waited up to its 5 s timeout, and held
//! one of the host's eight session connections meanwhile, so another
//! reader's page waited for a connection. Now an admission is one statement
//! that waits on no session lock, and a write it finds due (a use recorded,
//! an expired row deleted) is made only where nobody holds the lock. The
//! tests hold the lock as a command does (`hosts::command_session_lock`).
//!
//! And, from the first press (its ADR, amended 2026-11-06): **a host that
//! keeps sessions warms**, and a warm-up's requests, sent together, are each
//! answered. Six requests of one session read it at once, in this process
//! and on PostgreSQL, where one of them is a press.
//!
//! The PostgreSQL tests pass doing nothing without a database:
//! `just e14-session-overlap` refuses to run without one.

use super::second_host::{Schema, database, feed_host_signing_in};
use super::sign_in::{TestProvider, command, signed_in};
use super::*;
use crate::identity::Kept as _;
use crate::identity::tests::Meeting;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The runtime's files, as a build ships them, so that they are answered.
fn with_runtime_files(s: &Server) {
    for (file, body) in [
        ("pw-runtime.mjs", "export {};"),
        ("pw-resume.wasm", "resume"),
        ("pw-render.wasm", "render"),
    ] {
        std::fs::write(s.dist.join(file), body).expect("runtime file written");
    }
}

/// **A request a browser sends while a page loads**, and what it is
/// answered when nothing overlaps it.
#[derive(Clone, Debug)]
enum Ask {
    Get(String),
    /// A post of the feed, by its interaction.
    Post(String),
}

impl Ask {
    /// Every kind of request admission reads the session for: a page, the
    /// handler manifest, a handler module, each of the runtime's files (the
    /// next document's runtime with its query too), and a command.
    fn every(s: &Server) -> Vec<Ask> {
        let handler = s
            .handler_identities()
            .into_iter()
            .next()
            .expect("the feed has a handler");
        [
            "/",
            "/pw-handlers",
            &format!("/handler/{handler}.mjs"),
            "/pw-runtime.mjs",
            "/pw-runtime.mjs?document=1",
            "/pw-resume.wasm",
            "/pw-render.wasm",
        ]
        .into_iter()
        .map(|path| Ask::Get(path.to_string()))
        .chain([Ask::Post("overlap".into())])
        .collect()
    }

    /// Its answer, sent as `session` by HTTP; `n` tells posts apart.
    fn sent(&self, s: &Server, session: &str, n: usize) -> String {
        match self {
            Ask::Get(path) => get_as(s, session, path),
            Ask::Post(interaction) => command(
                s,
                "feed.app.post",
                session,
                &format!("{interaction}-{n}"),
                &serde_json::json!([format!("Posted while a page loaded, {n}")]).to_string(),
            ),
        }
    }

    /// Answered as it is when nothing overlaps it: a page or a file, 200; a
    /// post, 202 and committed.
    fn answered(&self, answer: &str) -> bool {
        match self {
            Ask::Get(_) => answer.starts_with("HTTP/1.1 200 OK"),
            Ask::Post(_) => {
                answer.starts_with("HTTP/1.1 202") && answer.contains("\"committed\":true")
            }
        }
    }
}

/// **A request admitted while another of its session is under way, in this
/// host's memory**: the page is held after its admission until the other
/// request is answered, and each is answered as it is alone. Memory reads
/// nothing outside its lock, so this held before the repair too; it keeps
/// holding (a guard, not the regression).
#[test]
fn in_memory_a_request_admitted_while_another_is_under_way_is_answered() {
    let s = served_feed();
    with_runtime_files(&s);
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    for (n, other) in Ask::every(&s).into_iter().enumerate() {
        let (admitted, page_admitted) = mpsc::channel();
        let (answered, other_answered) = mpsc::channel::<()>();
        *s.after_page_admission.lock().expect("page admission hook") = Some(Box::new(move |_| {
            admitted.send(()).expect("the page told it is admitted");
            other_answered
                .recv_timeout(Duration::from_secs(30))
                .expect("the other request answered while the page waited");
        }));
        let (page, theirs) = std::thread::scope(|scope| {
            let page = scope.spawn(|| get_as(&s, "reader", "/"));
            page_admitted
                .recv_timeout(Duration::from_secs(30))
                .expect("the page admitted");
            let theirs = other.sent(&s, "reader", n);
            answered.send(()).expect("the page told");
            (page.join().expect("the page answered"), theirs)
        });
        assert!(
            page.starts_with("HTTP/1.1 200 OK"),
            "the page beside {other:?}: {page}"
        );
        assert!(
            other.answered(&theirs),
            "{other:?} beside the page: {theirs}"
        );
    }
}

/// **A source of sessions in this process whose admission reads meet in
/// pairs**: each read waits, within a bound, until the other request's read
/// is under way too, then answers the reader it holds. A read that meets no
/// one is counted, never waited on without end.
struct Paired {
    reader: identity::SessionIdentity,
    meeting: Mutex<Arc<Meeting>>,
    reads: AtomicUsize,
    alone: AtomicUsize,
}

impl Paired {
    /// The next two reads meet each other.
    fn pair_next(&self) {
        *self.meeting.lock().expect("meeting") = Meeting::of(2);
    }
}

impl identity::Kept for Paired {
    fn session(
        &self,
        _: &[u8],
        _: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let meeting = self.meeting.lock().expect("meeting").clone();
        if !meeting.arrive() {
            self.alone.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Some(self.reader.clone()))
    }
    fn inspect(
        &self,
        _: &[u8],
        _: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        Ok(Some(self.reader.clone()))
    }
    fn open(&self, _: &[u8], _: &identity::Principal, _: &[u8; 16]) -> Result<(), String> {
        Err("no session is opened in this test".into())
    }
    fn close(&self, _: &[u8]) -> Result<(), String> {
        Err("no session is closed in this test".into())
    }
    fn begin_sign_in(&self, _: &[u8], _: &str, _: &str) -> Result<(), String> {
        Err("no sign-in in this test".into())
    }
    fn take_sign_in(&self, _: &[u8]) -> Result<Option<(String, String, Duration)>, String> {
        Err("no sign-in in this test".into())
    }
}

/// **Two requests of one session whose admission reads overlap are each
/// answered** (ADR-0389, amended 2026-11-02), for every kind of request that
/// is read for in this host (a command is the PostgreSQL test's: memory's
/// data layer fences no kept session). Each pair's two reads meet, so they
/// overlap whatever the scheduler does. Until the repair the second to end
/// was refused, and its request answered 503; the runtime's files were not
/// admitted at all (PR #63's exemption), so their reads met no one.
#[test]
fn two_requests_of_one_session_whose_admissions_overlap_are_each_answered() {
    let s = served_feed();
    with_runtime_files(&s);
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    let identity::ReaderIdentity::Account(reader) = s.identity.principals().bound_reader("reader")
    else {
        panic!("an account reader");
    };
    let source = Arc::new(Paired {
        reader,
        meeting: Mutex::new(Meeting::of(2)),
        reads: AtomicUsize::new(0),
        alone: AtomicUsize::new(0),
    });
    s.identity.principals().keep_in(source.clone());
    let page = Ask::Get("/".into());
    // Every pair is tried, and every failure said at the end: what the
    // defect refused, kind by kind, is the characterization.
    let mut failures = Vec::new();
    for (n, other) in Ask::every(&s)
        .into_iter()
        .filter(|a| matches!(a, Ask::Get(_)))
        .enumerate()
    {
        source.pair_next();
        let (before, alone) = (
            source.reads.load(Ordering::SeqCst),
            source.alone.load(Ordering::SeqCst),
        );
        let (mine, theirs) = std::thread::scope(|scope| {
            let mine = scope.spawn(|| page.sent(&s, "reader", n));
            let theirs = scope.spawn(|| other.sent(&s, "reader", n));
            (
                mine.join().expect("the page answered"),
                theirs.join().expect("the other answered"),
            )
        });
        let read = (
            source.reads.load(Ordering::SeqCst) - before,
            source.alone.load(Ordering::SeqCst) - alone,
        );
        if read != (2, 0) {
            failures.push(format!(
                "the page and {other:?} were not each admitted at once: (reads, alone) {read:?}"
            ));
        }
        for (ask, beside, answer) in [(&page, &other, &mine), (&other, &page, &theirs)] {
            if !ask.answered(answer) {
                let status = answer.lines().next().unwrap_or("no answer");
                failures.push(format!("{ask:?} beside {beside:?}: {status}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// **On PostgreSQL, two requests of one session whose admissions overlap are
/// each answered** (ADR-0389, amended 2026-11-02), for every kind of request,
/// a command beside a page and beside another command included. Each pair's
/// two admission reads meet after they read (a test's step in
/// `SessionsPg::session`): both have read before either ends. Until the
/// repair one of the two was refused: a page or a file 503, a command 403
/// `session-changed`. (Until the review the test held the session's lock,
/// which each admission took then, until both waited on it; an admission
/// takes no lock now.)
#[test]
fn on_postgres_two_requests_of_one_session_whose_admissions_overlap_are_each_answered() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "overlap");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    with_runtime_files(&a);
    let ada = signed_in(&a, &provider, "s-came", "ada");
    let kept = a.identity.principals().kept().expect("sessions kept");
    let meeting = Arc::new(Mutex::new(Meeting::of(2)));
    let (reads, alone) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let step = {
        let (meeting, reads, alone) = (meeting.clone(), reads.clone(), alone.clone());
        move || {
            reads.fetch_add(1, Ordering::SeqCst);
            let meeting = meeting.lock().expect("meeting").clone();
            if !meeting.arrive() {
                alone.fetch_add(1, Ordering::SeqCst);
            }
        }
    };
    kept.after_each_read(Some(Arc::new(step)));
    let every = Ask::every(&a);
    let post = every.last().expect("a post").clone();
    let pairs = every
        .iter()
        .map(|other| (Ask::Get("/".into()), other.clone()))
        .chain([(post.clone(), post)]);
    // Every pair is tried, and every failure said at the end.
    let mut failures = Vec::new();
    for (n, (first, second)) in pairs.enumerate() {
        *meeting.lock().expect("meeting") = Meeting::of(2);
        let before = (reads.load(Ordering::SeqCst), alone.load(Ordering::SeqCst));
        let (one, other) = std::thread::scope(|scope| {
            let one = scope.spawn(|| first.sent(&a, &ada, 2 * n));
            let other = scope.spawn(|| second.sent(&a, &ada, 2 * n + 1));
            (
                one.join().expect("the first answered"),
                other.join().expect("the second answered"),
            )
        });
        let read = (
            reads.load(Ordering::SeqCst) - before.0,
            alone.load(Ordering::SeqCst) - before.1,
        );
        if read != (2, 0) {
            failures.push(format!(
                "{first:?} and {second:?} were not each admitted at once: (reads, alone) {read:?}"
            ));
        }
        for (ask, beside, answer) in [(&first, &second, &one), (&second, &first, &other)] {
            if !ask.answered(answer) {
                let status = answer.lines().next().unwrap_or("no answer");
                failures.push(format!("{ask:?} beside {beside:?}: {status}"));
            }
        }
    }
    kept.after_each_read(None);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// **What a warming page has under way at once**: the runtime, its
/// WebAssembly and the handler table its head asks for together, and the
/// renderer and a handler module it warms (`warm` in `pw-runtime.mjs`).
/// With one more request of the session beside them, six: as many as a
/// browser sends to one origin over HTTP/1.1 at a time.
fn warm_up(s: &Server) -> Vec<Ask> {
    let handler = s
        .handler_identities()
        .into_iter()
        .next()
        .expect("the feed has a handler");
    [
        "/pw-runtime.mjs".to_string(),
        "/pw-resume.wasm".into(),
        "/pw-handlers".into(),
        "/pw-render.wasm".into(),
        format!("/handler/{handler}.mjs"),
    ]
    .into_iter()
    .map(Ask::Get)
    .collect()
}

/// Each of `asks` sent at once as `session`, its answers in its order.
fn sent_at_once(s: &Server, session: &str, asks: &[Ask]) -> Vec<String> {
    std::thread::scope(|scope| {
        let sent: Vec<_> = asks
            .iter()
            .enumerate()
            .map(|(n, ask)| scope.spawn(move || ask.sent(s, session, n)))
            .collect();
        sent.into_iter()
            .map(|answer| answer.join().expect("each request answered"))
            .collect()
    })
}

/// Each of `asks` not answered as it is alone, with its status line.
fn not_answered(asks: &[Ask], answers: &[String]) -> Vec<String> {
    asks.iter()
        .zip(answers)
        .filter(|(ask, answer)| !ask.answered(answer))
        .map(|(ask, answer)| format!("{ask:?}: {}", answer.lines().next().unwrap_or("no answer")))
        .collect()
}

/// Whether the handler table `answer` says its build warms (`#warm|1`).
fn says_it_warms(answer: &str) -> bool {
    answer.starts_with("HTTP/1.1 200 OK")
        && answer
            .split_once("\r\n\r\n")
            .is_some_and(|(_, table)| table.lines().any(|line| line == "#warm|1"))
}

/// **A host that keeps sessions warms, and a warm-up's requests, sent
/// together, are each answered** (the first press's ADR, amended
/// 2026-11-06). The first press warmed no such host: of two admissions of
/// one session whose reads overlapped, one was refused, and a warm-up asks
/// for its modules together. Here a warming page's five requests and a
/// second tab's page read the session at once, every read meeting the
/// others before any ends.
#[test]
fn a_host_that_keeps_sessions_warms_and_its_warm_up_is_each_answered() {
    let mut s = served_feed();
    s.server.warm = true;
    with_runtime_files(&s);
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    let identity::ReaderIdentity::Account(reader) = s.identity.principals().bound_reader("reader")
    else {
        panic!("an account reader");
    };
    let source = Arc::new(Paired {
        reader,
        meeting: Mutex::new(Meeting::of(1)),
        reads: AtomicUsize::new(0),
        alone: AtomicUsize::new(0),
    });
    s.identity.principals().keep_in(source.clone());
    let table = get_as(&s, "reader", "/pw-handlers");
    assert!(
        says_it_warms(&table),
        "a warming host that keeps sessions says it warms: {table}"
    );
    let mut asks = warm_up(&s);
    asks.push(Ask::Get("/".into()));
    *source.meeting.lock().expect("meeting") = Meeting::of(asks.len());
    let before = (
        source.reads.load(Ordering::SeqCst),
        source.alone.load(Ordering::SeqCst),
    );
    let answers = sent_at_once(&s, "reader", &asks);
    let read = (
        source.reads.load(Ordering::SeqCst) - before.0,
        source.alone.load(Ordering::SeqCst) - before.1,
    );
    assert_eq!(
        read,
        (asks.len(), 0),
        "each admitted at once: (reads, alone)"
    );
    let refused = not_answered(&asks, &answers);
    assert!(refused.is_empty(), "{refused:#?}");
}

/// **On PostgreSQL, a host that keeps sessions warms, and a warm-up's
/// requests are each answered, beside a press made while it warms** (the
/// first press's ADR, amended 2026-11-06): a warming page's five requests
/// and a post read the session at once, each read meeting the others after
/// it reads (a test's step in `SessionsPg::session`).
#[test]
fn on_postgres_a_host_that_keeps_sessions_warms_and_its_warm_up_is_each_answered() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "warmup");
    let provider = Arc::new(TestProvider::default());
    let mut a = feed_host_signing_in(&url, &db, &provider);
    a.server.warm = true;
    with_runtime_files(&a);
    let ada = signed_in(&a, &provider, "s-warm", "ada");
    let table = get_as(&a, &ada, "/pw-handlers");
    assert!(
        says_it_warms(&table),
        "a warming host that keeps sessions says it warms: {table}"
    );
    let kept = a.identity.principals().kept().expect("sessions kept");
    let mut asks = warm_up(&a);
    asks.push(Ask::Post("warming".into()));
    let meeting = Arc::new(Mutex::new(Meeting::of(asks.len())));
    let (reads, alone) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let step = {
        let (meeting, reads, alone) = (meeting.clone(), reads.clone(), alone.clone());
        move || {
            reads.fetch_add(1, Ordering::SeqCst);
            let meeting = meeting.lock().expect("meeting").clone();
            if !meeting.arrive() {
                alone.fetch_add(1, Ordering::SeqCst);
            }
        }
    };
    kept.after_each_read(Some(Arc::new(step)));
    let answers = sent_at_once(&a, &ada, &asks);
    kept.after_each_read(None);
    let read = (reads.load(Ordering::SeqCst), alone.load(Ordering::SeqCst));
    assert_eq!(
        read,
        (asks.len(), 0),
        "each admitted at once: (reads, alone)"
    );
    let refused = not_answered(&asks, &answers);
    assert!(refused.is_empty(), "{refused:#?}");
}

/// **How long an admission may take while a command holds its session's
/// lock**: well under that lock's 5 s timeout, which an admission waiting
/// for the lock reaches (the review measured 5001 ms), and far above one
/// statement's few milliseconds on a busy machine.
const UNWAITED: Duration = Duration::from_secs(2);

/// **A session's lock held as a command holds it**: the same session-level
/// advisory lock, taken by `hosts::command_session_lock` on a connection of
/// its own, as a command's staging takes it after its body and keeps it
/// until it ends (a payment's outbound call included).
struct CommandLock {
    c: postgres::Client,
    hash: Vec<u8>,
    /// The holding connection's server process, which a waiter names.
    pid: i32,
}

impl CommandLock {
    fn held(db: &Schema, hash: &[u8]) -> CommandLock {
        let mut c = db.sql();
        let pid = c
            .query_one("SELECT pg_backend_pid()", &[])
            .expect("the holder's process")
            .get(0);
        crate::hosts::command_session_lock(&mut c, hash, || {}).expect("the session's lock held");
        CommandLock {
            c,
            hash: hash.to_vec(),
            pid,
        }
    }

    /// Let go, as the command's staging lets go when it ends.
    fn released(mut self) {
        crate::hosts::command_session_unlock(&mut self.c, &self.hash, || {})
            .expect("the session's lock let go");
    }
}

/// **Ada signed in on a host of the feed on `db`**: the host, her session,
/// and the reader it admitted as she signed in.
fn ada_signed_in(url: &str, db: &Schema) -> (Served, String, identity::SessionIdentity) {
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(url, db, &provider);
    let ada = signed_in(&a, &provider, "s-came", "ada");
    let identity::ReaderIdentity::Account(reader) = a.identity.principals().bound_reader(&ada)
    else {
        panic!("Ada is signed in");
    };
    (a, ada, reader)
}

/// Whether the session `hash` names is kept, and whether its use was
/// recorded within the last minute, on the database's clock.
fn kept_and_recent(db: &Schema, hash: &[u8]) -> (bool, bool) {
    db.sql()
        .query_opt(
            "SELECT seen > clock_timestamp() - interval '1 minute' FROM sessions WHERE hash = $1",
            &[&hash],
        )
        .expect("the session read")
        .map_or((false, false), |row| (true, row.get(0)))
}

/// The session `hash` names last recorded as used `ago` (an interval's
/// text) before now, as a test ages it.
fn last_used(db: &Schema, hash: &[u8], ago: &str) {
    db.sql()
        .execute(
            "UPDATE sessions SET seen = clock_timestamp() - $2::text::interval WHERE hash = $1",
            &[&hash, &ago],
        )
        .expect("the session aged");
}

/// **An admission never waits for a command holding its session's lock**
/// (ADR-0389, amended 2026-11-02, from its review). Until then each
/// admission took the lock in a five-statement transaction and waited for
/// it, up to its 5 s timeout. Now it is one statement that takes no lock:
/// answered, as the reader, well inside the timeout.
#[test]
fn on_postgres_an_admission_never_waits_for_a_command_holding_its_session() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "unwaited");
    let (a, ada, reader) = ada_signed_in(&url, &db);
    let principals = a.identity.principals();
    let kept = principals.kept().expect("sessions kept");
    let command = CommandLock::held(&db, &identity::kept_hash(&ada));
    let (before, since) = (kept.ran(), Instant::now());
    let answer = principals.admit(&ada);
    let (took, ran) = (since.elapsed(), kept.ran() - before);
    command.released();
    assert_eq!(
        (answer, ran, took < UNWAITED),
        (Ok(identity::ReaderIdentity::Account(reader)), 1, true),
        "answered as the reader, by one statement, within {UNWAITED:?}: took {took:?}"
    );
}

/// **A use due to be recorded is skipped while a command holds the session's
/// lock, and recorded once it is free** (ADR-0389, amended 2026-11-02): a
/// session last recorded as used two minutes ago is due a touch. With the
/// lock held the admission writes nothing, waits for nothing, and is
/// answered as the reader; with the lock free the next admission records
/// the use. A touch only extends idle expiry, and comes at most a minute
/// late, so a skipped one is made by a later admission safely.
#[test]
fn on_postgres_a_due_touch_is_skipped_under_a_commands_lock_and_made_once_free() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "touch_skipped");
    let (a, ada, reader) = ada_signed_in(&url, &db);
    let principals = a.identity.principals();
    let hash = identity::kept_hash(&ada);
    last_used(&db, &hash, "2 minutes");
    let command = CommandLock::held(&db, &hash);
    let since = Instant::now();
    let held = principals.admit(&ada);
    let took = since.elapsed();
    let while_held = kept_and_recent(&db, &hash);
    command.released();
    let free = principals.admit(&ada);
    let once_free = kept_and_recent(&db, &hash);
    let reader = identity::ReaderIdentity::Account(reader);
    assert_eq!(
        (held, took < UNWAITED, while_held, free, once_free),
        (
            Ok(reader.clone()),
            true,
            (true, false),
            Ok(reader),
            (true, true)
        ),
        "answered without waiting (took {took:?}), its use unrecorded while the lock was held \
         and recorded once it was free"
    );
}

/// **A session past its lifetime is no one while a command holds its lock,
/// without waiting, and its row deleted once the lock is free** (ADR-0389,
/// amended 2026-11-02): opened sixty-one days ago and used now, only its
/// deletion is due. With the lock held the admission deletes nothing, waits
/// for nothing, and is no one, as an expired session is wherever it is
/// read; with the lock free the next admission deletes its row.
#[test]
fn on_postgres_an_expired_session_is_no_one_under_a_commands_lock_and_deleted_once_free() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "expired_held");
    let (a, ada, _) = ada_signed_in(&url, &db);
    let principals = a.identity.principals();
    let hash = identity::kept_hash(&ada);
    db.sql()
        .execute(
            "UPDATE sessions SET opened = clock_timestamp() - interval '61 days', \
             seen = clock_timestamp() WHERE hash = $1",
            &[&hash],
        )
        .expect("opened sixty-one days ago, used now");
    let command = CommandLock::held(&db, &hash);
    let since = Instant::now();
    let held = principals.admit(&ada);
    let took = since.elapsed();
    let while_held = kept_and_recent(&db, &hash);
    command.released();
    let free = principals.admit(&ada);
    let once_free = kept_and_recent(&db, &hash);
    let no_one = Ok(identity::ReaderIdentity::Anonymous);
    assert_eq!(
        (held, took < UNWAITED, while_held, free, once_free),
        (no_one.clone(), true, (true, true), no_one, (false, false)),
        "no one without waiting (took {took:?}), its row kept while the lock was held and \
         deleted once it was free"
    );
}

/// **A due write's answer is the admission's** (ADR-0389, amended
/// 2026-11-02): the write runs under the session's lock, its snapshot taken
/// after the lock, and answers the request. A session another host signs
/// out between the read (which found it live, its use due to be recorded)
/// and the write is no one, though the read found the reader.
#[test]
fn on_postgres_a_session_signed_out_between_the_read_and_the_write_is_no_one() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "closed_between");
    let (a, ada, _) = ada_signed_in(&url, &db);
    let principals = a.identity.principals();
    let kept = principals.kept().expect("sessions kept");
    let hash = identity::kept_hash(&ada);
    last_used(&db, &hash, "2 minutes");
    // Another host's sessions, on the same schema: it signs Ada out after
    // the first read, once.
    let other = Arc::new(crate::hosts::SessionsPg::new(&url, db.name()));
    let closed = Arc::new(AtomicUsize::new(0));
    let step = {
        let (closed, other, hash) = (closed.clone(), other.clone(), hash.clone());
        move || {
            if closed.fetch_add(1, Ordering::SeqCst) == 0 {
                other.close(&hash).expect("signed out on another host");
            }
        }
    };
    kept.after_each_read(Some(Arc::new(step)));
    let answer = principals.admit(&ada);
    kept.after_each_read(None);
    assert_eq!(
        (
            answer,
            closed.load(Ordering::SeqCst),
            kept_and_recent(&db, &hash)
        ),
        (Ok(identity::ReaderIdentity::Anonymous), 1, (false, false)),
        "the write, under the lock, found the session signed out since the read"
    );
}

/// **Admissions of a session whose lock a command holds delay no other
/// reader's page** (ADR-0389, amended 2026-11-02; the review's
/// reproduction): as many of Ada's requests as the host has session
/// connections (her page's runtime files) are asked at once while a command
/// holds her session's lock, and then Bob's page. Until then each of hers
/// waited on the lock, up to its 5 s timeout, holding a session connection;
/// one was answered 503 at 5001 ms, and Bob's page, waiting for a
/// connection, took 4969 ms where it took 18 ms alone.
#[test]
fn on_postgres_admissions_under_a_commands_lock_delay_no_other_readers_page() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "unwaited_pages");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    with_runtime_files(&a);
    let ada = signed_in(&a, &provider, "s-came-ada", "ada");
    let bob = signed_in(&a, &provider, "s-came-bob", "bob");
    let files: Vec<String> = ["/pw-runtime.mjs", "/pw-resume.wasm", "/pw-render.wasm"]
        .into_iter()
        .map(String::from)
        .chain((1..).map(|n| format!("/pw-runtime.mjs?document={n}")))
        .take(crate::hosts::SESSION_CONNECTIONS)
        .collect();
    let command = CommandLock::held(&db, &identity::kept_hash(&ada));
    let mut watcher = db.sql();
    let answered = AtomicUsize::new(0);
    let (hers, bobs, bob_took) = std::thread::scope(|scope| {
        let hers: Vec<_> = files
            .iter()
            .map(|file| {
                let (a, ada, answered) = (&a, &ada, &answered);
                scope.spawn(move || {
                    let since = Instant::now();
                    let answer = get_as(a, ada, file);
                    answered.fetch_add(1, Ordering::SeqCst);
                    let status = answer.lines().next().unwrap_or("no answer").to_string();
                    (status, since.elapsed())
                })
            })
            .collect();
        // Until all of hers are answered, or all wait on her session's lock,
        // each then holding a session connection, within a bound.
        let since = Instant::now();
        loop {
            let waiting: i64 = watcher
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))",
                    &[&command.pid],
                )
                .expect("who waits")
                .get(0);
            if answered.load(Ordering::SeqCst) == files.len()
                || waiting >= files.len() as i64
                || since.elapsed() > Duration::from_secs(3)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let since = Instant::now();
        let bobs = get_as(&a, &bob, "/");
        let bob_took = since.elapsed();
        command.released();
        let hers: Vec<_> = hers
            .into_iter()
            .map(|h| h.join().expect("answered"))
            .collect();
        let status = bobs.lines().next().unwrap_or("no answer").to_string();
        (hers, status, bob_took)
    });
    let unwaited = hers
        .iter()
        .all(|(status, took)| status == "HTTP/1.1 200 OK" && *took < UNWAITED);
    assert_eq!(
        (bobs.as_str(), bob_took < UNWAITED, unwaited),
        ("HTTP/1.1 200 OK", true, true),
        "Bob's page took {bob_took:?}; Ada's files: {hers:?}"
    );
}

/// **An admission waits on a locked sessions table no longer than the lock
/// timeout** (ADR-0389, amended 2026-11-02): its read takes no session
/// lock, but a migration's lock on the table holds any statement on it.
/// Each session connection bounds every lock wait at 5 s, as the
/// admission's transaction did when it was one, so the request is refused
/// rather than held as long as the table is.
#[test]
fn on_postgres_an_admission_waits_on_a_locked_table_no_longer_than_the_lock_timeout() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "table_locked");
    let (a, ada, _) = ada_signed_in(&url, &db);
    let principals = a.identity.principals();
    let mut migration = db.sql();
    migration
        .batch_execute("BEGIN; LOCK TABLE sessions IN ACCESS EXCLUSIVE MODE")
        .expect("the table locked, as a migration locks it");
    let (sent, answered) = mpsc::channel();
    let within = std::thread::scope(|scope| {
        scope.spawn(|| {
            let since = Instant::now();
            let answer = principals.admit(&ada);
            let _ = sent.send((answer, since.elapsed()));
        });
        let within = answered.recv_timeout(Duration::from_secs(8));
        migration
            .batch_execute("ROLLBACK")
            .expect("the table let go");
        within
    });
    assert!(
        matches!(
            &within,
            Ok((Err(identity::AuthorityError::SourceUnavailable), took))
                if *took >= Duration::from_secs(4)
        ),
        "refused at the lock timeout while the table was locked, within 8 s: {within:?}"
    );
}

/// A source of sessions that fails every read.
struct Unavailable;

impl identity::Kept for Unavailable {
    fn session(
        &self,
        _: &[u8],
        _: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        Err("unavailable".into())
    }
    fn inspect(
        &self,
        _: &[u8],
        _: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        Err("unavailable".into())
    }
    fn open(&self, _: &[u8], _: &identity::Principal, _: &[u8; 16]) -> Result<(), String> {
        Err("unavailable".into())
    }
    fn close(&self, _: &[u8]) -> Result<(), String> {
        Err("unavailable".into())
    }
    fn begin_sign_in(&self, _: &[u8], _: &str, _: &str) -> Result<(), String> {
        Err("unavailable".into())
    }
    fn take_sign_in(&self, _: &[u8]) -> Result<Option<(String, String, Duration)>, String> {
        Err("unavailable".into())
    }
}

/// **Every request is admitted, the runtime's own files too, and a read
/// that fails refuses it** (ADR-0389, amended 2026-11-02). PR #63 answered
/// `pw-runtime.mjs`, `pw-resume.wasm` and `pw-render.wasm` before admission,
/// for a 503 that overlapping admissions most likely caused (its run cannot
/// tell); with overlapping admissions answered, the three files are admitted
/// as every other request is, and no request is answered on no read. Its
/// test, `public_runtime_assets_load_while_session_authority_is_unavailable`,
/// held the opposite and is replaced by this one.
#[test]
fn every_request_is_admitted_and_a_failed_read_refuses_it() {
    let s = served_feed();
    with_runtime_files(&s);
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    s.identity.principals().keep_in(Arc::new(Unavailable));
    let answers: Vec<(Ask, String)> = Ask::every(&s)
        .into_iter()
        .enumerate()
        .map(|(n, ask)| {
            let answer = ask.sent(&s, "reader", n);
            (ask, answer)
        })
        .collect();
    let asked = answers.len();
    for (ask, answer) in answers {
        let refused = match ask {
            Ask::Get(_) => "HTTP/1.1 503 Service Unavailable",
            Ask::Post(_) => "HTTP/1.1 403",
        };
        assert!(answer.starts_with(refused), "{ask:?}: {answer}");
    }
    // Each refusal is logged by its kind, a page's and a command's alike: a
    // failed read and a superseded admission answer the same body, and the
    // host's log tells them apart (the 503 of 2026-10-31's run could not
    // be). The line names no session and no reader.
    assert_eq!(
        *s.refusals.lock().expect("refusals"),
        vec![
            "pw dev server: session admission refused: session authority source is unavailable"
                .to_string();
            asked
        ]
    );
}
