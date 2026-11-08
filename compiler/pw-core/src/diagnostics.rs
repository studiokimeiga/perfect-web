//! The one diagnostic type every semantic detector produces.
//!
//! Architect ruling: **no semantic rule may live only in the CLI**, and all
//! detectors must produce the same structured value so the same checker can be
//! driven by the CLI, a language server, a test harness, a build system, a
//! browser playground, an AI coding loop, and the semantic PR analyser.
//! Rendering belongs to callers.
//!
//! The other ruling this encodes: **one public code per violated invariant, not
//! per detector.** A developer should learn *"a resource cannot outlive the
//! scope that owns it"*, not which implementation pass happened to notice. The
//! detector and the sub-reason are metadata.

use std::ops::Range;

pub type Span = Range<usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Which analysis found the defect. Metadata; never part of the public code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detector {
    /// Read directly off a declaration header.
    DeclarationRule,
    /// `pw-core`'s scope graph, from handle flow.
    ScopeGraph,
    /// The exhaustiveness checker's pattern matrix.
    PatternMatrix,
    /// Declared-versus-actual component imports.
    CapabilityAudit,
    /// The type-directed boundary decoder.
    AbiDecoder,
    /// E6's resource dependency graph, over the whole program.
    ResourceGraph,
    /// **A call compared against its callee's declared signature.**
    ///
    /// Added 2026-08-20 with `PW0604`, the first check of an ordinary call
    /// site. Named rather than borrowed: it is neither a declaration-header
    /// rule (it needs the call) nor the pattern matrix (it is not a match), and
    /// a diagnostic that reports the wrong detector describes a mechanism that
    /// did not decide it.
    Signature,
    /// **The parser.** A syntax error, reported where a program is checked
    /// by a caller that holds only its units: `backend::lower::Checked::of`.
    /// Added 2026-09-25, when a query body the parser had reported as an
    /// unknown policy was compiled anyway, from the tree that survived.
    Parser,
    /// Exact executable backend support validation.
    Backend,
}

impl Detector {
    pub fn name(self) -> &'static str {
        match self {
            Detector::Signature => "signature",
            Detector::Parser => "parser",
            Detector::Backend => "backend",
            Detector::DeclarationRule => "declaration_rule",
            Detector::ScopeGraph => "scope_graph",
            Detector::PatternMatrix => "pattern_matrix",
            Detector::CapabilityAudit => "capability_audit",
            Detector::AbiDecoder => "abi_decoder",
            Detector::ResourceGraph => "resource_graph",
        }
    }
}

/// A span other than the primary one that the reader needs in order to
/// understand the defect — charter §16.3's "where the value originated".
///
/// **In the diagnostic's own file, or in another's** (ADR-0321): a layout
/// another module declares, a page in another file that calls a command.
/// Until 2026-10-12 every span was read as the diagnostic's own file's: a
/// note on a layout in another file labelled the page's own text, and past
/// the page's end stopped `pw check` (W8's finding, PW5046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    pub span: Span,
    pub label: String,
    /// **The file the span is in**, as it was given to the checker, where
    /// it is not the diagnostic's own; `None` where it is.
    pub file: Option<String>,
    /// The unit a detector found the span in, where it may be another than
    /// the diagnostic's own: the checker reads it as `file`.
    pub(crate) unit: Option<usize>,
}

impl Related {
    /// A note in the diagnostic's own file.
    pub fn new(span: Span, label: impl Into<String>) -> Related {
        Related {
            span,
            label: label.into(),
            file: None,
            unit: None,
        }
    }

    /// A note in the file of unit `unit`, which may be the diagnostic's own
    /// or another's: the checker says which (`resolve_files`).
    pub(crate) fn in_unit(unit: usize, span: Span, label: impl Into<String>) -> Related {
        Related {
            unit: Some(unit),
            ..Related::new(span, label)
        }
    }
}

/// **Each related note's file, by the units a program was checked as**
/// (ADR-0321): a note found in another unit than `own` names that unit's
/// path; one in `own` names none.
pub(crate) fn resolve_files(found: &mut [Diagnostic], own: usize, paths: &[&str]) {
    for d in found {
        for r in &mut d.related {
            if let Some(unit) = r.unit {
                r.file = (unit != own).then(|| paths[unit].to_string());
            }
        }
    }
}

