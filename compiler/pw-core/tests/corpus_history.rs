//! Historical-corpus compatibility — the guard against editing a fixture into
//! compliance.
//!
//! Architect ruling, 2026-08-06:
//!
//! > Keep two distinct measurements. Current-corpus conformance tells you
//! > whether today's specification passes. Historical-corpus compatibility
//! > prevents a fixture from being edited into compliance and silently erasing
//! > a previously discovered failure.
//!
//! Ten fixtures changed on the way to 44/44. Nine gained an import or a
//! declaration they called and the program lacked — a legitimate repair, and
//! also exactly the shape of change that could remove the **defect** instead of
//! the **obstruction** without anyone noticing, because the fixture would go on
//! being reported either way.
//!
//! So the old text is kept, and asked the only question that distinguishes the
//! two: does it still fail?

use pw_core::check::check_sources;

/// Old texts no longer caught for their declared invariant, each **classified**.
/// They may still be rejected for an unresolved capture or another real defect.
///
/// Architect ruling, 2026-08-06:
///
/// > Do not force it to 10/10 merely for neatness. Classify the remaining
/// > case, and report that classification next to the result. The history
/// > suite is valuable precisely because it can disagree with the current
/// > corpus.
///
/// Four classifications are possible, and they mean very different things:
///
/// - `FixtureDidNotExpressIt` — the old text did not actually contain the
///   violation it declared. Benign; the corpus got more precise.
/// - `SpecificationChanged` — the language or the charter changed, so the old
///   text is no longer wrong. Needs a charter reference.
/// - `CompilerRegressed` — it used to be caught and is not. A bug, and this
///   test is how it surfaces.
/// - `KnownCheckerGap` — the current checker cannot see it. Belongs in
///   `examples/generality/` as a `slips-through.pw` too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    FixtureDidNotExpressIt,
    #[allow(dead_code)]
    SpecificationChanged,
    #[allow(dead_code)]
    CompilerRegressed,
    #[allow(dead_code)]
    KnownCheckerGap,
}

const EXPECTED_TO_PASS: &[(&str, Why, &str)] = &[
    (
        "R-022",
        Why::FixtureDidNotExpressIt,
        "C0's on_press parameter names PressEvent without importing events. The old \
         mismatch compared its spelling to SubmitEvent even though one side named no \
         visible type. The current fixture imports the event declaration; that \
         resolved mismatch remains rejected. The new signature tests require missing \
         and invalid annotations to remain distinct and exercise a valid same-spelled \
         nominal mismatch across modules.",
    ),
    (
        "R-030",
        Why::FixtureDidNotExpressIt,
        "C0's Cart parameter imports neither domain.Cart nor the scoped resource. The \
         spelling alone formerly selected a global private type. It is now an \
         unnameable capture and remains refused, not considered public and \
         serializable. The current explicit-import fixture still reports \
         private_in_resume_manifest; the regression suite checks both boundaries.",
    ),
    (
        "R-010",
        Why::FixtureDidNotExpressIt,
        "C0 writes DatabaseConnection without importing its defining module. Resolved \
         signatures can no longer treat that spelling as the globally known resource. \
         The unknown capture is still refused as resume_capture_schema_unnameable; the \
         current fixture explicitly imports domain.DatabaseConnection and is refused \
         as unserializable_capture. The resolved-signature regression suite checks \
         both sides independently.",
    ),
    (
        "R-001",
        Why::FixtureDidNotExpressIt,
        "its old text called `fetch_store` without importing `Stores`, so the \
         call named nothing this program declares. It became catchable only \
         because effect inference matched a bare spelling against every \
         declaration in the program — the ambient resolution E2B removed and \
         the architect ordered deleted on 2026-08-07 (`docs/RISK_QUEUE.md` \
         34). With honest resolution a file that imports nothing sees nothing, \
         so the old text cannot express `forbidden_effect` and its silence is \
         correct. The repaired fixture, which imports `Stores`, is caught.",
    ),
    (
        "R-004",
        Why::FixtureDidNotExpressIt,
        "its old text rendered `Cart` without importing the module that \
         declares it, so the name resolved to nothing. `docs/CORPUS.md` records \
         that it was caught *through an ambient module union that E2B removed*; \
         what kept catching it afterwards was the privacy checker resolving a \
         label by BARE DECLARATION NAME, which item 1a replaced with resolution \
         through the workspace. `private_in_shared_cache` is a statement about \
         a query this page actually reads, and the old text reads nothing.",
    ),
    (
        "R-024",
        Why::FixtureDidNotExpressIt,
        "the same shape as R-001: `raw_html` was called without importing \
         `html`, so the call named nothing. `unsafe_audit_incomplete` is a \
         statement about a declared unsafe capability, and the old text had no \
         declaration to be incomplete about — it was caught only by spelling.",
    ),
    // R-023 was here until 2026-10-04: its old text declared no route, so
    // there was no route table for its link to be dead relative to. A-025,
    // the accepted page that states its title (ADR-0183), declares a route
    // in the program every old text is checked in, and the old link is dead
    // relative to it: the old text is caught for its invariant again.
];

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
    // The accepted modules a fixture may import, as the current harness does.
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

