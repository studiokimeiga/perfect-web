//! **A program's declared tables, served durably from PostgreSQL** (track
//! `durable-tables`): the guide's guestbook and the lists app, built as
//! `pw build` builds them and served through the same gate as in memory, by
//! the generic PostgreSQL adapter (`tables_pg.rs`, ADR-0393's adapter):
//!
//! - the guestbook's behaviours on PostgreSQL: its page, signing and
//!   removing, an entry its author's alone to remove, its numbers, a retry
//!   answered once (on another host too), every other page told, one state
//!   per render;
//! - a command's rows, its interaction's record, its outbox and its runs in
//!   ONE PostgreSQL transaction (one `xmin` for all of them), refused
//!   together by the database at its `COMMIT`;
//! - the authority unchanged: a guarded command calls no table function
//!   until its predicate answers; a session signed out before a command
//!   commits keeps nothing of it, and a sign-out waits for a command that
//!   was admitted first; a session past its lifetime at the commit, nothing;
//! - the conformance suite (`conformance.rs`), as written, over this layer,
//!   for both programs it covers, with the same oracle.
//!
//! Each test makes a schema of its own and drops it. Without a database
//! (`PW_TABLES_DATABASE_URL`, else `PW_STORE_DATABASE_URL` or
//! `PW_FEED_DATABASE_URL`) each passes doing nothing, as every PostgreSQL
//! test of the repository does (ADR-0246); `just e14-durable-tables` and its
//! mutation script refuse to run without one.

use super::tables::Asserted;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SIGN: &str = "guestbook.book.sign";
const REMOVE: &str = "guestbook.book.remove";
const LATEST: &str = "guestbook.book.Latest";

/// The throwaway database the tests may use, if one is named.
pub(super) fn database() -> Option<String> {
    [
        "PW_TABLES_DATABASE_URL",
        "PW_STORE_DATABASE_URL",
        "PW_FEED_DATABASE_URL",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

static SCHEMAS: AtomicUsize = AtomicUsize::new(0);

/// **A schema of one test's own**, its hosts' connections named for it (so
/// that a test finds them waiting in `pg_stat_activity`), and dropped with
/// everything in it when the test is done, whatever happened.
pub(super) struct Schema {
    url: String,
    name: String,
    /// The `application_name` its hosts' connections carry.
    application: String,
}

impl Schema {
    pub(super) fn new(url: &str, what: &str) -> Schema {
        let n = SCHEMAS.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let name = format!("pw_dt_{what}_{}_{n}_{nanos}", std::process::id());
        let application = format!("pw_dt_{}_{n}", std::process::id());
        Schema {
            url: url.to_string(),
            name,
            application,
        }
    }

    /// Where its hosts keep the program's tables.
    pub(super) fn database(&self) -> crate::tables_pg::Database {
        let joiner = if self.url.contains('?') { '&' } else { '?' };
        crate::tables_pg::Database {
            url: format!("{}{joiner}application_name={}", self.url, self.application),
            schema: Some(self.name.clone()),
            seats: crate::feed_pg::RenderSeats::default(),
        }
    }

    fn client(&self) -> Result<postgres::Client, String> {
        let mut c =
            postgres::Client::connect(&self.url, postgres::NoTls).map_err(crate::feed_pg::pg)?;
        c.batch_execute(&format!(
            "SET search_path TO {}",
            crate::feed_pg::ident(&self.name)
        ))
        .map_err(crate::feed_pg::pg)?;
        Ok(c)
    }

    /// `statements`, run as another writer of the database runs them.
    pub(super) fn sql(&self, statements: &str) -> Result<(), String> {
        self.client()?
            .batch_execute(statements)
            .map_err(crate::feed_pg::pg)
    }

    /// The first column of each row `query` answers, as text; a query that
    /// fails answers its error as its one line, for the asserting test.
    pub(super) fn texts(&self, query: &str) -> Vec<String> {
        match self
            .client()
            .and_then(|mut c| c.query(query, &[]).map_err(crate::feed_pg::pg))
        {
            Ok(rows) => rows
                .iter()
                .map(|r| {
                    r.try_get::<_, String>(0)
                        .unwrap_or_else(|e| format!("error: {e}"))
                })
                .collect(),
            Err(e) => vec![format!("error: {e}")],
        }
    }

    /// How many rows `table` holds (its name as SQL names it).
    pub(super) fn count(&self, table: &str) -> String {
        self.texts(&format!("SELECT count(*)::text FROM {table}"))
            .join(",")
    }

    /// How many of its hosts' connections wait for an advisory lock now.
    fn waiting(&self) -> String {
        self.texts(&format!(
            "SELECT count(*)::text FROM pg_stat_activity WHERE application_name = '{}' \
             AND wait_event_type = 'Lock' AND wait_event = 'advisory'",
            self.application
        ))
        .join(",")
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                crate::feed_pg::ident(&self.name)
            ));
        }
    }
}

/// **The tables' PostgreSQL layer for a build**, on `database`, as the host
/// makes it where a deployment names one (`TablesPg::for_build`).
pub(super) fn pg_layer(
    build: &std::path::Path,
    database: &crate::tables_pg::Database,
) -> Arc<dyn data::DataLayer> {
    let layer = crate::tables_pg::TablesPg::for_build(build, database)
        .asserted("the tables' PostgreSQL layer")
        .asserted_some("a program that declares tables");
    Arc::new(layer)
}

/// **A build served on PostgreSQL**, its tables in `schema`: one host;
/// another on the same schema is a second host of one deployment.
pub(super) fn serve(out: &std::path::Path, schema: &Schema) -> Server {
    served_on(out, &schema.database())
}

/// A build served by the tables' PostgreSQL layer on `database`.
fn served_on(out: &std::path::Path, database: &crate::tables_pg::Database) -> Server {
    Server::from_build_tables(
        out.to_path_buf(),
        out.to_path_buf(),
        pg_layer(out, database),
    )
    .asserted("served on PostgreSQL")
}

/// The guestbook, built as `guestbook.sh` builds it.
fn guestbook() -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(&["examples/guestbook"], &[])
}

/// The page `session` is served, its text as a reader sees it.
fn shown(s: &Server, session: &str) -> String {
    let answer: String = fetched_as(s, "/", Some(session))
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    visible(&answer)
}

fn sign(s: &Server, session: &str, text: &str, interaction: &str) -> Answered {
    s.command_answer(SIGN, session, &[serde_json::json!(text)], Some(interaction))
        .asserted("signing is answered")
}

fn remove(s: &Server, session: &str, number: i64, interaction: &str) -> Answered {
    s.command_answer(
        REMOVE,
        session,
        &[serde_json::json!(number)],
        Some(interaction),
    )
    .asserted("removing is answered")
}

/// The entries `Latest` answers `session`: each one's number, name, text,
/// and whether it is the reader's.
fn latest(s: &Server, session: &str) -> Vec<(i64, String, String, bool)> {
    let reader = Val::String(s.identity.principals().user_of(session));
    let lines = s
        .answer(LATEST, session, &[reader, Val::S64(20)])
        .asserted("read");
    assert!(matches!(lines, Val::List(_)), "a list: {lines:?}");
    let Val::List(lines) = lines else {
        unreachable!("asserted above")
    };
    lines
        .iter()
        .map(|line| {
            let field = |name: &str| val_at(line, &[name]).cloned();
            let fields = (field("number"), field("name"), field("text"), field("mine"));
            assert!(
                matches!(
                    fields,
                    (
                        Some(Val::S64(_)),
                        Some(Val::String(_)),
                        Some(Val::String(_)),
                        Some(Val::Bool(_))
                    )
                ),
                "a line: {fields:?}"
            );
            match fields {
                (
                    Some(Val::S64(number)),
                    Some(Val::String(name)),
                    Some(Val::String(text)),
                    Some(Val::Bool(mine)),
                ) => (number, name, text, mine),
                _ => unreachable!("asserted above"),
            }
        })
        .collect()
}

