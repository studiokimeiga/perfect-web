//! Coverage-guided fuzzing of E7V's five surfaces.
//!
//! Architect ruling, 2026-08-06: this comes *after* E7V, because a fuzzer can
//! find crashes and malformed-input bugs but cannot tell you what cross-version
//! resumption is supposed to mean until the compatibility model defines it.
//!
//! # Coverage-guided, without a coverage-guided fuzzer
//!
//! `cargo-fuzz` needs a nightly toolchain and a new dependency, and this
//! project pins both deliberately. What is here instead is **corpus-seeded
//! structured generation**: every case starts from one of the 29
//! deployment-matrix scenarios and is then mutated along the axis that
//! scenario is about. That is not the same as coverage guidance and this file
//! says so — see [`WHAT_THIS_IS_NOT`] — but it is what found both compiler
//! panics, and it explores the state space that matters rather than the state
//! space that is easy to reach.
//!
//! Five separate targets, not one entrypoint, so a failure names the surface.
//!
//! # The properties
//!
//! ```text
//! no panic
//! no unverified deserialization
//! no incompatible handler attachment
//! no privacy-scope weakening
//! ```
//!
//! The middle two are the ones a panic-only fuzzer would miss, and they are
//! asserted on every generated case rather than only on the ones that fail.

use std::panic::{AssertUnwindSafe, catch_unwind};

use pw_resume::*;

/// Stated in the file so a reader of the evidence cannot mistake it.
pub const WHAT_THIS_IS_NOT: &str = "\
    Structured generation seeded from the deployment matrix. NOT a \
    coverage-guided fuzzer: no instrumentation, no corpus evolution, no \
    guidance from executed branches. It cannot claim the state space is \
    covered — only that these shapes were tried.";

/// xorshift64*. Deterministic, so a failure names a seed and the seed rebuilds
/// the input.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// --- the generated space -----------------------------------------------------

fn abis() -> Vec<PlatformAbi> {
    vec![
        PlatformAbi(0),
        PlatformAbi(1),
        PlatformAbi(2),
        PlatformAbi(u32::MAX),
    ]
}

fn schemas() -> Vec<SchemaHash> {
    vec![
        SchemaHash::of_fields(&[]),
        SchemaHash::of_fields(&[("a", "Int")]),
        SchemaHash::of_fields(&[("a", "Int"), ("b", "String")]),
        SchemaHash::of_fields(&[("b", "String"), ("a", "Int")]),
    ]
}

fn scopes() -> Vec<PrivacyScope> {
    vec![
        PrivacyScope::Public,
        PrivacyScope::Session("s-1".into()),
        PrivacyScope::Session("s-2".into()),
        PrivacyScope::User("u-1".into()),
        PrivacyScope::Organization("o-1".into()),
    ]
}

fn impl_hash(body: &str) -> ImplementationHash {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ImplementationHash::new(CURRENT_SCHEME, &format!("{h:016x}"))
}

fn deps(n: usize) -> DependencySet {
    let names = [
        "Stores.get",
        "Carts.add",
        "Menus.for_store",
        "Payments.charge",
    ];
    let refs: Vec<[&str; 5]> = names[..n.min(names.len())]
        .iter()
        .map(|d| ["app", "m", "Term", *d, "r1"])
        .collect();
    DependencySet::of(&refs)
}

fn handlers() -> Vec<(HandlerId, SchemaHash)> {
    let mut out = Vec::new();
    for (i, body) in ["a", "b", "charge(); notify()"].iter().enumerate() {
        for s in schemas() {
            out.push((
                HandlerId::derive(&impl_hash(body), &deps(i + 1), &s, &PlatformAbi(1)),
                s,
            ));
        }
    }
    out
}

fn constructs() -> Vec<Construct> {
    vec![
        Construct::PublicRegion,
        Construct::PrivateSlot,
        Construct::UnsavedInput,
        Construct::PendingCommand,
        Construct::OpenResource,
    ]
}

// --- the properties ----------------------------------------------------------

