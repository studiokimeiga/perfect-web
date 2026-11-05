//! Independent generic upload metadata/grant axes, before any callback is installed.
use pw_host::{assets::*, *};
use serde_json::{Value, json};
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
        assert!(
            gate_links(&contract, &actual, None, true).is_err(),
            "unbound import {name}"
        );
        if name == "attached" {
            assert!(gate_links(&contract, &actual, Some(&manifest), false).is_ok());
        } else {
            assert!(gate_links(&contract, &actual, Some(&manifest), false).is_err());
        }
        for i in 0..caps.len() {
            let mut missing = contract.clone();
            missing.required_capabilities.remove(i);
            assert!(
                gate_links(&missing, &actual, Some(&manifest), true).is_err(),
                "missing contract axis {name} {}",
                caps[i]
            );
            let mut reduced = caps.clone();
            reduced.remove(i);
            assert!(
                gate_links(&contract, &grants(&reduced), Some(&manifest), true).is_err(),
                "missing actual axis {name} {}",
                caps[i]
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
    for (field, value) in [
        ("interface", json!("pw:uploads/other")),
        ("name", json!("src")),
        ("capability", json!("database.read<Other>")),
        ("kind", json!("component")),
    ] {
        let mut import = valid.clone();
        import[field] = value;
        assert!(
            gate_links(
                &contract(json!([import]), &caps),
                &actual,
                Some(&manifest),
                true
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(
        gate_links(
            &contract(json!([valid.clone(), valid]), &caps),
            &actual,
            Some(&manifest),
            true
        )
        .is_err()
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
    for (field, value) in [
        ("module", Value::Null),
        ("source", Value::Null),
        ("module", json!("Other")),
        ("types", json!(["png", "png"])),
        ("types", json!(["svg"])),
        ("max_bytes", json!(0)),
        ("max_width", json!(0)),
        ("route", json!("/../outside")),
        ("serves", json!("/uploads/post-image")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(
            Manifest::from_json(&json!([bad]).to_string()).is_err(),
            "{field}"
        );
    }
    let mut alias = valid.clone();
    alias["name"] = json!("Post_Image");
    alias["module"] = json!("Post_Image");
    alias["route"] = json!("/uploads/alias");
    alias["serves"] = json!("/alias");
    assert!(Manifest::from_json(&json!([valid.clone(), alias]).to_string()).is_err());
    let mut other = valid.clone();
    other["name"] = json!("Avatar");
    other["module"] = json!("Avatar");
    other["route"] = json!("/uploads/avatar");
    assert!(Manifest::from_json(&json!([valid.clone(), other.clone()]).to_string()).is_err());
    other["serves"] = json!("/images/avatar");
    assert!(Manifest::from_json(&json!([valid.clone(), other.clone()]).to_string()).is_err());
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
    for (field, value) in [
        ("key", json!("A".repeat(64))),
        ("generation", json!("../escape")),
        ("kind", json!("Png")),
        ("width", json!(0)),
        ("height", json!(-1)),
        ("alt", json!("extra")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(Reference::from_value(&bad).is_err(), "{field}");
    }
    for (field, value) in [("kind", json!("gif")), ("width", json!(4097))] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(
            declaration
                .validate_reference(&Reference::from_value(&bad).unwrap())
                .is_err()
        );
    }
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("generation");
    assert!(Reference::from_value(&missing).is_err());
}