fn line(number: i64, name: &str, text: &str, mine: bool) -> (i64, String, String, bool) {
    (number, name.into(), text.into(), mine)
}

const ENTRIES: &str = "\"Entries\"";

/// **The guestbook is served from PostgreSQL**, by the generic adapter,
/// through the gate: its page reads the tables, an entry signed is the
/// database's row, each reader told which are theirs, and a removed one
/// goes, from the database too.
#[test]
fn the_guestbook_is_served_from_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "book");
    let s = serve(&out, &schema);
    assert!(
        s.data.tables().is_some() && s.data.keeps_interactions() && s.data.runs().is_some(),
        "served by the tables' PostgreSQL layer, which keeps the platform's records"
    );
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    let empty = shown(&s, "c");
    assert!(empty.contains("No one has signed yet."), "{empty}");
    assert!(sign(&s, "a", "Hello from Ada", "i-1").committed);
    assert!(sign(&s, "b", "Hello from Bob", "i-2").committed);
    assert_eq!(
        latest(&s, "a"),
        [
            line(2, "Bob", "Hello from Bob", false),
            line(1, "Ada", "Hello from Ada", true),
        ]
    );
    assert_eq!(
        latest(&s, "b"),
        [
            line(2, "Bob", "Hello from Bob", true),
            line(1, "Ada", "Hello from Ada", false),
        ]
    );
    // The database's rows, the author's id the platform's.
    assert_eq!(
        schema.texts(
            "SELECT convert_from(name, 'UTF8') || ':' || number || ':' || convert_from(author, 'UTF8') \
             FROM \"Entries\" ORDER BY number"
        ),
        ["Ada:1:u-ada", "Bob:2:u-bob"]
    );
    let page = shown(&s, "a");
    assert!(page.contains("Signed in as Ada"), "{page}");
    assert!(page.contains("Hello from Bob"), "{page}");
    assert_eq!(page.matches("Remove").count(), 1, "{page}");
    assert!(remove(&s, "a", 1, "i-3").committed);
    assert_eq!(latest(&s, "b"), [line(2, "Bob", "Hello from Bob", true)]);
    assert_eq!(schema.count(ENTRIES), "1");
}

/// **An entry is its author's alone to remove, on PostgreSQL** (the
/// negative control of the rule the program holds): another reader's
/// removal answers `NotYours` and keeps the entry; a missing one,
/// `NotFound`; signed out, `requires SignedIn` refuses it before it calls a
/// single table function.
#[test]
fn an_entry_is_its_authors_alone_to_remove_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "mine");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    assert!(sign(&s, "a", "Mine", "i-1").committed);
    let theirs = remove(&s, "b", 1, "i-2");
    assert!(!theirs.committed);
    assert_eq!(
        theirs.result,
        Some(serde_json::json!({"$case": "err", "value": {"$case": "not-yours"}})),
        "{theirs:?}"
    );
    let missing = remove(&s, "b", 7, "i-3");
    assert!(!missing.committed);
    assert_eq!(
        missing.result,
        Some(serde_json::json!({"$case": "err", "value": {"$case": "not-found"}}))
    );
    assert_eq!(latest(&s, "a"), [line(1, "Ada", "Mine", true)]);
    assert_eq!(schema.count(ENTRIES), "1");
    // Signed out: refused by its predicate, no table function called.
    let calls = || {
        s.data
            .table_calls_for_test()
            .asserted_some("a layer that counts its commands table calls")
    };
    let before = calls();
    let refused = s
        .command_answer(REMOVE, "c", &[serde_json::json!(1)], Some("i-4"))
        .asserted("answered");
    assert_eq!(refused.refused.as_deref(), Some("SignedIn"));
    assert_eq!(calls(), before, "a guarded command called a table function");
    // Signed in, its body runs: the entry read, then refused as not hers.
    assert!(!remove(&s, "b", 1, "i-5").committed);
    assert!(calls() > before, "a signed-in removal reads the entry");
    assert_eq!(latest(&s, "a"), [line(1, "Ada", "Mine", true)]);
    assert_eq!(schema.count(ENTRIES), "1");
}

/// **A removed entry's number is never given again, on PostgreSQL.**
#[test]
fn a_removed_entrys_number_is_never_given_again_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "numbers");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "First", "i-1").committed);
    assert!(remove(&s, "a", 1, "i-2").committed);
    assert!(sign(&s, "a", "Second", "i-3").committed);
    assert_eq!(latest(&s, "a"), [line(2, "Ada", "Second", true)]);
    assert_eq!(
        schema.texts("SELECT signed::text FROM \"Books\""),
        ["2"],
        "the book counts every entry ever signed"
    );
}

/// **A signing sent twice is one entry, on this host and on another**
/// (ADR-0325): the retry is answered from the record its commit kept in
/// the database, by a second host of the deployment as by the first, and
/// nothing runs again; a new generation of the session cannot take it.
#[test]
fn a_signing_sent_twice_is_one_entry_on_any_host() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "twice");
    let (a, b) = pair(&out, &schema);
    let first = sign(&a, "owner", "Once", "i-1");
    let again = sign(&a, "owner", "Once", "i-1");
    let elsewhere = sign(&b, "owner", "Once", "i-1");
    assert!(first.committed && again.committed && elsewhere.committed);
    assert_eq!(first.result, again.result);
    assert_eq!(first.result, elsewhere.result);
    assert_eq!(schema.count(ENTRIES), "1");
    assert_eq!(schema.count("pw_interactions"), "1");
    // The second host reads what the first committed.
    assert_eq!(latest(&b, "owner"), [line(1, "Ada", "Once", true)]);
    // A session's change waits for no command that has ended: each gave
    // its session's lock back.
    assert!(
        b.identity.principals().admit("owner").is_ok(),
        "the session's lock is held by no ended command"
    );
    // Signed in anew, as another: the old receipt is not theirs.
    a.identity.signed_in_for_test("owner", "u-cy", "Cy");
    b.identity
        .principals()
        .admit("owner")
        .asserted("the new generation");
    let taken = b.command_answer(SIGN, "owner", &[serde_json::json!("Once")], Some("i-1"));
    assert!(
        taken.is_err() || taken.as_ref().is_ok_and(|a| !a.committed),
        "{taken:?}"
    );
    assert_eq!(schema.count(ENTRIES), "1");
    // The neighbour: another interaction is another entry.
    assert!(sign(&a, "owner", "Twice", "i-2").committed);
    assert_eq!(schema.count(ENTRIES), "2");
}

/// **A commit reaches every other open page of the entries**: the sign's
/// event, read back from the database's outbox, drops `Latest`, and the
/// other session reading it is told.
#[test]
fn a_signing_reaches_every_other_open_page_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "told");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    shown(&s, "b");
    s.telling.lock().expect("telling").clear();
    assert!(sign(&s, "a", "Live", "i-1").committed);
    let telling = s.telling.lock().expect("telling").clone();
    assert!(!telling.is_empty(), "a commit to tell of");
    let Some((committer, reached)) = telling.last() else {
        unreachable!("asserted above")
    };
    assert_eq!(committer, "a");
    assert!(reached.iter().any(|r| r.resource == LATEST), "{reached:?}");
    assert_eq!(s.others_reading("a", reached), ["b"]);
}

/// **What the outbox was handed, as each commit wrote it**: another hand's
/// trigger copies each row the outbox takes into `outbox_seen`, with the
/// transaction that wrote it (the host forgets its own rows once it has
/// delivered them, so the outbox itself keeps none for a test to read).
fn audit_outbox(schema: &Schema) {
    let s = crate::feed_pg::ident(&schema.name);
    schema
        .sql(&format!(
            "CREATE TABLE outbox_seen (xid text, row text); \
             CREATE FUNCTION seen() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN INSERT INTO {s}.outbox_seen VALUES (pg_current_xact_id()::xid::text, \
                 NEW.kind || ':' || NEW.name || ':' || NEW.args::text); RETURN NEW; END $$; \
             CREATE TRIGGER seen AFTER INSERT ON pw_outbox FOR EACH ROW EXECUTE FUNCTION seen();"
        ))
        .asserted("an audit of the outbox");
}

