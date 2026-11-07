//! corpus-check — validates the accepted/rejected specification corpus.
//!
//! The corpus (charter §16) is the executable specification: "Add examples
//! before features." Until Milestone 2 there is no `pw` compiler, so nothing can
//! actually *compile* these files. What can be checked today, and what this tool
//! checks, is that the corpus is well-formed and that it covers every category
//! the charter enumerates.
//!
//! From Milestone 2 onward this tool gains a second phase: run `pw check` over
//! each file and assert that accepted files produce no diagnostics and rejected
//! files produce exactly the declared `@rule` code at the declared span.
//!
//! Usage: corpus-check <examples-dir>
//! Exit:  0 = corpus valid and complete, 1 = problems found.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Charter §16.1. Every one of these must be covered by >=1 accepted example.
const ACCEPTED_CATEGORIES: &[&str] = &[
    "pure calculation",
    "exhaustive order-state rendering",
    "public store query",
    "private cart query",
    "idempotent command",
    "scoped WebSocket subscription",
    "map widget resource with cleanup",
    "streamed public recommendations",
    "edge materialized menu",
    "origin-only secret operation",
    "offline draft with explicit conflict policy",
    "decoder handling malformed JSON",
    "build-time deterministic page",
    "content-addressed resumable handler",
    // Charter v2 §7.5A layout/DOM phase families. Added because v2 introduced
    // eight effect families without extending §16's category lists, which
    // contradicted §16's own "add examples before features" rule.
    "phase-scheduled measure then mutate",
    "multiple measures share one layout snapshot",
    "batched layout-affecting mutations",
    "resize observation updates derived state",
    "intersection observation controls resource lifetime",
    "compositor-only animation",
    "custom paint with declared inputs",
    "post-paint non-layout work",
    "semantically independent contained subtree",
    "audited imperative widget escape",
    // ADR-0183, corpus C9: a page states its title. A user-facing invariant
    // the compiler gained, which by the architect's ruling of 2026-08-06
    // (below) has its canonical accepted and rejected specification.
    "page states its title",
    // ADR-0185, corpus C10: charter §8.2's ids and ARIA relations.
    "ids and the ARIA that names them",
    // ADR-0186, corpus C11: what a page says of itself unshown.
    "page states its description",
    // ADR-0189, corpus C12: a `<link>` where HTML allows it.
    "links the body allows",
    // ADR-0194, corpus C13: a type that contains itself.
    "a type that contains itself through a list",
    // ADR-0197, corpus C14: a pattern's name by its capital.
    "cases and bindings told apart by their capital",
    // ADR-0221, corpus C15: a form control's value where HTML reads it.
    "a form control's value where HTML reads it",
    // ADR-0226, corpus C16: a value the template computes.
    "a value the template computes",
    // ADR-0311, corpus C19: a page states its canonical address.
    "page states its canonical address",
    // ADR-0318 (track `store-accounts`), corpus C20: a `user` query, imported.
    "a user's query read on a user's page of another module",
    // ADR-0195's ruling 13, corpus C22: a page loads from its own origin.
    "a page loads from its own origin",
    // The scheduled-commands track, corpus C23: a run the clock makes.
    "a command schedules one the clock invokes",
    // 4c (track `bounded-checker`), corpus C25: a list's reads, one set read
    // before its loop.
    "a list read together before its loop",
    // The each-head-effects track, corpus C26: R-080's neighbour.
    "a loop over a query's value",
    // Track `tables` (ADR-0338, D1): an app's data, declared, and the
    // controls of PW0361, PW0362, PW0364 and PW0366.
    "a table read and written through the functions its declaration gives",
    "a reference between tables one source holds",
    "a reference across sources by a field that may name no row",
    "a table's change from its released schema, stated",
    // Track `device-store` (ADR-0366), corpus C28: a value the device keeps,
    // read and written by a page and never sent.
    "a value the reader's device keeps",
    // ADR-0351-generic-effects, assembled corpus C29: generic/member reads
    // retain their types and effects, and a receiver has a known type.
    "generic results preserve pure member types",
];

