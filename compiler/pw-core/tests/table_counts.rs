//! Aggregates of declared selectors keep the same authority and set boundary.
use pw_core::check::Unit;

const OWNED: &str = r#"module Counts
import capability.{ User, UserId }
type Entry = Entry { owner: UserId, id: Int, to: UserId, flag: Bool, text: String }
table Counts
    row Entry
    key owner, id
    index by_to on to, flag
    owned by owner
    shares with to
    private text
source CountData
    holds Counts
    transactions serializable
    reads strong
    changes none
    sets one_call
fn mine(reader: User<UserId>) -> Int !{ database.read<Counts> } {
    Counts.count_by_owner(reader)
}
fn to_me(reader: User<UserId>, flag: Bool) -> Int !{ database.read<Counts> } {
    Counts.count_by_to_flag(reader, flag)
}
fn several(reader: User<UserId>, flags: List<Bool>) -> List<CountsToFlagCount> !{ database.read<Counts> } {
    Counts.count_by_to_flag_many(reader, flags)
}
"#;

fn units(source: &str) -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = vec![("Counts.pw".to_string(), source.to_string())];
    for package in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(package))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pw"))
            .collect();
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
fn counts_are_compiled_reads_of_the_same_owned_and_shared_selectors() {
    let program = units(OWNED);
    let build = pw_core::build::build(&program);
    assert!(build.is_ok(), "counts build: {:?}", build.as_ref().err());
    let tables = serde_json::to_value(build.unwrap().tables).unwrap();
    let functions = tables["tables"][0]["functions"].as_array().unwrap();
    let own = functions.iter().find(|f| f["name"] == "count_by_owner");
    assert!(own.is_some(), "owned count is generated");
    let own = own.unwrap();
    assert_eq!(own["reader"], "owner");
    assert_eq!(own["effect"], "read");
    assert_eq!(own["returns"], "Int");
    let shared = functions
        .iter()
        .find(|f| f["name"] == "count_by_to_flag_many");
    assert!(shared.is_some(), "shared set aggregate is generated");
    let shared = shared.unwrap();
    assert_eq!(shared["reader"], "to");
    assert_eq!(shared["primitive"], "count-many");
    assert_eq!(shared["by"], serde_json::json!(["to", "flag"]));
    assert!(
        shared["returns"]
            .as_str()
            .unwrap()
            .contains("CountsToFlagCount")
    );
    assert!(
        !functions.iter().any(|f| f["name"] == "count"
            || f["name"] == "count_by_id"
            || f["name"] == "count_by_owner_many"),
        "counts add no global or non-reader selector"
    );
}

#[test]
fn an_owned_count_does_not_accept_a_plain_user_id() {
    let source = OWNED.replace("fn mine(reader: User<UserId>)", "fn mine(reader: UserId)");
    let build = pw_core::build::build(&units(&source));
    let why = build.err();
    assert!(
        why.as_ref()
            .is_some_and(|why| why.contains("argument 1 of `Counts.count_by_owner`")
                && why.contains("capability.User")),
        "plain id is refused at its reader argument: {why:?}"
    );
}

#[test]
fn counting_each_loop_element_is_refused_with_its_generated_set_repair() {
    let source = OWNED.replace("import capability", "import List\nimport capability")
        + r#"
fn per(reader: User<UserId>, ids: List<Int>) -> List<Int> !{ database.read<Counts> } {
    List.map(ids, id => Counts.count_by_owner_id(reader, id))
}
"#;
    let units = units(&source);
    let sources = units
        .iter()
        .map(|u| (u.path.clone(), u.src.clone()))
        .collect::<Vec<_>>();
    let diagnostics = pw_core::check::check_sources(&sources)
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.code, "PW0405");
    assert!(
        diagnostic
            .repairs
            .iter()
            .any(|r| r.description.contains("count_by_owner_id_many")),
        "{diagnostic:#?}"
    );
}

#[test]
fn count_scalar_and_set_names_cannot_silently_replace_each_other() {
    let source = r#"module CountNames
type Entry = Entry { a: Int, id: Int, a_many: Int }
table CountNames
    row Entry
    key a, id
    index by_other on a_many
source CountData
    holds CountNames
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;
    for source in [
        source.to_string(),
        source
            .replace("key a, id", "key a_many, id")
            .replace("on a_many", "on a"),
    ] {
        let sources = units(&source)
            .into_iter()
            .map(|u| (u.path, u.src))
            .collect::<Vec<_>>();
        let found = pw_core::check::check_sources(&sources)
            .into_iter()
            .flat_map(|(_, ds)| ds)
            .collect::<Vec<_>>();
        assert!(
            found.iter().any(|d| d.code == "PW0361"
                && d.message.contains("count_by_a_many")
                && d.message.contains("same import")),
            "every count scalar/set alias is refused: {found:#?}"
        );
        let valid = source.replace("a_many", "a_total");
        let built = pw_core::build::build(&units(&valid));
        assert!(built.is_ok(), "renamed selector builds: {:?}", built.err());
        let functions = &built.unwrap().tables.tables[0].functions;
        for name in [
            "count_by_a",
            "count_by_a_many",
            "count_by_a_total",
            "count_by_a_total_many",
        ] {
            assert_eq!(
                functions.iter().filter(|f| f.name == name).count(),
                1,
                "valid neighbor keeps {name}"
            );
        }
    }
}