/// **The outbox keeps no row its host delivered** (the review of 3ccd083a,
/// finding 1): each commit's events are written with it, read back and
/// delivered, then forgotten, so the outbox holds no more than what is not
/// delivered yet, however many commits there were; a row a host that ended
/// left behind is forgotten by the layer's housekeeping once it is a minute
/// old, and one younger is kept until then.
#[test]
fn the_outbox_keeps_no_row_its_host_delivered() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "outbox");
    let s = serve(&out, &schema);
    audit_outbox(&schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    for n in 1..=3 {
        shown(&s, "b");
        s.telling.lock().expect("telling").clear();
        assert!(sign(&s, "a", &format!("Entry {n}"), &format!("i-{n}")).committed);
        let telling = s.telling.lock().expect("telling").clone();
        assert!(
            telling
                .iter()
                .any(|(_, reached)| reached.iter().any(|r| r.resource == LATEST)),
            "commit {n} delivered: {telling:?}"
        );
        assert_eq!(schema.count("pw_outbox"), "0", "after commit {n}");
    }
    assert_eq!(
        schema.texts("SELECT row FROM outbox_seen"),
        ["event:guestbook.book.Signed:[\"guestbook\"]"; 3],
        "each commit wrote its event"
    );
    // Left behind by a host that ended between a commit and its forgetting:
    // one two minutes old, one written now.
    schema
        .sql(
            "INSERT INTO pw_outbox (kind, name, args, host, created) VALUES \
             ('event', 'left.Old', '[]', 'ended', clock_timestamp() - interval '2 minutes'), \
             ('event', 'left.Young', '[]', 'ended', clock_timestamp())",
        )
        .asserted("rows a host left behind");
    s.run_due();
    assert_eq!(schema.texts("SELECT name FROM pw_outbox"), ["left.Young"]);
    // A minute on, the young one is old.
    schema
        .sql("UPDATE pw_outbox SET created = created - interval '2 minutes'")
        .asserted("aged");
    s.query_clock.0.advance(61 * 1000);
    s.run_due();
    assert_eq!(schema.count("pw_outbox"), "0");
}

/// A page of two bindings, each reading the table: what one render of it
/// reads is one state of the table, or two.
const BOTH: &str = "module durable.both

import Notes

query First() -> String
    consistency strong
{
    match Notes.get(\"a\") {
        Some(note) => note.text,
        None => \"none\",
    }
}

query Second() -> String
    consistency strong
{
    match Notes.get(\"b\") {
        Some(note) => note.text,
        None => \"none\",
    }
}

page Both() {
    route \"/\"

    let first = query First()
    let second = query Second()

    view {
        <title>Both</title>
        <main>
            <p id=\"first\">first {first}</p>
            <p id=\"second\">second {second}</p>
        </main>
    }
}
";

/// **One render reads one state of the tables, on PostgreSQL** (NEXT 4c,
/// ADR-0354): the render's seat is shared by the read functions of each of
/// its queries, so a commit landing between two bindings, after the first
/// read the table, is not read by the second; the page read after it shows
/// it. (A render's state on PostgreSQL is its first read's, ADR-0393's
/// decision 8: a commit before any table read is the render's.)
#[test]
fn a_render_reads_one_state_of_the_tables_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("both.pw", BOTH)]);
    let schema = Schema::new(&url, "render");
    let s = serve(&out, &schema);
    let tables = super::conformance::tables_of(&out);
    let note = |id: &str, text: &str| {
        let row: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({"id": id, "text": text})).asserted("a note");
        BTreeMap::from([("Notes".to_string(), vec![row])])
    };
    super::conformance::written(&s, &tables, &note("a", "before"));
    let between = note("b", "between");
    let wrote: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let into = wrote.clone();
    *s.between_bindings.lock().expect("between") = Some(Box::new(move |server: &Server| {
        *into.lock().expect("wrote") =
            Some(super::conformance::write_rows(server, &tables, &between));
    }));
    let page = shown(&s, "c");
    assert_eq!(
        wrote.lock().expect("wrote").clone(),
        Some(Ok(())),
        "a commit landed between the page's bindings"
    );
    assert!(page.contains("first before"), "{page}");
    assert!(
        page.contains("second none"),
        "a commit during the render is not the render's: {page}"
    );
    let again = shown(&s, "c");
    assert!(again.contains("second between"), "{again}");
}

/// **A command's rows, its interaction's record and its outbox are one
/// PostgreSQL transaction**: each committed row carries one `xmin`, the
/// transaction that wrote it; and a commit the database refuses at its
/// `COMMIT` (a deferred trigger of another hand's) keeps none of them.
#[test]
fn a_commands_rows_record_and_outbox_commit_as_one_transaction() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "one");
    let s = serve(&out, &schema);
    audit_outbox(&schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "Together", "i-1").committed);
    // The event's row is forgotten once delivered: the audit kept the
    // transaction that wrote it.
    let xmins = schema.texts(
        "SELECT xmin::text FROM \"Entries\" UNION ALL SELECT xmin::text FROM \"Books\" \
         UNION ALL SELECT xmin::text FROM pw_interactions UNION ALL SELECT xid FROM outbox_seen",
    );
    assert_eq!(xmins.len(), 4, "{xmins:?}");
    assert!(
        xmins.iter().all(|x| *x == xmins[0]),
        "one transaction wrote the entry, the book, the record and the event: {xmins:?}"
    );
    // Refused by the database at its COMMIT: nothing of it.
    schema
        .sql(
            "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'refused at the commit'; END $$; \
             CREATE CONSTRAINT TRIGGER refuse AFTER INSERT ON \"Entries\" \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse();",
        )
        .asserted("a trigger refusing at the commit");
    // The database answered its rollback: refused, not an outcome unknown.
    let refused = s.command_answer(SIGN, "a", &[serde_json::json!("Refused")], Some("i-2"));
    assert!(refused.as_ref().is_ok_and(|a| !a.committed), "{refused:?}");
    assert_eq!(schema.count(ENTRIES), "1");
    assert_eq!(schema.texts("SELECT signed::text FROM \"Books\""), ["1"]);
    assert_eq!(schema.count("pw_interactions"), "1");
    assert_eq!(schema.count("outbox_seen"), "1", "its event rolled back");
    // And once the database takes it, a signing commits whole.
    schema
        .sql("DROP TRIGGER refuse ON \"Entries\"")
        .asserted("the trigger dropped");
    assert!(sign(&s, "a", "Accepted", "i-3").committed);
    assert_eq!(schema.count(ENTRIES), "2");
    assert_eq!(schema.count("pw_interactions"), "2");
}

/// A table and a command that schedules another the clock invokes, which
/// writes it: the platform's runs in the command's transaction.
const NOTES: &str = "module Notes

type Note = Note { id: String, text: String }

table Notes
    row Note
    key id
";

const LATER: &str = "module durable.later

import clock.{ minutes }
import Notes.{ Note }
import Notes

opaque type InteractionId = String