/// Charter §16.2: "Create at least one minimal file for each".
const REJECTED_CATEGORIES: &[&str] = &[
    "network request inside view",
    "database access in browser",
    "secret serialized to browser",
    "private cart in shared cache",
    "cross-tenant cache key missing tenant",
    "public log of secret value",
    "unhandled ADT variant",
    "ambient null assumption",
    "unchecked external cast",
    "nonserializable handler capture",
    "open transaction not committed or rolled back",
    "affine resource leaked",
    "detached ordinary task",
    "retry of non-idempotent command",
    "unbounded retry",
    "nondeterministic static render",
    "wall-clock read in shared deterministic materialization",
    "unkeyed mutable list",
    "invalid HTML nesting",
    "button behavior implemented on inaccessible div",
    "label missing from form control",
    "form handler type mismatch",
    "dead internal route",
    "raw unsafe HTML without capability",
    "browser-only API on origin",
    "server secret referenced by edge artifact",
    "query key changes without stale-work cancellation policy",
    "subscription outlives component scope",
    "optimistic state with no rollback path",
    "private data in resume manifest",
    "unsafe escape hatch without justification",
    // Charter v2 §7.5A. Note the three-way discipline the architect required:
    // every effect family needs one accepted use, one DIRECT rejected use, and
    // one rejected use HIDDEN behind a helper or generic callback. The last is
    // the one that matters — a checker that only catches direct calls is worth
    // approximately nothing.
    "synchronous geometry read in ordinary code",
    "geometry read during render",
    "layout read after layout-affecting write",
    "layout write during measure phase",
    "measure helper hides forbidden geometry read",
    "generic callback smuggles layout measure",
    "resize observer layout feedback cycle",
    "intersection observer unscoped subscription",
    "layout-affecting operation in compositor animation",
    "custom paint mutates document",
    "post-paint same-frame layout measure",
    "containment with cross-boundary layout dependency",
    "unaudited imperative dom escape",
    // E6, corpus C3. Two invariants the milestone introduced that no existing
    // category names, added on the architect's ruling of 2026-08-06:
    //
    //   > A new user-facing language invariant introduced by E6 should have at
    //   > least one canonical accepted/rejected specification pair. Otherwise
    //   > your dashboard eventually says `generality-tested 29 / 29` while the
    //   > compiler actually contains 31 or 32 semantic invariants.
    //
    // The denominator has to grow, or it stops meaning "all invariants".
    "private dependency of a shared materialization",
    "dependency graph edge to an undeclared target",
    // ADR-0183, corpus C9: PW5029 and PW5030.
    "page served at a route without a title",
    "title outside the top of a page's view",
    // ADR-0185, corpus C10: PW5031, PW5032 and PW5033.
    "duplicate id",
    "ARIA relation names nothing",
    "unknown ARIA attribute or role",
    // ADR-0186, corpus C11: PW5034.
    "metadata outside the top of a page's view",
    // ADR-0189, corpus C12: PW5035.
    "a link the head holds, written in markup",
    // ADR-0194, corpus C13: PW0624.
    "a type no finite value has",
    // ADR-0197, corpus C14: PW0625 and PW0608.
    "a case named in lowercase",
    "a misspelt case in a pattern",
    // ADR-0221, corpus C15: PW5036.
    "a select's value written as an attribute",
    // ADR-0226, corpus C16: PW0334, revision 2.
    "a value the template computes that performs an effect",
    // ADR-0230, corpus C17: PW0609, revision 2 (ruling 0071-a).
    "a template condition that tests a number",
    // ADR-0282, corpus C18: PW5101 and PW5002, three findings of 2026-10-03.
    "a secret a query answers, kept in a shared fragment",
    "a fragment of a session's data, built before any request",
    "a session read through a query of a built page's own",
    // ADR-0311, corpus C19: PW5051.
    "a canonical address written as text",
    // ADR-0318 (track `store-accounts`), corpus C20: PW5007, a `private`
    // query's record the user's; a `user` query's since ADR-0324, corpus
    // C21, which made `private` say only "not importable".
    "a user query's record captured into a public manifest from a view's parameter",
    "a user's query's record captured into a public page's manifest",
    // ADR-0195's ruling 13, corpus C22: PW5052.
    "a load from another origin",
    // The scheduled-commands track, corpus C23: PW6501, PW6502, PW6503.
    "a schedule that names a command the clock does not invoke",
    "a schedule written outside a command",
    "a run scheduled at what is no instant",
    // NEXT 6c's `invoked_by Payments`, corpus C24: PW6401 to PW6405.
    "a command the platform invokes that requires a reader",
    "a payment's command given what is no settled payment",
    "two commands a payment's outcome invokes",
    "a page that presses a command the platform invokes",
    "a command the platform invokes with no key",
    // 4c (track `bounded-checker`), corpus C25: PW0405, a read keyed by its
    // loop's element, in each loop form; and PW0407, a `set` that does not fit
    // its keyed read.
    "a read per element in a combinator's callback",
    "a read per element in a for loop",
    "a read per element in a recursion over data",
    "a set read that answers no result with its key",
    // The each-head-effects track, corpus C26: PW0401 at a loop's head.
    "a database read in a loop's list",
    // Track `tables` (ADR-0338, D1): PW0361, PW0362, PW0364 and
    // PW0366, each beside its accepted control (A-050 to A-053).
    "a table keyed by a field no database keys a row by",
    "an index on a field its row does not have",
    "a table read whole that states no bound",
    "a read a table's declaration does not give",
    "a table's rows read through a binding written by hand",
    "a reference whose field holds no key of its target",
    "a field dropped from a released schema with no migrate",
    "a field renamed from a released schema with no migrate",
    // Track `device-store` (ADR-0366), corpus C28: PW6801 and PW6802, each
    // beside its accepted control (A-054).
    "a value the device keeps sent as a command's argument",
    "a query keyed by a value the device keeps",
    // ADR-0351-generic-effects, assembled corpus C29: generic/member reads
    // retain their types and effects, and a receiver has a known type.
    "a generic result cannot hide a views member effect",
    "a generic result cannot hide a functions member effect",
    "a member read needs a known receiver",
    "a rigid parameter declares no members",
    "a function value receiver keeps its effect",
    "a recursive generic invocation keeps its effect",
    // The owned-rows ADR, corpus C33: PW0370, a table a reader's command
    // writes says whose its rows are (R-098).
    "a table a reader's command writes that says nothing of whose its rows are",
];

