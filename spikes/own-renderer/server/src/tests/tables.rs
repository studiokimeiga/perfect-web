//! **A program's declared tables, served** (track `data-path`; ADR-0338's D2
//! routing): `examples/tables`, a program of tables, queries and commands
//! with no line of Rust of its own, built as `pw build` builds it and served
//! by the development server through the generic memory adapter: every
//! call through the table gate, each component's host map holding its own
//! table functions alone, a command's writes staged in its own transaction
//! and published as the platform's transaction's last step, with its
//! records, or not at all.

use super::*;

/// **A domain result, asserted before it is read**: a test of this track
/// fails at an assertion, never at an `expect` (D2's strict mutation
/// producer counts only an assertion's failure as a kill).
pub(super) trait Asserted<T> {
    fn asserted(self, what: &str) -> T;
}

impl<T, E: std::fmt::Debug> Asserted<T> for Result<T, E> {
    fn asserted(self, what: &str) -> T {
        assert!(self.is_ok(), "{what}: {:?}", self.as_ref().err());
        match self {
            Ok(value) => value,
            Err(_) => unreachable!("asserted above"),
        }
    }
}

pub(super) const EVERY_LIST: &str = "tables.app.EveryList";
const LIST_AT: &str = "tables.app.ListAt";
const ITEMS_OF: &str = "tables.app.ItemsOf";
const OPEN_ITEMS: &str = "tables.app.OpenItems";
const EVERY_ITEM: &str = "tables.app.EveryItem";
pub(super) const MAKE_LIST: &str = "tables.app.make_list";
const KEEP_ITEM: &str = "tables.app.keep_item";
const DROP_ITEM: &str = "tables.app.drop_item";
const CLEAR_LIST: &str = "tables.app.clear_list";
const INSERT_THEN_DIVIDE: &str = "tables.writer.insert_then_divide";
const MAKE_SIGNED: &str = "tables.guarded.make_signed";

/// The lists app's tables and their source (`examples/tables`), which each
/// of its programs is built with.
pub(super) const TABLES: &[&str] = &[
    "examples/tables/Lists.pw",
    "examples/tables/Items.pw",
    "examples/tables/ListData.pw",
];

/// **A program, as `pw build` builds it**: the platform's packages, then
/// `sources` (each a directory of `.pw` files or one file, relative to the
/// repository), then `extra`'s modules; the build's directory, held as long
/// as the test holds it.
pub(super) fn built(
    sources: &[&str],
    extra: &[(&str, &str)],
) -> (tempfile::TempDir, std::path::PathBuf) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = tempfile::TempDir::with_prefix("pw-tables-").expect("a temporary directory");
    let mut units = Vec::new();
    let mut unit = |path: String, src: String| {
        units.push(pw_core::check::Unit {
            hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
            path,
            src,
        });
    };
    for source in ["packages/pw-std", "packages/pw-platform-web"]
        .iter()
        .chain(sources)
    {
        let at = root.join(source);
        let mut paths: Vec<std::path::PathBuf> = if at.is_dir() {
            std::fs::read_dir(&at)
                .unwrap_or_else(|e| panic!("{source}: {e}"))
                .map(|e| e.expect("entry").path())
                .filter(|p| p.extension().is_some_and(|x| x == "pw"))
                .collect()
        } else {
            vec![at]
        };
        paths.sort();
        for p in paths {
            let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{source}: {e}"));
            unit(p.display().to_string(), src);
        }
    }
    for (name, src) in extra {
        unit(name.to_string(), src.to_string());
    }
    let build = pw_core::build::build(&units);
    assert!(
        build.is_ok(),
        "the program builds: {:?}",
        build.as_ref().err()
    );
    let build = build.expect("the build was asserted");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let out = dir.path().join("build");
    build.write(&out).expect("the build is written");
    (dir, out)
}

/// **One of the lists app's programs, served**: its tables and `program`
/// (`app.pw`, `writer.pw`, `guarded.pw`).
fn served_program(program: &str) -> Served {
    let mut sources = TABLES.to_vec();
    let file = format!("examples/tables/{program}");
    sources.push(&file);
    let (dir, out) = built(&sources, &[]);
    Served {
        server: Server::from_build(out.clone(), out).asserted("served"),
        _dir: dir,
    }
}

/// The lists app (`examples/tables/app.pw`), served.
fn served_tables() -> Served {
    served_program("app.pw")
}

