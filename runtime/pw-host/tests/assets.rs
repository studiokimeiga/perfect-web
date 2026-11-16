//! Independent generic upload metadata/grant axes, before any callback is installed.
use pw_host::{assets::*, *};
use serde_json::{Value, json};

/// **A refusal held to its reason** (L5a's review, finding L9): `result` is
/// an error whose words, as its `Debug` shows them, hold `why`. A refusal for
/// any other reason, or none, fails here.
fn refused<T, E: std::fmt::Debug>(result: Result<T, E>, why: &str) {
    let said = result.err().map(|e| format!("{e:?}"));
    assert!(
        said.as_deref().is_some_and(|said| said.contains(why)),
        "a refusal saying {why:?}, where it said {said:?}"
    );
}
fn declaration() -> Value {
    json!({"name":"PostImage","module":"PostImage","source":"PhotoData","route":"/uploads/post-image","serves":"/images","max_bytes":5000000,"types":["png","jpeg"],"max_width":4096,"max_height":4096})
}
fn manifest() -> Manifest {
    Manifest::from_json(&json!([declaration()]).to_string()).unwrap()
}
fn contract(imports: Value, caps: &[String]) -> ComponentContract {
    let required = caps
        .iter()
        .map(|name| {
            let (head, arg) = name.split_once('<').unwrap();
            let (family, operation) = head.split_once('.').unwrap();
            json!({"family":family,"operation":operation,"argument":arg.trim_end_matches('>')})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"component_id":"photos.Keep","capability_mapping":CAPABILITY_MAPPING,"abi_schema":"abi","imports":imports,"required_capabilities":required,"allowed_placements":["origin"],"exports":[{"name":"Keep","kind":"command"}]})).unwrap()
}
fn grants(caps: &[String]) -> Granted {
    let contract = contract(json!([]), caps);
    let topology = Topology::from_json(
        &json!({"nodes":[{"name":"origin","world":"origin","grants":caps}]}).to_string(),
    )
    .unwrap();
    let admitted = admit(&contract, &topology, "origin", &[]);
    assert!(admitted.is_admitted(), "{admitted:?}");
    Granted::from(&admitted, &Default::default()).unwrap()
}
#[test]
fn upload_operations_keep_independent_database_and_resource_grants_in_both_axes() {
    let manifest = manifest();
    for (name, caps, operation) in [
        (
            "attached",
            vec!["database.read<PostImage>"],
            Operation::Attached,
        ),
        (
            "claim",
            vec![
                "database.read<PostImage>",
                "resource.acquire<PostImage.Claim>",
            ],
            Operation::Claim,
        ),
        (
            "publish",
            vec![
                "database.write<PostImage>",
                "resource.release<PostImage.Claim>",
            ],
            Operation::Publish,
        ),
        (
            "discard",
            vec![
                "database.write<PostImage>",
                "resource.release<PostImage.Claim>",
            ],
            Operation::Discard,
        ),
    ] {
        let caps = caps.into_iter().map(str::to_string).collect::<Vec<_>>();
        let actual = grants(&caps);
        let import =
            json!([{"interface":"pw:uploads/post-image","name":name,"capability":caps[0]}]);
        let contract = contract(import, &caps);
        let linked = gate_links(&contract, &actual, Some(&manifest), true).unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked.values().next().unwrap().operation(), operation);
        refused(
            gate_links(&contract, &actual, None, true),
            "upload gate: generic uploads are not bound",
        );
        if name == "attached" {
            assert!(gate_links(&contract, &actual, Some(&manifest), false).is_ok());
        } else {
            refused(
                gate_links(&contract, &actual, Some(&manifest), false),
                "upload gate: claims link only into commands",
            );
        }
        for i in 0..caps.len() {
            let mut missing = contract.clone();
            missing.required_capabilities.remove(i);
            // A missing contract axis and a missing actual one are each
            // told by the axis.
            let why = format!(
                "upload gate: {} is required in the contract and granted here",
                caps[i]
            );
            refused(gate_links(&missing, &actual, Some(&manifest), true), &why);
            let mut reduced = caps.clone();
            reduced.remove(i);
            refused(
                gate_links(&contract, &grants(&reduced), Some(&manifest), true),
                &why,
            );
        }
    }
}
#[test]
fn unused_or_forged_upload_imports_do_not_define_their_own_meaning() {
    let manifest = manifest();
    let caps = vec![
        "database.read<PostImage>".into(),
        "resource.acquire<PostImage.Claim>".into(),
    ];
    let actual = grants(&caps);
    let valid = json!({"interface":"pw:uploads/post-image","name":"claim","capability":caps[0]});
    for (field, value, why) in [
        (
            "interface",
            json!("pw:uploads/other"),
            "upload gate: no declaration for pw:uploads/other",
        ),
        (
            "name",
            json!("src"),
            "upload gate: no operation pw:uploads/post-image#src",
        ),
        (
            "capability",
            json!("database.read<Other>"),
            "upload gate: import claims another declaration's capability",
        ),
        (
            "kind",
            json!("component"),
            "upload gate: operation is not a platform import",
        ),
    ] {
        let mut import = valid.clone();
        import[field] = value;
        refused(
            gate_links(
                &contract(json!([import]), &caps),
                &actual,
                Some(&manifest),
                true,
            ),
            why,
        );
    }
    refused(
        gate_links(
            &contract(json!([valid.clone(), valid]), &caps),
            &actual,
            Some(&manifest),
            true,
        ),
        "upload gate: operation imported twice",
    );
    assert!(
        gate_links(&contract(json!([]), &[]), &grants(&[]), None, false)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn upload_metadata_rejects_legacy_identity_aliases_bad_limits_and_ambiguous_paths() {
    let valid = declaration();
    assert!(Manifest::from_json(&json!([valid.clone()]).to_string()).is_ok());
    let module = "PostImage is not a generic upload in its own module and explicit source";
    let limits = "PostImage has invalid upload limits or kinds";
    let paths = "PostImage has ambiguous or invalid upload paths";
    for (field, value, why) in [
        ("module", Value::Null, module),
        ("source", Value::Null, module),
        ("module", json!("Other"), module),
        ("types", json!(["png", "png"]), limits),
        ("types", json!(["svg"]), limits),
        ("max_bytes", json!(0), limits),
        ("max_width", json!(0), limits),
        ("route", json!("/../outside"), paths),
        ("serves", json!("/uploads/post-image"), paths),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        refused(Manifest::from_json(&json!([bad]).to_string()), why);
    }
    let mut alias = valid.clone();
    alias["name"] = json!("Post_Image");
    alias["module"] = json!("Post_Image");
    alias["route"] = json!("/uploads/alias");
    alias["serves"] = json!("/alias");
    refused(
        Manifest::from_json(&json!([valid.clone(), alias]).to_string()),
        "uploads.json repeats an upload declaration identity",
    );
    let mut other = valid.clone();
    other["name"] = json!("Avatar");
    other["module"] = json!("Avatar");
    other["route"] = json!("/uploads/avatar");
    refused(
        Manifest::from_json(&json!([valid.clone(), other.clone()]).to_string()),
        "uploads.json has overlapping upload paths",
    );
    other["serves"] = json!("/images/avatar");
    refused(
        Manifest::from_json(&json!([valid.clone(), other.clone()]).to_string()),
        "uploads.json has overlapping upload paths",
    );
    other["serves"] = json!("/avatars");
    assert!(Manifest::from_json(&json!([valid, other]).to_string()).is_ok());
}
#[test]
fn reference_shape_is_whole_data_and_declaration_limits_do_not_grant_a_lease() {
    let manifest = manifest();
    let declaration = manifest.declaration("PostImage").unwrap();
    let valid =
        json!({"key":"a".repeat(64),"generation":"b".repeat(32),"kind":"png","width":6,"height":4});
    let reference = Reference::from_value(&valid).unwrap();
    assert!(declaration.validate_reference(&reference).is_ok());
    let shape = "asset reference has invalid hash, generation, kind or dimensions";
    for (field, value, why) in [
        ("key", json!("A".repeat(64)), shape),
        ("generation", json!("../escape"), shape),
        ("kind", json!("Png"), shape),
        ("width", json!(0), shape),
        (
            "height",
            json!(-1),
            "invalid value: integer `-1`, expected u32",
        ),
        ("alt", json!("extra"), "unknown field `alt`"),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        refused(Reference::from_value(&bad), why);
    }
    for (field, value) in [("kind", json!("gif")), ("width", json!(4097))] {
        let mut bad = valid.clone();
        bad[field] = value;
        refused(
            declaration.validate_reference(&Reference::from_value(&bad).unwrap()),
            "PostImage reference exceeds its declared kind or dimensions",
        );
    }
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("generation");
    refused(
        Reference::from_value(&missing),
        "invalid asset reference: missing field `generation`",
    );
}

#[test]
fn the_host_rejects_a_forged_declaration_above_the_compilers_whole_body_ceiling() {
    for bytes in [1u64, 100_000_000] {
        let mut valid = declaration();
        valid["max_bytes"] = json!(bytes);
        assert!(Manifest::from_json(&json!([valid]).to_string()).is_ok());
    }
    for bytes in [100_000_001, u64::MAX] {
        let mut bad = declaration();
        bad["max_bytes"] = json!(bytes);
        refused(
            Manifest::from_json(&json!([bad]).to_string()),
            "PostImage has invalid upload limits or kinds",
        );
    }
}
#[test]
fn upload_wire_paths_use_the_compilers_literal_path_domain() {
    for path in ["/Images", "/images:private", "/imagés", "/a.b"] {
        let mut bad = declaration();
        bad["serves"] = json!(path);
        refused(
            Manifest::from_json(&json!([bad]).to_string()),
            "PostImage has ambiguous or invalid upload paths",
        );
    }
    let mut good = declaration();
    good["serves"] = json!("/a-z_09/images");
    assert!(Manifest::from_json(&json!([good]).to_string()).is_ok());
}

#[test]
fn versioned_and_bare_upload_namespaces_cannot_escape_declaration_admission() {
    let manifest = manifest();
    let caps = vec!["database.read<PostImage>".into()];
    let actual = grants(&caps);
    for interface in [
        "pw:uploads",
        "pw:uploads@1/post-image",
        "pw:uploads/post-image@1.0.0",
    ] {
        let contract = contract(
            json!([{"interface":interface,"name":"attached","capability":caps[0]}]),
            &caps,
        );
        refused(
            gate_links(&contract, &actual, Some(&manifest), false),
            &format!("upload gate: no declaration for {interface}"),
        );
        refused(
            gate_links(&contract, &actual, None, false),
            "upload gate: generic uploads are not bound",
        );
    }
}
