// The owned fixture's program, compiled, and the calls the owned tables'
// tests make of it (track `row-ownership`), included by each such test.

#[path = "assert_result.rs"]
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

/// The platform's sources, then `files`, each a path and its source.
fn units_of(files: Vec<(String, String)>) -> Vec<Unit> {
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
    sources.extend(files);
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

/// The platform's sources, and `examples/owned/`'s, `Notes.pw` and
/// `app.pw` written as `edited` gives them where it does.
fn units(edited: &[(&str, &str)]) -> Vec<Unit> {
    let mut files = Vec::new();
    for name in ["Notes.pw", "NoteData.pw", "app.pw"] {
        let path = root().join("examples/owned").join(name);
        let src = match edited.iter().find(|(n, _)| *n == name) {
            Some((_, src)) => src.to_string(),
            None => std::fs::read_to_string(&path).expect("fixture source"),
        };
        files.push((path.display().to_string(), src));
    }
    units_of(files)
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

    /// **The program's predicates with a body, compiled**, by the component
    /// that answers each: what the engine asks for a requirement naming one.
    fn predicates(&self) -> pw_host::engine::Predicates {
        let predicates = self
            .components
            .values()
            .filter(|c| c.contract.exports.iter().any(|e| e.kind == "predicate"))
            .map(|c| {
                pw_host::engine::Predicate::new(
                    pw_host::engine::Prepared::compile(&c.bytes).assert_ok("a predicate compiles"),
                    c.contract.clone(),
                    c.granted(),
                )
                .assert_ok("a program predicate")
            });
        pw_host::engine::Predicates::new(predicates).assert_ok("each predicate once")
    }

    /// **The platform's operations every component of the program imports**,
    /// but the tables': the reader's principal, answering `user`, and the
    /// outbox's, answering nothing. One map serves an export and the
    /// predicates it requires, as a host's does.
    fn platform(&self, user: &str) -> BTreeMap<String, HostFn> {
        let mut host = BTreeMap::new();
        for component in self.components.values() {
            host.extend(platform(component, user));
        }
        host
    }
}

/// The fixture program, as the compiler builds it.
fn program() -> Program {
    built(&[])
}

/// **A program of the platform and `files` alone**, each a name and its
/// source, as the compiler builds it.
fn built_from(files: &[(&str, &str)]) -> Program {
    from_units(units_of(
        files
            .iter()
            .map(|(name, src)| (name.to_string(), src.to_string()))
            .collect(),
    ))
}

fn built(edited: &[(&str, &str)]) -> Program {
    from_units(units(edited))
}

fn from_units(units: Vec<Unit>) -> Program {
    let build = pw_core::build::build(&units).assert_ok("the owned program builds");
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

/// **A call's tables, watched** (track `row-ownership`'s tests): what
/// `inner` gives the engine, each of the export's table functions counted in
/// `body` (so a test sees whether a body ran), each predicate's asking in
/// `asked`, and `after_read`, where given, run once each of a predicate's
/// reads has answered (where a test stands between a predicate and the body
/// it guards).
#[allow(dead_code)]
struct Watched<'a> {
    inner: &'a dyn pw_host::engine::CallTables,
    body: Arc<AtomicUsize>,
    asked: Arc<AtomicUsize>,
    after_read: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[allow(dead_code)]
impl<'a> Watched<'a> {
    fn new(inner: &'a dyn pw_host::engine::CallTables) -> Watched<'a> {
        Watched {
            inner,
            body: Arc::default(),
            asked: Arc::default(),
            after_read: None,
        }
    }
}

impl pw_host::engine::CallTables for Watched<'_> {
    fn manifest(&self) -> &pw_host::tables::Manifest {
        self.inner.manifest()
    }

    fn functions(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, HostFn>, String> {
        Ok(self
            .inner
            .functions(contract, granted)?
            .into_iter()
            .map(|(key, function)| (key, counted(function, &self.body)))
            .collect())
    }

    fn predicate_reads(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, HostFn>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let reads = self.inner.predicate_reads(contract, granted)?;
        let Some(after) = self.after_read.clone() else {
            return Ok(reads);
        };
        Ok(reads
            .into_iter()
            .map(|(key, read)| {
                let after = Arc::clone(&after);
                let watched: HostFn = Arc::new(move |args: &[Val]| {
                    let answer = read(args);
                    after();
                    answer
                });
                (key, watched)
            })
            .collect())
    }
}

/// **An export, made as `user`, its predicates asked first, as the engine
/// asks them**: the program's by the engine itself, on `tables` (a command's
/// transaction, or the published tables), the deployment's answering
/// `deployment`, under `limits`. Counts in `deployment_asked`.
#[allow(dead_code, clippy::too_many_arguments)]
fn authorized(
    program: &Program,
    id: &str,
    user: &str,
    tables: &dyn pw_host::engine::CallTables,
    limits: &pw_host::Limits,
    args: &[Val],
    deployment: bool,
    deployment_asked: &AtomicUsize,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let prepared =
        pw_host::engine::Prepared::compile(&component.bytes).assert_ok("the export compiles");
    prepared.call_authorized_with_tables_as(
        &component.contract,
        &component.granted(),
        limits,
        &program.platform(user),
        tables,
        &program.predicates(),
        &as_user(user),
        &component.export(),
        args,
        |_, _| {
            deployment_asked.fetch_add(1, Ordering::SeqCst);
            Ok(deployment)
        },
    )
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
/// declared error. A commit the adapter refuses (a reference, a bound) is
/// the command's refusal, told as the browser would be told it.
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
        staged
            .commit()
            .map_err(|error| format!("commit refused: {error:?}"))?;
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

