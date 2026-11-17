//! Real row transactions, with trusted proposals standing in for authenticated
//! leases. No storage or claim-authority acceptance follows from these tests.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/assets.rs");
use pw_host::assets::{Descriptor, Reference};
use pw_table_memory::Work;

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
    refused(c.prepare(), "asset reference names an unknown generation");
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
    refused(
        c.prepare(),
        "row reference differs from the immutable generation descriptor",
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
    refused(
        c.prepare(),
        "reference is not LIVE for this declaration and has no fresh publication",
    );
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
    refused(
        c.propose_asset("PostImage", candidate('b')),
        "generation identity has already been reserved",
    );
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
    assert!(matches!(
        db.asset_reference("PostImage", &old.key, "png"),
        Err(pw_table_memory::Error::StoragePoisoned)
    ));
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
    refused(
        c.prepare(),
        "reference is not LIVE for this declaration and has no fresh publication",
    );
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

#[test]
fn registry_accounting_counts_each_actual_path_without_hiding_row_primitive_cost() {
    let db = database();
    assert_eq!(db.asset_operations(), 0);
    assert_eq!(db.statements(), 0);
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("proposal");
    assert_eq!(db.asset_operations(), 1);
    assert_eq!(db.statements(), 0);
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row insert");
    assert_eq!(db.statements(), 1);
    assert_eq!(db.asset_operations(), 1);
    let p = c.prepare().assert_ok("prepare");
    assert_eq!(db.asset_operations(), 2);
    p.publish().assert_ok("publish");
    assert_eq!(db.asset_operations(), 3);
    assert_eq!(
        db.asset_reference("PostImage", &r.key, "png")
            .assert_ok("resolve"),
        Some(r.clone())
    );
    assert_eq!(db.asset_operations(), 4);
    assert_eq!(db.statements(), 1);
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("delete");
    let p = c.prepare().assert_ok("prepare removal");
    assert_eq!(db.asset_operations(), 5);
    p.publish().assert_ok("publish removal");
    assert_eq!(db.asset_operations(), 6);
    assert!(
        db.pending_asset_collections(0)
            .assert_ok("bounded jobs")
            .is_empty()
    );
    assert_eq!(db.asset_operations(), 7);
    assert_eq!(
        db.pending_asset_collections(1).assert_ok("one job"),
        [candidate('b')]
    );
    assert_eq!(db.asset_operations(), 8);
    db.asset_collected(&r.generation).assert_ok("completion");
    assert_eq!(db.asset_operations(), 9);
    assert!(
        db.asset_reference("PostImage", &r.key, "png")
            .assert_ok("no reference")
            .is_none()
    );
    assert_eq!(db.asset_operations(), 10);
    assert_eq!(db.statements(), 2);
}
#[test]
fn cleanup_enumeration_obeys_zero_one_and_two_bounds_and_completes_only_one_captured_job() {
    let db = database();
    let first = seed(&db);
    let mut c = db.begin().assert_ok("begin");
    let mut other = candidate('c');
    other.reference.key = "d".repeat(64);
    let second = c
        .propose_asset("PostImage", other.clone())
        .assert_ok("second bytes");
    c.write("pw:tables/assets#insert", &[row(2, Some(&second))])
        .assert_ok("second row");
    c.commit().assert_ok("commit");
    let mut c = db.begin().assert_ok("begin");
    for id in [1, 2] {
        c.write("pw:tables/assets#delete", &[json!(id)])
            .assert_ok("remove row");
    }
    c.commit().assert_ok("commit both cleanup jobs");
    assert!(
        db.pending_asset_collections(0)
            .assert_ok("zero bound")
            .is_empty()
    );
    assert_eq!(
        db.pending_asset_collections(1).assert_ok("one bound"),
        [candidate('b')]
    );
    assert_eq!(
        db.pending_asset_collections(2).assert_ok("two bound"),
        [candidate('b'), other.clone()]
    );
    db.asset_collected(&first.generation)
        .assert_ok("complete first only");
    assert_eq!(
        db.pending_asset_collections(2).assert_ok("remaining job"),
        [other]
    );
    assert!(visible(&db).is_none());
}

#[test]
fn prefix_delete_tracks_every_removed_asset_and_preserves_an_untouched_group() {
    let p = built_from(&[
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
            "Assets.pw",
            r#"module Assets
import PostImage
type Item = Item { group: String, id: Int, image: Option<PostImage.Ref> }
table Assets
    row Item
    key group, id
    bounded 100
    asset image from PostImage
"#,
        ),
        (
            "AssetData.pw",
            r#"module AssetData
import PostImage
import Assets
source AssetData
    holds PostImage, Assets
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
        ),
    ]);
    let db =
        MemoryTables::new_with_uploads(p.tables, &p.uploads).assert_ok("composite asset table");
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("claim");
    for (group, id) in [("a", 1), ("a", 2), ("b", 1)] {
        c.write(
            "pw:tables/assets#insert",
            &[json!({"group":group,"id":id,"image":r})],
        )
        .assert_ok("insert");
    }
    c.commit().assert_ok("commit three uses");
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#delete-of-group", &[json!("a")])
        .assert_ok("delete group a");
    let p = c.prepare().assert_ok("prepare prefix removal");
    let plans = p.asset_plans().assert_ok("plans");
    let plan = plans
        .first()
        .assert_ok("prefix has a final generation plan");
    assert_eq!(plan.generation.references.get("PostImage"), Some(&1));
    assert!(!plan.collect);
    p.publish().assert_ok("commit prefix removal");
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!("a"), json!(1)])
            .assert_ok("removed"),
        json!(null)
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!("b"), json!(1)])
            .assert_ok("untouched"),
        json!({"group":"b","id":1,"image":r})
    );
    assert_eq!(visible(&db), Some(r));
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#delete-of-group", &[json!("b")])
        .assert_ok("delete group b");
    let p = c.prepare().assert_ok("prepare last group");
    assert!(
        p.asset_plans()
            .assert_ok("plans")
            .first()
            .assert_ok("last generation plan")
            .collect
    );
    p.publish().assert_ok("commit last group");
    assert!(visible(&db).is_none());
    assert_eq!(
        db.pending_asset_collections(1).assert_ok("job"),
        [candidate('b')]
    );
}

