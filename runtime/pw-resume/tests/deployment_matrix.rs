//! E7V item 6 — the deployment matrix.
//!
//! Every row the architect specified, plus the accepted neighbours that stop
//! the checker collapsing into "any build difference means reload".
//!
//! For each **rejected** row, four things are asserted, because "it returned
//! Refuse" is not the same as "nothing bad happened":
//!
//! ```text
//! no incompatible capture is decoded
//! no handler attaches
//! no private value crosses into a broader scope
//! the declared recovery action occurs
//! ```
//!
//! The first is structural: `decide` receives `captures` as opaque bytes and
//! there is no decoder in this crate, so decoding cannot happen before the
//! decision authorises it. `a_refusal_never_reaches_the_captures` pins that.

use pw_resume::*;

// --- fixtures ----------------------------------------------------------------

const ABI: PlatformAbi = PlatformAbi(1);

fn schema_v1() -> SchemaHash {
    SchemaHash::of_fields(&[("store_id", "StoreId")])
}

fn schema_v2() -> SchemaHash {
    SchemaHash::of_fields(&[("store_id", "StoreId"), ("locale", "String")])
}

/// A digest as the compiler would produce it. The runtime never hashes source.
fn impl_hash(body: &str) -> ImplementationHash {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ImplementationHash::new(CURRENT_SCHEME, &format!("{h:016x}"))
}

fn deps(names: &[&str]) -> DependencySet {
    let refs: Vec<[&str; 5]> = names
        .iter()
        .map(|n| ["app", "store", "Term", n, "r1"])
        .collect();
    DependencySet::of(&refs)
}

fn handler(implementation: &str, schema: &SchemaHash) -> HandlerId {
    HandlerId::derive(
        &impl_hash(implementation),
        &deps(&["Stores.get"]),
        schema,
        &ABI,
    )
}

fn entry(h: &HandlerId, schema: &SchemaHash, build: &str, scope: PrivacyScope) -> ResumeEntry {
    ResumeEntry {
        hash_scheme: CURRENT_SCHEME,
        platform_abi: ABI,
        application_build: BuildId(build.into()),
        handler: h.clone(),
        capture_schema: schema.clone(),
        document_schema: SchemaHash::of_fields(&[("root", "section")]),
        privacy_scope: scope,
        captures: b"store-7".to_vec(),
    }
}

fn runtime(handlers: &[(&HandlerId, &SchemaHash)], scope: PrivacyScope) -> Runtime {
    Runtime {
        abi: vec![ABI],
        build: Some(BuildId("B2".into())),
        handlers: handlers
            .iter()
            .map(|(h, s)| ((*h).clone(), (*s).clone()))
            .collect(),
        document_schema: Some(SchemaHash::of_fields(&[("root", "section")])),
        scope: Some(scope),
        migrations: Vec::new(),
    }
}

fn add_locale(bytes: &[u8]) -> Result<Vec<u8>, MigrationError> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(b"|en");
    Ok(out)
}

