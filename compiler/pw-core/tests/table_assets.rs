//! Generic upload resources become public row references, never domain writes.
use pw_core::check::{Unit, check_sources};

const IMAGE: &str = r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#;
const POSTS: &str = r#"module Posts
import PostImage
type Post = Post { id: Int, image: Option<PostImage.Ref>, alt: String }
table Posts
    row Post
    key id
    bounded 100
    asset image from PostImage
    written by anyone
"#;
const SOURCE: &str = r#"module PhotoData
import PostImage
import Posts
source PhotoData
    holds PostImage, Posts
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;
const APP: &str = r#"module photos.app
import PostImage
import Posts
import context.{ current_session }
import capability.{ Session, SessionId }
opaque type InteractionId = String
command keep(id: Int, alt: String) -> Result<(),PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Posts.put(Posts.Post { id: id, image: Some(image), alt: alt })
    Ok(())
}
command discard() -> Result<PostImage.Attached,PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    PostImage.discard(current_session(),claim)
}
session query Attached(session: Session<SessionId>) -> List<PostImage.Attached>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key session
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.attached(session)
}
public query Url(image: PostImage.Ref) -> String
    freshness 0.seconds
    consistency snapshot
    cache shared
    key image
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.src(image)
}
"#;
fn files(posts: &str, image: &str, source: &str, app: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![
        ("Posts.pw".into(), posts.into()),
        ("PostImage.pw".into(), image.into()),
        ("PhotoData.pw".into(), source.into()),
        ("app.pw".into(), app.into()),
    ];
    for package in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths = std::fs::read_dir(root.join(package))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|p| p == "pw"))
            .collect::<Vec<_>>();
        paths.sort();
        files.extend(
            paths
                .into_iter()
                .map(|p| (p.display().to_string(), std::fs::read_to_string(p).unwrap())),
        );
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
fn diagnostics(posts: &str, image: &str, source: &str, app: &str) -> Vec<(String, String)> {
    check_sources(&files(posts, image, source, app))
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .map(|d| (d.code.to_string(), d.message))
        .collect()
}
/// **A refusal names the boundary it is held to** (charter §16.3; the L5
/// corpus, 2026-11-17: PW5604's and PW5605's named none): some refusal of
/// `code` in `files`, whose message holds `says`, has a related note in
/// `file` saying `note`, its span the text `at` there (its review, M2: a
/// note's span was held to nothing).
fn bounded(files: &[(String, String)], code: &str, says: &str, file: &str, note: &str, at: &str) {
    let got = check_sources(files);
    let text = |name: &str| {
        files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.as_str())
            .unwrap_or_else(|| panic!("no file {name}"))
    };
    let refusals: Vec<_> = got
        .iter()
        .flat_map(|(path, ds)| ds.iter().map(move |d| (path, d)))
        .filter(|(_, d)| d.code == code && d.message.contains(says))
        .collect();
    assert!(
        refusals.iter().any(|(path, d)| d.related.iter().any(|r| {
            let named = r.file.as_deref().unwrap_or(path.as_str());
            r.label.contains(note) && named == file && &text(named)[r.span.clone()] == at
        })),
        "a {code} saying {says:?} with a note in {file} at {at:?} saying {note:?}, where there were {:#?}",
        refusals
            .iter()
            .map(|(path, d)| (path, &d.message, &d.related))
            .collect::<Vec<_>>()
    );
}
/// **A refusal by its rule and its words**: a diagnostic of `code` whose
/// message holds `says` (the follow-up's review, L4: words alone, as short
/// as "claim", matched another rule's refusal).
fn refused(posts: &str, image: &str, source: &str, app: &str, code: &str, says: &str) {
    let got = diagnostics(posts, image, source, app);
    assert!(
        got.iter().any(|(c, m)| c == code && m.contains(says)),
        "expected {code} saying {says:?}, got {got:#?}"
    );
}

#[test]
fn the_neighbor_without_an_asset_remains_a_valid_table_and_upload() {
    let posts = POSTS
        .replace("Option<PostImage.Ref>", "Option<String>")
        .replace("    asset image from PostImage\n", "");
    let got = diagnostics(&posts, IMAGE, SOURCE, "module photos.app\n");
    assert!(got.is_empty(), "ordinary neighbor: {got:#?}");
}

