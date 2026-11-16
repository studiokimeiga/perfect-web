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
    let prepared = pw_host::engine::Prepared::compile(&component.bytes).unwrap();
    let result = prepared.admit_with_tables_as(
        &component.contract,
        &component.granted(),
        &bound,
        &Caller::Nobody,
        &component.export(),
        &[],
    );
    assert!(
        result
            .as_ref()
            .is_err_and(|why| why.to_string().contains("upload ABI")),
        "root ABI: {result:?}"
    );
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

#[test]
fn a_host_database_table_is_a_separate_source_from_the_explicit_upload_source() {
    let program = asset_program();
    let mut table = serde_json::to_value(&program.tables).unwrap();
    let mut ordinary = table["tables"][0].clone();
    ordinary["name"] = json!("Plain");
    ordinary["interface"] = json!("pw:tables/plain");
    ordinary["assets"] = json!([]);
    ordinary["fields"]
        .as_array_mut()
        .unwrap()
        .retain(|f| f["name"] == "id");
    for function in ordinary["functions"].as_array_mut().unwrap() {
        function["import"] = json!(
            function["import"]
                .as_str()
                .unwrap()
                .replace("pw:tables/assets", "pw:tables/plain")
        );
    }
    // Same-source ordinary rows are a valid neighbor of asset-bearing rows.
    table["tables"].as_array_mut().unwrap().push(ordinary);
    assert!(
        pw_host::tables::Manifest::from_json(&table.to_string())
            .unwrap()
            .with_uploads(&program.uploads)
            .is_ok()
    );
    table["tables"][1]["source"] = serde_json::Value::Null;
    let result = pw_host::tables::Manifest::from_json(&table.to_string())
        .unwrap()
        .with_uploads(&program.uploads);
    assert!(
        matches!(result,Err(ref why) if why.contains("multiple sources")),
        "{result:?}"
    );
}