/// Assert a refusal did everything a refusal must do.
fn assert_refused(d: &Decision, expected: Refusal, recovery: Recovery) {
    match d {
        Decision::Refuse {
            why,
            recovery: got,
            trace,
        } => {
            assert_eq!(*why, expected, "refused for the wrong reason");
            assert_eq!(*got, recovery, "wrong recovery action");
            // In words where the decision keeps them: the `trace` feature,
            // which the browser's build leaves out (ADR-0400).
            assert!(
                !cfg!(feature = "trace") || !trace.is_empty(),
                "a refusal must carry a causal trace"
            );
            assert!(!d.attaches(), "a refusal must not attach");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// --- the matrix --------------------------------------------------------------

#[test]
fn current_document_current_code_resumes() {
    let s = schema_v1();
    let h = handler("add_to_cart_v1", &s);
    let d = decide(
        &entry(&h, &s, "B2", PrivacyScope::Public),
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_eq!(d, Decision::Resume);
}

#[test]
fn an_old_document_resumes_when_its_exact_artifact_is_still_served() {
    // Content addressing is the point: the build moved on, and this handler's
    // identity did not, so the code that reads these captures is still here.
    let s = schema_v1();
    let h = handler("add_to_cart_v1", &s);
    let d = decide(
        &entry(&h, &s, "B1", PrivacyScope::Public),
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_eq!(
        d,
        Decision::Resume,
        "an old build id alone is not a mismatch"
    );
}

#[test]
fn an_old_document_with_a_changed_handler_is_refused_and_recovers() {
    let s = schema_v1();
    let old = handler("add_to_cart_v1", &s);
    let new = handler("add_to_cart_v2_different_behaviour", &s);
    let d = decide(
        &entry(&old, &s, "B1", PrivacyScope::Public),
        &runtime(&[(&new, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(&d, Refusal::UnknownHandler(old), Recovery::RefetchRegion);
}

#[test]
fn an_unchanged_handler_in_a_new_build_resumes() {
    // The neighbour that stops "any deployment means reload". The build
    // changed; this handler did not.
    let s = schema_v1();
    let h = handler("add_to_cart_v1", &s);
    let unrelated = handler("something_else_entirely", &s);
    let mut rt = runtime(&[(&h, &s), (&unrelated, &s)], PrivacyScope::Public);
    rt.build = Some(BuildId("B3".into()));
    assert_eq!(
        decide(
            &entry(&h, &s, "B1", PrivacyScope::Public),
            &rt,
            Construct::PublicRegion
        ),
        Decision::Resume
    );
}

#[test]
fn a_stale_cached_handler_is_refused_and_refetches() {
    // The mirror of the row above: the document is current and the CODE is old.
    let s = schema_v1();
    let current = handler("add_to_cart_v2", &s);
    let stale = handler("add_to_cart_v1", &s);
    let d = decide(
        &entry(&current, &s, "B2", PrivacyScope::Public),
        &runtime(&[(&stale, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::UnknownHandler(current),
        Recovery::RefetchRegion,
    );
}

#[test]
fn an_old_capture_schema_with_an_explicit_migration_migrates() {
    let (v1, v2) = (schema_v1(), schema_v2());
    // Same implementation text, different capture schema -> different identity.
    let old = handler("render", &v1);
    let new = handler("render", &v2);
    let mut rt = runtime(&[(&old, &v2)], PrivacyScope::Public);
    rt.migrations.push(Migration {
        from: v1.clone(),
        to: v2.clone(),
        apply: add_locale,
    });
    assert_ne!(old, new, "the capture schema is part of handler identity");

    match decide(
        &entry(&old, &v1, "B1", PrivacyScope::Public),
        &rt,
        Construct::PublicRegion,
    ) {
        Decision::Migrate { produced } => assert_eq!(produced, b"store-7|en"),
        other => panic!("expected a migration, got {other:?}"),
    }
}

#[test]
fn an_old_capture_schema_without_a_migration_is_refused() {
    let (v1, v2) = (schema_v1(), schema_v2());
    let h = handler("render", &v1);
    let d = decide(
        &entry(&h, &v1, "B1", PrivacyScope::Public),
        &runtime(&[(&h, &v2)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::NoMigration { from: v1, to: v2 },
        Recovery::RefetchRegion,
    );
}

#[test]
fn the_same_handler_name_with_a_changed_implementation_is_refused() {
    // The case a path-based identity cannot see. Both are "checkout/save"; the
    // bodies differ, so the identities differ.
    let s = schema_v1();
    let before = handler("save(order) { charge(order) }", &s);
    let after = handler("save(order) { charge(order); notify(order) }", &s);
    assert_ne!(before, after, "identity must follow the implementation");
    let d = decide(
        &entry(&before, &s, "B1", PrivacyScope::Public),
        &runtime(&[(&after, &s)], PrivacyScope::Public),
        Construct::PendingCommand,
    );
    // A pending mutation is never replayed automatically.
    assert_refused(
        &d,
        Refusal::UnknownHandler(before),
        Recovery::RetryInteraction,
    );
}

#[test]
fn the_same_implementation_with_a_changed_capture_schema_is_refused() {
    let (v1, v2) = (schema_v1(), schema_v2());
    let before = handler("render", &v1);
    let after = handler("render", &v2);
    assert_ne!(before, after);
    let d = decide(
        &entry(&before, &v1, "B1", PrivacyScope::Public),
        &runtime(&[(&after, &v2)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(&d, Refusal::UnknownHandler(before), Recovery::RefetchRegion);
}

#[test]
fn weakening_a_private_scope_to_public_is_refused() {
    let s = schema_v1();
    let h = handler("cart", &s);
    let d = decide(
        &entry(&h, &s, "B1", PrivacyScope::Session("s-1".into())),
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PrivateSlot,
    );
    assert_refused(
        &d,
        Refusal::PrivacyWidened {
            from: PrivacyScope::Session("s-1".into()),
            into: PrivacyScope::Public,
        },
        Recovery::RerenderPrivateSlot,
    );
}

#[test]
fn a_different_session_is_refused_rather_than_treated_as_stricter() {
    // Two sessions are different principals, not a hierarchy. An ordering over
    // scopes would make one "admit" the other, which is the cross-tenant leak.
    let s = schema_v1();
    let h = handler("cart", &s);
    let d = decide(
        &entry(&h, &s, "B1", PrivacyScope::Session("s-1".into())),
        &runtime(&[(&h, &s)], PrivacyScope::Session("s-2".into())),
        Construct::PrivateSlot,
    );
    assert_refused(
        &d,
        Refusal::PrivacyWidened {
            from: PrivacyScope::Session("s-1".into()),
            into: PrivacyScope::Session("s-2".into()),
        },
        Recovery::RerenderPrivateSlot,
    );
}

#[test]
fn a_patch_from_another_build_is_refused() {
    let d = patch_applies(&BuildId("A".into()), &BuildId("B".into()));
    assert_refused(
        &d,
        Refusal::MixedBuild {
            document: BuildId("A".into()),
            patch: BuildId("B".into()),
        },
        Recovery::ReloadDocument,
    );
}

#[test]
fn a_patch_from_the_same_build_applies() {
    assert_eq!(
        patch_applies(&BuildId("A".into()), &BuildId("A".into())),
        Decision::Resume
    );
}

#[test]
fn a_truncated_manifest_is_refused_without_panicking() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Public);
    e.captures.clear();
    let d = decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::MalformedManifest("captures are empty under a non-empty schema"),
        Recovery::RefetchRegion,
    );
}

#[test]
fn an_unknown_platform_abi_is_refused() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Public);
    e.platform_abi = PlatformAbi(99);
    let d = decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::UnsupportedAbi(PlatformAbi(99)),
        Recovery::RefetchRegion,
    );
}

#[test]
fn a_changed_document_part_is_refused() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut rt = runtime(&[(&h, &s)], PrivacyScope::Public);
    rt.document_schema = Some(SchemaHash::of_fields(&[("root", "article")]));
    let d = decide(
        &entry(&h, &s, "B1", PrivacyScope::Public),
        &rt,
        Construct::PublicRegion,
    );
    assert_refused(&d, Refusal::DocumentSchemaMismatch, Recovery::RefetchRegion);
}

// --- accepted neighbours -----------------------------------------------------

#[test]
fn an_unrelated_code_change_does_not_prevent_resumption() {
    // The neighbour that matters most. A deployment that changes some OTHER
    // handler must not invalidate this one, or every deploy is a full reload
    // and the feature is worthless.
    let s = schema_v1();
    let mine = handler("cart_badge", &s);
    let theirs_before = handler("footer_v1", &s);
    let theirs_after = handler("footer_v2", &s);
    assert_ne!(theirs_before, theirs_after);

    assert_eq!(
        decide(
            &entry(&mine, &s, "B1", PrivacyScope::Public),
            &runtime(&[(&mine, &s), (&theirs_after, &s)], PrivacyScope::Public),
            Construct::PublicRegion
        ),
        Decision::Resume
    );
}

#[test]
fn reference_order_is_not_a_behavioural_difference() {
    // Two builds that resolved the same references in a different order must
    // produce the same identity, or an unrelated reordering rejects every
    // resume in the application.
    assert_eq!(
        deps(&["Stores.get", "Carts.add"]),
        deps(&["Carts.add", "Stores.get"])
    );
    // And a repeated reference is the same SET. How many times it is used is a
    // property of the implementation, which is hashed separately and in order.
    assert_eq!(deps(&["Stores.get", "Stores.get"]), deps(&["Stores.get"]));
}

#[test]
fn statement_order_is_a_behavioural_difference() {
    // The other half, and the reason the two hashes are separate types.
    // `charge(); send_receipt()` is not `send_receipt(); charge()`.
    assert_ne!(
        impl_hash("charge(); send_receipt()"),
        impl_hash("send_receipt(); charge()")
    );
}

#[test]
fn a_dependency_set_difference_changes_identity() {
    let s = schema_v1();
    let body = impl_hash("same body");
    assert_ne!(
        HandlerId::derive(&body, &deps(&["Stores.get"]), &s, &ABI),
        HandlerId::derive(&body, &deps(&["Stores.get", "Carts.add"]), &s, &ABI),
        "a handler that reaches one more declaration is a different handler"
    );
}

#[test]
fn the_same_handler_resumes_into_two_different_documents() {
    // One handler, two document sizes. The document SCHEMA is what must match,
    // not the document's content.
    let s = schema_v1();
    let h = handler("row", &s);
    for build in ["B1", "B2"] {
        assert_eq!(
            decide(
                &entry(&h, &s, build, PrivacyScope::Public),
                &runtime(&[(&h, &s)], PrivacyScope::Public),
                Construct::PublicRegion
            ),
            Decision::Resume
        );
    }
}

#[test]
fn private_to_private_under_the_same_session_resumes() {
    let s = schema_v1();
    let h = handler("cart", &s);
    assert_eq!(
        decide(
            &entry(&h, &s, "B1", PrivacyScope::Session("s-1".into())),
            &runtime(&[(&h, &s)], PrivacyScope::Session("s-1".into())),
            Construct::PrivateSlot
        ),
        Decision::Resume
    );
}

#[test]
fn a_public_manifest_resumes_into_a_narrower_scope() {
    // Public state restricts nothing, so it is admissible anywhere. This is the
    // one widening-adjacent case that IS allowed, and it is allowed in the
    // direction that adds restrictions rather than removing them.
    let s = schema_v1();
    let h = handler("banner", &s);
    assert_eq!(
        decide(
            &entry(&h, &s, "B1", PrivacyScope::Public),
            &runtime(&[(&h, &s)], PrivacyScope::Session("s-1".into())),
            Construct::PrivateSlot
        ),
        Decision::Resume
    );
}

#[test]
fn an_added_optional_field_resumes_through_its_migration() {
    let (v1, v2) = (schema_v1(), schema_v2());
    let h = handler("render", &v1);
    let mut rt = runtime(&[(&h, &v2)], PrivacyScope::Public);
    rt.migrations.push(Migration {
        from: v1.clone(),
        to: v2,
        apply: add_locale,
    });
    assert!(
        decide(
            &entry(&h, &v1, "B1", PrivacyScope::Public),
            &rt,
            Construct::PublicRegion
        )
        .attaches()
    );
}

// --- properties --------------------------------------------------------------

#[test]
fn a_migration_is_tied_to_both_schemas_and_not_reused() {
    // A migration declared v1 -> v2 must not be applied to anything else, or
    // "explicit migration" degrades into structural inference.
    let (v1, v2) = (schema_v1(), schema_v2());
    let other = SchemaHash::of_fields(&[("unrelated", "Int")]);
    let h = handler("render", &other);
    let mut rt = runtime(&[(&h, &v2)], PrivacyScope::Public);
    rt.migrations.push(Migration {
        from: v1,
        to: v2.clone(),
        apply: add_locale,
    });
    let d = decide(
        &entry(&h, &other, "B1", PrivacyScope::Public),
        &rt,
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::NoMigration {
            from: other,
            to: v2,
        },
        Recovery::RefetchRegion,
    );
}

#[test]
fn a_failing_migration_refuses_rather_than_attaching_what_it_produced() {
    fn always_fails(_: &[u8]) -> Result<Vec<u8>, MigrationError> {
        Err(MigrationError("locale is not inferable".into()))
    }
    let (v1, v2) = (schema_v1(), schema_v2());
    let h = handler("render", &v1);
    let mut rt = runtime(&[(&h, &v2)], PrivacyScope::Public);
    rt.migrations.push(Migration {
        from: v1,
        to: v2,
        apply: always_fails,
    });
    let d = decide(
        &entry(&h, &schema_v1(), "B1", PrivacyScope::Public),
        &rt,
        Construct::PublicRegion,
    );
    assert!(!d.attaches());
}

#[test]
fn a_refusal_never_reaches_the_captures() {
    // Structural, and the reason `captures` is `Vec<u8>` here: this crate has
    // no decoder, so decoding cannot precede the decision that authorises it.
    // A privacy failure in particular must be refused BEFORE any interpretation
    // — which is why the scope check runs before the schema check.
    let s = schema_v1();
    let h = handler("cart", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Session("s-1".into()));
    e.captures = b"secret-token".to_vec();
    let d = decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PrivateSlot,
    );
    match d {
        Decision::Refuse { trace, .. } => assert!(
            !trace.iter().any(|t| t.contains("secret-token")),
            "the trace must not carry the captures it refused to decode"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn an_open_resource_is_never_recoverable() {
    assert_eq!(
        Construct::OpenResource.recoveries(),
        &[Recovery::RejectIrrecoverable]
    );
}

#[test]
fn a_pending_command_is_never_replayed_automatically() {
    // The one that would be a double charge. No recovery for a pending
    // mutation may re-run it without the user acting again.
    for r in Construct::PendingCommand.recoveries() {
        assert!(
            matches!(
                r,
                Recovery::RetryInteraction | Recovery::RequireUserConfirmation
            ),
            "{r:?} would replay a mutation the user did not re-request"
        );
    }
}

/// R3, as a matrix row now that the fuzzer found it.
///
/// The construct and the scope DISAGREE — a public region carrying session
/// state. No hand-written scenario pairs them, because the pairing is a
/// contradiction; the generator produced it by taking the cross product.
#[test]
fn a_private_manifest_in_a_public_region_never_gets_a_public_refetch() {
    let s = schema_v1();
    let present = handler("render", &s);
    let absent = handler("gone", &s);
    let d = decide(
        &entry(&absent, &s, "B1", PrivacyScope::Session("s-1".into())),
        &runtime(&[(&present, &s)], PrivacyScope::Session("s-1".into())),
        Construct::PublicRegion,
    );
    match d {
        Decision::Refuse { recovery, .. } => assert_eq!(
            recovery,
            Recovery::RerenderPrivateSlot,
            "private state must not be recovered by re-rendering a public region"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// And the control: public state in a public region still refetches.
#[test]
fn a_public_manifest_in_a_public_region_still_refetches() {
    let s = schema_v1();
    let present = handler("render", &s);
    let absent = handler("gone", &s);
    let d = decide(
        &entry(&absent, &s, "B1", PrivacyScope::Public),
        &runtime(&[(&present, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    match d {
        Decision::Refuse { recovery, .. } => assert_eq!(recovery, Recovery::RefetchRegion),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// --- the attachment path -----------------------------------------------------

/// There is no way to attach a handler without a decision.
///
/// Enforced by the type system rather than by a lint: `Authorised` has a
/// private field, so the only constructor is `authorise`, and `attach` takes
/// one by value. A caller cannot reach `attach` without holding the result of
/// `decide`.
///
/// This is the same move as deleting the by-name member fallback instead of
/// documenting that it should not be used — a rule nothing can forget.
#[test]
fn a_refusal_yields_no_authorisation() {
    let s = schema_v1();
    let absent = handler("gone", &s);
    let present = handler("here", &s);
    let e = entry(&absent, &s, "B1", PrivacyScope::Public);
    let d = decide(
        &e,
        &runtime(&[(&present, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert!(authorise(&e, &d).is_none(), "a refusal must not authorise");
}

#[test]
fn a_resume_authorises_the_manifests_own_captures() {
    let s = schema_v1();
    let h = handler("render", &s);
    let e = entry(&h, &s, "B1", PrivacyScope::Public);
    let d = decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    let proof = authorise(&e, &d).expect("a resume authorises");
    let (attached, captures) = attach(proof);
    assert_eq!(attached, h);
    assert_eq!(captures, e.captures);
}

/// A migration authorises the MIGRATED bytes, never the originals.
///
/// Handing the manifest's own captures to a handler that expects the new
/// schema would be the mismatch the migration exists to prevent, arriving one
/// step later and in a place nothing checks.
#[test]
fn a_migration_authorises_what_it_produced_and_not_the_original() {
    let (v1, v2) = (schema_v1(), schema_v2());
    let h = handler("render", &v1);
    let mut rt = runtime(&[(&h, &v2)], PrivacyScope::Public);
    rt.migrations.push(Migration {
        from: v1.clone(),
        to: v2,
        apply: add_locale,
    });
    let e = entry(&h, &v1, "B1", PrivacyScope::Public);
    let d = decide(&e, &rt, Construct::PublicRegion);
    let proof = authorise(&e, &d).expect("a migration authorises");
    assert_eq!(proof.captures(), b"store-7|en");
    assert_ne!(
        proof.captures(),
        &e.captures[..],
        "the ORIGINAL bytes must not reach the handler"
    );
}

// --- hash-scheme versioning --------------------------------------------------

/// A manifest from an older scheme is incomparable, not different.
///
/// This is the row that matters once manifests outlive deployments. Two
/// digests of the same length produced by different algorithms carry no
/// relation to each other, and comparing them anyway is how a coincidence
/// becomes an attachment. It fails closed through the ordinary recovery path.
#[test]
fn a_manifest_from_an_older_hash_scheme_is_refused() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Public);
    e.hash_scheme = HashScheme(1);
    let d = decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    );
    assert_refused(
        &d,
        Refusal::UnsupportedHashScheme(HashScheme(1)),
        Recovery::RefetchRegion,
    );
}

/// And a future one, which a deployed old runtime will meet.
#[test]
fn a_manifest_from_a_newer_hash_scheme_is_refused() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Public);
    e.hash_scheme = HashScheme(99);
    assert!(
        !decide(
            &e,
            &runtime(&[(&h, &s)], PrivacyScope::Public),
            Construct::PublicRegion
        )
        .attaches()
    );
}

/// The scheme is checked BEFORE the ABI, because it governs whether any
/// identity below can be compared at all.
#[test]
fn the_hash_scheme_is_checked_before_anything_it_governs() {
    let s = schema_v1();
    let h = handler("render", &s);
    let mut e = entry(&h, &s, "B1", PrivacyScope::Public);
    e.hash_scheme = HashScheme(1);
    e.platform_abi = PlatformAbi(99); // also wrong
    match decide(
        &e,
        &runtime(&[(&h, &s)], PrivacyScope::Public),
        Construct::PublicRegion,
    ) {
        Decision::Refuse { why, .. } => assert_eq!(
            why,
            Refusal::UnsupportedHashScheme(HashScheme(1)),
            "the scheme must be reported first — an ABI comparison under an \
             unknown scheme is meaningless"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn hashes_from_different_schemes_are_not_comparable() {
    let a = ImplementationHash::new(HashScheme(1), "abc");
    let b = ImplementationHash::new(HashScheme(2), "abc");
    assert!(!a.comparable_with(&b), "same digest, different algorithm");
    assert_ne!(a, b);
}