#[test]
fn a_public_optional_reference_and_resource_only_components_compile() {
    for reverse in [false, true] {
        let mut files = files(POSTS, IMAGE, SOURCE, APP);
        if reverse {
            files.reverse();
        }
        let got = check_sources(&files);
        assert!(
            got.iter().all(|(_, ds)| ds.is_empty()),
            "generic asset checks: {got:#?}"
        );
        let build = pw_core::build::build(&units(&files));
        assert!(
            build.is_ok(),
            "generic asset build: {:?}",
            build.as_ref().err()
        );
        let build = build.unwrap();
        let tables = serde_json::to_value(&build.tables).unwrap();
        let posts = tables["tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "Posts")
            .unwrap();
        assert_eq!(
            posts["assets"],
            serde_json::json!([{"field":"image","upload":"PostImage"}])
        );
        use pw_core::tables::Stored;
        let image = &build
            .tables
            .tables
            .iter()
            .find(|t| t.schema.name == "Posts")
            .unwrap()
            .schema
            .fields
            .iter()
            .find(|f| f.name == "image")
            .unwrap()
            .stored;
        let Stored::Option(image) = image else {
            panic!("optional image reference: {image:?}")
        };
        let Stored::Record(fields) = &**image else {
            panic!("record image reference: {image:?}")
        };
        assert_eq!(
            fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["key", "generation", "kind", "width", "height"]
        );
        for field in fields
            .iter()
            .filter(|f| matches!(f.name.as_str(), "key" | "generation"))
        {
            assert!(matches!(field.stored, Stored::String { .. }), "{field:?}");
        }
        let kinds = &fields.iter().find(|f| f.name == "kind").unwrap().stored;
        let Stored::Cases(kinds) = kinds else {
            panic!("closed declared image kinds: {kinds:?}")
        };
        assert_eq!(
            kinds.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ["Png", "Jpeg"]
        );
        for import in build
            .contracts
            .iter()
            .flat_map(|c| &c.imports)
            .filter(|i| i.interface == "pw:uploads/post-image")
        {
            let (operation, resource) = match import.name.as_str() {
                "attached" => ("read", None),
                "claim" => ("read", Some("acquire")),
                "publish" | "discard" => ("write", Some("release")),
                other => panic!("unexpected generated upload operation {other}"),
            };
            if import.name == "attached" {
                let signature = serde_json::to_value(import.signature.as_ref().unwrap()).unwrap();
                assert_eq!(
                    signature["params"],
                    serde_json::json!([{"declared":{"path":"capability.Session","args":[{"declared":{"path":"capability.SessionId"}}]}}])
                );
                assert_eq!(
                    signature["result"],
                    serde_json::json!({"builtin":{"ctor":"List","args":[{"declared":{"path":"PostImage.Attached"}}]}})
                );
            }
            let caps = import
                .capabilities
                .iter()
                .map(|c| c.name())
                .collect::<std::collections::BTreeSet<_>>();
            let mut expected =
                std::collections::BTreeSet::from([format!("database.{operation}<PostImage>")]);
            if let Some(resource) = resource {
                expected.insert(format!("resource.{resource}<PostImage.Claim>"));
            }
            assert_eq!(caps, expected, "resource-only import {}", import.key());
        }
        let uploads = serde_json::to_value(&build.uploads).unwrap();
        assert_eq!(uploads[0]["source"], "PhotoData");
        assert_eq!(uploads[0]["module"], "PostImage");
        assert!(
            build
                .contracts
                .iter()
                .flat_map(|c| &c.imports)
                .filter(|i| i.interface.starts_with("pw:uploads/"))
                .all(|i| i.owner == pw_core::contract::Ownership::Platform),
            "generic upload primitives belong to the platform"
        );
        let imports = build
            .contracts
            .iter()
            .flat_map(|c| c.imports.iter().map(|i| i.key()))
            .collect::<Vec<_>>();
        let capabilities = build
            .contracts
            .iter()
            .flat_map(|c| c.required_capabilities.iter().map(|cap| cap.name()))
            .collect::<Vec<_>>();
        for operation in [
            "pw:uploads/post-image#attached",
            "pw:uploads/post-image#claim",
            "pw:uploads/post-image#publish",
            "pw:uploads/post-image#discard",
        ] {
            assert!(
                imports.iter().any(|i| i == operation),
                "generated resource operation {operation}: {imports:?}"
            );
        }
        for effect in [
            "database.read<PostImage>",
            "database.write<PostImage>",
            "resource.acquire<PostImage.Claim>",
            "resource.release<PostImage.Claim>",
            "database.write<Posts>",
        ] {
            assert!(
                capabilities.iter().any(|cap| cap == effect),
                "generated effect {effect}: {capabilities:?}"
            );
        }
    }
}

#[test]
fn assets_require_the_actual_public_reference_field_and_held_source() {
    refused(
        &POSTS.replace("asset image from PostImage", "asset missing from PostImage"),
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "is not declared in the row",
    );
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "Option<String>"),
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "must have the exact generated upload reference type",
    );
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "List<PostImage.Ref>"),
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "must be one top-level Ref or Option<Ref>",
    );
    refused(
        &POSTS.replace("    key id", "    key id\n    private image"),
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "an asset requires a public asset-bearing table",
    );
    refused(
        POSTS,
        IMAGE,
        &SOURCE.replace("holds PostImage, Posts", "holds Posts"),
        APP,
        "PW0371",
        "an asset and its upload require the same explicit source",
    );
    refused(
        POSTS,
        IMAGE,
        SOURCE,
        &APP.replace("    idempotent_by InteractionId\n", ""),
        "PW5605",
        "acquires or releases a generic upload claim and requires idempotent_by",
    );
}

