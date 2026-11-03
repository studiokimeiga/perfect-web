//! `pw-core`'s algorithms running against real `.pw` source (`docs/NEXT.md` 7).
//!
//! The corpus is the specification. A file marked `@corpus: accepted` that
//! produces an error is a checker bug, and that is asserted first — a checker
//! that reports more by reporting wrongly is worse than one that reports less.

use pw_core::check::check_sources;

/// **One rejected fixture, checked as its own program** (`rejected_program`),
/// held to the schema it names as released, where it names one (track
/// `tables`, ADR-0338): `// @schema-lock: <file beside it>`. A change from a
/// release is refused only against the release (PW0366).
fn check_fixture(path: &std::path::Path) -> Vec<(String, Vec<pw_core::diagnostics::Diagnostic>)> {
    let src = std::fs::read_to_string(path).expect("read");
    let released = src
        .lines()
        .find_map(|l| l.trim().strip_prefix("// @schema-lock:"))
        .map(|file| {
            let lock = path.with_file_name(file.trim());
            let text = std::fs::read_to_string(&lock)
                .unwrap_or_else(|e| panic!("{}: {e}", lock.display()));
            pw_core::tables::Schema::read(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", lock.display()))
        });
    pw_core::check::check_sources_with(&rejected_program(path), released.as_ref())
}

/// Declaration-level rules over one file, through the one parser.
///
/// E6F: `rules::check` reads the HIR. Before, this went through a second
/// declaration parser with its own tables — the thing E6 showed can disagree
/// with the real one about the same file.
fn declaration_rules(src: &str) -> Vec<pw_core::diagnostics::Diagnostic> {
    let p = parse_tree(src);
    pw_core::rules::check(&lower_file(src, &p.green))
}
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

fn corpus_dir(which: &str) -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .canonicalize()
        .expect("examples/ exists");
    let mut out: Vec<_> = std::fs::read_dir(root.join(which))
        .expect("corpus dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "pw"))
        .collect();
    out.sort();
    out
}

/// The library every corpus program is checked against: the shared `domain`
/// module plus E2C's platform interfaces.
///
/// Before E2B these tests fed the checker all 68 corpus files as if they were
/// one program. They are not — five rejected fixtures reuse module names with
/// each other, and 17 are reused across the buckets — so the compiler now
/// reports 20 duplicate declarations for a question nobody meant to ask.
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
    let domain = root.join("examples/domain.pw");
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(&domain).expect("domain.pw"),
    ));
    out.sort();
    out
}

/// The accepted corpus as the one program it is: library + every accepted file.
fn whole_corpus() -> Vec<(String, String)> {
    let mut out = library();
    for p in corpus_dir("accepted") {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        out.push((name, std::fs::read_to_string(&p).expect("read")));
    }
    out
}

/// Every rejected fixture, each checked as its own program.
///
/// Returns `(file name, source, diagnostics)`. Five of them reuse module names
/// with each other, so checking them together asks a question nobody meant.
fn rejected_results() -> Vec<(String, String, Vec<pw_core::diagnostics::Diagnostic>)> {
    corpus_dir("rejected")
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let src = std::fs::read_to_string(&p).expect("read");
            let results = check_fixture(&p);
            let diags = results
                .into_iter()
                .find(|(n, _)| *n == name)
                .expect("the fixture")
                .1;
            (name, src, diags)
        })
        .collect()
}

/// One rejected fixture, as its own program: the library, whatever accepted
/// modules the fixture imports, and the fixture.
///
/// A counterexample may legitimately depend on a correct module — `R-004` is a
/// page that MISUSES a correctly-declared session query, and the `Session`
/// label that makes it a violation comes from that query's own declaration.
/// Pulling the dependency in by its import is what keeps the fixture honest:
/// it sees what it asked for and nothing else.
fn rejected_program(path: &std::path::Path) -> Vec<(String, String)> {
    let src = std::fs::read_to_string(path).expect("read");
    let imported: Vec<String> = src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("import "))
        .map(|m| m.split(['.', ' ']).next().unwrap_or("").to_string())
        .collect();

    let mut out = library();
    for p in corpus_dir("accepted") {
        let s = std::fs::read_to_string(&p).expect("read");
        let Some(module) = s
            .lines()
            .find_map(|l| l.trim().strip_prefix("module "))
            .map(str::trim)
        else {
            continue;
        };
        let head = module.split('.').next().unwrap_or(module);
        if imported.iter().any(|i| i == head) {
            out.push((p.file_name().unwrap().to_string_lossy().to_string(), s));
        }
    }
    out.push((path.file_name().unwrap().to_string_lossy().to_string(), src));
    out
}

