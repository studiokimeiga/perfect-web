//! **A predicate a command requires, and the words its refusal is told in**
//! (ADR-0302).
//!
//! `requires SignedIn, OwnsPost(post)` names predicates whose meaning is the
//! deployment's (ADR-0115): the host evaluates each over the caller and the
//! command's typed arguments, before the command runs. Until ADR-0302 nothing
//! said what a reader is told when one refuses, and the browser told
//! nothing: a press that was refused failed silently (the owner's finding of
//! 2026-10-08). Now the deployment that gives a predicate its meaning gives
//! it words too, and a program may declare its own:
//!
//! ```text
//! predicate OwnsPost(post: PostId)
//!     says "Only its author can delete a post."
//! ```
//!
//! PW0351 holds a declared predicate to its words, one string, not empty,
//! with no hole, so it names nothing the refusal read; and a `requires` that
//! names a declared predicate to its parameters, by count and by type. A
//! predicate the program does not declare is the deployment's alone, told in
//! the deployment's words.

use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{DeclKind, Hir};
use crate::resolve::{DefId, Namespace, Resolution, Workspace};
use crate::signatures::Signatures;

/// The words a predicate's `says` holds, as written between its quotes, or
/// why they are not words a refusal can be told in.
pub fn words(value: &str) -> Result<String, String> {
    let v = value.trim();
    let inner = v
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .filter(|inner| !inner.contains('"'))
        .ok_or_else(|| "its words are one string: `says \"Sign in to post.\"`".to_string())?;
    if inner.contains('{') || inner.contains('}') {
        return Err("its words name nothing the refusal read: a `{` would be a hole".to_string());
    }
    if inner.trim().is_empty() {
        return Err("its words are empty".to_string());
    }
    Ok(inner.to_string())
}

/// PW0351, over one unit.
pub fn check(hir: &Hir, unit: usize, sigs: &Signatures, ws: &Workspace) -> Vec<Diagnostic> {
    let code = crate::codes::PREDICATE_DECLARED;
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let refused = |span: crate::diagnostics::Span, message: String, repair: &str| Diagnostic {
            code: code.id,
            invariant: code.invariant,
            reason: "predicate_declared",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message,
            primary_span: span,
            related: vec![Related::new(hir.decl_span(id), format!("`{}`", decl.name))],
            explanation: Some(
                "A command a predicate refuses is told to the reader where the press was, \
                 in the predicate's own words (ADR-0302). The deployment says what a \
                 predicate means; the program says what it is called, what it takes, and \
                 what a reader is told when it refuses."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: repair.to_string(),
                replacement: None,
            }],
        };
        if decl.kind == DeclKind::Predicate {
            match decl.policy("says") {
                None => out.push(refused(
                    decl.name_span.clone(),
                    format!(
                        "`predicate {}` says nothing, and a refusal by it would be told nothing",
                        decl.name
                    ),
                    "say what a reader is told: `says \"Sign in to post.\"`",
                )),
                Some(p) => {
                    if let Err(why) = words(&p.value) {
                        out.push(refused(
                            p.span.clone(),
                            format!("`predicate {}`: {why}", decl.name),
                            "write one string, the words a reader is told",
                        ));
                    }
                }
            }
        }
        let Some(requires) = decl.policy("requires") else {
            continue;
        };
        // Its syntax, and its arguments as the command's parameters, are
        // held where every clause's value is.
        let Ok(predicates) = crate::policy::predicates(&requires.value) else {
            continue;
        };
        let command = sigs.by_def(DefId { unit, decl: id.0 });
        for predicate in predicates {
            // One the program does not declare is the deployment's alone.
            let def = match ws.resolve_path_in(unit, Namespace::Predicate, &predicate.name) {
                Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
                _ => continue,
            };
            let Some(declared) = sigs.by_def(def) else {
                continue;
            };
            if declared.params.len() != predicate.arguments.len() {
                out.push(refused(
                    requires.span.clone(),
                    format!(
                        "`{}` takes {} argument{}, and `requires` gives it {}",
                        predicate.name,
                        declared.params.len(),
                        if declared.params.len() == 1 { "" } else { "s" },
                        predicate.arguments.len()
                    ),
                    "give it the command's parameters it is declared to take",
                ));
                continue;
            }
            for (at, argument) in predicate.arguments.iter().enumerate() {
                let given = command.and_then(|c| {
                    let i = c.names.iter().position(|n| n == argument)?;
                    c.params.get(i)?.as_ref()?.resolved()
                });
                let taken = declared
                    .params
                    .get(at)
                    .and_then(Option::as_ref)
                    .and_then(|t| t.resolved());
                if let (Some(given), Some(taken)) = (given, taken)
                    && !given.same_as(taken)
                {
                    out.push(refused(
                        requires.span.clone(),
                        format!(
                            "`{}` takes a `{}`, and `{argument}` is a `{}`",
                            predicate.name,
                            taken.display_name(),
                            given.display_name()
                        ),
                        "give it a parameter of the type it is declared to take",
                    ));
                }
            }
        }
    }
    out
}