command remind(id: String) -> Result<Int, String>
    invoked_by    Clock
    idempotent_by InteractionId
{
    Notes.put(Note { id: id, text: \"reminded\" })
    Ok(1)
}

command note_later(id: String) -> Result<Int, String> !{ clock.schedule, database.write<Notes> }
{
    Notes.put(Note { id: id, text: \"noted\" })
    schedule remind(id) after minutes(30)
    Ok(1)
}
";

const NOTE_LATER: &str = "durable.later.note_later";

/// **A command's scheduled run commits with its rows, in one PostgreSQL
/// transaction, or not at all** (ADR-0334 in the adapter's transaction): the
/// note and its run share one `xmin`; a commit the database refuses keeps
/// neither; once due, the run runs once, as the user who scheduled it, its
/// write, its taking and its record committed together; and it is not run
/// again.
#[test]
fn a_commands_scheduled_run_commits_with_its_rows() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("later.pw", LATER)]);
    let schema = Schema::new(&url, "runs");
    let s = serve(&out, &schema);
    let noted = s
        .command_answered(NOTE_LATER, "a", &[Val::String("x".into())], None)
        .asserted("answered");
    assert!(noted.committed, "{noted:?}");
    let xmins =
        schema.texts("SELECT xmin::text FROM \"Notes\" UNION ALL SELECT xmin::text FROM pw_runs");
    assert_eq!(xmins.len(), 2, "{xmins:?}");
    assert_eq!(xmins[0], xmins[1], "the note and its run, one transaction");
    let runs = s.runs().all_runs().asserted("the runs");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(
        (runs[0].command.as_str(), runs[0].args.as_str()),
        ("durable.later.remind", "[\"x\"]")
    );
    // Refused at its COMMIT by the database: neither the note nor its run.
    schema
        .sql(
            "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'refused at the commit'; END $$; \
             CREATE CONSTRAINT TRIGGER refuse AFTER INSERT ON \"Notes\" \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse();",
        )
        .asserted("a trigger refusing at the commit");
    let refused = s.command_answered(NOTE_LATER, "a", &[Val::String("y".into())], None);
    assert!(
        refused.is_err() || refused.as_ref().is_ok_and(|a| !a.committed),
        "{refused:?}"
    );
    schema
        .sql("DROP TRIGGER refuse ON \"Notes\"")
        .asserted("the trigger dropped");
    assert_eq!(schema.count("\"Notes\""), "1");
    assert_eq!(s.runs().all_runs().asserted("the runs").len(), 1);
    // Not due yet; then due, and run once.
    assert_eq!(s.run_due().0, 0);
    s.query_clock.0.advance(31 * 60 * 1000);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(
        schema.texts("SELECT convert_from(text, 'UTF8') FROM \"Notes\""),
        ["reminded"]
    );
    assert_eq!(s.runs().all_runs().asserted("the runs"), vec![]);
    assert_eq!(schema.count("pw_interactions"), "1", "the run's record");
    let run_xmins = schema
        .texts("SELECT xmin::text FROM \"Notes\" UNION ALL SELECT xmin::text FROM pw_interactions");
    assert_eq!(run_xmins.len(), 2, "{run_xmins:?}");
    assert_eq!(
        run_xmins[0], run_xmins[1],
        "the run's write and its record, one transaction"
    );
    assert_eq!(s.run_due().0, 0, "a run runs once");
}

/// **A run's old generation does not run**: one replaced as its command
/// begins (another host's retry, after the scheduler found it current) is
/// taken by no transaction, so nothing of its command commits; one
/// scheduled again before it runs is not run at all; the generation kept
/// runs once it is due.
#[test]
fn an_old_generation_of_a_run_does_not_run() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("later.pw", LATER)]);
    let schema = Schema::new(&url, "generation");
    let s = serve(&out, &schema);
    let note_later = |s: &Server| {
        s.command_answered(NOTE_LATER, "a", &[Val::String("x".into())], None)
            .asserted("answered")
    };
    let notes = || schema.texts("SELECT convert_from(text, 'UTF8') FROM \"Notes\"");
    assert!(note_later(&s).committed);
    s.query_clock.0.advance(31 * 60 * 1000);
    // Found current by the scheduler, then replaced by another hand before
    // its command's transaction begins: its taking finds another generation.
    let watch = schema.database().url;
    let name = schema.name.clone();
    *s.before_command_authority.lock().expect("hook") = Some(Box::new(move |_| {
        if let Ok(mut c) = postgres::Client::connect(&watch, postgres::NoTls) {
            let _ = c.batch_execute(&format!(
                "UPDATE {}.pw_runs SET attempts = attempts + 1",
                crate::feed_pg::ident(&name)
            ));
        }
    }));
    assert_eq!(s.run_due().0, 1, "selected and run");
    assert_eq!(
        notes(),
        ["noted"],
        "the replaced generation's command committed nothing"
    );
    assert_eq!(schema.count("pw_interactions"), "0");
    let kept = s.runs().all_runs().asserted("the runs");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].attempts, 1, "the generation the other hand made");
    // Selected, then scheduled again by its command: not run at all.
    let old = schedules::select(s.runs(), s.now(), 16).asserted("selected");
    assert_eq!(old.len(), 1);
    assert!(note_later(&s).committed, "scheduled again");
    let again = s.runs().all_runs().asserted("the runs");
    assert_eq!(again.len(), 1, "{again:?}");
    assert_ne!(again[0].id, old[0].id, "a new generation");
    s.run_scheduled(&old[0]);
    assert_eq!(notes(), ["noted"]);
    assert_eq!(s.runs().all_runs().asserted("the runs"), again);
    // The generation kept runs once it is due.
    s.query_clock.0.advance(31 * 60 * 1000);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(notes(), ["reminded"]);
}

/// **A render without a seat reads no state at all**: every seat taken for
/// longer than a render waits, its reads fail, and the page is not served
/// from the latest commit instead; a seat given back, it is.
#[test]
fn a_render_without_a_seat_reads_no_other_state() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("both.pw", BOTH)]);
    let schema = Schema::new(&url, "seatless");
    let mut database = schema.database();
    database.seats = crate::feed_pg::RenderSeats {
        seats: 1,
        wait: Duration::from_millis(200),
    };
    let s = served_on(&out, &database);
    let tables = super::conformance::tables_of(&out);
    let row: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(serde_json::json!({"id": "a", "text": "kept"})).asserted("a note");
    super::conformance::written(
        &s,
        &tables,
        &BTreeMap::from([("Notes".to_string(), vec![row])]),
    );
    let held = s.data.snapshot();
    let answer: String = fetched_as(&s, "/", Some("c"))
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    // As a reader reads it: the renderer's part markers taken out.
    let read = visible(&answer);
    assert!(
        !read.contains("first kept"),
        "a render with no seat read the latest commit: {read}"
    );
    drop(held);
    let page = shown(&s, "c");
    assert!(page.contains("first kept"), "{page}");
}

/// **`PW_TABLES_SCHEMA` is a test's setting**: honoured for a development
/// deployment on this machine, refused for any other, and none where it is
/// not set.
#[test]
fn a_tables_schema_is_a_development_deployments_alone() {
    let at = |development: bool, origin: &str| identity::Deployment {
        development,
        origin: origin.to_string(),
    };
    let schema = crate::tables_pg::test_schema;
    assert_eq!(
        schema(Some("t"), &at(true, "http://127.0.0.1:3143")),
        Ok(Some("t".to_string()))
    );
    assert_eq!(schema(None, &at(false, "https://example.test")), Ok(None));
    assert_eq!(
        schema(Some(""), &at(false, "https://example.test")),
        Ok(None)
    );
    assert!(schema(Some("t"), &at(false, "http://127.0.0.1:3143")).is_err());
    assert!(schema(Some("t"), &at(true, "https://dev.example.test")).is_err());
}