/// A concrete legal alternative. Charter §16.3 requires at least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair {
    pub description: String,
    /// A machine-applicable edit, when one exists. `None` means the repair is
    /// advisory — which is honest, and better than a fix-it that guesses.
    pub replacement: Option<(Span, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable, public, identifies the **invariant**. For users and docs.
    pub code: &'static str,
    /// One sentence naming the invariant, in the developer's vocabulary.
    pub invariant: &'static str,
    /// Machine-readable sub-reason within the invariant. Distinguishes repairs
    /// without splitting the code.
    pub reason: &'static str,
    pub detector: Detector,
    pub severity: Severity,
    pub message: String,
    pub primary_span: Span,
    pub related: Vec<Related>,
    /// Why the rule exists, when that is not obvious from the message.
    pub explanation: Option<String>,
    pub repairs: Vec<Repair>,
}

impl Diagnostic {
    /// The **semantic identity** of what this reports.
    ///
    /// Read from the registry rather than stored on the diagnostic, so it
    /// cannot disagree with the registry — there is one source and no copy to
    /// drift. `PW0323` acquired three different meanings precisely because the
    /// number was typed at a construction site, the prose lived in the
    /// registry, and nothing compared them.
    pub fn symbol(&self) -> &'static str {
        crate::codes::lookup(self.code).map_or(UNREGISTERED, |c| c.symbol)
    }

    /// The registry revision of the invariant this reports. `0` means the code
    /// is not registered at all.
    pub fn revision(&self) -> u16 {
        crate::codes::lookup(self.code).map_or(0, |c| c.revision)
    }

    pub fn error(
        code: &'static str,
        invariant: &'static str,
        detector: Detector,
        message: impl Into<String>,
        primary_span: Span,
    ) -> Self {
        Self {
            code,
            invariant,
            reason: "",
            detector,
            severity: Severity::Error,
            message: message.into(),
            primary_span,
            related: Vec::new(),
            explanation: None,
            repairs: Vec::new(),
        }
    }

    pub fn warning(
        code: &'static str,
        invariant: &'static str,
        detector: Detector,
        message: impl Into<String>,
        primary_span: Span,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, invariant, detector, message, primary_span)
        }
    }

    pub fn reason(mut self, r: &'static str) -> Self {
        self.reason = r;
        self
    }

    pub fn related(mut self, span: Span, label: impl Into<String>) -> Self {
        self.related.push(Related::new(span, label.into()));
        self
    }

    pub fn explain(mut self, e: impl Into<String>) -> Self {
        self.explanation = Some(e.into());
        self
    }

    pub fn repair(mut self, description: impl Into<String>) -> Self {
        self.repairs.push(Repair {
            description: description.into(),
            replacement: None,
        });
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// The old numbers: each number the corpus wrote for an invariant before one
/// invariant got one code, with the code it is now. A reference, never a
/// translation.
///
/// Architect migration rule: if the corpus already declares one code as
/// canonical, preserve it; mark the other a deprecated alias; have every
/// detector emit the canonical code; remove the alias before diagnostic-code
/// stability is publicly promised.
///
/// The corpus migrated on 2026-10-15 (track evidence-integrity): each
/// rejected fixture's `@rule` is the code `pw check` reports, which never
/// printed these numbers. The same day the integrator ruled (track
/// evidence-integrity-2, ruling 1) that nothing translates through this
/// list where a code is compared: a test, `corpus-check` or a gate reads a
/// code as written, so an old number in a header is an unregistered code.
/// The list stays only where it is printed: the guide's reference of codes
/// (`scripts/guide_diagnostics.py`, track guide, reads this constant by its
/// name and form, and its test `guide.rs` counts a row for each), and a
/// refused header's message (`checking_source.rs`). The
/// registry's tests hold it: no old number is a registered or retired code,
/// and each names a registered one (`codes.rs`); nothing else reads it
/// (`no_comparison_translates_an_old_number`).
pub const DEPRECATED_ALIASES: &[(&str, &str)] = &[
    // (alias as the corpus wrote it until 2026-10-15, canonical invariant code)
    ("PW0326", "PW2004"), // a resource cannot outlive the scope that owns it
    ("PW0311", "PW2002"),
    // Not consumed on a path, and consumed somewhere the scope cannot see.
    // One invariant from two sides: exactly once, here.
    ("PW0309", "PW2005"),
    ("PW0310", "PW2005"), // an ordinary task cannot be detached from its scope
    // Both are "a body requires a capability its world cannot grant" — one for
    // a database in the browser, one for a secret at the edge. The corpus gave
    // them separate codes before the placement algebra existed; one invariant
    // gets one code, not one per instance.
    ("PW0301", "PW5002"),
    ("PW0324", "PW5002"),
    // The corpus's number for "the placement you named cannot grant this".
    ("PW0323", "PW5005"),
    ("PW0302", "PW5003"),
    // The resume manifest: one about what a value IS, one about who may read it.
    ("PW0308", "PW5008"),
    // `PW3011` was registered as "resumption manifest — private data crossing
    // into it". That is `private_in_resume_manifest`, which `PW5007` already
    // enforces — NOT the capture-schema check, and not general code-version
    // incompatibility.
    //
    // It was briefly aliased to `PW5016`, which would have made the alias imply
    // coverage that does not exist. Corrected on the architect's audit: an
    // alias must preserve the meaning, and `PW3011` meant this one.
    ("PW3011", "PW5007"),
    ("PW0328", "PW5007"),
    // R-006: a secret reaching a sink declared `Public`.
    ("PW0304", "PW5006"), // a secret cannot be rendered to the browser
    ("PW0303", "PW5004"), // a shared cache key must carry every partition
    // R-004 declared PW0100, which `rules.rs` also used for this invariant —
    // the charter §16.3 worked example. The scope graph and the label algebra
    // reach the same conclusion by different routes, so one code, not two.
    ("PW0100", "PW5001"),
    // The layout family. `PW3001` is the corpus's code for an effect reaching a
    // context that may not have it — which is one invariant, not one per
    // effect family.
    ("PW3001", "PW0401"),
    ("PW3002", "PW0401"),
    // The frame-phase family. Each names "work happening in a phase that does
    // not permit it" — a write during measure, a measure after paint. One
    // invariant, stated once; which phase and which effect are metadata.
    ("PW3004", "PW0402"),
    ("PW3008", "PW0402"),
    // R-034 measures after a layout-affecting write in one transaction. The
    // measure block is measuring, which is what it is for — but a measure that
    // an earlier write has already invalidated is not the measure phase at all,
    // so it is the same invariant seen from the ordering side.
    ("PW3003", "PW0402"),
    // A compositor animation and a painter both forbid work regardless of what
    // the declaration admits to, which is what PW0401 says.
    ("PW3006", "PW0401"),
    ("PW3007", "PW0401"),
    // These two are NOT effect-row violations, so they are not aliased onto
    // one. A cycle is a relation between an observation and a write; a false
    // independence is a declared assertion the code contradicts.
    ("PW3005", "PW0403"),
    ("PW3009", "PW0404"),
    // A view that fetches, a build-time page that reads the wall clock, a
    // shared fragment that does. Each is "an effect is not permitted where this
    // runs, whatever the row says" — PW0401 — differing only in which context
    // and which effect, which the diagnostic carries as metadata.
    ("PW0300", "PW0401"),
    ("PW0314", "PW0401"),
    ("PW0315", "PW0401"),
    // Types. Each is a distinct question about what a value IS: one may be
    // absent, one has not been decoded, one does not accept what it is given.
    ("PW0306", "PW0600"),
    ("PW0307", "PW0601"),
    ("PW0320", "PW0602"),
    ("PW0316", "PW5011"), // a list over a mutable collection needs a stable key
    ("PW0321", "PW5009"),
    ("PW0317", "PW5012"), // an element may only contain the children HTML permits
    ("PW0318", "PW5013"), // interactive behaviour belongs on an interactive element
    ("PW0319", "PW5014"), // a form control must have something that names it
    // Both are "an unsafe escape hatch must record why it is necessary": one
    // written with no justification at all, one with an unaudited use.
    ("PW0329", "PW5010"),
    // R-024 has no audit record at all, which is the most incomplete one.
    ("PW0322", "PW5010"),
    ("PW3010", "PW5010"),
    // "A query whose key changes says what its stale work does", asked twice
    // until ADR-0403: PW0325 of a keyed query alone that declared a
    // `concurrency`, PW5309 of a query a page's signal keys. A key changes
    // while its read is in flight only in the second, so the second is the
    // one code. Unlike the numbers above, `pw check` printed this one.
    ("PW0325", "PW5309"),
];

/// The unregistered marker. A diagnostic carrying it named a code the registry
/// does not have, which `every_emitted_diagnostic_carries_a_registered_symbol`
/// rejects.
pub const UNREGISTERED: &str = "<unregistered>";

#[cfg(test)]
mod tests {
    use super::*;

    /// Track evidence-integrity-2, the integrator's ruling 1: no comparison of
    /// a code translates an old number. Until 2026-10-15 a function of this
    /// module did, for the corpus's tests and `codes::rule_status`, so a
    /// fixture whose header named a number `pw check` never prints passed.
    /// The old numbers are read only where they are printed: this module, the
    /// registry's own tests of the list (`codes.rs`), and a refused header's
    /// message (`checking_source.rs`); each Rust source of the workspace that
    /// names the list is one of those, and none names that function.
    #[test]
    fn no_comparison_translates_an_old_number() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let allowed = [
            "compiler/pw-core/src/diagnostics.rs",
            "compiler/pw-core/src/codes.rs",
            "compiler/pw-core/tests/checking_source.rs",
            // Track guide's test of the reference page, which counts a row for
            // each old number (on its branch until it lands).
            "compiler/pw-cli/tests/guide.rs",
        ];
        let mut readers = Vec::new();
        let mut translators = Vec::new();
        let mut seen = 0;
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if path.is_dir() {
                    if !matches!(
                        name.as_ref(),
                        "target" | "node_modules" | ".git" | ".toolchain"
                    ) {
                        stack.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|x| x != "rs") {
                    continue;
                }
                seen += 1;
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if text.contains(concat!("canonical", "_code")) {
                    translators.push(rel.clone());
                }
                if text.contains(concat!("DEPRECATED", "_ALIASES"))
                    && !allowed.contains(&rel.as_str())
                {
                    readers.push(rel);
                }
            }
        }
        assert!(
            seen > 100,
            "read {seen} Rust sources: the walk found nothing"
        );
        assert!(
            translators.is_empty(),
            "an old number is translated where a code is compared: {translators:?}"
        );
        assert!(
            readers.is_empty(),
            "the old numbers are read outside where they are printed: {readers:?}"
        );
    }

    #[test]
    fn syntax_codes_and_semantic_codes_do_not_share_a_range() {
        // They did: `PW0100` was the parser's "expected X, found Y" AND the
        // shared-cache invariant, so a corpus file declaring it could match
        // either. Syntax now lives in PW00xx and semantics at PW01xx and above.
        for (alias, canonical) in DEPRECATED_ALIASES {
            assert!(
                !alias.starts_with("PW000") && !alias.starts_with("PW009"),
                "{alias} is in the syntax range"
            );
            assert!(
                !canonical.starts_with("PW000"),
                "{canonical} is in the syntax range"
            );
        }
    }

    #[test]
    fn one_invariant_can_be_reported_by_two_detectors_under_one_code() {
        // The architect's worked example: the developer learns one invariant;
        // the detector and reason are metadata.
        let from_declaration = Diagnostic::error(
            "PW2004",
            "a resource cannot outlive the scope that owns it",
            Detector::DeclarationRule,
            "subscription `Tracking` declares `application` scope",
            0..10,
        )
        .reason("declared_scope_exceeds_owner");

        let from_graph = Diagnostic::error(
            "PW2004",
            "a resource cannot outlive the scope that owns it",
            Detector::ScopeGraph,
            "task handle `load_menu` escapes the component that owns it",
            20..30,
        )
        .reason("handle_escapes_to_ancestor");

        assert_eq!(from_declaration.code, from_graph.code);
        assert_eq!(from_declaration.invariant, from_graph.invariant);
        assert_ne!(from_declaration.detector, from_graph.detector);
        assert_ne!(from_declaration.reason, from_graph.reason);
    }

    #[test]
    fn a_diagnostic_carries_the_charter_16_3_shape() {
        let d = Diagnostic::error(
            "PW0100",
            "a shared cache may contain only Public values",
            Detector::DeclarationRule,
            "msg",
            0..5,
        )
        .related(10..20, "declared `session` here")
        .explain("why")
        .repair("use `cache private`");
        assert!(!d.related.is_empty(), "origin span");
        assert!(d.explanation.is_some());
        assert!(!d.repairs.is_empty(), "at least one legal alternative");
    }
}
