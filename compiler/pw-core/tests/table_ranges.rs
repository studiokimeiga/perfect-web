//! Strict counts after declared order marks, with heterogeneous set requests.
use pw_core::check::Unit;

const BASE: &str = r#"module Ranges
import List
import String
import capability.{ User, UserId }
opaque type Mark = Int where value >= 0
type Entry = Entry { owner: UserId, bucket: Int, id: Int, recipient: UserId, stamp: Mark, draft: String }
table Ranges
    row Entry
    key owner, bucket, id
    order stamp
    index by_recipient on recipient
    index by_recipient_bucket on recipient, bucket
    owned by owner
    shares with recipient
    private draft
source RangeData
    holds Ranges
    transactions serializable
    reads strong
    changes none
    sets one_call
event RangeChanged(who: UserId)
"#;

const QUERIES: &str = r#"
fn mine(reader: User<UserId>, mark: Mark) -> Int !{ database.read<Ranges> } {
    Ranges.count_after_by_owner(reader, mark)
}
fn shared(reader: User<UserId>, bucket: Int, mark: Mark) -> Int !{ database.read<Ranges> } {
    Ranges.count_after_by_recipient_bucket(reader, bucket, mark)
}
user query Several(reader: User<UserId>, ranges: List<RangesBucketAfter>) -> List<RangesOwnerBucketAfterCount>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, ranges
    invalidates_on RangeChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Ranges.count_after_by_owner_bucket_many(reader, ranges)
}
user query Marks(reader: User<UserId>, ranges: List<RangesAfter>) -> List<RangesOwnerAfterCount>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, ranges
    invalidates_on RangeChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Ranges.count_after_by_owner_many(reader, ranges)
}
"#;

fn units(source: &str) -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = vec![("Ranges.pw".to_string(), source.to_string())];
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

fn diagnostics(source: &str) -> Vec<pw_core::diagnostics::Diagnostic> {
    let sources = units(source)
        .into_iter()
        .map(|u| (u.path, u.src))
        .collect::<Vec<_>>();
    pw_core::check::check_sources(&sources)
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .collect()
}

#[test]
fn range_counts_compile_through_owned_shared_and_reader_only_set_imports() {
    for mark in [
        "Int where value >= 0",
        "String where String.length(value) >= 1",
        "Bool",
    ] {
        let source = (BASE.to_string() + QUERIES).replace("Int where value >= 0", mark);
        let build = pw_core::build::build(&units(&source));
        assert!(
            build.is_ok(),
            "range build for {mark}: {:?}",
            build.as_ref().err()
        );
        let build = build.unwrap();
        let tables = serde_json::to_value(&build.tables).unwrap();
        let functions = tables["tables"][0]["functions"].as_array().unwrap();
        for (name, primitive, reader, by) in [
            (
                "count_after_by_owner",
                "count-after",
                "owner",
                vec!["owner"],
            ),
            (
                "count_after_by_owner_many",
                "count-after-many",
                "owner",
                vec!["owner"],
            ),
            (
                "count_after_by_owner_bucket_many",
                "count-after-many",
                "owner",
                vec!["owner", "bucket"],
            ),
            (
                "count_after_by_recipient_bucket",
                "count-after",
                "recipient",
                vec!["recipient", "bucket"],
            ),
        ] {
            let f = functions.iter().find(|f| f["name"] == name).unwrap();
            assert_eq!(f["primitive"], primitive);
            assert_eq!(f["reader"], reader);
            assert_eq!(f["by"], serde_json::json!(by));
            assert_eq!(f["effect"], "read");
            assert!(
                f["sorted_by"]
                    .as_array()
                    .is_none_or(|fields| fields.is_empty()),
                "range counts have no stored-row sort: {f}"
            );
        }
        assert!(!functions.iter().any(|f| matches!(
            f["name"].as_str(),
            Some("count_after" | "count_after_by_id")
        )));
        let own = units(&source);
        let own = &own.iter().find(|u| u.path == "Ranges.pw").unwrap().hir;
        for (name, fields) in [
            ("RangesAfter", vec!["after"]),
            ("RangesBucketAfter", vec!["bucket", "after"]),
            ("RangesOwnerBucketAfter", vec!["owner", "bucket", "after"]),
        ] {
            let record = own.all_decls().find(|(_, d)| d.name == name).unwrap().1;
            assert_eq!(
                record
                    .fields
                    .iter()
                    .flatten()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>(),
                fields
            );
        }
        for name in ["Ranges.Several", "Ranges.Marks"] {
            assert!(
                build.components.iter().any(|(id, artifact)| id == name
                    && matches!(
                        artifact,
                        pw_core::backend::component::Built::Component { .. }
                    )),
                "real heterogeneous set component {name}"
            );
        }
    }
}