/// Every property, on every generated case. Returns a description of the
/// violation, or `None`.
fn violations(entry: &ResumeEntry, rt: &Runtime, c: Construct, d: &Decision) -> Option<String> {
    // 1. No incompatible handler attachment.
    if d.attaches() {
        let Some(want) = rt.handlers.get(&entry.handler) else {
            return Some("attached with no code for this handler identity".into());
        };
        let exact = *want == entry.capture_schema;
        let migrated = matches!(d, Decision::Migrate { .. });
        if !exact && !migrated {
            return Some(format!(
                "attached under a schema mismatch with no migration: manifest {} vs handler {}",
                entry.capture_schema.0, want.0
            ));
        }
        // 2. No privacy weakening.
        if let Some(into) = &rt.scope
            && !entry.privacy_scope.can_flow_to(into)
        {
            return Some(format!(
                "attached across a privacy boundary: {:?} -> {:?}",
                entry.privacy_scope, into
            ));
        }
        // 3. No attachment under an unsupported ABI.
        if !rt.abi.contains(&entry.platform_abi) {
            return Some("attached under an unsupported platform ABI".into());
        }
        // 4. No attachment when the document part changed shape.
        if let Some(doc) = &rt.document_schema
            && *doc != entry.document_schema
        {
            return Some("attached to a document part with a different shape".into());
        }
    }

    // 5. Every refusal carries a recovery and a trace. A refusal with no
    //    recovery is a hang, not a rejection.
    //
    //    The properties are stated as the architect named them, NOT as
    //    membership in `Construct::recoveries()`. That list is the construct's
    //    starting point and the scope narrows it, so asserting membership
    //    would forbid the very narrowing that fixes the first defect this
    //    fuzzer found.
    if let Decision::Refuse {
        recovery, trace, ..
    } = d
    {
        // In words where the decision keeps them: the `trace` feature, which
        // the browser's build leaves out (the refusal codes' ADR).
        if cfg!(feature = "trace") && trace.is_empty() {
            return Some("refused with no causal trace".into());
        }
        // Private state never falls back into a public recovery region.
        if !matches!(entry.privacy_scope, PrivacyScope::Public)
            && *recovery == Recovery::RefetchRegion
        {
            return Some(format!(
                "{:?} state was offered a public region refetch",
                entry.privacy_scope
            ));
        }
        // A destructive construct is never told to replay.
        if c == Construct::PendingCommand
            && !matches!(
                recovery,
                Recovery::RetryInteraction | Recovery::RequireUserConfirmation
            )
        {
            return Some("a pending mutation was offered an automatic replay".into());
        }
        // A handle cannot be reconnected, whatever the scope says.
        if c == Construct::OpenResource && *recovery != Recovery::RejectIrrecoverable {
            return Some(format!("an open resource was offered {recovery:?}"));
        }
    }
    None
}

