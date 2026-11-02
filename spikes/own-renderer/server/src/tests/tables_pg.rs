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

/// **A build served on PostgreSQL**, its tables in `schema`: one host;
/// another on the same schema is a second host of one deployment.
pub(super) fn serve(out: &std::path::Path, schema: &Schema) -> Server {
    Server::from_build_tables(
        out.to_path_buf(),
        out.to_path_buf(),
        Some(&schema.database()),
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
    assert_eq!(
        schema.texts("SELECT kind || ':' || name || ':' || args::text FROM pw_outbox"),
        ["event:guestbook.book.Signed:[\"guestbook\"]"]
    );
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
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "Together", "i-1").committed);
    let xmins = schema.texts(
        "SELECT xmin::text FROM \"Entries\" UNION ALL SELECT xmin::text FROM \"Books\" \
         UNION ALL SELECT xmin::text FROM pw_interactions UNION ALL SELECT xmin::text FROM pw_outbox",
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
    let refused = s.command_answer(SIGN, "a", &[serde_json::json!("Refused")], Some("i-2"));
    assert!(
        refused.is_err() || refused.as_ref().is_ok_and(|a| !a.committed),
        "{refused:?}"
    );
    assert_eq!(schema.count(ENTRIES), "1");
    assert_eq!(schema.texts("SELECT signed::text FROM \"Books\""), ["1"]);
    assert_eq!(schema.count("pw_interactions"), "1");
    assert_eq!(schema.count("pw_outbox"), "1");
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

/// A server on a fresh schema for each part of the suite, each schema
/// dropped when the test is done.
fn suite_on_postgresql(out: &std::path::Path, url: &str, what: &str) -> usize {
    let schemas: std::cell::RefCell<Vec<Schema>> = Default::default();
    super::conformance::conformance(out, || {
        let schema = Schema::new(url, what);
        let s = serve(out, &schema);
        schemas.borrow_mut().push(schema);
        s
    })
}

/// **The conformance suite holds for the guestbook's tables on
/// PostgreSQL**: `conformance.rs`'s suite, as written, its oracle the same.
#[test]
fn the_suite_holds_for_the_guestbooks_tables_on_postgresql() {
    let Some(url) = database() else { return };
    let (_dir, out) = guestbook();
    let checked = suite_on_postgresql(&out, &url, "suite_book");
    assert!(checked > 0);
    eprintln!("conformance on PostgreSQL: the guestbook's tables, {checked} reads checked");
}

/// **The conformance suite holds for the lists app's tables on
/// PostgreSQL.**
#[test]
fn the_suite_holds_for_the_lists_apps_tables_on_postgresql() {
    let Some(url) = database() else { return };
    let mut sources = super::tables::TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_dir, out) = super::tables::built(&sources, &[]);
    let checked = suite_on_postgresql(&out, &url, "suite_lists");
    assert!(checked > 0);
    eprintln!("conformance on PostgreSQL: the lists app's tables, {checked} reads checked");
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
            vec![text("u-conformance"), Val::S64(20)],
            vec!["pw:tables/entries#of-book"],
        ),
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