#[test]
fn a_table_without_declared_order_has_no_range_counts() {
    let source = BASE.replace("    order stamp\n", "");
    let build = pw_core::build::build(&units(&source)).unwrap();
    assert!(
        !build.tables.tables[0]
            .functions
            .iter()
            .any(|f| f.name.starts_with("count_after"))
    );
    assert!(
        build.tables.tables[0]
            .functions
            .iter()
            .any(|f| f.name == "count_by_owner")
    );
}

#[test]
fn range_count_inputs_keep_the_reader_and_the_nominal_order_type() {
    for (source, required) in [
        (
            (BASE.to_string() + QUERIES)
                .replace("fn mine(reader: User<UserId>", "fn mine(reader: UserId"),
            "argument 1 of `Ranges.count_after_by_owner`",
        ),
        (
            (BASE.to_string() + QUERIES).replace(
                "fn mine(reader: User<UserId>, mark: Mark)",
                "fn mine(reader: User<UserId>, mark: Int)",
            ),
            "argument 2 of `Ranges.count_after_by_owner`",
        ),
    ] {
        let why = pw_core::build::build(&units(&source)).err();
        assert!(
            why.as_ref().is_some_and(|why| why.contains(required)),
            "typed range refusal: {why:?}"
        );
    }
}

#[test]
fn range_count_loops_repair_to_selector_and_mark_records() {
    for (parameters, body) in [
        (
            "reader: User<UserId>, buckets: List<Int>, mark: Mark",
            "List.map(buckets, bucket => Ranges.count_after_by_owner_bucket(reader, bucket, mark))",
        ),
        (
            "reader: User<UserId>, bucket: Int, marks: List<Mark>",
            "List.map(marks, mark => Ranges.count_after_by_owner_bucket(reader, bucket, mark))",
        ),
        (
            "reader: User<UserId>, requests: List<Request>",
            "List.map(requests, request => Ranges.count_after_by_owner_bucket(reader, request.bucket, request.mark))",
        ),
    ] {
        let source = BASE.to_string()
            + &format!(
                "\ntype Request = Request {{ bucket: Int, mark: Mark }}\nfn per({parameters}) -> List<Int> !{{ database.read<Ranges> }} {{ {body} }}\n"
            );
        let ds = diagnostics(&source);
        assert_eq!(ds.len(), 1, "{ds:#?}");
        assert_eq!(ds[0].code, "PW0405", "{ds:#?}");
        let help = &ds[0].repairs[0].description;
        assert!(
            help.contains("Ranges.count_after_by_owner_bucket_many")
                && help.contains("RangesBucketAfter"),
            "{help}"
        );
        assert!(
            help.contains("`bucket`")
                && help.contains("`after`")
                && help.contains("reader handle separately"),
            "each key includes its own mark: {help}"
        );
        assert!(
            !help.contains("one common bound"),
            "heterogeneous marks are not one shared window bound: {help}"
        );
    }
}