/// A failed COMMIT closes the whole adapter, including cached queries
/// outside that command's invalidations. The cached neighbor really avoids
/// a table read; a deferred constraint then fails the actual SQLite COMMIT.
#[test]
fn closed_tables_refuse_cached_answers_outside_the_failed_commands_invalidations() {
    let s = served_tables();
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    let binding = serde_json::json!({
        "resource": LIST_AT,
        "policy": {
            "freshness_ms": 60_000, "cache": "shared", "privacy": "public", "key": [0]
        }
    });
    let args = [text("food")];
    let first = s
        .fetch_answer("b", &binding, &args)
        .asserted("a real answer");
    assert!(matches!(first, Val::Option(Some(_))), "{first:?}");
    let count = s.data.statements().ok_or("no counter").asserted("counted");
    assert_eq!(
        s.fetch_answer("b", &binding, &args).asserted("cached"),
        first
    );
    assert_eq!(
        s.data.statements(),
        Some(count),
        "a cache hit reads no table"
    );

    s.materializer
        .command(|tx| {
            tx.execute_batch(
                "COMMIT; PRAGMA foreign_keys = ON; BEGIN; \
             CREATE TABLE fault_parent (id TEXT PRIMARY KEY); \
             CREATE TABLE fault_child (parent TEXT REFERENCES fault_parent (id) \
                 DEFERRABLE INITIALLY DEFERRED); \
             CREATE TRIGGER break_commit AFTER INSERT ON state \
             BEGIN INSERT INTO fault_child VALUES ('missing'); END;",
            )
            .map_err(|e| e.to_string())?;
            Ok::<_, String>(Vec::new())
        })
        .asserted("deferred COMMIT failure installed");
    let failed = s.command_recorded(MAKE_LIST, "a", &[list("chores", "home", 10)], None, None);
    assert!(
        failed.as_ref().is_err_and(|e| e.contains("did not commit")),
        "{failed:?}"
    );
    let cold = s.answer(LIST_AT, "b", &args);
    assert!(
        cold.as_ref().is_err_and(|e| e.contains("StoragePoisoned")),
        "{cold:?}"
    );
    let cached = s.fetch_answer("b", &binding, &args);
    assert!(
        cached
            .as_ref()
            .is_err_and(|e| e.contains("StoragePoisoned")),
        "a cache hit must not bypass the closed adapter: {cached:?}"
    );
}

#[test]
fn closed_tables_refuse_an_answer_closed_while_its_query_was_in_flight() {
    let s = served_tables();
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    let binding = serde_json::json!({
        "resource": EVERY_LIST,
        "policy": { "freshness_ms": 60_000, "cache": "shared", "privacy": "public", "key": [] }
    });
    *s.in_flight.lock().expect("in flight") = Some(Box::new(|server| {
        server
            .data
            .begin("a")
            .unpublished("closed after the query read");
    }));
    let answer = s.fetch_answer("b", &binding, &[]);
    assert!(
        answer
            .as_ref()
            .is_err_and(|e| e.contains("StoragePoisoned")),
        "closing during a read must refuse its delivery: {answer:?}"
    );
}

/// A live render also remembers operation answers. Those answers must not
/// bypass an adapter closed after its snapshot and first read were taken.
#[test]
fn closed_tables_refuse_a_renders_remembered_operations() {
    let s = served_tables();
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    let _render = crate::render_merge::begin(&*s.data, s.queries.epoch());
    let first = s.answer(EVERY_LIST, "", &[]).asserted("first operation");
    let count = s.data.statements().ok_or("no counter").asserted("counted");
    assert_eq!(s.answer(EVERY_LIST, "", &[]).asserted("merged"), first);
    assert_eq!(
        s.read_value(EVERY_LIST, &[])
            .asserted("derived read merged"),
        first
    );
    assert_eq!(
        s.data.statements(),
        Some(count),
        "the render merged the operation"
    );
    s.data
        .begin("a")
        .unpublished("closed inside an existing render");
    let direct = s.answer(EVERY_LIST, "", &[]);
    assert!(
        direct
            .as_ref()
            .is_err_and(|e| e.contains("StoragePoisoned")),
        "a remembered operation must refuse a closed adapter: {direct:?}"
    );
    let derived = s.read_value(EVERY_LIST, &[]);
    assert!(
        derived
            .as_ref()
            .is_err_and(|e| e.contains("StoragePoisoned")),
        "a materialization's read without a page policy must also refuse: {derived:?}"
    );
}

