//! **Hosts sharing one database** (ADR-0369, track `second-host`): two hosts
//! in one process, each its own `Server` on a layer of its own opened on one
//! schema, as two processes on one PostgreSQL are. Each test passes doing
//! nothing without a database: `just e14-second-host` runs them.

use super::sign_in::{REDIRECT, TestProvider, command, exchanged, get, location, set_cookie};
use super::*;
use crate::data::DataLayer;
use crate::feed_pg::{FeedPg, Isolation, RenderSeats};
use crate::hosts::Tail;
use crate::store_pg::StorePg;

/// The database the feed's tests run against, where the environment names
/// one.
fn database() -> Option<String> {
    match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
            None
        }
    }
}

/// **A schema of the test's own**, dropped when the test is done with it.
struct Schema {
    url: String,
    name: String,
}

impl Schema {
    fn new(url: &str, test: &str) -> Schema {
        let schema = Schema {
            url: url.to_string(),
            name: format!("pw_hosts_{}_{test}", std::process::id()),
        };
        schema
            .admin()
            .batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", schema.name))
            .expect("a stale schema dropped");
        schema
    }

    fn admin(&self) -> postgres::Client {
        postgres::Client::connect(&self.url, postgres::NoTls).expect("connected")
    }

    fn sql(&self) -> postgres::Client {
        let mut c = self.admin();
        c.batch_execute(&format!("SET search_path TO {}", self.name))
            .expect("the test's schema");
        c
    }

    fn count(&self, query: &str) -> i64 {
        self.sql().query_one(query, &[]).expect(query).get(0)
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!(
                "SET lock_timeout = '10s'; DROP SCHEMA IF EXISTS {} CASCADE",
                self.name
            ));
        }
    }
}

/// **A host of the feed**, started on the schema as a process is: its own
/// layer, its own id, its own cursor.
fn feed_host(url: &str, schema: &Schema) -> Served {
    let layer = FeedPg::open(url, Some(&schema.name)).expect("the layer");
    let (dir, out) = built_feed_with(|app| app.to_string());
    let server = Server::from_build_with(out.clone(), out, Some(Arc::new(layer))).expect("served");
    Served { server, _dir: dir }
}

fn tail(s: &Server) -> &Tail {
    s.data
        .tail()
        .expect("a layer on a shared database reads the outbox")
}

fn post(s: &Server, session: &str, text: &str, interaction: &str) -> Answered {
    s.command_answered(
        "feed.app.post",
        session,
        &[Val::String(text.into())],
        Some(interaction),
    )
    .expect("runs")
}

/// `session`'s home page, served by `s`: its document.
fn home(s: &Server, session: &str) -> Doc {
    let (html, _, _, _) = s
        .serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
        .expect("served");
    assert!(html.contains("Hello, feed."), "{html}");
    latest(&s.pending.lock().expect("pending"), session)
}

/// Every frame waiting for `doc`, as text.
fn frames(s: &Server, doc: &Doc) -> Vec<String> {
    s.pending
        .lock()
        .expect("pending")
        .get(doc)
        .map(|w| w.frames.iter().map(|(_, f)| format!("{f:?}")).collect())
        .unwrap_or_default()
}

/// **A command through A is told to a page on B** (ADR-0369): B's reader
/// finds A's commit in the outbox, after its cursor, and tells B's session
/// that reads what it dropped. Not before B reads it: nothing else carries a
/// commit from one host to the other. Until ADR-0369 the committing host
/// consumed its rows, and B heard nothing.
#[test]
fn a_post_through_one_host_is_told_to_a_page_on_another() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "told");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let theirs = home(&b, "b");
    let before = sets_of(&b, &theirs).len();
    let answered = post(&a, "a", "Posted through A, read on B", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(sets_of(&b, &theirs).len(), before, "not before B reads it");
    b.hear();
    let set = format!("{:?}", sets_of(&b, &theirs).pop().expect("a patch set"));
    assert!(set.contains("Posted through A, read on B"), "{set}");
    assert_eq!(
        tail(&b).cursor(),
        db.count("SELECT max(id) FROM outbox"),
        "B's cursor past A's commit"
    );
}

/// **Each commit is told once on each host**: the committing host tells its
/// own pages at once and its reader passes its rows by; another host tells
/// its pages when it reads them, and not again.
#[test]
fn a_commit_is_told_once_on_each_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "once");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let mine = home(&a, "a2");
    let theirs = home(&b, "b");
    post(&a, "a", "Told once on each host", "i-1");
    a.tell_waiting();
    let told_on_a = frames(&a, &mine).len();
    assert!(told_on_a > 0, "A's own page told at once");
    assert!(
        tail(&a).heard().expect("read").commits.is_empty(),
        "A's reader hears none of A's own commits"
    );
    a.hear();
    assert_eq!(
        frames(&a, &mine).len(),
        told_on_a,
        "A's reader passes A's own rows by"
    );
    b.hear();
    let told_on_b = frames(&b, &theirs).len();
    assert!(told_on_b > 0, "B's page told");
    b.hear();
    assert_eq!(frames(&b, &theirs).len(), told_on_b, "and not again");
}

/// **A host whose listening connection drops catches up from its cursor**:
/// the notification is lost with the connection (PostgreSQL keeps none for
/// a session not listening); the rows are not, and B reads them once its
/// connection is made again.
#[test]
fn a_host_that_misses_its_notification_catches_up_from_the_outbox() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "missed");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let theirs = home(&b, "b");
    let pid = tail(&b).backend_pid().expect("B listens");
    let ended: bool = db
        .admin()
        .query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .expect("terminated")
        .get(0);
    assert!(ended, "B's listening connection ended");
    post(&a, "a", "Committed while B was not listening", "i-1");
    // The first read finds the connection gone; a read after it is made
    // again finds the rows.
    for _ in 0..3 {
        b.hear();
        if !sets_of(&b, &theirs).is_empty() {
            break;
        }
    }
    let set = format!("{:?}", sets_of(&b, &theirs).pop().expect("a patch set"));
    assert!(set.contains("Committed while B was not listening"), "{set}");
    assert_ne!(
        tail(&b).backend_pid().expect("B listens again"),
        pid,
        "on a connection made again"
    );
}

