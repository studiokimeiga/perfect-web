//! Charter §7.1, §7.10, §8.2 — three questions about what a value *is*.
//!
//! Not a type checker. A type checker infers a type for every expression; this
//! answers three specific questions the corpus asks, each from a type the
//! author **wrote down**:
//!
//! - **R-008** uses `Option<Store>` where `Store` is expected. Absence is a
//!   value of type `Option<T>` and never an ambient null, so the only way to
//!   reach the `Store` is to match.
//! - **R-009** casts `Unknown` to `Store`. External data enters as `Unknown`
//!   and there is no cast that skips decoding — that is what makes the
//!   boundary trustworthy rather than merely typed.
//! - **R-022** hands a `PressEvent` handler to `on:submit`.
//!
//! Scoping it this way is deliberate. Inferring types for everything would
//! reach these three and a great deal else, and the "else" is where a partial
//! inference engine reports confident nonsense about programs it does not
//! understand. Every rule here fires only where an annotation exists, so a
//! program without annotations gets silence rather than guesses.
//!
//! What that costs is recorded rather than hidden: `store_name` would still be
//! accepted if the author dropped the `: Option<Store>` annotation. The
//! annotation is the specification, and E9 proper is what replaces it.

use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{AttrValue, Body, Decl, Expr, Hir, Node, Span, TypeRefId};
use crate::signatures::Signatures;

/// Where an `on:` attribute's event type is declared. One name, and the only
/// module name the compiler knows.
const EVENTS_MODULE: &str = "events";

/// **The record an event gives its handler** (ADR-0131): the parameter type
/// of the platform's `events.<event>`, `InputEvent` for `input`.
pub(crate) fn event_type<'s>(
    sigs: &'s Signatures,
    event: &str,
) -> Option<&'s crate::resolved::ResolvedType> {
    sigs.by_path(&format!("{EVENTS_MODULE}.{event}"))
        .and_then(|s| s.params.first())
        .and_then(Option::as_ref)
        .and_then(crate::resolved::TypeResolution::resolved)
}

pub fn check(hir: &Hir, sigs: &Signatures, out: &mut Vec<Diagnostic>) {
    for (id, decl) in hir.all_decls() {
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let at = hir.decl_span(id);
        let module = module_of(hir, id);
        let types = crate::infer::Types::of_decl(sigs, hir, id, body);
        optional_used_as_present(hir, &types, body, decl, &at, out);
        unchecked_cast(hir, sigs, &types, body, decl, &at, out);
        handler_matches_event(hir, body, sigs, &types, decl, module, &at, out);
        results_are_handled(&types, body, decl, &at, out);
    }
}

// --- ADR-0099: a failure is handled ------------------------------------------

