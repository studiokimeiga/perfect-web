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