/// **A host started after commits starts past them**: its pages read the
/// database as it is; nothing before its start is told again.
#[test]
fn a_host_started_after_commits_starts_past_them() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "started");
    let a = feed_host(&url, &db);
    post(&a, "a", "Before B started", "i-1");
    let b = feed_host(&url, &db);
    assert_eq!(
        tail(&b).cursor(),
        db.count("SELECT max(id) FROM outbox"),
        "B starts at the highest id"
    );
    let theirs = home(&b, "b");
    b.hear();
    assert!(
        frames(&b, &theirs).is_empty(),
        "told nothing it already read"
    );
    assert_eq!(
        db.count("SELECT count(*) FROM outbox"),
        1,
        "A's row kept, its post's event: B deleted nothing at its start"
    );
}

/// **A host connects what it uses**: a layer opened on a shared database
/// holds no listening connection until its host first reads the outbox, and
/// its cursor is the outbox's highest id all the same, read as the layer
/// opened; the first read listens, then reads, so a commit made meanwhile is
/// heard. A layer whose host never reads (a test's) holds none, beside the
/// connections its commands and reads take (the render merge's bounded
/// model).
#[test]
fn a_layer_listens_only_once_its_host_reads_the_outbox() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "listens");
    let a = feed_host(&url, &db);
    post(&a, "a", "Before B opened", "i-1");
    let b = FeedPg::open(&url, Some(&db.name)).expect("B's layer");
    let reader = b.tail().expect("a shared database");
    assert!(!reader.listening(), "no listening connection at the open");
    assert_eq!(
        reader.cursor(),
        db.count("SELECT max(id) FROM outbox"),
        "at the highest id all the same"
    );
    post(&a, "a", "After B opened, before it listened", "i-2");
    let heard = reader.heard().expect("read");
    assert!(reader.listening(), "listening once it read");
    assert_eq!(heard.commits.len(), 1, "the commit made meanwhile heard");
}

/// **A host opening the database deletes nothing from its outbox**, in
/// either layer: a row another host has yet to read stays. Until ADR-0369
/// each opening deleted the outbox, so a host started, or restarted, beside
/// a running one ate the rows that host had not read.
#[test]
fn a_host_opening_the_database_deletes_nothing_from_its_outbox() {
    let Some(url) = database() else { return };
    let unread = "INSERT INTO outbox (kind, name, args, host) \
                  VALUES ('event', 'app.Changed', '[]', 'h-another')";
    let db = Schema::new(&url, "opened");
    let _a = FeedPg::open(&url, Some(&db.name)).expect("A's layer");
    db.sql()
        .batch_execute(unread)
        .expect("a row A has yet to read");
    let _b = FeedPg::open(&url, Some(&db.name)).expect("B's layer");
    assert_eq!(
        db.count("SELECT count(*) FROM outbox"),
        1,
        "the feed's row kept"
    );
    let Some(url) = store_database() else { return };
    let a = StorePg::for_test(&url).expect("A's layer");
    a.sql()
        .batch_execute(unread)
        .expect("a row A has yet to read");
    let _b = StorePg::open_with(
        &url,
        a.schema(),
        Isolation::Serializable,
        RenderSeats::default(),
    )
    .expect("B's layer");
    let kept: i64 = a
        .sql()
        .query_one("SELECT count(*) FROM outbox", &[])
        .expect("counted")
        .get(0);
    assert_eq!(kept, 1, "the store's row kept");
}

/// **A host whose cursor fell below what was trimmed reloads its pages**:
/// what the trimmed rows said can no longer be told, so each document is
/// told to reload, and the host goes on from the trimmed id, never skipping
/// in silence.
#[test]
fn a_host_behind_the_trim_tells_its_pages_to_reload() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "trimmed");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let theirs = home(&b, "b");
    post(&a, "a", "Trimmed before B read it", "i-1");
    tail(&a).trim_older_than("0 seconds").expect("trimmed");
    assert_eq!(db.count("SELECT count(*) FROM outbox"), 0, "trimmed");
    b.hear();
    let told = frames(&b, &theirs);
    assert!(
        told.iter().any(|f| f.contains("Reload")),
        "told to reload: {told:?}"
    );
    assert_eq!(
        tail(&b).cursor(),
        db.count("SELECT through FROM outbox_trimmed"),
        "on from the trimmed id"
    );
}

/// **A trim between a host's two reads is learned, never passed by in
/// silence**: a host reads the rows after its cursor and only then what was
/// trimmed, so a trim that comes between the two (here of every row, A's
/// post's among them) leaves its mark past B's cursor, and B's pages
/// reload. Read the other way round, the mark read before the trim said
/// none was missed, and the rows, gone by the second read, were passed by.
#[test]
fn a_trim_between_a_hosts_two_reads_is_learned() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "trim_between");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let theirs = home(&b, "b");
    post(&a, "a", "Trimmed between B's two reads", "i-1");
    let trimmer = Tail::open(&url, tail(&a).outbox().clone(), "h-trimmer").expect("a trimmer");
    tail(&b).between_reads_once(move || {
        trimmer.trim_older_than("0 seconds").expect("trimmed");
    });
    b.hear();
    assert_eq!(
        db.count("SELECT count(*) FROM outbox"),
        0,
        "trimmed between B's reads"
    );
    let told = frames(&b, &theirs);
    assert!(
        told.iter().any(|f| f.contains("Reload")),
        "told to reload: {told:?}"
    );
    assert_eq!(
        tail(&b).cursor(),
        db.count("SELECT through FROM outbox_trimmed"),
        "on from the trimmed id"
    );
}