#[test]
fn captured_asset_publisher_borrows_only_its_original_memory_command_and_never_ends_it() {
    let db = database();
    let mut c = db.begin().assert_ok("original command");
    let publish = c.asset_publisher();
    let image = publish("PostImage", candidate('b'))
        .assert_ok("borrowed publication leaves original command open");
    c.write("pw:tables/assets#insert", &[row(1, Some(&image))])
        .assert_ok("original command still owns its turn");
    let p = c.prepare().assert_ok("original command prepares once");
    assert!(matches!(
        publish("PostImage", candidate('c')),
        Err(pw_table_memory::Error::Ended)
    ));
    p.publish().assert_ok("original owner publishes");
    let mut next = db.begin().assert_ok("new command owns new turn");
    assert!(matches!(
        publish("PostImage", candidate('c')),
        Err(pw_table_memory::Error::Ended)
    ));
    next.write("pw:tables/assets#insert", &[row(2, Some(&image))])
        .assert_ok("stale callback did not end the new command");
    next.commit().assert_ok("new command remains valid");
    assert_eq!(visible(&db), Some(image));
}

/// **A committed generation's descriptor is the one its publication
/// recorded** (ADR-0440, L5b: what the server holds the bytes it serves
/// to): none for another declaration or kind, and none once the last row
/// naming it is gone.
#[test]
fn a_committed_generations_descriptor_is_its_publications_and_goes_with_its_last_row() {
    let db = database();
    let published = seed(&db);
    let key = "a".repeat(64);
    let found = db
        .asset_descriptor("PostImage", &key, "png")
        .assert_ok("resolve registry")
        .assert_ok("a committed generation");
    assert_eq!(found.reference, published);
    assert_eq!(found.bytes, 17);
    assert_eq!(
        db.asset_descriptor("Avatar", &key, "png")
            .assert_ok("resolve"),
        None
    );
    assert_eq!(
        db.asset_descriptor("PostImage", &key, "jpeg")
            .assert_ok("resolve"),
        None
    );
    let mut c = db.begin().assert_ok("a command");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("the last row goes");
    c.commit().assert_ok("committed");
    assert_eq!(
        db.asset_descriptor("PostImage", &key, "png")
            .assert_ok("resolve"),
        None
    );
}

