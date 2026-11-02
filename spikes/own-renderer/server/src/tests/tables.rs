//! **A program's declared tables, served** (track `data-path`; ADR-0338's D2
//! routing): `examples/tables`, a program of tables, queries and commands
//! with no line of Rust of its own, built as `pw build` builds it and served
//! by the development server through the generic memory adapter: every
//! call through the table gate, each component's host map holding its own
//! table functions alone, a command's writes staged in its own transaction
//! and published as the platform's transaction's last step, with its
//! records, or not at all.

use super::*;

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
        server: Server::from_build(out.clone(), out).expect("served"),
        _dir: dir,
    }
}

/// The lists app (`examples/tables/app.pw`), served.
fn served_tables() -> Served {
    served_program("app.pw")
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
    let Val::List(rows) = value else {
        panic!("a list: {value:?}")
    };
    rows.iter()
        .map(|row| match val_at(row, &["id"]) {
            Some(Val::String(id)) => id.clone(),
            other => panic!("a row's id: {other:?}"),
        })
        .collect()
}

/// A command answered, and committed.
fn committed(s: &Server, command: &str, session: &str, args: &[Val]) -> Answered {
    let answered = s
        .command_answered(command, session, args, None)
        .unwrap_or_else(|why| panic!("{command} is answered: {why}"));
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
        s.answer(EVERY_LIST, "a", &[]).expect("read"),
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
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
        ["chores", "groceries"]
    );
    match s.answer(LIST_AT, "b", &[text("food")]).expect("read") {
        Val::Option(Some(found)) => assert_eq!(val_at(&found, &["id"]), Some(&text("groceries"))),
        other => panic!("the list at its slug: {other:?}"),
    }
    assert_eq!(
        ids(&s.answer(ITEMS_OF, "b", &[text("groceries")]).expect("read")),
        ["eggs", "milk"]
    );
    assert_eq!(
        ids(&s.answer(OPEN_ITEMS, "b", &[]).expect("read")),
        ["sweep", "milk"]
    );
    assert_eq!(
        ids(&s.answer(EVERY_ITEM, "b", &[]).expect("read")),
        ["sweep", "eggs", "milk"]
    );
    committed(&s, DROP_ITEM, "a", &[text("groceries"), text("milk")]);
    assert_eq!(
        ids(&s.answer(ITEMS_OF, "b", &[text("groceries")]).expect("read")),
        ["eggs"]
    );
    committed(&s, CLEAR_LIST, "a", &[text("groceries")]);
    assert_eq!(
        ids(&s.answer(EVERY_ITEM, "b", &[]).expect("read")),
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
        s.answer(query, "a", &args).expect("read");
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
        *into.lock().expect("seen") = Some(server.answer(EVERY_LIST, "b", &[]).expect("read"));
    }));
    committed(&s, MAKE_LIST, "a", &[list("groceries", "food", 20)]);
    assert_eq!(
        seen.lock().expect("seen").clone(),
        Some(Val::List(vec![])),
        "staged, it was read by no other query"
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
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
        .expect("answered");
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
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
        ["groceries"]
    );
    assert_eq!(
        s.answer(EVERY_ITEM, "b", &[]).expect("read"),
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
        .expect("answered");
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
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
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
        s.recalled_interaction("a:make:2").expect("recall"),
        None,
        "no record of a commit no reader sees"
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
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
        .expect("answered");
    assert!(kept.committed);
    assert!(
        s.recalled_interaction("a:make:3")
            .expect("recall")
            .is_some()
    );
    assert_eq!(
        ids(&s.answer(EVERY_LIST, "b", &[]).expect("read")),
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
        .expect("answered");
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
        .expect("answered");
    assert!(made.committed, "{:?} {:?}", made.result, made.why);
    assert_eq!(
        s.materializer.state("pw:tables/Lists").as_deref(),
        Some("written"),
        "signed in, it wrote its list"
    );
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
    let refused = Server::from_build(out.clone(), out)
        .err()
        .expect("a mixed program is refused");
    assert!(
        refused.contains("`feed:data/users#viewer-name`, a hand-written data layer's"),
        "{refused}"
    );
    // The neighbour: the same program without it is served.
    let (_dir, out) = built(&sources, &[]);
    assert!(Server::from_build(out.clone(), out).is_ok());
}
