//! Real row transactions, with trusted proposals standing in for authenticated
//! leases. No storage or claim-authority acceptance follows from these tests.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/assets.rs");
use pw_host::assets::{Descriptor, Reference};
fn database() -> MemoryTables {
    let p = asset_program();
    MemoryTables::new_with_uploads(p.tables, &p.uploads).assert_ok("explicit upload-bound adapter")
}
fn candidate(generation: char) -> Descriptor {
    Descriptor {
        reference: Reference {
            key: "a".repeat(64),
            generation: generation.to_string().repeat(32),
            kind: "png".into(),
            width: 6,
            height: 4,
        },
        bytes: 17,
    }
}
fn row(id: i64, image: Option<&Reference>) -> serde_json::Value {
    json!({"id":id,"image":image,"alt":"actual row"})
}
fn visible(db: &MemoryTables) -> Option<Reference> {
    db.asset_reference("PostImage", &"a".repeat(64), "png")
        .assert_ok("resolve registry")
}
fn seed(db: &MemoryTables) -> Reference {
    let mut c = db.begin().assert_ok("row integration remains valid");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("verified proposal");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row integration remains valid");
    c.commit().assert_ok("publish row and descriptor");
    r
}
#[test]
fn upload_rows_and_registry_publish_together_and_dropped_preparation_publishes_neither() {
    let db = database();
    let mut c = db.begin().assert_ok("row integration remains valid");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("row integration remains valid");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row integration remains valid");
    assert!(visible(&db).is_none());
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("row integration remains valid"),
        json!(null)
    );
    let p = c.prepare().assert_ok("row integration remains valid");
    let plans = p.asset_plans().assert_ok("row integration remains valid");
    assert_eq!(plans.len(), 1);
    assert!(plans.first().assert_ok("a generation plan exists").keep_new);
    assert_eq!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .generation
            .references
            .get("PostImage"),
        Some(&1)
    );
    assert!(visible(&db).is_none());
    drop(p);
    assert!(visible(&db).is_none());
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("row integration remains valid"),
        json!(null)
    );
    assert_eq!(seed(&db), r);
    assert_eq!(visible(&db), Some(r));
    assert!(db.asset_operations() > 0);
}
#[test]
fn rejected_writes_and_final_moves_cannot_drift_reference_counts() {
    let db = database();
    let r = seed(&db);
    let mut c = db.begin().assert_ok("row integration remains valid");
    assert!(matches!(
        c.write("pw:tables/assets#insert", &[row(1, None)]),
        Err(pw_table_memory::Error::Conflict { .. })
    ));
    c.commit().assert_ok("row integration remains valid");
    assert_eq!(visible(&db), Some(r.clone()));
    for reverse in [false, true] {
        let (old, new) = if reverse { (2, 1) } else { (1, 2) };
        let mut c = db.begin().assert_ok("row integration remains valid");
        if reverse {
            c.write("pw:tables/assets#put", &[row(new, Some(&r))])
                .assert_ok("row integration remains valid");
        }
        c.write("pw:tables/assets#delete", &[json!(old)])
            .assert_ok("row integration remains valid");
        if !reverse {
            c.write("pw:tables/assets#put", &[row(new, Some(&r))])
                .assert_ok("row integration remains valid");
        }
        let p = c.prepare().assert_ok("row integration remains valid");
        let plans = p.asset_plans().assert_ok("row integration remains valid");
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans
                .first()
                .assert_ok("a generation plan exists")
                .generation
                .references
                .get("PostImage"),
            Some(&1)
        );
        assert!(
            !plans.first().assert_ok("a generation plan exists").collect
                && !plans.first().assert_ok("a generation plan exists").keep_new
        );
        p.publish().assert_ok("row integration remains valid");
    }
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("row integration remains valid");
    let p = c.prepare().assert_ok("row integration remains valid");
    assert!(
        p.asset_plans()
            .assert_ok("row integration remains valid")
            .first()
            .assert_ok("a generation plan exists")
            .collect
    );
    p.publish().assert_ok("row integration remains valid");
    assert!(visible(&db).is_none());
    assert_eq!(
        db.pending_asset_collections(10)
            .assert_ok("row integration remains valid"),
        [candidate('b')]
    );
}
#[test]
fn plain_stale_and_changed_refs_refuse_the_whole_row_transaction() {
    let db = database();
    let r = candidate('b').reference;
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row integration remains valid");
    assert!(
        c.prepare().is_err(),
        "plain data cannot create registry authority"
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("row integration remains valid"),
        json!(null)
    );
    let r = seed(&db);
    let mut changed = r.clone();
    changed.width = 7;
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#put", &[row(1, Some(&changed))])
        .assert_ok("row integration remains valid");
    assert!(
        c.prepare().is_err(),
        "same count still validates every final descriptor"
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("row integration remains valid"),
        row(1, Some(&r))
    );
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("row integration remains valid");
    c.commit().assert_ok("row integration remains valid");
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#insert", &[row(2, Some(&r))])
        .assert_ok("row integration remains valid");
    assert!(c.prepare().is_err());
    assert!(visible(&db).is_none());
}
#[test]
fn ignored_publication_and_captured_old_collection_do_not_serve_or_remove_new_bytes() {
    let db = database();
    let c = db.begin().assert_ok("row integration remains valid");
    c.propose_asset("PostImage", candidate('b'))
        .assert_ok("row integration remains valid");
    let p = c.prepare().assert_ok("row integration remains valid");
    let plans = p.asset_plans().assert_ok("row integration remains valid");
    assert!(!plans.first().assert_ok("a generation plan exists").keep_new);
    assert!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .used_publications
            .is_empty()
    );
    p.publish().assert_ok("row integration remains valid");
    assert!(visible(&db).is_none());
    let c = db.begin().assert_ok("row integration remains valid");
    assert!(c.propose_asset("PostImage", candidate('b')).is_err());
    drop(c);
    let mut c = db.begin().assert_ok("row integration remains valid");
    let old = c
        .propose_asset("PostImage", candidate('c'))
        .assert_ok("row integration remains valid");
    c.write("pw:tables/assets#put", &[row(1, Some(&old))])
        .assert_ok("row integration remains valid");
    c.commit().assert_ok("row integration remains valid");
    let mut c = db.begin().assert_ok("row integration remains valid");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("row integration remains valid");
    c.commit().assert_ok("row integration remains valid");
    let mut c = db.begin().assert_ok("row integration remains valid");
    let fresh = c
        .propose_asset("PostImage", candidate('d'))
        .assert_ok("row integration remains valid");
    c.write("pw:tables/assets#put", &[row(2, Some(&fresh))])
        .assert_ok("row integration remains valid");
    c.commit().assert_ok("row integration remains valid");
    assert_ne!(fresh.generation, old.generation);
    db.asset_collected(&old.generation)
        .assert_ok("row integration remains valid");
    assert_eq!(visible(&db), Some(fresh));
    assert!(
        db.pending_asset_collections(10)
            .assert_ok("row integration remains valid")
            .is_empty()
    );
    db.fail_closed();
    assert!(db.asset_reference("PostImage", &old.key, "png").is_err());
}