fn c0() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/history/C0");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&root)
        .expect("examples/history/C0")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "pw"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn the_pre_change_text_of_every_repaired_fixture_still_fails() {
    let all = c0();
    assert_eq!(
        all.len(),
        10,
        "docs/CORPUS.md records ten changed fixtures; C0 holds {}",
        all.len()
    );

    let mut wrong = Vec::new();
    for (name, src) in &all {
        let id = &name[..5];
        let mut files = library();
        files.push((name.clone(), src.clone()));
        let diags = check_sources(&files)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d)
            .unwrap_or_default();

        // Its DECLARED invariant, not merely "some error". Asking only whether
        // the old text is red would be satisfied by a syntax error, and an
        // earlier version of this test was: the first C0 baseline predated the
        // corpus being well-formed, so R-025's old text failed to PARSE and the
        // test called that historical compatibility.
        let declared = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/rejected")
                .join(name),
        )
        .expect("the current fixture")
        .lines()
        .find_map(|l| l.trim().strip_prefix("// @invariant:"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| panic!("{name} has no @invariant"));

        let caught = diags.iter().any(|d| d.symbol() == declared);
        let excused = EXPECTED_TO_PASS.iter().find(|(f, _, _)| *f == id);

        match (caught, excused) {
            (false, None) => wrong.push(format!(
                "{id}: its C0 text is no longer caught for `{declared}` (it reports \
                 {:?}). The repair recorded in docs/CORPUS.md removed the DEFECT, \
                 not the obstruction — a previously discovered failure has been \
                 erased.",
                diags.iter().map(|d| d.symbol()).collect::<Vec<_>>()
            )),
            (true, Some((_, class, why))) => wrong.push(format!(
                "{id}: listed as {class:?} but its C0 text is caught for \
                 `{declared}` again. Good news; remove the entry. The recorded \
                 reason was: {why}"
            )),
            // A regression or a known gap is NOT an excuse — it is a defect
            // being tracked in the wrong place. Only the two benign
            // classifications may sit in this list quietly.
            (false, Some((_, class, why)))
                if matches!(class, Why::CompilerRegressed | Why::KnownCheckerGap) =>
            {
                wrong.push(format!(
                    "{id}: classified {class:?}, which is a defect rather than an \
                     explanation. A regression must be fixed; a checker gap must \
                     also exist as a slips-through.pw witness. Reason on file: {why}"
                ))
            }
            _ => {}
        }
    }
    eprintln!(
        "  historical compatibility: {}/{} C0 texts still caught",
        all.len() - EXPECTED_TO_PASS.len(),
        all.len()
    );
    for (id, class, _) in EXPECTED_TO_PASS {
        eprintln!("    {id}: {class:?}");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// The history is a record, not a second corpus.
///
/// If a C0 file drifts — reformatted, "fixed", quietly updated — it stops
/// answering the question it exists to answer, and the drift would be
/// invisible because nothing else reads these files.
#[test]
fn the_history_is_not_edited_to_match_the_present() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (name, old) in c0() {
        let current = root.join("rejected").join(&name);
        let current = std::fs::read_to_string(&current).expect("the current fixture");
        assert_ne!(
            old.trim(),
            current.trim(),
            "{name}: the C0 text and the current text are identical, so the \
             history proves nothing. Either the fixture never changed and this \
             file should not exist, or the history has been overwritten."
        );
    }
}