/// **`commit`, run on A while `holder` holds the outbox's lock, waits for
/// it, and wakes `b`, waiting, at once**: the holder, as another host's
/// commit does, writes a row while A waits, and A's rows take their ids
/// only after the holder's commit, past the holder's, so ids are in commit
/// order; and what B waits for is the database's notification at A's
/// commit, not its wait's end.
fn waits_for_the_lock_and_wakes(
    mut holder: postgres::Client,
    a: &Server,
    b: &Server,
    commit: impl FnOnce() + Send,
) {
    let lock = tail(b).outbox().lock.clone();
    holder.batch_execute("BEGIN").expect("begun");
    holder
        .execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&lock])
        .expect("the outbox's lock held");
    tail(b).backend_pid().expect("B listens");
    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| {
            let started = std::time::Instant::now();
            tail(b).wait(20_000);
            started.elapsed()
        });
        let committing = scope.spawn(move || {
            commit();
            std::time::Instant::now()
        });
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(
            !committing.is_finished(),
            "A's commit waits for the outbox's lock"
        );
        let held: i64 = holder
            .query_one(
                "INSERT INTO outbox (kind, name, args, host) \
                 VALUES ('event', 'test.Held', '[]', 'h-holder') RETURNING id",
                &[],
            )
            .expect("the holder's row")
            .get(0);
        let released = std::time::Instant::now();
        holder.batch_execute("COMMIT").expect("the lock released");
        let committed = committing.join().expect("A committed");
        assert!(committed >= released, "A commits after the lock is let go");
        let waited = waiting.join().expect("B waited");
        assert!(
            waited < std::time::Duration::from_secs(10),
            "B woken by A's commit, not its wait's end: {waited:?}"
        );
        let mine: Vec<i64> = holder
            .query(
                "SELECT id FROM outbox WHERE host = $1 ORDER BY id",
                &[&tail(a).host()],
            )
            .expect("A's rows")
            .iter()
            .map(|r| r.get(0))
            .collect();
        assert!(
            !mine.is_empty() && mine.iter().all(|id| *id > held),
            "A's ids taken after the holder's commit, past its {held}: {mine:?}"
        );
    });
}

/// **A commit through the feed waits for the outbox's lock, and wakes a
/// waiting host at once** (ADR-0369): its rows take their ids in commit
/// order, and what a page on B waits for, of a commit through A, is the
/// database's delivery, not a poll's period.
#[test]
fn a_feed_commit_waits_for_the_outbox_lock_and_wakes_another_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "woken");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    waits_for_the_lock_and_wakes(db.sql(), &a, &b, || {
        assert!(post(&a, "a", "Wakes B", "i-1").committed);
    });
}

/// **Ids are taken in commit order**: a transaction that takes the
/// outbox's lock waits for one that holds it to commit, so its rows take
/// later ids and commit later, and a reader that has seen an id has seen
/// every smaller one that will ever commit.
#[test]
fn outbox_ids_are_taken_in_commit_order() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "ordered");
    let a = feed_host(&url, &db);
    let outbox = tail(&a).outbox().clone();
    let insert = |c: &mut postgres::Client| -> i64 {
        crate::hosts::ordered(c, &outbox).expect("ordered");
        c.query_one(
            "INSERT INTO outbox (kind, name, args, host) \
             VALUES ('event', 'feed.app.Posted', '[]', 'h-test') RETURNING id",
            &[],
        )
        .expect("inserted")
        .get(0)
    };
    let mut first = db.sql();
    first.batch_execute("BEGIN").expect("begun");
    let first_id = insert(&mut first);
    let (second_id, second_done) = std::thread::scope(|scope| {
        let second = scope.spawn(|| {
            let mut c = db.sql();
            c.batch_execute("BEGIN").expect("begun");
            let id = insert(&mut c);
            c.batch_execute("COMMIT").expect("committed");
            (id, std::time::Instant::now())
        });
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(
            !second.is_finished(),
            "the second waits for the first's lock"
        );
        let committed = std::time::Instant::now();
        first.batch_execute("COMMIT").expect("committed");
        let (id, done) = second.join().expect("the second");
        assert!(done >= committed, "the second commits after the first");
        (id, done)
    });
    let _ = second_done;
    assert!(second_id > first_id, "{second_id} after {first_id}");
}

// ---------------------------------------------------------------------------
// Sessions kept where the hosts share them
// ---------------------------------------------------------------------------

/// [`feed_host`], its readers signed in through `provider`.
fn feed_host_signing_in(url: &str, schema: &Schema, provider: &Arc<TestProvider>) -> Served {
    let s = feed_host(url, schema);
    s.identity.use_provider(provider.clone(), REDIRECT);
    s
}

/// A post through `s` by `session`, over HTTP, as a browser sends it: the
/// request reads its session as the hosts keep it.
fn posted_by(s: &Server, session: &str, interaction: &str, text: &str) -> String {
    command(
        s,
        "feed.app.post",
        session,
        interaction,
        &serde_json::json!([text]).to_string(),
    )
}

