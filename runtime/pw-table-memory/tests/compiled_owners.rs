//! **An owned table, compiled and run on the memory adapter, each function
//! its caller's** (track `row-ownership`, ADR-0384 H1).
//!
//! `examples/owned/`'s queries and commands, compiled by the compiler and
//! run through the table gate (`call_with_tables_as`) on the memory adapter,
//! each call made as one user (`Caller`), as the host makes it from the
//! session's principal: a reader reads their own notes and the notes sent to
//! them, the latter without the draft their author keeps; a handle that is
//! not the call's (a forged argument) is refused before any row is read; a
//! call no one makes reads no one's rows; a command writes and deletes its
//! caller's notes alone, and a note whose author is another is refused
//! before it is written. The manifest is held whole for its owners, and an
//! import typed otherwise than its owned function is refused where it links.

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pw_core::backend::component::Built;
use pw_core::check::Unit;
use pw_host::engine::{HostFn, Val};
use pw_host::tables::Caller;
use pw_table_memory::MemoryTables;
use serde_json::json;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The platform's sources, and `examples/owned/`'s, `Notes.pw` and
/// `app.pw` written as `edited` gives them where it does.
fn units(edited: &[(&str, &str)]) -> Vec<Unit> {
    let mut sources = Vec::new();
    for directory in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root().join(directory))
            .expect("platform sources")
            .map(|entry| entry.expect("source entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "pw"))
            .collect();
        paths.sort();
        for path in paths {
            sources.push((
                path.display().to_string(),
                std::fs::read_to_string(&path).expect("platform source"),
            ));
        }
    }
    for name in ["Notes.pw", "NoteData.pw", "app.pw"] {
        let path = root().join("examples/owned").join(name);
        let src = match edited.iter().find(|(n, _)| *n == name) {
            Some((_, src)) => src.to_string(),
            None => std::fs::read_to_string(&path).expect("fixture source"),
        };
        sources.push((path.display().to_string(), src));
    }
    sources
        .into_iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(&src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            let hir = pw_core::lower::lower_file(&src, &parsed.green);
            Unit { path, src, hir }
        })
        .collect()
}

/// One compiled query or command.
struct Component {
    bytes: Vec<u8>,
    contract: pw_host::ComponentContract,
    imports: Vec<String>,
}

impl Component {
    fn export(&self) -> [&str; 2] {
        let export = self.contract.exports[0]
            .component
            .as_ref()
            .expect("a compiled export has a component path");
        [&export.interface[..], &export.function[..]]
    }

    /// Admitted on a node granting every capability its contract requires.
    fn granted(&self) -> pw_host::Granted {
        let grants = self
            .contract
            .required_capabilities
            .iter()
            .map(|c| format!("{:?}", c.name()))
            .collect::<Vec<_>>()
            .join(",");
        let topology = pw_host::Topology::from_json(&format!(
            r#"{{"nodes":[{{"name":"origin","world":"origin","grants":[{grants}]}}]}}"#
        ))
        .expect("topology");
        let admission = pw_host::admit(&self.contract, &topology, "origin", &self.imports);
        assert!(admission.is_admitted(), "{admission:?}");
        pw_host::Granted::from(&admission, &Default::default()).expect("admitted")
    }
}

struct Program {
    tables: pw_core::tables::TablesJson,
    manifest: pw_host::tables::Manifest,
    components: BTreeMap<String, Component>,
}

impl Program {
    fn component(&self, id: &str) -> &Component {
        self.components
            .get(&format!("owned.app.{id}"))
            .unwrap_or_else(|| panic!("no component owned.app.{id}"))
    }
}

/// The fixture program, as the compiler builds it.
fn program() -> Program {
    built(&[])
}