/// Missing imports must not silently become public serializable captures.
#[test]
fn historical_unknown_captures_remain_refused_and_imported_controls_keep_their_rule() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (prefix, expected) in [
        ("R-010", "unserializable_capture"),
        ("R-030", "private_in_resume_manifest"),
    ] {
        let (name, old) = c0()
            .into_iter()
            .find(|(name, _)| name.starts_with(prefix))
            .unwrap();
        for (src, symbol) in [
            (old, "resume_capture_schema_unnameable"),
            (
                std::fs::read_to_string(root.join("rejected").join(&name)).unwrap(),
                expected,
            ),
        ] {
            let mut files = library();
            files.push((name.clone(), src));
            let ds = check_sources(&files)
                .into_iter()
                .find(|(n, _)| n == &name)
                .unwrap()
                .1;
            assert!(
                ds.iter().any(|d| d.symbol() == symbol),
                "{prefix} must report {symbol}: {ds:?}"
            );
        }
    }
}

/// C0's unknown event annotation is not a known incompatible event type.
#[test]
fn historical_event_annotation_is_blocked_not_a_nominal_mismatch() {
    let (name, old) = c0()
        .into_iter()
        .find(|(name, _)| name.starts_with("R-022"))
        .unwrap();
    let mut files = library();
    files.push((name.clone(), old));
    let hirs: Vec<_> = files
        .iter()
        .map(|(_, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{:?}", parsed.errors);
            pw_core::lower::lower_file(src, &parsed.green)
        })
        .collect();
    let refs: Vec<_> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &refs);
    let sig = sigs.by_path("checkout.address_form.on_press").unwrap();
    assert!(sig.params[0].as_ref().unwrap().resolved().is_none());
    // This does not establish comprehensive type-error diagnostics. That E9
    // obligation remains open; it must consume the blocked state above.
    let current = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/rejected")
            .join(&name),
    )
    .unwrap();
    files.last_mut().unwrap().1 = current;
    let ds = check_sources(&files)
        .into_iter()
        .find(|(n, _)| n == &name)
        .unwrap()
        .1;
    assert!(
        ds.iter()
            .any(|d| d.symbol() == "handler_signature_mismatch"),
        "{ds:?}"
    );
}

/// **C8: the value relations' import repairs removed obstructions, not defects.**
///
/// Twenty-five rejected fixtures gained an import or a corrected library type
/// on 2026-09-24 (`docs/CORPUS.md` §C8), because the value relations made a
/// written type that names nothing an error. This suite distinguishes their
/// required rejections: declared-invariant catches, the existing R-019 syntax
/// boundary, and two explicitly classified texts: R-035's missing receiver
/// import, and R-027's query, whose key nothing changes. A classified original
/// is checked by its own control; it is not a declared-invariant catch.
// C8's original R-035 cannot establish a receiver effect: ElementRef was
// never imported. Preserve that refusal rather than inferring a member from
// a program-wide spelling. The import-only and typed controls below retain
// the illegal phase effect as a separate, causal obligation.
const C8_CLASSIFIED: &[(&str, Why, &str)] = &[
    (
        "R-035-layout-write-during-measure-phase.pw",
        Why::FixtureDidNotExpressIt,
        "C8's ElementRef receiver has no visible type. The original stays refused \
         for its exact unresolved-receiver diagnostics; adding only its import \
         restores wrong_frame_phase at the setter, and the current typed fixture \
         retains that violation while its pure typed neighbour is clean.",
    ),
    (
        C8_R027,
        Why::FixtureDidNotExpressIt,
        "C8's R-027 is a query alone. Nothing in it changes the key its read is \
         keyed by, so no read of it goes stale: its category, a key that changes \
         without a stale-work policy (charter §15.6, test 7), needs a key that \
         changes. A key changes while its read runs only where a page's signal \
         keys the query (ADR-0152), and ADR-0403 made PW0325, the rule \
         it declared, PW5309's old number. The current fixture is that page, \
         refused for signal_key_stale_work (C30); the old text keeps only the \
         two unresolved types C8's repair imported.",
    ),
];

