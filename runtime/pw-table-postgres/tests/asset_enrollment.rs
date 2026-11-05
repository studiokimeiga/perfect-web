//! Actual PostgreSQL schema/metadata transactions. MarkerStore models only
//! enrollment; physical provider and row/claim lifecycle proofs are separate.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/assets.rs");
mod common;
use pw_host::assets::{Descriptor, Namespace, Storage};
use pw_table_postgres::{Options, PgTables, Uploads};
use std::{collections::BTreeSet, io, sync::Mutex};
#[derive(Default)]
struct MarkerStore {
    names: Mutex<BTreeSet<Namespace>>,
}
impl Storage for MarkerStore {
    fn enroll(&self, n: &Namespace, fresh: bool) -> io::Result<()> {
        let mut names = self.names.lock().unwrap();
        if fresh {
            if !names.insert(n.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "namespace reserved",
                ));
            }
        } else if !names.contains(n) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "missing namespace marker",
            ));
        }
        Ok(())
    }
    fn keep_new(&self, _: &Namespace, _: &Descriptor, _: &[u8]) -> io::Result<()> {
        Err(io::Error::other("enrollment fixture has no physical bytes"))
    }
    fn get_generation(&self, _: &Namespace, _: &Descriptor) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::other("enrollment fixture has no physical bytes"))
    }
    fn collect_generation(&self, _: &Namespace, _: &Descriptor) -> io::Result<()> {
        Err(io::Error::other("enrollment fixture has no physical bytes"))
    }
}
fn model(field: &str, more: bool) -> Program {
    let (field, clause) = match field {
        "caption" => (", caption: Option<String>", ""),
        "image" => (
            ", extra: Option<PostImage.Ref>",
            "    asset extra from PostImage\n",
        ),
        _ => ("", ""),
    };
    let table = format!(
        r#"module Assets
import PostImage
type Item = Item {{ id: Int, image: Option<PostImage.Ref>, alt: String{field} }}
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
{clause}"#
    );
    let other = if more {
        r#"module MoreAssets
import PostImage
type Other = Other { id: Int, image: Option<PostImage.Ref> }
table MoreAssets
    row Other
    key id
    bounded 100
    asset image from PostImage
"#
    } else {
        "module MoreAssets\n"
    };
    let source = format!(
        r#"module AssetData
import PostImage
import Assets
import MoreAssets
source AssetData
    holds PostImage, Assets{}
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
        if more { ", MoreAssets" } else { "" }
    );
    built_from(&[
        (
            "PostImage.pw",
            r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#,
        ),
        ("Assets.pw", &table),
        ("MoreAssets.pw", other),
        ("AssetData.pw", &source),
    ])
}
fn open(
    s: &common::Scratch,
    p: Program,
    store: &Arc<MarkerStore>,
    candidate: u8,
) -> Result<PgTables, pw_table_postgres::Error> {
    PgTables::open_with_uploads(
        &s.url,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([candidate; 16]),
        },
    )
}
fn rehash(p: &mut pw_core::tables::TablesJson) {
    p.hash = pw_core::tables::Schema::of(p.tables.iter().map(|t| t.schema.clone()).collect()).hash;
    p.migration.to = p.hash.clone();
}
#[test]
fn fresh_namespace_and_immutable_metadata_commit_then_reopen_uses_the_recorded_namespace() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_enroll");
    let store = Arc::new(MarkerStore::default());
    let db = open(&s, model("", false), &store, 1).assert_ok("fresh enrollment");
    assert_eq!(db.upload_namespace(), Some(&Namespace::from_bytes([1; 16])));
    assert_eq!(
        s.texts("SELECT namespace FROM pw_asset_namespace"),
        [Namespace::from_bytes([1; 16]).hex()]
    );
    assert_eq!(
        s.texts("SELECT name FROM pw_asset_declarations"),
        ["PostImage"]
    );
    assert_eq!(
        s.texts("SELECT table_name || '.' || field_name || ':' || upload FROM pw_asset_bindings"),
        ["Assets.image:PostImage"]
    );
    assert_eq!(
        s.texts("SELECT version FROM pw_platform ORDER BY version")
            .last()
            .map(String::as_str),
        Some("0004_upload_generations")
    );
    assert!(
        db.begin().is_err(),
        "enrollment alone must not enable unfinished row lifecycle"
    );
    let other = open(&s, model("", false), &store, 2).assert_ok("existing enrollment");
    assert_eq!(other.upload_namespace(), db.upload_namespace());
    assert_eq!(
        store.names.lock().unwrap().len(),
        1,
        "reopen does not enroll its fresh candidate"
    );
}
#[test]
fn missing_existing_marker_or_a_different_storage_root_refuses_without_schema_changes() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_marker");
    let store = Arc::new(MarkerStore::default());
    let db = open(&s, model("", false), &store, 1).assert_ok("fresh");
    let ns = db.upload_namespace().cloned().assert_ok("namespace");
    let hashes = s.texts("SELECT hash FROM pw_schema ORDER BY applied");
    assert!(
        open(
            &s,
            model("caption", false),
            &Arc::new(MarkerStore::default()),
            2
        )
        .is_err()
    );
    store.names.lock().unwrap().remove(&ns);
    assert!(open(&s, model("caption", false), &store, 2).is_err());
    assert_eq!(
        s.texts("SELECT hash FROM pw_schema ORDER BY applied"),
        hashes
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["0"]);
}
#[test]
fn changed_base_declaration_refuses_before_an_additive_application_column_is_applied() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_policy");
    let store = Arc::new(MarkerStore::default());
    open(&s, model("", false), &store, 1).assert_ok("fresh");
    let mut changed = model("caption", false);
    let mut uploads: serde_json::Value = serde_json::from_str(&changed.uploads).unwrap();
    uploads[0]["max_bytes"] = json!(4_000_000);
    changed.uploads = uploads.to_string();
    assert!(open(&s, changed, &store, 2).is_err());
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["0"]);
    assert_eq!(
        s.texts("SELECT declaration->>'max_bytes' FROM pw_asset_declarations"),
        ["5000000"]
    );
}
#[test]
fn an_unbound_manifest_cannot_remove_prior_asset_associations() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_remove");
    let store = Arc::new(MarkerStore::default());
    open(&s, model("", false), &store, 1).assert_ok("fresh");
    let mut unbound = model("", false).tables;
    for t in &mut unbound.tables {
        t.schema.assets.clear();
    }
    rehash(&mut unbound);
    let refused = PgTables::open(&url, s.schema(), unbound);
    assert!(
        matches!(refused,Err(pw_table_postgres::Error::Schema(ref why)) if why.contains("upload")),
        "prior upload metadata must be checked even with no asset clauses"
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_bindings"),
        ["1"]
    );
}
#[test]
fn adding_an_optional_non_asset_field_and_a_wholly_new_asset_table_preserves_enrollment() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_additive");
    let store = Arc::new(MarkerStore::default());
    let first = open(&s, model("", false), &store, 1).assert_ok("fresh");
    let next = open(&s, model("caption", true), &store, 2).assert_ok("safe additive migration");
    assert_eq!(next.upload_namespace(), first.upload_namespace());
    assert_eq!(
        s.texts("SELECT table_name || '.' || field_name FROM pw_asset_bindings ORDER BY 1"),
        ["Assets.image", "MoreAssets.image"]
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["1"]);
}
#[test]
fn adding_an_asset_clause_to_an_existing_table_even_on_a_new_optional_field_refuses() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_existing");
    let store = Arc::new(MarkerStore::default());
    open(&s, model("", false), &store, 1).assert_ok("fresh");
    assert!(open(&s, model("image", false), &store, 2).is_err());
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='extra'"),["0"]);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_bindings"),
        ["1"]
    );
}
#[test]
fn old_structural_reference_data_is_not_implicitly_enrolled_as_public_upload_authority() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_legacy_ref");
    let store = Arc::new(MarkerStore::default());
    let mut raw = model("", false).tables;
    for t in &mut raw.tables {
        t.schema.assets.clear();
    }
    rehash(&mut raw);
    let plain = PgTables::open(&url, s.schema(), raw).assert_ok("ordinary structural row schema");
    plain.transaction(|c|c.write("pw:tables/assets#insert",&[json!({"id":1,"alt":"ordinary data","image":{"key":"a".repeat(64),"generation":"b".repeat(32),"kind":"png","width":6,"height":4}})])).assert_ok("ordinary structural data");
    assert!(
        open(&s, model("", false), &store, 1).is_err(),
        "existing Ref-shaped fields need explicit conversion beyond this seam"
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_namespace"),
        ["0"]
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM \"Assets\""), ["1"]);
}
#[test]
fn failed_application_ddl_rolls_back_namespace_rows_but_keeps_the_storage_reservation() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_ddl_rollback");
    let store = Arc::new(MarkerStore::default());
    s.sql(&format!("CREATE SCHEMA \"{}\"; SET search_path TO \"{}\"; CREATE TABLE \"Assets\" (foreign_column bigint)",s.schema,s.schema)).assert_ok("unmanaged application table");
    assert!(open(&s, model("", false), &store, 1).is_err());
    assert!(
        store
            .names
            .lock()
            .unwrap()
            .contains(&Namespace::from_bytes([1; 16])),
        "storage enrollment precedes application DDL/commit"
    );
    assert_eq!(
        s.texts("SELECT coalesce(to_regclass('pw_asset_namespace')::text,'missing')"),
        ["missing"]
    );
    s.sql("DROP TABLE \"Assets\"")
        .assert_ok("remove unmanaged table");
    assert!(
        open(&s, model("", false), &store, 1).is_err(),
        "the same physical namespace reservation is never reused"
    );
    open(&s, model("", false), &store, 2).assert_ok("new candidate after failed enrollment");
}

