//! What the compiled-component tests share: the fixture programs
//! (`examples/tables`) as the compiler builds them (the readers, the app, the
//! writers), their components, their contracts and tables.json as the host
//! reads them.
//!
//! Each test file uses part of it.
#![allow(dead_code)]

use crate::assert_result::*;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pw_core::backend::component::Built;
use pw_core::check::Unit;
use pw_host::engine::{HostFn, Val};

fn units(fixtures: &[&str]) -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for directory in ["packages/pw-std", "packages/pw-platform-web"] {
        paths.extend(
            std::fs::read_dir(root.join(directory))
                .expect("platform sources")
                .map(|entry| entry.expect("source entry").path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "pw")),
        );
    }
    paths.sort();
    for name in fixtures {
        paths.push(root.join("examples/tables").join(name));
    }
    paths
        .into_iter()
        .map(|path| {
            let src = std::fs::read_to_string(&path).expect("fixture source");
            let parsed = pw_syntax::parse_tree(&src);
            assert!(parsed.ok(), "{}: {:?}", path.display(), parsed.errors);
            let hir = pw_core::lower::lower_file(&src, &parsed.green);
            Unit {
                path: path.display().to_string(),
                src,
                hir,
            }
        })
        .collect()
}

/// The reader that gets one list by its key, and the one that gets several.
pub const BY_ID: &str = "tables.reader.ById";
pub const BY_IDS: &str = "tables.reader.ByIds";
pub const GET: &str = "pw:tables/lists#get";
pub const GET_MANY: &str = "pw:tables/lists#get-many";
/// A node's grant of the readers' one capability, as topology JSON writes it.
pub const READ_LISTS: &str = r#""database.read<Lists>""#;

/// One compiled reader: its bytes, its contract as the host reads it, and
/// the imports Wasmtime finds in its bytes.
pub struct Reader {
    pub bytes: Vec<u8>,
    pub contract: pw_host::ComponentContract,
    pub imports: Vec<String>,
}

impl Reader {
    pub fn export(&self) -> [&str; 2] {
        let export = self.contract.exports[0]
            .component
            .as_ref()
            .expect("a reader's export has a component path");
        [&export.interface[..], &export.function[..]]
    }

    /// `contract`'s admission on a node granting `grants`, by the imports in
    /// this reader's bytes.
    pub fn admission(
        &self,
        contract: &pw_host::ComponentContract,
        grants: &str,
    ) -> pw_host::Admission {
        let topology = pw_host::Topology::from_json(&format!(
            r#"{{"nodes":[{{"name":"origin","world":"origin","grants":[{grants}]}}]}}"#
        ))
        .expect("topology");
        pw_host::admit(contract, &topology, "origin", &self.imports)
    }

    /// Admitted, as [`Reader::admission`], and what it grants.
    pub fn admitted(
        &self,
        contract: &pw_host::ComponentContract,
        grants: &str,
    ) -> pw_host::Granted {
        let admission = self.admission(contract, grants);
        assert!(admission.is_admitted(), "{admission:?}");
        pw_host::Granted::from(&admission, &Default::default()).expect("admitted")
    }

    /// Call its export with `contract`, linking table imports only where
    /// `tables` is given.
    pub fn call_as(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
        host: &BTreeMap<String, HostFn>,
        tables: Option<&pw_host::tables::Manifest>,
        args: &[Val],
    ) -> Result<Vec<Val>, String> {
        let limits = pw_host::Limits::unbounded();
        match tables {
            Some(tables) => pw_host::engine::call_with_tables(
                &self.bytes,
                contract,
                granted,
                &limits,
                host,
                tables,
                &self.export(),
                args,
            ),
            None => pw_host::engine::call_within(
                &self.bytes,
                contract,
                granted,
                &limits,
                host,
                &self.export(),
                args,
            ),
        }
    }

    /// Call it with its own contract and the program's tables.
    pub fn call(
        &self,
        granted: &pw_host::Granted,
        host: &BTreeMap<String, HostFn>,
        tables: &pw_host::tables::Manifest,
        args: &[Val],
    ) -> Result<Vec<Val>, String> {
        self.call_as(&self.contract, granted, host, Some(tables), args)
    }
}

