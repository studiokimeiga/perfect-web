//! Fixed global windows through declared membership relations.
use pw_core::check::{Unit, check_sources};

const ROOTS: &str = r#"module Roots
import capability.{ UserId }
type Root = Root { id: Int, author: UserId, stamp: Int, text: String }
table Roots
    row Root
    key id
    order stamp
    index by_author on author
"#;
const WINDOW: &str = "    window posts from Roots row Roots.Root on author to author\n";
const LINKS: &str = r#"module Links
import List
import Roots
import capability.{ User, UserId }
type Link = Link { reader: UserId, topic: Int, author: UserId, edge: Int, to: UserId, secret: String }
table Links
    row Link
    key reader, topic, author, edge
    order edge
    index by_recipient_topic_author on to, topic, author
    owned by reader
    shares with to
    private secret
    window posts from Roots row Roots.Root on author to author
"#;
const DATA: &str = r#"module RelationData
import Links
import Roots
source RelationData
    holds Links, Roots
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;
const QUERIES: &str = r#"
user query Mine(reader: User<UserId>, topic: Int, n: Int) -> List<Roots.Root>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, topic, n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.last_posts_by_reader_topic(reader, topic, n)
}
user query Several(reader: User<UserId>, topics: List<Int>, n: Int) -> List<LinksReaderTopicPostsWindow>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, topics, n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.last_posts_by_reader_topic_many(reader, topics, n)
}
user query Shared(reader: User<UserId>, topic: Int, n: Int) -> List<Roots.Root>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, topic, n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.first_posts_by_to_topic(reader, topic, n)
}
user query SharedSeveral(reader: User<UserId>, topics: List<Int>, n: Int) -> List<LinksToTopicPostsWindow>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader, topics, n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    Links.first_posts_by_to_topic_many(reader, topics, n)
}
"#;

fn files(links: &str, roots: &str, data: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![
        ("Links.pw".into(), links.into()),
        ("Roots.pw".into(), roots.into()),
        ("RelationData.pw".into(), data.into()),
    ];
    for package in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths = std::fs::read_dir(root.join(package))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|p| p == "pw"))
            .collect::<Vec<_>>();
        paths.sort();
        files.extend(paths.into_iter().map(|path| {
            (
                path.display().to_string(),
                std::fs::read_to_string(path).unwrap(),
            )
        }));
    }
    files
}
fn units(files: &[(String, String)]) -> Vec<Unit> {
    files
        .iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path: path.clone(),
                src: src.clone(),
                hir: pw_core::lower::lower_file(src, &parsed.green),
            }
        })
        .collect()
}
fn diagnostics(links: &str, roots: &str, data: &str) -> Vec<(String, String)> {
    check_sources(&files(links, roots, data))
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .map(|d| (d.code.to_string(), d.message))
        .collect()
}
fn refused(links: &str, roots: &str, data: &str, code: &str, says: &str) {
    let got = diagnostics(links, roots, data);
    assert!(
        got.iter().any(|(c, m)| c == code && m.contains(says)),
        "expected {code} {says:?}, got {got:#?}"
    );
}

#[test]
fn the_existing_table_neighbor_without_a_relation_is_valid() {
    let got = diagnostics(&LINKS.replace(WINDOW, ""), ROOTS, DATA);
    assert!(got.is_empty(), "fixture neighbor: {got:#?}");
}