/// **A `Result` whose value nothing uses** is a failure dropped: the first
/// `Carts.add(..)` of two, as a statement, failed and the command went on.
/// The charter's "unhandled ADT variant", one step earlier: neither case of
/// the value is handled. `?` passes the failure on, `match` handles it, and
/// a binding that says so, `let _cleared = ..`, discards it by name.
///
/// Which values are used: a statement's is not, nor a `for` body's, nor a
/// body declared `-> ()`'s last. A lambda's body is its result, used by
/// whoever calls it, except a handler's: the runtime calls it, and drops what
/// it gives back, its last value, a value it `return`s and the failure a `?`
/// returns (ADR-0159). Until 2026-10-03 a handler's command answer was the
/// runtime's to drop, since a handler could not read it (ADR-0157).
fn results_are_handled(
    types: &crate::infer::Types<'_>,
    body: &Body,
    decl: &Decl,
    at: &Span,
    out: &mut Vec<Diagnostic>,
) {
    let returns_unit = decl
        .ret
        .as_ref()
        .is_none_or(|t| matches!(t.written().as_str(), "()" | ""));
    let handlers: std::collections::BTreeSet<crate::hir::ExprId> =
        crate::resume::handlers_in(body).into_iter().collect();
    let mut dropped = Vec::new();
    let fate = match returns_unit {
        true => Fate::Dropped,
        false => Fate::Used,
    };
    unused(body, body.root, fate, false, &handlers, &mut dropped);
    let mut found: Vec<(crate::hir::ExprId, Fate, crate::resolved::ResolvedType)> = dropped
        .into_iter()
        .filter_map(|(e, fate)| Some((e, fate, types.of(body, e)?)))
        .collect();
    // A declaration named as the handler, `on:press={clear}`, gives the
    // runtime its answer, as `() => clear()` does (ADR-0199).
    for &h in &handlers {
        if let Some(answer) = types
            .named_handler(body, h)
            .and_then(|s| s.returns.as_ref())
            .and_then(crate::resolved::TypeResolution::resolved)
        {
            found.push((h, Fate::ToTheRuntime, answer.clone()));
        }
    }
    for (e, fate, ty) in found {
        if ty.as_builtin() != Some(crate::resolved::Builtin::Result) {
            continue;
        }
        let ty = ty.display_name();
        let (reason, message, explanation, repair) = match fate {
            Fate::ToTheRuntime => (
                "handler_result_dropped",
                format!(
                    "this handler gives the runtime its `{ty}`, which the runtime drops, and its failure with it"
                ),
                "A handler's value goes to the runtime that called it, which does \
                 nothing with it: a failure the handler does not take apart goes \
                 unshown, and the press looks as though it worked (ADR-0159). That is \
                 its last value, a value it returns, and the failure a `?` returns.",
                "take it apart with `match` and show its failure, or discard it, `let _ = ..`",
            ),
            Fate::FailureToTheRuntime => (
                "handler_failure_dropped",
                format!("`?` gives the runtime this `{ty}`'s failure, which the runtime drops"),
                "A handler's `?` returns its failure to the runtime that called the \
                 handler, which does nothing with it: the failure goes unshown, and the \
                 press looks as though it worked (ADR-0159).",
                "take it apart with `match` and show its failure, or discard it, `let _ = ..`",
            ),
            _ => (
                "result_dropped",
                format!("this `{ty}` is dropped, and its failure with it"),
                // Until 2026-09-26 a statement's `Result` was dropped without a word.
                "A `Result` carries a failure, and a value nothing uses handles neither \
                 of its cases: the program goes on as though it had succeeded.",
                "pass the failure on with `?`, handle it with `match`, or discard it, `let _ = ..`",
            ),
        };
        out.push(Diagnostic {
            code: codes::RESULT_DROPPED.id,
            invariant: codes::RESULT_DROPPED.invariant,
            reason,
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message,
            primary_span: body.expr_span(e),
            related: vec![Related::new(at.clone(), format!("inside `{}`", decl.name))],
            explanation: Some(explanation.to_string()),
            repairs: vec![Repair {
                description: repair.to_string(),
                replacement: None,
            }],
        });
    }
}

/// **What becomes of a value** (ADR-0099, ADR-0159).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// Something reads it.
    Used,
    /// Nothing does: a statement's value.
    Dropped,
    /// A handler gives it to the runtime that called it, which drops it.
    ToTheRuntime,
    /// A handler's `?` gives the runtime its failure, which the runtime
    /// drops.
    FailureToTheRuntime,
}

/// Is `id` the head of a clause whose value is executable code, written as a
/// statement: `acquire`, or `release(handle)`?
fn executable_head(body: &Body, id: crate::hir::ExprId) -> bool {
    let name = match body.expr(id) {
        Expr::Name(n) => n,
        Expr::Call { callee, .. } => match body.expr(*callee) {
            Expr::Name(n) => n,
            _ => return false,
        },
        _ => return false,
    };
    crate::policy::domain_of(name) == Some(crate::policy::Domain::Body)
}

