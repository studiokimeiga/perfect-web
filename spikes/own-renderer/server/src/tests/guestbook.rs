//! **The guide's guestbook, on tables it declares** (track `data-path`):
//! `examples/guestbook` declares its books and entries (`Books.pw`,
//! `Entries.pw`), reads and writes them through the functions the compiler
//! writes for them, and is served by the development server through the
//! table gate with no data layer of its own: its page, its signing and
//! removing, who may remove what, its numbers, a retry answered once, and
//! the other readers told of a commit.

use super::tables::Asserted;
use super::*;

const SIGN: &str = "guestbook.book.sign";
const REMOVE: &str = "guestbook.book.remove";
const LATEST: &str = "guestbook.book.Latest";

/// The guestbook, built as `guestbook.sh` builds it, and served.
fn served_guestbook() -> Served {
    let (dir, out) = super::tables::built(&["examples/guestbook"], &[]);
    Served {
        server: Server::from_build(out.clone(), out).asserted("served"),
        _dir: dir,
    }
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

/// `session` signs `text`, as its page's handler sends it, interaction
/// `interaction`: what it was answered.
fn sign(s: &Server, session: &str, text: &str, interaction: &str) -> Answered {
    s.command_answer(SIGN, session, &[serde_json::json!(text)], Some(interaction))
        .asserted("signing is answered")
}

/// `session` removes entry `number`.
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

/// **The guestbook is served from the tables it declares**, by the generic
/// adapter, and its page reads them: empty, then an entry signed, newest
/// first, each reader told which are theirs; a removed one goes.
#[test]
fn the_guestbook_is_served_from_the_tables_it_declares() {
    let s = served_guestbook();
    assert!(
        s.data.tables().is_some(),
        "served by the tables' layer, from its build's tables.json"
    );
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    let empty = shown(&s, "c");
    assert!(empty.contains("No one has signed yet."), "{empty}");
    assert!(empty.contains("Sign in to sign the guestbook."), "{empty}");
    assert!(sign(&s, "a", "Hello from Ada", "i-1").committed);
    assert!(sign(&s, "b", "Hello from Bob", "i-2").committed);
    assert_eq!(
        latest(&s, "a"),
        [
            line(2, "Bob", "Hello from Bob", false),
            line(1, "Ada", "Hello from Ada", true),
        ],
        "newest first, Ada's hers"
    );
    assert_eq!(
        latest(&s, "b"),
        [
            line(2, "Bob", "Hello from Bob", true),
            line(1, "Ada", "Hello from Ada", false),
        ],
        "and Bob's his"
    );
    let page = shown(&s, "a");
    assert!(page.contains("Signed in as Ada"), "{page}");
    assert!(page.contains("Hello from Bob"), "{page}");
    assert_eq!(
        page.matches("Remove").count(),
        1,
        "one Remove, hers: {page}"
    );
    let signed_out = shown(&s, "c");
    assert!(signed_out.contains("Hello from Ada"), "{signed_out}");
    assert!(!signed_out.contains("Remove"), "{signed_out}");
    assert!(remove(&s, "a", 1, "i-3").committed);
    assert_eq!(latest(&s, "b"), [line(2, "Bob", "Hello from Bob", true)]);
}

/// **Whether the composer is hidden on `session`'s page**, as it is served:
/// the element it sits in, the `<div>` before "Your message", says `hidden`.
fn composer_hidden(s: &Server, session: &str) -> bool {
    let raw: String = fetched_as(s, "/", Some(session))
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    let at = raw.find("Your message");
    assert!(at.is_some(), "the composer is in the page: {raw}");
    let before = &raw[..at.unwrap_or_default()];
    let div = before.rfind("<div").map_or("", |d| &before[d..]);
    let tag = &div[..div.find('>').unwrap_or(div.len())];
    tag.split_whitespace().any(|word| word == "hidden")
}

/// **The composer is a reader's with an account alone** (the integrator's
/// note a of 2026-11-02): an entry names its author by their account, so a
/// reader with none is given no composer. Until then a guest of the guest
/// model (whom `SignedIn` holds) was given one, and its signing answered
/// `SignedOut`, which the page's handler dropped silently; now the page
/// says it (`refused`), were it ever pressed. Ada, signed in, is given the
/// composer; a reader signed out is not.
#[test]
fn the_composer_is_a_reader_with_an_accounts_alone() {
    let s = served_guestbook();
    assert!(composer_hidden(&s, "g-1"), "a guest has no account");
    let guest = sign(&s, "g-1", "As a guest", "i-1");
    assert!(!guest.committed);
    assert_eq!(
        guest.result,
        Some(serde_json::json!({"$case": "err", "value": {"$case": "signed-out"}})),
        "a guest's signing is the program's refusal, which the page says"
    );
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(!composer_hidden(&s, "a"), "Ada has an account");
    assert!(composer_hidden(&s, "c"), "a reader signed out has none");
}

/// **An entry is its author's alone to remove**: another reader's removal
/// answers `NotYours`, and keeps it; one not there, `NotFound`.
#[test]
fn an_entry_is_its_authors_alone_to_remove() {
    let s = served_guestbook();
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
    // Signed out, `requires SignedIn` refuses it before it runs.
    let refused = s
        .command_answer(REMOVE, "c", &[serde_json::json!(1)], Some("i-4"))
        .asserted("answered");
    assert_eq!(refused.refused.as_deref(), Some("SignedIn"));
    assert_eq!(latest(&s, "a"), [line(1, "Ada", "Mine", true)]);
}

/// **A removed entry's number is never given again**: the book counts every
/// entry ever signed, in the transaction that signs one.
#[test]
fn a_removed_entrys_number_is_never_given_again() {
    let s = served_guestbook();
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "First", "i-1").committed);
    assert!(remove(&s, "a", 1, "i-2").committed);
    assert!(sign(&s, "a", "Second", "i-3").committed);
    assert_eq!(latest(&s, "a"), [line(2, "Ada", "Second", true)]);
}