fn built(edited: &[(&str, &str)]) -> Program {
    let build = pw_core::build::build(&units(edited)).assert_ok("the owned program builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).expect("serialize compiled contracts"),
    )
    .assert_ok("the host reads the compiler's contracts");
    let manifest = pw_host::tables::Manifest::from_json(
        &serde_json::to_string(&build.tables).expect("serialize tables.json"),
    )
    .assert_ok("the host reads the compiler's tables.json");
    let mut components = BTreeMap::new();
    for (id, artifact) in &build.components {
        let Built::Component { compiled, .. } = artifact else {
            continue;
        };
        let bytes = compiled.component.bytes.clone();
        let imports = pw_host::engine::imports_of(&bytes).assert_ok("Wasmtime reads its imports");
        let contract = contracts
            .iter()
            .find(|contract| contract.component_id == *id)
            .unwrap_or_else(|| panic!("{id} has no contract"))
            .clone();
        components.insert(
            id.clone(),
            Component {
                bytes,
                contract,
                imports,
            },
        );
    }
    Program {
        tables: build.tables.clone(),
        manifest,
        components,
    }
}

const ALICE: &str = "alice";
const BOB: &str = "bob";
const CAROL: &str = "carol";

fn as_user(user: &str) -> Caller {
    Caller::User(user.to_string())
}

/// A note as the adapter stores it.
fn note_json(author: &str, id: &str, to: &str, added: i64) -> serde_json::Value {
    json!({
        "author": author, "id": id, "to": to, "text": format!("{id} from {author}"),
        "draft": format!("{author}'s draft of {id}"), "added": added
    })
}

/// A note as the component's WIT declares it.
fn note(author: &str, id: &str, to: &str, added: i64) -> Val {
    Val::Record(vec![
        ("author".into(), Val::String(author.into())),
        ("id".into(), Val::String(id.into())),
        ("to".into(), Val::String(to.into())),
        ("text".into(), Val::String(format!("{id} from {author}"))),
        (
            "draft".into(),
            Val::String(format!("{author}'s draft of {id}")),
        ),
        ("added".into(), Val::S64(added)),
    ])
}

/// A note as a reader it is sent to sees it: no `draft`.
fn seen(author: &str, id: &str, to: &str, added: i64) -> Val {
    Val::Record(vec![
        ("author".into(), Val::String(author.into())),
        ("id".into(), Val::String(id.into())),
        ("to".into(), Val::String(to.into())),
        ("text".into(), Val::String(format!("{id} from {author}"))),
        ("added".into(), Val::S64(added)),
    ])
}

/// The program's tables, holding alice's two notes and bob's one, the same
/// id as alice's first, written through the adapter's own transaction.
fn seeded(program: &Program) -> MemoryTables {
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    tables
        .transaction(|tx| {
            tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 1)])?;
            tx.write(
                "pw:tables/notes#insert",
                &[note_json(ALICE, "n2", CAROL, 2)],
            )?;
            tx.write("pw:tables/notes#insert", &[note_json(BOB, "n1", ALICE, 3)])?;
            Ok(())
        })
        .assert_ok("seeded through the adapter's own transaction");
    tables
}

/// The platform's imports a component's contract names but a table: the
/// reader's principal, answering `user`, and the outbox's, answering
/// nothing.
fn platform(component: &Component, user: &str) -> BTreeMap<String, HostFn> {
    let mut host = BTreeMap::new();
    for import in &component.contract.imports {
        if pw_host::tables::is_table_interface(&import.interface) {
            continue;
        }
        let key = import.key();
        let answer = match key.as_str() {
            "pw:host/principal#read" => vec![Val::String(user.to_string())],
            _ => Vec::new(),
        };
        host.insert(key, Arc::new(move |_: &[Val]| Ok(answer.clone())) as HostFn);
    }
    host
}

/// A host function that counts its calls, then does what `inner` does.
fn counted(inner: HostFn, calls: &Arc<AtomicUsize>) -> HostFn {
    let calls = Arc::clone(calls);
    Arc::new(move |args: &[Val]| {
        calls.fetch_add(1, Ordering::SeqCst);
        inner(args)
    })
}

/// **A query, made as `caller`**, from the published tables, its table
/// reads counted in `reads`.
fn query_counted(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    reads: &Arc<AtomicUsize>,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let mut host: BTreeMap<String, HostFn> = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the query's reads")
        .into_iter()
        .map(|(key, read)| (key, counted(read, reads)))
        .collect();
    let user = match caller {
        Caller::User(user) => user.clone(),
        Caller::Nobody => String::new(),
    };
    host.extend(platform(component, &user));
    pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        caller,
        &component.export(),
        args,
    )
}