/// The expressions under `id` whose value nothing uses, each with what
/// becomes of it, where `fate` is what becomes of `id`'s own. A block, an
/// `if` and a `match` pass their value on; everything else uses what it
/// contains. `handler` says whether `id` is in a handler's own body, where a
/// value returned, and a failure `?` returns, go to the runtime.
fn unused(
    body: &Body,
    id: crate::hir::ExprId,
    fate: Fate,
    handler: bool,
    handlers: &std::collections::BTreeSet<crate::hir::ExprId>,
    out: &mut Vec<(crate::hir::ExprId, Fate)>,
) {
    let returned_to = match handler {
        true => Fate::ToTheRuntime,
        false => Fate::Used,
    };
    match body.expr(id) {
        Expr::Block { stmts } => {
            let last = stmts.len().saturating_sub(1);
            for (i, s) in stmts.iter().enumerate() {
                // `acquire { .. }`, `release(h) { .. }`: a block after a clause
                // whose value is code is that clause's value, which the
                // platform takes (the handle `acquire` produces).
                let clause = i > 0 && executable_head(body, stmts[i - 1]);
                // `return x` is two statements, and `x` is the value returned.
                let returned =
                    i > 0 && matches!(body.expr(stmts[i - 1]), Expr::Name(n) if n == "return");
                let fate = if clause {
                    Fate::Used
                } else if returned {
                    returned_to
                } else if i == last {
                    fate
                } else {
                    Fate::Dropped
                };
                unused(body, *s, fate, handler, handlers, out);
            }
        }
        Expr::If { cond, then, els } => {
            unused(body, *cond, Fate::Used, handler, handlers, out);
            unused(body, *then, fate, handler, handlers, out);
            if let Some(e) = els {
                unused(body, *e, fate, handler, handlers, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            unused(body, *scrutinee, Fate::Used, handler, handlers, out);
            for a in arms {
                unused(body, a.body, fate, handler, handlers, out);
            }
        }
        Expr::For {
            iterable, body: b, ..
        } => {
            unused(body, *iterable, Fate::Used, handler, handlers, out);
            unused(body, *b, Fate::Dropped, handler, handlers, out);
        }
        // A handler's body is the runtime's to drop; any other lambda's is
        // its caller's to use, a lambda inside a handler included.
        Expr::Lambda { body: b, .. } => match handlers.contains(&id) {
            true => unused(body, *b, Fate::ToTheRuntime, true, handlers, out),
            false => unused(body, *b, Fate::Used, false, handlers, out),
        },
        // `e?` in a handler returns `e`'s failure to the runtime.
        Expr::Try { value } if handler => {
            out.push((*value, Fate::FailureToTheRuntime));
            for c in body.children(*value) {
                unused(body, c, Fate::Used, handler, handlers, out);
            }
        }
        other => {
            let _ = other;
            if fate != Fate::Used {
                out.push((id, fate));
            }
            for c in body.children(id) {
                unused(body, c, Fate::Used, handler, handlers, out);
            }
        }
    }
}

/// The module a declaration belongs to.
///
/// A handler named in `on:submit={on_press}` is a declaration in THIS module,
/// so it is looked up by its qualified path. Falling back to the unique-member
/// rule made the check depend on the name being unique across the whole
/// program — `resolve_corpus` caught it: adding a sibling that declared
/// `on_press` too made the lookup ambiguous, and the rule went silent instead
/// of going wrong, which is the harder failure to notice.
fn module_of(hir: &Hir, decl: crate::hir::DeclId) -> Option<&str> {
    hir.modules
        .iter()
        .find(|(_, m, _)| m.decls.contains(&decl))
        .map(|(_, m, _)| m.name.as_str())
}

// --- R-008: a value that may be absent, used as though it is present --------

fn optional_used_as_present(
    hir: &Hir,
    types: &crate::infer::Types<'_>,
    body: &Body,
    decl: &Decl,
    at: &Span,
    out: &mut Vec<Diagnostic>,
) {
    // Bindings that hold an `Option<T>`, with the inner type.
    //
    // From the ANNOTATION where the author wrote one, and otherwise from the
    // initialiser's declared return type. The second source is what makes this
    // a rule about the program rather than about how much the author chose to
    // write down: `let store = Stores.find(id)` is an `Option<Store>` because
    // `find` says so, and dropping the annotation used to make the defect
    // disappear.
    let mut optional: Vec<(String, String, String, Span)> = Vec::new();
    for id in body.walk() {
        let Expr::Let {
            pat: Some(pat),
            ty,
            init,
        } = body.expr(id)
        else {
            continue;
        };
        let crate::hir::Pattern::Bind { name, .. } = body.pat(*pat) else {
            continue;
        };
        let _ = (ty, init);
        let Some(ty) = types.binding(crate::lexical::Binder::Pattern(*pat)) else {
            continue;
        };
        if ty.as_builtin() != Some(crate::resolved::Builtin::Option) {
            continue;
        }
        let Some(inner) = ty.args().first() else {
            continue;
        };
        let (written, inner) = (ty.display_name(), inner.display_name());
        optional.push((name.clone(), written, inner, body.expr_span(id)));
    }
    if optional.is_empty() {
        return;
    }

    for id in body.walk() {
        // A field access is the case the corpus writes. `Option<T>` has no
        // fields of `T`; `store.name` is asking the container for something
        // only its contents have.
        let Expr::Field { base, name: field } = body.expr(id) else {
            continue;
        };
        let Expr::Name(base_name) = body.expr(*base) else {
            continue;
        };
        let Some((_, written, inner, origin)) = optional.iter().find(|(n, ..)| n == base_name)
        else {
            continue;
        };
        out.push(Diagnostic {
            code: codes::OPTION_USED_AS_VALUE.id,
            invariant: codes::OPTION_USED_AS_VALUE.invariant,
            reason: "optional_used_without_matching",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!("`{written}` cannot be used where `{inner}` is expected"),
            primary_span: body.expr_span(id),
            related: vec![
                Related::new(origin.clone(), format!("`{base_name}` is `{written}` here")),
                Related::new(
                    at.clone(),
                    format!("`{}` assumes the lookup succeeded", decl.name),
                ),
            ],
            explanation: Some(format!(
                "`.{field}` is a field of `{inner}`, and `{base_name}` is not an \
                 `{inner}` — it is a value that may or may not be one. There is no \
                 ambient `null` in this language; match on `Some`/`None`, and the \
                 branch where the value is absent becomes something you decided \
                 rather than something that happens at three in the morning."
            )),
            repairs: vec![Repair {
                description: format!("match {base_name} {{ Some(v) => v.{field}, None => … }}"),
                replacement: None,
            }],
        });
        let _ = hir;
    }
}

// --- R-009: a cast where a decode belongs -----------------------------------

#[allow(clippy::too_many_arguments)]
fn unchecked_cast(
    hir: &Hir,
    sigs: &Signatures,
    types: &crate::infer::Types<'_>,
    body: &Body,
    decl: &Decl,
    at: &Span,
    out: &mut Vec<Diagnostic>,
) {
    for id in body.walk() {
        let Expr::Cast { value, ty } = body.expr(id) else {
            continue;
        };
        // The operand's INFERRED type, so a rebinding does not launder it.
        // Reading only parameters and annotated `let`s meant `let same = raw`
        // produced a value the rule no longer recognised as external — the
        // same narrowness privacy labels had, in a different analysis.
        if !types.of(body, *value).is_some_and(|t| {
            sigs.language_type("decode", "Unknown")
                .is_some_and(|unknown| t.same_as(&unknown))
        }) {
            continue;
        }
        let name = match body.expr(*value) {
            Expr::Name(n) => n.clone(),
            _ => "this value".to_string(),
        };
        let target = written(body, *ty);
        out.push(Diagnostic {
            code: codes::UNCHECKED_EXTERNAL_CAST.id,
            invariant: codes::UNCHECKED_EXTERNAL_CAST.invariant,
            reason: "cast_from_unknown",
            detector: Detector::PatternMatrix,
            severity: Severity::Error,
            message: format!("`Unknown` cannot be cast to `{target}`"),
            primary_span: body.expr_span(id),
            related: vec![Related::new(
                at.clone(),
                format!("`{}` receives external data", decl.name),
            )],
            explanation: Some(format!(
                "`{name}` came from outside the program, so nothing has checked it \
                 yet. A cast asserts the shape without looking; external values must \
                 be decoded with a `Decoder<{target}>` returning `Result`, so the \
                 case where the outside world sent something else is a value you \
                 handle rather than a crash you find later. That is what makes the \
                 boundary trustworthy rather than merely typed."
            )),
            repairs: vec![Repair {
                description: format!("decode.run(decoder_for_{}, {name})", lower_head(&target)),
                replacement: None,
            }],
        });
        let _ = hir;
    }
}

// --- R-022: a handler that does not accept what it is given -----------------

#[allow(clippy::too_many_arguments)]
fn handler_matches_event(
    hir: &Hir,
    body: &Body,
    sigs: &Signatures,
    types: &crate::infer::Types<'_>,
    decl: &Decl,
    module: Option<&str>,
    at: &Span,
    out: &mut Vec<Diagnostic>,
) {
    let mut roots = Vec::new();
    for id in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(id) {
            roots.extend(r.iter().copied());
        }
    }

    for n in body.walk_markup(&roots) {
        let Node::Element { attrs, .. } = body.node(n) else {
            continue;
        };
        for a in attrs {
            let Some((event, modifiers)) = a.event() else {
                continue;
            };
            // What the runtime does before any code loads (ADR-0131): a
            // modifier it does not know would be dropped.
            for m in modifiers
                .iter()
                .filter(|m| !crate::hir::EVENT_MODIFIERS.contains(m))
            {
                out.push(unknown_modifier(decl, at, a, event, m));
            }
            // The event an attribute delivers is a platform fact:
            // `events.submit(event: SubmitEvent)`. The MODULE is a language
            // binding, in the same way `measure` names a frame phase — `on:`
            // attributes resolve in `events`, and that is stated here rather
            // than reached by "exactly one module declares a `submit`", which
            // would make the rule depend on an accident of the program.
            // An event the platform does not declare is not an event
            // (ADR-0093). A program with no `events` module declares none,
            // and there is nothing to hold the attribute to.
            if sigs.by_path(&format!("{EVENTS_MODULE}.{event}")).is_none() {
                let mut declared: Vec<&str> = sigs
                    .iter()
                    .filter_map(|(path, _)| path.strip_prefix(EVENTS_MODULE)?.strip_prefix('.'))
                    .collect();
                declared.sort();
                if !declared.is_empty() {
                    out.push(unknown_event(decl, at, a, event, &declared));
                }
                continue;
            }
            let Some(expected) = sigs
                .by_path(&format!("{EVENTS_MODULE}.{event}"))
                .and_then(|s| s.params.first())
                .and_then(Option::as_ref)
                .and_then(crate::resolved::TypeResolution::resolved)
            else {
                continue;
            };
            let AttrValue::Expr(e) = a.value else {
                continue;
            };
            // A lambda takes nothing, or the event (ADR-0138): its record,
            // typed as written, or as the event gives it where nothing is.
            if let Expr::Lambda { params, .. } = body.expr(e) {
                let params = crate::values::lambda_params(body, params);
                let wrong = match params.as_slice() {
                    [] => None,
                    [p] if !matches!(
                        body.pat(*p),
                        crate::hir::Pattern::Bind { .. } | crate::hir::Pattern::Wild
                    ) =>
                    {
                        Some("one that takes its event apart".to_string())
                    }
                    [p] => body
                        .param_types
                        .get(p)
                        .and_then(|t| crate::resolved::written_in_body(body, *t))
                        .and_then(|t| {
                            sigs.resolve_type(module, decl, &t, 0..0)
                                .resolved()
                                .cloned()
                        })
                        .filter(|t| !t.same_as(expected))
                        .map(|t| format!("one taking `{t}`")),
                    many => Some(format!("one taking {} parameters", many.len())),
                };
                if let Some(found) = wrong {
                    out.push(lambda_takes_another_event(
                        decl, at, body, e, event, expected, &found,
                    ));
                }
                continue;
            }
            // Otherwise a function or a command named as the handler,
            // `on:submit={save}` or `{forms.save}`, which is `(e) => save(e)`
            // (ADR-0199). Anything else is refused here, not when built.
            if !matches!(body.expr(e), Expr::Name(_) | Expr::Field { .. }) {
                continue;
            }
            let handler = crate::infer::path_of(body, e);
            let Some(sig) = types.named_handler(body, e) else {
                if let Some(what) = names_no_handler(decl, body, sigs, types, module, e, &handler) {
                    out.push(not_a_handler(decl, at, body, e, event, &handler, &what));
                }
                continue;
            };
            // It is given one value, its event.
            if sig.params.len() > 1 {
                out.push(lambda_takes_another_event(
                    decl,
                    at,
                    body,
                    e,
                    event,
                    expected,
                    &format!("`{handler}`, which takes {} parameters", sig.params.len()),
                ));
                continue;
            }
            let Some(actual) = sig
                .params
                .first()
                .and_then(Option::as_ref)
                .and_then(crate::resolved::TypeResolution::resolved)
            else {
                continue;
            };
            if actual.same_as(expected) {
                continue;
            }
            out.push(Diagnostic {
                code: codes::HANDLER_SIGNATURE_MISMATCH.id,
                invariant: codes::HANDLER_SIGNATURE_MISMATCH.invariant,
                reason: "handler_takes_the_wrong_event",
                detector: Detector::PatternMatrix,
                severity: Severity::Error,
                message: format!(
                    "`on:{event}` expects a handler taking `{expected}`, found one \
                     taking `{actual}`"
                ),
                primary_span: body.expr_span(e),
                related: vec![Related::new(
                    at.clone(),
                    format!("`{}` binds the handler here", decl.name),
                )],
                explanation: Some(format!(
                    "Form submission and pointer activation are different events, and \
                     `{handler}` is written for `{actual}`. Without this check the \
                     handler would simply never fire — which looks like a bug in the \
                     form rather than a mismatch at the binding."
                )),
                repairs: vec![Repair {
                    description: format!(
                        "give `{handler}` a `{expected}` parameter, or bind a handler \
                         that already takes one"
                    ),
                    replacement: None,
                }],
            });
        }
    }
    let _ = hir;
}

/// **What an `on:` value names, where it is no function or command**
/// (ADR-0199): a local's value that is a function, or a declaration of
/// another kind, described. `None` where another rule reports it: a value
/// that is no function at all (PW0614), or a name that resolves to nothing
/// (PW0021).
fn names_no_handler(
    decl: &Decl,
    body: &Body,
    sigs: &Signatures,
    types: &crate::infer::Types<'_>,
    module: Option<&str>,
    e: crate::hir::ExprId,
    path: &str,
) -> Option<String> {
    use crate::hir::DeclKind;
    use crate::resolve::{Namespace, Resolution};
    let workspace = sigs.workspace();
    let unit = sigs.unit_of(module)?;
    if crate::resume::root_name(body, e).is_some_and(|r| types.lexical().binder(r).is_some()) {
        // A local's value: one whose type is known, and no function, is
        // the value relation's to report (PW0614, `values.rs`).
        use crate::values::Ty;
        return match crate::values::type_of(sigs, workspace, unit, module, decl, body, e).0 {
            Ty::Builtin(crate::resolved::Builtin::Function, _)
            | Ty::Any
            | Ty::Unknown
            | Ty::Var(_)
            | Ty::Parameter { .. } => Some("a value this declaration binds".to_string()),
            _ => None,
        };
    }
    let def = [
        Namespace::Term,
        Namespace::Type,
        Namespace::Ui,
        Namespace::Event,
    ]
    .into_iter()
    .find_map(|ns| {
        let found = match path.contains('.') {
            true => workspace.resolve_path_in(unit, ns, path),
            false => workspace.resolve_in(unit, ns, path),
        };
        match found {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => Some(d),
            _ => None,
        }
    });
    let Some(def) = def else {
        // A case, the language's `Some` or one of a type this unit sees;
        // any other name that resolves to nothing is PW0021's.
        let case = matches!(path, "Some" | "None" | "Ok" | "Err")
            || crate::values::bare_case(sigs, workspace, unit, path).is_some();
        return case.then(|| "a case".to_string());
    };
    Some(
        match sigs.kind_of(def)? {
            DeclKind::View => "a view",
            DeclKind::Component => "a component",
            DeclKind::Page => "a page",
            DeclKind::Layout => "a layout",
            DeclKind::Query => "a query",
            DeclKind::Subscription => "a subscription",
            DeclKind::Resource => "a resource",
            DeclKind::Task => "a task",
            DeclKind::Materialize => "a materialization",
            DeclKind::Event => "an event",
            DeclKind::Type | DeclKind::Opaque => "a type",
            // A function or a command is a handler; a signal or a module's
            // value that is no function is PW0614's.
            _ => return None,
        }
        .to_string(),
    )
}

/// `on:press={save}`, where `save` is a local's value or a page: no function
/// or command the program declares (ADR-0199).
fn not_a_handler(
    decl: &Decl,
    at: &Span,
    body: &Body,
    e: crate::hir::ExprId,
    event: &str,
    handler: &str,
    what: &str,
) -> Diagnostic {
    Diagnostic {
        code: codes::NOT_CALLABLE.id,
        invariant: codes::NOT_CALLABLE.invariant,
        reason: "event_attribute_names_no_handler",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`on:{event}` names `{handler}`, {what}, which is no function or command to call"
        ),
        primary_span: body.expr_span(e),
        related: vec![Related::new(
            at.clone(),
            format!("`{}` binds the handler here", decl.name),
        )],
        explanation: Some(
            "An event attribute is given a lambda, its handler, or names a function or a \
             command the program declares, which is `(e) => save(e)` (ADR-0199). A \
             handler is compiled from the code it names, and a local's value or a \
             declaration of another kind has none to compile."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: format!(
                "write the handler in the attribute, `on:{event}={{() => ..}}`, or name a \
                 function or a command"
            ),
            replacement: None,
        }],
    }
}