fn multi_database() -> MemoryTables {
    let program = built_from(&[
        (
            "PostImage.pw",
            r#"module PostImage
upload PostImage
    route "/uploads/photo"
    serves "/photos"
    max_bytes 5_000_000
    types png
    max_width 4096
    max_height 4096
"#,
        ),
        (
            "Avatar.pw",
            r#"module Avatar
upload Avatar
    route "/uploads/avatar"
    serves "/avatars"
    max_bytes 100_000
    types png
    max_width 64
    max_height 64
"#,
        ),
        (
            "Assets.pw",
            r#"module Assets
import PostImage
import Avatar
type Item = Item { id: Int, image: Option<PostImage.Ref>, thumb: Option<PostImage.Ref>, avatar: Option<Avatar.Ref> }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
    asset thumb from PostImage
    asset avatar from Avatar
"#,
        ),
        (
            "AssetData.pw",
            r#"module AssetData
import PostImage
import Avatar
import Assets
source AssetData
    holds PostImage, Avatar, Assets
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
        ),
    ]);
    MemoryTables::new_with_uploads(program.tables, &program.uploads)
        .assert_ok("multi-asset schema binds")
}
#[test]
fn duplicate_fields_and_independent_declarations_keep_exact_final_counts_and_serving() {
    let db = multi_database();
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("photo claim");
    c.write(
        "pw:tables/assets#insert",
        &[json!({"id":1,"image":r,"thumb":r,"avatar":null})],
    )
    .assert_ok("two uses");
    let p = c.prepare().assert_ok("prepare");
    assert_eq!(
        p.asset_plans()
            .assert_ok("plans")
            .first()
            .assert_ok("a generation plan exists")
            .generation
            .references
            .get("PostImage"),
        Some(&2)
    );
    p.publish().assert_ok("commit");
    let mut c = db.begin().assert_ok("begin");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":r,"thumb":r,"avatar":r})],
    )
    .assert_ok("structural input");
    assert!(c.prepare().is_err(), "Photo liveness cannot enroll Avatar");
    let mut c = db.begin().assert_ok("begin");
    let avatar = c
        .propose_asset("Avatar", candidate('c'))
        .assert_ok("independent avatar claim");
    assert_eq!(avatar, r);
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":r,"thumb":r,"avatar":avatar})],
    )
    .assert_ok("three uses");
    let p = c.prepare().assert_ok("prepare");
    let plans = p.asset_plans().assert_ok("plans");
    assert_eq!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .generation
            .references
            .get("PostImage"),
        Some(&2)
    );
    assert_eq!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .generation
            .references
            .get("Avatar"),
        Some(&1)
    );
    assert!(!plans.first().assert_ok("a generation plan exists").keep_new);
    p.publish().assert_ok("commit");
    let mut c = db.begin().assert_ok("begin");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":null,"thumb":null,"avatar":avatar})],
    )
    .assert_ok("remove two uses");
    c.commit().assert_ok("commit");
    assert!(visible(&db).is_none());
    assert_eq!(
        db.asset_reference("Avatar", &avatar.key, "png")
            .assert_ok("avatar resolver"),
        Some(avatar)
    );
    assert!(db.pending_asset_collections(1).assert_ok("jobs").is_empty());
}
#[test]
fn rejected_duplicate_with_a_fresh_publication_settles_no_untouched_old_row_as_use() {
    let db = database();
    let r = seed(&db);
    let mut c = db.begin().assert_ok("begin");
    assert_eq!(
        c.propose_asset("PostImage", candidate('c'))
            .assert_ok("same bytes"),
        r
    );
    assert!(matches!(
        c.write("pw:tables/assets#insert", &[row(1, Some(&r))]),
        Err(pw_table_memory::Error::Conflict { .. })
    ));
    let p = c.prepare().assert_ok("prepare rejected write");
    let plans = p.asset_plans().assert_ok("plans");
    assert_eq!(plans.len(), 1);
    assert!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .used_publications
            .is_empty()
    );
    assert_eq!(
        plans
            .first()
            .assert_ok("a generation plan exists")
            .generation
            .references
            .get("PostImage"),
        Some(&1)
    );
    p.publish().assert_ok("commit");
    assert_eq!(visible(&db), Some(r));
}

