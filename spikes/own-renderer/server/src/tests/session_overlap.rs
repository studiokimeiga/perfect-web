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
//! where the test holds the session's advisory lock until both admissions
//! wait on it, so that both read before either ends. The PostgreSQL test
//! passes doing nothing without a database: `just e14-session-overlap`
//! refuses to run without one.

use super::second_host::{Schema, database, feed_host_signing_in};
use super::sign_in::{TestProvider, command, signed_in};
use super::*;
use crate::identity::tests::Meeting;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

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
/// a command beside a page and beside another command included. The test
/// holds the session's advisory lock, which each admission's statement takes
/// (`SessionsPg::session`), until both admissions wait on it: both have
/// begun before either ends. Until the repair one of the two was refused: a
/// page or a file 503, a command 403 `session-changed`.
#[test]
fn on_postgres_two_requests_of_one_session_whose_admissions_overlap_are_each_answered() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "overlap");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    with_runtime_files(&a);
    let ada = signed_in(&a, &provider, "s-came", "ada");
    let hash = identity::kept_hash(&ada);
    let key = crate::hosts::SESSION_AUTHORITY_KEY;
    let mut holder = db.sql();
    let holding: i32 = holder
        .query_one("SELECT pg_backend_pid()", &[])
        .expect("the holder's process")
        .get(0);
    let mut watcher = db.sql();
    let every = Ask::every(&a);
    let post = every.last().expect("a post").clone();
    let pairs = every
        .iter()
        .map(|other| (Ask::Get("/".into()), other.clone()))
        .chain([(post.clone(), post)]);
    // Every pair is tried, and every failure said at the end.
    let mut failures = Vec::new();
    for (n, (first, second)) in pairs.enumerate() {
        holder
            .query_one(&format!("SELECT pg_advisory_lock({key})"), &[&hash])
            .expect("the session's lock held");
        let (waited, one, other) = std::thread::scope(|scope| {
            let one = scope.spawn(|| first.sent(&a, &ada, 2 * n));
            let other = scope.spawn(|| second.sent(&a, &ada, 2 * n + 1));
            // Both admissions wait on the lock, or the bound passes (well
            // inside the statement's own 5 s `lock_timeout`).
            let since = std::time::Instant::now();
            let waited = loop {
                let waiting: i64 = watcher
                    .query_one(
                        "SELECT count(*) FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))",
                        &[&holding],
                    )
                    .expect("who waits")
                    .get(0);
                if waiting >= 2 || since.elapsed() > Duration::from_secs(3) {
                    break waiting;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let released: bool = holder
                .query_one(&format!("SELECT pg_advisory_unlock({key})"), &[&hash])
                .expect("the session's lock released")
                .get(0);
            assert!(released, "the test held the lock it released");
            (
                waited,
                one.join().expect("the first answered"),
                other.join().expect("the second answered"),
            )
        });
        if waited != 2 {
            failures.push(format!(
                "{first:?} and {second:?} did not both wait for the session's lock: {waited} did"
            ));
        }
        for (ask, beside, answer) in [(&first, &second, &one), (&second, &first, &other)] {
            if !ask.answered(answer) {
                let status = answer.lines().next().unwrap_or("no answer");
                failures.push(format!("{ask:?} beside {beside:?}: {status}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
    for (ask, answer) in answers {
        let refused = match ask {
            Ask::Get(_) => "HTTP/1.1 503 Service Unavailable",
            Ask::Post(_) => "HTTP/1.1 403",
        };
        assert!(answer.starts_with(refused), "{ask:?}: {answer}");
    }
}
