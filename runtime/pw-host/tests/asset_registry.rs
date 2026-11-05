//! Arithmetic/identity proofs only; publications here stand in for verified claims.
use pw_host::assets::*;
use serde_json::json;
use std::collections::BTreeMap;
fn manifest() -> Manifest {
    Manifest::from_json(&json!([
        {"name":"Photo","module":"Photo","source":"Data","route":"/uploads/photo","serves":"/photos","max_bytes":5000000,"types":["png","jpeg"],"max_width":4096,"max_height":4096},
        {"name":"Avatar","module":"Avatar","source":"Data","route":"/uploads/avatar","serves":"/avatars","max_bytes":100000,"types":["png"],"max_width":64,"max_height":64}
    ]).to_string()).unwrap()
}
fn descriptor() -> Descriptor {
    Descriptor {
        reference: Reference {
            key: "a".repeat(64),
            generation: "b".repeat(32),
            kind: "png".into(),
            width: 6,
            height: 4,
        },
        bytes: 17,
    }
}
fn publication(name: &str) -> Publication {
    Publication {
        declaration: name.into(),
        descriptor: descriptor(),
    }
}
fn usage(name: &str, before: u64, after: u64) -> ReferenceUse {
    ReferenceUse {
        declaration: name.into(),
        reference: descriptor().reference,
        before,
        after,
    }
}
fn live(counts: &[(&str, u64)]) -> Generation {
    Generation {
        descriptor: descriptor(),
        state: GenerationState::Live,
        references: counts
            .iter()
            .map(|(name, count)| (name.to_string(), *count))
            .collect(),
    }
}