#[test]
fn claims_and_lookalike_reference_records_cannot_be_stored_as_assets() {
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "Option<PostImage.Claim>"),
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "cannot store an upload claim, a transaction-owned resource",
    );
    let image = IMAGE.to_string() + "\ntype Ref = Ref { key: String }\n";
    refused(
        POSTS,
        &image,
        SOURCE,
        APP,
        "PW5604",
        "is a generated upload declaration and the program declares its emitted identity",
    );
    let image = IMAGE.to_string() + "\nfn publish() -> Int { 1 }\n";
    refused(
        POSTS,
        &image,
        SOURCE,
        APP,
        "PW5604",
        "is a generated upload declaration and the program declares its emitted identity",
    );
}

#[test]
fn qualified_capability_types_require_the_real_visible_public_declaration() {
    let posts = POSTS
        .replace("Option<PostImage.Ref>", "Option<String>")
        .replace("    asset image from PostImage\n", "");
    let app = "module photos.app\nfn acquire() -> String !{ resource.acquire<Hidden.Token> }\n    host \"test:hidden/acquire#run\"\n";
    let mut files = files(&posts, IMAGE, SOURCE, app);
    files.push((
        "hidden.pw".into(),
        "module Hidden\nopaque type Token = String\n".into(),
    ));
    let found = check_sources(&files);
    assert!(
        found
            .iter()
            .flat_map(|(_, ds)| ds)
            .any(|d| d.code == "PW5200" && d.message.contains("Hidden.Token")),
        "unimported qualified type: {found:#?}"
    );
    let imported = files.iter_mut().find(|(path, _)| path == "app.pw").unwrap();
    imported.1 = app.replace("module photos.app\n", "module photos.app\nimport Hidden\n");
    let found = check_sources(&files);
    assert!(
        found.iter().all(|(_, ds)| ds.is_empty()),
        "visible qualified type: {found:#?}"
    );
    files
        .iter_mut()
        .find(|(path, _)| path == "hidden.pw")
        .unwrap()
        .1 = "module Hidden\nprivate opaque type Token = String\n".into();
    let found = check_sources(&files);
    assert!(
        found
            .iter()
            .flat_map(|(_, ds)| ds)
            .any(|d| d.code == "PW5200" && d.message.contains("Hidden.Token")),
        "private qualified type: {found:#?}"
    );
}

#[test]
fn equal_layout_references_of_two_uploads_keep_nominal_identity() {
    let avatar = IMAGE
        .replace("PostImage", "Avatar")
        .replace("/uploads/post-image", "/uploads/avatar")
        .replace("/images", "/avatars");
    let posts = POSTS
        .replace("import PostImage", "import PostImage\nimport Avatar")
        .replace("alt: String }", "alt: String, avatar: Option<Avatar.Ref> }")
        .replace(
            "    asset image from PostImage",
            "    asset image from PostImage\n    asset avatar from Avatar",
        );
    let source = SOURCE
        .replace("import PostImage", "import PostImage\nimport Avatar")
        .replace("holds PostImage, Posts", "holds PostImage, Avatar, Posts");
    let app = APP.replace(
        "image: Some(image), alt: alt",
        "image: Some(image), alt: alt, avatar: None",
    );
    let mut good = files(&posts, IMAGE, &source, &app);
    good.push(("Avatar.pw".into(), avatar));
    let got = check_sources(&good);
    assert!(
        got.iter().all(|(_, ds)| ds.is_empty()),
        "distinct typed references: {got:#?}"
    );
    let build = pw_core::build::build(&units(&good));
    assert!(
        build.is_ok(),
        "two references compile: {:?}",
        build.as_ref().err()
    );
    good.iter_mut()
        .find(|(path, _)| path == "Posts.pw")
        .unwrap()
        .1 = posts.replace("asset avatar from Avatar", "asset avatar from PostImage");
    let got = check_sources(&good);
    assert!(
        got.iter()
            .flat_map(|(_, ds)| ds)
            .any(|d| d.message.contains("exact generated upload reference type")),
        "same-layout different nominal upload: {got:#?}"
    );
}