/// **What each command works on** (L7's G10, ADR-0440 amended 2026-11-17),
/// by its compiled artifact's imports: `Keep`, which claims and publishes,
/// and `Forget`, which deletes an asset's row, work on the uploads; `Note`,
/// which writes the notes alone, and `Attached`, a session's read of its
/// uploads, on their tables alone. A program bound to no upload works on
/// its tables alone, whatever its commands import.
#[test]
fn each_command_works_on_the_uploads_only_where_it_claims_or_writes_an_asset() {
    let program = asset_work_program();
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .assert_ok("the compiler's upload binding");
    let work = |manifest: &pw_host::tables::Manifest, id: &str| {
        manifest.work(program.component(id).imports.iter().map(String::as_str))
    };
    for (id, expected) in [
        ("Keep", Work::Uploads),
        ("Forget", Work::Uploads),
        ("Note", Work::Tables),
        ("Attached", Work::Tables),
    ] {
        assert_eq!(work(&bound, id), expected, "{id}");
    }
    assert_eq!(
        work(&program.manifest, "Keep"),
        Work::Tables,
        "a program bound to no upload works on its tables alone"
    );
    // Each import by itself: a discard works on the uploads as a claim
    // does; an asset table's read, or a table's write that keeps no asset,
    // does not.
    for (import, expected) in [
        ("pw:uploads/post-image#discard", Work::Uploads),
        ("pw:uploads/post-image#attached", Work::Tables),
        ("pw:tables/assets#put", Work::Uploads),
        ("pw:tables/assets#get", Work::Tables),
        ("pw:tables/notes#put", Work::Tables),
    ] {
        assert_eq!(bound.work([import]), expected, "{import}");
    }
}

/// **A command begun for its tables alone is refused asset work** (L7's
/// G10), as on PostgreSQL, where it holds no uploads' namespace: it writes
/// no table keeping an asset, and publishes no upload, and what it wrote
/// before either is undone with it. Its notes it writes and commits.
#[test]
fn a_command_begun_for_its_tables_alone_is_refused_asset_work_as_on_postgresql() {
    let p = asset_work_program();
    let db = MemoryTables::new_with_uploads(p.tables, &p.uploads).assert_ok("bound adapter");
    let mut c = db.begin_for(Work::Tables).assert_ok("an ordinary command");
    c.write("pw:tables/notes#put", &[json!({"id":1,"text":"kept"})])
        .assert_ok("its notes");
    refused(
        c.write("pw:tables/assets#insert", &[row(1, None)]),
        "which keeps an asset, in a command begun for its tables alone",
    );
    refused(
        c.propose_asset("PostImage", candidate('b')),
        "a command begun for its tables alone publishes no upload",
    );
    // Its commit is refused as its first refusal was, as on PostgreSQL
    // (the review's L5): the note it wrote before is not kept.
    refused(
        c.commit(),
        "which keeps an asset, in a command begun for its tables alone",
    );
    assert!(visible(&db).is_none(), "no generation was published");
    let mut c = db.begin_for(Work::Tables).assert_ok("another");
    c.write("pw:tables/notes#put", &[json!({"id":2,"text":"kept"})])
        .assert_ok("its notes");
    c.commit().assert_ok("an ordinary command commits");
    let mut c = db.begin().assert_ok("a command working on the uploads");
    let r = c
        .propose_asset("PostImage", candidate('b'))
        .assert_ok("publishes");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("writes an asset's row");
    c.commit().assert_ok("and commits");
    assert_eq!(visible(&db), Some(r));
}

/// [`owned_asset_program`]'s posts, bound to its upload (L7's G4).
fn owned_database() -> MemoryTables {
    let p = owned_asset_program();
    MemoryTables::new_with_uploads(p.tables, &p.uploads).assert_ok("an owned public table binds")
}

/// [`candidate`]'s descriptor for bytes of another hash: a second image.
fn other(generation: char) -> Descriptor {
    let mut d = candidate(generation);
    d.reference.key = "d".repeat(64);
    d
}

/// One post, its image `image`, committed through the adapter.
fn posted(db: &MemoryTables, author: &str, id: i64, image: Descriptor) -> Reference {
    let mut c = db.begin().assert_ok("an author's command");
    let r = c.propose_asset("PostImage", image).assert_ok("their image");
    c.write(
        "pw:tables/posts#insert",
        &[owned_post(author, id, Some(&r))],
    )
    .assert_ok("their post");
    c.commit().assert_ok("their post commits");
    r
}