/// Two hosts of the guestbook on one schema, a session `owner` signed in
/// on the first and admitted by the second, as a deployment's are.
fn pair(out: &std::path::Path, schema: &Schema) -> (Server, Server) {
    let a = serve(out, schema);
    let b = serve(out, schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    a.identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    b.identity.use_provider(provider, super::sign_in::REDIRECT);
    a.identity.signed_in_for_test("owner", "u-ada", "Ada");
    b.identity
        .principals()
        .admit("owner")
        .asserted("the other host admits the session");
    (a, b)
}

fn sign_out(s: &Server) -> String {
    super::sign_in::exchanged(
        s,
        "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\n\
         Sec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n",
    )
}

/// **A session signed out before a command takes its authority keeps
/// nothing of the command**: the actor captured, the reader signed out on
/// another host, and the command then fenced on its session finds it gone;
/// no entry, record or event is kept.
#[test]
fn a_session_signed_out_before_its_command_commits_keeps_nothing() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "revoked");
    let (a, b) = pair(&out, &schema);
    let (entered, reached) = std::sync::mpsc::channel();
    let (release, go) = std::sync::mpsc::channel::<()>();
    *a.before_command_authority.lock().expect("hook") = Some(Box::new(move |_| {
        let _ = entered.send(());
        let _ = go.recv_timeout(Duration::from_secs(20));
    }));
    let (result, out) = std::thread::scope(|scope| {
        let command = scope.spawn(|| {
            a.command_answer(SIGN, "owner", &[serde_json::json!("Revoked")], Some("i-1"))
        });
        let held = reached.recv_timeout(Duration::from_secs(20)).is_ok();
        let out = sign_out(&b);
        let _ = release.send(());
        let result = command.join();
        assert!(
            held,
            "the command's actor was captured before its authority"
        );
        (result, out)
    });
    assert!(out.starts_with("HTTP/1.1 303"), "{out}");
    assert!(
        matches!(&result, Ok(Err(_))) || matches!(&result, Ok(Ok(a)) if !a.committed),
        "{result:?}"
    );
    assert_eq!(schema.count(ENTRIES), "0");
    assert_eq!(schema.count("pw_interactions"), "0");
    assert_eq!(schema.count("pw_outbox"), "0");
}

/// **A sign-out waits for a command admitted before it**, which commits as
/// the session it was admitted as, once; then the session is no one.
#[test]
fn a_sign_out_waits_for_a_command_admitted_before_it() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "first");
    let (a, b) = pair(&out, &schema);
    let (entered, reached) = std::sync::mpsc::channel();
    let (release, go) = std::sync::mpsc::channel::<()>();
    *a.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
        let _ = entered.send(());
        let _ = go.recv_timeout(Duration::from_secs(20));
    }));
    let (answer, out, waited, early) = std::thread::scope(|scope| {
        let command = scope
            .spawn(|| a.command_answer(SIGN, "owner", &[serde_json::json!("First")], Some("i-1")));
        let held = reached.recv_timeout(Duration::from_secs(20)).is_ok();
        let closing = scope.spawn(|| sign_out(&b));
        let until = Instant::now() + Duration::from_secs(3);
        let mut waited = false;
        while Instant::now() < until && !closing.is_finished() {
            if schema.waiting() == "1" {
                waited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let early = closing.is_finished();
        let _ = release.send(());
        let answer = command.join();
        let out = closing.join();
        assert!(held, "the command reached its commit, fenced");
        (answer, out, waited, early)
    });
    assert!(waited, "the sign-out waits on the command's session lock");
    assert!(!early, "and does not finish before it");
    // Its answer may say the session changed after it committed: the
    // database says what it committed.
    assert!(answer.is_ok(), "{answer:?}");
    assert!(
        matches!(&out, Ok(o) if o.starts_with("HTTP/1.1 303")),
        "{out:?}"
    );
    assert_eq!(schema.count(ENTRIES), "1");
    assert_eq!(schema.count("pw_interactions"), "1");
    assert_eq!(schema.count("pw_sessions"), "0", "signed out once it ended");
}

/// **A session past its lifetime at the commit keeps nothing**: the
/// database's clock passes the session's lifetime while the command is at
/// its commit, and the fence's last check refuses it.
#[test]
fn a_session_past_its_lifetime_at_the_commit_keeps_nothing() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "expired");
    let (a, _b) = pair(&out, &schema);
    a.identity
        .principals()
        .expire_after(identity::SessionLifetimes {
            idle: Duration::from_secs(2),
            absolute: Duration::from_secs(2),
        });
    let crossed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = crossed.clone();
    let watch = schema.database().url;
    let hash = identity::kept_hash("owner");
    let name = schema.name.clone();
    *a.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
        let Ok(mut c) = postgres::Client::connect(&watch, postgres::NoTls) else {
            return;
        };
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            let past = c
                .query_one(
                    &format!(
                        "SELECT clock_timestamp() > opened + interval '2 seconds' FROM {}.pw_sessions \
                         WHERE hash = $1",
                        crate::feed_pg::ident(&name)
                    ),
                    &[&hash],
                )
                .ok()
                .and_then(|r| r.try_get::<_, bool>(0).ok());
            if past == Some(true) {
                observed.store(true, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }));
    let result = a.command_answer(SIGN, "owner", &[serde_json::json!("Late")], Some("i-1"));
    assert!(
        crossed.load(Ordering::SeqCst),
        "the database's clock passed the session's lifetime at the commit"
    );
    assert!(
        result.is_err() || result.as_ref().is_ok_and(|a| !a.committed),
        "{result:?}"
    );
    assert_eq!(schema.count(ENTRIES), "0");
    assert_eq!(schema.count("pw_interactions"), "0");
    assert_eq!(schema.count("pw_outbox"), "0");
}

/// **A command waiting for its session holds no other session's command
/// up** (the review of 3ccd083a, finding 2): a reader's command, admitted,
/// waits for its session's lock, which another host's command fenced on
/// the session took meanwhile, before it takes its host's turn, so another
/// reader's command on the host commits at once; once the lock is given
/// back, the waiting command commits too. (The other host's command writes
/// nothing, and each reader's writes a note of its own: commands writing
/// what another reads wait, or are refused, as they should, which is not
/// what is held here.)
#[test]
fn a_command_waiting_for_its_session_holds_no_other_session_up() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("later.pw", LATER)]);
    let schema = Schema::new(&url, "turns");
    let (a, b) = pair(&out, &schema);
    a.identity.signed_in_for_test("other", "u-bob", "Bob");
    // The command here is admitted, then held before its authority until the
    // command there holds the session's lock (an admission waits for that
    // lock too, so it is admitted first).
    let (admitted, admission) = std::sync::mpsc::channel();
    let (proceed, onward) = std::sync::mpsc::channel::<()>();
    *a.before_command_authority.lock().expect("hook") = Some(Box::new(move |_| {
        let _ = admitted.send(());
        let _ = onward.recv_timeout(Duration::from_secs(20));
    }));
    let noted = |s: &Server, session: &str, id: &str| {
        s.command_answer(NOTE_LATER, session, &[serde_json::json!(id)], None)
    };
    let actor = b
        .identity
        .principals()
        .command_actor("owner")
        .asserted("the session, on the other host");
    let (held, waited, other, elapsed, still, here) = std::thread::scope(|scope| {
        let here = scope.spawn(|| noted(&a, "owner", "here"));
        let first = admission.recv_timeout(Duration::from_secs(20)).is_ok();
        // The other host's command, fenced on the session: its lock taken.
        let mut there = b.data.begin("owner");
        let held = first && there.authorize(&actor).is_ok();
        let _ = proceed.send(());
        let until = Instant::now() + Duration::from_secs(4);
        let mut waited = false;
        while Instant::now() < until {
            if schema.waiting() == "1" {
                waited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        let other = noted(&a, "other", "meanwhile");
        let elapsed = started.elapsed();
        // Whether the waiting command still waits: it does until the lock
        // is given back, below; one that held the turn through its wait let
        // the other command run only once its own wait had failed.
        let still = !here.is_finished();
        drop(there);
        (held, waited, other, elapsed, still, here.join())
    });
    assert!(held, "another host's command holds the session's lock");
    assert!(waited, "the session's command on this host waits for it");
    assert!(matches!(&other, Ok(a) if a.committed), "{other:?}");
    assert!(
        still && elapsed < Duration::from_secs(4),
        "another session's command is not held up by the one waiting: {elapsed:?}, \
         the waiting one still waiting: {still}"
    );
    assert!(matches!(&here, Ok(Ok(a)) if a.committed), "{here:?}");
    assert_eq!(schema.count("\"Notes\""), "2");
}

/// **A run whose taking another hand suppresses is retried after its
/// backoff, never taken for a run another command completed** (the review
/// of 3ccd083a, finding 3): a trigger answers `NULL` to every deletion of a
/// run, so its command's transaction is refused, nothing of it kept, and
/// the run is retried two seconds on; not found stale and selected again
/// at once, which ran a thousand transactions a second.
#[test]
fn a_run_whose_taking_is_suppressed_is_retried_after_its_backoff() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Notes.pw", NOTES), ("later.pw", LATER)]);
    let schema = Schema::new(&url, "suppressed");
    let s = serve(&out, &schema);
    let noted = s
        .command_answered(NOTE_LATER, "a", &[Val::String("x".into())], None)
        .asserted("answered");
    assert!(noted.committed, "{noted:?}");
    schema
        .sql(
            "CREATE FUNCTION keep() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RETURN NULL; END $$; \
             CREATE TRIGGER keep BEFORE DELETE ON pw_runs FOR EACH ROW EXECUTE FUNCTION keep();",
        )
        .asserted("a trigger suppressing every deletion of a run");
    s.query_clock.0.advance(31 * 60 * 1000);
    assert_eq!(s.run_due().0, 1, "selected and run");
    let counted = |n: &std::sync::atomic::AtomicU64| n.load(Ordering::SeqCst);
    assert_eq!(
        (
            counted(&s.counted_runs.stale),
            counted(&s.counted_runs.retried)
        ),
        (0, 1),
        "retried, not found stale"
    );
    let kept = s.runs().all_runs().asserted("the runs");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(
        kept[0].attempts == 1 && kept[0].due_ms > s.now(),
        "due after its backoff: {kept:?}"
    );
    assert_eq!(s.run_due().0, 0, "not selected again at once");
    assert_eq!(
        schema.texts("SELECT convert_from(text, 'UTF8') FROM \"Notes\""),
        ["noted"],
        "nothing of its command kept"
    );
    assert_eq!(schema.count("pw_interactions"), "0");
}