/// **PW6905: a predicate's body reads, and does nothing else** (ADR-0384's
/// H2; ADR-0115 as amended). It is the program's authorization, run by the
/// host before the body it guards, in its transaction or its snapshot: what
/// it may do is read the program's tables and its reader (`database.read`,
/// `session.read`), never write, emit, schedule, fetch, log or read the
/// clock, so asking it changes nothing and tells nothing but its answer.
pub fn bodies(
    hir: &Hir,
    unit: usize,
    inference: &crate::effects::Inference<'_>,
) -> Vec<Diagnostic> {
    let code = crate::codes::PREDICATE_BODY_READS_ONLY;
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if decl.kind != DeclKind::Predicate || decl.body.is_none() {
            continue;
        }
        let done: Vec<String> = inference
            .effective_effects(unit, hir, id)
            .into_iter()
            .filter(|e| {
                let family = e.split('<').next().unwrap_or(e);
                !matches!(family, "database.read" | "session.read")
            })
            .collect();
        if done.is_empty() {
            continue;
        }
        out.push(Diagnostic {
            code: code.id,
            invariant: code.invariant,
            reason: "predicate_body_reads_only",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!(
                "`predicate {}`'s body does more than read: {}",
                decl.name,
                done.iter()
                    .map(|e| format!("`{e}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            primary_span: decl.name_span.clone(),
            related: vec![Related::new(hir.decl_span(id), format!("`{}`", decl.name))],
            explanation: Some(
                "A predicate the program gives a body is its authorization (ADR-0115 as \
                 amended by ADR-0384): the host asks it before the command or query it guards \
                 runs, in that command's transaction or that read's snapshot. Asking it must \
                 change nothing and tell nothing but whether it holds, so its body reads the \
                 program's tables and its reader, and does nothing else."
                    .to_string(),
            ),
            repairs: vec![Repair {
                description: "read what decides it, and do the rest in the command it guards"
                    .to_string(),
                replacement: None,
            }],
        });
    }
    out
}

/// **The words each predicate the program declares is told in**, by name,
/// which `pw build` writes for the host (`predicates.json`). The first
/// declaration of a name, in unit order; PW0351 refuses a second.
pub fn words_of(hirs: &[&Hir]) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for hir in hirs {
        for (_, decl) in hir.all_decls() {
            if decl.kind != DeclKind::Predicate {
                continue;
            }
            if let Some(Ok(said)) = decl.policy("says").map(|p| words(&p.value)) {
                out.entry(decl.name.clone()).or_insert(said);
            }
        }
    }
    out
}

/// PW0351, over the program: a predicate is declared once, since a host
/// tells a refusal by it in one set of words. Reported at each declaration
/// of unit `unit` that an earlier one, in this unit or another, made.
pub fn declared_twice(hirs: &[&Hir], unit: usize) -> Vec<Diagnostic> {
    let code = crate::codes::PREDICATE_DECLARED;
    let mut first: std::collections::BTreeMap<&str, (usize, &str, crate::hir::Span)> =
        Default::default();
    let mut out = Vec::new();
    for (u, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != DeclKind::Predicate {
                continue;
            }
            match first.get(decl.name.as_str()) {
                None => {
                    let module = hir
                        .modules
                        .iter()
                        .next()
                        .map_or("", |(_, m, _)| m.name.as_str());
                    first.insert(decl.name.as_str(), (u, module, hir.decl_span(id)));
                }
                Some((at, module, span)) if u == unit => out.push(Diagnostic {
                    code: code.id,
                    invariant: code.invariant,
                    reason: "predicate_declared",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: format!(
                        "`predicate {}` is declared in `{module}` already: a refusal by it is \
                         told in one set of words",
                        decl.name
                    ),
                    primary_span: decl.name_span.clone(),
                    // The first declaration, in its own file (ADR-0321):
                    // until 2026-10-12 the note named this one again.
                    related: vec![Related::in_unit(
                        *at,
                        span.clone(),
                        format!("`predicate {}` is first declared here", decl.name),
                    )],
                    explanation: None,
                    repairs: vec![Repair {
                        description: "declare it once, and import it where it is required"
                            .to_string(),
                        replacement: None,
                    }],
                }),
                Some(_) => {}
            }
        }
    }
    out
}