#[test]
fn closed_tables_refuse_fresh_and_last_known_good_materializations() {
    const COUNT: &str = "cached.Count";
    let (_dir, out) = built(
        TABLES,
        &[(
            "cached.pw",
            r#"
module cached
import List
import Lists.{ TodoList }
import Lists

event Changed()

query EveryList() -> List<TodoList>
    consistency strong
{
    Lists.all()
}

materialize Count() -> Int {
    partition  public
    invalidates_on Changed()
    regenerate on_invalidation
    stampede   single_flight
    fallback   last_known_good

    let lists = query EveryList()
    List.length(lists)
}

command make_list(list: TodoList) -> Result<(), TodoList>
    transaction serializable
    emits Changed()
    invalidates EveryList()
{
    Lists.insert(list)
}
"#,
        )],
    );
    let s = Server::from_build(out.clone(), out).asserted("served");
    committed(
        &s,
        "cached.make_list",
        "a",
        &[list("groceries", "food", 20)],
    );
    assert_eq!(s.materialized(COUNT, &[]).asserted("derived"), Val::S64(1));
    let count = s.data.statements().ok_or("no counter").asserted("counted");
    assert_eq!(s.materialized(COUNT, &[]).asserted("kept"), Val::S64(1));
    assert_eq!(s.data.statements(), Some(count), "the fresh value was kept");

    let key = Server::kept_key(COUNT, &[]);
    s.materializer_faults
        .lock()
        .expect("faults")
        .insert(COUNT.into());
    s.materializer.invalidate(&key, 0);
    assert_eq!(
        s.materialized(COUNT, &[])
            .asserted("ordinary failure keeps fallback"),
        Val::S64(1)
    );
    assert!(matches!(
        s.materializer.read(&key),
        pw_materialize::Read::LastKnownGood(_)
    ));
    // Restore a fresh entry too, so both retained-value branches are covered.
    s.materializer_faults.lock().expect("faults").clear();
    s.regenerate_kept(COUNT, &[], None);
    assert!(matches!(
        s.materializer.read(&key),
        pw_materialize::Read::Fresh(_)
    ));
    s.data
        .begin("a")
        .unpublished("closed after a materialization was kept");
    let fresh = s.materialized(COUNT, &[]);
    assert!(
        fresh.as_ref().is_err_and(|e| e.contains("StoragePoisoned")),
        "{fresh:?}"
    );
    s.materializer.invalidate(&key, 0);
    let fallback = s.materialized(COUNT, &[]);
    assert!(
        fallback
            .as_ref()
            .is_err_and(|e| e.contains("StoragePoisoned")),
        "{fallback:?}"
    );
}

fn text(value: &str) -> Val {
    Val::String(value.into())
}

/// A `TodoList` as its WIT record declares it.
fn list(id: &str, slug: &str, added: i64) -> Val {
    Val::Record(vec![
        ("id".into(), text(id)),
        ("name".into(), text(&format!("List {id}"))),
        ("slug".into(), text(slug)),
        ("added".into(), Val::S64(added)),
    ])
}

/// An `Item` as its WIT record declares it.
fn item(list: &str, id: &str, done: bool, added: i64) -> Val {
    Val::Record(vec![
        ("list".into(), text(list)),
        ("id".into(), text(id)),
        ("title".into(), text(&format!("Item {id}"))),
        ("done".into(), Val::Bool(done)),
        ("count".into(), Val::S64(1)),
        ("added".into(), Val::S64(added)),
        ("note".into(), Val::Option(None)),
    ])
}

/// The ids of a list of lists, or of items, as a query answered it.
fn ids(value: &Val) -> Vec<String> {
    assert!(matches!(value, Val::List(_)), "a list: {value:?}");
    let Val::List(rows) = value else {
        unreachable!("asserted above")
    };
    rows.iter()
        .map(|row| {
            let id = val_at(row, &["id"]);
            assert!(matches!(id, Some(Val::String(_))), "a row's id: {id:?}");
            match id {
                Some(Val::String(id)) => id.clone(),
                _ => unreachable!("asserted above"),
            }
        })
        .collect()
}

/// A command answered, and committed.
fn committed(s: &Server, command: &str, session: &str, args: &[Val]) -> Answered {
    let answered = s
        .command_answered(command, session, args, None)
        .asserted(command);
    assert!(
        answered.committed,
        "{command} commits: {:?}",
        answered.result
    );
    answered
}