#[test]
fn a_fresh_publication_requires_a_final_row_use_to_become_live() {
    let plan = plan_generation(
        &manifest(),
        None,
        &[publication("Photo")],
        &[usage("Photo", 0, 1)],
    )
    .unwrap();
    assert_eq!(plan.generation, live(&[("Photo", 1)]));
    assert!(plan.keep_new);
    assert!(!plan.collect);
    assert_eq!(
        plan.used_publications.into_iter().collect::<Vec<_>>(),
        ["Photo"]
    );
    let ignored = plan_generation(&manifest(), None, &[publication("Photo")], &[]).unwrap();
    assert_eq!(ignored.generation.state, GenerationState::Tombstone);
    assert!(ignored.generation.references.is_empty());
    assert!(!ignored.keep_new && !ignored.collect);
    assert!(ignored.used_publications.is_empty());
    assert!(ignored.generation.live_reference("Photo").is_none());
    assert!(
        plan_generation(
            &manifest(),
            Some(&ignored.generation),
            &[],
            &[usage("Photo", 0, 1)]
        )
        .is_err()
    );
}
#[test]
fn plain_reference_data_is_not_authority_to_create_a_generation() {
    assert!(plan_generation(&manifest(), None, &[], &[usage("Photo", 0, 1)]).is_err());
    assert!(plan_generation(&manifest(), None, &[], &[usage("Photo", 1, 0)]).is_err());
}
#[test]
fn current_same_declaration_references_allow_copy_but_another_declaration_does_not() {
    let old = live(&[("Photo", 1)]);
    let copied = plan_generation(&manifest(), Some(&old), &[], &[usage("Photo", 0, 2)]).unwrap();
    assert_eq!(copied.generation, live(&[("Photo", 3)]));
    assert!(!copied.keep_new && !copied.collect);
    assert!(plan_generation(&manifest(), Some(&old), &[], &[usage("Avatar", 0, 1)]).is_err());
    let enrolled = plan_generation(
        &manifest(),
        Some(&old),
        &[publication("Avatar")],
        &[usage("Avatar", 0, 1)],
    )
    .unwrap();
    assert_eq!(enrolled.generation, live(&[("Avatar", 1), ("Photo", 1)]));
    assert!(!enrolled.keep_new && !enrolled.collect);
    assert_eq!(
        enrolled.used_publications.into_iter().collect::<Vec<_>>(),
        ["Avatar"]
    );
    assert_eq!(
        old,
        live(&[("Photo", 1)]),
        "planning never edits the committed snapshot"
    );
}
#[test]
fn a_declarations_last_reference_ends_its_adoption_and_serving_even_if_another_keeps_bytes_live() {
    let old = live(&[("Avatar", 1), ("Photo", 1)]);
    let plan = plan_generation(&manifest(), Some(&old), &[], &[usage("Avatar", 1, 0)]).unwrap();
    assert_eq!(plan.generation, live(&[("Photo", 1)]));
    assert!(!plan.collect);
    assert!(plan.generation.live_reference("Avatar").is_none());
    assert!(plan.generation.live_reference("Photo").is_some());
    assert!(
        plan_generation(
            &manifest(),
            Some(&plan.generation),
            &[],
            &[usage("Avatar", 0, 1)]
        )
        .is_err()
    );
    assert!(
        plan_generation(
            &manifest(),
            Some(&plan.generation),
            &[publication("Avatar")],
            &[usage("Avatar", 0, 1)]
        )
        .is_ok()
    );
}
#[test]
fn final_net_moves_do_not_transiently_retire_a_generation_and_unchanged_refs_are_validated() {
    let old = live(&[("Photo", 1)]);
    let moves = [usage("Photo", 1, 0), usage("Photo", 0, 1)];
    for changes in [moves.to_vec(), moves.into_iter().rev().collect()] {
        let moved = plan_generation(&manifest(), Some(&old), &[], &changes).unwrap();
        assert_eq!(moved.generation, old);
        assert!(!moved.collect && !moved.keep_new);
    }
    let unchanged = usage("Photo", 1, 1);
    assert!(
        plan_generation(
            &manifest(),
            Some(&old),
            &[],
            std::slice::from_ref(&unchanged)
        )
        .is_ok()
    );
    let mut malformed = unchanged;
    malformed.reference.width = 7;
    assert!(
        plan_generation(&manifest(), Some(&old), &[], &[malformed]).is_err(),
        "zero net delta cannot mask a changed descriptor"
    );
}
#[test]
fn last_removal_captures_only_the_old_generation_and_same_bytes_require_a_new_identity() {
    let old = live(&[("Photo", 1)]);
    let removed = plan_generation(&manifest(), Some(&old), &[], &[usage("Photo", 1, 0)]).unwrap();
    assert_eq!(removed.generation.state, GenerationState::Tombstone);
    assert!(removed.collect);
    assert!(!removed.keep_new);
    assert!(removed.generation.references.is_empty());
    assert!(
        plan_generation(
            &manifest(),
            Some(&removed.generation),
            &[publication("Photo")],
            &[usage("Photo", 0, 1)]
        )
        .is_err()
    );
    let mut fresh = publication("Photo");
    fresh.descriptor.reference.generation = "c".repeat(32);
    let mut change = usage("Photo", 0, 1);
    change.reference = fresh.descriptor.reference.clone();
    let republished = plan_generation(&manifest(), None, &[fresh], &[change]).unwrap();
    assert_eq!(
        republished.generation.descriptor.reference.key,
        removed.generation.descriptor.reference.key
    );
    assert_ne!(
        republished.generation.descriptor.reference.generation,
        removed.generation.descriptor.reference.generation,
        "captured cleanup cannot identify the new path"
    );
}
#[test]
fn ignored_publications_do_not_enroll_a_declaration_or_count_untouched_rows_as_usage() {
    let old = live(&[("Photo", 1)]);
    for name in ["Photo", "Avatar"] {
        let plan = plan_generation(&manifest(), Some(&old), &[publication(name)], &[]).unwrap();
        assert_eq!(plan.generation, old);
        assert!(plan.used_publications.is_empty());
        assert!(!plan.keep_new && !plan.collect);
    }
    assert!(old.live_reference("Avatar").is_none());
}
#[test]
fn contradictory_counts_overflow_unknown_declarations_and_descriptors_refuse_whole() {
    let old = live(&[("Photo", 1)]);
    for invalid in [usage("Photo", 2, 0), usage("Missing", 0, 1)] {
        assert!(plan_generation(&manifest(), Some(&old), &[], &[invalid]).is_err());
    }
    assert!(
        plan_generation(
            &manifest(),
            Some(&live(&[("Photo", u64::MAX)])),
            &[],
            &[usage("Photo", 0, 1)]
        )
        .is_err()
    );
    assert!(plan_generation(&manifest(), Some(&live(&[("Photo", 0)])), &[], &[]).is_err());
    assert!(
        plan_generation(
            &manifest(),
            Some(&Generation {
                descriptor: descriptor(),
                state: GenerationState::Tombstone,
                references: BTreeMap::from([("Photo".into(), 1)])
            }),
            &[],
            &[]
        )
        .is_err()
    );
    assert!(
        plan_generation(
            &manifest(),
            None,
            &[publication("Photo"), publication("Photo")],
            &[]
        )
        .is_err()
    );
    let mut wrong = publication("Photo");
    wrong.descriptor.bytes = 18;
    assert!(plan_generation(&manifest(), Some(&old), &[wrong], &[]).is_err());
    let mut empty = publication("Photo");
    empty.descriptor.bytes = 0;
    assert!(plan_generation(&manifest(), None, &[empty], &[]).is_err());
    let mut too_wide = publication("Avatar");
    too_wide.descriptor.reference.width = 65;
    assert!(plan_generation(&manifest(), None, &[too_wide], &[]).is_err());
    let mut invalid = publication("Photo");
    invalid.descriptor.reference.generation = "../outside".into();
    assert!(plan_generation(&manifest(), None, &[invalid], &[]).is_err());
}

