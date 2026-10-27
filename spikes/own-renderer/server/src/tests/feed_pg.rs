//! **The feed on PostgreSQL** (ADR-0246): the feed reference app served by
//! the host, its data in a real database, held to what its `source` states.
//!
//! Each test runs against `PW_FEED_DATABASE_URL`, in a schema of its own that
//! it drops when done, and passes doing nothing without one: `just
//! e14-feed-postgres` is what runs them, and says so when it has no database.

use super::*;
use crate::feed_pg::{FeedPg, Isolation};

/// The database the tests run against, where the environment names one.
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
pub(super) struct Schema {
    url: String,
    name: String,
}

impl Schema {
    fn new(url: &str, test: &str) -> Schema {
        let name = format!("pw_test_{}_{test}", std::process::id());
        let schema = Schema {
            url: url.to_string(),
            name,
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

    /// A connection in the test's schema, to look at what was committed.
    fn sql(&self) -> postgres::Client {
        let mut c = self.admin();
        c.batch_execute(&format!("SET search_path TO {}", self.name))
            .expect("the test's schema");
        c
    }

    pub(super) fn count(&self, query: &str) -> i64 {
        self.sql().query_one(query, &[]).expect(query).get(0)
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.name));
        }
    }
}

/// **The feed, built and served on PostgreSQL**, its server dropped before
/// its schema.
pub(super) struct OnPostgres {
    served: Served,
    pub(super) db: Schema,
}

impl std::ops::Deref for OnPostgres {
    type Target = Server;
    fn deref(&self) -> &Server {
        &self.served
    }
}

/// The feed, its `app.pw` changed by `change`, served on the layer
/// `open` opens in the test's schema, or why the host refused it.
fn served_with(
    url: &str,
    test: &str,
    change: fn(&str) -> String,
    open: impl FnOnce(&str) -> Result<FeedPg, String>,
) -> Result<OnPostgres, String> {
    let db = Schema::new(url, test);
    let layer = open(&db.name)?;
    let (dir, out) = built_feed_with(change);
    let server = Server::from_build_with(out.clone(), out, Some(Arc::new(layer)))?;
    Ok(OnPostgres {
        served: Served { server, _dir: dir },
        db,
    })
}

/// The feed as it is, on PostgreSQL.
pub(super) fn served_on(url: &str, test: &str) -> OnPostgres {
    served(url, test)
}

/// The feed as it is, on PostgreSQL.
fn served(url: &str, test: &str) -> OnPostgres {
    served_with(
        url,
        test,
        |app| app.to_string(),
        |schema| FeedPg::open(url, Some(schema)),
    )
    .expect("served on PostgreSQL")
}

/// **Every row the outbox is given is seen**, with the transaction that
/// gave it: what a test compares with the transaction that wrote the post.
/// Installed in the test's schema only.
fn audit_the_outbox(db: &Schema) {
    db.sql()
        .batch_execute(
            "CREATE TABLE outbox_seen (name text NOT NULL, committed_in xid8 NOT NULL);
             CREATE FUNCTION outbox_seen() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 INSERT INTO outbox_seen VALUES (NEW.name, NEW.committed_in);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER outbox_seen AFTER INSERT ON outbox
                 FOR EACH ROW EXECUTE FUNCTION outbox_seen();",
        )
        .expect("the outbox audited");
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

/// **An interaction's answer outlives its host, on the feed too**
/// (ADR-0325): a post committed on one host, and sent again to a host
/// started after it over the same schema, is answered as the first was, and
/// the feed holds the one post. Until ADR-0325 the second posted it again.
#[test]
fn on_postgres_a_posts_interaction_outlives_its_host() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "outlives");
    let host = || {
        let layer = FeedPg::open(&url, Some(&db.name)).expect("the layer");
        let (dir, out) = built_feed_with(|app| app.to_string());
        let server =
            Server::from_build_with(out.clone(), out, Some(Arc::new(layer))).expect("served");
        Served { server, _dir: dir }
    };
    let first = {
        let s = host();
        posted(&s, "/command/feed.app.post", "a", "i-1", "[\"Once\"]")
    };
    assert!(first.contains("\"committed\":true"), "{first}");
    let posts = db.count("SELECT count(*) FROM posts");
    let s = host();
    let again = posted(&s, "/command/feed.app.post", "a", "i-1", "[\"Once\"]");
    assert!(again.contains("\"committed\":true"), "{again}");
    assert_eq!(
        db.count("SELECT count(*) FROM posts"),
        posts,
        "the one post"
    );
    assert_eq!(
        db.count("SELECT count(*) FROM interactions"),
        1,
        "its record, committed with it"
    );
}