/// **A program that declares its tables is served by the generic adapter**,
/// from its build's tables.json: its queries read them through every read
/// primitive, its commands write them through every write primitive, and a
/// commit is read by every session after it: the tables are the program's.
#[test]
fn a_program_that_declares_its_tables_is_served_by_the_generic_adapter() {
    let s = served_tables();
    assert_eq!(
        s.answer(EVERY_LIST, "a", &[]).asserted("read"),
        Val::List(vec![]),
        "no list yet"
    );
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    committed(&s, MAKE_LIST, "a", &[list("chores", "home", 10)]);
    committed(&s, KEEP_ITEM, "a", &[item("groceries", "milk", false, 3)]);
    committed(&s, KEEP_ITEM, "a", &[item("groceries", "eggs", true, 1)]);
    committed(&s, KEEP_ITEM, "a", &[item("chores", "sweep", false, 2)]);
    // Read by another session: the tables are the program's, in their
    // declared order (`order added`).
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["chores", "groceries"]
    );
    let at = s.answer(LIST_AT, "b", &[text("food")]).asserted("read");
    assert!(
        matches!(&at, Val::Option(Some(found)) if val_at(found, &["id"]) == Some(&text("groceries"))),
        "the list at its slug: {at:?}"
    );
    assert_eq!(
        ids(&s
            .answer(ITEMS_OF, "b", &[text("groceries")])
            .asserted("read")),
        ["eggs", "milk"]
    );
    assert_eq!(
        ids(&s.answer(OPEN_ITEMS, "b", &[]).asserted("read")),
        ["sweep", "milk"]
    );
    assert_eq!(
        ids(&s.answer(EVERY_ITEM, "b", &[]).asserted("read")),
        ["sweep", "eggs", "milk"]
    );
    committed(&s, DROP_ITEM, "a", &[text("groceries"), text("milk")]);
    assert_eq!(
        ids(&s
            .answer(ITEMS_OF, "b", &[text("groceries")])
            .asserted("read")),
        ["eggs"]
    );
    committed(&s, CLEAR_LIST, "a", &[text("groceries")]);
    assert_eq!(
        ids(&s.answer(EVERY_ITEM, "b", &[]).asserted("read")),
        ["sweep"]
    );
}

/// **Each component's host map holds its own table functions alone**: the
/// gate's admitted imports for its contract, no other table's, and no other
/// function of its own table's.
#[test]
fn each_components_host_map_holds_its_own_table_functions_alone() {
    let s = served_tables();
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    committed(&s, KEEP_ITEM, "a", &[item("groceries", "milk", false, 3)]);
    committed(&s, DROP_ITEM, "a", &[text("groceries"), text("milk")]);
    committed(&s, CLEAR_LIST, "a", &[text("groceries")]);
    for (query, args) in [
        (EVERY_LIST, vec![]),
        (LIST_AT, vec![text("food")]),
        (ITEMS_OF, vec![text("groceries")]),
        (OPEN_ITEMS, vec![]),
        (EVERY_ITEM, vec![]),
    ] {
        s.answer(query, "a", &args).asserted("read");
    }
    let maps = s.table_maps.lock().expect("table maps").clone();
    let expected: Vec<(&str, Vec<&str>)> = vec![
        (MAKE_LIST, vec!["pw:tables/lists#insert"]),
        (KEEP_ITEM, vec!["pw:tables/items#put"]),
        (DROP_ITEM, vec!["pw:tables/items#delete"]),
        (CLEAR_LIST, vec!["pw:tables/items#delete-of-list"]),
        (EVERY_LIST, vec!["pw:tables/lists#all"]),
        (LIST_AT, vec!["pw:tables/lists#by-slug"]),
        (ITEMS_OF, vec!["pw:tables/items#of-list"]),
        (OPEN_ITEMS, vec!["pw:tables/items#by-done"]),
        (
            EVERY_ITEM,
            vec!["pw:tables/items#of-list-many", "pw:tables/lists#all"],
        ),
    ];
    assert_eq!(
        maps.iter()
            .map(|(c, keys)| (c.as_str(), keys.iter().map(String::as_str).collect()))
            .collect::<Vec<(&str, Vec<&str>)>>(),
        expected,
        "each call's host map, in the order the calls were made"
    );
    // And each is exactly what its contract imports of the tables.
    for (component, keys) in &maps {
        let contract = s
            .contracts
            .iter()
            .find(|c| c.component_id == *component)
            .expect("its contract");
        let imported: Vec<String> = contract
            .imports
            .iter()
            .map(|i| i.key())
            .filter(|key| pw_host::tables::is_table_import(key))
            .collect();
        assert_eq!(keys, &imported, "{component}");
    }
}