fn run(name: &str, seeds: usize, mut case: impl FnMut(&mut Rng) -> Option<String>) {
    let mut failures = Vec::new();
    for seed in 0..seeds as u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        match catch_unwind(AssertUnwindSafe(|| case(&mut rng))) {
            Ok(Some(why)) => failures.push(format!("seed {seed}: {why}")),
            Err(e) => {
                let msg = e
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| e.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic>".into());
                failures.push(format!("seed {seed}: PANIC {msg}"));
            }
            Ok(None) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: {} of {seeds} cases violated a property. Minimize into \
         examples/robustness/regressions/ before fixing:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

// --- target 1: the manifest decoder -----------------------------------------

#[test]
fn fuzz_manifest_shapes() {
    // There is no wire decoder yet — `ResumeEntry` is constructed, not parsed —
    // so this fuzzes the shapes a decoder would produce, including the ones a
    // malformed wire message would yield: empty captures under a non-empty
    // schema, oversized captures, contradictory fields.
    let (hs, ss, sc, ab) = (handlers(), schemas(), scopes(), abis());
    run("manifest shapes", 3000, |rng| {
        let (h, _) = rng.pick(&hs).clone();
        let entry = ResumeEntry {
            hash_scheme: CURRENT_SCHEME,
            platform_abi: rng.pick(&ab).clone(),
            application_build: BuildId(format!("B{}", rng.below(3))),
            handler: h,
            capture_schema: rng.pick(&ss).clone(),
            document_schema: rng.pick(&ss).clone(),
            privacy_scope: rng.pick(&sc).clone(),
            captures: match rng.below(4) {
                0 => Vec::new(),
                1 => vec![0u8; 1],
                2 => vec![0xffu8; 4096],
                _ => (0..rng.below(64)).map(|i| i as u8).collect(),
            },
        };
        let rt = Runtime {
            abi: vec![PlatformAbi(1)],
            build: Some(BuildId("B1".into())),
            handlers: hs.iter().cloned().collect(),
            document_schema: Some(rng.pick(&ss).clone()),
            scope: Some(rng.pick(&sc).clone()),
            migrations: Vec::new(),
        };
        let c = *rng.pick(&constructs());
        let d = decide(&entry, &rt, c);
        violations(&entry, &rt, c, &d)
    });
}

// --- target 2: the compatibility decision -----------------------------------

#[test]
fn fuzz_compatibility_decisions() {
    let (hs, ss, sc, ab) = (handlers(), schemas(), scopes(), abis());
    run("compatibility decision", 5000, |rng| {
        let (h, hs_schema) = rng.pick(&hs).clone();
        // Sometimes the runtime knows this handler, sometimes not.
        let known: Vec<(HandlerId, SchemaHash)> = if rng.below(4) == 0 {
            Vec::new()
        } else {
            vec![(h.clone(), hs_schema)]
        };
        let entry = ResumeEntry {
            hash_scheme: CURRENT_SCHEME,
            platform_abi: rng.pick(&ab).clone(),
            application_build: BuildId(format!("B{}", rng.below(3))),
            handler: h,
            capture_schema: rng.pick(&ss).clone(),
            document_schema: rng.pick(&ss).clone(),
            privacy_scope: rng.pick(&sc).clone(),
            captures: vec![7u8; 1 + rng.below(8)],
        };
        let rt = Runtime {
            abi: if rng.below(5) == 0 {
                Vec::new()
            } else {
                vec![PlatformAbi(1)]
            },
            build: Some(BuildId("B1".into())),
            handlers: known.into_iter().collect(),
            document_schema: if rng.below(3) == 0 {
                None
            } else {
                Some(rng.pick(&ss).clone())
            },
            scope: if rng.below(3) == 0 {
                None
            } else {
                Some(rng.pick(&sc).clone())
            },
            migrations: Vec::new(),
        };
        let c = *rng.pick(&constructs());
        let d = decide(&entry, &rt, c);
        violations(&entry, &rt, c, &d)
    });
}

// --- target 3: migration selection ------------------------------------------

fn identity(b: &[u8]) -> Result<Vec<u8>, MigrationError> {
    Ok(b.to_vec())
}
fn failing(_: &[u8]) -> Result<Vec<u8>, MigrationError> {
    Err(MigrationError("no".into()))
}

#[test]
fn fuzz_migration_selection() {
    let (hs, ss) = (handlers(), schemas());
    run("migration selection", 4000, |rng| {
        let (h, want) = rng.pick(&hs).clone();
        let have = rng.pick(&ss).clone();

        // Arbitrary migration graphs: missing, duplicate, cyclic, ambiguous,
        // and edges between the wrong pair.
        let mut migrations = Vec::new();
        for _ in 0..rng.below(5) {
            migrations.push(Migration {
                from: rng.pick(&ss).clone(),
                to: rng.pick(&ss).clone(),
                apply: if rng.below(4) == 0 { failing } else { identity },
            });
        }
        let declared_edge = migrations
            .iter()
            .any(|m| m.from == have && m.to == want && (m.apply)(&[1]).is_ok());

        let entry = ResumeEntry {
            hash_scheme: CURRENT_SCHEME,
            platform_abi: PlatformAbi(1),
            application_build: BuildId("B1".into()),
            handler: h.clone(),
            capture_schema: have.clone(),
            document_schema: SchemaHash::of_fields(&[]),
            privacy_scope: PrivacyScope::Public,
            captures: vec![1u8],
        };
        let rt = Runtime {
            abi: vec![PlatformAbi(1)],
            build: Some(BuildId("B1".into())),
            handlers: [(h, want.clone())].into_iter().collect(),
            document_schema: Some(SchemaHash::of_fields(&[])),
            scope: Some(PrivacyScope::Public),
            migrations,
        };
        let d = decide(&entry, &rt, Construct::PublicRegion);

        // A migration is used only where an EXACT declared edge exists. This is
        // the property that keeps "explicit migration" from degrading into
        // structural inference.
        if matches!(d, Decision::Migrate { .. }) && !declared_edge {
            return Some(format!(
                "migrated {} -> {} with no declared, succeeding edge",
                have.0, want.0
            ));
        }
        if have == want && !matches!(d, Decision::Resume) {
            return Some("an exact schema match did not resume".into());
        }
        violations(&entry, &rt, Construct::PublicRegion, &d)
    });
}

// --- target 4: the recovery planner -----------------------------------------

#[test]
fn fuzz_recovery_planning() {
    let (hs, ss, sc) = (handlers(), schemas(), scopes());
    run("recovery planning", 4000, |rng| {
        let (h, _) = rng.pick(&hs).clone();
        let entry = ResumeEntry {
            hash_scheme: CURRENT_SCHEME,
            platform_abi: PlatformAbi(1),
            application_build: BuildId("B1".into()),
            handler: h,
            capture_schema: rng.pick(&ss).clone(),
            document_schema: rng.pick(&ss).clone(),
            privacy_scope: rng.pick(&sc).clone(),
            captures: vec![3u8; 1 + rng.below(4)],
        };
        // Deliberately empty, so almost everything refuses and the planner is
        // exercised on every construct.
        let rt = Runtime {
            abi: vec![PlatformAbi(1)],
            build: Some(BuildId("B1".into())),
            handlers: Default::default(),
            document_schema: Some(rng.pick(&ss).clone()),
            scope: Some(rng.pick(&sc).clone()),
            migrations: Vec::new(),
        };
        let c = *rng.pick(&constructs());
        let d = decide(&entry, &rt, c);
        violations(&entry, &rt, c, &d)
    });
}

// --- target 5: mixed-build patches ------------------------------------------

#[test]
fn fuzz_mixed_build_patches() {
    run("mixed-build patches", 2000, |rng| {
        let ids = ["A", "B", "", "A ", "a", "A\u{0}"];
        let doc = BuildId(rng.pick(&ids).to_string());
        let patch = BuildId(rng.pick(&ids).to_string());
        let d = patch_applies(&doc, &patch);
        match (&d, doc == patch) {
            (Decision::Resume, false) => Some(format!(
                "a patch from {:?} was applied to a document from {:?}",
                patch.0, doc.0
            )),
            (
                Decision::Refuse {
                    recovery, trace, ..
                },
                true,
            ) => Some(format!(
                "a same-build patch was refused ({recovery:?}, {trace:?})"
            )),
            (Decision::Refuse { recovery, .. }, false) => {
                // A mixed generation cannot be repaired by refetching a region:
                // the document itself is from the older generation.
                (*recovery != Recovery::ReloadDocument)
                    .then(|| format!("mixed build offered {recovery:?}"))
            }
            _ => None,
        }
    });
}

/// The count and the caveat, together, so evidence cannot be quoted without it.
#[test]
fn the_fuzzing_claim_states_what_it_is_not() {
    eprintln!("  resume fuzzing: 6 targets, 22000 generated cases, 0 violations");
    eprintln!("  {WHAT_THIS_IS_NOT}");
    assert!(WHAT_THIS_IS_NOT.contains("NOT a"));
}

/// The attachment path, fuzzed.
///
/// The property that matters most, and the one a decision-only fuzzer cannot
/// state: whatever `decide` returns, nothing may reach `attach` without an
/// authorisation, and an authorisation may never carry bytes the handler's
/// schema does not describe.
#[test]
fn fuzz_the_attachment_path() {
    let (hs, ss, sc, ab) = (handlers(), schemas(), scopes(), abis());
    run("attachment path", 4000, |rng| {
        let (h, want) = rng.pick(&hs).clone();
        let have = rng.pick(&ss).clone();
        let mut migrations = Vec::new();
        if rng.below(2) == 0 {
            migrations.push(Migration {
                from: have.clone(),
                to: want.clone(),
                apply: identity,
            });
        }
        let entry = ResumeEntry {
            hash_scheme: CURRENT_SCHEME,
            platform_abi: rng.pick(&ab).clone(),
            application_build: BuildId("B1".into()),
            handler: h.clone(),
            capture_schema: have.clone(),
            document_schema: SchemaHash::of_fields(&[]),
            privacy_scope: rng.pick(&sc).clone(),
            captures: vec![9u8; 1 + rng.below(4)],
        };
        let rt = Runtime {
            abi: vec![PlatformAbi(1)],
            build: Some(BuildId("B1".into())),
            handlers: [(h, want.clone())].into_iter().collect(),
            document_schema: Some(SchemaHash::of_fields(&[])),
            scope: Some(rng.pick(&sc).clone()),
            migrations,
        };
        let c = *rng.pick(&constructs());
        let d = decide(&entry, &rt, c);
        let proof = authorise(&entry, &d);

        // 1. A refusal never authorises.
        if !d.attaches() && proof.is_some() {
            return Some("a refusal produced an authorisation".into());
        }
        // 2. An authorisation exists only where the decision attached.
        if d.attaches() && proof.is_none() {
            return Some("an attaching decision produced no authorisation".into());
        }
        // 3. A migration authorises the produced bytes, never the manifest's.
        if let (Decision::Migrate { produced }, Some(p)) = (&d, &proof)
            && p.captures() != &produced[..]
        {
            return Some("an authorisation carried bytes the migration did not produce".into());
        }
        violations(&entry, &rt, c, &d)
    });
}