/// **A sign-in begun on one host completes on another, and its session holds
/// on both** (ADR-0369): the sign-in under way is kept in the database, so
/// the provider's callback completes it on whichever host it reaches; the
/// session it opens is kept there by its id's hash, so each host's next
/// request finds its reader. Until ADR-0369 both were the first host's
/// memory: the callback on B was refused, and a session made on A was no one
/// on B.
#[test]
fn a_sign_in_begun_on_one_host_completes_on_another_and_holds_on_both() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "signed");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let started = get(&a, "/sign-in", "pw-session=s-came");
    let (state, _) = set_cookie(&started, "pw-sign-in").expect("the sign-in's cookie");
    let back = provider.authenticate(&location(&started), "ada", "Ada");
    let answer = get(
        &b,
        &format!("/sign-in/callback?{back}"),
        &format!("pw-session=s-came; pw-sign-in={state}"),
    );
    assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
    let ada = set_cookie(&answer, "pw-session").expect("a session").0;
    for (host, text) in [(&a, "Signed in on B, posting on A"), (&b, "And on B")] {
        let answer = posted_by(host, &ada, &format!("i-{}", text.len()), text);
        assert!(answer.contains("\"committed\":true"), "{answer}");
    }
    assert_eq!(
        db.count("SELECT count(*) FROM posts WHERE author = 'ada'"),
        2,
        "both ada's"
    );
    // Kept by its hash alone: no column holds the id itself.
    let mut c = db.sql();
    let hashed: i64 = c
        .query_one(
            "SELECT count(*) FROM sessions WHERE hash = $1",
            &[&crate::identity::kept_hash(&ada)],
        )
        .expect("counted")
        .get(0);
    assert_eq!(hashed, 1, "the session kept by its hash");
    let plain: i64 = c
        .query_one(
            "SELECT count(*) FROM sessions WHERE hash = convert_to($1, 'UTF8') \
             OR user_id = $1 OR handle = $1 OR name = $1 OR issuer = $1",
            &[&ada],
        )
        .expect("counted")
        .get(0);
    assert_eq!(plain, 0, "never the id itself");
    assert_eq!(
        db.count("SELECT count(*) FROM sign_ins"),
        0,
        "the sign-in taken once"
    );
}

/// The sign-in a browser begins on `s` as `sub`, stopped before its
/// callback: the callback's path and the cookies the browser sends it.
fn begun(s: &Server, provider: &TestProvider, sub: &str) -> (String, String) {
    let started = get(s, "/sign-in", "pw-session=s-came");
    let (state, _) = set_cookie(&started, "pw-sign-in").expect("the sign-in's cookie");
    let back = provider.authenticate(&location(&started), sub, sub);
    (
        format!("/sign-in/callback?{back}"),
        format!("pw-session=s-came; pw-sign-in={state}"),
    )
}

/// **A sign-in completes once, whichever host its callback reaches again**
/// (ADR-0258's "once", held across hosts): the callback B completed,
/// replayed on A, is refused, its sign-in already taken from where the
/// hosts keep it, and no session is given.
#[test]
fn a_sign_in_completed_on_one_host_is_refused_again_on_another() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "replayed");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let (callback, cookies) = begun(&a, &provider, "ada");
    let first = get(&b, &callback, &cookies);
    assert!(first.starts_with("HTTP/1.1 303"), "{first}");
    let again = get(&a, &callback, &cookies);
    assert!(again.starts_with("HTTP/1.1 400"), "{again}");
    assert!(again.contains("already completed or is unknown"), "{again}");
    assert!(set_cookie(&again, "pw-session").is_none(), "{again}");
}

/// **A sign-in begun more than ten minutes ago is refused on any host**
/// (ADR-0258's lifetime): its age is the database's clock's, which every
/// host reads alike, not the clock of the host that began it.
#[test]
fn a_sign_in_begun_too_long_ago_is_refused_on_another_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "too_long");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let (callback, cookies) = begun(&a, &provider, "ada");
    db.sql()
        .batch_execute("UPDATE sign_ins SET started = started - interval '11 minutes'")
        .expect("begun eleven minutes ago");
    let answer = get(&b, &callback, &cookies);
    assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
    assert!(answer.contains("took too long"), "{answer}");
    assert!(set_cookie(&answer, "pw-session").is_none(), "{answer}");
}

/// **A sign-in abandoned is forgotten by the next one begun** on any host,
/// once its ten minutes are past: the table holds the sign-ins under way,
/// not every one ever begun.
#[test]
fn an_abandoned_sign_in_is_forgotten_by_the_next_one() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "abandoned");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    begun(&a, &provider, "ada");
    db.sql()
        .batch_execute("UPDATE sign_ins SET started = started - interval '11 minutes'")
        .expect("abandoned eleven minutes ago");
    begun(&b, &provider, "grace");
    assert_eq!(
        db.count("SELECT count(*) FROM sign_ins"),
        1,
        "the one under way"
    );
}

/// **A session signed out on one host is no one on another** (ADR-0258's
/// sign-out, held across hosts): B's next request reads no session, and its
/// post is refused for `SignedIn`.
#[test]
fn a_session_signed_out_on_one_host_is_no_one_on_another() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "signed_out");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    let answer = posted_by(&b, &ada, "i-1", "Signed in on A, posting on B");
    assert!(answer.contains("\"committed\":true"), "{answer}");
    let out = exchanged(
        &a,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={ada}\r\n\
             Sec-Fetch-Site: same-origin\r\ncontent-length: 0\r\n\r\n"
        ),
    );
    assert!(out.starts_with("HTTP/1.1 303"), "{out}");
    let refused = posted_by(&b, &ada, "i-2", "Signed out on A");
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(refused.contains("SignedIn"), "{refused}");
    assert_eq!(db.count("SELECT count(*) FROM sessions"), 0, "none kept");
}

/// **A restarted host keeps its readers signed in** (ADR-0369): a host
/// started again on the database finds the session a sign-in made before.
/// Until ADR-0369 a restart signed everyone out.
#[test]
fn a_restarted_host_keeps_its_readers_signed_in() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "restarted");
    let provider = Arc::new(TestProvider::default());
    let ada = {
        let a = feed_host_signing_in(&url, &db, &provider);
        super::sign_in::signed_in(&a, &provider, "s-came", "ada")
    };
    let again = feed_host_signing_in(&url, &db, &provider);
    let answer = posted_by(&again, &ada, "i-1", "After a restart");
    assert!(answer.contains("\"committed\":true"), "{answer}");
}