/// **A staged write is invisible to every other read until its commit**, a
/// query run between the command's body and its commit reading the
/// tables as they were.
#[test]
fn a_staged_write_is_invisible_until_its_commit() {
    let s = served_tables();
    let seen: Arc<Mutex<Option<Val>>> = Arc::default();
    let into = seen.clone();
    *s.before_commit.lock().expect("hook") = Some(Box::new(move |server| {
        *into.lock().expect("seen") = Some(server.answer(EVERY_LIST, "b", &[]).asserted("read"));
    }));
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    assert_eq!(
        seen.lock().expect("seen").clone(),
        Some(Val::List(vec![])),
        "staged, it was read by no other query"
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["groceries"],
        "committed, it is read"
    );
}

/// **A command that commits nothing keeps nothing**: a declared error, a
/// trap after its write, and a commit its tables refuse (a reference to a
/// list no row holds) each leave the tables as they were, and record no
/// commit in the materializer's transaction.
#[test]
fn a_command_that_does_not_commit_keeps_nothing() {
    let s = served_tables();
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    assert_eq!(
        s.materializer.state("pw:tables/Lists").as_deref(),
        Some("written"),
        "a commit's tables, recorded in its transaction"
    );
    // A declared error: the slug another list holds.
    let refused = s
        .command_answered(MAKE_LIST, "a", &[list("other", "food", 30)], None)
        .asserted("answered");
    assert!(!refused.committed, "{:?}", refused.result);
    // A commit its tables refuse: an item of no list.
    let orphan = s.command_answered(KEEP_ITEM, "a", &[item("nowhere", "milk", false, 3)], None);
    assert!(
        orphan
            .as_ref()
            .is_err_and(|why| why.contains("references a missing Lists")),
        "{orphan:?}"
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["groceries"]
    );
    assert_eq!(
        s.answer(EVERY_ITEM, "b", &[]).asserted("read"),
        Val::List(vec![])
    );
    assert_eq!(
        s.materializer.state("pw:tables/Items"),
        None,
        "no commit of the items' was recorded"
    );
    // A trap after its write (`writer.pw`'s, divided by zero) publishes
    // nothing either, beside the same command dividing by one.
    let w = served_program("writer.pw");
    let trapped = w.command_answered(
        INSERT_THEN_DIVIDE,
        "a",
        &[list("divided", "divided", 40), Val::S64(0)],
        None,
    );
    assert!(
        trapped
            .as_ref()
            .is_err_and(|why| why.contains("division by zero")),
        "{trapped:?}"
    );
    assert_eq!(w.materializer.state("pw:tables/Lists"), None);
    committed(
        &w,
        INSERT_THEN_DIVIDE,
        "a",
        &[list("divided", "divided", 40), Val::S64(1)],
    );
    assert_eq!(
        w.materializer.state("pw:tables/Lists").as_deref(),
        Some("written")
    );
}

/// **A command's rows are published as its transaction's last step**: a
/// commit whose records the materializer refuses (its interaction recorded
/// already) publishes no row, and a publication that fails rolls its
/// records back, the interaction's and the outbox's: neither is kept
/// without the other.
#[test]
fn a_commits_rows_and_records_are_kept_together_or_not_at_all() {
    let s = served_tables();
    let first = s
        .command_recorded(
            MAKE_LIST,
            "a",
            &[list("groceries", "food", 20)],
            None,
            Some(("a:make:1", "[1]")),
        )
        .asserted("answered");
    assert!(first.committed);
    // The same interaction's record again, for another list: the
    // materializer refuses its record, and the list is not published.
    let again = s.command_recorded(
        MAKE_LIST,
        "a",
        &[list("chores", "home", 10)],
        None,
        Some(("a:make:1", "[2]")),
    );
    assert!(again.is_err(), "{again:?}");
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["groceries"],
        "a commit whose records were refused published no row"
    );
    // A publication that fails: the record and the outbox roll back with it.
    crate::tables::FAIL_NEXT_PUBLISH.with(|fail| fail.set(true));
    let failed = s.command_recorded(
        MAKE_LIST,
        "a",
        &[list("chores", "home", 10)],
        None,
        Some(("a:make:2", "[3]")),
    );
    assert!(
        failed
            .as_ref()
            .is_err_and(|why| why.contains("did not publish")),
        "{failed:?}"
    );
    assert_eq!(
        s.recalled_interaction("a:make:2").asserted("recall"),
        None,
        "no record of a commit no reader sees"
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["groceries"],
        "and no row of it"
    );
    // The neighbour: the same command and record, published, keeps both.
    let kept = s
        .command_recorded(
            MAKE_LIST,
            "a",
            &[list("chores", "home", 10)],
            None,
            Some(("a:make:3", "[4]")),
        )
        .asserted("answered");
    assert!(kept.committed);
    assert!(
        s.recalled_interaction("a:make:3")
            .asserted("recall")
            .is_some()
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).asserted("read")),
        ["chores", "groceries"]
    );
}

