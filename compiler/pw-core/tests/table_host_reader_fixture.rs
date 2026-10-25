//! Compile the D2 get/get-many readers and exercise one exact get HostFn.
//! MemoryTables binding and present-row/get-many execution remain separate.

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

    // Read the compiler's JSON contract through the runtime's independent
    // contract mirror, rather than passing compiler types into the host.
    let host_contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).expect("serialize compiled contracts"),
    )
    .expect("host accepts compiler-emitted contracts");
    let origin = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":["database.read<Lists>"]}]}"#,
    )
    .expect("read-enabled origin");
    let no_read = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":[]}]}"#,
    )
    .expect("origin without read grant");

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

        // Compiler bookkeeping is not proof of what Wasmtime will load.
        // Read imports from the compiled binary through the actual host engine.
        let actual_imports = pw_host::engine::imports_of(&compiled.component.bytes)
            .unwrap_or_else(|error| panic!("{component_id} fails Wasmtime: {error}"));
        assert_eq!(
            actual_imports,
            [key],
            "{component_id}: Wasmtime imports must be exactly the manifest's read"
        );

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

        // Admit the actual Wasmtime imports, not the compiler's claimed list.
        let host_contract = host_contracts
            .iter()
            .find(|contract| contract.component_id == component_id)
            .expect("host has the corresponding emitted contract");
        let admission = pw_host::admit(host_contract, &origin, "origin", &actual_imports);
        assert!(admission.is_admitted(), "{component_id}: {admission:?}");
        let granted =
            pw_host::Granted::from(&admission, &Default::default()).expect("admitted read grant");
        assert_eq!(
            pw_host::linkable(host_contract, &granted),
            actual_imports,
            "{component_id}: host must link only the declared read",
        );

        // A matching contract without the table read grant cannot run.
        let denied = pw_host::admit(host_contract, &no_read, "origin", &actual_imports);
        assert!(
            denied
                .refusals()
                .iter()
                .any(|reason| matches!(reason, pw_host::Refusal::Ungranted { .. })),
            "{component_id}: missing read grant must refuse admission: {denied:?}",
        );

        // Negative control: the sibling is another real compiled component,
        // but its get/get-many import is absent from this reader's contract.
        let sibling_id = if component_id == "tables.reader.ById" {
            "tables.reader.ByIds"
        } else {
            "tables.reader.ById"
        };
        let (_, sibling) = build
            .components
            .iter()
            .find(|(id, _)| id == sibling_id)
            .expect("the sibling reader component");
        let Built::Component {
            compiled: sibling_compiled,
            ..
        } = sibling
        else {
            panic!("{sibling_id} did not compile");
        };
        let sibling_imports = pw_host::engine::imports_of(&sibling_compiled.component.bytes)
            .expect("Wasmtime inspects sibling imports");
        let mismatch = pw_host::admit(host_contract, &origin, "origin", &sibling_imports);
        assert!(
            mismatch
                .refusals()
                .iter()
                .any(|reason| matches!(reason, pw_host::Refusal::UndeclaredImport { .. })),
            "{component_id}: sibling import must be undeclared: {mismatch:?}",
        );

        if component_id == "tables.reader.ById" {
            // Admission is not execution: exercise the real compiled guest
            // with exactly its declared get handler, but no memory adapter yet.
            let export = host_contract.exports[0]
                .component
                .as_ref()
                .expect("reader export has a component path");
            let path = [&export.interface[..], &export.function[..]];
            let args = [pw_host::engine::Val::String("missing".into())];

            // A capability grant alone must never supply a default/stub host.
            let unimplemented = pw_host::engine::call_within(
                &compiled.component.bytes,
                host_contract,
                &granted,
                &pw_host::Limits::unbounded(),
                &Default::default(),
                &path,
                &args,
            )
            .expect_err("get with no host callback must be refused");
            assert!(
                unimplemented.contains(key),
                "the missing exact read must be named: {unimplemented}"
            );

            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed = std::sync::Arc::clone(&calls);
            let handler: pw_host::engine::HostFn = std::sync::Arc::new(move |received| {
                assert_eq!(received, &[pw_host::engine::Val::String("missing".into())]);
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(vec![pw_host::engine::Val::Option(None)])
            });
            let exact_host = std::collections::BTreeMap::from([(key.to_owned(), handler)]);
            assert_eq!(
                pw_host::engine::call_within(
                    &compiled.component.bytes,
                    host_contract,
                    &granted,
                    &pw_host::Limits::unbounded(),
                    &exact_host,
                    &path,
                    &args,
                ),
                Ok(vec![pw_host::engine::Val::Option(None)]),
                "the compiled get reader must call the granted exact HostFn"
            );
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }
}