/// **Two owners' posts at one id each keep their own image** (L7's G4,
/// ADR-0440 amended 2026-11-17): a post's key leads with its author, so
/// alice's post 1 and bob's are two rows, and each image is counted by its
/// row's whole key. Alice's delete ends her image's one use alone, collected
/// at its commit; bob's post and image stay, until his delete of every post
/// of his (his key's prefix) ends his.
#[test]
fn two_owners_posts_at_one_id_each_keep_their_own_image_until_its_owner_deletes() {
    let db = owned_database();
    let a = posted(&db, ALICE, 1, candidate('b'));
    let b = posted(&db, BOB, 1, other('c'));
    assert_ne!(a.key, b.key);
    for (author, image) in [(ALICE, &a), (BOB, &b)] {
        assert_eq!(
            db.read("pw:tables/posts#get", &[json!(author), json!(1)])
                .assert_ok("a post"),
            owned_post(author, 1, Some(image)),
            "{author}'s post 1 keeps their own image"
        );
    }
    let mut c = db.begin().assert_ok("alice's delete");
    c.write("pw:tables/posts#delete", &[json!(ALICE), json!(1)])
        .assert_ok("her post 1");
    let p = c.prepare().assert_ok("prepare");
    let plans = p.asset_plans().assert_ok("plans");
    assert_eq!(plans.len(), 1, "her image's plan alone");
    let plan = plans.first().assert_ok("a generation plan exists");
    assert_eq!(plan.generation.descriptor.reference, a);
    assert!(plan.collect, "her image's last use ends");
    p.publish().assert_ok("commit");
    assert!(
        db.asset_reference("PostImage", &a.key, "png")
            .assert_ok("resolve")
            .is_none()
    );
    assert_eq!(
        db.asset_reference("PostImage", &b.key, "png")
            .assert_ok("resolve"),
        Some(b.clone()),
        "bob's image is served still"
    );
    assert_eq!(
        db.pending_asset_collections(10).assert_ok("jobs"),
        [candidate('b')]
    );
    assert_eq!(
        db.read("pw:tables/posts#get", &[json!(BOB), json!(1)])
            .assert_ok("bob's post"),
        owned_post(BOB, 1, Some(&b))
    );
    let mut c = db.begin().assert_ok("bob's delete");
    c.write("pw:tables/posts#delete-of-author", &[json!(BOB)])
        .assert_ok("every post of his");
    c.commit().assert_ok("commit");
    assert!(
        db.asset_reference("PostImage", &b.key, "png")
            .assert_ok("resolve")
            .is_none()
    );
    assert_eq!(
        db.pending_asset_collections(10).assert_ok("jobs"),
        [candidate('b'), other('c')]
    );
}

/// **A reference copied into another owner's post keeps its image after the
/// first owner deletes theirs** (L7's G4): a write may copy a LIVE reference
/// of its declaration, with no claim, whoever's row named it first, and a
/// generation's references are counted per declaration, never per owner.
/// Alice's delete is no erasure: bob's post names her image still, and it is
/// served; his delete ends its last use.
#[test]
fn a_reference_copied_into_another_owners_post_keeps_its_image_after_the_first_deletes() {
    let db = owned_database();
    let r = posted(&db, ALICE, 1, candidate('b'));
    let mut c = db.begin().assert_ok("bob's command");
    c.write("pw:tables/posts#insert", &[owned_post(BOB, 7, Some(&r))])
        .assert_ok("his post, her image's reference copied");
    let p = c.prepare().assert_ok("a copy needs no claim");
    let plans = p.asset_plans().assert_ok("plans");
    let plan = plans.first().assert_ok("a generation plan exists");
    assert_eq!(plan.generation.references.get("PostImage"), Some(&2));
    assert!(!plan.keep_new && !plan.collect);
    p.publish().assert_ok("commit");
    let mut c = db.begin().assert_ok("alice's delete");
    c.write("pw:tables/posts#delete", &[json!(ALICE), json!(1)])
        .assert_ok("her post");
    let p = c.prepare().assert_ok("prepare");
    let plans = p.asset_plans().assert_ok("plans");
    let plan = plans.first().assert_ok("a generation plan exists");
    assert_eq!(plan.generation.references.get("PostImage"), Some(&1));
    assert!(!plan.collect, "not collected while bob's post names it");
    p.publish().assert_ok("commit");
    assert_eq!(visible(&db), Some(r.clone()), "served still");
    assert!(
        db.pending_asset_collections(10)
            .assert_ok("jobs")
            .is_empty()
    );
    let mut c = db.begin().assert_ok("bob's delete");
    c.write("pw:tables/posts#delete", &[json!(BOB), json!(7)])
        .assert_ok("his post");
    c.commit().assert_ok("commit");
    assert!(visible(&db).is_none(), "its last use ended");
    assert_eq!(
        db.pending_asset_collections(10).assert_ok("jobs"),
        [candidate('b')]
    );
}