/// The fixture program as the compiler builds it.
pub struct Program {
    /// tables.json, as the compiler writes it.
    pub tables: pw_core::tables::TablesJson,
    /// tables.json, as the host reads it.
    pub manifest: pw_host::tables::Manifest,
    pub readers: BTreeMap<String, Reader>,
}

impl Program {
    pub fn reader(&self, id: &str) -> &Reader {
        self.readers
            .get(id)
            .unwrap_or_else(|| panic!("the program has no reader {id}"))
    }
}

/// The readers' program: the lists, their items, their source, `reader.pw`.
pub fn program() -> Program {
    program_of(&["reader.pw"], &[BY_ID, BY_IDS])
}

/// `examples/tables/app.pw`'s queries and commands (ADR-0386's acceptance).
pub const EVERY_LIST: &str = "tables.app.EveryList";
pub const LIST_AT: &str = "tables.app.ListAt";
pub const ITEMS_OF: &str = "tables.app.ItemsOf";
pub const OPEN_ITEMS: &str = "tables.app.OpenItems";
pub const EVERY_ITEM: &str = "tables.app.EveryItem";
pub const MAKE_LIST: &str = "tables.app.make_list";
pub const KEEP_ITEM: &str = "tables.app.keep_item";
pub const DROP_ITEM: &str = "tables.app.drop_item";
pub const CLEAR_LIST: &str = "tables.app.clear_list";

/// The app's program: the lists, their items, their source, `app.pw`.
pub fn app() -> Program {
    program_of(
        &["app.pw"],
        &[
            EVERY_LIST, LIST_AT, ITEMS_OF, OPEN_ITEMS, EVERY_ITEM, MAKE_LIST, KEEP_ITEM, DROP_ITEM,
            CLEAR_LIST,
        ],
    )
}

/// `examples/tables/writer.pw`'s commands.
pub const INSERT_THEN_GET: &str = "tables.writer.insert_then_get";
pub const PUT_THEN_OPEN: &str = "tables.writer.put_then_open";
pub const INSERT_THEN_DIVIDE: &str = "tables.writer.insert_then_divide";
pub const RENAME: &str = "tables.writer.rename";

/// The writers' program: the lists, their items, their source, `writer.pw`.
pub fn writer() -> Program {
    program_of(
        &["writer.pw"],
        &[INSERT_THEN_GET, PUT_THEN_OPEN, INSERT_THEN_DIVIDE, RENAME],
    )
}

/// **The tables' declarations and `program`, its source edited by `edit`,
/// as `pw build` checks and builds them**: the build, or why the program does
/// not check.
pub fn build_edited(
    program: &str,
    edit: impl Fn(&str) -> String,
) -> Result<pw_core::build::Build, String> {
    let mut units = units(&["Lists.pw", "Items.pw", "ListData.pw", program]);
    let last = units.last_mut().expect("the program's unit");
    last.src = edit(&last.src);
    let parsed = pw_syntax::parse_tree(&last.src);
    assert!(parsed.ok(), "{:?}", parsed.errors);
    last.hir = pw_core::lower::lower_file(&last.src, &parsed.green);
    pw_core::program::build(&units)
}

/// The tables' declarations and `program`, built by the compiler, and the
/// components `ids` compiled.
fn program_of(program: &[&str], ids: &[&str]) -> Program {
    let mut fixtures = vec!["Lists.pw", "Items.pw", "ListData.pw"];
    fixtures.extend(program);
    let build = pw_core::build::build(&units(&fixtures)).assert_ok("the table program builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).expect("serialize compiled contracts"),
    )
    .assert_ok("the host reads the compiler's contracts");
    let manifest = pw_host::tables::Manifest::from_json(
        &serde_json::to_string(&build.tables).expect("serialize tables.json"),
    )
    .assert_ok("the host reads the compiler's tables.json");
    let mut readers = BTreeMap::new();
    for &id in ids {
        let (_, artifact) = build
            .components
            .iter()
            .find(|(component, _)| component == id)
            .unwrap_or_else(|| panic!("{id} is not emitted"));
        let Built::Component { compiled, .. } = artifact else {
            panic!("{id} did not compile: {artifact:?}");
        };
        let bytes = compiled.component.bytes.clone();
        let imports = pw_host::engine::imports_of(&bytes).assert_ok("Wasmtime reads its imports");
        let contract = contracts
            .iter()
            .find(|contract| contract.component_id == id)
            .unwrap_or_else(|| panic!("{id} has no contract"))
            .clone();
        readers.insert(
            id.to_string(),
            Reader {
                bytes,
                contract,
                imports,
            },
        );
    }
    Program {
        tables: build.tables.clone(),
        manifest,
        readers,
    }
}