/// Milestone 0 gate (charter §14 M0): ">= 10 accepted and 20 rejected".
const GATE_MIN_ACCEPTED: usize = 10;
const GATE_MIN_REJECTED: usize = 20;

#[derive(Debug)]
struct Example {
    fields: BTreeMap<String, Vec<String>>,
}

impl Example {
    fn one(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .and_then(|v| v.first())
            .map(|s| s.as_str())
    }
    fn many(&self, key: &str) -> &[String] {
        self.fields.get(key).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

fn parse_header(text: &str) -> Example {
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("// @") else {
            // The header must be a contiguous block at the top of the file.
            if line.starts_with("//") || line.is_empty() {
                continue;
            }
            break;
        };
        let Some((k, v)) = rest.split_once(':') else {
            continue;
        };
        fields
            .entry(k.trim().to_string())
            .or_default()
            .push(v.trim().to_string());
    }
    Example { fields }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "pw") {
            out.push(p);
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples".to_string());
    let root = Path::new(&root);

    let mut problems: Vec<String> = Vec::new();
    let mut report = String::new();

    for (bucket, categories, min) in [
        ("accepted", ACCEPTED_CATEGORIES, GATE_MIN_ACCEPTED),
        ("rejected", REJECTED_CATEGORIES, GATE_MIN_REJECTED),
    ] {
        let dir = root.join(bucket);
        let mut files = Vec::new();
        if let Err(e) = collect(&dir, &mut files) {
            problems.push(format!("cannot read {}: {e}", dir.display()));
            continue;
        }

        let mut seen_ids: BTreeSet<String> = BTreeSet::new();
        let mut covered: BTreeSet<String> = BTreeSet::new();

        for path in &files {
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    problems.push(format!("{}: cannot read: {e}", path.display()));
                    continue;
                }
            };
            let ex = parse_header(&text);
            let where_ = path
                .strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string();

            // --- required fields, same for both buckets ---
            match ex.one("corpus") {
                Some(c) if c == bucket => {}
                Some(c) => problems.push(format!(
                    "{where_}: @corpus is `{c}` but the file is under {bucket}/"
                )),
                None => problems.push(format!("{where_}: missing `// @corpus: {bucket}`")),
            }
            match ex.one("id") {
                Some(id) => {
                    if !seen_ids.insert(id.to_string()) {
                        problems.push(format!("{where_}: duplicate @id `{id}`"));
                    }
                }
                None => problems.push(format!("{where_}: missing `// @id:`")),
            }
            match ex.one("category") {
                Some(cat) => {
                    if categories.contains(&cat) {
                        covered.insert(cat.to_string());
                    } else {
                        problems.push(format!(
                            "{where_}: @category `{cat}` is not one of the charter §16 categories"
                        ));
                    }
                }
                None => problems.push(format!("{where_}: missing `// @category:`")),
            }
            if ex.one("charter").is_none() {
                problems.push(format!(
                    "{where_}: missing `// @charter:` (which charter section this encodes)"
                ));
            }
            match ex.one("milestone").map(|m| m.parse::<u32>()) {
                Some(Ok(m)) if m <= 15 => {}
                Some(Ok(m)) => {
                    problems.push(format!("{where_}: @milestone {m} is out of range 0..=15"))
                }
                Some(Err(_)) => problems.push(format!("{where_}: @milestone must be a number")),
                None => problems.push(format!(
                    "{where_}: missing `// @milestone:` (which milestone makes this enforceable)"
                )),
            }

            // --- bucket-specific ---
            if bucket == "rejected" {
                if ex.many("expect-error").is_empty() {
                    problems.push(format!(
                        "{where_}: rejected examples need >=1 `// @expect-error:`"
                    ));
                }
                if ex.one("rule").is_none() {
                    problems.push(format!(
                        "{where_}: rejected examples need a `// @rule:` diagnostic code"
                    ));
                }
            } else if !ex.many("expect-error").is_empty() {
                problems.push(format!(
                    "{where_}: accepted examples must not declare `@expect-error`"
                ));
            }

            if text.lines().count() < 3 {
                problems.push(format!("{where_}: file has no body below the header"));
            }
        }

        // --- gate arithmetic ---
        let _ = writeln!(report, "{bucket}: {} files", files.len());
        if files.len() < min {
            problems.push(format!(
                "gate: {bucket} corpus has {} files, charter Milestone 0 requires >= {min}",
                files.len()
            ));
        }

        let missing: Vec<&&str> = categories
            .iter()
            .filter(|c| !covered.contains(**c))
            .collect();
        let _ = writeln!(
            report,
            "{bucket}: {}/{} charter §16 categories covered",
            covered.len(),
            categories.len()
        );
        for m in &missing {
            problems.push(format!("{bucket}: charter §16 category not covered: `{m}`"));
        }
    }

    print!("{report}");
    if problems.is_empty() {
        println!("corpus-check: OK");
        std::process::ExitCode::SUCCESS
    } else {
        println!();
        for p in &problems {
            println!("corpus-check: {p}");
        }
        println!("\ncorpus-check: {} problem(s)", problems.len());
        std::process::ExitCode::from(1)
    }
}