#[test]
fn a_fresh_other_declaration_can_replace_the_last_old_reference_in_one_final_state() {
    let old = live(&[("Photo", 1)]);
    let plan = plan_generation(
        &manifest(),
        Some(&old),
        &[publication("Avatar")],
        &[usage("Photo", 1, 0), usage("Avatar", 0, 1)],
    )
    .unwrap();
    assert_eq!(plan.generation, live(&[("Avatar", 1)]));
    assert!(!plan.collect && !plan.keep_new);
    assert_eq!(
        plan.used_publications.into_iter().collect::<Vec<_>>(),
        ["Avatar"]
    );
}
#[test]
fn shared_generation_proposals_cannot_cross_sources_or_change_descriptor_bytes() {
    let declarations = manifest().declarations().cloned().collect::<Vec<_>>();
    let mut value = serde_json::to_value(&declarations).unwrap();
    value[0]["source"] = json!("Different");
    let other = Manifest::from_json(&value.to_string()).unwrap();
    assert!(
        plan_generation(
            &other,
            Some(&live(&[("Photo", 1)])),
            &[publication("Avatar")],
            &[usage("Avatar", 0, 1)]
        )
        .is_err()
    );
    let mut second = publication("Avatar");
    second.descriptor.bytes = 18;
    assert!(
        plan_generation(
            &manifest(),
            None,
            &[publication("Photo"), second],
            &[usage("Photo", 0, 1)]
        )
        .is_err()
    );
}