fn thread_of(id: &str) -> Params {
    Params::from([("id".to_string(), id.to_string())])
}

/// **What the database provides, measured** (ADR-0246): a command's
/// transaction is serializable though the connection's default is read
/// committed, a read is of the latest commit, and no change feed.
#[test]
fn on_postgres_the_database_provides_what_the_feed_states() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "provides");
    let default: String = db
        .sql()
        .query_one("SHOW default_transaction_isolation", &[])
        .expect("shown")
        .get(0);
    assert_eq!(
        default, "read committed",
        "the control: the server's default"
    );
    let layer = FeedPg::open(&url, Some(&db.name)).expect("opened");
    let provided = crate::data::DataLayer::provides(&layer).expect("measured");
    assert_eq!(provided.transactions, "serializable");
    assert_eq!(
        provided.reads,
        std::collections::BTreeSet::from(["strong".to_string()])
    );
    assert!(!provided.feed);
}

/// **A post commits with its event, in one transaction** (ADR-0208): the
/// post's row and the outbox's are written by one transaction, and what was
/// delivered is kept for the other hosts, the committing host's
/// (ADR-0369).
#[test]
fn on_postgres_a_post_commits_with_its_event_in_one_transaction() {
    let Some(url) = database() else { return };
    let s = served(&url, "post");
    audit_the_outbox(&s.db);
    let answered = post(&s, "a", "Kept in PostgreSQL", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    let mut sql = s.db.sql();
    let rows = sql
        .query(
            "SELECT s.name, s.committed_in = p.committed_in \
             FROM outbox_seen s, posts p WHERE p.text = 'Kept in PostgreSQL'",
            &[],
        )
        .expect("read");
    let seen: Vec<(String, bool)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(
        seen,
        [("feed.app.Posted".to_string(), true)],
        "one event, written by the post's own transaction"
    );
    // Delivered, and kept for the other hosts on the database, which read
    // it from their cursors (ADR-0369): until then it was consumed.
    assert_eq!(
        s.db.count("SELECT count(*) FROM outbox"),
        1,
        "delivered, and kept for the other hosts"
    );
    let host = s.data.tail().expect("a shared database").host().to_string();
    assert_eq!(
        s.db.sql()
            .query_one("SELECT count(*) FROM outbox WHERE host = $1", &[&host])
            .expect("counted")
            .get::<_, i64>(0),
        1,
        "the committing host's"
    );
}

/// **A post reaches another session's open timeline** (ADR-0219), as with
/// the in-memory layer: not before the author is answered, and once the
/// telling runs.
#[test]
fn on_postgres_a_post_reaches_another_session() {
    let Some(url) = database() else { return };
    let s = served(&url, "reaches");
    for session in ["a", "b"] {
        let (html, _, _, _) = s
            .serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        assert!(html.contains("Hello, feed."), "{html}");
    }
    let theirs = latest(&s.pending.lock().expect("pending"), "b");
    let before = sets_of(&s, &theirs).len();
    let answered = post(&s, "a", "Seen by everyone, from PostgreSQL", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(sets_of(&s, &theirs).len(), before, "not before the answer");
    s.tell_waiting();
    let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
    assert!(set.contains("Seen by everyone, from PostgreSQL"), "{set}");
    assert!(set.contains("Guest a"), "{set}");
}

/// **A like counts, and reaches the thread another session shows**.
#[test]
fn on_postgres_a_like_counts_and_reaches_an_open_thread() {
    let Some(url) = database() else { return };
    let s = served(&url, "like");
    let (html, _, _, _) = s
        .serve_document_settled("b", "feed.app.PostPage~ui", &thread_of("p1"), &[])
        .expect("served");
    assert!(html.contains("2 likes"), "{html}");
    let theirs = latest(&s.pending.lock().expect("pending"), "b");
    let answered = s
        .command_answered(
            "feed.app.like",
            "a",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(
        s.db.count("SELECT count(*) FROM likes WHERE post = 'p1'"),
        3
    );
    s.tell_waiting();
    let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
    assert!(set.contains("3 likes"), "{set}");
    // A post that is not there: its declared error, and nothing written.
    let missing = s
        .command_answered(
            "feed.app.like",
            "a",
            &[Val::String("p99".into())],
            Some("i-2"),
        )
        .expect("runs");
    assert!(!missing.committed, "{:?}", missing.result);
    assert_eq!(s.db.count("SELECT count(*) FROM likes"), 4);
}

/// **A reply is in its thread, as deep as it goes**, and a reply to a post
/// that is not there writes nothing.
#[test]
fn on_postgres_a_reply_is_in_its_thread_as_deep_as_it_goes() {
    let Some(url) = database() else { return };
    let s = served(&url, "reply");
    let answered = s
        .command_answered(
            "feed.app.reply",
            "a",
            &[
                Val::String("p3".into()),
                Val::String("Three deep, in PostgreSQL".into()),
            ],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    let (html, _, _, _) = s
        .serve_document_settled("c", "feed.app.PostPage~ui", &thread_of("p1"), &[])
        .expect("served");
    // p1, its reply p2, p2's reply p3, and p3's reply: in that order.
    let at = |text: &str| html.find(text).unwrap_or_else(|| panic!("{text}: {html}"));
    assert!(at("Hello, feed.") < at("Hello, Ada."));
    assert!(at("Hello, Ada.") < at("And a reply to the reply."));
    assert!(at("And a reply to the reply.") < at("Three deep, in PostgreSQL"));
    assert_eq!(
        s.db.count("SELECT count(*) FROM posts WHERE reply_to = 'p3'"),
        1
    );
    let nowhere = s
        .command_answered(
            "feed.app.reply",
            "a",
            &[Val::String("p99".into()), Val::String("To no one".into())],
            Some("i-2"),
        )
        .expect("runs");
    assert!(!nowhere.committed, "{:?}", nowhere.result);
    assert_eq!(s.db.count("SELECT count(*) FROM posts"), 4);
}

/// **The timeline is read a page at a time**: twenty, then forty when the
/// page asks for more, newest first, replies in none.
#[test]
fn on_postgres_the_timeline_loads_more() {
    let Some(url) = database() else { return };
    let s = served(&url, "pages");
    s.db.sql()
        .batch_execute(
            "INSERT INTO posts (author, text) \
             SELECT 'u-ada', 'Numbered ' || n FROM generate_series(1, 30) n",
        )
        .expect("thirty posts");
    let (html, _, _, _) = s
        .serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
        .expect("served");
    // The newest twenty: 30 down to 11.
    assert!(html.contains("Numbered 30<"), "{html}");
    assert!(html.contains("Numbered 11<"), "{html}");
    assert!(!html.contains("Numbered 10<"), "{html}");
    assert!(!html.contains("Hello, feed."), "{html}");
    let doc = latest(&s.pending.lock().expect("pending"), "a");
    let more = BTreeMap::from([("shown".to_string(), serde_json::json!(40))]);
    let read = s.read_keyed("a", "feed", 1, doc.1, &more, STAYED);
    assert_eq!(read, Ok(KeyOutcome::Applied));
    let set = format!("{:?}", sets_of(&s, &doc).pop().expect("a patch set"));
    assert!(set.contains("Numbered 1<"), "{set}");
    assert!(set.contains("Hello, feed."), "{set}");
    assert!(
        !set.contains("Hello, Ada."),
        "a reply is in no timeline: {set}"
    );
}

/// **A command refused mid-transaction leaves no rows and no events**: the
/// database refuses the post's commit (a constraint checked at commit, after
/// the post and its event were written in the transaction), the author is
/// answered that nothing committed, no other session is told, and the next
/// post commits on the same connections.
#[test]
fn on_postgres_a_command_refused_at_its_commit_leaves_nothing() {
    let Some(url) = database() else { return };
    let s = served(&url, "refused");
    audit_the_outbox(&s.db);
    s.db.sql()
        .batch_execute(
            "CREATE FUNCTION refuse_at_commit() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.text = 'Refused at commit' THEN
                     RAISE EXCEPTION 'refused at commit';
                 END IF;
                 RETURN NULL;
             END $$;
             CREATE CONSTRAINT TRIGGER refuse_at_commit AFTER INSERT ON posts
                 DEFERRABLE INITIALLY DEFERRED
                 FOR EACH ROW EXECUTE FUNCTION refuse_at_commit();",
        )
        .expect("a refusal at commit");
    for session in ["a", "b"] {
        s.serve_document_settled(session, "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
    }
    let (mine, theirs) = {
        let pending = s.pending.lock().expect("pending");
        (latest(&pending, "a"), latest(&pending, "b"))
    };
    let (mine_before, theirs_before) = (sets_of(&s, &mine).len(), sets_of(&s, &theirs).len());
    let answer = posted(
        &s,
        "/command/feed.app.post",
        "a",
        "i-1",
        "[\"Refused at commit\"]",
    );
    assert!(
        answer.contains("\"outcome\":\"unknown\"") && !answer.contains("\"committed\":false"),
        "commit errors are conservatively unknown without a confirming receipt: {answer}"
    );
    // Why, as the host has it: the database's refusal, at the commit.
    let why = s
        .command_answered(
            "feed.app.post",
            "a",
            &[Val::String("Refused at commit".into())],
            Some("i-0"),
        )
        .expect_err("refused");
    assert!(why.contains("P0001: refused at commit"), "{why}");
    s.tell_waiting();
    assert_eq!(s.db.count("SELECT count(*) FROM posts"), 3, "no post");
    assert_eq!(s.db.count("SELECT count(*) FROM outbox"), 0, "no event");
    assert_eq!(
        s.db.count("SELECT count(*) FROM outbox_seen"),
        0,
        "no event written by any committed transaction"
    );
    // An unknown acknowledgement triggers an authoritative refresh even
    // when this deferred constraint actually rolled back. Empty convergence
    // frames are allowed; attempted business patches/events are not.
    assert!(
        sets_of(&s, &mine)
            .into_iter()
            .skip(mine_before)
            .all(|set| set.patches.is_empty()),
        "the author receives no attempted write"
    );
    assert!(
        sets_of(&s, &theirs)
            .into_iter()
            .skip(theirs_before)
            .all(|set| set.patches.is_empty()),
        "other readers receive no attempted write"
    );
    assert_eq!(
        s.db.count("SELECT count(*) FROM interactions"),
        0,
        "no receipt survived either refused transaction"
    );
    // The control: the next post commits, and is told.
    let answer = posted(
        &s,
        "/command/feed.app.post",
        "a",
        "i-2",
        "[\"After the refusal\"]",
    );
    assert!(answer.contains("\"committed\":true"), "{answer}");
    assert_eq!(s.db.count("SELECT count(*) FROM posts"), 4);
    let set = format!("{:?}", sets_of(&s, &theirs).pop().expect("a patch set"));
    assert!(set.contains("After the refusal"), "{set}");
}

/// **The feed on PostgreSQL serves what it serves in memory**: the same
/// commands, then the same home page and the same thread, to the byte.
#[test]
fn on_postgres_the_feed_serves_what_it_serves_in_memory() {
    let Some(url) = database() else { return };
    let pg = served(&url, "parity");
    let memory = served_feed();
    let mut pages = Vec::new();
    for s in [&*pg, &*memory] {
        assert!(post(s, "a", "One post", "i-1").committed);
        let reply = s
            .command_answered(
                "feed.app.reply",
                "b",
                &[Val::String("p4".into()), Val::String("Its reply".into())],
                Some("i-2"),
            )
            .expect("runs");
        assert!(reply.committed, "{:?}", reply.result);
        let like = s
            .command_answered(
                "feed.app.like",
                "b",
                &[Val::String("p4".into())],
                Some("i-3"),
            )
            .expect("runs");
        assert!(like.committed, "{:?}", like.result);
        let (home, _, _, _) = s
            .serve_document_settled("c", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
        let (thread, _, _, _) = s
            .serve_document_settled("c", "feed.app.PostPage~ui", &thread_of("p4"), &[])
            .expect("served");
        pages.push((home, thread));
    }
    assert!(pages[1].0.contains("One post"), "{}", pages[1].0);
    assert!(pages[1].1.contains("Its reply"), "{}", pages[1].1);
    assert_eq!(pages[0], pages[1]);
}

/// **A source stating a feed of its changes is refused** (ADR-0246's
/// negative control): the layer delivers none, so the host does not serve,
/// and says which clause.
#[test]
fn on_postgres_a_stated_change_feed_the_layer_cannot_deliver_is_refused() {
    let Some(url) = database() else { return };
    let changed = |app: &str| {
        let out = app.replacen("    changes      none\n", "    changes      feed\n", 1);
        assert_ne!(out, app, "the feed states its changes");
        out
    };
    let Err(why) = served_with(&url, "feed_refused", changed, |schema| {
        FeedPg::open(&url, Some(schema))
    }) else {
        panic!("a change feed the layer cannot deliver was served");
    };
    assert!(why.contains("`FeedData` states `changes feed`"), "{why}");
}

/// **Serializable, stated against a connection that opens read committed,
/// is refused**, unless the layer sets it on each transaction, or the
/// connection's default gives it (ADR-0246).
#[test]
fn on_postgres_serializable_against_a_read_committed_default_is_refused() {
    let Some(url) = database() else { return };
    let as_is = |app: &str| app.to_string();
    let Err(why) = served_with(&url, "default_refused", as_is, |schema| {
        FeedPg::open_with(
            &url,
            Some(schema),
            Isolation::ConnectionDefault,
            crate::feed_pg::RenderSeats::default(),
        )
    }) else {
        panic!("serializable was stated, and read committed served");
    };
    assert!(
        why.contains(
            "`FeedData` states `transactions serializable`, and the database's are read_committed"
        ),
        "{why}"
    );
    // The controls: the same connection's default, made serializable, gives
    // it; and the layer setting it on each transaction gives it.
    let separator = if url.contains('?') { '&' } else { '?' };
    let serializable =
        format!("{url}{separator}options=-c%20default_transaction_isolation%3Dserializable");
    served_with(&url, "default_given", as_is, |schema| {
        FeedPg::open_with(
            &serializable,
            Some(schema),
            Isolation::ConnectionDefault,
            crate::feed_pg::RenderSeats::default(),
        )
    })
    .expect("a serializable default gives serializable");
    served_with(&url, "per_transaction", as_is, |schema| {
        FeedPg::open(&url, Some(schema))
    })
    .expect("serializable on each transaction");
}

/// **An entry written `_` goes through the outbox as `null`** (ADR-0256):
/// committed with the command's writes, read back as every value at its
/// position, and dropped as in memory: each session's timeline at 20.
#[test]
fn on_postgres_an_entry_written_with_a_wildcard_is_every_entry() {
    use super::every_entry::{forgetting, keep, kept, timeline_at};
    let Some(url) = database() else { return };
    let s = served_with(&url, "every", forgetting, |schema| {
        FeedPg::open(&url, Some(schema))
    })
    .expect("served on PostgreSQL");
    let keys = [
        timeline_at("a", 20),
        timeline_at("a", 40),
        timeline_at("b", 20),
    ];
    keep(&s, &keys);
    let answered = s
        .command_answered(
            "feed.app.forget_twenty",
            "a",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(
        kept(&s, &keys),
        [timeline_at("a", 40)],
        "each session's timeline at 20 was dropped"
    );
}

// --- the scheduled-commands track ---------------------------------------

/// **Two commands a test adds to the feed**: `post_in_five`, which
/// schedules a post of `text` five minutes on, and `post_later`, which the
/// clock runs then, on behalf of the reader who scheduled it.
const SCHEDULING: &str = r#"
// A test's: a post of `text`, by the clock.
command post_later(text: PostText) -> Result<Post, FeedError>
    invoked_by    Clock
    idempotent_by InteractionId
    emits         Posted(author(current_session()))
{
    publish(current_session(), text)
}

// A test's: `post_later(text)` scheduled five minutes on.
command post_in_five(text: PostText) -> Result<Int, FeedError>
    requires      SignedIn
    idempotent_by InteractionId
{
    schedule post_later(text) after minutes(5)
    Ok(1)
}
"#;

/// The feed's `app.pw`, with [`SCHEDULING`]'s commands and the clock's
/// `minutes` they read.
pub(super) fn with_scheduling(app: &str) -> String {
    let imported = app.replacen(
        "\nimport List\n",
        "\nimport List\nimport clock.{ minutes }\n",
        1,
    );
    format!("{imported}{SCHEDULING}")
}

/// **On the feed too, a run is kept by its database** (the
/// scheduled-commands track): written with the scheduling command's writes
/// in the feed's schema, run by a host started after it once its time has
/// passed, as the reader who scheduled it, and removed. The control: before
/// its time, nothing is posted.
#[test]
fn on_postgres_a_feeds_run_is_kept_by_its_database() {
    let Some(url) = database() else { return };
    let db = Schema::new(&url, "scheduled");
    let (_dir, out) = built_feed_with(with_scheduling);
    let host = || {
        let layer = FeedPg::open(&url, Some(&db.name)).expect("the layer");
        Server::from_build_with(out.clone(), out.clone(), Some(Arc::new(layer))).expect("served")
    };
    let posts = {
        let s = host();
        let scheduled = posted(
            &s,
            "/command/feed.app.post_in_five",
            "a",
            "i-1",
            "[\"Later\"]",
        );
        assert!(scheduled.contains("\"committed\":true"), "{scheduled}");
        db.count("SELECT count(*) FROM posts")
    };
    assert_eq!(db.count("SELECT count(*) FROM schedules"), 1);
    let due = db.count("SELECT due_ms FROM schedules");
    let s = host();
    let ahead = due - s.now();
    posted(
        &s,
        &format!("/bench/clock?advance={}", ahead - 1000),
        "clock",
        "c-1",
        "",
    );
    assert_eq!(s.run_due().0, 0, "the control: a second before its time");
    assert_eq!(db.count("SELECT count(*) FROM posts"), posts);
    posted(&s, "/bench/clock?advance=1000", "clock", "c-2", "");
    assert_eq!(s.run_due().0, 1);
    assert_eq!(db.count("SELECT count(*) FROM posts"), posts + 1);
    assert_eq!(db.count("SELECT count(*) FROM schedules"), 0, "removed");
}