/// tables.json as the host reads it, after `change` edits the compiler's.
pub fn manifest_where(
    tables: &pw_core::tables::TablesJson,
    change: impl FnOnce(&mut serde_json::Value),
) -> Result<pw_host::tables::Manifest, String> {
    let mut json = serde_json::to_value(tables).expect("tables.json as JSON");
    change(&mut json);
    pw_host::tables::Manifest::from_json(&json.to_string())
}

/// A `TodoList` as the component's WIT declares it: its fields in their
/// declared order, not the order of a JSON object's keys.
pub fn todo_list(id: &str, name: &str, added: i64) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(id.into())),
        ("name".into(), Val::String(name.into())),
        ("slug".into(), Val::String(id.into())),
        ("added".into(), Val::S64(added)),
    ])
}

/// A host function that answers `answer` and counts its calls.
pub fn answering(answer: Vec<Val>) -> (HostFn, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&calls);
    let host: HostFn = std::sync::Arc::new(move |_| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(answer.clone())
    });
    (host, calls)
}

/// A `TodoList` with a slug of its own, as the component's WIT declares it.
pub fn list(id: &str, slug: &str, added: i64) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(id.into())),
        ("name".into(), Val::String(format!("List {id}"))),
        ("slug".into(), Val::String(slug.into())),
        ("added".into(), Val::S64(added)),
    ])
}

/// [`list`], as the adapter stores it.
pub fn list_json(id: &str, slug: &str, added: i64) -> serde_json::Value {
    serde_json::json!({"id": id, "name": format!("List {id}"), "slug": slug, "added": added})
}

/// An `Item` as the component's WIT declares it: `list`, `id`, `title`,
/// `done`, `count`, `added`, `note`.
pub fn item(list: &str, id: &str, done: bool, added: i64) -> Val {
    Val::Record(vec![
        ("list".into(), Val::String(list.into())),
        ("id".into(), Val::String(id.into())),
        ("title".into(), Val::String(format!("Item {id}"))),
        ("done".into(), Val::Bool(done)),
        ("count".into(), Val::S64(1)),
        ("added".into(), Val::S64(added)),
        ("note".into(), Val::Option(None)),
    ])
}

/// [`item`], as the adapter stores it.
pub fn item_json(list: &str, id: &str, done: bool, added: i64) -> serde_json::Value {
    serde_json::json!({
        "list": list, "id": id, "title": format!("Item {id}"), "done": done, "count": 1,
        "added": added, "note": null
    })
}

/// A node's grants, as topology JSON writes them.
pub fn grants(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// What platform imports were handed, by their key, in the order called.
pub type Handed = Arc<Mutex<Vec<(String, Vec<Val>)>>>;

/// Every platform import of `reader`'s contract (its outbox's invalidations,
/// ADR-0209), each answering nothing and recording what it was given, by its
/// key, in the order it was called.
pub fn platform(reader: &Reader) -> (BTreeMap<String, HostFn>, Handed) {
    let calls: Handed = Arc::default();
    let mut host = BTreeMap::new();
    for import in &reader.contract.imports {
        if pw_host::tables::is_table_interface(&import.interface) {
            continue;
        }
        let key = import.key();
        let (named, record) = (key.clone(), Arc::clone(&calls));
        let callback: HostFn = Arc::new(move |args| {
            record.lock().unwrap().push((named.clone(), args.to_vec()));
            Ok(Vec::new())
        });
        host.insert(key, callback);
    }
    (host, calls)
}