#[test]
fn imported_reference_spelling_is_valid_and_claims_do_not_escape_through_nested_rows() {
    let posts = POSTS
        .replace(
            "import PostImage",
            "import PostImage\nimport PostImage.{ Ref }",
        )
        .replace("Option<PostImage.Ref>", "Option<Ref>");
    let got = diagnostics(&posts, IMAGE, SOURCE, APP);
    assert!(got.is_empty(), "same imported Ref: {got:#?}");
    let posts = POSTS
        .replace(
            "type Post =",
            "type Wrapper = Wrapper { claims: List<PostImage.Claim> }\ntype Post =",
        )
        .replace("alt: String }", "alt: String, hidden: Option<Wrapper> }")
        .replace("    asset image from PostImage\n", "");
    refused(
        &posts,
        IMAGE,
        SOURCE,
        "module photos.app\n",
        "PW0371",
        "cannot store an upload claim, a transaction-owned resource",
    );
}

#[test]
fn a_persisted_reference_requires_its_asset_association_and_top_level_shape() {
    let unannotated = POSTS.replace("    asset image from PostImage\n", "");
    refused(
        &unannotated,
        IMAGE,
        SOURCE,
        APP,
        "PW0371",
        "requires its asset association",
    );
    let nested = unannotated
        .replace(
            "type Post =",
            "type Wrapper = Wrapper { inner: PostImage.Ref }\ntype Post =",
        )
        .replace("Option<PostImage.Ref>", "Option<Wrapper>");
    refused(
        &nested,
        IMAGE,
        SOURCE,
        "module photos.app\n",
        "PW0371",
        "must be a top-level Ref or Option<Ref>",
    );
}

#[test]
fn generated_upload_visibility_and_interface_identities_are_not_silently_broadened() {
    refused(
        POSTS,
        &IMAGE.replace("upload PostImage", "private upload PostImage"),
        SOURCE,
        APP,
        "PW0371",
        "an asset requires a public upload declaration",
    );
    let inputs = files(
        POSTS,
        &IMAGE.replace("upload PostImage", "private upload PostImage"),
        SOURCE,
        APP,
    );
    let units = units(&inputs);
    let upload = units.iter().find(|u| u.path == "PostImage.pw").unwrap();
    assert!(!upload.hir.generated_uploads.is_empty());
    assert!(
        upload
            .hir
            .generated_uploads
            .keys()
            .all(|id| upload.hir.decl(*id).visibility.as_deref() == Some("private")),
        "generated types and resource operations inherit their declaration visibility"
    );
}

#[test]
fn distinct_upload_names_cannot_emit_one_resource_interface() {
    let alias = IMAGE
        .replace("PostImage", "Post_Image")
        .replace("/uploads/post-image", "/uploads/second")
        .replace("/images", "/second");
    let posts = POSTS
        .replace("import PostImage", "import PostImage\nimport Post_Image")
        .replace(
            "alt: String }",
            "alt: String, other: Option<Post_Image.Ref> }",
        )
        .replace(
            "    asset image from PostImage",
            "    asset image from PostImage\n    asset other from Post_Image",
        );
    let source = SOURCE
        .replace("import PostImage", "import PostImage\nimport Post_Image")
        .replace(
            "holds PostImage, Posts",
            "holds PostImage, Post_Image, Posts",
        );
    let mut files = files(&posts, IMAGE, &source, "module photos.app\n");
    files.push(("alias.pw".into(), alias));
    let got = check_sources(&files);
    assert!(
        got.iter()
            .flat_map(|(_, ds)| ds)
            .any(|d| d.code == "PW5604" && d.message.contains("generated upload interface")),
        "two names emit one upload interface, told PW5604: {got:#?}"
    );
    assert!(
        got.iter()
            .flat_map(|(_, ds)| ds)
            .all(|d| d.code != "PW5601"),
        "{got:#?}"
    );
    // Each at the other, which emits the same interface (charter §16.3).
    bounded(
        &files,
        "PW5604",
        "`PostImage` emits a generated upload interface",
        "alias.pw",
        "`Post_Image` emits the interface `post-image` too",
        "Post_Image",
    );
    bounded(
        &files,
        "PW5604",
        "`Post_Image` emits a generated upload interface",
        "PostImage.pw",
        "`PostImage` emits the interface `post-image` too",
        "PostImage",
    );
}