#[test]
fn generated_range_parameters_and_fields_do_not_alias_after_or_reader_names() {
    for field in ["after", "after_mark", "values", "row"] {
        let source = (BASE.to_string() + QUERIES)
            .replace("owner", field)
            .replace(
                "Owner",
                &field
                    .split('_')
                    .map(|part| format!("{}{}", part[..1].to_uppercase(), &part[1..]))
                    .collect::<String>(),
            );
        let units = units(&source);
        let own = &units.iter().find(|u| u.path == "Ranges.pw").unwrap().hir;
        for (id, declaration) in own.all_decls() {
            if !own.generated.contains_key(&id) {
                continue;
            }
            let names = declaration
                .params
                .iter()
                .map(|p| pw_core::wit::ident(&p.name))
                .collect::<Vec<_>>();
            assert_eq!(
                names
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                names.len(),
                "{} params: {names:?}",
                declaration.name
            );
            let fields = declaration
                .fields
                .iter()
                .flatten()
                .map(|f| pw_core::wit::ident(&f.name))
                .collect::<Vec<_>>();
            assert_eq!(
                fields
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                fields.len(),
                "{} fields: {fields:?}",
                declaration.name
            );
        }
        let build = pw_core::build::build(&units);
        assert!(
            build.is_ok(),
            "reader field {field}: {:?}",
            build.as_ref().err()
        );
    }
}

#[test]
fn range_record_names_never_silently_alias_an_ordinary_selector_tuple() {
    let source = BASE.replace("bucket", "after");
    let found = diagnostics(&source);
    assert!(
        found.iter().any(|d| d.code == "PW0361"
            && d.message.contains("same type")
            && d.message.contains("ranges-owner-after")),
        "different range and ordinary selector meanings are refused by emitted identity: {found:#?}"
    );
    let valid = source.replace("after", "cursor");
    let build = pw_core::build::build(&units(&valid));
    assert!(
        build.is_ok(),
        "renamed selector retains distinct records: {:?}",
        build.as_ref().err()
    );
    assert!(
        build.unwrap().tables.tables[0]
            .functions
            .iter()
            .any(|f| f.name == "count_after_by_owner_cursor_many")
    );
}

#[test]
fn range_scalar_and_set_imports_cannot_silently_alias() {
    let source = "module RangeNames\ntype Entry = Entry { a: Int, id: Int, a_many: Int, stamp: Int }\ntable RangeNames\n    row Entry\n    key a, id\n    order stamp\n    index by_other on a_many\nsource RangeData\n    holds RangeNames\n    transactions serializable\n    reads strong\n    changes none\n    sets one_call\n";
    for source in [
        source.to_string(),
        source
            .replace("key a, id", "key a_many, id")
            .replace("on a_many", "on a"),
    ] {
        let found = diagnostics(&source);
        assert!(
            found.iter().any(|d| d.code == "PW0361"
                && d.message.contains("count_after_by_a_many")
                && d.message.contains("same import")),
            "{found:#?}"
        );
        let valid = source.replace("a_many", "a_total");
        let build = pw_core::build::build(&units(&valid)).unwrap();
        for name in [
            "count_after_by_a",
            "count_after_by_a_many",
            "count_after_by_a_total",
            "count_after_by_a_total_many",
        ] {
            assert_eq!(
                build.tables.tables[0]
                    .functions
                    .iter()
                    .filter(|f| f.name == name)
                    .count(),
                1,
                "{name}"
            );
        }
    }
}

#[test]
fn the_range_mark_name_avoids_every_existing_row_field() {
    let source = (BASE.to_string() + QUERIES).replace(
        "draft: String }",
        "draft: String, after: String, after_mark: Int }",
    );
    let program = units(&source);
    let build = pw_core::build::build(&program).unwrap();
    let scalar = build.tables.tables[0]
        .functions
        .iter()
        .find(|f| f.name == "count_after_by_owner")
        .unwrap();
    assert_eq!(scalar.params.last().unwrap().name, "after_mark_mark");
    let hir = &program.iter().find(|u| u.path == "Ranges.pw").unwrap().hir;
    let record = hir
        .all_decls()
        .find(|(_, d)| d.name == "RangesAfter")
        .unwrap()
        .1;
    assert_eq!(record.fields.as_ref().unwrap()[0].name, "after_mark_mark");
}
