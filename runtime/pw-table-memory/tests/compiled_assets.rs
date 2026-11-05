//! A reference's pure projection needs no claim or data callback.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/assets.rs");

#[test]
fn a_compiled_reference_projects_the_existing_public_url_without_host_work() {
    let program = asset_program();
    let query = program.component("Url");
    assert!(
        query.imports.is_empty(),
        "pure URL projection must import no host operation"
    );
    let answer = pw_host::engine::call_with_tables_as(
        &query.bytes,
        &query.contract,
        &query.granted(),
        &pw_host::Limits::unbounded(),
        &BTreeMap::new(),
        &program.manifest,
        &Caller::Nobody,
        &query.export(),
        &[image_reference()],
    );
    assert_eq!(
        answer,
        Ok(vec![Val::String(format!("/images/{}.png", "a".repeat(64)))])
    );
}

#[test]
fn asset_metadata_is_not_silently_served_by_an_adapter_with_no_upload_binding() {
    let program = asset_program();
    let result = MemoryTables::new(program.tables);
    assert!(
        matches!(result,Err(pw_table_memory::Error::Schema(ref why)) if why.contains("asset")),
        "asset declarations need their actual storage binding"
    );
}

#[test]
fn actual_compiler_upload_metadata_binds_its_exact_public_reference_field() {
    let program = asset_program();
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .assert_ok("compiler upload binding");
    let image = bound.uploads().unwrap().declaration("PostImage").unwrap();
    assert_eq!(image.source(), "AssetData");
    assert_eq!(image.interface(), "pw:uploads/post-image");
    assert_eq!(image.kinds(), &["png", "jpeg"]);
    assert!(program.manifest.uploads().is_none(), "binding is explicit");
    let table = serde_json::to_value(&program.tables).unwrap();
    let upload = serde_json::from_str::<serde_json::Value>(&program.uploads).unwrap();
    let mut wrong_source = upload.clone();
    wrong_source[0]["source"] = json!("Another");
    assert!(
        program
            .manifest
            .clone()
            .with_uploads(&wrong_source.to_string())
            .is_err()
    );
    for (field, value) in [("field", json!("alt")), ("upload", json!("Other"))] {
        let mut bad = table.clone();
        bad["tables"][0]["assets"][0][field] = value;
        let manifest = pw_host::tables::Manifest::from_json(&bad.to_string()).unwrap();
        assert!(manifest.with_uploads(&program.uploads).is_err());
    }
    let mut duplicate = table.clone();
    let binding = duplicate["tables"][0]["assets"][0].clone();
    duplicate["tables"][0]["assets"]
        .as_array_mut()
        .unwrap()
        .push(binding);
    assert!(
        pw_host::tables::Manifest::from_json(&duplicate.to_string())
            .unwrap()
            .with_uploads(&program.uploads)
            .is_err()
    );
    let mut wrong_shape = table;
    wrong_shape["tables"][0]["fields"][1]["stored"] = json!({"option":{"string":{}}});
    assert!(
        pw_host::tables::Manifest::from_json(&wrong_shape.to_string())
            .unwrap()
            .with_uploads(&program.uploads)
            .is_err()
    );
}