#[test]
fn changed_binding_identity_refuses_before_additive_application_ddl() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_rebind");
    let store = Arc::new(MarkerStore::default());
    open(&s, model("", false), &store, 1).assert_ok("fresh");
    let mut changed = model("caption", false);
    let mut uploads: serde_json::Value = serde_json::from_str(&changed.uploads).unwrap();
    let mut avatar = uploads[0].clone();
    avatar["name"] = json!("Avatar");
    avatar["module"] = json!("Avatar");
    avatar["route"] = json!("/uploads/avatar");
    avatar["serves"] = json!("/avatars");
    uploads.as_array_mut().unwrap().push(avatar);
    changed.uploads = uploads.to_string();
    changed.tables.tables[0].schema.assets[0].upload = "Avatar".into();
    rehash(&mut changed.tables);
    assert!(open(&s, changed, &store, 2).is_err());
    assert_eq!(
        s.texts("SELECT upload FROM pw_asset_bindings"),
        ["PostImage"]
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["0"]);
}
#[test]
fn suppressed_namespace_or_binding_insertion_refuses_and_rolls_back_the_metadata_transaction() {
    let Some(url) = common::database() else {
        return;
    };
    for suppressed in ["pw_asset_namespace", "pw_asset_bindings"] {
        let s = common::Scratch::new(&url, "asset_suppressed");
        let store = Arc::new(MarkerStore::default());
        let mut empty = model("", false).tables;
        empty.tables.clear();
        rehash(&mut empty);
        PgTables::open(&url, s.schema(), empty).assert_ok("platform on empty app schema");
        s.sql(&format!("CREATE FUNCTION suppress_asset_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress BEFORE INSERT ON {suppressed} FOR EACH ROW EXECUTE FUNCTION suppress_asset_insert()" )).assert_ok("suppress actual metadata insert");
        assert!(
            open(&s, model("", false), &store, 1).is_err(),
            "a suppressed {suppressed} insert cannot count as enrolled"
        );
        assert_eq!(
            s.texts("SELECT count(*)::text FROM pw_asset_namespace"),
            ["0"]
        );
        assert_eq!(
            s.texts("SELECT count(*)::text FROM pw_asset_declarations"),
            ["0"]
        );
        assert_eq!(
            s.texts("SELECT count(*)::text FROM pw_asset_bindings"),
            ["0"]
        );
        assert_eq!(
            s.texts("SELECT coalesce(to_regclass('\"Assets\"')::text,'missing')"),
            ["missing"]
        );
        s.sql(&format!("DROP TRIGGER suppress ON {suppressed}"))
            .assert_ok("valid neighbor");
        open(&s, model("", false), &store, 2).assert_ok("new candidate after known rollback");
    }
}
#[test]
fn every_asset_platform_relation_name_is_reserved_before_connecting_and_a_neighbor_is_ordinary() {
    let named = |name: &str| {
        let mut p = model("", false).tables;
        let table = &mut p.tables[0];
        table.schema.assets.clear();
        table.schema.name = name.into();
        table.interface = format!("pw:tables/{}", name.replace('_', "-"));
        for f in &mut table.functions {
            f.import = f.import.replace("pw:tables/assets", &table.interface);
        }
        rehash(&mut p);
        p
    };
    for name in [
        "pw_asset_namespace",
        "pw_asset_namespace_pkey",
        "pw_asset_namespace_namespace_key",
        "pw_asset_live_key",
        "pw_asset_generations",
        "pw_asset_references_pkey",
        "pw_asset_collections_pkey",
        "pw_asset_future_index",
    ] {
        let result = PgTables::open("not a database URL", Some("unused"), named(name));
        assert!(
            matches!(result,Err(pw_table_postgres::Error::Schema(ref why)) if why.contains(name)),
            "{name} must refuse before connecting"
        );
    }
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_name_neighbor");
    PgTables::open(&url, s.schema(), named("pw_assets_app"))
        .assert_ok("close nonreserved neighbor");
}