/// **A signing sent twice is one entry** (`idempotent_by`, ADR-0325): the
/// retry is answered as the first was, and nothing runs again.
#[test]
fn a_signing_sent_twice_is_one_entry() {
    let s = served_guestbook();
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let first = sign(&s, "a", "Once", "i-1");
    let again = sign(&s, "a", "Once", "i-1");
    assert!(first.committed && again.committed);
    assert_eq!(first.result, again.result);
    assert_eq!(latest(&s, "a"), [line(1, "Ada", "Once", true)]);
    // The neighbour: another interaction is another entry.
    assert!(sign(&s, "a", "Once", "i-2").committed);
    assert_eq!(latest(&s, "a").len(), 2);
}

/// **A commit reaches every other reader of the entries**: the sign's event
/// drops `Latest`, and another session whose open page reads it is the one
/// told, not the committing one.
#[test]
fn a_signing_reaches_every_other_open_page_of_the_entries() {
    let s = served_guestbook();
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
    assert!(
        reached.iter().any(|r| r.resource == LATEST),
        "the commit dropped the entries' query: {reached:?}"
    );
    assert_eq!(s.others_reading("a", reached), ["b"]);
}

/// The texts `Latest` answers `session`, newest first.
fn texts(s: &Server, session: &str) -> Vec<String> {
    latest(s, session)
        .into_iter()
        .map(|(_, _, text, _)| text)
        .collect()
}

/// Ada's signing's interaction, by the key its record is kept by.
fn signing(interaction: &str) -> String {
    crate::identity::interaction_key("a", SIGN, interaction)
}

/// **A fault in the materializer's database**, as a test injects one (as
/// `scheduled_memory.rs` does): `sql`, run in a transaction of its own.
fn fault(s: &Server, sql: &str) {
    let installed = s.materializer.command(|tx| {
        tx.execute_batch(sql).map_err(|e| e.to_string())?;
        Ok::<_, String>(Vec::new())
    });
    assert!(installed.is_ok(), "{sql}: {installed:?}");
}