/// **A post is written by its author alone, its image among it** (L7's G4),
/// through the compiled commands: bob's `Keep` of a post naming alice its
/// author is refused before anything is written; his own post, copying her
/// image's reference, is kept. Alice's `TakeDown` leaves her image served
/// while his post names it, and his ends its last use.
#[test]
fn a_compiled_put_naming_another_author_is_refused_and_ones_own_copy_is_kept() {
    let program = owned_asset_program();
    let db = MemoryTables::new_with_uploads(program.tables.clone(), &program.uploads)
        .assert_ok("bound adapter");
    let r = posted(&db, ALICE, 1, candidate('b'));
    let post = |author: &str, id: i64, alt: &str| {
        Val::Record(vec![
            ("author".into(), Val::String(author.into())),
            ("id".into(), Val::S64(id)),
            (
                "image".into(),
                Val::Option(Some(Box::new(image_reference()))),
            ),
            ("alt".into(), Val::String(alt.into())),
        ])
    };
    // Its words are not hers (G4's review, L1): a forgery written over her
    // post would be told apart from it.
    let refused = command(
        &program,
        &db,
        "Keep",
        BOB,
        &[post(ALICE, 1, "bob's, as alice's")],
    )
    .assert_err("a post of another's is refused");
    assert!(
        refused.contains("whose `author` is not its writer"),
        "{refused}"
    );
    assert_eq!(
        db.read("pw:tables/posts#get", &[json!(ALICE), json!(1)])
            .assert_ok("hers"),
        owned_post(ALICE, 1, Some(&r)),
        "her post is not rewritten"
    );
    assert_eq!(
        command(&program, &db, "Keep", BOB, &[post(BOB, 7, "bob's post 7")]),
        Ok(vec![])
    );
    assert_eq!(
        db.read("pw:tables/posts#get", &[json!(BOB), json!(7)])
            .assert_ok("his"),
        owned_post(BOB, 7, Some(&r))
    );
    assert_eq!(
        command(&program, &db, "TakeDown", ALICE, &[Val::S64(1)]),
        Ok(vec![])
    );
    assert_eq!(visible(&db), Some(r), "his post names it still");
    assert_eq!(
        command(&program, &db, "TakeDown", BOB, &[Val::S64(7)]),
        Ok(vec![])
    );
    assert!(visible(&db).is_none(), "its last use ended");
}

/// **An owned asset table's write works on the uploads** (L7's G4; G10's
/// classifier, unchanged): `Keep` and `TakeDown`, which write the posts and
/// claim nothing, work on the uploads as `Publish` does; `Attached`, and a
/// post's read, on their tables alone. A command begun for its tables alone
/// is refused an owned post's write, as any asset table's.
#[test]
fn an_owned_asset_tables_write_works_on_the_uploads() {
    let program = owned_asset_program();
    let bound = program
        .manifest
        .clone()
        .with_uploads(&program.uploads)
        .assert_ok("the compiler's upload binding");
    for (id, expected) in [
        ("Publish", Work::Uploads),
        ("Keep", Work::Uploads),
        ("TakeDown", Work::Uploads),
        ("Attached", Work::Tables),
    ] {
        let imports = program.component(id).imports.iter().map(String::as_str);
        assert_eq!(bound.work(imports), expected, "{id}");
    }
    for (import, expected) in [
        ("pw:tables/posts#put", Work::Uploads),
        ("pw:tables/posts#delete", Work::Uploads),
        ("pw:tables/posts#delete-of-author", Work::Uploads),
        ("pw:tables/posts#get", Work::Tables),
    ] {
        assert_eq!(bound.work([import]), expected, "{import}");
    }
    let db = owned_database();
    let mut c = db.begin_for(Work::Tables).assert_ok("an ordinary command");
    refused(
        c.write("pw:tables/posts#insert", &[owned_post(ALICE, 1, None)]),
        "which keeps an asset, in a command begun for its tables alone",
    );
}