/// A table of counts, and a command the clock invokes that counts and
/// schedules itself again.
const COUNTS: &str = "module Counts

type Count = Count { id: String, n: Int }

table Counts
    row Count
    key id
";

const TICKS: &str = "module durable.ticks

import clock.{ minutes }
import Counts.{ Count }
import Counts

opaque type InteractionId = String

command tick(id: String) -> Result<Int, String> !{ clock.schedule, database.read<Counts>, database.write<Counts> }
    invoked_by    Clock
    idempotent_by InteractionId
{
    let n = match Counts.get(id) {
        Some(c) => c.n + 1,
        None => 1,
    }
    Counts.put(Count { id: id, n: n })
    schedule tick(id) after minutes(30)
    Ok(n)
}

command start(id: String) -> Result<Int, String> !{ clock.schedule, database.write<Counts> }
{
    Counts.put(Count { id: id, n: 0 })
    schedule tick(id) after minutes(30)
    Ok(0)
}
";

/// **A run that schedules itself again runs each generation once** (the
/// review of 3ccd083a, finding 6): its command takes the run it completes
/// before it writes the next generation under the same key, so the taking
/// finds the run selected, and the generation it writes is the one kept,
/// due a half hour on; each runs once it is due, and none sooner.
#[test]
fn a_run_that_schedules_itself_again_runs_each_generation_once() {
    let Some(url) = database() else { return };
    let (_dir, out) = super::tables::built(&[], &[("Counts.pw", COUNTS), ("ticks.pw", TICKS)]);
    let schema = Schema::new(&url, "ticks");
    let s = serve(&out, &schema);
    let started = s
        .command_answered("durable.ticks.start", "a", &[Val::String("t".into())], None)
        .asserted("answered");
    assert!(started.committed, "{started:?}");
    let counts = || schema.texts("SELECT n::text FROM \"Counts\"");
    for n in 1..=3 {
        let before = s.runs().all_runs().asserted("the runs");
        assert_eq!(before.len(), 1, "{before:?}");
        assert_eq!(
            s.run_due().0,
            0,
            "generation {n} is not due before its time"
        );
        s.query_clock.0.advance(31 * 60 * 1000);
        assert_eq!(s.run_due().0, 1, "generation {n} is selected");
        assert_eq!(counts(), [n.to_string()], "generation {n} ran once");
        let after = s.runs().all_runs().asserted("the runs");
        assert!(
            after.len() == 1 && after[0].id != before[0].id && after[0].due_ms > s.now(),
            "the next generation kept, due on: {before:?} then {after:?}"
        );
    }
    assert_eq!(s.counted_runs.stale.load(Ordering::SeqCst), 0);
}

/// **A commit whose connection ends under its `COMMIT` is reconciled, not
/// answered refused** (the review of 3ccd083a, finding 6): the database's
/// session ends as the commit runs (a deferred trigger of another hand's
/// ends its own backend), so whether it committed is unknown to the
/// adapter; the host reconciles it from the interaction's record, finds
/// none, and says the outcome is unknown, never that it did not commit.
/// Nothing was kept, and the host's next command commits.
#[test]
fn a_commit_whose_connection_ends_under_it_is_reconciled_not_refused() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "ended");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    schema
        .sql(
            "CREATE FUNCTION die() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN PERFORM pg_terminate_backend(pg_backend_pid()); RETURN NULL; END $$; \
             CREATE CONSTRAINT TRIGGER die AFTER INSERT ON \"Entries\" \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION die();",
        )
        .asserted("a trigger ending its own backend at the commit");
    let ended = s.command_answer(SIGN, "a", &[serde_json::json!("Ended")], Some("i-1"));
    assert!(
        matches!(&ended, Err(why) if why.contains("unknown")),
        "{ended:?}"
    );
    assert_eq!(schema.count(ENTRIES), "0");
    assert_eq!(schema.count("pw_interactions"), "0");
    schema
        .sql("DROP TRIGGER die ON \"Entries\"")
        .asserted("the trigger dropped");
    assert!(sign(&s, "a", "After", "i-2").committed);
    assert_eq!(schema.count(ENTRIES), "1");
}