const C8_R027: &str = "R-027-query-key-changes-without-stale-work-cancellation-policy.pw";

/// C8's R-027 reports the types C8's repair imported, and nothing else: no
/// page changes its query's key (`C8_CLASSIFIED`).
fn c8_r027_changes_no_key(name: &str, src: &str) {
    assert!(
        !src.lines().any(|l| l.trim_start().starts_with("page ")),
        "{name}: a page could change the key"
    );
    let symbols: Vec<&'static str> = check_sources(&c8_program(name, src))
        .into_iter()
        .find(|(n, _)| n == name)
        .expect("C8's R-027 was actually checked")
        .1
        .iter()
        .map(|d| d.symbol())
        .collect();
    assert_eq!(symbols, ["unresolved_type", "unresolved_type"], "{name}");
}

fn c8_r035_diagnostics(name: &str, src: &str) -> Vec<pw_core::diagnostics::Diagnostic> {
    let judged = check_sources(&c8_program(name, src));
    for (other, diagnostics) in &judged {
        if other != name {
            assert!(
                diagnostics.is_empty(),
                "the C8 R-035 control's library must be clean: {other}: {diagnostics:?}"
            );
        }
    }
    judged
        .into_iter()
        .find(|(file, _)| file == name)
        .expect("the C8 R-035 control was actually checked")
        .1
}