#[test]
fn real_compiled_upload_imports_need_explicit_binding_before_any_resource_callback() {
    let program = asset_program();
    let component = program.component("Keep");
    let calls = Arc::new(AtomicUsize::new(0));
    let hooks = upload_hooks(component, &calls);
    let call = |manifest: &pw_host::tables::Manifest| {
        pw_host::engine::call_with_tables_as(
            &component.bytes,
            &component.contract,
            &component.granted(),
            &pw_host::Limits::unbounded(),
            &hooks,
            manifest,
            &Caller::Nobody,
            &component.export(),
            &[],
        )
    };
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .assert_ok("bind declarations");
    assert_eq!(
        call(&bound),
        Ok(vec![Val::Result(Ok(Some(Box::new(image_reference()))))])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    calls.store(0, Ordering::SeqCst);
    let result = call(&program.manifest);
    assert!(
        matches!(result,Err(ref why) if why.contains("upload gate")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn upload_grants_without(component: &Component, missing: &str) -> pw_host::Granted {
    let mut contract = component.contract.clone();
    contract.imports.clear();
    contract
        .required_capabilities
        .retain(|cap| cap.name() != missing);
    let caps = contract
        .required_capabilities
        .iter()
        .map(|c| c.name())
        .collect::<std::collections::BTreeSet<_>>();
    let topology = pw_host::Topology {
        nodes: vec![pw_host::Node {
            name: "origin".into(),
            world: "origin".into(),
            grants: caps,
        }],
    };
    let admitted = pw_host::admit(&contract, &topology, "origin", &[]);
    assert!(admitted.is_admitted());
    pw_host::Granted::from(&admitted, &BTreeMap::new()).unwrap()
}
#[test]
fn actual_compiled_resource_grants_and_callback_answers_are_checked_independently() {
    let program = asset_program();
    let component = program.component("Keep");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut hooks = upload_hooks(component, &calls);
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .unwrap();
    let full = component.granted();
    for capability in [
        "resource.acquire<PostImage.Claim>",
        "resource.release<PostImage.Claim>",
    ] {
        let mut missing = component.contract.clone();
        missing
            .required_capabilities
            .retain(|cap| cap.name() != capability);
        let result = pw_host::engine::call_with_tables_as(
            &component.bytes,
            &missing,
            &full,
            &pw_host::Limits::unbounded(),
            &hooks,
            &bound,
            &Caller::Nobody,
            &component.export(),
            &[],
        );
        assert!(
            matches!(result,Err(ref why) if why.contains("upload gate")),
            "{result:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let result = pw_host::engine::call_with_tables_as(
            &component.bytes,
            &component.contract,
            &upload_grants_without(component, capability),
            &pw_host::Limits::unbounded(),
            &hooks,
            &bound,
            &Caller::Nobody,
            &component.export(),
            &[],
        );
        assert!(
            matches!(result,Err(ref why) if why.contains("upload gate")),
            "{result:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    let called = Arc::clone(&calls);
    hooks.insert(
        "pw:uploads/post-image#publish".into(),
        Arc::new(move |_: &[Val]| {
            called.fetch_add(1, Ordering::SeqCst);
            let mut reference = image_reference();
            let Val::Record(fields) = &mut reference else {
                panic!("reference")
            };
            fields[3].1 = Val::S64(0);
            Ok(vec![Val::Result(Ok(Some(Box::new(reference))))])
        }),
    );
    let result = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &full,
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &component.export(),
        &[],
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("invalid hash, generation, kind or dimensions")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[test]
fn actual_upload_reference_abi_is_checked_before_any_claim_callback() {
    let program = asset_program();
    let component = program.component("Keep");
    let calls = Arc::new(AtomicUsize::new(0));
    let hooks = upload_hooks(component, &calls);
    let mut tables = serde_json::to_value(&program.tables).unwrap();
    let mut uploads = serde_json::from_str::<serde_json::Value>(&program.uploads).unwrap();
    uploads[0]["types"].as_array_mut().unwrap().reverse();
    tables["tables"][0]["fields"][1]["stored"]["option"]["record"][2]["stored"]["cases"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let bound = pw_host::tables::Manifest::from_json(&tables.to_string())
        .unwrap()
        .with_uploads(&uploads.to_string())
        .assert_ok("reordered metadata is internally consistent");
    let result = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &component.granted(),
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &component.export(),
        &[],
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("upload ABI") && why.contains("variant cases or payloads differ")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn compiled_attached_reads_validate_the_declared_record_without_resource_authority() {
    let program = asset_program();
    let component = program.component("Attached");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut hooks = upload_hooks(component, &calls);
    assert!(
        component
            .contract
            .required_capabilities
            .iter()
            .all(|cap| cap.family != "resource")
    );
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .unwrap();
    let expected = Val::List(vec![Val::Record(vec![
        (
            "src".into(),
            Val::String("/uploads/post-image/lease-token".into()),
        ),
        ("width".into(), Val::S64(6)),
        ("height".into(), Val::S64(4)),
    ])]);
    let result = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &component.granted(),
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &component.export(),
        &[Val::String("request-session".into())],
    );
    assert_eq!(result, Ok(vec![expected]));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    hooks.insert(
        "pw:uploads/post-image#attached".into(),
        Arc::new(|_: &[Val]| {
            Ok(vec![Val::List(vec![Val::Record(vec![
                (
                    "src".into(),
                    Val::String("/uploads/post-image/lease-token".into()),
                ),
                ("width".into(), Val::S64(-1)),
                ("height".into(), Val::S64(4)),
            ])])])
        }),
    );
    let result = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &component.granted(),
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &component.export(),
        &[Val::String("request-session".into())],
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("outside its bounds")),
        "{result:?}"
    );
}
#[test]
fn asset_binding_requires_its_own_explicit_source_even_when_only_one_is_named() {
    let program = asset_program();
    let mut table = serde_json::to_value(&program.tables).unwrap();
    table["tables"][0]["source"] = serde_json::Value::Null;
    assert!(
        pw_host::tables::Manifest::from_json(&table.to_string())
            .unwrap()
            .with_uploads(&program.uploads)
            .is_err()
    );
}