fn publish_one(registry: &mut Registry, name: &str, descriptor: Descriptor) -> Reference {
    let mut draft = registry.begin();
    let reference = registry
        .propose(&manifest(), &mut draft, name, descriptor)
        .unwrap();
    let use_ref = ReferenceUse {
        declaration: name.into(),
        reference: reference.clone(),
        before: 0,
        after: 1,
    };
    let patch = registry.prepare(&manifest(), draft, &[use_ref]).unwrap();
    registry.apply(patch).unwrap();
    reference
}
#[test]
fn the_index_shares_candidates_in_one_command_and_publishes_only_at_apply() {
    let mut registry = Registry::default();
    let mut draft = registry.begin();
    let photo = registry
        .propose(&manifest(), &mut draft, "Photo", descriptor())
        .unwrap();
    let mut other = descriptor();
    other.reference.generation = "c".repeat(32);
    let avatar = registry
        .propose(&manifest(), &mut draft, "Avatar", other)
        .unwrap();
    assert_eq!(photo, avatar);
    assert!(
        registry
            .current_reference("Photo", &photo.key, "png")
            .is_none()
    );
    let patch = registry
        .prepare(
            &manifest(),
            draft,
            &[usage("Photo", 0, 1), usage("Avatar", 0, 1)],
        )
        .unwrap();
    assert_eq!(patch.plans().len(), 1);
    assert!(patch.plans()[0].keep_new);
    assert!(registry.generation(&photo.generation).is_none());
    registry.apply(patch).unwrap();
    assert_eq!(
        registry.current_reference("Photo", &photo.key, "png"),
        Some(&photo)
    );
    assert_eq!(
        registry.current_reference("Avatar", &photo.key, "png"),
        Some(&avatar)
    );
}
#[test]
fn committed_liveness_is_declaration_specific_and_ignored_proposals_never_enroll() {
    let mut registry = Registry::default();
    let photo = publish_one(&mut registry, "Photo", descriptor());
    assert!(
        registry
            .current_reference("Avatar", &photo.key, "png")
            .is_none()
    );
    assert!(
        registry
            .current_reference("Photo", &photo.key, "jpeg")
            .is_none()
    );
    let mut draft = registry.begin();
    let mut incoming = descriptor();
    incoming.reference.generation = "c".repeat(32);
    assert_eq!(
        registry
            .propose(&manifest(), &mut draft, "Avatar", incoming)
            .unwrap(),
        photo
    );
    let patch = registry.prepare(&manifest(), draft, &[]).unwrap();
    assert!(patch.plans()[0].used_publications.is_empty());
    registry.apply(patch).unwrap();
    assert!(
        registry
            .current_reference("Avatar", &photo.key, "png")
            .is_none()
    );
    assert_eq!(
        registry.current_reference("Photo", &photo.key, "png"),
        Some(&photo)
    );
}
#[test]
fn captured_collection_completion_cannot_clear_a_new_same_hash_generation() {
    let mut registry = Registry::default();
    let old = publish_one(&mut registry, "Photo", descriptor());
    let patch = registry
        .prepare(&manifest(), registry.begin(), &[usage("Photo", 1, 0)])
        .unwrap();
    registry.apply(patch).unwrap();
    let pending = registry.pending_collections().next();
    assert!(
        pending.is_some(),
        "last removal schedules captured generation cleanup"
    );
    let captured = pending.unwrap().clone();
    assert_eq!(captured.reference, old);
    assert!(
        registry
            .current_reference("Photo", &old.key, "png")
            .is_none()
    );
    let mut fresh = descriptor();
    fresh.reference.generation = "c".repeat(32);
    let new = publish_one(&mut registry, "Photo", fresh);
    assert_ne!(new.generation, old.generation);
    registry.collected(&captured.reference.generation).unwrap();
    assert_eq!(
        registry.current_reference("Photo", &old.key, "png"),
        Some(&new)
    );
    assert!(registry.pending_collections().next().is_none());
    assert!(
        registry.generation(&old.generation).is_some(),
        "retired identity retained"
    );
    assert!(registry.collected(&new.generation).is_err());
}
#[test]
fn generation_ids_are_not_reused_after_ignored_publication_or_collection() {
    let mut registry = Registry::default();
    let mut draft = registry.begin();
    registry
        .propose(&manifest(), &mut draft, "Photo", descriptor())
        .unwrap();
    let patch = registry.prepare(&manifest(), draft, &[]).unwrap();
    registry.apply(patch).unwrap();
    assert!(
        registry
            .propose(&manifest(), &mut registry.begin(), "Photo", descriptor())
            .is_err()
    );
    let mut fresh = descriptor();
    fresh.reference.generation = "c".repeat(32);
    let live = publish_one(&mut registry, "Photo", fresh);
    let mut delete = usage("Photo", 1, 0);
    delete.reference = live.clone();
    let patch = registry
        .prepare(&manifest(), registry.begin(), &[delete])
        .unwrap();
    registry.apply(patch).unwrap();
    registry.collected(&live.generation).unwrap();
    let mut old_candidate = descriptor();
    old_candidate.reference = live;
    assert!(
        registry
            .propose(&manifest(), &mut registry.begin(), "Photo", old_candidate)
            .is_err()
    );
}
#[test]
fn stale_or_refused_patches_leave_all_indexes_unchanged() {
    let mut registry = Registry::default();
    let old = publish_one(&mut registry, "Photo", descriptor());
    let stale = registry
        .prepare(&manifest(), registry.begin(), &[usage("Photo", 0, 1)])
        .unwrap();
    let remove = registry
        .prepare(&manifest(), registry.begin(), &[usage("Photo", 1, 0)])
        .unwrap();
    registry.apply(remove).unwrap();
    assert!(registry.apply(stale).is_err());
    assert!(
        registry
            .current_reference("Photo", &old.key, "png")
            .is_none()
    );
    assert_eq!(registry.pending_collections().count(), 1);
    let mut malformed = usage("Photo", 0, 1);
    malformed.reference.width = 7;
    assert!(
        registry
            .prepare(&manifest(), registry.begin(), &[malformed])
            .is_err()
    );
    assert_eq!(registry.pending_collections().count(), 1);
}
#[test]
fn candidate_identity_and_canonical_descriptors_refuse_collisions_before_mutation() {
    let registry = Registry::default();
    let mut draft = registry.begin();
    registry
        .propose(&manifest(), &mut draft, "Photo", descriptor())
        .unwrap();
    let mut other_hash = descriptor();
    other_hash.reference.key = "d".repeat(64);
    assert!(
        registry
            .propose(&manifest(), &mut draft, "Avatar", other_hash)
            .is_err()
    );
    let mut wrong = descriptor();
    wrong.bytes = 18;
    assert!(
        registry
            .propose(&manifest(), &mut draft, "Avatar", wrong)
            .is_err()
    );
    assert!(
        registry
            .propose(&manifest(), &mut draft, "Photo", descriptor())
            .is_err()
    );
    assert!(
        registry
            .current_reference("Photo", &descriptor().reference.key, "png")
            .is_none()
    );
}