/// **PW5604's note names an upload that emits the interface, and not one of
/// its name that emits nothing** (its review, L1): an upload outside the
/// module of its name generates no declaration and emits no interface, so it
/// is no boundary of the collision.
#[test]
fn an_interface_collision_is_noted_only_at_an_upload_that_emits_it() {
    let alias = IMAGE
        .replace("PostImage", "Post_Image")
        .replace("/uploads/post-image", "/uploads/second")
        .replace("/images", "/second");
    let stray = IMAGE
        .replace("module PostImage", "module feed.other")
        .replace("upload PostImage", "upload Post_Image")
        .replace("/uploads/post-image", "/uploads/third")
        .replace("/images", "/third");
    let mut program = files(POSTS, IMAGE, SOURCE, "module photos.app\n");
    program.push(("alias.pw".into(), alias));
    program.push(("other.pw".into(), stray));
    let got = check_sources(&program);
    let noted: Vec<_> = got
        .iter()
        .filter(|(path, _)| path == "PostImage.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| {
            d.code == "PW5604"
                && d.message
                    .contains("`PostImage` emits a generated upload interface")
        })
        .flat_map(|d| d.related.iter().map(|r| r.file.clone()))
        .collect();
    assert_eq!(noted, vec![Some("alias.pw".to_string())], "{got:#?}");
}

/// **A declaration may not take a generated upload declaration's identity**
/// (PW5604, L5a's review, finding L7: it was PW5601's, the upload's clauses
/// and limits): `KIND` emits `kind`, as the upload's generated `Kind` does.
#[test]
fn a_declaration_emitting_a_generated_upload_identity_is_pw5604() {
    let taken = format!("{IMAGE}opaque type KIND = String\n");
    let got = diagnostics(POSTS, &taken, SOURCE, APP);
    assert!(
        got.iter()
            .any(|(code, m)| code == "PW5604" && m.contains("is a generated upload declaration")),
        "a declaration named as a generated one, told PW5604: {got:#?}"
    );
    assert!(got.iter().all(|(code, _)| code != "PW5601"), "{got:#?}");
    // At the program's declaration, which takes the generated one's name.
    bounded(
        &files(POSTS, &taken, SOURCE, APP),
        "PW5604",
        "`Kind` is a generated upload declaration",
        "PostImage.pw",
        "`KIND` is declared here, and emits `kind`",
        "KIND",
    );
}

/// **A command that claims or releases an upload declares `idempotent_by`**
/// (PW5605, L5a's review, finding L7: it was PW5601's).
#[test]
fn a_command_claiming_an_upload_without_idempotent_by_is_pw5605() {
    let got = diagnostics(
        POSTS,
        IMAGE,
        SOURCE,
        &APP.replace("    idempotent_by InteractionId\n", ""),
    );
    assert!(
        got.iter()
            .any(|(code, m)| code == "PW5605" && m.contains("requires idempotent_by")),
        "a claiming command without idempotent_by, told PW5605: {got:#?}"
    );
    assert!(got.iter().all(|(code, _)| code != "PW5601"), "{got:#?}");
    // At the upload whose claim each command takes, in its own module.
    let program = files(
        POSTS,
        IMAGE,
        SOURCE,
        &APP.replace("    idempotent_by InteractionId\n", ""),
    );
    for command in ["keep", "discard"] {
        bounded(
            &program,
            "PW5605",
            &format!("`{command}` acquires or releases"),
            "PostImage.pw",
            &format!("`PostImage` is the upload whose claim `{command}` takes"),
            "PostImage",
        );
    }
}

#[test]
fn the_public_asset_rule_refuses_an_otherwise_valid_owned_table() {
    let owned = POSTS
        .replace(
            "import PostImage",
            "import PostImage\nimport capability.{ UserId }",
        )
        .replace("id: Int, image:", "owner: UserId, id: Int, image:")
        .replace(
            "    key id\n    bounded 100",
            "    key owner, id\n    owned by owner",
        )
        .replace("    written by anyone\n", "");
    let neighbor = owned
        .replace("Option<PostImage.Ref>", "Option<String>")
        .replace("    asset image from PostImage\n", "");
    let got = diagnostics(&neighbor, IMAGE, SOURCE, "module photos.app\n");
    assert!(
        got.is_empty(),
        "ordinary owned neighbor is independently valid: {got:#?}"
    );
    refused(
        &owned,
        IMAGE,
        SOURCE,
        "module photos.app\n",
        "PW0371",
        "an asset requires a public asset-bearing table",
    );
}
