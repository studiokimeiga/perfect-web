//! **An app's data, declared** (track `tables`, ADR-0338's D1): a table's
//! clauses, the functions it gives its module, their types and effects, its
//! refusals each beside its control, `tables.json` for the fixture program
//! (`examples/tables/`), and the schema's diff against the one released.
//!
//! Recorded by `just e14-tables` in `docs/evidence/E14/tables.txt`.

use pw_core::check::{Unit, check_sources, check_sources_with};
use pw_core::diagnostics::{Diagnostic, Severity};
use pw_core::tables::{Schema, Step};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The platform's packages: `database.read` and `List` are theirs.
fn platform() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root().join(dir))
            .expect("packages")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                format!("{dir}/{}", p.file_name().unwrap().to_string_lossy()),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out
}

/// A program: the platform and `files`, each `(name, source)`.
fn program(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = platform();
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.to_string())));
    out
}

/// The diagnostics of every file `files` names, held to `released`.
fn diagnostics_with(files: &[(&str, &str)], released: Option<&Schema>) -> Vec<Diagnostic> {
    let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
    check_sources_with(&program(files), released)
        .into_iter()
        .filter(|(n, _)| names.contains(&n.as_str()))
        .flat_map(|(_, d)| d)
        .collect()
}

fn diagnostics(files: &[(&str, &str)]) -> Vec<Diagnostic> {
    diagnostics_with(files, None)
}

fn codes(ds: &[Diagnostic]) -> Vec<&str> {
    ds.iter().map(|d| d.code).collect()
}

fn units(files: &[(String, String)]) -> Vec<Unit> {
    files
        .iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path: path.clone(),
                src: src.clone(),
                hir: pw_core::lower::lower_file(src, &parsed.green),
            }
        })
        .collect()
}

/// A well-formed table, held by a source, the control of every refusal below.
const TODOS: &str = "module Todos\n\
\n\
opaque type TodoId = String\n\
opaque type ListId = String\n\
\n\
type Todo = Todo { list: ListId, id: TodoId, title: String, done: Bool, added: Int }\n\
\n\
table Todos\n    row     Todo\n    key     list, id\n    order   added\n    index   by_done on done\n    bounded 500\n\
\n\
source TodoData\n    holds        Todos\n    transactions serializable\n    reads        strong\n    changes      none\n";

/// `TODOS`, with `from` written `to`.
fn todos(from: &str, to: &str) -> String {
    assert!(TODOS.contains(from), "{from:?} is not in the control");
    TODOS.replacen(from, to, 1)
}

/// The one table rule `src` breaks, as `module Todos`: its code and message.
fn refused(src: &str) -> Vec<(String, String)> {
    diagnostics(&[("Todos.pw", src)])
        .into_iter()
        .map(|d| (d.code.to_string(), d.message))
        .collect()
}

#[track_caller]
fn assert_refused(src: &str, code: &str, says: &str) {
    let found = refused(src);
    assert!(
        found.iter().any(|(c, m)| c == code && m.contains(says)),
        "expected {code} saying {says:?}, got {found:#?}"
    );
}

// --- each clause's words -----------------------------------------------------

#[test]
fn the_control_checks_clean_and_reads_each_clause() {
    assert!(refused(TODOS).is_empty(), "{:#?}", refused(TODOS));
    let units = units(&[("Todos.pw".to_string(), TODOS.to_string())]);
    let t = &pw_core::tables::declared(&units[0].hir)[0];
    assert_eq!(t.name, "Todos");
    assert_eq!(t.row.as_ref().map(|r| r.value.as_str()), Some("Todo"));
    assert_eq!(
        t.key.as_ref().map(|k| k.value.clone()),
        Some(vec!["list".to_string(), "id".to_string()])
    );
    assert_eq!(t.order.as_ref().map(|o| o.value.as_str()), Some("added"));
    assert_eq!(t.indexes.len(), 1);
    assert_eq!(t.indexes[0].value.name, "by_done");
    assert_eq!(t.indexes[0].value.fields, ["done"]);
    assert!(!t.indexes[0].value.unique);
    assert_eq!(t.bounded.as_ref().map(|b| b.value), Some(500));
    assert!(t.unread.is_empty());
}

#[test]
fn a_clause_on_another_declaration_is_no_tables() {
    // `row` and `index` belong to a table (PW5105), and a query's `key` to
    // its parameters still (PW0335).
    let src = "module Todos\nopaque type TodoId = String\nquery Q(id: TodoId) -> Int\n    order added\n    key   nothing\n{\n    1\n}\n";
    let diagnostics = diagnostics(&[("Todos.pw", src)]);
    let found = codes(&diagnostics);
    assert!(found.contains(&"PW5105"), "{found:?}");
    assert!(found.contains(&"PW0335"), "{found:?}");
}

#[test]
fn a_bound_is_a_count_and_each_clause_its_grammar() {
    assert_refused(&todos("bounded 500", "bounded 0"), "PW0335", "bounded");
    assert_refused(
        &todos("index   by_done on done", "index   by_done"),
        "PW0361",
        "`<name> on <fields>`",
    );
    assert_refused(
        &todos("key     list, id", "key     list, list"),
        "PW0361",
        "names `list` twice",
    );
    assert_refused(
        &todos(
            "    bounded 500\n",
            "    bounded 500\n    refers list Lists\n",
        ),
        "PW0361",
        "`<field> to <table>`",
    );
    assert_refused(
        &todos(
            "    bounded 500\n",
            "    bounded 500\n    migrate title moved\n",
        ),
        "PW0361",
        "renamed from",
    );
}

// --- the functions it gives --------------------------------------------------

