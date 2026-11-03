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
use crate::hir::{Decl, DeclKind, Hir};
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

/// **The predicate with a body a `requires` names, wherever it is
/// declared**, and its unit: a bare name is the program's one predicate of
/// that name (PW0351: a predicate is declared once, by name, since a host
/// tells a refusal by it in one set of words); a qualified one, `m.P`, is the
/// predicate `P` the module named `m`, whole, declares. No name is read by
/// its last segment alone.
fn bodied<'a>(
    hirs: &[&'a Hir],
    ws: &Workspace,
    written: &str,
) -> Option<(usize, crate::hir::DeclId, &'a Decl)> {
    let (module, name) = match written.rsplit_once('.') {
        Some((module, name)) => (Some(module), name),
        None => (None, written),
    };
    hirs.iter()
        .enumerate()
        .filter(|(u, _)| module.is_none_or(|m| ws.module_of(*u).is_some_and(|own| own.name == m)))
        .find_map(|(u, hir)| {
            hir.all_decls()
                .find(|(_, d)| d.kind == DeclKind::Predicate && d.body.is_some() && d.name == name)
                .map(|(id, d)| (u, id, d))
        })
}

const ANSWERED_BY_ITS_BODY: &str = "A predicate the program gives a body is the program's \
     authorization (ADR-0115 as amended by ADR-0384): the host asks that body, and never the \
     deployment, wherever an export requires it. A `requires` that does not see it names no \
     predicate of the program, so it would be asked of the deployment, whose predicate of the \
     same name, if any, answers in its place; and a page asks a predicate of its reader \
     through the deployment, which knows nothing of the body. So a predicate with a body is \
     seen where it is required, imported as any declaration is, and no page asks it yet.";

/// **PW6925: a predicate the program gives a body is answered by that body
/// alone** (track `row-ownership`, the review's findings 3 and 4; ADR-0115
/// as amended): a `requires` in `unit` that sees no predicate of its name,
/// where the program gives one a body elsewhere, would be the deployment's
/// (`contract::asked_by` names no component for it); and a page's `{#if P}`
/// is answered by the deployment (ADR-0307), so a page asks no predicate
/// with a body. Two declarations of one name, a body's and a deployment's,
/// are PW0351's.
pub fn answered_by_its_body(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    sigs: &Signatures,
) -> Vec<Diagnostic> {
    let code = crate::codes::PREDICATE_ANSWERED_BY_ITS_BODY;
    let Some(hir) = hirs.get(unit) else {
        return Vec::new();
    };
    let module = ws
        .module_of(unit)
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let refused = |span: crate::diagnostics::Span,
                   message: String,
                   related: Vec<Related>,
                   repair: String| Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason: "predicate_answered_by_its_body",
        detector: Detector::DeclarationRule,
        severity: Severity::Error,
        message,
        primary_span: span,
        related,
        explanation: Some(ANSWERED_BY_ITS_BODY.to_string()),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    };
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        // A `requires` that sees no predicate of the name a body has. One
        // on another declaration is PW0335's.
        if matches!(decl.kind, DeclKind::Command | DeclKind::Query)
            && let Some(requires) = decl.policy("requires")
            && let Ok(predicates) = crate::policy::predicates(&requires.value)
        {
            for predicate in predicates {
                if !matches!(
                    ws.resolve_path_in(unit, Namespace::Predicate, &predicate.name),
                    Resolution::Unresolved
                ) {
                    continue;
                }
                let Some((at, given, declared)) = bodied(hirs, ws, &predicate.name) else {
                    continue;
                };
                let name = &declared.name;
                let from = ws.module_of(at).map(|m| m.name.clone()).unwrap_or_default();
                out.push(refused(
                    requires.span.clone(),
                    format!(
                        "`requires {}` names no predicate `{module}` sees, and `{from}` gives \
                         `{name}` a body: the host would ask the deployment for it instead",
                        predicate.name
                    ),
                    vec![Related::in_unit(
                        at,
                        hirs[at].decl_span(given),
                        format!("`predicate {}` is given its body here", declared.name),
                    )],
                    format!("import it where it is required: `import {from}.{{ {name} }}`"),
                ));
            }
        }
        // A page, a view or a layout asking one of its reader.
        if !matches!(
            decl.kind,
            DeclKind::Page | DeclKind::View | DeclKind::Component | DeclKind::Layout
        ) || decl.body.is_none()
        {
            continue;
        }
        for name in crate::holds::asked(hirs, ws, sigs, unit, id) {
            let def = match ws.resolve_in(unit, Namespace::Predicate, &name) {
                Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
                _ => continue,
            };
            let Some(declared) = hirs
                .get(def.unit)
                .map(|h| h.decl(crate::hir::DeclId(def.decl)))
            else {
                continue;
            };
            if declared.body.is_none() {
                continue;
            }
            out.push(refused(
                decl.name_span.clone(),
                format!(
                    "`{}` asks `{name}` of its reader, and the program gives `{name}` a body: a \
                     host answers what a page asks from the deployment's predicates, which know \
                     nothing of it",
                    decl.name
                ),
                vec![Related::in_unit(
                    def.unit,
                    hirs[def.unit].decl_span(crate::hir::DeclId(def.decl)),
                    format!("`predicate {name}` is given its body here"),
                )],
                "show the control to every reader and tell its refusal in the predicate's words \
                 (`on:press|refusable`, ADR-0302): a page asking a predicate with a body is not \
                 built"
                    .to_string(),
            ));
        }
    }
    out
}