fn c8_r035_keeps_its_refusal_and_typed_phase_controls(name: &str, src: &str) {
    use pw_core::diagnostics::Severity;

    let original = c8_r035_diagnostics(name, src);
    assert!(original.iter().all(|d| d.severity == Severity::Error));
    let observed: Vec<_> = original
        .iter()
        .map(|d| {
            (
                d.code,
                d.symbol(),
                d.primary_span.clone(),
                &src[d.primary_span.clone()],
                d.message.as_str(),
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            (
                "PW0026",
                "unresolved_type",
                528..551,
                "items: List<ElementRef>",
                "parameter `items` names `List<ElementRef>`: `ElementRef` is not a type visible here",
            ),
            (
                "PW0408",
                "member_receiver_untyped",
                736..744,
                "it.style",
                "the type of `it` is not known here, so what `.style` reads cannot be checked",
            ),
            (
                "PW0408",
                "member_receiver_untyped",
                736..756,
                "it.style.set_padding",
                "the type of `it.style` is not known here, so what `.set_padding` reads cannot be checked",
            ),
            (
                "PW0408",
                "member_receiver_untyped",
                779..787,
                "it.width",
                "the type of `it` is not known here, so what `.width` reads cannot be checked",
            ),
            (
                "PW0605",
                "argument_type",
                857..881,
                "widths |> List.maximum()",
                "argument 2 of `style.set_width` is declared `Float` and this is `Option<Float>`",
            ),
        ],
        "C8 R-035 is classified only while these exact original refusals hold"
    );

    assert_eq!(src.matches("import style\n").count(), 1);
    assert!(!src.contains("import browser"));
    let imported = src.replacen(
        "import style\n",
        "import style\nimport browser.{ ElementRef }\n",
        1,
    );
    let import_diagnostics = c8_r035_diagnostics(name, &imported);
    assert_eq!(
        import_diagnostics
            .iter()
            .map(|d| (d.code, d.symbol()))
            .collect::<Vec<_>>(),
        vec![
            ("PW0402", "wrong_frame_phase"),
            ("PW0610", "unknown_member"),
            ("PW0605", "argument_type")
        ],
        "adding only ElementRef's import restores the phase finding beside the old width/maximum defects"
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let current = std::fs::read_to_string(root.join("examples/rejected").join(name))
        .expect("the current typed R-035 fixture");
    let current_diagnostics = c8_r035_diagnostics(name, &current);
    assert_eq!(current_diagnostics.len(), 1, "{current_diagnostics:?}");
    for (text, diagnostic) in [
        (imported.as_str(), &import_diagnostics[0]),
        (current.as_str(), &current_diagnostics[0]),
    ] {
        assert_eq!(diagnostic.code, "PW0402");
        assert_eq!(diagnostic.symbol(), "wrong_frame_phase");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.primary_span, 766..792);
        assert_eq!(
            &text[diagnostic.primary_span.clone()],
            "it.style.set_padding(4.px)"
        );
        assert_eq!(
            diagnostic.message,
            "`style.mutate<LayoutAffect>` happens in the `measure` phase, which may not do it"
        );
    }

    let setter = "                it.style.set_padding(4.px)\n";
    assert_eq!(current.matches(setter).count(), 1);
    let pure = current.replacen(setter, "", 1);
    assert!(
        c8_r035_diagnostics(name, &pure).is_empty(),
        "removing only the illegal setter makes the typed neighbour clean"
    );
}

/// **The C8 texts that no longer parse**, each refused for that instead.
///
/// `pw check` keeps a file that does not parse out of the program, and since
/// ADR-0200 so does `check_sources`: its invariant is not checked. R-019's C8
/// text writes a comment as `//` in markup, which is text since ADR-0167, so
/// the `<ul>` it mentions opens an element that never closes. Until ADR-0200
/// this suite checked what recovery made of it.
const NO_LONGER_PARSES: &[&str] = &["R-019-invalid-HTML-nesting.pw"];

#[test]
fn the_c8_text_of_every_repaired_rejected_fixture_is_still_caught() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/history/C8");
    let mut old: Vec<(String, String)> = std::fs::read_dir(&root)
        .expect("examples/history/C8")
        .map(|e| e.expect("entry").path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "pw")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("R-"))
        })
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            )
        })
        .collect();
    old.sort();
    // A floor, so an emptied directory cannot pass vacuously.
    assert_eq!(
        old.len(),
        25,
        "docs/CORPUS.md §C8 records 25 repaired rejected fixtures"
    );

    let mut wrong = Vec::new();
    let mut classified = Vec::new();
    let mut invariant_caught = 0;
    let mut syntax_refused = 0;
    for (name, src) in &old {
        let declared = src
            .lines()
            .find_map(|l| l.trim().strip_prefix("// @invariant:"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| panic!("{name} has no @invariant"));
        if let Some((_, class, why)) = C8_CLASSIFIED
            .iter()
            .find(|(file, _, _)| *file == name.as_str())
        {
            assert_eq!(*class, Why::FixtureDidNotExpressIt);
            if name == C8_R027 {
                assert_eq!(declared, "stale_key_policy");
                c8_r027_changes_no_key(name, src);
            } else {
                assert_eq!(declared, "wrong_frame_phase");
                c8_r035_keeps_its_refusal_and_typed_phase_controls(name, src);
            }
            classified.push((name.as_str(), *class, *why));
            continue;
        }
        // The old text as the rejected-corpus harness assembles a fixture: the
        // CURRENT library (its own C8 repairs included — the question is
        // whether the fixture's defect survived its obstruction being removed)
        // plus only the accepted modules the fixture imports, since several
        // rejected fixtures reuse an accepted module's name.
        let files = c8_program(name, src);
        let symbols: Vec<&'static str> = check_sources(&files)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.iter().map(|d| d.symbol()).collect())
            .unwrap_or_default();
        if NO_LONGER_PARSES.contains(&name.as_str()) {
            assert!(
                !pw_syntax::parse_tree(src).ok() && symbols.contains(&"unclosed_block"),
                "{name}: listed as no longer parsing, and it reports {symbols:?}"
            );
            syntax_refused += 1;
            continue;
        }
        // Declaration-header rules run inside `check_sources` since ADR-0090;
        // R-014's and R-015's are such rules (R-027's was, until
        // ADR-0403).
        if !symbols.contains(&declared.as_str()) {
            wrong.push(format!(
                "{name}: its pre-C8 text is no longer caught for `{declared}` \
                 (it reports {symbols:?}) — the C8 repair removed the defect"
            ));
        } else {
            invariant_caught += 1;
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(
        C8_CLASSIFIED.len(),
        2,
        "two explicitly classified C8 fixtures"
    );
    assert_eq!(classified.len(), C8_CLASSIFIED.len());
    assert_eq!(
        (invariant_caught, syntax_refused, classified.len()),
        (22, 1, 2)
    );
    assert_eq!(
        invariant_caught + syntax_refused + classified.len(),
        old.len()
    );
    eprintln!(
        "  historical compatibility: {invariant_caught}/{} C8 declared-invariant catches; {syntax_refused} syntax refusal; {} classified",
        old.len(),
        classified.len()
    );
    for (name, class, why) in classified {
        eprintln!("    {name}: {class:?}: {why}");
    }
}

/// **C9: a page states its title** (ADR-0183, `docs/CORPUS.md` §C9).
///
/// R-023's page is served at a route, so it gained a `<title>`: PW5029 now
/// refuses a page served at a route that states none. Its old text must still
/// be caught for its declared invariant, `dead_internal_link`, now beside
/// `title_missing` for the title the repair added; and the routed pages of
/// the link rule's other fixtures, refused for the title alone.
#[test]
fn the_c9_texts_are_still_caught_for_their_invariant_and_refused_for_the_title() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/history/C9");
    let read = |path: &str| std::fs::read_to_string(root.join(path)).expect(path);
    let symbols = |name: &str, src: &str| -> Vec<&'static str> {
        check_sources(&c8_program(name, src))
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.iter().map(|d| d.symbol()).collect())
            .unwrap_or_default()
    };
    let name = "R-023-dead-internal-route.pw";
    let old = symbols(name, &read(name));
    assert!(
        old.contains(&"dead_internal_link") && old.contains(&"title_missing"),
        "{name}: its C9 text reports {old:?}"
    );
    // The link rule's GENERAL and NEIGHBOUR witnesses, and its rule fixture:
    // the same programs, each refused for the title it did not state.
    for (path, expected) in [
        (
            "generality/dead_internal_link/caught.pw",
            &["dead_internal_link", "title_missing"][..],
        ),
        (
            "generality/dead_internal_link/neighbour.pw",
            &["title_missing"][..],
        ),
        (
            "rules/routes/link-to-declared-route.pw",
            &["title_missing"][..],
        ),
    ] {
        let name = path.rsplit('/').next().expect("a file name");
        let mut got = symbols(name, &read(path));
        got.sort();
        got.dedup();
        assert_eq!(got, expected, "{path}");
    }
}