#[test]
fn repeated_writes_of_one_key_keep_its_first_committed_descriptor_and_final_use() {
    let db = database();
    let old = seed(&db);
    let mut c = db.begin().assert_ok("begin");
    let mut incoming = candidate('c');
    incoming.reference.key = "d".repeat(64);
    let unused = c
        .propose_asset("PostImage", incoming)
        .assert_ok("fresh different image");
    c.write("pw:tables/assets#put", &[row(1, Some(&unused))])
        .assert_ok("transient replacement");
    c.write("pw:tables/assets#put", &[row(1, Some(&old))])
        .assert_ok("restore final descriptor");
    let p = c.prepare().assert_ok("first old and final row only");
    let plans = p.asset_plans().assert_ok("plans");
    assert_eq!(plans.len(), 2);
    let kept = plans
        .iter()
        .find(|p| p.generation.descriptor.reference == old)
        .assert_ok("old plan");
    assert_eq!(kept.generation.references.get("PostImage"), Some(&1));
    assert!(!kept.collect);
    let ignored = plans
        .iter()
        .find(|p| p.generation.descriptor.reference == unused)
        .assert_ok("ignored plan");
    assert!(ignored.used_publications.is_empty() && !ignored.keep_new);
    p.publish().assert_ok("commit");
    assert_eq!(visible(&db), Some(old));
    assert!(
        db.asset_reference("PostImage", &unused.key, "png")
            .assert_ok("unused resolver")
            .is_none()
    );
}