/// **An outbox that refuses a signing's event publishes none of its rows**
/// (the review's S1; the integrator's ruling of 2026-11-02: the rows are
/// published after the outbox is written, immediately before the COMMIT).
/// The materializer's outbox refusing every insert (a trigger, the
/// review's), a signing is answered refused, never a panic: no other reader
/// sees its entry, and its interaction keeps no record. Until then its rows
/// were published first: the command panicked at the outbox, another reader
/// read the entry, and its record and its event were lost. The fault taken
/// away, Ada signs it again and it is kept, once.
#[test]
fn an_outbox_that_refuses_a_signings_event_publishes_none_of_its_rows() {
    let s = served_guestbook();
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "First", "i-1").committed);
    fault(
        &s,
        "CREATE TRIGGER refuse_outbox BEFORE INSERT ON outbox \
         BEGIN SELECT RAISE(ABORT, 'outbox refused'); END;",
    );
    // As the browser's press is answered: an error, never a panic (the
    // route answers any interaction whose commit failed as one whose
    // outcome it cannot tell, ADR-0325).
    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.command_answer(SIGN, "a", &[serde_json::json!("Second")], Some("i-2"))
    }));
    assert!(pressed.is_ok(), "the refusal is an answer, not a panic");
    let Ok(pressed) = pressed else {
        unreachable!("asserted above")
    };
    assert!(pressed.is_err(), "{pressed:?}");
    // And why, as the command's commit says it.
    let refused = signed_directly(&s, "Second", "i-3");
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("outbox refused")),
        "{refused:?}"
    );
    assert_eq!(
        texts(&s, "b"),
        ["First"],
        "no reader sees a write whose records were refused"
    );
    for refused in ["i-2", "i-3"] {
        assert_eq!(
            s.recalled_interaction(&signing(refused)).asserted("recall"),
            None,
            "and no record of it is kept"
        );
    }
    fault(&s, "DROP TRIGGER refuse_outbox");
    assert!(sign(&s, "a", "Second", "i-4").committed);
    assert_eq!(texts(&s, "b"), ["Second", "First"]);
    assert!(
        s.recalled_interaction(&signing("i-4"))
            .asserted("recall")
            .is_some()
    );
}

/// Ada signs `text` with its record keyed by `interaction`, as the press
/// runs it past the route that answers it: what its commit said.
fn signed_directly(s: &Server, text: &str, interaction: &str) -> Result<Answered, String> {
    let key = signing(interaction);
    s.command_recorded(
        SIGN,
        "a",
        &[Val::String(text.into())],
        Some(interaction),
        Some((&key, "[]")),
    )
}

/// **A COMMIT that fails after a signing's rows are published closes the
/// tables** (the review's S1; the integrator's ruling of 2026-11-02: a
/// failure after publishing fails closed). A deferred foreign key the
/// outbox's trigger breaks, which SQLite holds at COMMIT, fails the
/// materializer's COMMIT after the entry was published: the signing is
/// answered refused, its interaction keeps no record, and every read and
/// write of the tables is refused from then on, so no reader goes on
/// reading an entry whose records were lost. The neighbour: before the
/// fault, a signing is kept.
#[test]
fn a_commit_failing_after_its_rows_are_published_closes_the_tables() {
    let s = served_guestbook();
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "First", "i-1").committed);
    assert_eq!(texts(&s, "b"), ["First"]);
    // SQLite enforces foreign keys where a connection asks, outside a
    // transaction: the fault's own transaction is ended, and begun again,
    // around the asking.
    fault(&s, "COMMIT; PRAGMA foreign_keys = ON; BEGIN;");
    fault(
        &s,
        "CREATE TABLE fault_parent (id TEXT PRIMARY KEY); \
         CREATE TABLE fault_child (parent TEXT REFERENCES fault_parent (id) \
             DEFERRABLE INITIALLY DEFERRED); \
         CREATE TRIGGER break_commit AFTER INSERT ON outbox \
         BEGIN INSERT INTO fault_child VALUES ('missing'); END;",
    );
    let failed = signed_directly(&s, "Second", "i-2");
    assert!(
        failed
            .as_ref()
            .is_err_and(|why| why.contains("did not commit")),
        "{failed:?}"
    );
    assert_eq!(
        s.recalled_interaction(&signing("i-2")).asserted("recall"),
        None,
        "no record of the signing is kept"
    );
    let reader = Val::String(s.identity.principals().user_of("b"));
    let read = s.answer(LATEST, "b", &[reader, Val::S64(20)]);
    assert!(
        read.as_ref()
            .is_err_and(|why| why.contains("StoragePoisoned")),
        "the tables are closed, not read with the entry in them: {read:?}"
    );
    let next = signed_directly(&s, "Third", "i-3");
    assert!(
        next.as_ref()
            .is_err_and(|why| why.contains("StoragePoisoned")),
        "and no command writes them: {next:?}"
    );
}