#[test]
fn link_only_instantiation_also_refuses_an_unbound_compiled_upload_import() {
    let program = asset_program();
    for name in ["Keep", "Attached"] {
        let component = program.component(name);
        let result = pw_host::engine::instantiate(
            &component.bytes,
            &component.contract,
            &component.granted(),
        );
        assert!(
            matches!(result,Err(ref why) if why.contains("upload gate")),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn the_runtime_upload_ceiling_matches_the_compiler_wire_policy() {
    assert_eq!(pw_host::assets::MOST_BYTES, pw_core::uploads::MOST_BYTES);
}

struct UploadAdmissionOnly<'a> {
    manifest: &'a pw_host::tables::Manifest,
    callbacks: Arc<AtomicUsize>,
}
impl pw_host::engine::CallTables for UploadAdmissionOnly<'_> {
    fn manifest(&self) -> &pw_host::tables::Manifest {
        self.manifest
    }
    fn functions(
        &self,
        _: &pw_host::ComponentContract,
        _: &pw_host::Granted,
    ) -> Result<BTreeMap<String, HostFn>, String> {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        Err("authorization-only fetched body callbacks".into())
    }
    fn predicate_reads(
        &self,
        _: &pw_host::ComponentContract,
        _: &pw_host::Granted,
    ) -> Result<BTreeMap<String, HostFn>, String> {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        Err("fixture has no predicates".into())
    }
}
#[test]
fn authorization_only_upload_reuse_checks_root_binding_without_running_a_body() {
    let program = asset_program();
    let component = program.component("Attached");
    let prepared = pw_host::engine::Prepared::compile(&component.bytes).unwrap();
    let predicates = pw_host::engine::Predicates::new([]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .unwrap();
    for (manifest, allowed) in [(&bound, true), (&program.manifest, false)] {
        let state = UploadAdmissionOnly {
            manifest,
            callbacks: Arc::clone(&calls),
        };
        let result = prepared.authorize_with_tables_as(
            &component.contract,
            &component.granted(),
            &pw_host::Limits::unbounded(),
            &BTreeMap::new(),
            &state,
            &predicates,
            &Caller::Nobody,
            &component.export(),
            &[Val::String("request-session".into())],
            |_, _| Ok(false),
        );
        assert_eq!(
            result.is_ok(),
            allowed,
            "authorization-only root binding: {result:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn authorization_only_uses_the_actual_root_grant_and_contract_axes() {
    let program = asset_program();
    let component = program.component("Attached");
    let prepared = pw_host::engine::Prepared::compile(&component.bytes).unwrap();
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = UploadAdmissionOnly {
        manifest: &bound,
        callbacks: Arc::clone(&calls),
    };
    let predicates = pw_host::engine::Predicates::new([]).unwrap();
    let mut missing = component.contract.clone();
    missing.required_capabilities.clear();
    for (contract, granted) in [
        (&missing, component.granted()),
        (
            &component.contract,
            upload_grants_without(component, "database.read<PostImage>"),
        ),
    ] {
        let result = prepared.authorize_with_tables_as(
            contract,
            &granted,
            &pw_host::Limits::unbounded(),
            &BTreeMap::new(),
            &state,
            &predicates,
            &Caller::Nobody,
            &component.export(),
            &[Val::String("request-session".into())],
            |_, _| Ok(false),
        );
        assert!(
            result
                .as_ref()
                .is_err_and(|why| why.to_string().contains("upload gate")),
            "{result:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn empty_session_inputs_and_empty_claim_answers_are_refused_before_the_next_callback() {
    let program = asset_program();
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let attached = program.component("Attached");
    let hooks = upload_hooks(attached, &calls);
    let result = pw_host::engine::call_with_tables_as(
        &attached.bytes,
        &attached.contract,
        &attached.granted(),
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &attached.export(),
        &[Val::String(String::new())],
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("nonempty handles")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let keep = program.component("Keep");
    let mut hooks = upload_hooks(keep, &calls);
    let called = Arc::clone(&calls);
    hooks.insert(
        "pw:uploads/post-image#claim".into(),
        Arc::new(move |_: &[Val]| {
            called.fetch_add(1, Ordering::SeqCst);
            let value = Val::String(String::new());
            Ok(vec![Val::Result(Ok(Some(Box::new(value))))])
        }),
    );
    let result = pw_host::engine::call_with_tables_as(
        &keep.bytes,
        &keep.contract,
        &keep.granted(),
        &pw_host::Limits::unbounded(),
        &hooks,
        &bound,
        &Caller::Nobody,
        &keep.export(),
        &[],
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("claim answer is empty")),
        "{result:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "empty claim never reaches publish"
    );
}

#[test]
fn otherwise_valid_owned_data_cannot_be_forged_into_a_public_asset_binding() {
    let uploads = asset_program().uploads;
    let program = built_from(&[
        (
            "PrivateAssets.pw",
            r#"module PrivateAssets
import capability.{ UserId }
type Kind = Png | Jpeg
type Data = Data { key: String, generation: String, kind: Kind, width: Int, height: Int }
type Row = Row { owner: UserId, id: Int, image: Option<Data> }
table PrivateAssets
    row Row
    key owner, id
    owned by owner
"#,
        ),
        (
            "AssetData.pw",
            r#"module AssetData
import PrivateAssets
source AssetData
    holds PrivateAssets
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
        ),
        (
            "app.pw",
            r#"module owned.app
public query Noop()->Int
    freshness 0.seconds
    consistency snapshot
    cache shared
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{ 1 }
"#,
        ),
    ]);
    // Plain owned data with the same structural fields is otherwise valid;
    // declaring a public upload in this source grants it no asset association.
    assert!(program.manifest.clone().with_uploads(&uploads).is_ok());
    let mut tables = serde_json::to_value(&program.tables).unwrap();
    tables["tables"][0]["assets"] = json!([{"field":"image","upload":"PostImage"}]);
    let result = pw_host::tables::Manifest::from_json(&tables.to_string())
        .unwrap()
        .with_uploads(&uploads);
    assert!(
        matches!(result,Err(ref why) if why.contains("asset tables must be public")),
        "{result:?}"
    );
}

fn ordinary_table_root_axes(missing_contract: bool) {
    let program = program();
    let query = program.component("Mine");
    let prepared = pw_host::engine::Prepared::compile(&query.bytes).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = UploadAdmissionOnly {
        manifest: &program.manifest,
        callbacks: Arc::clone(&calls),
    };
    let predicates = program.predicates();
    let full = query.granted();
    let args = [handle(ALICE)];
    assert!(
        prepared
            .authorize_with_tables_as(
                &query.contract,
                &full,
                &pw_host::Limits::unbounded(),
                &program.platform(ALICE),
                &state,
                &predicates,
                &as_user(ALICE),
                &query.export(),
                &args,
                |_, _| Ok(false)
            )
            .is_ok()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "full-grant authorization-only runs no body"
    );
    let mut contract = query.contract.clone();
    let granted = if missing_contract {
        contract
            .required_capabilities
            .retain(|cap| cap.name() != "database.read<Notes>");
        full
    } else {
        upload_grants_without(query, "database.read<Notes>")
    };
    let result = prepared.authorize_with_tables_as(
        &contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &program.platform(ALICE),
        &state,
        &predicates,
        &as_user(ALICE),
        &query.export(),
        &args,
        |_, _| Ok(false),
    );
    assert!(
        result
            .as_ref()
            .is_err_and(|why| why.to_string().contains("database.read<Notes>")),
        "ordinary table root axis contract={missing_contract}: {result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let host = tables
        .read_host_fns(&query.contract, &query.granted())
        .unwrap();
    let result = pw_host::engine::call_with_tables_as(
        &query.bytes,
        &contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(ALICE),
        &query.export(),
        &args,
    );
    assert!(
        result.is_err(),
        "fresh execution also refuses the missing axis"
    );
}
#[test]
fn authorization_only_ordinary_tables_require_the_actual_root_handle() {
    ordinary_table_root_axes(false);
}
#[test]
fn authorization_only_ordinary_tables_require_the_root_contract_capability() {
    ordinary_table_root_axes(true);
}

#[test]
fn ordinary_table_primary_identity_is_checked_even_with_both_real_capabilities() {
    let program = program();
    let query = program.component("Mine");
    let mut contract = query.contract.clone();
    contract.required_capabilities.push(pw_host::Capability {
        family: "database".into(),
        operation: "read".into(),
        argument: Some("Other".into()),
    });
    let grants = Component {
        bytes: query.bytes.clone(),
        contract: contract.clone(),
        imports: query.imports.clone(),
    }
    .granted();
    assert!(
        program.manifest.admit_reads(&contract, &grants).is_ok(),
        "both grants do not invalidate an honest primary identity"
    );
    let import = contract
        .imports
        .iter_mut()
        .find(|i| pw_host::tables::is_table_interface(&i.interface))
        .unwrap();
    import.capability = "database.read<Other>".into();
    let result = program.manifest.admit_reads(&contract, &grants);
    assert!(
        matches!(result,Err(ref why) if why.contains("contract claims") && why.contains("database.read<Notes>")),
        "{result:?}"
    );
}