/// **C10: ids and ARIA, checked at build** (ADR-0185, `docs/CORPUS.md`
/// §C10).
///
/// Two `control_without_label` witnesses named an id nothing had. Since
/// ADR-0185 a reference to nothing is reported as itself (PW5032), and a
/// field it leaves unnamed is not reported again. Their old texts:
/// - `label-for-another.pw`'s label named a missing id, beside a field
///   nothing names: both are reported, each where it is;
/// - `labelledby-nothing.pw`'s field named itself by a missing id: the
///   reference is reported, and the witness moved to
///   `reference_names_nothing`, where it is that invariant's.
#[test]
fn the_c10_texts_are_reported_where_their_defects_are() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/history/C10/generality/control_without_label");
    let symbols = |name: &str| -> Vec<&'static str> {
        let src = std::fs::read_to_string(root.join(name)).expect(name);
        let mut got: Vec<&'static str> = check_sources(&c8_program(name, &src))
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.iter().map(|d| d.symbol()).collect())
            .unwrap_or_default();
        got.sort();
        got.dedup();
        got
    };
    assert_eq!(
        symbols("label-for-another.pw"),
        ["control_without_label", "reference_names_nothing"]
    );
    assert_eq!(
        symbols("labelledby-nothing.pw"),
        ["reference_names_nothing"]
    );
}