/// **An existing session is admitted once, and inspected before delivery**:
/// one activity read plus one read-only authority check (ADR-0389). A fresh
/// cookie has no admission read and still needs its anonymous delivery check.
#[test]
fn a_request_admits_once_and_inspects_before_private_delivery() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "read_once");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    let kept = a.data.sessions().expect("sessions kept");
    let before = kept.ran();
    let page = get(&a, "/", &format!("pw-session={ada}"));
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert_eq!(
        kept.ran() - before,
        2,
        "exactly one activity admission and one final read-only authority check"
    );
    let before = kept.ran();
    let fresh = get(&a, "/", "");
    assert!(fresh.starts_with("HTTP/1.1 200"), "{fresh}");
    assert_eq!(
        kept.ran() - before,
        1,
        "a fresh anonymous cookie has only its final read-only authority check"
    );
}

/// The body of an HTTP answer.
fn body_of(answer: &str) -> &str {
    answer.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

/// `session`'s post of `text`, pressed on the page numbered `page`, sent to
/// `s` over HTTP as the browser sends it: its page named (`pw-document`) and
/// its interaction (`pw-interaction`).
fn pressed_on(s: &Server, session: &str, page: u64, interaction: &str, text: &str) -> String {
    let body = serde_json::json!([text]).to_string();
    exchanged(
        s,
        &format!(
            "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\npw-interaction: {interaction}\r\n\
             pw-document: {page}\r\ncontent-type: application/json\r\n\
             content-length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// How many posts say `text`.
fn posts_saying(db: &Schema, text: &str) -> i64 {
    db.sql()
        .query_one("SELECT count(*) FROM posts WHERE text = $1", &[&text])
        .expect("counted")
        .get(0)
}

/// **Hosts on one database number their pages apart** (the integrator's
/// ruling 1 of 2026-10-20): each host's numbers carry its instance, taken
/// from the database as it opened its layer, so A's first page and B's never
/// share a number, and each host tells the other's pages from its own. Each
/// stays below JavaScript's exact integers (2^53), as the runtime reads it.
/// Until then every host numbered its pages from one, and B took A's page
/// of a session for its own page of that session.
#[test]
fn hosts_on_one_database_number_their_pages_apart() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "numbered");
    let a = feed_host(&url, &db);
    let b = feed_host(&url, &db);
    let on_a = home(&a, "a").1;
    let on_b = home(&b, "b").1;
    assert_ne!(on_a, on_b, "apart");
    assert!(
        crate::hosts::another_hosts(on_a, b.first_page)
            && crate::hosts::another_hosts(on_b, a.first_page),
        "each another host's to the other"
    );
    assert!(
        !crate::hosts::another_hosts(on_a, a.first_page),
        "and its own to each"
    );
    for page in [on_a, on_b] {
        assert!(page < 1 << 53, "{page} exact in JavaScript");
    }
}

/// **A press made on a page one host served, sent again to another, is
/// answered from what the first committed** (ADR-0325, across hosts, the
/// integrator's ruling 1 of 2026-10-20): the request names its page by A's
/// number, in A's instance, and B answers from the record A's commit wrote
/// with its writes before it would refuse the page; nothing runs twice. A
/// press on that page that never committed is refused as another host's
/// page, never as one out of date. Until then B refused the retry, though A
/// had committed it.
#[test]
fn a_press_on_one_hosts_page_sent_again_to_another_is_answered_from_its_record() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "pressed_on");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    let on_a = home(&a, &ada).1;
    let text = "Pressed on A's page";
    let first = pressed_on(&a, &ada, on_a, "i-1", text);
    assert!(first.contains("\"committed\":true"), "{first}");
    let again = pressed_on(&b, &ada, on_a, "i-1", text);
    assert!(again.starts_with("HTTP/1.1 202"), "{again}");
    // What A committed, and its result: a kept answer carries no basis of
    // values, on one host as on two.
    let answer = |a: &str| {
        let v: serde_json::Value = serde_json::from_str(body_of(a)).expect("JSON");
        (v["committed"].clone(), v["result"].clone())
    };
    assert_eq!(answer(&again), answer(&first), "A's answer");
    assert_eq!(answer(&again).0, serde_json::json!(true));
    assert_eq!(posts_saying(&db, text), 1, "posted once");
    let never = pressed_on(&b, &ada, on_a, "i-2", text);
    assert!(never.starts_with("HTTP/1.1 403"), "{never}");
    assert!(never.contains("\"refused\":\"another-host\""), "{never}");
    assert_eq!(posts_saying(&db, text), 1, "and nothing ran");
}

/// **A press on a page another host served, which committed nowhere, is
/// refused as that host's page** (the integrator's ruling 1 of 2026-10-20):
/// ada has a page on each host; her press on A's page, sent to B, is refused
/// 403 `another-host`, in the platform's words, and nothing runs, where B
/// once took it for its own page of hers (both numbered from one) and ran
/// it. The controls: the same press sent to A runs, as one on B's own page
/// does on B; one on a page B gave and no longer holds, or one numbered in
/// no host's instance, is a page out of date, as before.
#[test]
fn a_press_on_another_hosts_page_that_committed_nowhere_is_refused_as_that_hosts() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "another_host");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    // What B's request for her page does first.
    b.identity.principals().refresh(&ada);
    let on_a = home(&a, &ada).1;
    let on_b = home(&b, &ada).1;
    let refused = pressed_on(&b, &ada, on_a, "i-1", "Pressed on A's page, sent to B");
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(
        refused.contains("\"refused\":\"another-host\"")
            && refused
                .contains("\"says\":\"This page is served by another host. Reload it to go on.\""),
        "{refused}"
    );
    assert_eq!(
        posts_saying(&db, "Pressed on A's page, sent to B"),
        0,
        "and nothing ran"
    );
    // The controls.
    let on_its_host = pressed_on(&a, &ada, on_a, "i-1", "Pressed on A's page, sent to B");
    assert!(on_its_host.contains("\"committed\":true"), "{on_its_host}");
    let own = pressed_on(&b, &ada, on_b, "i-2", "Pressed on B's page, on B");
    assert!(own.contains("\"committed\":true"), "{own}");
    b.pages.lock().expect("pages").remove(&(ada.clone(), on_b));
    for page in [on_b, 7] {
        let gone = pressed_on(&b, &ada, page, "i-3", "Pressed on a page B does not hold");
        assert!(gone.starts_with("HTTP/1.1 403"), "{page}: {gone}");
        assert!(
            gone.contains("\"refused\":\"out-of-date\""),
            "{page}: {gone}"
        );
    }
}