/// **A path to the database that loses one `COMMIT`'s answer**: every byte
/// passed both ways until it is armed; then the next `COMMIT` a client
/// sends is passed on, the database's answer read and dropped, and both
/// ends closed (the database's session ends with them, and every lock it
/// held), so the commit landed and its client cannot know it.
struct LosingProxy {
    port: u16,
    upstream: String,
    armed: Arc<std::sync::atomic::AtomicBool>,
    lost: Arc<AtomicUsize>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl LosingProxy {
    /// A proxy to the database `url` names (`host:port`, after its `@`).
    fn start(url: &str) -> LosingProxy {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener, TcpStream};
        let authority = url
            .split_once("://")
            .and_then(|(_, rest)| rest.split('/').next())
            .and_then(|a| a.rsplit('@').next())
            .map(|hp| {
                if hp.contains(':') {
                    hp.to_string()
                } else {
                    format!("{hp}:5432")
                }
            });
        let upstream = authority.asserted_some("a database URL with a host");
        let listener = TcpListener::bind("127.0.0.1:0").asserted("a port for the proxy");
        let port = listener.local_addr().asserted("its address").port();
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lost = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (to, arm, losses, stop) = (
            upstream.clone(),
            armed.clone(),
            lost.clone(),
            stopped.clone(),
        );
        std::thread::spawn(move || {
            for client in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let (Ok(client), Ok(server)) = (client, TcpStream::connect(&to)) else {
                    continue;
                };
                let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
                // The database's answers: passed, unless its COMMIT was cut.
                if let (Ok(mut from), Ok(mut back)) = (server.try_clone(), client.try_clone()) {
                    let (cut, losses) = (cut.clone(), losses.clone());
                    std::thread::spawn(move || {
                        let mut buf = [0u8; 16384];
                        loop {
                            let n = match from.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if cut.load(Ordering::SeqCst) {
                                losses.fetch_add(1, Ordering::SeqCst);
                                let _ = back.shutdown(Shutdown::Both);
                                let _ = from.shutdown(Shutdown::Both);
                                break;
                            }
                            if back.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                    });
                }
                // The client's messages, one at a time: the first untyped.
                let (arm, cut) = (arm.clone(), cut);
                std::thread::spawn(move || {
                    let (mut from, mut onward) = (client, server);
                    let mut length = [0u8; 4];
                    if from.read_exact(&mut length).is_err() {
                        return;
                    }
                    let mut body =
                        vec![0u8; (u32::from_be_bytes(length) as usize).saturating_sub(4)];
                    if from.read_exact(&mut body).is_err()
                        || onward.write_all(&length).is_err()
                        || onward.write_all(&body).is_err()
                    {
                        return;
                    }
                    loop {
                        let mut head = [0u8; 5];
                        if from.read_exact(&mut head).is_err() {
                            break;
                        }
                        let n = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
                        let mut body = vec![0u8; n.saturating_sub(4)];
                        if from.read_exact(&mut body).is_err() {
                            break;
                        }
                        if head[0] == b'Q'
                            && body.starts_with(b"COMMIT")
                            && arm.swap(false, Ordering::SeqCst)
                        {
                            cut.store(true, Ordering::SeqCst);
                        }
                        if onward.write_all(&head).is_err() || onward.write_all(&body).is_err() {
                            break;
                        }
                    }
                    let _ = onward.shutdown(Shutdown::Write);
                });
            }
        });
        LosingProxy {
            port,
            upstream,
            armed,
            lost,
            stopped,
        }
    }

    /// `url`, its database reached through the proxy.
    fn url(&self, url: &str) -> String {
        url.replacen(
            &format!("@{}", self.upstream),
            &format!("@127.0.0.1:{}", self.port),
            1,
        )
        .replacen(
            &format!("@{}/", self.upstream.trim_end_matches(":5432")),
            &format!("@127.0.0.1:{}/", self.port),
            1,
        )
    }
}

impl Drop for LosingProxy {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// **A commit that landed and whose answer was lost is answered committed,
/// from its record** (the review of 3ccd083a, finding 6): the connection's
/// path to the database passes the `COMMIT` on and loses its answer, so the
/// adapter cannot know whether it committed; the host reconciles it from
/// the interaction's record the commit kept, and answers it committed,
/// once, and the same again when it is sent again.
#[test]
fn a_commit_whose_answer_was_lost_is_answered_from_its_record() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "lost");
    let proxy = LosingProxy::start(&url);
    let mut database = schema.database();
    database.url = proxy.url(&database.url);
    assert!(
        database.url.contains(&format!("@127.0.0.1:{}", proxy.port)),
        "{}",
        database.url
    );
    let s = served_on(&out, &database);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let armed = proxy.armed.clone();
    *s.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
        armed.store(true, Ordering::SeqCst);
    }));
    let landed = s.command_answer(SIGN, "a", &[serde_json::json!("Landed")], Some("i-1"));
    assert_eq!(
        proxy.lost.load(Ordering::SeqCst),
        1,
        "the COMMIT's answer was lost"
    );
    assert!(matches!(&landed, Ok(a) if a.committed), "{landed:?}");
    assert_eq!(schema.count(ENTRIES), "1");
    assert_eq!(schema.count("pw_interactions"), "1");
    let again = s.command_answer(SIGN, "a", &[serde_json::json!("Landed")], Some("i-1"));
    assert_eq!(again, landed, "answered as it was");
    assert_eq!(schema.count(ENTRIES), "1");
}

/// **A commit the database rolled back is answered refused, at once** (the
/// review of 3ccd083a, finding 6): a serialization failure at its `COMMIT`
/// (SQLSTATE 40001, as a deferred trigger of another hand's raises it)
/// keeps nothing, and the host knows it: the reader is told the command did
/// not commit, never that its outcome is unknown.
#[test]
fn a_commit_the_database_rolled_back_is_answered_refused() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "rolled");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    schema
        .sql(
            "CREATE FUNCTION conflict() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'could not serialize' USING ERRCODE = '40001'; END $$; \
             CREATE CONSTRAINT TRIGGER conflict AFTER INSERT ON \"Entries\" \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION conflict();",
        )
        .asserted("a trigger failing the commit's serialization");
    let refused = s.command_answer(SIGN, "a", &[serde_json::json!("Conflict")], Some("i-1"));
    assert!(
        matches!(&refused, Ok(a) if !a.committed
            && a.why.as_deref().is_some_and(|why| why.contains("Serialization"))),
        "{refused:?}"
    );
    assert_eq!(schema.count(ENTRIES), "0");
    assert_eq!(schema.count("pw_interactions"), "0");
}

/// **The sessions past their lifetimes and the sign-ins past theirs are
/// swept by the host** (the review of 3ccd083a, finding 4), not only where
/// the same cookie comes back: the scheduler's passes sweep them, a minute
/// apart at most, a bounded batch at a time; the sessions alive and the
/// sign-ins under way are kept.
#[test]
fn sessions_and_sign_ins_past_their_lifetimes_are_swept_by_the_host() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "swept");
    let s = serve(&out, &schema);
    s.identity
        .principals()
        .expire_after(identity::SessionLifetimes {
            idle: Duration::from_secs(3600),
            absolute: Duration::from_secs(7200),
        });
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    s.run_due();
    assert_eq!(schema.count("pw_sessions"), "2");
    // Ada's idle past her hour; a sign-in begun eleven minutes ago, and one
    // begun now.
    schema
        .sql(
            "UPDATE pw_sessions SET seen = seen - interval '2 hours' WHERE user_id = 'u-ada'; \
             INSERT INTO pw_sign_ins (hash, nonce, verifier, started) VALUES \
             (decode(repeat('01', 32), 'hex'), 'old-nonce', 'old-verifier', \
              clock_timestamp() - interval '11 minutes'), \
             (decode(repeat('02', 32), 'hex'), 'new-nonce', 'new-verifier', clock_timestamp());",
        )
        .asserted("aged");
    s.run_due();
    assert_eq!(
        (schema.count("pw_sessions"), schema.count("pw_sign_ins")),
        ("2".to_string(), "2".to_string()),
        "a minute apart at most"
    );
    s.query_clock.0.advance(61 * 1000);
    s.run_due();
    assert_eq!(schema.texts("SELECT user_id FROM pw_sessions"), ["u-bob"]);
    assert_eq!(schema.texts("SELECT nonce FROM pw_sign_ins"), ["new-nonce"]);
}

/// **A command is fenced from its first statement or never** (the review of
/// 3ccd083a, finding 6): a staging whose transaction began before its
/// reader's session authority was asked for (its table functions linked
/// first) is refused that authority, and is unusable from then on: it links
/// nothing more and commits nothing, knowing it kept nothing. Asked first,
/// the authority fences the command, which commits.
#[test]
fn a_command_is_fenced_from_its_first_statement_or_never() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "fenced");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let tables = super::conformance::tables_of(&out);
    let [_, (writes, granted)] = super::conformance::suite_contracts(&tables);
    let actor = s
        .identity
        .principals()
        .command_actor("a")
        .asserted("admitted");
    let mut late = s.data.begin("a");
    let linked = late.table_ops(&writes, &granted).map(|_| ());
    let refused = late.authorize(&actor);
    let relinked = late.table_ops(&writes, &granted).map(|_| ());
    let committed = late.commit(&[], &[], None, &[], None).map(|_| ());
    let definite = late.definitely_refused();
    drop(late);
    assert_eq!(linked, Ok(()), "linked before its authority");
    assert!(refused.is_err(), "and refused it after");
    assert!(
        relinked.is_err() && committed.is_err() && definite,
        "unusable: {relinked:?} {committed:?} {definite}"
    );
    // The neighbour: asked first, it fences the command, which commits.
    let mut first = s.data.begin("a");
    assert_eq!(first.authorize(&actor), Ok(()));
    assert!(first.table_ops(&writes, &granted).is_ok());
    assert!(matches!(
        first.commit(&[], &[], None, &[], None),
        Ok(data::Commit::Applied(_))
    ));
}