/// **Everything the host keeps and logs, as text** (track `data-path`):
/// every row of every table of the materializer's database (the
/// interactions' records, their keys and what was sent, the state, the
/// outbox, the runs), its materialized entries, the keys of every cached
/// read (each private one's partition among them, and the program's handle
/// on its session where a query is keyed by it), the host's own log of
/// tellings, and what `/bench/records` and `/metrics` serve, asked with
/// `cookie`. A blob is written both as text and as hex.
pub(super) fn kept_by_host(s: &Server, cookie: &str) -> String {
    let mut kept = String::new();
    let mut dump = |tx: &rusqlite::Transaction<'_>| -> rusqlite::Result<()> {
        let names: Vec<String> = tx
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for name in names {
            let mut rows = tx.prepare(&format!("SELECT * FROM \"{name}\""))?;
            let columns = rows.column_count();
            let mut rows = rows.query([])?;
            while let Some(row) = rows.next()? {
                for at in 0..columns {
                    let value: rusqlite::types::Value = row.get(at)?;
                    if let rusqlite::types::Value::Blob(bytes) = &value {
                        kept.push_str(&String::from_utf8_lossy(bytes));
                        kept.push_str(&crate::identity::hex(bytes));
                    }
                    kept.push_str(&format!("{name}: {value:?}\n"));
                }
            }
        }
        Ok(())
    };
    let dumped = s.materializer.command(|tx| {
        dump(tx).map_err(|e| e.to_string())?;
        Ok::<_, String>(Vec::new())
    });
    assert!(
        dumped.is_ok(),
        "the materializer's database read: {dumped:?}"
    );
    kept.push_str(&format!("{:?}\n", s.materializer.all_entries()));
    s.queries.evict_where(|key| {
        kept.push_str(&format!("{key:?}\n"));
        false
    });
    kept.push_str(
        &s.tellings
            .lock()
            .expect("tellings")
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    // What the routes serve, their bodies alone: the request's own cookie is
    // the browser's to send, and is no answer's.
    let session = format!("pw-session={cookie}");
    for route in ["/bench/records", "/metrics"] {
        let answer = super::sign_in::get(s, route, &session);
        kept.push_str(answer.split_once("\r\n\r\n").map_or("", |(_, body)| body));
    }
    kept
}

/// **No session in `cookies` is in `kept`**, as written or in hex.
pub(super) fn holds_none_of(kept: &str, cookies: &[&str]) {
    for secret in cookies {
        for written in [secret.to_string(), crate::identity::hex(secret.as_bytes())] {
            assert!(
                !kept.contains(&written),
                "the host keeps or logs a session's cookie, `{written}`"
            );
        }
    }
}

/// **[`kept_by_host`] of a served guestbook, and the guestbook's rows**.
fn kept_and_logged(s: &Server, out: &std::path::Path, cookie: &str) -> String {
    let mut kept = kept_by_host(s, cookie);
    let tables = super::conformance::tables_of(out);
    let [(reads, granted), _] = super::conformance::suite_contracts(&tables);
    let ops = s
        .data
        .table_reads(&reads, &granted, None)
        .asserted("the tables' reads");
    for (import, args) in [
        ("pw:tables/entries#all", vec![]),
        ("pw:tables/books#get", vec![Val::String("guestbook".into())]),
    ] {
        let read = ops.get(import).map(|read| read(&args));
        assert!(matches!(read, Some(Ok(_))), "{import}: {read:?}");
        kept.push_str(&format!("{read:?}\n"));
    }
    kept
}