/// `on:input={(e: PressEvent) => ..}`: a handler written for another event,
/// or for more than one (ADR-0138).
fn lambda_takes_another_event(
    decl: &Decl,
    at: &Span,
    body: &Body,
    e: crate::hir::ExprId,
    event: &str,
    expected: &crate::resolved::ResolvedType,
    found: &str,
) -> Diagnostic {
    Diagnostic {
        code: codes::HANDLER_SIGNATURE_MISMATCH.id,
        invariant: codes::HANDLER_SIGNATURE_MISMATCH.invariant,
        reason: "handler_takes_the_wrong_event",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`on:{event}` expects a handler taking `{expected}`, or nothing, found {found}"
        ),
        primary_span: body.expr_span(e),
        related: vec![Related::new(
            at.clone(),
            format!("`{}` binds the handler here", decl.name),
        )],
        explanation: Some(format!(
            "An event gives its handler one record of plain data, `{expected}` for \
             `on:{event}`, read in the browser when the event happens (ADR-0131). A \
             handler written for another record would read fields this one does not \
             have."
        )),
        repairs: vec![Repair {
            description: format!(
                "take `(e: {expected}) =>`, or `(e) =>`, which is typed by the event"
            ),
            replacement: None,
        }],
    }
}

/// `on:submit|prevnt`: a modifier the runtime does not apply (ADR-0138).
fn unknown_modifier(
    decl: &Decl,
    at: &Span,
    attr: &crate::hir::Attr,
    event: &str,
    modifier: &str,
) -> Diagnostic {
    Diagnostic {
        code: codes::UNKNOWN_MODIFIER.id,
        invariant: codes::UNKNOWN_MODIFIER.invariant,
        reason: "unknown_modifier",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`on:{event}|{modifier}` names no modifier the runtime applies: {}",
            crate::check::listed(crate::hir::EVENT_MODIFIERS)
        ),
        primary_span: attr.span.clone(),
        related: vec![Related::new(
            at.clone(),
            format!("`{}` binds a handler to it here", decl.name),
        )],
        explanation: Some(
            "A modifier is what the runtime does in the listener, before the handler's \
             code has loaded: `prevent` stops the browser's own action, a form's \
             submission or a link's navigation, and `stop` keeps the event from an \
             enclosing element's handler. A modifier it does not apply would be \
             dropped, and the browser's action taken."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "write `prevent` or `stop`".to_string(),
            replacement: None,
        }],
    }
}