#[test]
fn no_accepted_corpus_file_reports_a_body_level_error() {
    // Checked first and on its own. A false positive on an accepted file means
    // the checker is wrong about the language, which is a worse failure than
    // missing a rejected case.
    let results = check_sources(&whole_corpus());
    let mut bad = Vec::new();
    for (path, diags) in &results {
        if !path.starts_with("A-") {
            continue;
        }
        // An accepted program may deliberately exercise a warning. Hold it
        // to exactly the warnings it names; an error never counts as one.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/accepted")
                .join(path),
        )
        .expect("accepted source");
        let mut expected: Vec<&str> = src
            .lines()
            .filter_map(|l| l.trim().strip_prefix("// @expect-warning:"))
            .map(str::trim)
            .collect();
        let mut actual = Vec::new();
        for d in diags {
            if d.severity == pw_core::diagnostics::Severity::Warning {
                actual.push(d.code);
            } else {
                bad.push(format!("{path}: [{}] {}", d.code, d.message));
            }
        }
        expected.sort();
        actual.sort();
        if expected != actual {
            bad.push(format!(
                "{path}: expected warnings {expected:?}, got {actual:?}"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "accepted files must be clean:\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_unhandled_variant_case_is_caught_at_its_real_span() {
    // R-007 declares `@rule: PW0305`. Since E2B it imports `OrderState` from
    // `domain` rather than seeing it through an ambient union, so its program
    // is library + itself.
    let path = corpus_dir("rejected")
        .into_iter()
        .find(|p| p.to_string_lossy().contains("R-007"))
        .expect("R-007");
    let results = check_sources(&rejected_program(&path));
    let (_, diags) = results
        .iter()
        .find(|(p, _)| p.starts_with("R-007"))
        .expect("R-007 is in the program");

    let d = diags
        .iter()
        .find(|d| d.code == "PW0305")
        .unwrap_or_else(|| panic!("R-007 must report PW0305, got {diags:?}"));

    assert_eq!(d.message, "match on `OrderState` is not exhaustive");

    let src = std::fs::read_to_string(&path).expect("read");

    // The span must cover the match, in the original `.pw` file — charter §14
    // M2 gate item 2 says "at original `.pw` spans", not at a generated file's.
    let text = &src[d.primary_span.clone()];
    assert!(
        text.starts_with("match state"),
        "primary span should cover the match, got {text:?}"
    );
    // And the related span must point at the scrutinee, per charter §16.3.
    assert_eq!(&src[d.related[0].span.clone()], "state");

    // The witnesses the file declares it expects.
    let repairs = d
        .repairs
        .iter()
        .map(|r| r.description.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    for want in ["Cancelled", "Failed"] {
        assert!(
            repairs.contains(want),
            "the missing variant `{want}` must be named, got {repairs}"
        );
    }
}

#[test]
fn a_nullary_constructor_is_not_read_as_a_binding() {
    // The load-bearing conversion. `Draft` has no parentheses, so the grammar
    // emits a binding pattern. Read as a binding it matches everything, the
    // first arm covers the scrutinee, and a non-exhaustive match is reported as
    // exhaustive — silently, and in the favourable direction.
    let src = "module m\n\
               type State = | Draft | Sent | Done\n\
               fn f(s: State) -> Int !{} {\n    match s {\n        Draft => 1,\n    }\n}\n";
    let files = vec![("t.pw".to_string(), src.to_string())];
    let diags = &check_sources(&files)[0].1;
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    let repairs: Vec<_> = diags[0].repairs.iter().map(|r| &r.description).collect();
    assert!(
        repairs.iter().any(|r| r.contains("Sent")) && repairs.iter().any(|r| r.contains("Done")),
        "both remaining variants must be named, got {repairs:?}"
    );

    // Positive control: the same match with every arm is clean.
    let full = src.replace(
        "        Draft => 1,\n",
        "        Draft => 1,\n        Sent => 2,\n        Done => 3,\n",
    );
    let clean = &check_sources(&[("t.pw".to_string(), full)])[0].1;
    assert!(
        clean.is_empty(),
        "the exhaustive version must be clean: {clean:?}"
    );

    // Negative control: a genuine binding still matches everything.
    let bound = src.replace("        Draft => 1,\n", "        anything => 1,\n");
    let none = &check_sources(&[("t.pw".to_string(), bound)])[0].1;
    assert!(
        none.is_empty(),
        "a binding that names no constructor must cover the match: {none:?}"
    );
}

#[test]
fn a_match_whose_scrutinee_type_is_unknown_is_not_reported() {
    // The rule that keeps this honest: no guessing. `x` has no declared type,
    // so nothing is claimed about it — silence, not a wildcard-shaped error.
    //
    // `x` was a name nothing declared until 2026-09-25. It is refused now
    // (PW0021, ADR-0047), so the unknown type comes from a parameter written
    // without one, which is what the rule is about.
    let src = "module m\n\
               type State = | Draft | Sent\n\
               fn f(x) -> Int !{} {\n    match x {\n        Draft => 1,\n    }\n}\n";
    let diags = &check_sources(&[("t.pw".to_string(), src.to_string())])[0].1;
    assert!(diags.is_empty(), "must not guess a type: {diags:?}");

    // Control: give it a type and the same match is reported.
    let typed = src.replace("fn f(x)", "fn f(x: State)");
    let now = &check_sources(&[("t.pw".to_string(), typed)])[0].1;
    assert_eq!(now.len(), 1, "with a declared type it must be caught");
}

#[test]
fn constructor_arity_is_respected() {
    // `Confirmed(_)` covers `Confirmed`, but `Confirmed` alone must not be
    // treated as covering a constructor that carries a field, and vice versa.
    let src = "module m\n\
               type S = | A | B(Int)\n\
               fn f(s: S) -> Int !{} {\n    match s {\n        A => 1,\n        B(n) => n,\n    }\n}\n";
    let diags = &check_sources(&[("t.pw".to_string(), src.to_string())])[0].1;
    assert!(diags.is_empty(), "this is exhaustive: {diags:?}");

    let missing_b = src.replace("        B(n) => n,\n", "");
    let d = &check_sources(&[("t.pw".to_string(), missing_b)])[0].1;
    assert_eq!(d.len(), 1);
    assert!(
        d[0].repairs.iter().any(|r| r.description.contains("B(")),
        "the witness must show the field, got {:?}",
        d[0].repairs
    );
}

#[test]
fn the_environment_spans_every_file_checked_together() {
    // Assumption A-009 made testable: a type declared in one file is visible to
    // a match in another, because one invocation is one program.
    let a = (
        "a.pw".to_string(),
        "module a\ntype T = | X | Y\n".to_string(),
    );
    // **`b` imports `T`.** It did not until 2026-08-21, and the match was
    // checkable anyway — because the type environment was keyed by bare NAME
    // and found `T` ambiently, in a module that never asked for it. That is the
    // same defect as calling a name you did not import (`PW0021`), one layer
    // over, and it was holding this test up.
    //
    // The assumption A-009 makes testable is that one invocation is one
    // program, so an IMPORTED type crosses a file. Not that any type does.
    let b = (
        "b.pw".to_string(),
        "module b\nimport a.{ T }\nfn f(t: T) -> Int !{} {\n    match t {\n        X => 1,\n    }\n}\n"
            .to_string(),
    );

    let together = check_sources(&[a.clone(), b.clone()]);
    assert_eq!(
        together.iter().map(|(_, d)| d.len()).sum::<usize>(),
        1,
        "checked together, the match is checkable"
    );

    // Control: alone, `T` is unknown and **no exhaustiveness claim is made**.
    //
    // Not "no diagnostics at all" — that was the assertion until 2026-08-21 and
    // it became wrong in the right direction. `b` alone is a module importing
    // one that is not there, and `PW0020` says so. What must not happen is a
    // verdict about the match, because the analysis has no constructors to
    // reason from and guessing is how a wrong answer gets a right shape.
    let alone = check_sources(&[b]);
    let codes: Vec<&str> = alone[0].1.iter().map(|d| d.code).collect();
    assert!(
        !codes.contains(&"PW0305"),
        "alone, the type is unknown and must not be guessed: {codes:?}"
    );
    assert_eq!(
        codes,
        ["PW0020"],
        "and the reason it is unknown is reported rather than silent"
    );
}

#[test]
fn every_diagnostic_carries_what_charter_16_3_requires() {
    // rule + origin span + boundary span + inferred label + legal alternative.
    let mut all: Vec<(String, String, Vec<pw_core::diagnostics::Diagnostic>)> = rejected_results();
    // Accepted files must produce none, which is asserted elsewhere; include
    // them so a diagnostic that only appears there is still shape-checked.
    for (n, s) in whole_corpus() {
        let d = check_sources(&whole_corpus())
            .into_iter()
            .find(|(p, _)| *p == n)
            .map(|(_, d)| d)
            .unwrap_or_default();
        all.push((n, s, d));
    }
    // A note may be in another file the checker was given (ADR-0321: a
    // first declaration in its own file), held to that file's length.
    let lengths: std::collections::BTreeMap<String, usize> = whole_corpus()
        .into_iter()
        .map(|(name, src)| (name, src.len()))
        .collect();
    let mut n = 0;
    for (path, src, diags) in &all {
        for d in diags {
            n += 1;
            assert!(!d.code.is_empty() && !d.invariant.is_empty(), "{path}");
            assert!(d.primary_span.end <= src.len(), "{path}: span out of range");
            assert!(!d.related.is_empty(), "{path}: no boundary span");
            assert!(
                d.related.iter().all(|r| {
                    let len = match &r.file {
                        None => Some(src.len()),
                        Some(file) => lengths.get(file).copied(),
                    };
                    len.is_some_and(|len| r.span.end <= len)
                }),
                "{path}: related span out of range of its file"
            );
            assert!(
                !d.repairs.is_empty(),
                "{path}: no legal alternative offered"
            );
            assert!(d.explanation.is_some(), "{path}: no explanation");
        }
    }
    assert!(n > 0, "the corpus must produce at least one diagnostic");
}

/// Everything a developer would be shown for one diagnostic, as one string.
fn rendered(d: &pw_core::diagnostics::Diagnostic) -> String {
    let mut s = format!("[{}] {} — {}", d.code, d.invariant, d.message);
    if let Some(e) = &d.explanation {
        s.push_str(e);
    }
    for r in &d.repairs {
        s.push_str(&r.description);
    }
    for r in &d.related {
        s.push_str(&r.label);
    }
    s
}

/// The backticked payloads of a corpus file's `@expect-error` lines.
fn expected_payloads(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|l| l.trim().strip_prefix("// @expect-error:"))
        .flat_map(|l| {
            l.split('`')
                .skip(1)
                .step_by(2)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_caught_file_conveys_every_fact_its_expect_error_lines_declare() {
    // The corpus is the specification. `@expect-error` states what the
    // developer must be told; this asserts the diagnostic actually says it,
    // without pinning the exact prose.
    let mut checked = 0;
    let mut missing = Vec::new();

    for p in corpus_dir("rejected") {
        let results = check_fixture(&p);
        let path = p.file_name().unwrap().to_string_lossy().to_string();
        let diags = &results
            .iter()
            .find(|(n, _)| *n == path)
            .expect("the fixture")
            .1;
        if diags.is_empty() {
            continue;
        }
        let src = &std::fs::read_to_string(&p).expect("read");
        let path = &path;
        let all: String = diags.iter().map(rendered).collect::<Vec<_>>().join(" ");
        for want in expected_payloads(src) {
            checked += 1;
            if !all.contains(&want) {
                missing.push(format!("{path}: never mentions `{want}`"));
            }
        }
    }

    assert!(checked > 0, "no caught file declared an @expect-error");
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// Fixtures whose declared invariant is only *partly* enforced.
///
/// Architect ruling, 2026-08-06:
///
/// > A corpus fixture counts as enforced only when the entire invariant it
/// > specifies is checked, not merely when its canonical code appears.
///
/// R-044 was the case that produced the rule: the checker required a `because`
/// justification and stopped, so a fixture missing its *attribution target*
/// went green. Both halves are now checked and the list is empty — it stays,
/// because the next partial rule will need somewhere to be recorded rather
/// than rounded up.
const PARTIALLY_ENFORCED: &[(&str, &str)] = &[];

#[test]
fn corpus_enforcement_is_reported_as_three_numbers_not_one() {
    // "19/44 enforced" hides the difference between a red diagnostic, the
    // *right* red diagnostic, and the whole declared invariant being checked.
    // A header's code is read as written (track evidence-integrity-2).
    let (mut errored, mut declared_code, mut fully) = (0, 0, 0);
    let mut total = 0;

    for (name, src, diags) in rejected_results() {
        total += 1;
        let mut codes: Vec<&str> = declaration_rules(&src)
            .iter()
            .filter(|f| f.is_error())
            .map(|f| f.code)
            .collect();
        codes.extend(diags.iter().map(|d| d.code));
        if codes.is_empty() {
            continue;
        }
        errored += 1;

        let want = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @rule:"))
            .map(str::trim);
        if !want.is_some_and(|w| codes.contains(&w)) {
            continue;
        }
        declared_code += 1;

        let id = &name[..5];
        if !PARTIALLY_ENFORCED.iter().any(|(f, _)| *f == id) {
            fully += 1;
        }
    }

    // Recorded rather than asserted at an exact value: these move as rules land.
    eprintln!("  {errored}/{total} rejected fixtures produce a compile error");
    eprintln!("  {declared_code}/{total} emit their declared canonical code");
    eprintln!("  {fully}/{total} fully enforce the complete declared invariant");

    // The floor moves only upward, and only when a real rule lands. It was
    // briefly 27 while `secret<Payments>` failed to cover `secret.read`: two
    // fixtures reported an effect they had in fact declared. Both were wrong-
    // reason catches, which is why `declared_code == errored` is asserted for
    // equality and not as another floor — a catch that is merely red is a
    // regression even when the count goes up.
    // The floor is the corpus, not a constant. It was 44 at C2 and is 46 at
    // C3, and writing the number twice is how a corpus grows while the ratchet
    // stays where it was — the count rises, the assertion still passes, and the
    // new fixtures are unenforced without anything going red.
    assert!(
        errored >= total,
        "regressed: {errored}/{total} produce an error"
    );
    assert_eq!(
        declared_code, errored,
        "every catch must be for the declared invariant, not merely red"
    );
    assert!(
        fully >= total,
        "regressed: {fully}/{total} fully enforced, partial list = {PARTIALLY_ENFORCED:?}"
    );
}

#[test]
fn semantic_coverage_of_the_rejected_corpus_does_not_regress() {
    // The ratchet from docs/NEXT.md item 8, counting declaration rules AND
    // body-level checks together, each fixture as its own program.
    let mut caught = Vec::new();
    let mut total = 0;
    for (name, src, diags) in rejected_results() {
        total += 1;
        let decl_hits = declaration_rules(&src).iter().any(|f| f.is_error());
        if decl_hits || !diags.is_empty() {
            caught.push(name);
        }
    }

    assert!(
        total >= 44,
        "expected the full rejected corpus, saw {total}"
    );
    assert!(
        caught.len() >= 25,
        "semantic coverage regressed: {}/{total} caught — {caught:?}",
        caught.len()
    );
}

#[test]
fn every_body_level_rejection_matches_its_declared_rule() {
    let mut mismatches = Vec::new();
    let mut checked = 0;

    for (path, src, diags) in rejected_results() {
        if diags.is_empty() {
            continue;
        }
        let src = &src;
        let Some(rule) = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @rule:"))
            .map(str::trim)
        else {
            continue;
        };
        checked += 1;
        // As written: an old number is not what `pw check` reports.
        let want = rule;
        let got: Vec<&str> = diags.iter().map(|d| d.code).collect();
        if !got.contains(&want) {
            mismatches.push(format!("{path}: declares {want}, got {got:?}"));
        }
    }

    assert!(checked > 0, "no rejected file was caught by a body check");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn the_structured_concurrency_rules_run_on_real_bodies() {
    // docs/NEXT.md item 7 for `scope.rs`: the E2A-S graph and its rules were
    // written and tested against hand-built inputs. Nothing is re-implemented
    // here — the bridge builds the graph from HIR and converts what comes back.
    let all = rejected_results();
    let find = |prefix: &str, code: &'static str| {
        let (_, src, diags) = all
            .iter()
            .find(|(p, _, _)| p.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} is in the corpus"));
        let d = diags
            .iter()
            .find(|d| d.code == code)
            .unwrap_or_else(|| panic!("{prefix} must report {code}, got {diags:?}"));
        (src.clone(), d.clone())
    };

    // R-013: `task.spawn(detached)` with no durable capability. The file
    // declares PW2002, the invariant's code (it said PW0311, an alias, until
    // track evidence-integrity held each header to its code as written).
    let (src, d) = find("R-013", "PW2002");
    let src = &src;
    // The primary span is the offending argument, the related span is where the
    // handle came from — charter §16.3 wants both, and they must differ.
    assert_eq!(&src[d.primary_span.clone()], "detached");
    assert!(src[d.related[0].span.clone()].starts_with("task.spawn"));
    assert_ne!(d.primary_span, d.related[0].span);
    assert_ne!(
        d.explanation.as_deref(),
        Some(d.repairs[0].description.as_str()),
        "the note and the help must not be the same sentence"
    );

    // R-039: a subscription declaring a scope that outlives its component.
    let (src, d) = find("R-039", "PW2004");
    let src = &src;
    assert_eq!(&src[d.primary_span.clone()], "scope application");

    // Control: the same subscription scoped to its component is legal, and a
    // `durable.spawn` is the sanctioned way to outlive a scope.
    let ok = "module m\ncomponent C() {\n    let v = observe intersection(self) -> Bool { scope component }\n    view { <p /> }\n}\n";
    assert!(
        check_sources(&[("ok.pw".into(), ok.into())])[0]
            .1
            .is_empty(),
        "a component-scoped observation must be accepted: {:?}",
        check_sources(&[("ok.pw".into(), ok.into())])[0].1
    );
    // `g` is declared: since 2026-08-10 a bare call to a name nothing declares
    // is `PW0021`, and a control that tripped it would be measuring the wrong
    // rule.
    let plain =
        "module m\nfn g() -> () !{} { todo }\nview V() !{} {\n    task.spawn(work) { g() }\n}\n";
    assert!(
        check_sources(&[("p.pw".into(), plain.into())])[0]
            .1
            .is_empty(),
        "an ordinary scoped spawn must be accepted"
    );
}

#[test]
fn the_privacy_and_placement_rules_catch_their_corpus_cases() {
    // E5's first slice. Each case is a *flow* answered by the algebra in
    // `pw_core::privacy` and `pw_core::placement`, not a syntax pattern.
    let all = rejected_results();
    let want = [
        ("R-002", "PW5002"), // database read inside a browser-placed component
        ("R-003", "PW5003"), // a secret rendered into markup
        ("R-005", "PW5004"), // shared cache keyed without the tenant
        // A secret capability at the edge. PW5002 until ADR-0216, when its
        // `placement edge` moved where a function's is read, and the rule it
        // was written for spoke.
        ("R-026", "PW5005"),
    ];
    for (file, code) in want {
        let (_, src, diags) = all
            .iter()
            .find(|(p, _, _)| p.starts_with(file))
            .unwrap_or_else(|| panic!("{file} is in the corpus"));
        let d = diags
            .iter()
            .find(|d| d.code == code)
            .unwrap_or_else(|| panic!("{file} must report {code}, got {diags:?}"));

        // Charter §14 M5 gate: "Diagnostics name the source value and invalid
        // boundary, not merely a type mismatch."
        assert!(d.primary_span.end <= src.len());
        assert!(!d.related.is_empty(), "{file}: no boundary span");
        let note = d.explanation.as_deref().unwrap_or("");
        assert!(
            note.len() > 40,
            "{file}: the note must explain the cause chain, got {note:?}"
        );
    }

    // Control: the placement solver must not fire on the accepted corpus, where
    // every declaration has somewhere legal to run.
    let bad: Vec<String> = check_sources(&whole_corpus())
        .into_iter()
        .filter(|(p, d)| p.starts_with("A-") && d.iter().any(|d| d.code.starts_with("PW50")))
        .map(|(p, _)| p)
        .collect();
    assert!(bad.is_empty(), "false positives on accepted files: {bad:?}");
}

#[test]
fn an_unmodelled_capability_family_never_manufactures_a_placement_error() {
    // The failure this rule already had once: an effect family absent from the
    // table ruled out every world, so two corpus files were reported for the
    // wrong reason. A coverage number that goes up without a detection is worse
    // than one that stays put.
    let src = "module m\nfn f() -> Int !{ telemetry.emit, log<Public> } {\n    1\n}\n";
    let diags = &check_sources(&[("t.pw".into(), src.into())])[0].1;
    assert!(
        diags.iter().all(|d| d.code != "PW5002"),
        "an unmodelled family must not be unplaceable: {diags:?}"
    );
}

/// The per-fixture enforcement table, written to `docs/evidence/E2D/`, and
/// held to what the corpus checks now.
///
/// The charter requires a gate claim to be backed by a file a recorded command
/// produced. The obvious way to produce it — a separate reporting binary — was
/// rejected: it would need its own copy of "the library, plus the accepted
/// modules a fixture imports, plus the fixture", and a second copy of that is
/// how a reported number drifts from an enforced one. This is the same code
/// path the ratchet asserts on.
///
/// Written only when asked (`just evidence-corpus`), it drifted all the same:
/// recorded on 2026-08-07, it said 46 of 46 fixtures enforce their invariant
/// of a corpus of 74, with a column of old numbers the corpus no longer
/// wrote, until track evidence-integrity-2 (the integrator's ruling 2,
/// 2026-10-15). Now, unless asked to write it, the test holds the committed
/// file to the table it computes, as `evidence_is_current.rs` holds the WIT:
/// staleness is a failure with its repair in the message. A header's code is
/// read as written: no old number is translated (ruling 1).
#[test]
fn the_committed_enforcement_table_is_what_the_corpus_checks_now() {
    let mut rows: Vec<String> = Vec::new();
    let (mut caught, mut total) = (0, 0);

    for (name, src, diags) in rejected_results() {
        total += 1;
        let mut codes: Vec<String> = declaration_rules(&src)
            .iter()
            .filter(|f| f.is_error())
            .map(|f| f.code.to_string())
            .collect();
        codes.extend(diags.iter().map(|d| d.code.to_string()));
        codes.sort();
        codes.dedup();

        let declared = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @rule:"))
            .map(|r| r.trim().to_string())
            .unwrap_or_else(|| "-".to_string());
        let category = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @category:"))
            .unwrap_or("")
            .trim();

        let verdict = if codes.contains(&declared) {
            caught += 1;
            "enforced".to_string()
        } else if codes.is_empty() {
            "UNCAUGHT".to_string()
        } else {
            format!("WRONG REASON ({})", codes.join(" "))
        };
        let id = &name[..5];
        rows.push(format!(
            "{id}  {verdict:<24}  {declared}\n       {category}"
        ));
    }

    let mut report = String::new();
    report.push_str(
        "corpus enforcement — every rejected fixture, checked as its own program\n\
         (the shared library, the accepted modules it imports, and the fixture)\n\n\
         produced by: just evidence-corpus\n\
         held to the corpus by: compiler/pw-core/tests/checking_source.rs\n\
         (the_committed_enforcement_table_is_what_the_corpus_checks_now)\n\n",
    );
    report.push_str(&format!(
        "{caught}/{total} enforce their declared invariant\n\n"
    ));
    for r in &rows {
        report.push_str(r);
        report.push('\n');
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/evidence/E2D/corpus-enforcement.txt");
    if std::env::var("PW_WRITE_EVIDENCE").is_ok() {
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("evidence dir");
        std::fs::write(&path, &report).expect("write evidence");
        eprintln!("wrote docs/evidence/E2D/corpus-enforcement.txt");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == report,
        "docs/evidence/E2D/corpus-enforcement.txt is not what the corpus checks now \
         ({caught}/{total} enforce their declared invariant): run `just evidence-corpus` \
         and commit the file"
    );
}

/// The three-way agreement the architect asked for after `PW0323`.
///
/// > fixture symbol == emitted invariant symbol == registry meaning for the
/// > emitted numeric code
///
/// Matching on the numeric code alone is matching on a *label*. `PW0323`
/// carried three meanings at once — the registry's prose, the placement rule
/// that emitted it, and the fixture that declared it — and every code-level
/// assertion in this file passed the whole time, because the code genuinely
/// matched. The symbol is the semantic identity; the number is for users.
#[test]
fn a_fixtures_declared_symbol_matches_the_symbol_it_is_caught_by() {
    use pw_core::codes::lookup;
    use pw_core::diagnostics::UNREGISTERED;
    let mut wrong = Vec::new();
    let mut checked = 0;

    for (name, src, diags) in rejected_results() {
        let Some(declared) = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @invariant:"))
            .map(str::trim)
        else {
            wrong.push(format!("{name}: no `// @invariant:` line"));
            continue;
        };

        // What the registry says the fixture's declared CODE means.
        let want = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @rule:"))
            .map(str::trim)
            .and_then(lookup);
        match want {
            Some(c) if c.symbol == declared => {}
            Some(c) => {
                wrong.push(format!(
                    "{name}: declares `{declared}` but its @rule resolves to \
                     `{}` — the number and the symbol disagree",
                    c.symbol
                ));
                continue;
            }
            None => {
                wrong.push(format!("{name}: its @rule is not a registered code"));
                continue;
            }
        }

        // What actually caught it.
        let mut symbols: Vec<&str> = declaration_rules(&src)
            .iter()
            .filter(|f| f.is_error())
            .filter_map(|f| lookup(f.code).map(|c| c.symbol))
            .collect();
        symbols.extend(diags.iter().map(|d| d.symbol()));
        assert!(
            !symbols.contains(&UNREGISTERED),
            "{name}: emitted a diagnostic whose code is not in the registry"
        );

        checked += 1;
        if !symbols.contains(&declared) {
            wrong.push(format!(
                "{name}: declares `{declared}`, caught by {symbols:?}"
            ));
        }
    }

    assert_eq!(
        checked,
        rejected_results().len(),
        "every rejected fixture must be checked"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What is wrong with a rejected fixture's `@rule` and `@invariant`, read as
/// written, against the codes its check reports (track evidence-integrity,
/// finding 3).
///
/// The tests above read a header's number through a translation of the old
/// numbers until 2026-10-15, so a number the registry had retired into an
/// alias still passed: R-004 said `PW0100` where the code is `PW5001`, R-024
/// `PW0322` where it is `PW5010`, and forty fixtures in all named a number
/// `pw check` never prints. A header is what a reader looks up, so it says
/// the code itself; and since track evidence-integrity-2 (ruling 1) nothing
/// translates an old number: the list is read here only to say, in the
/// refusal, which code an old number became.
fn header_problems(name: &str, src: &str, reported: &[&str]) -> Vec<String> {
    use pw_core::codes::{RETIRED, lookup};
    use pw_core::diagnostics::DEPRECATED_ALIASES;
    let header = |key: &str| {
        src.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(str::trim)
    };
    let Some(rule) = header("// @rule:") else {
        return vec![format!("{name}: no `// @rule:` line")];
    };
    if let Some((_, code)) = DEPRECATED_ALIASES.iter().find(|(alias, _)| *alias == rule) {
        return vec![format!(
            "{name}: @rule {rule} is a retired number, an alias of {code}, \
             which is what `pw check` reports"
        )];
    }
    if RETIRED.iter().any(|(id, _)| *id == rule) {
        return vec![format!("{name}: @rule {rule} is a retired code")];
    }
    let Some(code) = lookup(rule) else {
        return vec![format!("{name}: @rule {rule} is no registered code")];
    };
    let mut out = Vec::new();
    let invariant = header("// @invariant:");
    if invariant != Some(code.symbol) {
        out.push(format!(
            "{name}: @invariant {invariant:?}, and {rule} is `{}`",
            code.symbol
        ));
    }
    if !reported.contains(&rule) {
        out.push(format!(
            "{name}: @rule {rule}, and its check reports {reported:?}"
        ));
    }
    out
}

#[test]
fn a_fixtures_header_is_held_to_its_code_as_written() {
    let src = |rule: &str, invariant: &str| {
        format!("// @corpus: rejected\n// @rule: {rule}\n// @invariant: {invariant}\nmodule m\n")
    };
    let problems = |rule: &str, invariant: &str, reported: &[&str]| {
        header_problems("R-0", &src(rule, invariant), reported).join("\n")
    };
    // The control: the code its check reports, and that code's invariant.
    assert_eq!(
        problems("PW5001", "private_in_shared_cache", &["PW5001"]),
        ""
    );
    // As R-004's was: a number retired into an alias of the code reported.
    assert!(
        problems("PW0100", "private_in_shared_cache", &["PW5001"]).contains("an alias of PW5001"),
        "an alias passed"
    );
    // A code its check does not report, another code's invariant, a code no
    // one registered, and one retired.
    assert!(
        problems("PW5001", "private_in_shared_cache", &["PW5004"]).contains("its check reports"),
        "a code not reported passed"
    );
    assert!(
        problems("PW5001", "unsafe_audit_incomplete", &["PW5001"]).contains("@invariant"),
        "another code's invariant passed"
    );
    assert!(
        problems("PW9999", "private_in_shared_cache", &["PW9999"]).contains("no registered code"),
        "an unknown code passed"
    );
    assert!(
        problems("PW5102", "private_in_shared_cache", &["PW5102"]).contains("retired code"),
        "a retired code passed"
    );
}

/// **Every rejected fixture's `@rule` is a code `pw check` reports for it,
/// as written, and its `@invariant` is that code's** (track
/// evidence-integrity, finding 3): the fixture checked as its own program,
/// by `check_sources`, which `pw check` runs.
#[test]
fn every_rejected_fixtures_rule_is_a_code_its_check_reports() {
    let all = rejected_results();
    let mut wrong = Vec::new();
    for (name, src, diags) in &all {
        let reported: Vec<&str> = diags.iter().map(|d| d.code).collect();
        wrong.extend(header_problems(name, src, &reported));
    }
    assert!(
        all.len() >= 44,
        "expected the full rejected corpus, saw {}",
        all.len()
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A construction site may not restate an invariant in its own words.
///
/// The `invariant` field is written at each `Diagnostic { .. }` literal, and a
/// second prose definition of one number is half of what made `PW0323`
/// invisible. The registry's sentence is the sentence.
#[test]
fn no_diagnostic_restates_its_invariant_in_its_own_words() {
    use pw_core::codes::lookup;

    let mut wrong = Vec::new();
    for (name, _, diags) in rejected_results() {
        for d in &diags {
            let Some(c) = lookup(d.code) else { continue };
            if d.invariant != c.invariant {
                wrong.push(format!(
                    "{name}: {} says {:?}, the registry says {:?}",
                    d.code, d.invariant, c.invariant
                ));
            }
        }
    }
    wrong.sort();
    wrong.dedup();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Fixtures that deliberately contain more than one defect.
///
/// Empty, and it should stay that way. An entry here is a fixture that is not
/// a single-defect test, and it must list every invariant it expects — a
/// fixture with an unexplained extra diagnostic is how R-004 hid a false
/// positive for a day.
const MULTI_DEFECT: &[(&str, &[&str])] = &[];

/// Every rejected fixture emits ONLY what it is meant to test.
///
/// Architect ruling, 2026-08-06, after `query` was found to be parsed as two
/// expressions:
///
/// > A fixture passes only when it emits exactly its intended semantic defect.
///
/// R-004 is why. It is caught for `private_in_shared_cache`, its declared
/// invariant, so the ratchet said "enforced, right reason" — while a second
/// diagnostic sat in the output saying the page reached the database during
/// rendering, which was a false positive about a perfectly ordinary query
/// dependency. Nothing looked at it, because nothing was asked to.
///
/// The stronger C1 statement this buys:
///
/// > Every rejected fixture is otherwise valid and emits no unexplained
/// > semantic diagnostic.
#[test]
fn every_rejected_fixture_emits_only_its_own_defect() {
    use pw_core::codes::lookup;
    let mut clean = 0;
    let mut total = 0;
    let mut noisy = Vec::new();
    for (name, src, diags) in rejected_results() {
        total += 1;
        let declared = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @invariant:"))
            .map(str::trim)
            .unwrap_or("");
        let mut got: Vec<&str> = declaration_rules(&src)
            .iter()
            .filter(|f| f.is_error())
            .filter_map(|f| lookup(f.code).map(|c| c.symbol))
            .collect();
        got.extend(diags.iter().map(|d| d.symbol()));
        got.sort();
        got.dedup();
        let id = &name[..5];
        let also_expected: &[&str] = MULTI_DEFECT
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, v)| *v)
            .unwrap_or(&[]);
        let extra: Vec<&str> = got
            .into_iter()
            .filter(|s| *s != declared && !also_expected.contains(s))
            .collect();
        if extra.is_empty() {
            clean += 1;
        } else {
            noisy.push(format!(
                "{id}: declares `{declared}` and also emits {extra:?} — either the \
                 fixture has a second defect (declare it in MULTI_DEFECT) or one of \
                 those is a false positive"
            ));
        }
    }
    eprintln!("  single-defect isolation: {clean}/{total}");
    assert!(noisy.is_empty(), "{}", noisy.join("\n"));
    assert_eq!(clean, total);
}