/// **Every digest the host makes of a session, pinned** (track `data-path`,
/// the integrator's rulings of 2026-11-02): for one session, each value the
/// host stores, shows or logs of it is SHA-256 over its own domain and the
/// session, as another implementation (.NET's) computed them: a guest's id,
/// the hash a session is kept by, an interaction's record's key, a private
/// read's partition, a cart's state's key, a log's name for the session, and
/// the handle a program holds it by. Pinned, no use's digest is another's,
/// none is the session's digest in no domain (`8d6894e8…`), and none holds
/// the session: a use that lost its domain, took another's, or kept the
/// session itself fails here.
#[test]
fn each_digest_of_a_session_is_its_own_domains() {
    use crate::identity::{self, ReaderIdentity};
    const SESSION: &str = "s-0123456789abcdef";
    const GUEST: &str = "39b5a104827c63ab5e67262554160bd1ed5b731470b3838805b38ecc0830b076";
    const KEPT: &str = "be0dbb4c2574847a7ae9f1b09f0ab2267491df3ae4b4d49d0121e4465a32ffa6";
    const INTERACTION: &str = "c0d990547e02d7c523acf6dfb58bf614f2c9b616a3d1b5ece72b3434613ca220";
    const PARTITION: &str = "2ef80990b57eeda58c30ec2628c39e76a9e9af80efde39b66e0621b93523451e";
    const STATE: &str = "19cf7580629386da9c9c06860ae0d2de8ee70340762504a7b4aba9e87df7e1c5";
    const LOG: &str = "49291b48f90ada91605ecd461aa1617ba7523a43b47bd4a37ddeb14a7f37c986";
    const HANDLE: &str = "b48c392062e307f8f5eb2600a21723a066070024b15e285fea135108ce29d368";
    assert_eq!(identity::guest_user(SESSION), format!("u-{}", &GUEST[..32]));
    assert_eq!(identity::hex(&identity::kept_hash(SESSION)), KEPT);
    assert_eq!(
        identity::interaction_key(SESSION, SIGN, "i-1"),
        format!("{INTERACTION}\u{1f}{SIGN}\u{1f}i-1")
    );
    assert_eq!(
        ReaderIdentity::Guest.partition(SESSION),
        format!("v2:{PARTITION}:g")
    );
    assert_eq!(
        crate::store::cart_state_key(SESSION),
        format!("cart:{STATE}")
    );
    assert_eq!(crate::masked(SESSION), format!("s{}", &LOG[..8]));
    assert_eq!(identity::session_handle(SESSION), HANDLE);
}

/// **Nothing the host keeps or logs holds a session's cookie** (the
/// integrator's ruling of 2026-11-02, from the PostgreSQL layer's review,
/// which signed in as Ada by sending a stored record's key's first field as
/// the `pw-session` cookie). A browser signs in through the flow, reads the
/// page and signs the guestbook through the command's route; then every
/// record, partition, row and log line the host kept is searched for the
/// cookie it was given, and for the one it came with, as written and in
/// hex: neither is there. The neighbour: the signing's record is kept,
/// under the key the session's digest makes, so the search is of what a
/// command keeps.
#[test]
fn nothing_the_host_keeps_or_logs_holds_a_session_cookie() {
    let (_dir, out) = super::tables::built(&["examples/guestbook"], &[]);
    let s = Server::from_build(out.clone(), out.clone()).asserted("served");
    let provider = Arc::new(super::sign_in::TestProvider::default());
    s.identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    let came_with = crate::identity::new_session_id();
    let cookie = super::sign_in::signed_in(&s, &provider, &came_with, "ada");
    let page = super::sign_in::get(&s, "/", &format!("pw-session={cookie}"));
    assert!(page.contains("Signed in as"), "{page}");
    let signed = super::sign_in::command(
        &s,
        SIGN,
        &cookie,
        "i-kept",
        r#"["Kept, and never its cookie"]"#,
    );
    assert!(signed.contains("\"committed\":true"), "{signed}");
    let record = s
        .recalled_interaction(&crate::identity::interaction_key(&cookie, SIGN, "i-kept"))
        .asserted("recall");
    assert!(
        record.is_some(),
        "the signing's record, by its digest's key"
    );
    let kept = kept_and_logged(&s, &out, &cookie);
    assert!(
        kept.contains("Kept, and never its cookie"),
        "the search reads the rows"
    );
    holds_none_of(&kept, &[&cookie, &came_with]);
}