/// **The query a path written in `unit` names, where it is one that
/// `requires` a predicate**, and the declaration: what PW6907 and PW6926
/// hold.
fn guarded_query<'a>(
    hirs: &[&'a Hir],
    ws: &Workspace,
    unit: usize,
    written: &str,
) -> Option<(DefId, &'a Decl)> {
    let def = crate::page_values::resolve_term(ws, unit, written)?;
    let decl = hirs.get(def.unit)?.decl(crate::hir::DeclId(def.decl));
    (decl.kind == DeclKind::Query && decl.policy("requires").is_some()).then_some((def, decl))
}

/// What a guarded query requires, as written.
fn required(decl: &Decl) -> String {
    decl.policy("requires")
        .map(|p| p.value.trim().to_string())
        .unwrap_or_default()
}

const PER_READER: &str = "A query that `requires` a predicate is answered to a reader only \
     once the predicate holds for them (ADR-0339's ruling 3, ADR-0384's H2). The host keys a \
     read by its reader only where the query is cached `private` or is a `user` or `session` \
     query; any other read is keyed by its arguments alone, so a shared cache answers the next \
     reader of them from the first one's entry, and a reader asking while another's read of the \
     same arguments is in flight joins that read and is answered theirs, never asked. A \
     materialization is kept for every reader alike, and asks none.";