fn query(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    query_counted(program, tables, id, caller, args, &Arc::default())
}

/// **A command, made as `user`**, as the platform runs one: in a command of
/// its own, committed where it answers, undone where it fails or answers a
/// declared error.
fn command(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    user: &str,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let staged = tables.begin().assert_ok("its turn");
    let mut host = staged
        .host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the command's reads and writes");
    host.extend(platform(component, user));
    let answer = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(user),
        &component.export(),
        args,
    );
    let declared_error = matches!(answer.as_deref(), Ok([Val::Result(Err(_))]));
    if answer.is_ok() && !declared_error {
        staged.commit().assert_ok("its commit");
    }
    answer
}

fn handle(user: &str) -> Val {
    Val::String(user.to_string())
}

/// Every note, as the adapter holds it, by author and id.
fn every_note(tables: &MemoryTables) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for author in [ALICE, BOB, CAROL] {
        let rows = tables
            .read("pw:tables/notes#of-author", &[json!(author)])
            .assert_ok("the adapter reads an owner's notes");
        for row in rows.as_array().cloned().unwrap_or_default() {
            out.push((
                row["author"].as_str().unwrap_or_default().to_string(),
                row["id"].as_str().unwrap_or_default().to_string(),
            ));
        }
    }
    out
}

// --- reads ----------------------------------------------------------------------

#[test]
fn a_reader_reads_their_own_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    assert_eq!(
        query(&program, &tables, "Mine", &as_user(ALICE), &[handle(ALICE)]),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", CAROL, 2)
        ])])
    );
    assert_eq!(
        query(&program, &tables, "Mine", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![note(BOB, "n1", ALICE, 3)])])
    );
    // One id, two authors: each reads their own.
    let n1 = Val::String("n1".into());
    assert_eq!(
        query(
            &program,
            &tables,
            "One",
            &as_user(ALICE),
            &[handle(ALICE), n1.clone()]
        ),
        Ok(vec![Val::Option(Some(Box::new(note(ALICE, "n1", BOB, 1))))])
    );
    assert_eq!(
        query(&program, &tables, "One", &as_user(BOB), &[handle(BOB), n1]),
        Ok(vec![Val::Option(Some(Box::new(note(BOB, "n1", ALICE, 3))))])
    );
    // A set form: the handle beside the list, each key the reader's.
    let ids = Val::List(vec![
        Val::String("n2".into()),
        Val::String("n1".into()),
        Val::String("n9".into()),
    ]);
    assert_eq!(
        query(
            &program,
            &tables,
            "Picked",
            &as_user(ALICE),
            &[handle(ALICE), ids.clone()]
        ),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", CAROL, 2)
        ])])
    );
    assert_eq!(
        query(
            &program,
            &tables,
            "Picked",
            &as_user(BOB),
            &[handle(BOB), ids]
        ),
        Ok(vec![Val::List(vec![note(BOB, "n1", ALICE, 3)])])
    );
}

#[test]
fn a_reader_a_note_is_sent_to_reads_it_without_its_draft() {
    let program = program();
    let tables = seeded(&program);
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n1", BOB, 1)])])
    );
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(CAROL), &[handle(CAROL)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n2", CAROL, 2)])])
    );
    // The draft is nowhere in what the sharer is answered.
    let answer = query(&program, &tables, "ToMe", &as_user(ALICE), &[handle(ALICE)])
        .assert_ok("alice reads what bob sent her");
    assert_eq!(answer, vec![Val::List(vec![seen(BOB, "n1", ALICE, 3)])]);
    assert!(
        !format!("{answer:?}").contains("draft"),
        "a sharer was answered a private field: {answer:?}"
    );
}

#[test]
fn a_forged_reader_is_refused_before_any_row_is_read() {
    let program = program();
    let tables = seeded(&program);
    // A query's `reader` the host would make from the session; given
    // another's, as a forged request or a forged component would.
    for (id, args) in [
        ("Mine", vec![handle(ALICE)]),
        ("ToMe", vec![handle(ALICE)]),
        ("One", vec![handle(ALICE), Val::String("n1".into())]),
        (
            "Picked",
            vec![handle(ALICE), Val::List(vec![Val::String("n1".into())])],
        ),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let refused = query_counted(&program, &tables, id, &as_user(BOB), &args, &reads)
            .assert_err("a handle that is not the call's is refused");
        assert!(
            refused.contains("a handle that is not its caller's"),
            "{id}: {refused}"
        );
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "{id}: the adapter read a row"
        );
    }
}