/// **C16: a value the template computes** (ADR-0226, `docs/CORPUS.md`
/// §C16). Two witnesses' and a rule fixture's old texts:
/// - `forbidden_effect`'s valid neighbour read the wall clock in its
///   template, where a value is computed again when what it reads changes,
///   and performs nothing since corpus C16. Its old text is that defect, and
///   no other: the page may read the clock, as it does now, in its body.
/// - `task_detached`'s nested witness called `List.each`, which no module
///   declares. The qualified call check took an import's name for a local
///   until ADR-0226, so it checked clean. Its old text is that, beside the
///   detached task it is about.
/// - `decoded-not-cast.pw`, the cast rule's clean control, gave its decoder
///   to `decode.run`, which `decode` does not declare either.
#[test]
fn the_c16_texts_are_reported_where_their_defects_are() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/history/C16");
    let symbols = |path: &str| -> Vec<&'static str> {
        let name = path.rsplit('/').next().expect("a file name");
        let src = std::fs::read_to_string(root.join(path)).expect(path);
        let mut got: Vec<&'static str> = check_sources(&c8_program(name, &src))
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.iter().map(|d| d.symbol()).collect())
            .unwrap_or_default();
        got.sort();
        got.dedup();
        got
    };
    assert_eq!(
        symbols("generality/forbidden_effect/page-may-read-clock.pw"),
        ["derived_not_pure"]
    );
    assert_eq!(
        symbols("generality/task_detached/nested.pw"),
        ["task_detached", "unresolved_name"]
    );
    assert_eq!(
        symbols("rules/annotations/decoded-not-cast.pw"),
        ["unresolved_name"]
    );
}

/// **C30: a key that changes is asked where it changes** (ADR-0403,
/// `docs/CORPUS.md` §C30).
///
/// R-027 was a query alone that declares a `concurrency` and no
/// `on_key_change`, refused by PW0325; `stale_key_policy`'s GENERAL witness
/// was the same query with another `concurrency`. Nothing in either changes
/// the query's key, so no read of it goes stale, and since PW0325 became
/// PW5309's old number both check clean: this is the change the ADR made, and
/// what it keeps. The class is refused where a key changes: R-027 is now a
/// page whose signal keys the query, and so is `signal_key_stale_work`'s
/// GENERAL witness.
#[test]
fn the_c30_texts_change_no_key_and_check_clean() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/history/C30");
    for path in [C8_R027, "generality/stale_key_policy/caught.pw"] {
        let name = path.rsplit('/').next().expect("a file name");
        let src = std::fs::read_to_string(root.join(path)).expect(path);
        let declares = |clause: &str| src.lines().any(|l| l.trim_start().starts_with(clause));
        assert!(
            declares("concurrency") && !declares("on_key_change") && !declares("page "),
            "{path}: no longer the shape PW0325 refused"
        );
        let found: Vec<(&'static str, String)> = check_sources(&c8_program(name, &src))
            .into_iter()
            .find(|(n, _)| n == name)
            .expect("the C30 text was actually checked")
            .1
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect();
        assert!(found.is_empty(), "{path}: {found:?}");
    }
}

/// A C8 fixture's program: the library, the accepted modules it imports, and
/// the fixture. The same assembly `checking_source.rs` uses for the current
/// rejected corpus, so the old and new texts are asked one question.
fn c8_program(name: &str, src: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let imported: Vec<String> = src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("import "))
        .map(|m| m.split(['.', ' ']).next().unwrap_or("").to_string())
        .collect();
    let mut out: Vec<(String, String)> = library()
        .into_iter()
        .filter(|(n, s)| {
            // `library()` here includes the accepted corpus; keep only the
            // modules this fixture imports from it.
            let module = s
                .lines()
                .find_map(|l| l.trim().strip_prefix("module "))
                .map(str::trim);
            let accepted = root.join("examples/accepted").join(n).exists();
            !accepted
                || module.is_some_and(|m| {
                    imported
                        .iter()
                        .any(|i| i == m.split('.').next().unwrap_or(m))
                })
        })
        .filter(|(n, _)| n != name)
        .collect();
    out.push((name.to_string(), src.to_string()));
    out
}