/// **The guestbook keeps at most its bound of entries, and its page is
/// measured at it** (the review's S3; the integrator's ruling of
/// 2026-11-02). `Entries` is `bounded 1000`, since `Latest` reads every
/// entry (no paged read yet): 16,000 entries took 274 ms, and 48,000
/// exhausted a component's memory. At the bound, a signing is refused
/// `Full`, a declared error that commits nothing, whose words the page
/// shows; one entry removed, a signing is kept again. The page rendered at
/// the bound, and `Latest` read whole there, are each within the query's
/// timeout (2 s), and their times are printed.
#[test]
fn the_guestbook_keeps_its_bound_and_its_page_is_measured_at_it() {
    let (_dir, out) = super::tables::built(&["examples/guestbook"], &[]);
    let s = Server::from_build(out.clone(), out.clone()).asserted("served");
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let bound: i64 = 1000;
    let tables = super::conformance::tables_of(&out);
    let row = |value: serde_json::Value| -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_value(value).asserted("a row")
    };
    let mut rows = BTreeMap::new();
    rows.insert(
        "Books".to_string(),
        vec![row(
            serde_json::json!({"name": "guestbook", "signed": bound}),
        )],
    );
    rows.insert(
        "Entries".to_string(),
        (1..=bound)
            .map(|number| {
                row(serde_json::json!({
                    "book": "guestbook", "number": number, "author": "u-ada", "name": "Ada",
                    "text": format!("entry {number:04} {}", "w".repeat(269)),
                }))
            })
            .collect(),
    );
    assert_eq!(
        super::conformance::write_rows(&s, &tables, &rows),
        Ok(()),
        "the guestbook at its bound"
    );
    let started = std::time::Instant::now();
    let page = shown(&s, "a");
    let rendered = started.elapsed();
    assert!(
        page.contains("entry 1000") && page.contains("entry 0981"),
        "the newest twenty: {page}"
    );
    assert!(!page.contains("entry 0980"), "twenty, and no more");
    let reader = Val::String(s.identity.principals().user_of("a"));
    let started = std::time::Instant::now();
    let whole = s
        .answer(LATEST, "a", &[reader, Val::S64(bound)])
        .asserted("Latest, whole");
    let read = started.elapsed();
    assert!(
        matches!(&whole, Val::List(lines) if lines.len() == bound as usize),
        "every entry: {}",
        format!("{whole:?}").len()
    );
    eprintln!(
        "the guestbook at its bound of {bound} entries: its page rendered in {rendered:?}, \
         `Latest` read whole in {read:?}"
    );
    let timeout = std::time::Duration::from_secs(2);
    assert!(
        rendered < timeout && read < timeout,
        "{rendered:?}, {read:?}"
    );
    // Past the bound, refused, its words the program's.
    let full = sign(&s, "a", "One too many", "i-full");
    assert!(!full.committed);
    assert_eq!(
        full.result,
        Some(serde_json::json!({"$case": "err", "value": {"$case": "full"}}))
    );
    assert_eq!(latest(&s, "a")[0].0, bound, "nothing kept");
    // One removed, there is room.
    assert!(remove(&s, "a", 1, "i-remove").committed);
    assert!(sign(&s, "a", "Room again", "i-room").committed);
    assert_eq!(
        latest(&s, "a")[0],
        line(bound + 1, "Ada", "Room again", true)
    );
}

/// **One render reads one state of the tables** (NEXT 4c; the adapter's
/// snapshot): a commit landing between the page's bindings, after `me` and
/// before `entries`, is not read by that render, whose every table read is
/// its snapshot's; the page read after it shows it.
#[test]
fn a_render_reads_one_state_of_the_tables() {
    let (_dir, out) = super::tables::built(&["examples/guestbook"], &[]);
    let s = Server::from_build(out.clone(), out.clone()).asserted("served");
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    assert!(sign(&s, "a", "Before", "i-1").committed);
    let tables = super::conformance::tables_of(&out);
    let mut between: BTreeMap<String, Vec<serde_json::Map<String, serde_json::Value>>> =
        BTreeMap::new();
    between.insert(
        "Books".into(),
        vec![
            serde_json::from_value(serde_json::json!({"name": "other", "signed": 0}))
                .asserted("a book"),
        ],
    );
    between.insert(
        "Entries".into(),
        vec![serde_json::from_value(serde_json::json!({
            "book": "guestbook", "number": 9, "author": "u-bob", "name": "Bob", "text": "Between"
        }))
        .asserted("an entry")],
    );
    // The hook runs on the server's own thread: it says what it wrote, and
    // the test asserts it there.
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
    assert!(page.contains("Before"), "{page}");
    assert!(
        !page.contains("Between"),
        "a commit during the render is not the render's: {page}"
    );
    let again = shown(&s, "c");
    assert!(again.contains("Between"), "{again}");
}