/// **A command a predicate guards runs once its predicate holds, and writes
/// nothing where it does not** (`call_authorized_with_tables`): signed out,
/// the guarded command is refused by `SignedIn` and its list is not made;
/// signed in, it is.
#[test]
fn a_guarded_command_writes_its_tables_once_its_predicate_holds() {
    let s = served_program("guarded.pw");
    // The accounts model, someone else signed in: `a` is no one.
    s.identity.signed_in_for_test("b", "u-bob", "Bob");
    let sent =
        [serde_json::json!({"id": "signed", "name": "List signed", "slug": "signed", "added": 1})];
    let refused = s
        .command_answer(MAKE_SIGNED, "a", &sent, None)
        .asserted("answered");
    assert!(!refused.committed, "{:?}", refused.result);
    assert_eq!(refused.refused.as_deref(), Some("SignedIn"));
    assert_eq!(
        s.materializer.state("pw:tables/Lists"),
        None,
        "refused, it wrote nothing"
    );
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let made = s
        .command_answer(MAKE_SIGNED, "a", &sent, None)
        .asserted("answered");
    assert!(made.committed, "{:?} {:?}", made.result, made.why);
    assert_eq!(
        s.materializer.state("pw:tables/Lists").as_deref(),
        Some("written"),
        "signed in, it wrote its list"
    );
}

/// A table of what each command was answered of its reader's account, by
/// the row's id: the account's id and name, or none.
const SEENS: &str = "module Seens

import capability.{ UserId }

type Seen = Seen { id: String, who: Option<UserId>, name: String }

table Seens
    row Seen
    key id
";

const SEEN_DATA: &str = "module SeenData

import Seens

source SeenData
    holds        Seens
    transactions serializable
    reads        strong
    changes      none
";

/// **A program that writes its reader's account three ways**: by a call of
/// the reader's own (`now`), by a run the platform makes when the clock
/// passes its time (`later` schedules `noted`), and by the platform's
/// settling of a payment the reader began (`settled`).
const BEHALF: &str = "module tables.behalf

import context.{ current_user, account_of, Account }
import clock.{ minutes }
import payments.{ Settled }
import Seens.{ Seen }
import Seens

opaque type InteractionId = String