/// Whether `session`'s post of `text` through `s` committed, over HTTP.
fn posts(s: &Server, session: &str, interaction: &str, text: &str) -> bool {
    posted_by(s, session, interaction, text).contains("\"committed\":true")
}

/// Whether `session`'s post through `s` was refused for `SignedIn`: it is
/// no one there.
fn refused_as_no_one(s: &Server, session: &str, interaction: &str) -> bool {
    let answer = posted_by(s, session, interaction, "A post by no one");
    answer.starts_with("HTTP/1.1 403") && answer.contains("SignedIn")
}

/// **A session idle past its idle time is signed out on every host** (the
/// integrator's ruling 2 of 2026-10-20: fourteen days where the deployment
/// says nothing), by the database's clock and cleanly: its row is gone, and
/// each host's next request finds no one. The control: idle thirteen days,
/// it holds.
#[test]
fn a_session_idle_past_its_idle_time_is_signed_out_on_every_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "idle");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    db.sql()
        .batch_execute("UPDATE sessions SET seen = clock_timestamp() - interval '13 days'")
        .expect("idle thirteen days");
    assert!(posts(&b, &ada, "i-1", "Idle thirteen days"), "it holds");
    db.sql()
        .batch_execute("UPDATE sessions SET seen = clock_timestamp() - interval '15 days'")
        .expect("idle fifteen days");
    assert!(refused_as_no_one(&b, &ada, "i-2"), "signed out on B");
    assert_eq!(
        db.count("SELECT count(*) FROM sessions"),
        0,
        "cleanly: its row is gone"
    );
    assert!(refused_as_no_one(&a, &ada, "i-3"), "and on A");
}

/// **A session past its lifetime is signed out however it is used** (the
/// integrator's ruling 2 of 2026-10-20: sixty days where the deployment says
/// nothing), on every host, its row gone. The control: opened fifty-nine
/// days ago and used now, it holds.
#[test]
fn a_session_past_its_lifetime_is_signed_out_however_it_is_used() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "lifetime");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    db.sql()
        .batch_execute("UPDATE sessions SET opened = clock_timestamp() - interval '59 days'")
        .expect("opened fifty-nine days ago");
    assert!(
        posts(&b, &ada, "i-1", "Opened fifty-nine days ago"),
        "it holds"
    );
    db.sql()
        .batch_execute(
            "UPDATE sessions SET opened = clock_timestamp() - interval '61 days', \
             seen = clock_timestamp()",
        )
        .expect("opened sixty-one days ago, used now");
    assert!(refused_as_no_one(&b, &ada, "i-2"), "signed out on B");
    assert_eq!(
        db.count("SELECT count(*) FROM sessions"),
        0,
        "its row is gone"
    );
    assert!(refused_as_no_one(&a, &ada, "i-3"), "and on A");
}

/// **A session in use is recorded as used**, on the database's clock, at
/// most once a minute: idle thirteen days, one request makes it used now, so
/// it holds fourteen days more; a request a moment later writes nothing.
#[test]
fn a_session_in_use_is_recorded_as_used_at_most_once_a_minute() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "used");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    db.sql()
        .batch_execute("UPDATE sessions SET seen = clock_timestamp() - interval '13 days'")
        .expect("idle thirteen days");
    let cookie = format!("pw-session={ada}");
    assert!(get(&b, "/", &cookie).starts_with("HTTP/1.1 200"));
    assert_eq!(
        db.count(
            "SELECT count(*) FROM sessions WHERE seen > clock_timestamp() - interval '1 minute'"
        ),
        1,
        "used now"
    );
    let seen = || -> String {
        db.sql()
            .query_one("SELECT seen::text FROM sessions", &[])
            .expect("read")
            .get(0)
    };
    let before = seen();
    assert!(get(&b, "/", &cookie).starts_with("HTTP/1.1 200"));
    assert_eq!(seen(), before, "not written again within the minute");
}

/// **A deployment states how long its sessions live**, or its host is
/// refused at start (the integrator's ruling 2 of 2026-10-20; read as
/// `PW_RENDER_SEATS` is): `PW_SESSION_IDLE` and `PW_SESSION_LIFETIME`, whole
/// seconds, one at least; fourteen days and sixty where unset or empty. And
/// a host holds its sessions to them: idle two hours, a session holds by the
/// defaults, and is signed out where the idle time is one hour.
#[test]
fn a_deployment_states_how_long_its_sessions_live() {
    use crate::identity::SessionLifetimes;
    let day = std::time::Duration::from_secs(86_400);
    let defaults = SessionLifetimes {
        idle: day * 14,
        absolute: day * 60,
    };
    assert_eq!(SessionLifetimes::stated(None, None), Ok(defaults));
    assert_eq!(SessionLifetimes::stated(Some(""), Some("")), Ok(defaults));
    assert_eq!(
        SessionLifetimes::stated(Some("3600"), Some("86400")),
        Ok(SessionLifetimes {
            idle: std::time::Duration::from_secs(3600),
            absolute: day,
        })
    );
    for (idle, lifetime, named) in [
        (Some("0"), None, "PW_SESSION_IDLE"),
        (Some("an hour"), None, "PW_SESSION_IDLE"),
        (None, Some("0"), "PW_SESSION_LIFETIME"),
        (None, Some("-1"), "PW_SESSION_LIFETIME"),
    ] {
        let refused = SessionLifetimes::stated(idle, lifetime).expect_err("refused");
        assert!(refused.contains(named), "{refused}");
    }
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "lifetimes");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let ada = super::sign_in::signed_in(&a, &provider, "s-came", "ada");
    let idle_two_hours = "UPDATE sessions SET seen = clock_timestamp() - interval '2 hours'";
    db.sql().batch_execute(idle_two_hours).expect("idle");
    assert!(posts(&b, &ada, "i-1", "Idle two hours, by the defaults"));
    b.identity.principals().expire_after(SessionLifetimes {
        idle: std::time::Duration::from_secs(3600),
        absolute: day * 60,
    });
    db.sql().batch_execute(idle_two_hours).expect("idle");
    assert!(refused_as_no_one(&b, &ada, "i-2"), "one hour idle allowed");
}