#[test]
fn relation_windows_compile_real_owned_shared_and_group_components_with_dual_grants() {
    for reversed in [false, true] {
        let mut files = files(&(LINKS.to_string() + QUERIES), ROOTS, DATA);
        if reversed {
            files.reverse();
        }
        let got = check_sources(&files);
        assert!(
            got.iter().all(|(_, d)| d.is_empty()),
            "relation checks: {got:#?}"
        );
        let build = pw_core::build::build(&units(&files));
        assert!(build.is_ok(), "relation build: {:?}", build.as_ref().err());
        let build = build.unwrap();
        let json = serde_json::to_value(&build.tables).unwrap();
        let table = json["tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "Links")
            .unwrap();
        for (name, primitive, by, reader) in [
            (
                "last_posts_by_reader_topic",
                "related-last",
                vec!["reader", "topic"],
                "reader",
            ),
            (
                "last_posts_by_reader_topic_many",
                "related-last-many",
                vec!["reader", "topic"],
                "reader",
            ),
            (
                "first_posts_by_to_topic",
                "related-first",
                vec!["to", "topic"],
                "to",
            ),
            (
                "first_posts_by_to_topic_many",
                "related-first-many",
                vec!["to", "topic"],
                "to",
            ),
        ] {
            let function = table["functions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["name"] == name);
            assert!(function.is_some(), "missing {name}: {table}");
            let function = function.unwrap();
            assert_eq!(function["primitive"], primitive);
            assert_eq!(function["by"], serde_json::json!(by));
            assert_eq!(function["reader"], reader);
            assert_eq!(function["relation"], "posts");
            assert_eq!(
                function["sorted_by"],
                serde_json::json!(["stamp", "id"]),
                "target order only"
            );
        }
        for name in [
            "Links.Mine",
            "Links.Several",
            "Links.Shared",
            "Links.SharedSeveral",
        ] {
            assert!(
                build.components.iter().any(|(id, b)| id == name
                    && matches!(b, pw_core::backend::component::Built::Component { .. })),
                "real component {name}"
            );
            let contract = build
                .contracts
                .iter()
                .find(|c| c.component_id == name)
                .unwrap();
            for table in ["Links", "Roots"] {
                assert!(
                    contract
                        .required_capabilities
                        .iter()
                        .any(|c| c.family == "database"
                            && c.operation == "read"
                            && c.argument.as_deref() == Some(table)),
                    "{name} missing read<{table}>: {contract:?}"
                );
                assert!(
                    contract
                        .imports
                        .iter()
                        .filter(|i| i.interface == "pw:tables/links")
                        .all(|i| i
                            .capabilities
                            .iter()
                            .any(|c| c.argument.as_deref() == Some(table))),
                    "import dual grant {name}: {contract:?}"
                );
            }
        }
    }
}

#[test]
fn a_relation_row_annotation_is_the_exact_declared_target_row() {
    let identical = "\ntype Other = Other { id: Int, author: UserId, stamp: Int, text: String }\n";
    let roots = ROOTS.to_string() + identical;
    refused(
        &LINKS.replace("row Roots.Root on", "row Roots.Other on"),
        &roots,
        DATA,
        "PW0361",
        "row type",
    );
}

#[test]
fn a_relation_requires_one_explicit_source_for_both_tables() {
    let split = DATA.replace("holds Links, Roots", "holds Links")
        + "\nsource OtherData\n    holds Roots\n    transactions serializable\n    reads strong\n    changes none\n";
    refused(LINKS, ROOTS, &split, "PW0361", "same source");
    refused(LINKS, ROOTS, "module RelationData\n", "PW0361", "source");
}

#[test]
fn a_protected_relation_target_is_refused_beside_the_public_neighbor() {
    let owned = ROOTS
        .replace("    key id", "    key author, id")
        .replace("    order stamp", "    order stamp\n    owned by author");
    refused(LINKS, &owned, DATA, "PW0368", "public");
}

#[test]
fn a_relation_does_not_disclose_memberships_through_a_shared_cache() {
    let leak = "\nimport context.{ current_user }\nquery Leak() -> List<Roots.Root>\n    consistency strong\n    cache shared\n{\n    Links.last_posts_by_reader_topic(current_user(),0,2)\n}\n";
    refused(
        &(LINKS.to_string() + leak),
        ROOTS,
        DATA,
        "PW5001",
        "shared cache",
    );
}

#[test]
fn reader_only_relations_keep_the_existing_empty_set_exception() {
    let links = LINKS
        .replace(
            "key reader, topic, author, edge",
            "key reader, author, edge",
        )
        .replace("on to, topic, author", "on to, author");
    let build = pw_core::build::build(&units(&files(&links, ROOTS, DATA)));
    assert!(build.is_ok(), "reader-only: {:?}", build.as_ref().err());
    let build = build.unwrap();
    let table = build
        .tables
        .tables
        .iter()
        .find(|t| t.schema.name == "Links")
        .unwrap();
    for selector in ["reader", "to"] {
        assert!(
            table
                .functions
                .iter()
                .any(|f| f.name == format!("last_posts_by_{selector}"))
        );
        assert!(
            !table
                .functions
                .iter()
                .any(|f| f.name == format!("last_posts_by_{selector}_many"))
        );
    }
}

#[test]
fn a_relation_loop_names_its_own_set_and_nested_empty_groups() {
    let links = LINKS.replace(
        WINDOW,
        &format!("{WINDOW}    window stories from Roots row Roots.Root on author to author\n"),
    );
    let looped = r#"
user query Bad(reader: User<UserId>, topics: List<Int>, n: Int) -> List<List<Roots.Root>>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader,topics,n
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    List.map(topics,topic => Links.last_stories_by_reader_topic(reader,topic,n))
}
"#;
    let sources = files(&(links + looped), ROOTS, DATA);
    let got = check_sources(&sources)
        .into_iter()
        .flat_map(|(_, d)| d)
        .collect::<Vec<_>>();
    let refusal = got.iter().find(|d| d.code == "PW0405");
    assert!(refusal.is_some(), "missing keyed-loop refusal: {got:#?}");
    let hint = format!("{:#?}", refusal.unwrap());
    assert!(
        !hint.contains("; +"),
        "no patch marker in rendered relation repair: {hint}"
    );
    assert!(
        hint.contains("last_stories_by_reader_topic_many"),
        "correct relation set: {hint}"
    );
    assert!(
        !hint.contains("last_posts_by_reader_topic_many"),
        "another relation's set: {hint}"
    );
    assert!(
        hint.contains("key/rows") && hint.contains("empty"),
        "nested result groups including empty: {hint}"
    );
}

#[test]
fn duplicate_relation_names_and_user_group_names_are_named_refusals() {
    refused(
        &LINKS.replace(WINDOW, &format!("{WINDOW}{WINDOW}")),
        ROOTS,
        DATA,
        "PW0361",
        "posts",
    );
    refused(
        &(LINKS.to_string()
            + "\ntype LinksReaderTopicPostsWindow = LinksReaderTopicPostsWindow { fake: Int }\n"),
        ROOTS,
        DATA,
        "PW0361",
        "LinksReaderTopicPostsWindow",
    );
}

#[test]
fn matching_fields_keep_nominal_identity_and_both_declared_lookup_paths() {
    let roots = ROOTS
        .replace("type Root =", "opaque type OtherId = String\ntype Root =")
        .replace("author: UserId", "author: OtherId");
    refused(LINKS, &roots, DATA, "PW0361", "same complete keyable type");
    refused(
        LINKS,
        &ROOTS.replace("    index by_author on author\n", ""),
        DATA,
        "PW0361",
        "declared lookup",
    );
    refused(
        LINKS,
        &ROOTS.replace("    order stamp\n", ""),
        DATA,
        "PW0361",
        "target order",
    );
    let links = LINKS
        .replace("key reader, topic, author, edge", "key reader, topic, edge")
        .replace(
            "    index by_recipient_topic_author on to, topic, author\n",
            "",
        );
    refused(&links, ROOTS, DATA, "PW0361", "declared lookup");
}

#[test]
fn relation_owner_formals_do_not_alias_the_list_or_bound() {
    for owner in ["keys", "limit"] {
        let pascal = format!("{}{}", owner[..1].to_uppercase(), &owner[1..]);
        let links = (LINKS.to_string() + QUERIES)
            .replace("reader", owner)
            .replace("Reader", &pascal);
        let build = pw_core::build::build(&units(&files(&links, ROOTS, DATA)));
        assert!(build.is_ok(), "owner {owner}: {:?}", build.as_ref().err());
        let build = build.unwrap();
        for name in ["Links.Mine", "Links.Several"] {
            assert!(
                build.components.iter().any(|(id, b)| id == name
                    && matches!(b, pw_core::backend::component::Built::Component { .. })),
                "real WIT names {owner} {name}"
            );
        }
    }
}

#[test]
fn nominal_names_are_qualified_and_imported_row_spellings_preserve_identity() {
    let roots = ROOTS
        .replace("import capability", "import Beta\nimport capability")
        .replace("author: UserId", "author: Beta.Tag");
    let links = LINKS
        .replace("import Roots", "import Alpha\nimport Roots")
        .replace("author: UserId", "author: Alpha.Tag");
    let extra = [
        (
            "Alpha.pw".into(),
            "module Alpha\nopaque type Tag = String\n".into(),
        ),
        (
            "Beta.pw".into(),
            "module Beta\nopaque type Tag = String\n".into(),
        ),
    ];
    let mut bad = files(&links, &roots, DATA);
    bad.extend(extra.clone());
    let got = check_sources(&bad)
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .collect::<Vec<_>>();
    assert!(
        got.iter()
            .any(|d| d.code == "PW0361" && d.message.contains("same complete keyable type")),
        "same local Tag/string is not same nominal type: {got:#?}"
    );
    let mut good = files(&links.replace("Alpha", "Beta"), &roots, DATA);
    good.extend(extra);
    let got = check_sources(&good);
    assert!(
        got.iter().all(|(_, ds)| ds.is_empty()),
        "one imported nominal Tag: {got:#?}"
    );
    let imported = LINKS
        .replace("import Roots\n", "import Roots\nimport Roots.{ Root }\n")
        .replace("row Roots.Root on", "row Root on");
    assert!(
        diagnostics(&imported, ROOTS, DATA).is_empty(),
        "unqualified import denotes the same row"
    );
    let twins = ROOTS
        .split("table Roots")
        .next()
        .unwrap()
        .replace("module Roots", "module Twins");
    let wrong = LINKS
        .replace("import Roots\n", "import Roots\nimport Twins\n")
        .replace("row Roots.Root on", "row Twins.Root on");
    let mut sources = files(&wrong, ROOTS, DATA);
    sources.push(("Twins.pw".into(), twins));
    let got = check_sources(&sources)
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .collect::<Vec<_>>();
    assert!(
        got.iter()
            .any(|d| d.code == "PW0361" && d.message.contains("row type")),
        "same local Root/layout is not the same row: {got:#?}"
    );
}

#[test]
fn relation_row_annotations_obey_visibility_and_require_the_declared_shape() {
    let private = ROOTS.replace("type Root =", "private type Root =");
    let got = diagnostics(LINKS, &private, DATA);
    assert!(
        got.iter().any(
            |(code, message)| (code == "PW0361" && message.contains("row type"))
                || code == "PW0023"
        ),
        "private row annotation is not importable: {got:#?}"
    );
    refused(
        &LINKS.replace("row Roots.Root ", ""),
        ROOTS,
        DATA,
        "PW0361",
        "window",
    );
    refused(
        &LINKS.replace("row Roots.Root on", "row Roots.Missing on"),
        ROOTS,
        DATA,
        "PW0361",
        "row type",
    );
    refused(
        &LINKS.replace("from Roots row", "from Missing row"),
        ROOTS,
        DATA,
        "PW0361",
        "target table",
    );
}

#[test]
fn an_owner_may_match_private_fields_but_a_sharer_cannot_select_through_them() {
    let owner = LINKS
        .replace("key reader, topic, author, edge", "key reader, edge")
        .replace(
            "index by_recipient_topic_author on to, topic, author",
            "index by_reader_author on reader, author\n    index by_to on to",
        )
        .replace("private secret", "private secret, author");
    let got = diagnostics(&owner, ROOTS, DATA);
    assert!(
        got.is_empty(),
        "owner-only private match is valid beside ordinary shared rows: {got:#?}"
    );
    let shared = owner.replace(
        "index by_to on to",
        "index by_to on to\n    index by_to_author on to, author",
    );
    refused(&shared, ROOTS, DATA, "PW0369", "private match field");
}