fn seen(id: String, account: Option<Account>) -> Seen !{} {
    match account {
        Some(account) => Seen { id: id, who: Some(account.id), name: account.name },
        None => Seen { id: id, who: None, name: \"\" },
    }
}

command now(id: String) -> Result<Int, String>
    idempotent_by InteractionId
    invalidates   Who(id)
{
    Seens.put(seen(id, account_of(current_user())))
    Ok(1)
}

command noted(id: String) -> Result<Int, String>
    invoked_by    Clock
    idempotent_by InteractionId
    invalidates   Who(id)
{
    Seens.put(seen(id, account_of(current_user())))
    Ok(1)
}

command later(id: String) -> Result<Int, String> !{ clock.schedule }
    idempotent_by InteractionId
{
    schedule noted(id) after minutes(1)
    Ok(1)
}

command settled(outcome: Settled) -> Result<Int, String>
    invoked_by    Payments
    idempotent_by InteractionId
    invalidates   Who(_)
{
    Seens.put(seen(outcome.order, account_of(current_user())))
    Ok(1)
}

query Who(id: String) -> Option<Seen>
    consistency strong
{
    Seens.get(id)
}
";

/// The program that writes its reader's account, built.
fn built_behalf() -> (tempfile::TempDir, std::path::PathBuf) {
    built(
        &[],
        &[
            ("Seens.pw", SEENS),
            ("SeenData.pw", SEEN_DATA),
            ("behalf.pw", BEHALF),
        ],
    )
}

/// A `Seen` row as `Who` answers it: the account's id and name, or none.
fn seen(id: &str, account: Option<(&str, &str)>) -> Val {
    Val::Option(Some(Box::new(Val::Record(vec![
        ("id".into(), text(id)),
        (
            "who".into(),
            Val::Option(account.map(|(who, _)| Box::new(text(who)))),
        ),
        ("name".into(), text(account.map_or("", |(_, name)| name))),
    ]))))
}

/// **`session`'s reader's account, as the program is answered it three
/// ways**: by a call of theirs (`<tag>-now`), by the run the clock makes
/// on their behalf (`<tag>-later`), and by the settling of a payment they
/// began (`<tag>-paid`); each row as another session reads it.
fn answered_three_ways(s: &Server, session: &str, tag: &str) -> [Val; 3] {
    for (command, id, interaction) in [
        (
            "tables.behalf.now",
            format!("{tag}-now"),
            format!("{tag}-1"),
        ),
        (
            "tables.behalf.later",
            format!("{tag}-later"),
            format!("{tag}-2"),
        ),
    ] {
        let answered = s
            .command_answer(
                command,
                session,
                &[serde_json::json!(id)],
                Some(&interaction),
            )
            .asserted(command);
        assert!(answered.committed, "{command}: {answered:?}");
    }
    s.query_clock.0.advance(2 * 60 * 1000);
    let (ran, _) = s.run_due();
    assert_eq!(ran, 1, "{tag}: its run was due, and ran");
    let settled = s.settle_payment(&payments::Settled {
        intent: format!("pi_{tag}"),
        order: format!("{tag}-paid"),
        owner: s.identity.principals().user_of(session),
        amount: 1,
        status: "succeeded".into(),
    });
    assert!(settled.is_ok(), "{tag}: the payment settled: {settled:?}");
    ["now", "later", "paid"].map(|way| {
        s.answer(
            "tables.behalf.Who",
            "another-reader",
            &[text(&format!("{tag}-{way}"))],
        )
        .asserted("the row, read by another session")
    })
}

/// **A command the platform runs on a reader's behalf is answered the
/// reader's account, as their own call is, and never their session** (the
/// review's B1; the integrator's rulings 1 and 2 of 2026-11-02): a run the
/// clock makes, and a payment's settlement, each in a session the platform
/// opens on the reader's behalf. Until then the stand-in passed for an
/// account: a guest's and a signed-out reader's run handed the program `u-`
/// and their session's cookie as an account's id, which it stored where
/// another session read it, and an account's run the name `""`. Now a
/// guest's and a signed-out reader's are answered none, three ways, and
/// their session is nowhere another reader reads; Ada's are her account,
/// three ways alike.
#[test]
fn a_run_on_a_readers_behalf_is_answered_their_account_and_never_their_session() {
    let (_dir, out) = built_behalf();
    // The guest model: every session its own guest, and none an account.
    let s = Server::from_build(out.clone(), out.clone()).asserted("served");
    let guest = "s-SECRET-guest-0123456789abcdef";
    let rows = answered_three_ways(&s, guest, "g");
    assert_eq!(
        rows,
        [
            seen("g-now", None),
            seen("g-later", None),
            seen("g-paid", None)
        ],
        "a guest has no account, run by the clock or a payment as by itself"
    );
    assert!(!format!("{rows:?}").contains("SECRET"), "{rows:?}");
    // The accounts model: Ada signed in, and a reader signed out.
    let s = Server::from_build(out.clone(), out).asserted("served");
    s.identity.signed_in_for_test("s-ada", "u-ada", "Ada");
    let ada = Some(("u-ada", "Ada"));
    assert_eq!(
        answered_three_ways(&s, "s-ada", "a"),
        [
            seen("a-now", ada),
            seen("a-later", ada),
            seen("a-paid", ada)
        ],
        "Ada's account, id and name, however her command runs"
    );
    let out_session = "s-SECRET-signed-out-0123456789";
    let rows = answered_three_ways(&s, out_session, "o");
    assert_eq!(
        rows,
        [
            seen("o-now", None),
            seen("o-later", None),
            seen("o-paid", None)
        ],
        "a signed-out reader has no account"
    );
    assert!(!format!("{rows:?}").contains("SECRET"), "{rows:?}");
}

/// **A build whose row is of a handle's type is refused where it is served**
/// (the integrator's ruling 3 of 2026-11-02, PW6901 held again by the host):
/// the compiler refuses such a row, and a tables.json that names one all
/// the same, by a stale or forged build, refuses the server's start, the
/// table, the field and the handle named. The neighbour: the build as the
/// compiler wrote it, its row naming its reader by an id, is served.
#[test]
fn a_build_whose_row_names_a_handle_is_refused_where_it_is_served() {
    let (_dir, out) = built_behalf();
    assert!(
        Server::from_build(out.clone(), out.clone()).is_ok(),
        "the build as compiled: its row holds an id"
    );
    let path = out.join("tables.json");
    let written = std::fs::read_to_string(&path).asserted("tables.json");
    for (handle, whose) in [
        ("User<UserId>", "a user's"),
        ("Option<capability.Session<SessionId>>", "a session's"),
        ("List<Organization<OrganizationId>>", "an organization's"),
    ] {
        let mut json: serde_json::Value = serde_json::from_str(&written).asserted("JSON");
        let field = json["tables"]
            .as_array_mut()
            .and_then(|tables| tables.iter_mut().find(|t| t["name"] == "Seens"))
            .and_then(|table| table["fields"].as_array_mut())
            .and_then(|fields| fields.iter_mut().find(|f| f["name"] == "who"));
        assert!(field.is_some(), "Seens' field `who`");
        if let Some(field) = field {
            field["type"] = serde_json::json!(handle);
        }
        std::fs::write(&path, json.to_string()).asserted("tables.json written");
        let refused = Server::from_build(out.clone(), out.clone());
        let why = refused.err().unwrap_or_default();
        assert!(
            why.contains("`Seens`'s field `who`") && why.contains(whose) && why.contains("PW6901"),
            "{handle}: {why}"
        );
    }
}

/// **A program of tables that declares an upload is refused where it is
/// served** (the integrator's note b of 2026-11-02): a table keeps no
/// upload's image, and such a program was served, its images taken and kept
/// nowhere. Refused, the upload named; the neighbour, the same program
/// without it, is served.
#[test]
fn a_program_of_tables_declaring_an_upload_is_refused_where_it_is_served() {
    let upload = "module tables.uploads\n\nupload ListImage\n    route      \
                  \"/uploads/list-image\"\n    serves     \"/images\"\n    \
                  max_bytes  5_000_000\n    types      png, jpeg\n    \
                  max_width  1024\n    max_height 1024\n";
    let mut sources = TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_dir, out) = built(&sources, &[("uploads.pw", upload)]);
    let why = Server::from_build(out.clone(), out)
        .err()
        .unwrap_or_default();
    assert!(why.contains("`ListImage` is an `upload`"), "{why}");
    let (_dir, out) = built(&sources, &[]);
    assert!(Server::from_build(out.clone(), out).is_ok());
}

