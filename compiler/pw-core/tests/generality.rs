//! `examples/generality/**` — the second score.
//!
//! Architect ruling, 2026-08-06, after 44/44:
//!
//! > Corpus conformance ≠ analysis generality ≠ language soundness ≠
//! > production readiness.
//! >
//! > Before 44/44, progress meant catching more fixtures. After 44/44,
//! > progress means making the same guarantees survive programs the fixtures
//! > did not anticipate.
//!
//! So each invariant gets a directory named for its **symbol**, holding
//! programs the corpus fixture did not anticipate:
//!
//! - `caught.pw` — structurally different from the fixture, same invariant,
//!   must be reported. This is what promotes an invariant to
//!   `GENERALLY_ENFORCED`.
//! - `slips-through.pw` — a program that violates the invariant and is **not**
//!   caught today. It must compile clean, and the moment it stops, the gap has
//!   closed and the file is promoted rather than deleted.
//!
//! The second file is the unusual one and it is the point: a known gap that is
//! only written down drifts out of date silently, and a known gap that is
//! executable cannot. `docs/evidence/P0/readiness.txt` lists these in prose;
//! this is the same list, run.

use std::collections::{BTreeMap, BTreeSet};

fn library() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in [
        "examples/lib",
        "packages/pw-std",
        "packages/pw-platform-web",
    ] {
        for e in std::fs::read_dir(root.join(dir)).unwrap_or_else(|e| panic!("{dir}: {e}")) {
            let p = e.expect("entry").path();
            if p.extension().is_none_or(|x| x != "pw") {
                continue;
            }
            out.push((
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    // The accepted modules too, as `rejected_program()` does: a witness may
    // import a query declared in the accepted corpus, and the label that makes
    // it a violation lives in that declaration.
    for e in std::fs::read_dir(root.join("examples/accepted")).expect("accepted") {
        let p = e.expect("entry").path();
        if p.extension().is_some_and(|x| x == "pw") {
            out.push((
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain.pw"),
    ));
    out.sort();
    out
}

struct Witness {
    invariant: String,
    status: String,
    name: String,
    src: String,
}

fn witnesses() -> Vec<Witness> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/generality");
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(&root) else {
        return out;
    };
    for d in dirs {
        let dir = d.expect("entry").path();
        if !dir.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&dir).expect("invariant dir") {
            let p = e.expect("entry").path();
            if p.extension().is_none_or(|x| x != "pw") {
                continue;
            }
            let src = std::fs::read_to_string(&p).expect("read");
            let field = |k: &str| {
                src.lines()
                    .find_map(|l| l.trim().strip_prefix(k))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|| panic!("{p:?} has no `{k}`"))
            };
            out.push(Witness {
                invariant: field("// @invariant:"),
                status: field("// @status:"),
                name: format!(
                    "{}/{}",
                    dir.file_name().unwrap().to_string_lossy(),
                    p.file_name().unwrap().to_string_lossy()
                ),
                src,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The symbols a witness file reports, checked as its own program.
fn symbols_for(w: &Witness) -> BTreeSet<&'static str> {
    let mut files = library();
    let leaf = w.name.rsplit('/').next().unwrap().to_string();
    files.push((leaf.clone(), w.src.clone()));
    // The schema it is held to as released, where it names one (track
    // `tables`): `// @schema-lock: <file beside it>`.
    let released = w
        .src
        .lines()
        .find_map(|l| l.trim().strip_prefix("// @schema-lock:"))
        .map(|file| {
            let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/generality")
                .join(w.name.rsplit_once('/').map_or("", |(dir, _)| dir))
                .join(file.trim());
            let text = std::fs::read_to_string(&lock)
                .unwrap_or_else(|e| panic!("{}: {e}", lock.display()));
            pw_core::tables::Schema::read(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", lock.display()))
        });
    let diags = pw_core::check::check_sources_with(&files, released.as_ref())
        .into_iter()
        .find(|(n, _)| *n == leaf)
        .map(|(_, d)| d)
        .unwrap_or_default();

    // The declaration rules run inside `check_sources` since ADR-0090.
    let out: BTreeSet<&'static str> = diags.iter().map(|d| d.symbol()).collect();
    out
}

#[test]
fn every_witness_names_a_registered_invariant() {
    let symbols: BTreeSet<&str> = pw_core::codes::ALL.iter().map(|c| c.symbol).collect();
    for w in witnesses() {
        assert!(
            symbols.contains(w.invariant.as_str()),
            "{}: `{}` is not a registered invariant symbol",
            w.name,
            w.invariant
        );
        assert!(
            matches!(w.status.as_str(), "GENERAL" | "NARROW" | "NEIGHBOUR"),
            "{}: unknown @status {:?} — expected GENERAL (a violation that must \
             be caught), NARROW (a violation that is not caught yet), or \
             NEIGHBOUR (a VALID program that must stay clean)",
            w.name,
            w.status
        );
    }
}

/// What a witness must clear before its result means anything.
///
/// Architect ruling, 2026-08-06:
///
/// > A `slips-through.pw` witness must pass parsing, resolution, typing, and
/// > every unrelated prerequisite before its silence is interpreted as an
/// > analysis gap. If any prerequisite fails, the witness is INVALID EVIDENCE,
/// > not a compiler miss.
///
/// Reported as a pipeline rather than a boolean, because "this file is
/// invalid" and "this file is valid and the analysis missed it" are opposite
/// conclusions and the difference cost a day.
#[derive(Debug, Default)]
struct Validity {
    parsed: bool,
    resolved: bool,
    arities_valid: bool,
    no_unrelated: bool,
    target_present: bool,
    failures: Vec<String>,
}

impl Validity {
    fn is_valid_evidence(&self) -> bool {
        self.parsed && self.resolved && self.arities_valid && self.no_unrelated
    }

    fn render(&self, name: &str) -> String {
        let tick = |b: bool| if b { "OK  " } else { "FAIL" };
        format!(
            "{name}\n    \
             {} parsed without recovery\n    \
             {} all names resolved\n    \
             {} constructor arities valid\n    \
             {} no unrelated diagnostics\n    \
             {} target diagnostic present\n    {}",
            tick(self.parsed),
            tick(self.resolved),
            tick(self.arities_valid),
            tick(self.no_unrelated),
            tick(self.target_present),
            self.failures.join("\n    ")
        )
    }
}

/// Run the pipeline for one witness.
fn validity(w: &Witness) -> Validity {
    use pw_core::codes::lookup;

    let mut v = Validity::default();
    let parsed = pw_syntax::parse_tree(&w.src);
    v.parsed = parsed.errors.is_empty();
    if !v.parsed {
        v.failures.push(format!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        ));
    }

    let got = symbols_for(w);
    // Resolution and arity are prerequisites for EVERY analysis, so a witness
    // tripping one has not reached the analysis it claims to be about.
    let is = |sym: &str| got.contains(sym);
    v.resolved = !is("unresolved_module") && !is("unresolved_name") && !is("duplicate_declaration");
    v.arities_valid = !is("constructor_arity");
    if !v.resolved {
        v.failures.push("names do not resolve".to_string());
    }
    if !v.arities_valid {
        v.failures
            .push("a constructor pattern's arity disagrees with its declaration".to_string());
    }

    let unrelated: Vec<&str> = got
        .iter()
        .copied()
        .filter(|s| *s != w.invariant.as_str())
        .collect();
    v.no_unrelated = unrelated.is_empty();
    if !v.no_unrelated {
        v.failures
            .push(format!("unrelated diagnostics: {unrelated:?}"));
    }
    v.target_present = got.contains(w.invariant.as_str());
    let _ = lookup;
    v
}

#[test]
fn every_witness_is_valid_evidence() {
    let mut bad = Vec::new();
    for w in witnesses() {
        let v = validity(&w);
        if !v.is_valid_evidence() {
            bad.push(v.render(&w.name));
        }
    }
    assert!(
        bad.is_empty(),
        "these witnesses are INVALID EVIDENCE — whatever they report or fail to \
         report is a property of the file, not of the analysis:\n\n  {}",
        bad.join("\n\n  ")
    );
}

/// A witness must be a valid program in every respect except the one it is
/// about.
///
/// The lesson from a witness that spent a few hours misfiled as a gap in the
/// exhaustiveness checker. It used constructor names its type did not have, so
/// its patterns lowered to nothing meaningful, the inner match was never
/// reached, and the file's silence was recorded as a property of the checker
/// rather than of the file. It was testing the compiler's behaviour on a
/// program that did not typecheck.
///
/// A `slips-through.pw` is the dangerous case — it is *expected* to be silent,
/// so an unrelated defect that makes it silent is invisible. But a `caught.pw`
/// can lie the same way, by being caught for something other than its
/// invariant.
#[test]
fn a_witness_is_valid_except_for_the_invariant_it_is_about() {
    let mut wrong = Vec::new();
    for w in witnesses() {
        let got = symbols_for(&w);
        let unrelated: Vec<&str> = got
            .iter()
            .copied()
            .filter(|s| *s != w.invariant.as_str())
            .collect();
        if !unrelated.is_empty() {
            wrong.push(format!(
                "{}: also reports {unrelated:?}. A witness must be a valid program \
                 except for `{}` — an unrelated defect makes its result a property \
                 of the file rather than of the analysis.",
                w.name, w.invariant
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_general_witness_is_caught_and_a_narrow_one_is_not() {
    let all = witnesses();
    assert!(!all.is_empty(), "no generality witnesses");

    let mut wrong = Vec::new();
    for w in &all {
        let got = symbols_for(w);
        match w.status.as_str() {
            "GENERAL" => {
                if !got.contains(w.invariant.as_str()) {
                    wrong.push(format!(
                        "{}: declares GENERAL for `{}` but was caught by {got:?} — the \
                         analysis does not reach this shape",
                        w.name, w.invariant
                    ));
                }
            }
            // A valid near-neighbour. It exists to prove the rule reacts to
            // the INVARIANT rather than to the construct, so it must report
            // nothing at all — not merely "not this invariant".
            "NEIGHBOUR" if !got.is_empty() => {
                wrong.push(format!(
                    "{}: is a valid neighbour and must compile clean, but reports \
                     {got:?}. Either the rule is banning the construct rather than \
                     checking the invariant, or the file is not actually valid.",
                    w.name
                ));
            }
            // The gap must still be a gap. If this fires, that is GOOD NEWS
            // reported as a failure: the analysis got better and the witness
            // needs promoting to `caught.pw`.
            "NARROW" if got.contains(w.invariant.as_str()) => {
                wrong.push(format!(
                    "{}: declares NARROW for `{}` but is now caught — the gap has \
                     CLOSED. Rename it to `caught.pw`, set @status: GENERAL, and \
                     update docs/evidence/P0/readiness.txt",
                    w.name, w.invariant
                ));
            }
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Invariants that appear in a headline P0 claim.
///
/// Architect ruling, 2026-08-06:
///
/// > Every invariant explicitly demonstrated in P0 must be challenge-tested
/// > rather than merely represented by one corpus fixture.
///
/// So these need a matrix — several dimensions of how the same violation can
/// be written — and a `DIMENSIONS.md` saying which dimensions are relevant and
/// which are not, with reasons. A single `caught.pw` is not enough for a claim
/// that goes in front of people.
const HEADLINE: &[&str] = &[
    "value_exceeds_sink_level",
    "private_in_shared_cache",
    "declared_placement_cannot_grant",
    "undeclared_effect",
    "forbidden_effect",
    "non_exhaustive_match",
    "affine_not_consumed_once",
    "scope_outlives_owner",
    // E6. Architect ruling, 2026-08-06:
    //
    //   > `PW5101` is especially important because it catches something no
    //   > individual declaration violates: the error exists on the EDGE between
    //   > two individually valid declarations. "Private data can never leak into
    //   > shared materialization through a dependency edge" is basically one of
    //   > the original reasons this project exists.
    //
    // `graph_edge_unresolved` is important but structural, so it needs
    // generality testing and not headline status.
    "private_in_shared_materialization",
];

#[test]
fn every_headline_invariant_has_a_challenge_matrix() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/generality");
    let all = witnesses();
    let mut missing = Vec::new();

    for h in HEADLINE {
        let files: Vec<&Witness> = all.iter().filter(|w| w.invariant == *h).collect();
        let general = files.iter().filter(|w| w.status == "GENERAL").count();
        let dims = root.join(h).join("DIMENSIONS.md").exists();

        // Three is the floor, not a target: the fixture's own shape, at least
        // two ways of writing it that the fixture does not have, and the
        // neighbour that proves the rule is not banning the construct.
        if general < 3 || !dims {
            missing.push(format!(
                "{h}: {general} general witness(es), DIMENSIONS.md {}",
                if dims { "present" } else { "MISSING" }
            ));
        }
    }

    // Recorded rather than asserted at zero: the matrices are being built one
    // invariant at a time, and this test names which are still owed. The floor
    // moves up as each lands.
    eprintln!(
        "  headline invariants with a matrix: {}/{}",
        HEADLINE.len() - missing.len(),
        HEADLINE.len()
    );
    for m in &missing {
        eprintln!("    owed: {m}");
    }
    // All eight now have one, so this is an equality: a headline claim
    // without a matrix is a claim resting on a single fixture.
    assert!(
        missing.is_empty(),
        "a headline invariant lost its challenge matrix:\n    {}",
        missing.join("\n    ")
    );
}

/// The second score, recorded next to the first.
///
/// Named **generality-tested** rather than "generally enforced" on the
/// architect's correction: no finite test suite establishes general
/// enforcement in the formal sense. What an invariant in this count has
/// survived is precisely
///
/// > its original fixture, a known counterexample to the previous narrow
/// > implementation, and the currently required neighbouring controls
///
/// which is stronger than corpus conformance and weaker than proof. The
/// wording has to stay true when the number reaches 29/29.
#[test]
fn generality_is_reported_separately_from_conformance() {
    let all = witnesses();
    let mut by_invariant: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
    for w in &all {
        let e = by_invariant.entry(w.invariant.as_str()).or_default();
        match w.status.as_str() {
            "GENERAL" => e.0 = true,
            "NARROW" => e.1 = true,
            // A neighbour on its own proves nothing about generality; it is a
            // control for the witnesses beside it.
            _ => {}
        }
    }

    // Every invariant the rejected corpus exercises.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rejected");
    let mut exercised: BTreeSet<String> = BTreeSet::new();
    for e in std::fs::read_dir(&root).expect("rejected") {
        let p = e.expect("entry").path();
        if p.extension().is_none_or(|x| x != "pw") {
            continue;
        }
        let src = std::fs::read_to_string(&p).expect("read");
        if let Some(sym) = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @invariant:"))
        {
            exercised.insert(sym.trim().to_string());
        }
    }

    // `capture_schema_disagrees` has witnesses but no corpus fixture — E7V is
    // newer than C1, which is frozen. Counted in the numerator only when the
    // corpus exercises it, so the ratio stays honest.
    let general: Vec<&str> = exercised
        .iter()
        .filter(|s| by_invariant.get(s.as_str()).is_some_and(|(g, n)| *g && !*n))
        .map(String::as_str)
        .collect();
    let narrow: Vec<&str> = exercised
        .iter()
        .filter(|s| by_invariant.get(s.as_str()).is_some_and(|(_, n)| *n))
        .map(String::as_str)
        .collect();
    let untested: Vec<&str> = exercised
        .iter()
        .filter(|s| !by_invariant.contains_key(s.as_str()))
        .map(String::as_str)
        .collect();

    // "generality-tested", not "generally enforced". No finite suite
    // establishes general enforcement in the formal sense; what nine of these
    // have survived is their original fixture, a counterexample that defeated
    // the previous implementation, and the neighbouring controls. That wording
    // stays honest even at 29/29.
    eprintln!("  corpus conformance:     44 / 44 (checking_source.rs)");
    eprintln!(
        "  generality-tested:      {} / {}",
        general.len(),
        exercised.len()
    );
    eprintln!("  known narrow witness:   {} — {narrow:?}", narrow.len());
    eprintln!(
        "  generality untested:    {} — {untested:?}",
        untested.len()
    );

    // Every invariant the corpus exercises has a challenge witness. That is an
    // equality, not a floor: an invariant losing its witness is a claim losing
    // its evidence.
    //
    // It does NOT mean the analyses are general. It means each has survived a
    // program its fixture did not anticipate, which is the strongest thing a
    // finite suite can say.
    assert_eq!(
        general.len() + narrow.len(),
        exercised.len(),
        "an invariant lost its challenge witness entirely: untested={untested:?}"
    );

    // A KNOWN-NARROW invariant is a lower claim, and it may only be made where
    // the gap is written down as a gap. Without this, `NARROW` is the escape
    // hatch that turns any failing witness into an accepted limitation — the
    // score would stay green while the numerator quietly fell.
    //
    // `DIMENSIONS.md` is where it has to be written, because that is the file
    // a reader of the claim opens.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/generality");
    let mut undeclared = Vec::new();
    for n in &narrow {
        let doc = root.join(n).join("DIMENSIONS.md");
        let says_so = std::fs::read_to_string(&doc)
            .map(|t| t.contains("Known gaps"))
            .unwrap_or(false);
        if !says_so {
            undeclared.push(format!(
                "{n}: has a NARROW witness and no `## Known gaps` section in \
                 {}",
                doc.display()
            ));
        }
    }
    assert!(
        undeclared.is_empty(),
        "a gap must be declared where the claim is read, not only in a file \
         header:\n  {}",
        undeclared.join("\n  ")
    );

    // And the headline number is what a reader sees, so it is asserted here
    // rather than only printed. `generality-tested` counts invariants with a
    // GENERAL witness and NO known gap.
    //
    // 31 / 31 since 2026-10-02: ADR-0128 closed the one invariant with a
    // known gap, `private_in_shared_materialization`, by reading what a
    // declaration is given. 33 / 33 since 2026-10-04: ADR-0183's two,
    // `title_missing` and `title_misplaced`, each with a GENERAL witness. 36 /
    // 36 the same day: ADR-0185's three, `duplicate_id`,
    // `reference_names_nothing` and `aria_unknown`. 37 / 37 the same day:
    // ADR-0186's `metadata_misplaced`. 38 / 38 the same day: ADR-0189's
    // `link_not_in_body`. 39 / 39 on 2026-10-05: ADR-0194's
    // `no_finite_value`. 41 / 41 the same day: ADR-0197's
    // `case_name_capitalized`, and `pattern_constructor`, whose misspelt
    // case it made a refusal. 42 / 42 the same day: ADR-0221's
    // `form_control_value`. 43 / 43 the same day: ADR-0226's
    // `derived_not_pure`, a value a template computes. 44 / 44 the same day:
    // ADR-0230's `operand_type`, a condition that tests a number. 45 / 45
    // on 2026-10-12: ADR-0311's `canonical_address`, a page's canonical
    // address. 46 / 46 on 2026-10-14: ADR-0195's ruling 13's
    // `load_from_another_origin`, a page loading from another origin. 49 /
    // 49 the same day: the scheduled-commands track's
    // `schedule_names_no_clocked_command`, `scheduled_outside_a_command`
    // and `schedule_time`. 54 / 54 the same day: corpus C24, the five of
    // NEXT 6c's `invoked_by`, which no fixture exercised until then. 56 / 56
    // on 2026-10-15: 4c's `read_per_element` and `set_pair_shape`. 60 / 60
    // on 2026-10-16: tables' `table_malformed`, `data_not_declared`,
    // `all_unbounded` and `migration_unstated` (ADR-0350, corpus C27).
    // PW0365's warning has its own witnesses too. 65 / 65 at the combined
    // train-12 merge: device-store adds two witnesses (ADR-0366, C28),
    // generic effects adds three (ADR-0351, C29). 66 / 66 on 2026-11-06:
    // the image ADR's `image_without_alt`, an `<img>` with no `alt` (C31).
    // Measured below; update STATUS and NEXT with the integrated result.
    assert_eq!(
        (general.len(), narrow.len(), untested.len()),
        (66, 0, 0),
        "the published figure moved. If that is intended, update
         docs/STATUS.md and docs/NEXT.md in the same commit — a number in a
         status report that no test holds is a number that drifts."
    );
}