#[test]
fn a_call_no_one_makes_reads_no_owned_row() {
    let program = program();
    let tables = seeded(&program);
    let reads = Arc::new(AtomicUsize::new(0));
    let refused = query_counted(
        &program,
        &tables,
        "Mine",
        &Caller::Nobody,
        &[handle(ALICE)],
        &reads,
    )
    .assert_err("no one's call reads an owned table");
    assert!(refused.contains("no one makes this call"), "{refused}");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    // `call_with_tables` is a call made as no one.
    let component = program.component("Mine");
    let granted = component.granted();
    let host = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the query's reads");
    let refused = pw_host::engine::call_with_tables(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &component.export(),
        &[handle(ALICE)],
    )
    .assert_err("call_with_tables is made as no one");
    assert!(refused.contains("no one makes this call"), "{refused}");
}

// --- writes ---------------------------------------------------------------------

#[test]
fn a_command_writes_its_callers_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    // `write` makes a note of the caller's.
    let text = Val::String("hello".into());
    assert_eq!(
        command(
            &program,
            &tables,
            "write",
            CAROL,
            &[Val::String("n7".into()), handle(ALICE), text]
        ),
        Ok(vec![Val::Result(Ok(None))])
    );
    assert!(
        every_note(&tables).contains(&(CAROL.to_string(), "n7".to_string())),
        "{:?}",
        every_note(&tables)
    );
    // `keep`, given a note whose author is another, as a forged request
    // gives it: refused before it is written, and nothing commits.
    let before = every_note(&tables);
    let refused = command(&program, &tables, "keep", BOB, &[note(ALICE, "n5", BOB, 9)])
        .assert_err("a note of another's is refused");
    assert!(
        refused.contains("whose `author` is not its writer"),
        "{refused}"
    );
    assert_eq!(every_note(&tables), before, "a refused command wrote");
    // Its own: kept.
    assert_eq!(
        command(&program, &tables, "keep", BOB, &[note(BOB, "n5", ALICE, 9)]),
        Ok(vec![])
    );
    assert!(every_note(&tables).contains(&(BOB.to_string(), "n5".to_string())));
}

#[test]
fn a_command_deletes_its_callers_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    // Bob removes his `n1`; alice's `n1`, the same id, stays.
    assert_eq!(
        command(
            &program,
            &tables,
            "remove",
            BOB,
            &[Val::String("n1".into()), handle(ALICE)]
        ),
        Ok(vec![])
    );
    assert_eq!(
        every_note(&tables),
        [
            (ALICE.to_string(), "n1".to_string()),
            (ALICE.to_string(), "n2".to_string())
        ]
    );
    // Alice forgets every note of hers, and no one else's.
    let tables = seeded(&program);
    assert_eq!(command(&program, &tables, "forget", ALICE, &[]), Ok(vec![]));
    assert_eq!(every_note(&tables), [(BOB.to_string(), "n1".to_string())]);
}

// --- the manifest, and the link ----------------------------------------------------

