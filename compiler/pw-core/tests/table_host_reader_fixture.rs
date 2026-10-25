//! Compile the D2 get/get-many reader fixture without running a host.
//! The separate Wasmtime integration must later invoke these actual components
//! using only manifest-declared, admitted bindings.

use pw_core::backend::component::Built;
use pw_core::check::Unit;
use pw_core::contract::Ownership;
use pw_core::tables::Primitive;

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

#[test]
fn compiled_table_readers_have_exact_manifest_and_contract_imports() {
    let build = pw_core::build::build(&units()).expect("the table reader builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());

    let table = build
        .tables
        .tables
        .iter()
        .find(|table| table.schema.name == "Lists")
        .expect("compiler emitted Lists in tables.json");

    for (component_id, key, primitive) in [
        ("tables.reader.ById", "pw:tables/lists#get", Primitive::Get),
        (
            "tables.reader.ByIds",
            "pw:tables/lists#get-many",
            Primitive::GetMany,
        ),
    ] {
        let function = table
            .functions
            .iter()
            .find(|function| function.import == key)
            .expect("table manifest declares the read");
        assert_eq!(function.primitive, primitive);
        assert_eq!(function.effect, "read");

        let (_, artifact) = build
            .components
            .iter()
            .find(|(id, _)| id == component_id)
            .expect("reader component is emitted");
        let Built::Component { compiled, audited } = artifact else {
            panic!("{component_id} did not compile: {artifact:?}");
        };
        assert!(!compiled.component.bytes.is_empty());
        assert!(*audited > 0);
        assert_eq!(compiled.component.imports, [key], "{component_id}");

        let contract = build
            .contracts
            .iter()
            .find(|contract| contract.component_id == component_id)
            .expect("compiler emitted the reader's component contract");
        assert_eq!(
            contract
                .imports
                .iter()
                .map(|import| import.key())
                .collect::<Vec<_>>(),
            [key],
            "{component_id} must not receive neighboring or wildcard imports",
        );
        let read = &contract.imports[0];
        assert_eq!(read.capability, "database.read<Lists>");
        assert_eq!(read.owner, Ownership::Platform);
        assert!(
            contract
                .required_capabilities
                .iter()
                .any(|capability| capability.name() == "database.read<Lists>"),
            "{component_id} must declare its read capability"
        );
    }
}
