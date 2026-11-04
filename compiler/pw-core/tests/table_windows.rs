//! Ordered windows of existing selectors keep their reader and set boundary.
use pw_core::check::Unit;

const SOURCE: &str = r#"module Windows
import capability.{ User, UserId }
type Entry = Entry { owner: UserId, bucket: Int, id: Int, recipient: UserId, stamp: Int, draft: String }
table Windows
    row Entry
    key owner, bucket, id
    order stamp
    index by_owner_bucket on owner, bucket
    index by_recipient_bucket on recipient, bucket
    owned by owner
    shares with recipient
    private draft
source WindowData
    holds Windows
    transactions serializable
    reads strong
    changes none
    sets one_call
fn first(reader: User<UserId>, n: Int) -> List<Entry> !{ database.read<Windows> } {
    Windows.first_by_owner(reader, n)
}
fn latest_shared(reader: User<UserId>, bucket: Int, n: Int) -> List<EntrySeen> !{ database.read<Windows> } {
    Windows.last_by_recipient_bucket(reader, bucket, n)
}
fn each_bucket(reader: User<UserId>, buckets: List<Int>, n: Int) -> List<Entry> !{ database.read<Windows> } {
    Windows.last_by_owner_bucket_many(reader, buckets, n)
}
"#;

fn units(source: &str) -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = vec![("Windows.pw".to_string(), source.to_string())];
    for package in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths = std::fs::read_dir(root.join(package))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pw"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            sources.push((
                path.display().to_string(),
                std::fs::read_to_string(path).unwrap(),
            ));
        }
    }
    sources
        .into_iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(&src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path,
                hir: pw_core::lower::lower_file(&src, &parsed.green),
                src,
            }
        })
        .collect()
}

#[test]
fn windows_compile_as_owned_and_shared_bounded_reads_of_existing_selectors() {
    let build = pw_core::build::build(&units(SOURCE));
    assert!(build.is_ok(), "windows build: {:?}", build.as_ref().err());
    let tables = serde_json::to_value(build.unwrap().tables).unwrap();
    let functions = tables["tables"][0]["functions"].as_array().unwrap();
    for (name, primitive, by, reader, returns) in [
        (
            "first_by_owner",
            "first",
            vec!["owner"],
            "owner",
            "List<Entry>",
        ),
        (
            "last_by_recipient_bucket",
            "last",
            vec!["recipient", "bucket"],
            "recipient",
            "List<EntrySeen>",
        ),
        (
            "last_by_owner_bucket_many",
            "last-many",
            vec!["owner", "bucket"],
            "owner",
            "List<Entry>",
        ),
    ] {
        let function = functions.iter().find(|f| f["name"] == name).unwrap();
        assert_eq!(function["primitive"], primitive);
        assert_eq!(function["effect"], "read");
        assert_eq!(function["by"], serde_json::json!(by));
        assert_eq!(function["reader"], reader);
        assert_eq!(function["returns"], returns);
    }
    let grouped = functions
        .iter()
        .find(|f| f["name"] == "last_by_owner_bucket_many")
        .unwrap();
    assert_eq!(
        grouped["sorted_by"],
        serde_json::json!(["owner", "bucket", "stamp", "id"])
    );
    assert!(
        !functions.iter().any(|f| [
            "first",
            "last",
            "first_by_owner_many",
            "last_by_owner_many",
            "last_by_id"
        ]
        .contains(&f["name"].as_str().unwrap())),
        "windows add neither global/non-reader reads nor empty owned set selectors"
    );
}

#[test]
fn window_bounds_do_not_alias_a_selector_parameter_named_limit() {
    let source = SOURCE.replace("bucket", "limit");
    let build = pw_core::build::build(&units(&source));
    assert!(
        build.is_ok(),
        "selector and bound parameter stay distinct: {:?}",
        build.err()
    );
}

#[test]
fn window_reads_still_require_the_readers_handle() {
    let source = SOURCE.replace("fn first(reader: User<UserId>", "fn first(reader: UserId");
    let why = pw_core::build::build(&units(&source)).err();
    assert!(
        why.as_ref()
            .is_some_and(|why| why.contains("argument 1 of `Windows.first_by_owner`")
                && why.contains("capability.User")),
        "plain id is not a window reader: {why:?}"
    );
}

#[test]
fn windows_inside_element_dependent_loops_keep_the_round_trip_refusal() {
    for (parameters, body, set) in [
        (
            "reader: User<UserId>, buckets: List<Int>, n: Int",
            "List.map(buckets, bucket => Windows.last_by_owner_bucket(reader, bucket, n))",
            Some("last_by_owner_bucket_many"),
        ),
        (
            "reader: User<UserId>, limits: List<Int>",
            "List.map(limits, n => Windows.last_by_owner(reader, n))",
            None,
        ),
    ] {
        let source = SOURCE.replace("import capability", "import List\nimport capability")
            + &format!(
                "\nfn per({parameters}) -> List<List<Entry>> !{{ database.read<Windows> }} {{\n    {body}\n}}\n"
            );
        let sources = units(&source)
            .into_iter()
            .map(|u| (u.path, u.src))
            .collect::<Vec<_>>();
        let found = pw_core::check::check_sources(&sources)
            .into_iter()
            .flat_map(|(_, ds)| ds)
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].code, "PW0405", "{found:#?}");
        if let Some(set) = set {
            assert!(
                found[0].repairs.iter().any(|r| r.description.contains(set)),
                "{found:#?}"
            );
        } else {
            assert!(
                !found[0]
                    .repairs
                    .iter()
                    .any(|r| r.description.contains("last_by_owner_many")),
                "an owned empty-selector set form does not exist: {found:#?}"
            );
        }
    }
}

#[test]
fn an_owned_readers_field_name_cannot_alias_generated_parameters() {
    for owner in ["values", "keys", "row", "limit"] {
        let mut source = SOURCE.replace("owner", owner);
        source.push_str(&format!("\nevent WindowChanged(who: UserId)\nuser query Latest(reader: User<UserId>, buckets: List<Int>, n: Int) -> List<Entry>\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key reader, buckets, n\n    invalidates_on WindowChanged(reader)\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    Windows.last_by_{owner}_bucket_many(reader, buckets, n)\n}}\n"));
        let units = units(&source);
        let own = units.iter().find(|u| u.path == "Windows.pw").unwrap();
        for (id, declaration) in own.hir.all_decls() {
            if !own.hir.generated.contains_key(&id) {
                continue;
            }
            let params = declaration
                .params
                .iter()
                .map(|p| pw_core::wit::ident(&p.name))
                .collect::<Vec<_>>();
            let unique = params.iter().collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                unique.len(),
                params.len(),
                "owner {owner}, {} gives every generated parameter a distinct emitted name: {params:?}",
                declaration.name
            );
        }
        let build = pw_core::build::build(&units);
        assert!(
            build.is_ok(),
            "an owned field named {owner} is valid: {:?}",
            build.as_ref().err()
        );
        assert!(
            build
                .unwrap()
                .components
                .iter()
                .any(|(id, artifact)| id == "Windows.Latest"
                    && matches!(
                        artifact,
                        pw_core::backend::component::Built::Component { .. }
                    )),
            "the differently named reader/list/limit cross a real WIT import"
        );
    }
}
