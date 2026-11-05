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
opaque type InteractionId = String
command keep(id: Int, alt: String) -> Result<(),PostImage.Error>
    idempotent_by InteractionId
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Posts.put(Posts.Post { id: id, image: Some(image), alt: alt })
    Ok(())
}
command discard() -> Result<PostImage.Attached,PostImage.Error>
    idempotent_by InteractionId
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    PostImage.discard(current_session(),claim)
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
fn refused(posts: &str, image: &str, source: &str, app: &str, says: &str) {
    let got = diagnostics(posts, image, source, app);
    assert!(
        got.iter().any(|(_, m)| m.contains(says)),
        "expected named {says:?} refusal, got {got:#?}"
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
            let (operation, resource) = if import.name == "claim" {
                ("read", "acquire")
            } else {
                ("write", "release")
            };
            let caps = import
                .capabilities
                .iter()
                .map(|c| c.name())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                caps,
                [
                    format!("database.{operation}<PostImage>"),
                    format!("resource.{resource}<PostImage.Claim>")
                ]
                .into_iter()
                .collect(),
                "resource-only import {}",
                import.key()
            );
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
        "asset field",
    );
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "Option<String>"),
        IMAGE,
        SOURCE,
        APP,
        "reference type",
    );
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "List<PostImage.Ref>"),
        IMAGE,
        SOURCE,
        APP,
        "top-level",
    );
    refused(
        &POSTS.replace("    key id", "    key id\n    private image"),
        IMAGE,
        SOURCE,
        APP,
        "public asset-bearing table",
    );
    refused(
        POSTS,
        IMAGE,
        &SOURCE.replace("holds PostImage, Posts", "holds Posts"),
        APP,
        "same explicit source",
    );
    refused(
        POSTS,
        IMAGE,
        SOURCE,
        &APP.replace("    idempotent_by InteractionId\n", ""),
        "idempotent_by",
    );
}

#[test]
fn claims_and_lookalike_reference_records_cannot_be_stored_as_assets() {
    refused(
        &POSTS.replace("Option<PostImage.Ref>", "Option<PostImage.Claim>"),
        IMAGE,
        SOURCE,
        APP,
        "claim",
    );
    let image = IMAGE.to_string() + "\ntype Ref = Ref { key: String }\n";
    refused(POSTS, &image, SOURCE, APP, "generated upload");
    let image = IMAGE.to_string() + "\nfn publish() -> Int { 1 }\n";
    refused(POSTS, &image, SOURCE, APP, "generated upload");
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
    refused(&posts, IMAGE, SOURCE, "module photos.app\n", "upload claim");
}

#[test]
fn a_persisted_reference_requires_its_asset_association_and_top_level_shape() {
    let unannotated = POSTS.replace("    asset image from PostImage\n", "");
    refused(&unannotated, IMAGE, SOURCE, APP, "asset association");
    let nested = unannotated
        .replace(
            "type Post =",
            "type Wrapper = Wrapper { inner: PostImage.Ref }\ntype Post =",
        )
        .replace("Option<PostImage.Ref>", "Option<Wrapper>");
    refused(&nested, IMAGE, SOURCE, "module photos.app\n", "top-level");
}

#[test]
fn generated_upload_visibility_and_interface_identities_are_not_silently_broadened() {
    refused(
        POSTS,
        &IMAGE.replace("upload PostImage", "private upload PostImage"),
        SOURCE,
        APP,
        "public upload declaration",
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
            .any(|d| d.message.contains("generated upload interface")),
        "two names emit one upload interface: {got:#?}"
    );
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
        );
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
        "public asset-bearing table",
    );
}