/// **One layer a program**: a program that imports a hand-written data
/// layer's operation beside its tables is refused where it is served, by
/// name, before anything is served (T1 routes several layers, ADR-0338 5.1).
#[test]
fn a_program_mixing_its_tables_with_a_hand_written_layer_is_refused() {
    let mixed = "module tables.mixed\n\nimport capability.{ Session, SessionId }\n\n\
                 fn viewer(s: Session<SessionId>) -> String !{ session.read }\n    \
                 host \"feed:data/users#viewer-name\"\n\n\
                 session query Viewer(session: Session<SessionId>) -> String\n    \
                 freshness     0.seconds\n    consistency   read_your_writes\n    \
                 cache         private\n    key           session\n{\n    \
                 viewer(session)\n}\n";
    let mut sources = TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_dir, out) = built(&sources, &[("mixed.pw", mixed)]);
    let refused = Server::from_build(out.clone(), out);
    assert!(refused.is_err(), "a mixed program is refused");
    let refused = refused.err().unwrap_or_default();
    assert!(
        refused.contains("`feed:data/users#viewer-name`, a hand-written data layer's"),
        "{refused}"
    );
    // The neighbour: the same program without it is served.
    let (_dir, out) = built(&sources, &[]);
    assert!(Server::from_build(out.clone(), out).is_ok());
}
