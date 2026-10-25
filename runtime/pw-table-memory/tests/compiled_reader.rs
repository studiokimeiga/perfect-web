//! A compiled component reads the memory adapter through exactly the table
//! host functions its admitted contract names (ADR-0338 D2, issue #27).
//!
//! The pieces were written apart: the adapter's import gate and selector
//! checks (PRs #29, #33), the host's WIT selector and row codec and its
//! callback (#30 to #32), and the compiled readers' admission and Wasmtime
//! calls with test callbacks (#34 to #40). Here they meet: the compiler's
//! own `tables.reader` components, admitted by the imports Wasmtime reads
//! from their bytes, call `MemoryTables` through
//! [`MemoryTables::read_host_fns`], and Wasmtime holds every value the host
//! returns to the component's own WIT types.

use std::collections::BTreeMap;
use std::sync::Arc;

use pw_core::backend::component::Built;
use pw_core::check::Unit;
use pw_host::engine::Val;
use pw_table_memory::{Error, MemoryTables};
use serde_json::json;

fn units() -> Vec<Unit> {
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
    for name in ["Lists.pw", "Items.pw", "ListData.pw", "reader.pw"] {
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

/// One compiled reader: its bytes, its contract as the host reads it, and
/// the imports Wasmtime finds in its bytes.
struct Reader {
    bytes: Vec<u8>,
    contract: pw_host::ComponentContract,
    imports: Vec<String>,
}

impl Reader {
    fn export(&self) -> [&str; 2] {
        let export = self.contract.exports[0]
            .component
            .as_ref()
            .expect("a reader's export has a component path");
        [&export.interface[..], &export.function[..]]
    }

    /// Admitted on a node granting `grants`, by the imports in its bytes.
    fn admitted(&self, contract: &pw_host::ComponentContract, grants: &str) -> pw_host::Granted {
        let topology = pw_host::Topology::from_json(&format!(
            r#"{{"nodes":[{{"name":"origin","world":"origin","grants":[{grants}]}}]}}"#
        ))
        .expect("topology");
        let admission = pw_host::admit(contract, &topology, "origin", &self.imports);
        assert!(admission.is_admitted(), "{admission:?}");
        pw_host::Granted::from(&admission, &Default::default()).expect("admitted")
    }

    fn call(
        &self,
        granted: &pw_host::Granted,
        host: &BTreeMap<String, pw_host::engine::HostFn>,
        args: &[Val],
    ) -> Result<Vec<Val>, String> {
        pw_host::engine::call_within(
            &self.bytes,
            &self.contract,
            granted,
            &pw_host::Limits::unbounded(),
            host,
            &self.export(),
            args,
        )
    }
}

/// The program's `tables.json` in a memory adapter holding three lists, and
/// its two compiled readers, by component id.
fn program() -> (Arc<MemoryTables>, BTreeMap<String, Reader>) {
    let build = pw_core::build::build(&units()).expect("the table readers build");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).expect("serialize compiled contracts"),
    )
    .expect("the host reads the compiler's contracts");
    let mut readers = BTreeMap::new();
    for id in ["tables.reader.ById", "tables.reader.ByIds"] {
        let (_, artifact) = build
            .components
            .iter()
            .find(|(component, _)| component == id)
            .expect("the reader is emitted");
        let Built::Component { compiled, .. } = artifact else {
            panic!("{id} did not compile: {artifact:?}");
        };
        let bytes = compiled.component.bytes.clone();
        let imports = pw_host::engine::imports_of(&bytes).expect("Wasmtime reads its imports");
        let contract = contracts
            .iter()
            .find(|contract| contract.component_id == id)
            .expect("the reader's contract")
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

    let tables = Arc::new(MemoryTables::new(build.tables.clone()).expect("the adapter"));
    tables
        .transaction(|tx| {
            for (id, name, added) in [("list-b", "List B", 20), ("list-a", "List A", 10)] {
                tx.write(
                    "pw:tables/lists#insert",
                    &[json!({"id": id, "name": name, "slug": id, "added": added})],
                )?;
            }
            tx.write(
                "pw:tables/lists#insert",
                &[json!({"id": "list-c", "name": "List C", "slug": "list-c", "added": 5})],
            )
        })
        .expect("seeded through the adapter's own transaction");
    (tables, readers)
}

/// A `TodoList` as the component's WIT declares it: its fields in their
/// declared order, not the order of a JSON object's keys.
fn todo_list(id: &str, name: &str, added: i64) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(id.into())),
        ("name".into(), Val::String(name.into())),
        ("slug".into(), Val::String(id.into())),
        ("added".into(), Val::S64(added)),
    ])
}

const READ_LISTS: &str = r#""database.read<Lists>""#;

#[test]
fn compiled_get_reads_present_and_missing_rows_from_the_memory_adapter() {
    let (tables, readers) = program();
    let reader = &readers["tables.reader.ById"];
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let host = tables
        .read_host_fns(&reader.contract, &granted)
        .expect("the admitted get is served");
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        ["pw:tables/lists#get"],
        "exactly the reader's own import, not its table's other reads"
    );

    assert_eq!(
        reader.call(&granted, &host, &[Val::String("list-a".into())]),
        Ok(vec![Val::Option(Some(Box::new(todo_list(
            "list-a", "List A", 10
        ))))]),
        "a present key answers the stored row, typed as the WIT record"
    );
    assert_eq!(
        reader.call(&granted, &host, &[Val::String("missing".into())]),
        Ok(vec![Val::Option(None)]),
        "a missing key answers none, not a default row"
    );
}

#[test]
fn compiled_get_many_answers_the_declared_order_once_per_present_key() {
    let (tables, readers) = program();
    let reader = &readers["tables.reader.ByIds"];
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let host = tables
        .read_host_fns(&reader.contract, &granted)
        .expect("the admitted get-many is served");
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        ["pw:tables/lists#get-many"]
    );

    let keys = ["list-b", "missing", "list-a", "list-b"]
        .map(|id| Val::String(id.into()))
        .to_vec();
    assert_eq!(
        reader.call(&granted, &host, &[Val::List(keys)]),
        Ok(vec![Val::List(vec![
            todo_list("list-a", "List A", 10),
            todo_list("list-b", "List B", 20),
        ])]),
        "the declared order (by key), each present row once, a missing key none"
    );
    assert_eq!(
        reader.call(&granted, &host, &[Val::List(vec![])]),
        Ok(vec![Val::List(vec![])])
    );
}

#[test]
fn no_table_host_function_without_the_tables_read_grant() {
    let (tables, readers) = program();
    let reader = &readers["tables.reader.ById"];
    // The real contract, its requirement removed so that a node granting
    // nothing admits it: what is admitted then holds no read of Lists, and
    // the adapter installs nothing for it.
    let mut ungranted = reader.contract.clone();
    ungranted.required_capabilities.clear();
    let granted = reader.admitted(&ungranted, "");
    assert!(matches!(
        tables.read_host_fns(&ungranted, &granted),
        Err(Error::Input(reason)) if reason.contains("pw:tables/lists#get")
    ));
    // Nor may the host link the read some other way: with no callback for
    // the granted import, the call is refused rather than stubbed.
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    assert!(
        reader
            .call(&granted, &BTreeMap::new(), &[Val::String("list-a".into())])
            .is_err()
    );
}