/// `on:clik={go}`: an event the platform does not declare (ADR-0093).
fn unknown_event(
    decl: &Decl,
    at: &Span,
    attr: &crate::hir::Attr,
    event: &str,
    declared: &[&str],
) -> Diagnostic {
    Diagnostic {
        code: codes::UNKNOWN_EVENT.id,
        invariant: codes::UNKNOWN_EVENT.invariant,
        reason: "unknown_event",
        detector: Detector::PatternMatrix,
        severity: Severity::Error,
        message: format!(
            "`on:{event}` names no event the platform declares: {}",
            crate::check::listed(declared)
        ),
        primary_span: attr.span.clone(),
        related: vec![Related::new(
            at.clone(),
            format!("`{}` binds a handler to it here", decl.name),
        )],
        // The runtime listens for a click whatever the name, so until 2026-09-26
        // such a handler ran by accident.
        explanation: Some(
            "Which event an `on:` attribute delivers is declared by the platform, in \
             `events`. An attribute naming no declared event binds its handler to nothing: \
             a runtime that listens for the named event would never run it."
                .to_string(),
        ),
        repairs: vec![Repair {
            description: "name a declared event, or declare it in the platform's `events`"
                .to_string(),
            replacement: None,
        }],
    }
}

// --- shared ------------------------------------------------------------------

/// A type as the author wrote it: `Option<Store>`, not `Option`.
fn written(body: &Body, id: TypeRefId) -> String {
    let Some(t) = body.types.get(id.index()) else {
        return String::new();
    };
    if t.args.is_empty() {
        return t.path.clone();
    }
    let args: Vec<String> = t.args.iter().map(|a| written(body, *a)).collect();
    format!("{}<{}>", t.path, args.join(", "))
}

fn lower_head(t: &str) -> String {
    t.split('<').next().unwrap_or(t).to_lowercase()
}