/// **A session in one host's memory alone expires** (the integrator's
/// ruling 2 of 2026-10-20), by the host's clock, where no database keeps
/// sessions: idle past its idle time, or past its lifetime however it was
/// used, it is no one in the accounts model, its own guest in the guest
/// model, and forgotten; used within its idle time it holds, and its use is
/// recorded.
#[test]
fn a_session_in_one_hosts_memory_alone_expires_idle_and_past_its_lifetime() {
    use crate::identity::{Principal, Principals, SessionLifetimes};
    use std::time::Duration;
    let lifetimes = SessionLifetimes {
        idle: Duration::from_secs(50),
        absolute: Duration::from_secs(200),
    };
    let identity = crate::identity::Identity::default();
    identity.signed_in_for_test("s-ada", "ada", "Ada");
    let principals = identity.principals();
    principals.expire_after(lifetimes);
    let ada = principals.of("s-ada").expect("signed in");
    let aged = |p: &Principals, session: &str, opened_ago: u64, seen_after: u64| {
        p.held_for_test(
            session,
            ada.clone(),
            Duration::from_secs(opened_ago),
            Duration::from_secs(seen_after),
        );
    };
    aged(&principals, "idle", 100, 0);
    aged(&principals, "used", 100, 90);
    aged(&principals, "old", 300, 299);
    assert_eq!(principals.of("idle"), None, "idle past fifty seconds");
    assert_eq!(
        principals.of("old"),
        None,
        "past its two hundred, used a moment ago"
    );
    assert_eq!(
        principals.of("used"),
        Some(ada.clone()),
        "used ten seconds ago"
    );
    assert_eq!(
        principals.seen_for_test("used"),
        Some(Duration::from_secs(90)),
        "an internal principal read cannot renew idle lifetime"
    );
    principals.refresh("used"); // Explicit client admission records activity.
    let recorded = principals.seen_for_test("used").expect("held");
    assert!(
        recorded >= Duration::from_secs(100),
        "its use recorded now: {recorded:?}"
    );
    for gone in ["idle", "old"] {
        assert_eq!(principals.seen_for_test(gone), None, "{gone} forgotten");
    }
    // The guest model: a session expired is its own guest.
    let guests = Principals::default();
    guests.expire_after(lifetimes);
    aged(&guests, "idle-guest", 100, 0);
    assert_eq!(
        guests.of("idle-guest"),
        Some(Principal::guest("idle-guest")),
        "its own guest"
    );
}

/// **Signing in gives a new session, on every host** (the integrator's
/// ruling 2 of 2026-10-20; OWASP's session fixation defence): the session the
/// browser came with, which another could have set, is no one on either
/// host; and signing in again as another user, a change of whom the session
/// is, gives another, and the one before it is no one on either host. A
/// sign-out gives a new one too (ADR-0258), and its old one is no one on any
/// host (`a_session_signed_out_on_one_host_is_no_one_on_another`).
#[test]
fn signing_in_rotates_the_session_on_every_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "rotated");
    let provider = Arc::new(TestProvider::default());
    let a = feed_host_signing_in(&url, &db, &provider);
    let b = feed_host_signing_in(&url, &db, &provider);
    let came = "s-set-by-someone-else";
    let ada = super::sign_in::signed_in(&a, &provider, came, "ada");
    assert_ne!(ada, came, "a new session");
    for (host, i) in [(&a, "i-1"), (&b, "i-2")] {
        assert!(
            refused_as_no_one(host, came, i),
            "the one it came with is no one"
        );
    }
    assert!(posts(&b, &ada, "i-3", "Ada on B"));
    let grace = super::sign_in::signed_in(&a, &provider, &ada, "grace");
    assert_ne!(grace, ada, "another, signing in as another user");
    for (host, i) in [(&a, "i-4"), (&b, "i-5")] {
        assert!(refused_as_no_one(host, &ada, i), "ada's is no one now");
    }
    assert!(posts(&b, &grace, "i-6", "Grace on B"));
    assert_eq!(
        db.count("SELECT count(*) FROM posts WHERE author = 'grace'"),
        1,
        "grace's post"
    );
}

/// **`PW_FEED_SCHEMA` is a test's setting** (the integrator's ruling 4 of
/// 2026-10-20): honored for a development deployment on this machine, which
/// a test run's hosts are, and refused for a production one or one served
/// from elsewhere, whose tables are where its connection's search path says.
#[test]
fn the_feeds_schema_is_a_tests_setting() {
    use crate::identity::Deployment;
    let dev = Deployment::development(3143);
    assert_eq!(
        crate::hosts::test_schema(Some("pw_e2e"), &dev),
        Ok(Some("pw_e2e".to_string()))
    );
    assert_eq!(crate::hosts::test_schema(None, &dev), Ok(None));
    assert_eq!(crate::hosts::test_schema(Some(""), &dev), Ok(None));
    for deployment in [
        Deployment {
            development: false,
            origin: "http://127.0.0.1:3143".to_string(),
        },
        Deployment {
            development: true,
            origin: "https://feed.example.com".to_string(),
        },
    ] {
        let refused = crate::hosts::test_schema(Some("pw_e2e"), &deployment).expect_err("refused");
        assert!(refused.contains("PW_FEED_SCHEMA"), "{refused}");
        assert_eq!(
            crate::hosts::test_schema(None, &deployment),
            Ok(None),
            "unset, nothing to refuse"
        );
    }
}