/// **The conformance suite, its layer under test this one** (`conformance.rs`
/// takes the layer, ADR-0338's ruling 7): each server it builds served by a
/// PostgreSQL layer on a schema of its own, every schema dropped when the
/// test is done.
fn suite_on_postgresql(
    out: &std::path::Path,
    url: &str,
    what: &str,
) -> super::conformance::Checked {
    let schemas: std::cell::RefCell<Vec<Schema>> = Default::default();
    let layer = |build: &std::path::Path| {
        let schema = Schema::new(url, what);
        let layer = pg_layer(build, &schema.database());
        schemas.borrow_mut().push(schema);
        layer
    };
    super::conformance::conformance(out, &layer)
}

/// **The conformance suite holds for the guestbook's tables on
/// PostgreSQL**: `conformance.rs`'s suite, as written, its oracle the same,
/// and every table refused as on memory, by what it declares (a reference
/// to no row, a row deleted while another refers to it, its bound), at
/// whichever of the write or the `COMMIT` the database refuses it.
#[test]
fn the_suite_holds_for_the_guestbooks_tables_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let checked = suite_on_postgresql(&out, &url, "suite_book");
    assert!(checked.reads > 0);
    assert_eq!(
        checked.refusals,
        BTreeMap::from([("Books".to_string(), 1), ("Entries".to_string(), 2)]),
        "Books deleted while an entry refers to it; Entries referring to no book, and past \
         its bound"
    );
    eprintln!(
        "conformance on PostgreSQL: the guestbook's tables, {} reads checked, refusals {:?}",
        checked.reads, checked.refusals
    );
}

/// **The conformance suite holds for the lists app's tables on
/// PostgreSQL**, every table refused as on memory.
#[test]
fn the_suite_holds_for_the_lists_apps_tables_on_postgresql() {
    let Some(url) = database() else { return };
    let mut sources = super::tables::TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_dir, out) = super::tables::built(&sources, &[]);
    let checked = suite_on_postgresql(&out, &url, "suite_lists");
    assert!(checked.reads > 0);
    assert_eq!(
        checked.refusals,
        BTreeMap::from([("Items".to_string(), 1), ("Lists".to_string(), 2)]),
        "Items referring to no list; Lists deleted while an item refers to it, and past its \
         bound"
    );
    eprintln!(
        "conformance on PostgreSQL: the lists app's tables, {} reads checked, refusals {:?}",
        checked.reads, checked.refusals
    );
}

/// **Part 5, idempotency, on PostgreSQL**: each command of the guestbook
/// that declares `idempotent_by`, sent again with its interaction, is
/// answered as it was and writes nothing more (`conformance.rs`'s, on this
/// layer).
#[test]
fn each_idempotent_command_sent_again_is_answered_once_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let schema = Schema::new(&url, "again");
    let s = serve(&out, &schema);
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    for (command, args) in [
        ("guestbook.book.sign", serde_json::json!(["Once"])),
        ("guestbook.book.remove", serde_json::json!([1])),
    ] {
        let args = args.as_array().expect("arguments").clone();
        let first = s
            .command_answer(command, "a", &args, Some("i-again"))
            .asserted("answered");
        assert!(first.committed, "{command}: {first:?}");
        let after_first = schema.texts(
            "SELECT number::text || convert_from(text, 'UTF8') FROM \"Entries\" ORDER BY number",
        );
        let records = schema.count("pw_interactions");
        let again = s
            .command_answer(command, "a", &args, Some("i-again"))
            .asserted("answered");
        assert_eq!(again, first, "{command}: the retry is answered as it was");
        assert_eq!(
            schema.texts(
                "SELECT number::text || convert_from(text, 'UTF8') FROM \"Entries\" ORDER BY number"
            ),
            after_first,
            "{command}: and writes nothing more"
        );
        assert_eq!(schema.count("pw_interactions"), records);
    }
}

/// **Each query of a program reads its tables a fixed number of times on
/// PostgreSQL, whatever they hold** (`conformance.rs`'s pins, on this
/// layer): one call of each table function at 3 rows a table and at 24, and
/// one statement each.
#[test]
fn each_query_reads_its_tables_a_fixed_number_of_times_on_postgresql() {
    let Some(url) = database() else { return };
    let mut sources = super::tables::TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_lists_dir, lists) = super::tables::built(&sources, &[]);
    let (_book_dir, book) = guestbook();
    let text = |s: &str| Val::String(s.into());
    // The reader's handle, as the platform makes it for the session the
    // pins read as (its guest's, from the session's digest), as on memory.
    let reader = crate::identity::guest_user("conformance");
    let pins: Vec<(&std::path::Path, &str, Vec<Val>, Vec<&str>)> = vec![
        (
            &lists,
            "tables.app.EveryList",
            vec![],
            vec!["pw:tables/lists#all"],
        ),
        (
            &lists,
            "tables.app.ListAt",
            vec![text("v0")],
            vec!["pw:tables/lists#by-slug"],
        ),
        (
            &lists,
            "tables.app.ItemsOf",
            vec![text("v0")],
            vec!["pw:tables/items#of-list"],
        ),
        (
            &lists,
            "tables.app.OpenItems",
            vec![],
            vec!["pw:tables/items#by-done"],
        ),
        (
            &lists,
            "tables.app.EveryItem",
            vec![],
            vec!["pw:tables/items#of-list-many", "pw:tables/lists#all"],
        ),
        (
            &book,
            "guestbook.book.Latest",
            vec![text(&reader), Val::S64(20)],
            vec!["pw:tables/entries#of-book"],
        ),
        (&book, "guestbook.book.Me", vec![text(&reader)], vec![]),
    ];
    for n in [3, 24] {
        for (build, query, args, reads) in &pins {
            let schema = Schema::new(&url, "pins");
            let s = serve(build, &schema);
            let tables = super::conformance::tables_of(build);
            super::conformance::written(&s, &tables, &super::conformance::generated(&tables, n, 0));
            let calls = s.calls.lock().expect("calls").clone();
            let statements = s
                .data
                .statements()
                .asserted_some("a layer that counts statements");
            s.answer(query, "conformance", args).asserted(query);
            let made: BTreeMap<String, u64> = s
                .calls
                .lock()
                .expect("calls")
                .iter()
                .filter(|(op, _)| pw_host::tables::is_table_import(op))
                .map(|(op, n)| (op.clone(), n - calls.get(op).copied().unwrap_or(0)))
                .filter(|(_, n)| *n > 0)
                .collect();
            let expected: BTreeMap<String, u64> =
                reads.iter().map(|op| (op.to_string(), 1)).collect();
            assert_eq!(made, expected, "{query} at {n} rows a table");
            let ran = s.data.statements().asserted_some("statements") - statements;
            assert_eq!(
                ran,
                reads.len() as u64,
                "{query} at {n} rows a table: one statement a read"
            );
        }
    }
}

/// An option asserted to hold, then read.
trait AssertedSome<T> {
    fn asserted_some(self, what: &str) -> T;
}

impl<T> AssertedSome<T> for Option<T> {
    fn asserted_some(self, what: &str) -> T {
        assert!(self.is_some(), "{what}");
        match self {
            Some(value) => value,
            None => unreachable!("asserted above"),
        }
    }
}