/// **PW6907: a query that requires a predicate is answered to its reader
/// alone** (ADR-0384's H2 code for a query's `requires`; the reviews of
/// track `row-ownership`, finding 7b and the second review's finding 1): its
/// answer is given where the predicate holds for the reader asking, so every
/// read of it is keyed by its reader, `cache private` or a `user` or
/// `session` query, never by its arguments alone, where a shared cache's
/// entry or another reader's flight would answer readers never asked; and no
/// materialization, kept for every reader alike, depends on it. A `user` or
/// `session` query in a shared cache is PW5001's already, the one defect told
/// once (ADR-0112); a `cache` that is no word of its domain, PW0335's.
pub fn answered_per_reader(hirs: &[&Hir], unit: usize, ws: &Workspace) -> Vec<Diagnostic> {
    let code = crate::codes::REQUIRED_QUERY_UNSHARED;
    let Some(hir) = hirs.get(unit) else {
        return Vec::new();
    };
    let refused =
        |message: String, span: crate::diagnostics::Span, related: Vec<Related>, repair: &str| {
            Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "required_query_unshared",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: span,
                related,
                explanation: Some(PER_READER.to_string()),
                repairs: vec![Repair {
                    description: repair.to_string(),
                    replacement: None,
                }],
            }
        };
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        match decl.kind {
            DeclKind::Query => {
                let Some(requires) = decl.policy("requires") else {
                    continue;
                };
                if matches!(decl.visibility.as_deref(), Some("user" | "session")) {
                    continue;
                }
                let cache = decl.policy("cache");
                let related = vec![
                    Related::new(requires.span.clone(), "the predicate it is answered under"),
                    Related::new(hir.decl_span(id), format!("`{}`", decl.name)),
                ];
                let (message, span) = match cache.map(|c| (c.value.trim(), c)) {
                    Some(("private", _)) => continue,
                    Some(("shared", c)) => (
                        format!(
                            "`{}` requires {} and declares a shared cache: its answer is given \
                             where the predicate holds for the reader asking, and a shared \
                             cache would give it to readers it never asked",
                            decl.name,
                            requires.value.trim()
                        ),
                        c.span.clone(),
                    ),
                    Some(_) => continue,
                    None => (
                        format!(
                            "`{}` requires {} and is read for every reader alike, keyed by its \
                             arguments alone: a reader asking while another's read of them is in \
                             flight would join it, and be answered theirs, never asked",
                            decl.name,
                            requires.value.trim()
                        ),
                        decl.name_span.clone(),
                    ),
                };
                out.push(refused(
                    message,
                    span,
                    related,
                    "cache it for its reader alone, `cache private`, or make it a `user` or \
                     `session` query, keyed by its reader",
                ));
            }
            DeclKind::Materialize => {
                let Some(depends) = decl.policy("depends_on") else {
                    continue;
                };
                for key in &depends.keys {
                    let Some((def, query)) = guarded_query(hirs, ws, unit, &key.name) else {
                        continue;
                    };
                    out.push(refused(
                        format!(
                            "`{}` depends on `{}`, which requires {}: a materialization is kept \
                             for every reader alike, and asks none of them",
                            decl.name,
                            key.name,
                            required(query)
                        ),
                        depends.span.clone(),
                        vec![Related::in_unit(
                            def.unit,
                            hirs[def.unit].decl_span(crate::hir::DeclId(def.decl)),
                            format!("`{}` requires its predicate here", query.name),
                        )],
                        "read the query where a page binds it, `query Q(..)`, for its reader, \
                         or depend on one that requires nothing",
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

const BOUND_BY_A_PAGE: &str = "A query that `requires` a predicate is the host's to ask before \
     its body runs (ADR-0339's ruling 3, ADR-0384's H2), and the host asks it where a page binds \
     the query, `query Q(..)`, a read of its own that it makes for the page's reader. Called, \
     bound or named anywhere else, its body is compiled into that declaration, whose contract \
     requires nothing, so it would run for a reader its predicate refuses: as a command is sent \
     by a handler alone (PW0339), a query that requires is read by a page's binding alone.";

/// **PW6926: a query that requires a predicate is read by a page's binding
/// alone** (the second review of track `row-ownership`, finding 2; ADR-0339's
/// ruling 3, ADR-0384's H2). A query's body called from another declaration
/// is compiled into it, and that declaration's contract requires nothing, so
/// `Wrapper() { Secret() }`, a `fn` around it, a command, a predicate or a
/// materialization would each run `Secret` for anyone. Refused wherever such
/// a query is named outside a page's binding, `query Q(..)`, which a page, a
/// view or a layout writes and the host reads as a request of its own: a
/// call, a name used as a value, and a binding in any other declaration. A
/// name a body binds itself is that body's own (ADR-0063).
pub fn read_unasked(hirs: &[&Hir], unit: usize, ws: &Workspace) -> Vec<Diagnostic> {
    use crate::hir::Expr;
    let code = crate::codes::REQUIRED_QUERY_BOUND_BY_A_PAGE;
    let Some(hir) = hirs.get(unit) else {
        return Vec::new();
    };
    // A program no query of which requires a predicate has nothing to hold:
    // every program but those H2 lets guard a query, read at once.
    let guards = hirs.iter().any(|h| {
        h.all_decls()
            .any(|(_, d)| d.kind == DeclKind::Query && d.policy("requires").is_some())
    });
    if !guards {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        let Some(b) = decl.body else {
            continue;
        };
        let body = hir.body(b);
        let binds = matches!(
            decl.kind,
            DeclKind::Page | DeclKind::View | DeclKind::Component | DeclKind::Layout
        );
        // Built where a guarded query is named, for the names a body binds.
        let mut lexical: Option<Option<crate::lexical::Lexical>> = None;
        for e in body.walk() {
            let (written, named) = match body.expr(e) {
                Expr::Name(_) | Expr::Field { .. } => (crate::infer::path_of(body, e), true),
                Expr::Keyword {
                    keyword, modifiers, ..
                } if keyword == "query" && !binds => match modifiers.first() {
                    Some(query) => (query.clone(), false),
                    None => continue,
                },
                _ => continue,
            };
            if written.is_empty() {
                continue;
            }
            let Some((def, query)) = guarded_query(hirs, ws, unit, &written) else {
                continue;
            };
            let how = match named {
                false => "binds",
                true => {
                    let scope =
                        lexical.get_or_insert_with(|| crate::lexical::Lexical::build_in(hir, id));
                    if scope.as_ref().is_some_and(|l| l.binder(e).is_some()) {
                        continue;
                    }
                    let called = body
                        .walk()
                        .into_iter()
                        .any(|c| matches!(body.expr(c), Expr::Call { callee, .. } if *callee == e));
                    if called { "calls" } else { "names" }
                }
            };
            out.push(Diagnostic {
                code: code.id,
                invariant: code.invariant,
                reason: "required_query_bound_by_a_page",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}` {how} `{written}`, which requires {}: its body would run inside `{}` \
                     unasked, and the host asks its reader only where a page binds it",
                    decl.name,
                    required(query),
                    decl.name
                ),
                primary_span: body.expr_span(e),
                related: vec![Related::in_unit(
                    def.unit,
                    hirs[def.unit].decl_span(crate::hir::DeclId(def.decl)),
                    format!("`{}` requires its predicate here", query.name),
                )],
                explanation: Some(BOUND_BY_A_PAGE.to_string()),
                repairs: vec![Repair {
                    description: format!(
                        "bind it in a page, `let x = query {written}(..)`, where its reader is \
                         asked; or move what `{}` shares with it into a `fn` that reads what \
                         its reader may",
                        decl.name
                    ),
                    replacement: None,
                }],
            });
        }
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