/// **A layer's sessions are read on a bounded few connections**, as the
/// render merge holds every pool (stated, and waited for past the bound):
/// twenty-four requests at once, each holding its connection half a second,
/// are all answered, and no more than [`crate::hosts::SESSION_CONNECTIONS`]
/// connections are open at once, that many reached. Until then a connection
/// was made for every request that asked.
#[test]
fn a_layers_sessions_are_read_on_a_bounded_few_connections() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "sessions_bound");
    let sessions = crate::hosts::SessionsPg::new(&url, &db.name);
    std::thread::scope(|scope| {
        let held: Vec<_> = (0..24)
            .map(|_| scope.spawn(|| sessions.held_for_test(0.5)))
            .collect();
        for one in held {
            one.join().expect("a request").expect("answered");
        }
    });
    assert_eq!(
        sessions.most_open(),
        crate::hosts::SESSION_CONNECTIONS,
        "the bound reached, and held"
    );
}

// ---------------------------------------------------------------------------
// The store on two hosts
// ---------------------------------------------------------------------------

/// The database the store's tests run against.
fn store_database() -> Option<String> {
    match std::env::var("PW_STORE_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_STORE_DATABASE_URL is not set");
            None
        }
    }
}

/// **Two hosts of the canonical store on one schema**, each its own layer,
/// their readers signed in through `provider`; dropped B, then A, then
/// their layers, A's dropping the schema.
struct Stores {
    b: Server,
    a: Server,
    b_layer: Arc<StorePg>,
    a_layer: Arc<StorePg>,
    _dir: tempfile::TempDir,
}

fn stores(url: &str, provider: &Arc<TestProvider>) -> Stores {
    let a_layer = Arc::new(StorePg::for_test(url).expect("A's layer"));
    let b_layer = Arc::new(
        StorePg::open_with(
            url,
            a_layer.schema(),
            Isolation::Serializable,
            RenderSeats::default(),
        )
        .expect("B's layer"),
    );
    let (dir, out) = built_from_patches_in("examples", |app| app.to_string(), &[]);
    let host = |layer: &Arc<StorePg>| {
        let s = Server::from_build_layers(out.clone(), out.clone(), None, Some(layer.clone()))
            .expect("served on PostgreSQL");
        s.identity.use_provider(provider.clone(), REDIRECT);
        s
    };
    Stores {
        a: host(&a_layer),
        b: host(&b_layer),
        b_layer,
        a_layer,
        _dir: dir,
    }
}

/// **A commit through the store waits for the outbox's lock, and wakes a
/// waiting host at once**, as the feed's does (ADR-0369).
#[test]
fn a_store_commit_waits_for_the_outbox_lock_and_wakes_another_host() {
    let Some(url) = store_database() else { return };
    let provider = Arc::new(TestProvider::default());
    let hosts = stores(&url, &provider);
    let ada = super::sign_in::signed_in(&hosts.a, &provider, "came-ada", "ada");
    assert_eq!(hosts.b_layer.schema(), hosts.a_layer.schema(), "one schema");
    waits_for_the_lock_and_wakes(hosts.a_layer.sql(), &hosts.a, &hosts.b, || {
        hosts
            .a
            .command(ADD, &ada, &add_shown("espresso", 1), false)
            .expect("ada adds through A");
    });
}

const CART: &str = "store.page.CartPage~ui";

/// **A reader's cart changed through one host is told to their page on
/// another**: ada, signed in through A, reads her cart on B (her session
/// found in the database by B's request); her add through A reaches B's page
/// when B reads the outbox, by principal, as her other session on one host
/// is told (ADR-0318).
#[test]
fn a_readers_cart_changed_through_one_host_is_told_to_their_page_on_another() {
    let Some(url) = store_database() else { return };
    let provider = Arc::new(TestProvider::default());
    let hosts = stores(&url, &provider);
    let ada = super::sign_in::signed_in(&hosts.a, &provider, "came-ada", "ada");
    // What B's request for the page does first.
    hosts.b.identity.principals().refresh(&ada);
    hosts
        .b
        .serve_document_settled(&ada, CART, &Params::new(), &[])
        .expect("served on B");
    let doc = latest(&hosts.b.pending.lock().expect("pending"), &ada);
    let before = sets_of(&hosts.b, &doc).len();
    hosts
        .a
        .command(ADD, &ada, &add_shown("espresso", 1), false)
        .expect("ada adds through A");
    assert_eq!(
        sets_of(&hosts.b, &doc).len(),
        before,
        "not before B reads it"
    );
    hosts.b.hear();
    let told: Vec<String> = sets_of(&hosts.b, &doc)[before..]
        .iter()
        .map(written)
        .collect();
    assert!(
        told.iter().any(|w| w.contains("$3.50")),
        "her page on B is told: {told:?}"
    );
}

/// **A retry through another host of a press one host committed runs once**
/// (ADR-0325, held across two live hosts): the same interaction sent to B
/// is answered from A's record, committed with A's writes, and the cart
/// holds the one line.
#[test]
fn a_retry_through_another_host_is_answered_from_the_first_hosts_record() {
    let Some(url) = store_database() else { return };
    let provider = Arc::new(TestProvider::default());
    let hosts = stores(&url, &provider);
    let ada = super::sign_in::signed_in(&hosts.a, &provider, "came-ada", "ada");
    let request = [shown("espresso"), serde_json::json!(1)];
    let first = hosts
        .a
        .command_answer(ADD, &ada, &request, Some("press-1"))
        .expect("a well-formed request");
    assert!(first.committed, "{first:?}");
    hosts.b.identity.principals().refresh(&ada);
    let again = hosts
        .b
        .command_answer(ADD, &ada, &request, Some("press-1"))
        .expect("a well-formed request");
    assert_eq!(again, first, "A's answer");
    assert_eq!(hosts.b.cart_lines(&ada), [("espresso".to_string(), 1)]);
    assert_eq!(hosts.a.cart_lines(&ada), [("espresso".to_string(), 1)]);
}