/// Each generated function of `Todos`, with its signature as the checker
/// resolves it and the effects its row declares.
fn functions_of(src: &str) -> Vec<(String, String, Vec<String>, String)> {
    let files = program(&[("Todos.pw", src)]);
    let units = units(&files);
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let unit = units.len() - 1;
    let hir = &units[unit].hir;
    hir.generated
        .keys()
        .map(|id| {
            let d = hir.decl(*id);
            let Some(sig) = sigs.by_def(pw_core::resolve::DefId { unit, decl: id.0 }) else {
                assert_eq!(d.kind, pw_core::hir::DeclKind::Type);
                return (d.name.clone(), String::new(), Vec::new(), String::new());
            };
            let shown = |t: Option<&pw_core::resolved::TypeResolution>| {
                t.map_or("?".to_string(), ToString::to_string)
            };
            (
                d.name.clone(),
                format!(
                    "({}) -> {}",
                    sig.params
                        .iter()
                        .map(|p| shown(p.as_ref()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    shown(sig.returns.as_ref())
                ),
                d.declared_effects
                    .iter()
                    .flatten()
                    .map(|e| e.written.clone())
                    .collect(),
                d.policy("host")
                    .map(|p| p.value.clone())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn a_table_gives_each_read_and_write_with_its_type_and_effect() {
    let got = functions_of(TODOS);
    let read = vec!["database.read<Todos>".to_string()];
    let write = vec!["database.write<Todos>".to_string()];
    let none = Vec::new();
    let want: Vec<(&str, &str, &Vec<String>, &str)> = vec![
        ("TodosListId", "", &none, ""),
        (
            "get",
            "(ListId, TodoId) -> Option<Todo>",
            &read,
            "\"pw:tables/todos#get\"",
        ),
        (
            "get_many",
            "(List<TodosListId>) -> List<Todo>",
            &read,
            "\"pw:tables/todos#get-many\"",
        ),
        (
            "of_list",
            "(ListId) -> List<Todo>",
            &read,
            "\"pw:tables/todos#of-list\"",
        ),
        (
            "of_list_many",
            "(List<ListId>) -> List<Todo>",
            &read,
            "\"pw:tables/todos#of-list-many\"",
        ),
        (
            "by_done",
            "(Bool) -> List<Todo>",
            &read,
            "\"pw:tables/todos#by-done\"",
        ),
        (
            "by_done_many",
            "(List<Bool>) -> List<Todo>",
            &read,
            "\"pw:tables/todos#by-done-many\"",
        ),
        ("all", "() -> List<Todo>", &read, "\"pw:tables/todos#all\""),
        (
            "insert",
            "(Todo) -> Result<(), Todo>",
            &write,
            "\"pw:tables/todos#insert\"",
        ),
        ("put", "(Todo) -> ()", &write, "\"pw:tables/todos#put\""),
        (
            "delete",
            "(ListId, TodoId) -> ()",
            &write,
            "\"pw:tables/todos#delete\"",
        ),
        (
            "delete_of_list",
            "(ListId) -> ()",
            &write,
            "\"pw:tables/todos#delete-of-list\"",
        ),
    ];
    let names: Vec<&str> = got.iter().map(|(n, ..)| n.as_str()).collect();
    assert_eq!(
        names,
        want.iter().map(|(n, ..)| *n).collect::<Vec<_>>(),
        "{got:#?}"
    );
    for ((name, sig, effects, host), (_, want_sig, want_effects, want_host)) in
        got.iter().zip(&want)
    {
        if name.starts_with("Todos") {
            // The record a set form's list holds for the two-field key.
            continue;
        }
        assert_eq!(sig, want_sig, "{name}");
        assert_eq!(effects, *want_effects, "{name}");
        assert_eq!(host, want_host, "{name}");
    }
}

#[test]
fn every_keyed_read_has_its_set_form() {
    // ADR-0338, 1.2: the 4c note's `set` pairing, generated.
    let src = todos(
        "    bounded 500\n",
        "    bounded 500\n    unique  by_title_done on title, done\n",
    );
    let got = functions_of(&src);
    let names: Vec<&str> = got.iter().map(|(n, ..)| n.as_str()).collect();
    for (keyed, set) in [
        ("get", "get_many"),
        ("of_list", "of_list_many"),
        ("by_done", "by_done_many"),
        ("by_title_done", "by_title_done_many"),
    ] {
        assert!(names.contains(&keyed), "{keyed}: {names:?}");
        assert!(names.contains(&set), "{set}: {names:?}");
    }
    let sig = |n: &str| {
        got.iter()
            .find(|(m, ..)| m == n)
            .map(|(_, s, ..)| s.clone())
    };
    // A unique index answers one row or none; its set form a list.
    assert_eq!(
        sig("by_title_done").as_deref(),
        Some("(String, Bool) -> Option<Todo>")
    );
    assert_eq!(
        sig("by_title_done_many").as_deref(),
        Some("(List<TodosTitleDone>) -> List<Todo>")
    );
    // And a table with a unique index refuses a `put` that takes its value.
    assert_eq!(sig("put").as_deref(), Some("(Todo) -> Result<(), Todo>"));
}

#[test]
fn a_bound_gives_all_and_no_bound_none() {
    let names =
        |src: &str| -> Vec<String> { functions_of(src).into_iter().map(|(n, ..)| n).collect() };
    assert!(names(TODOS).contains(&"all".to_string()));
    assert!(!names(&todos("    bounded 500\n", "")).contains(&"all".to_string()));
}

#[test]
fn a_three_field_key_gives_a_read_by_each_proper_prefix() {
    let src = todos("key     list, id", "key     list, done, id");
    let names: Vec<String> = functions_of(&src).into_iter().map(|(n, ..)| n).collect();
    for n in [
        "of_list",
        "of_list_many",
        "of_list_done",
        "of_list_done_many",
        "delete_of_list",
        "delete_of_list_done",
    ] {
        assert!(names.contains(&n.to_string()), "{n}: {names:?}");
    }
    assert!(!names.contains(&"of_list_done_id".to_string()), "{names:?}");
}

#[test]
fn its_imports_are_the_platforms_and_its_program_builds() {
    // The fixture program: each query and command a component, each import
    // a table's primitive, owned by the platform.
    let mut files = platform();
    for f in ["Lists.pw", "Items.pw", "ListData.pw", "app.pw"] {
        files.push((
            f.to_string(),
            std::fs::read_to_string(root().join("examples/tables").join(f)).expect("read"),
        ));
    }
    let build = pw_core::build::build(&units(&files)).expect("the fixture builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let built: Vec<&str> = build
        .components
        .iter()
        .filter(|(_, b)| matches!(b, pw_core::backend::component::Built::Component { .. }))
        .map(|(id, _)| id.as_str())
        .collect();
    for c in [
        "tables.app.EveryList",
        "tables.app.ListAt",
        "tables.app.ItemsOf",
        "tables.app.OpenItems",
        "tables.app.EveryItem",
        "tables.app.make_list",
        "tables.app.keep_item",
        "tables.app.drop_item",
        "tables.app.clear_list",
    ] {
        assert!(built.contains(&c), "{c} is not built: {built:?}");
    }
    let imports: Vec<_> = build
        .contracts
        .iter()
        .flat_map(|c| c.imports.iter())
        .filter(|i| i.interface.starts_with("pw:tables/"))
        .collect();
    assert!(imports.len() >= 9, "{imports:#?}");
    for i in &imports {
        assert_eq!(i.owner, pw_core::contract::Ownership::Platform, "{i:?}");
    }
    // A case that carries nothing is `_` in WIT, which has no empty tuple.
    assert!(
        build
            .wit
            .contains("insert: func(arg0: lists-todo-list) -> result<_, lists-todo-list>;"),
        "{}",
        build.wit
    );
    assert!(!build.wit.contains("tuple<>"), "{}", build.wit);
}

// --- tables.json -------------------------------------------------------------

#[test]
fn tables_json_states_each_table_its_functions_and_their_order() {
    let mut files = platform();
    for f in ["Lists.pw", "Items.pw", "ListData.pw", "app.pw"] {
        files.push((
            f.to_string(),
            std::fs::read_to_string(root().join("examples/tables").join(f)).expect("read"),
        ));
    }
    let lock = Schema::read(
        &std::fs::read_to_string(root().join("examples/tables/schema.lock")).expect("the lock"),
    )
    .expect("a lock");
    let build = pw_core::build::build_with(&units(&files), Some(&lock)).expect("builds");
    let json = serde_json::to_value(&build.tables).expect("json");
    // Its schema is the one released: nothing to migrate.
    assert_eq!(json["hash"], lock.hash);
    assert_eq!(json["migration"]["from"], lock.hash);
    assert_eq!(json["migration"]["steps"], serde_json::json!([]));
    assert_eq!(build.schema, lock);
    let tables = json["tables"].as_array().expect("tables");
    assert_eq!(tables.len(), 2);
    let items = &tables[0];
    assert_eq!(items["name"], "Items");
    assert_eq!(items["interface"], "pw:tables/items");
    assert_eq!(items["source"], "ListData");
    assert_eq!(items["row"], "Item");
    assert_eq!(items["key"], serde_json::json!(["list", "id"]));
    assert_eq!(items["order"], "added");
    assert_eq!(
        items["references"],
        serde_json::json!([{"field": "list", "table": "Lists"}])
    );
    // A refinement is stored with its bound.
    let count = items["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "count")
        .expect("count");
    assert_eq!(count["type"], "Count");
    assert_eq!(count["stored"], serde_json::json!({"int": {"at_least": 1}}));
    let note = items["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "note")
        .expect("note");
    assert_eq!(
        note["stored"],
        serde_json::json!({"option": {"string": {}}})
    );
    let function = |name: &str| {
        items["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name}"))
    };
    let of_list = function("of_list");
    assert_eq!(of_list["import"], "pw:tables/items#of-list");
    assert_eq!(of_list["primitive"], "prefix");
    assert_eq!(of_list["effect"], "read");
    assert_eq!(of_list["by"], serde_json::json!(["list"]));
    // A prefix's rows in `order`, then by the key.
    assert_eq!(
        of_list["sorted_by"],
        serde_json::json!(["added", "list", "id"])
    );
    // A set form's grouped by what it was given, first.
    let many = function("of_list_many");
    assert_eq!(many["primitive"], "prefix-many");
    assert_eq!(
        many["sorted_by"],
        serde_json::json!(["list", "added", "id"])
    );
    assert_eq!(
        function("by_done_many")["sorted_by"],
        serde_json::json!(["done", "added", "list", "id"])
    );
    // One row or none is in no order; a write answers no rows.
    assert!(function("get").get("sorted_by").is_none());
    assert_eq!(function("put")["effect"], "write");
    assert_eq!(function("delete_of_list")["primitive"], "delete-prefix");
    let lists = &tables[1];
    assert_eq!(lists["bounded"], 100);
    let by_slug = lists["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "by_slug")
        .cloned()
        .expect("by_slug");
    assert_eq!(by_slug["returns"], "Option<TodoList>");
    assert!(
        by_slug.get("sorted_by").is_none(),
        "a unique index's read is one row"
    );
    let slug = lists["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "slug")
        .expect("slug");
    assert_eq!(
        slug["stored"],
        serde_json::json!({"string": {"max_length": 64}})
    );
}

// --- the refusals, each beside its control ----------------------------------

#[test]
fn pw0361_a_table_is_declared_in_the_module_of_its_name() {
    assert_refused(
        &TODOS.replace("module Todos", "module todo.data"),
        "PW0361",
        "a table is declared in the module of its name",
    );
}

#[test]
fn pw0361_its_row_is_a_record_declared_beside_it() {
    assert_refused(
        &todos("row     Todo", "row     Nothing"),
        "PW0361",
        "names no record type declared",
    );
    assert_refused(
        &todos("row     Todo", "row     TodoId"),
        "PW0361",
        "no record",
    );
    assert_refused(
        &todos("    row     Todo\n", ""),
        "PW0361",
        "states no `row`",
    );
    // Declared in another module, and imported.
    let other = "module todo.types\nopaque type TodoId = String\ntype Todo = Todo { id: TodoId }\n";
    let table = "module Todos\nimport todo.types.{ Todo }\ntable Todos\n    row Todo\n    key id\n";
    let found = diagnostics(&[("types.pw", other), ("Todos.pw", table)]);
    assert!(
        found
            .iter()
            .any(|d| d.code == "PW0361" && d.message.contains("declared in `todo.types`")),
        "{found:#?}"
    );
}

#[test]
fn pw0361_its_key_names_its_rows_fields_of_types_every_adapter_keys() {
    assert_refused(
        &todos("    key     list, id\n", ""),
        "PW0361",
        "states no `key`",
    );
    assert_refused(
        &todos("key     list, id", "key     list, code"),
        "PW0361",
        "`key` names `code`, which is no field of `Todo`",
    );
    // A list, a number with a fraction, an option, a record, a case that
    // carries a value: none is a key.
    for (ty, why) in [
        ("List<String>", "a `List`"),
        ("Float", "a `Float`"),
        ("Option<String>", "an `Option`"),
        ("Box", "a record"),
        ("Shape", "carries a value"),
    ] {
        let src = todos("title: String", &format!("title: {ty}"))
            .replace("key     list, id", "key     list, title")
            .replace(
                "type Todo = ",
                "type Box = Box { inner: Int }\ntype Shape = | Square(Int) | Dot\ntype Todo = ",
            );
        assert_refused(&src, "PW0361", why);
    }
    // The control: an opaque type over a string, a number, a flag, and cases
    // that carry nothing.
    for ty in ["TodoId", "Int", "Bool", "Light"] {
        let src = todos("title: String", &format!("title: {ty}"))
            .replace("key     list, id", "key     list, title")
            .replace("type Todo = ", "type Light = | Red | Green\ntype Todo = ");
        assert!(refused(&src).is_empty(), "{ty}: {:#?}", refused(&src));
    }
}

#[test]
fn pw0361_its_order_and_indexes_name_its_rows_fields() {
    assert_refused(
        &todos("order   added", "order   created"),
        "PW0361",
        "`order` names `created`",
    );
    assert_refused(
        &todos("index   by_done on done", "index   by_done on finished"),
        "PW0361",
        "`index` names `finished`, which is no field of `Todo`",
    );
    assert_refused(
        &todos("index   by_done on done", "index   by_done on title, gone"),
        "PW0361",
        "`index` names `gone`",
    );
    assert_refused(
        &todos("index   by_done on done", "index   done on done"),
        "PW0361",
        "named as no index read is",
    );
    assert_refused(
        &todos(
            "    bounded 500\n",
            "    bounded 500\n    unique  by_done on title\n",
        ),
        "PW0361",
        "`by_done` is the read of two",
    );
    assert_refused(
        &todos(
            "    bounded 500\n",
            "    bounded 500\n    unique  by_done_many on title\n",
        ),
        "PW0361",
        "`by_done_many` is the read of two",
    );
    // An index on a field of a type no adapter keys.
    assert_refused(
        &todos("index   by_done on done", "index   by_done on done, tags")
            .replace("added: Int }", "added: Int, tags: List<String> }"),
        "PW0361",
        "`index` names `tags`, of `List<String>`",
    );
}

const USERS: &str = "module Users\nopaque type UserId = String\ntype User = User { id: UserId, name: String }\ntable Users\n    row User\n    key id\n";

#[test]
fn pw0361_a_reference_names_a_table_keyed_by_its_fields_type() {
    let with = |field: &str, refers: &str| {
        todos("added: Int }", &format!("added: Int, {field} }}"))
            .replace(
                "module Todos\n",
                "module Todos\nimport Users.{ UserId }\nimport Users\n",
            )
            .replace(
                "    bounded 500\n",
                &format!("    bounded 500\n    refers  {refers}\n"),
            )
    };
    let check = |src: &str| diagnostics(&[("Users.pw", USERS), ("Todos.pw", src)]);
    let says = |src: &str, code: &str, says: &str| {
        let found = check(src);
        assert!(
            found
                .iter()
                .any(|d| d.code == code && d.message.contains(says)),
            "expected {code} {says:?}: {found:#?}"
        );
    };
    says(
        &with("owner: Option<UserId>", "owner to Nobody"),
        "PW0361",
        "names no table this module sees",
    );
    says(
        &with("owner: Option<UserId>", "boss to Users"),
        "PW0361",
        "`boss`, which is no field of `Todo`",
    );
    says(
        &with("owner: Option<String>", "owner to Users"),
        "PW0361",
        "`owner` is `Option<String>`, and `Users` is keyed by `UserId`",
    );
    // The control: the key's type, or none.
    let field = "owner: Option<UserId>";
    let found = check(
        &with(field, "owner to Users").replace("holds        Todos", "holds        Todos, Users"),
    );
    assert!(found.is_empty(), "{field}: {found:#?}");
    // A table keyed by two fields is no reference's.
    let two = "module Users\nopaque type UserId = String\ntype User = User { org: String, id: UserId }\ntable Users\n    row User\n    key org, id\n";
    let found = diagnostics(&[
        ("Users.pw", two),
        ("Todos.pw", &with("owner: Option<UserId>", "owner to Users")),
    ]);
    assert!(
        found
            .iter()
            .any(|d| d.code == "PW0361" && d.message.contains("keyed by 2 fields")),
        "{found:#?}"
    );
}

#[test]
fn pw0362_its_rows_are_reached_only_through_its_functions() {
    // A binding by hand: a read, a write, and one of the table primitives.
    for (binding, says) in [
        (
            "fn count() -> Int !{ database.read<Todos> }\n    host \"todo:data/todos#count\"\n",
            "`count` reads `Todos` through `host \"todo:data/todos#count\"`, and `Todos` is a table",
        ),
        (
            "fn wipe() -> () !{ database.write<Todos> }\n    host \"todo:data/todos#wipe\"\n",
            "`wipe` writes `Todos`",
        ),
        (
            "fn count() -> Int !{}\n    host \"pw:tables/todos#count\"\n",
            "binds `pw:tables/todos#count` by hand",
        ),
    ] {
        assert_refused(&format!("{TODOS}\n{binding}"), "PW0362", says);
    }
    // A read the declaration does not give, from another module.
    let app = "module todo.app\nimport Todos.{ Todo, TodoId }\nimport Todos\nquery Q(id: TodoId) -> List<Todo>\n{\n    Todos.by_title(\"x\")\n}\n";
    let found = diagnostics(&[("Todos.pw", TODOS), ("app.pw", app)]);
    assert!(
        found.iter().any(|d| d.code == "PW0362"
            && d.message
                .contains("`Todos.by_title` is no read or write `Todos` gives")),
        "{found:#?}"
    );
    // The control: a function of the program's own over the reads it gives,
    // and a call of one it gives.
    let composed = format!(
        "{TODOS}\nfn open() -> Int !{{ database.read<Todos> }} {{\n    List.length(Todos.by_done(false))\n}}\n"
    )
    .replace("module Todos\n", "module Todos\nimport List\n");
    assert!(refused(&composed).is_empty(), "{:#?}", refused(&composed));
    let given = app.replace("Todos.by_title(\"x\")", "Todos.by_done(true)");
    assert!(diagnostics(&[("Todos.pw", TODOS), ("app.pw", &given)]).is_empty());
}

#[test]
fn pw0364_all_reads_only_a_bounded_table() {
    let unbounded = todos("    bounded 500\n", "");
    let qualified = format!(
        "{unbounded}\nfn every() -> List<Todo> !{{ database.read<Todos> }} {{\n    Todos.all()\n}}\n"
    );
    assert_refused(
        &qualified,
        "PW0364",
        "`Todos.all()` reads every row of `Todos`, which states no `bounded`",
    );
    let bare = format!(
        "{unbounded}\nfn every() -> List<Todo> !{{ database.read<Todos> }} {{\n    all()\n}}\n"
    );
    assert_refused(&bare, "PW0364", "`all()` reads every row");
    // The control: the same calls, of the bounded table.
    for src in [
        format!(
            "{TODOS}\nfn every() -> List<Todo> !{{ database.read<Todos> }} {{\n    Todos.all()\n}}\n"
        ),
        format!(
            "{TODOS}\nfn every() -> List<Todo> !{{ database.read<Todos> }} {{\n    all()\n}}\n"
        ),
    ] {
        assert!(refused(&src).is_empty(), "{:#?}", refused(&src));
    }
}

#[test]
fn pw0365_every_reference_across_sources_is_warned_and_builds() {
    let users = format!(
        "{USERS}\nsource UserData\n    holds        Users\n    transactions serializable\n    reads        strong\n    changes      none\n"
    );
    let with = |field: &str| {
        todos("added: Int }", &format!("added: Int, {field} }}"))
            .replace(
                "module Todos\n",
                "module Todos\nimport Users.{ UserId }\nimport Users\n",
            )
            .replace(
                "    bounded 500\n",
                "    bounded 500\n    refers  owner to Users\n",
            )
    };
    for field in ["owner: UserId", "owner: Option<UserId>"] {
        let todo = with(field);
        let found = diagnostics(&[("Users.pw", &users), ("Todos.pw", &todo)]);
        let refusal = found
            .iter()
            .find(|d| d.code == "PW0365")
            .unwrap_or_else(|| panic!("{found:#?}"));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(refusal.severity, Severity::Warning);
        assert!(
            refusal.message.contains(
                "`Todos.owner` refers to `Users` in `UserData`, and `Todos` is in `TodoData`"
            ),
            "{refusal:#?}"
        );
        let files = program(&[("Users.pw", &users), ("Todos.pw", &todo)]);
        pw_core::build::build(&units(&files)).expect("a warning permits a build");
    }
    // The control: both tables in one source.
    let one_source =
        with("owner: UserId").replace("holds        Todos", "holds        Todos, Users");
    let found = diagnostics(&[("Users.pw", USERS), ("Todos.pw", &one_source)]);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn pw0361_a_source_name_cannot_hide_another_database() {
    let users = format!(
        "{USERS}\nsource Data\n    holds Users\n    transactions serializable\n    reads strong\n    changes none\n"
    );
    let todo = todos("added: Int }", "added: Int, owner: UserId }")
        .replace(
            "module Todos\n",
            "module Todos\nimport Users.{ UserId }\nimport Users\n",
        )
        .replace(
            "    bounded 500\n",
            "    bounded 500\n    refers owner to Users\n",
        );
    let same_name = todo.replace("source TodoData", "source Data");
    let found = diagnostics(&[("Users.pw", &users), ("Todos.pw", &same_name)]);
    assert!(found.iter().any(|d| d.code == "PW0361" && d.message.contains("`Data` names 2 source declarations")), "{found:#?}");
    // A target's ambiguous source matters even when this table's own source
    // has a distinct name.
    let other = "module Other\ntype Other = Other { id: String }\nsource Data\n    holds Other\n    transactions serializable\n    reads strong\n    changes none\n";
    let found = diagnostics(&[
        ("Users.pw", &users),
        ("Todos.pw", &todo),
        ("Other.pw", other),
    ]);
    assert!(found.iter().any(|d| d.code == "PW0361" && d.message.contains("`Data` names 2 source declarations")), "{found:#?}");
    // Distinct source names permit the cross-source relation, warned.
    let found = diagnostics(&[("Users.pw", &users), ("Todos.pw", &todo)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        (found[0].code, found[0].severity),
        ("PW0365", Severity::Warning)
    );
}

#[test]
fn pw0361_generated_composite_records_have_one_shape_each() {
    let src = "module Tuples\ntype Row = Row { a_b: String, c: Int, a: String, b_c: Bool }\ntable Tuples\n    row Row\n    key a_b, c\n    index by_other on a, b_c\n";
    let found = diagnostics(&[("Tuples.pw", src)]);
    assert!(
        found.iter().any(|d| d.code == "PW0361"
            && d.message
                .contains("`TuplesABC` would hold two different field tuples")),
        "{found:#?}"
    );
    let control = src
        .replace("a: String", "owner: String")
        .replace("on a, b_c", "on owner, b_c");
    assert!(diagnostics(&[("Tuples.pw", &control)]).is_empty());
}

#[test]
fn pw0361_two_tables_cannot_emit_the_same_interface() {
    let first = "module Todo_Items\ntype Row = Row { id: String }\ntable Todo_Items\n    row Row\n    key id\n";
    let second = first.replace("Todo_Items", "TodoItems");
    let found = diagnostics(&[("Todo_Items.pw", first), ("TodoItems.pw", &second)]);
    assert!(
        found.iter().any(|d| d.code == "PW0361"
            && d.message
                .contains("emit the same interface `pw:tables/todo-items`")),
        "{found:#?}"
    );
    let distinct = first.replace("Todo_Items", "ArchivedItems");
    assert!(diagnostics(&[("Todo_Items.pw", first), ("ArchivedItems.pw", &distinct)]).is_empty());
}

#[test]
fn pw0361_two_functions_cannot_emit_the_same_import() {
    let src = todos(
        "    bounded 500\n",
        "    bounded 500\n    index by_title_done on title\n    index by_title__done on added\n",
    );
    let found = diagnostics(&[("Todos.pw", &src)]);
    assert!(
        found.iter().any(|d| d.code == "PW0361"
            && d.message
                .contains("emit the same import `pw:tables/todos#by-title-done`")),
        "{found:#?}"
    );
    let distinct = src.replace("by_title__done", "by_finished");
    assert!(diagnostics(&[("Todos.pw", &distinct)]).is_empty());
}

#[test]
fn generic_fields_project_their_arguments_recursively_and_a_change_is_destructive() {
    use pw_core::tables::{Stored, StoredField};
    let boxed = todos("added: Int }", "added: Int, payload: Box<Int> }").replace(
        "\ntable Todos",
        "\ntype Box<T> = Box { value: Option<List<T>> }\n\ntable Todos",
    );
    let lock = released(&boxed);
    let payload = |lock: &Schema| {
        lock.tables[0]
            .fields
            .iter()
            .find(|f| f.name == "payload")
            .unwrap()
            .stored
            .clone()
    };
    let int = Stored::Int {
        at_least: None,
        at_most: None,
    };
    let wrap = |stored| {
        Stored::Record(vec![StoredField {
            name: "value".into(),
            stored: Stored::Option(Box::new(Stored::List(Box::new(stored)))),
        }])
    };
    assert_eq!(payload(&lock), wrap(int.clone()));
    // Repeated generic definitions in a finite type are not recursion.
    let nested = boxed.replace("payload: Box<Int>", "payload: Box<Box<Int>>");
    assert_eq!(payload(&released(&nested)), wrap(wrap(int)));
    let changed = boxed.replace("payload: Box<Int>", "payload: Box<String>");
    assert_unstated(&boxed, &changed, "`Todos.payload` was released");
    // Actual recursive definitions retain an explicit recursive projection.
    let recursive = boxed.replace(
        "type Box<T> = Box { value: Option<List<T>> }",
        "type Box<T> = Box { value: T, next: Option<Box<T>> }",
    );
    let stored = payload(&released(&recursive));
    let Stored::Record(fields) = stored else {
        panic!("{stored:#?}")
    };
    assert_eq!(
        fields[0].stored,
        Stored::Int {
            at_least: None,
            at_most: None
        }
    );
    // A recurrence keeps its declaration's symbolic arguments, with the
    // concrete argument already projected in the enclosing field.
    assert_eq!(
        fields[1].stored,
        Stored::Option(Box::new(Stored::Recursive("Box<T>".into())))
    );
}

// --- the schema's diff -------------------------------------------------------

/// The schema `src` declares, as `pw build` writes `schema.lock`.
fn released(src: &str) -> Schema {
    let files = program(&[("Todos.pw", src)]);
    let build = pw_core::build::build(&units(&files)).expect("the release builds");
    Schema::read(&build.schema.written()).expect("a lock reads back")
}

/// The steps from `before`, released, to `after`, and the refusals.
fn diff(before: &str, after: &str) -> (Vec<Step>, Vec<(String, String)>) {
    let lock = released(before);
    let files = program(&[("Todos.pw", after)]);
    let units = units(&files);
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let (plan, refused) = pw_core::tables::held_to(&hirs, &ws, &sigs, Some(&lock));
    (
        plan.steps,
        refused
            .into_iter()
            .map(|(_, d)| (d.code.to_string(), d.message))
            .collect(),
    )
}

#[track_caller]
fn assert_unstated(before: &str, after: &str, says: &str) {
    let (steps, refused) = diff(before, after);
    assert!(
        refused
            .iter()
            .any(|(c, m)| c == "PW0366" && m.contains(says)),
        "expected PW0366 saying {says:?}: {refused:#?} (steps {steps:#?})"
    );
}

#[test]
fn an_additive_change_is_planned() {
    let after = todos("added: Int }", "added: Int, note: Option<String> }").replace(
        "    bounded 500\n",
        "    bounded 500\n    unique  by_title on title\n",
    );
    let (steps, refused) = diff(TODOS, &after);
    assert!(refused.is_empty(), "{refused:#?}");
    assert_eq!(
        steps,
        vec![
            Step::AddField {
                table: "Todos".into(),
                field: "note".into(),
                backfill: None
            },
            Step::AddIndex {
                table: "Todos".into(),
                index: "by_title".into()
            },
        ]
    );
    // A table no release had is created whole, and the same schema is no step.
    let (steps, refused) = diff(TODOS, TODOS);
    assert!(
        steps.is_empty() && refused.is_empty(),
        "{steps:?} {refused:?}"
    );
    let empty = "module Other\n";
    let (steps, _) = diff(empty, TODOS);
    assert_eq!(
        steps,
        vec![Step::CreateTable {
            table: "Todos".into()
        }]
    );
}

#[test]
fn a_dropped_field_is_refused_without_its_migrate_and_planned_with_it() {
    let after = todos(" title: String,", "");
    assert_unstated(
        TODOS,
        &after,
        "`Todos.title` was released and is no field of `Todos`'s row",
    );
    let stated = after.replace(
        "    bounded 500\n",
        "    bounded 500\n    migrate title dropped\n",
    );
    let (steps, refused) = diff(TODOS, &stated);
    assert!(refused.is_empty(), "{refused:#?}");
    assert_eq!(
        steps,
        vec![Step::DropField {
            table: "Todos".into(),
            field: "title".into()
        }]
    );
}

#[test]
fn a_rename_is_refused_without_its_migrate_and_planned_with_it() {
    let after = todos("title: String", "name: String");
    let (_, refused) = diff(TODOS, &after);
    let said: Vec<&str> = refused.iter().map(|(_, m)| m.as_str()).collect();
    assert!(
        said.iter()
            .any(|m| m.contains("`Todos.title` was released")),
        "{said:#?}"
    );
    assert!(
        said.iter()
            .any(|m| m.contains("`Todos.name` is new, of `String`")),
        "{said:#?}"
    );
    let stated = after.replace(
        "    bounded 500\n",
        "    bounded 500\n    migrate name renamed from title\n",
    );
    let (steps, refused) = diff(TODOS, &stated);
    assert!(refused.is_empty(), "{refused:#?}");
    assert_eq!(
        steps,
        vec![Step::RenameField {
            table: "Todos".into(),
            from: "title".into(),
            to: "name".into()
        }]
    );
    // A key field renamed keeps its key.
    let key_renamed = TODOS
        .replace("list: ListId", "folder: ListId")
        .replace("key     list, id", "key     folder, id")
        .replace(
            "    bounded 500\n",
            "    bounded 500\n    migrate folder renamed from list\n",
        );
    let (steps, refused) = diff(TODOS, &key_renamed);
    assert!(refused.is_empty(), "{refused:#?}");
    assert!(steps.contains(&Step::RenameField {
        table: "Todos".into(),
        from: "list".into(),
        to: "folder".into()
    }));
}

#[test]
fn a_type_changed_or_a_field_added_with_no_value_is_refused_unless_backfilled() {
    let retyped = todos("added: Int", "added: String");
    assert_unstated(
        TODOS,
        &retyped,
        "`Todos.added` was released as `Int` and is `String`",
    );
    let stated = retyped.replace(
        "    bounded 500\n",
        "    bounded 500\n    migrate added backfilled with \"never\"\n",
    );
    let (steps, refused) = diff(TODOS, &stated);
    assert!(refused.is_empty(), "{refused:#?}");
    assert!(steps.contains(&Step::RetypeField {
        table: "Todos".into(),
        field: "added".into(),
        backfill: pw_core::tables::Literal::Str("never".into())
    }));
    let added = todos("added: Int }", "added: Int, rank: Int }");
    assert_unstated(TODOS, &added, "`Todos.rank` is new, of `Int`");
    let stated = added.replace(
        "    bounded 500\n",
        "    bounded 500\n    migrate rank backfilled with 0\n",
    );
    let (_, refused) = diff(TODOS, &stated);
    assert!(refused.is_empty(), "{refused:#?}");
    // A bound changed, its type kept, is planned as a check changed.
    let bounded = TODOS
        .replace(
            "opaque type ListId = String",
            "opaque type ListId = String\nopaque type Rank = Int where value >= 0",
        )
        .replace("added: Int }", "added: Int, rank: Option<Rank> }");
    let tighter = bounded.replace("where value >= 0", "where value >= 1");
    let (steps, refused) = diff(&bounded, &tighter);
    assert!(refused.is_empty(), "{refused:#?}");
    assert_eq!(
        steps,
        vec![Step::ChangeBounds {
            table: "Todos".into(),
            field: "rank".into()
        }]
    );
    // A backfill of a value its field's type does not hold is refused where
    // it is written.
    let wrong = added.replace(
        "    bounded 500\n",
        "    bounded 500\n    migrate rank backfilled with \"none\"\n",
    );
    assert_refused(&wrong, "PW0361", "backfilled with \"none\"");
}

#[test]
fn a_change_no_migrate_states_is_refused() {
    // A key changed, a table dropped, a table moved between sources.
    assert_unstated(
        TODOS,
        &todos("key     list, id", "key     id"),
        "`Todos`'s key was `list, id` and is `id`",
    );
    assert_unstated(
        TODOS,
        "module Other\n",
        "`Todos` was released and is declared no more",
    );
    assert_unstated(
        TODOS,
        &TODOS.replace("TodoData", "OtherData"),
        "was released in `TodoData` and is in `OtherData`",
    );
    // And a statement of a change the row does not make.
    let stale = todos(
        "    bounded 500\n",
        "    bounded 500\n    migrate rank backfilled with 0\n",
    )
    .replace("added: Int }", "added: Int, rank: Int }");
    let (_, refused) = diff(&stale, &stale);
    assert!(
        refused
            .iter()
            .any(|(c, m)| c == "PW0366" && m.contains("states no change of `Todos`")),
        "{refused:#?}"
    );
}

#[test]
fn a_table_reference_uses_its_own_module_beside_a_namesake() {
    let mine = "module Labels\ntype Label = Label { id: Int, owner: String }\ntable Labels\n    row Label\n    key id\n    refers owner to Labels\n";
    let sibling = mine.replace("id: Int", "id: String");
    let refusals = |files: &[(&str, &str)]| {
        check_sources(&program(files))
            .into_iter()
            .find(|(name, _)| name == "mine.pw")
            .expect("mine was checked")
            .1
            .into_iter()
            .filter(|d| d.code == "PW0361" && d.message.contains("`owner` is"))
            .map(|d| d.message)
            .collect::<Vec<_>>()
    };
    let alone = refusals(&[("mine.pw", mine)]);
    assert_eq!(
        alone.len(),
        1,
        "its String reference is no Int key: {alone:#?}"
    );
    for files in [
        [("mine.pw", mine), ("sibling.pw", sibling.as_str())],
        [("sibling.pw", sibling.as_str()), ("mine.pw", mine)],
    ] {
        assert_eq!(refusals(&files), alone, "a namesake changed its table");
    }
}

#[test]
fn source_ambiguity_is_not_an_alternate_copy_of_the_own_module() {
    for files in [
        [("mine.pw", TODOS), ("sibling.pw", TODOS)],
        [("sibling.pw", TODOS), ("mine.pw", TODOS)],
    ] {
        let checked = check_sources(&program(&files));
        assert!(
            checked
                .iter()
                .flat_map(|(_, ds)| ds)
                .any(|d| d.code == "PW0024"),
            "duplicate modules remain refused"
        );
        let mine = &checked
            .iter()
            .find(|(name, _)| name == "mine.pw")
            .unwrap()
            .1;
        assert!(
            !mine.iter().any(|d| d.code == "PW0361"),
            "a cloned module is not an extra database: {mine:#?}"
        );
    }
    // Two declarations in one file are still two sources.
    let twice = format!(
        "{TODOS}\nsource TodoData\n    holds Todos\n    transactions serializable\n    reads strong\n    changes none\n"
    );
    assert!(
        diagnostics(&[("mine.pw", &twice)])
            .iter()
            .any(|d| { d.code == "PW0361" && d.message.contains("2 source declarations") })
    );
    // Distinct modules remain distinct even if neither imports the other.
    let elsewhere = TODOS.replace("Todos", "Elsewhere");
    let checked = check_sources(&program(&[
        ("mine.pw", TODOS),
        ("elsewhere.pw", &elsewhere),
    ]));
    assert!(
        checked
            .iter()
            .flat_map(|(_, ds)| ds)
            .all(|d| d.code != "PW0024")
    );
    assert!(
        checked
            .iter()
            .flat_map(|(_, ds)| ds)
            .any(|d| { d.code == "PW0361" && d.message.contains("2 source declarations") })
    );
}

#[test]
fn a_lock_changed_by_hand_is_no_release() {
    let lock = released(TODOS).written();
    assert!(Schema::read(&lock).is_ok());
    let edited = lock.replace("\"added\"", "\"created\"");
    let why = Schema::read(&edited).expect_err("its hash is not its tables'");
    assert!(why.contains("changed by hand"), "{why}");
    let format = lock.replace("\"schema\": 1", "\"schema\": 2");
    assert!(
        Schema::read(&format)
            .expect_err("another format")
            .contains("format 2")
    );
}

#[test]
fn a_program_held_to_its_release_is_refused_where_it_is_checked_and_built() {
    let lock = released(TODOS);
    let after = todos(" title: String,", "");
    // `pw check --schema-lock`, through `check_sources_with`.
    let found = diagnostics_with(&[("Todos.pw", &after)], Some(&lock));
    assert!(found.iter().any(|d| d.code == "PW0366"), "{found:#?}");
    // Without the release, no change is judged.
    assert!(diagnostics(&[("Todos.pw", &after)]).is_empty());
    // `pw build --schema-lock` builds nothing.
    let files = program(&[("Todos.pw", &after)]);
    let refused = pw_core::build::build_with(&units(&files), Some(&lock)).expect_err("refused");
    assert!(refused.contains("[PW0366]"), "{refused}");
}

#[test]
fn the_corpus_fixtures_that_name_a_release_are_held_to_it() {
    // Each accepted fixture with a `schema.lock` plans its change clean, and
    // each rejected one is refused for it (`checking_source.rs` runs the
    // rejected through the same reader).
    for (bucket, clean) in [("accepted", true), ("rejected", false)] {
        let dir = root().join("examples").join(bucket);
        let mut seen = 0;
        for e in std::fs::read_dir(&dir).expect("corpus") {
            let p = e.expect("entry").path();
            if p.extension().is_none_or(|x| x != "pw") {
                continue;
            }
            let src = std::fs::read_to_string(&p).expect("read");
            let Some(file) = src
                .lines()
                .find_map(|l| l.trim().strip_prefix("// @schema-lock:"))
            else {
                continue;
            };
            seen += 1;
            let lock = Schema::read(
                &std::fs::read_to_string(p.with_file_name(file.trim())).expect("the lock"),
            )
            .expect("a lock");
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let found = diagnostics_with(&[(name.as_str(), src.as_str())], Some(&lock));
            match clean {
                true => assert!(found.is_empty(), "{name}: {found:#?}"),
                false => assert!(
                    found.iter().any(|d| d.code == "PW0366"),
                    "{name}: {found:#?}"
                ),
            }
        }
        assert!(seen >= 1, "no {bucket} fixture names a release");
    }
    // A-053's four steps.
    let src = std::fs::read_to_string(
        root().join("examples/accepted/A-053-a-tables-change-from-its-released-schema-stated.pw"),
    )
    .expect("A-053");
    let lock = Schema::read(
        &std::fs::read_to_string(root().join(
            "examples/accepted/A-053-a-tables-change-from-its-released-schema-stated.schema.lock",
        ))
        .expect("lock"),
    )
    .expect("a lock");
    let files = program(&[("A-053.pw", &src)]);
    let units = units(&files);
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let (plan, refused) = pw_core::tables::held_to(&hirs, &ws, &sigs, Some(&lock));
    assert!(refused.is_empty(), "{refused:#?}");
    assert_eq!(plan.steps.len(), 4, "{:#?}", plan.steps);
}

#[test]
fn a_program_with_no_table_has_no_tables_file() {
    let files = program(&[(
        "plain.pw",
        "module plain\nfn one() -> Int !{} {\n    1\n}\n",
    )]);
    let build = pw_core::build::build(&units(&files)).expect("builds");
    assert!(build.tables.tables.is_empty());
    let dir = std::env::temp_dir().join(format!("pw-tables-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    build.write(&dir).expect("writes");
    assert!(!dir.join("tables.json").exists());
    assert!(!dir.join("schema.lock").exists());
    let _ = std::fs::remove_dir_all(&dir);
    // And a program with one writes both, the lock reading back as written.
    let files = program(&[("Todos.pw", TODOS)]);
    let build = pw_core::build::build(&units(&files)).expect("builds");
    let dir = std::env::temp_dir().join(format!("pw-tables-some-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    build.write(&dir).expect("writes");
    let lock = Schema::read(&std::fs::read_to_string(dir.join("schema.lock")).expect("lock"))
        .expect("reads");
    assert_eq!(lock, build.schema);
    assert!(dir.join("tables.json").exists());
    let plain = program(&[("plain.pw", "module plain\nfn one() -> Int !{} { 1 }\n")]);
    pw_core::build::build(&units(&plain))
        .expect("plain builds")
        .write(&dir)
        .expect("rebuilds");
    assert!(!dir.join("tables.json").exists());
    assert!(!dir.join("schema.lock").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checking_with_no_release_is_checking() {
    // `check_sources` is `check_sources_with` and no release.
    let files = program(&[("Todos.pw", TODOS)]);
    assert_eq!(
        format!("{:?}", check_sources(&files)),
        format!("{:?}", check_sources_with(&files, None))
    );
}

#[test]
fn full_validation_preserves_native_migration_unit_span_and_reason() {
    let original = "module Notes\ntype Note = Note { id: String, text: String }\ntable Notes\n    row Note\n    key id\n";
    let lock = released(original);
    let after = original.replace(", text: String", "");
    let files = program(&[("Notes.pw", &after)]);
    let expected = pw_core::check::check_sources_with(&files, Some(&lock));
    let actual = pw_core::program::validate_sources_with(&files, Some(&lock));
    let migration = |found: &Vec<(String, Vec<pw_core::diagnostics::Diagnostic>)>| {
        found
            .iter()
            .flat_map(|(path, ds)| {
                ds.iter()
                    .filter(|d| d.code == "PW0366")
                    .map(move |d| (path.clone(), d.clone()))
            })
            .collect::<Vec<_>>()
    };
    let expected = migration(&expected);
    assert!(!expected.is_empty(), "the changed schema must be refused");
    assert_eq!(migration(&actual.diagnostics), expected);
    assert!(
        actual
            .diagnostics
            .iter()
            .flat_map(|(_, ds)| ds)
            .all(|d| d.code != "PW6601")
    );
    assert!(actual.into_build().is_err());
}

#[test]
fn two_indexes_cannot_describe_one_ordered_field_list() {
    let fixture = include_str!("fixtures/tables-duplicate-index-fields.pw");
    let found = diagnostics(&[("Todos.pw", fixture)]);
    assert!(
        found
            .iter()
            .any(|d| d.code == "PW0361" && d.message.contains("same ordered fields")),
        "{found:#?}"
    );
    for first in ["index", "unique"] {
        for second in ["index", "unique"] {
            for fields in ["done", "title, done"] {
                let source = todos(
                    "index   by_done on done",
                    &format!("{first} by_first on {fields}\n    {second} by_second on {fields}"),
                );
                let found = diagnostics(&[("Todos.pw", &source)]);
                let repeated: Vec<_> = found
                    .iter()
                    .filter(|d| d.code == "PW0361" && d.message.contains("same ordered fields"))
                    .collect();
                assert_eq!(
                    repeated.len(),
                    1,
                    "{first} / {second} / {fields}: {found:#?}"
                );
                assert_eq!(
                    repeated[0].related.len(),
                    1,
                    "the first declaration is identified"
                );
            }
        }
    }
    for source in [
        TODOS.to_owned(),
        todos("index   by_done on done", "unique by_done on done"),
        todos(
            "index   by_done on done",
            "index by_forward on title, done\n    unique by_reverse on done, title",
        ),
    ] {
        assert!(
            refused(&source).is_empty(),
            "valid ordered-field neighbor: {:#?}",
            refused(&source)
        );
    }
}