#[test]
fn the_manifest_is_held_whole_for_its_owners() {
    let program = program();
    let refused = |change: &dyn Fn(&mut serde_json::Value), says: &str| {
        let mut json = serde_json::to_value(&program.tables).expect("tables.json as JSON");
        change(&mut json["tables"][0]);
        let why = pw_host::tables::Manifest::from_json(&json.to_string())
            .assert_err("the host refuses the manifest");
        assert!(why.contains(says), "{why}");
    };
    refused(
        &|t| {
            t.as_object_mut().map(|t| t.remove("owner"));
        },
        "shares or keeps fields private, and no one owns it",
    );
    // A host knows a field by what it stores: `text` is a string, and the
    // compiler's PW0367 is what holds it to a principal's id.
    refused(
        &|t| t["owner"] = json!("text"),
        "its key leads with another",
    );
    refused(
        &|t| t["owner"] = json!("added"),
        "which is no field holding a principal's id",
    );
    refused(&|t| t["shares"] = json!(["author"]), "shares with `author`");
    refused(
        &|t| {
            t["owner"] = json!("to");
            t["shares"] = json!([]);
        },
        "is owned by `to`, and its key leads with another",
    );
    refused(&|t| t["private"] = json!(["to"]), "keeps `to` private");
    refused(
        &|t| t["private"] = json!(["added"]),
        "keeps `added` private",
    );
    // `by_to` read by `to`, which it no longer shares with.
    refused(
        &|t| t["shares"] = json!([]),
        "its index `by_to` is read by no reader of its rows alone",
    );
    refused(
        &|t| {
            t["functions"][0]["reader"] = json!("to");
        },
        "`get` is not given its reader's handle",
    );
    refused(
        &|t| {
            t["functions"][0]
                .as_object_mut()
                .map(|f| f.remove("reader"));
        },
        "`get` is not given its reader's handle",
    );
    refused(
        &|t| {
            let insert = t["functions"]
                .as_array_mut()
                .and_then(|fs| fs.iter_mut().find(|f| f["name"] == "insert"));
            if let Some(insert) = insert {
                insert["reader"] = json!("to");
            }
        },
        "`insert` is not given its reader's handle",
    );
    refused(
        &|t| {
            t["bounded"] = json!(10);
            t["functions"].as_array_mut().map(|fs| {
                fs.push(json!({
                    "name": "all", "import": "pw:tables/notes#all", "primitive": "all",
                    "effect": "read", "by": [], "params": [], "returns": "List<Note>",
                    "sorted_by": ["added", "author", "id"], "reader": "author"
                }))
            });
        },
        "is read whole by `all`",
    );
}

#[test]
fn an_import_typed_as_an_unowned_tables_is_refused_where_it_links() {
    // The same program built from `Notes` with no owner: its `Picked`
    // imports `get-many` taking one list of keys, which the owned manifest
    // types as the handle beside a list of ids. A stale build, refused
    // before anything is instantiated.
    let src = std::fs::read_to_string(root().join("examples/owned/Notes.pw")).expect("Notes.pw");
    let unowned = src
        .replacen("    owned   by author\n", "", 1)
        .replacen("    shares  with to\n", "", 1)
        .replacen("    private draft\n", "", 1);
    let app = "module owned.app\n\nimport List\nimport Notes.{ Note, NotesAuthorId }\nimport Notes\n\n\
               query Picked(keys: List<NotesAuthorId>) -> List<Note>\n    consistency strong\n{\n    \
               Notes.get_many(keys)\n}\n";
    let stale = built(&[("Notes.pw", &unowned), ("app.pw", app)]);
    let program = program();
    let tables = seeded(&program);
    let component = stale.component("Picked");
    let granted = component.granted();
    let host = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the reads");
    let keys = Val::List(vec![Val::Record(vec![
        ("author".into(), Val::String(ALICE.into())),
        ("id".into(), Val::String("n1".into())),
    ])]);
    let refused = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(ALICE),
        &component.export(),
        &[Val::String(ALICE.into()), keys],
    )
    .assert_err("a stale build is refused where it links");
    assert!(
        refused.contains("typed otherwise than tables.json declares the owned table's function"),
        "{refused}"
    );
}

#[test]
fn a_sharers_read_answered_another_readers_row_is_refused() {
    // Whatever answers a sharer's read, a row for another reader is no
    // answer of it: held to what it was asked, then shown as the seen row.
    let program = program();
    let tables = seeded(&program);
    let component = program.component("ToMe");
    let granted = component.granted();
    let mut host: BTreeMap<String, HostFn> = BTreeMap::new();
    host.insert(
        "pw:tables/notes#by-to".into(),
        Arc::new(|_: &[Val]| Ok(vec![Val::List(vec![note(ALICE, "n2", CAROL, 2)])])) as HostFn,
    );
    let refused = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(BOB),
        &component.export(),
        &[handle(BOB)],
    )
    .assert_err("another reader's row is refused");
    assert!(refused.contains("was not asked for"), "{refused}");
    // The control: the adapter's own answer.
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n1", BOB, 1)])])
    );
}
