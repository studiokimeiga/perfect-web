//! **A control is shown where its command's predicates hold** (ADR-0307).
//!
//! The owner's finding of 2026-10-08: signed out, a reader typed into the
//! feed's composer and pressed Post, and the server refused it (`requires
//! SignedIn`, 403) in silence. ADR-0302 tells a refusal where the press was;
//! this holds what a page can tell before the press. A page asks a predicate
//! of its reader by naming it, `{#if SignedIn}`, and the deployment that
//! answers `requires` answers it for the page's principal when the document
//! is rendered: the control and its command ask one policy, as Pundit's
//! views, CASL's `<Can>` and Cedar's UI filtering do, not a second fact of
//! the program's (`me.signed_in`) that can disagree with it.
//!
//! - **PW0629**: a name a template reads is a value. `{#if InteractionId}`
//!   (a type), `{#if P}` (a page) and `{#if SignedIn}` (a predicate) checked
//!   and built until 2026-10-10, and the page failed where it was served,
//!   "no value for `SignedIn`". A predicate with no parameters is what the
//!   page asks of its reader; one with parameters is not asked yet, and a
//!   handler asks none (it runs in the browser, after the document).
//! - **PW5048**: a control whose handler sends a command that `requires` a
//!   predicate with no parameters stands where the page asks that predicate
//!   and it holds: in the first branch of an `{#if}` whose condition has it
//!   as a conjunct, or under `hidden={!P}` (a disjunct), on the control or an
//!   element around it. A view's control is held in the view's own markup.
//!   A control marked `on:press|refusable` is shown to every reader and its
//!   refusal told (ADR-0302): one in a part served to everyone, which no
//!   reader's answer can decide, or for a predicate the page does not ask.
//! - **PW5049**: a page that asks a predicate of its reader is its reader's;
//!   one served to everyone asks none.

use std::collections::BTreeSet;

use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{AttrValue, BinOp, Body, DeclId, DeclKind, Expr, ExprId, Hir, Node, NodeId, UnOp};
use crate::lexical::Lexical;
use crate::resolve::{DefId, Namespace, Resolution, Workspace};
use crate::signatures::Signatures;

/// The declarations whose markup a page's document holds.
fn shows_markup(kind: DeclKind) -> bool {
    matches!(
        kind,
        DeclKind::Page | DeclKind::View | DeclKind::Component | DeclKind::Layout
    )
}

/// **What a name a template reads names**, where no binding holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    /// A value: a function or a command, a constant, a signal, a case.
    Value,
    /// A predicate with no parameters, which the page asks of its reader.
    Asked(DefId),
    /// A predicate with parameters, asked of nothing yet.
    Parameterised(DefId),
    /// A declaration that holds no value, by what it is: "a type", "a page".
    NoValue(&'static str),
    /// Nothing: PW0020's.
    Unresolved,
}

/// **What `name` names in `unit`**, as a template reads it.
pub fn named(hirs: &[&Hir], ws: &Workspace, sigs: &Signatures, unit: usize, name: &str) -> Named {
    if let Resolution::Local(def) | Resolution::Imported { def, .. } =
        ws.resolve_in(unit, Namespace::Predicate, name)
    {
        return match sigs.by_def(def) {
            Some(sig) if sig.params.is_empty() => Named::Asked(def),
            Some(_) => Named::Parameterised(def),
            None => Named::Asked(def),
        };
    }
    match ws.resolve_in(unit, Namespace::Term, name) {
        Resolution::Local(def) | Resolution::Imported { def, .. } => {
            let Some(decl) = crate::resolve::declaration(hirs, def) else {
                return Named::Value;
            };
            match decl.kind {
                // A resource is mounted on the element whose `resource={..}`
                // names it (charter §7.5A, A-007).
                DeclKind::Fn
                | DeclKind::Command
                | DeclKind::Let
                | DeclKind::Signal
                | DeclKind::Resource => Named::Value,
                DeclKind::Query | DeclKind::Subscription => {
                    Named::NoValue("a query, which a page binds (`let x = query X(..)`)")
                }
                DeclKind::Materialize => {
                    Named::NoValue("a materialization, which a page binds (`let x = query X(..)`)")
                }
                DeclKind::Page => Named::NoValue("a page, which a link or a `navigate` names"),
                DeclKind::View | DeclKind::Component => Named::NoValue("a view, which a tag names"),
                DeclKind::Layout => Named::NoValue("a layout, which a page's `layout` names"),
                DeclKind::Event => Named::NoValue("an event, which a command emits"),
                DeclKind::Source => Named::NoValue("a data source"),
                DeclKind::Table => {
                    Named::NoValue("a table, whose module's functions read and write it")
                }
                DeclKind::Upload => Named::NoValue("an upload, which a form posts"),
                DeclKind::Type | DeclKind::Opaque => Named::NoValue("a type"),
                _ => Named::NoValue("a declaration that holds no value"),
            }
        }
        // A case, the language's own values, or a declaration of another
        // namespace, which holds no value: a view, an event, a type.
        _ => {
            let found = |ns| {
                matches!(
                    ws.resolve_in(unit, ns, name),
                    Resolution::Local(_) | Resolution::Imported { .. }
                )
            };
            if is_case(sigs, ws, unit, name) {
                Named::Value
            } else if found(Namespace::Ui) {
                Named::NoValue(
                    "a page, a view or a layout, which a link, a tag or a page's `layout` names",
                )
            } else if found(Namespace::Event) {
                Named::NoValue("an event, which a command emits")
            } else if found(Namespace::Effect) {
                Named::NoValue("an effect")
            } else if found(Namespace::Type) {
                Named::NoValue("a type")
            } else {
                Named::Unresolved
            }
        }
    }
}

/// Does a block's directive, as written, open `head` (`{#if`)?
fn opens(directive: &str, head: &str) -> bool {
    directive
        .trim()
        .strip_prefix(head)
        .is_some_and(|r| r.starts_with(char::is_whitespace) || r.starts_with('}'))
}

/// Is `name` a sum type's case this unit sees, written alone?
fn is_case(sigs: &Signatures, ws: &Workspace, unit: usize, name: &str) -> bool {
    crate::values::bare_case(sigs, ws, unit, name).is_some()
}

/// **Each predicate with no parameters a declaration's markup asks**, by
/// name, outside any handler: what a host evaluates for the document.
pub fn asked(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    unit: usize,
    decl: DeclId,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let hir = hirs[unit];
    let d = hir.decl(decl);
    let Some(body) = d.body.map(|b| hir.body(b)) else {
        return out;
    };
    let lexical = Lexical::build_in(hir, decl);
    let handlers = handler_exprs(body);
    for e in body.walk() {
        let Expr::Name(n) = body.expr(e) else {
            continue;
        };
        if handlers.contains(&e) || lexical.as_ref().is_some_and(|l| l.binder(e).is_some()) {
            continue;
        }
        if let Named::Asked(_) = named(hirs, ws, sigs, unit, n) {
            out.insert(n.clone());
        }
    }
    out
}

/// Every node of the body's markup.
fn nodes(body: &Body) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = template_roots(body);
    while let Some(n) = stack.pop() {
        out.push(n);
        match body.node(n) {
            Node::Element { children, .. } | Node::Block { children, .. } => {
                stack.extend(children.iter().copied())
            }
            _ => {}
        }
    }
    out
}

/// Every expression inside a handler of the body's markup.
fn handler_exprs(body: &Body) -> BTreeSet<ExprId> {
    let mut out = BTreeSet::new();
    for n in nodes(body) {
        if let Node::Element { attrs, .. } = body.node(n) {
            for a in attrs.iter().filter(|a| a.event().is_some()) {
                if let AttrValue::Expr(e) = &a.value {
                    out.extend(body.walk_from(*e));
                }
            }
        }
    }
    out
}

/// PW0629, PW5048 and PW5049, over one unit.
pub fn check(hirs: &[&Hir], ws: &Workspace, sigs: &Signatures, unit: usize) -> Vec<Diagnostic> {
    let hir = hirs[unit];
    let mut out = Vec::new();
    for (id, decl) in hir.all_decls() {
        if !shows_markup(decl.kind) {
            continue;
        }
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        let lexical = Lexical::build_in(hir, id);
        let bound = |e: ExprId| lexical.as_ref().is_some_and(|l| l.binder(e).is_some());
        let handlers = handler_exprs(body);
        let callees: BTreeSet<ExprId> = body
            .walk()
            .into_iter()
            .filter_map(|e| match body.expr(e) {
                Expr::Call { callee, .. } => Some(*callee),
                _ => None,
            })
            .collect();
        // A name a path starts with: a type that qualifies its case
        // (`Panel.Open`), a module its function (`List.length`).
        let qualifiers: BTreeSet<ExprId> = body
            .walk()
            .into_iter()
            .filter_map(|e| match body.expr(e) {
                Expr::Field { base, .. } => Some(*base),
                _ => None,
            })
            .collect();
        let related = vec![Related::new(hir.decl_span(id), format!("`{}`", decl.name))];
        // PW0629: what each name the markup and its values read names.
        for e in body.walk() {
            let Expr::Name(n) = body.expr(e) else {
                continue;
            };
            if bound(e) || callees.contains(&e) {
                continue;
            }
            // A type that qualifies a case names it; nothing reads the type.
            if qualifiers.contains(&e)
                && matches!(named(hirs, ws, sigs, unit, n), Named::NoValue("a type"))
            {
                continue;
            }
            let span = body.expr_span(e);
            let refuse = |message: String, repair: &str| Diagnostic {
                code: crate::codes::NAMES_NO_VALUE.id,
                invariant: crate::codes::NAMES_NO_VALUE.invariant,
                reason: "names_no_value",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message,
                primary_span: span.clone(),
                related: related.clone(),
                // Until 2026-10-10 such a name built, and the page failed where it
                // was served.
                explanation: Some(
                    "What a template reads is rendered from a value: a binding, a parameter, \
                     a signal or a constant. A name that holds none would build, and the \
                     page would fail where it is served (\"no value for `SignedIn`\"). A \
                     predicate with no parameters is the one exception: the page asks it of \
                     its reader, and the deployment answers it as it answers `requires` \
                     (ADR-0307)."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: repair.to_string(),
                    replacement: None,
                }],
            };
            // A handler's names are its own rules' (PW0614 names what a
            // named handler may be); here, only that it asks no predicate.
            let in_handler = handlers.contains(&e);
            match named(hirs, ws, sigs, unit, n) {
                Named::NoValue(_) | Named::Parameterised(_) if in_handler => {}
                Named::NoValue(what) => out.push(refuse(
                    format!("`{n}` is {what}, and holds no value to show"),
                    "read a value: bind the query, or name the binding that holds it",
                )),
                Named::Parameterised(_) => out.push(refuse(
                    format!("`{n}` is a predicate with parameters, which a page does not ask yet"),
                    "ask a predicate with no parameters, or show the control to every reader and let its refusal be told",
                )),
                Named::Asked(_) if in_handler => out.push(refuse(
                    format!(
                        "a handler asks `{n}`, which the page asks of its reader when it is rendered, before any press"
                    ),
                    "ask it in the markup, around the control: `{#if SignedIn}`",
                )),
                _ => {}
            }
        }
        // PW5048: each control where its command's predicates hold.
        let mut controls = Vec::new();
        for root in template_roots(body) {
            walk(body, root, &BTreeSet::new(), &mut controls, &|e| {
                predicates_in(hirs, ws, sigs, unit, body, e, &bound)
            });
        }
        for (control, handler, held) in controls {
            for (command, predicate) in required(hirs, ws, unit, body, handler) {
                if held.contains(&predicate) {
                    continue;
                }
                out.push(Diagnostic {
                    code: crate::codes::CONTROL_WHERE_REFUSED.id,
                    invariant: crate::codes::CONTROL_WHERE_REFUSED.invariant,
                    reason: "control_where_refused",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Error,
                    message: format!(
                        "this control sends `{command}`, which `requires {predicate}`, and is shown where the page does not ask `{predicate}`"
                    ),
                    primary_span: body.node_span(control),
                    related: related.clone(),
                    explanation: Some(format!(
                        "A control the server refuses is a dead control: shown to a reader \
                         for whom `{predicate}` does not hold, it offers a press that can only \
                         be refused. A page asks a predicate of its reader by naming it, and \
                         the deployment answers it as it answers `requires`, so a control \
                         shown under it is refused only if the reader changed since the page \
                         was rendered, and then told where the press was (ADR-0302). \
                         `{predicate}` is the program's to declare (`predicate {predicate} \
                         says \"…\"`) where only the deployment knows it."
                    )),
                    repairs: vec![Repair {
                        description: format!(
                            "show it where `{predicate}` holds: `{{#if {predicate}}}` around it, or `hidden={{!{predicate}}}` on it; where no reader's answer can decide it, as in a part served to everyone, mark its event `|refusable`"
                        ),
                        replacement: None,
                    }],
                });
            }
        }
        // PW5049: a page served to everyone asks nothing of its reader.
        if decl.kind == DeclKind::Page
            && !asked(hirs, ws, sigs, unit, id).is_empty()
            && crate::resume::page_scope(hir, decl) == "public"
        {
            let asked = asked(hirs, ws, sigs, unit, id);
            out.push(Diagnostic {
                code: crate::codes::READER_PREDICATE_SHARED.id,
                invariant: crate::codes::READER_PREDICATE_SHARED.invariant,
                reason: "reader_predicate_shared",
                detector: Detector::DeclarationRule,
                severity: Severity::Error,
                message: format!(
                    "`{}` asks {} of its reader, and is served to everyone",
                    decl.name,
                    asked
                        .iter()
                        .map(|p| format!("`{p}`"))
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
                primary_span: decl.name_span.clone(),
                related: Vec::new(),
                explanation: Some(
                    "What a predicate answers is its reader's: a document that shows a control \
                     because its reader is signed in is that reader's, and served to another it \
                     would show them a control their press is refused, or hide one from them."
                        .to_string(),
                ),
                repairs: vec![Repair {
                    description: "declare whose the page is: `session page`, `user page`"
                        .to_string(),
                    replacement: None,
                }],
            });
        }
    }
    out
}

/// The markup's roots.
fn template_roots(body: &Body) -> Vec<NodeId> {
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    roots
}

/// **Each control and what is known to hold where it is**: its node, its
/// handler, and the predicates the conditions around it ask and assert.
fn walk(
    body: &Body,
    node: NodeId,
    held: &BTreeSet<String>,
    out: &mut Vec<(NodeId, ExprId, BTreeSet<String>)>,
    asked: &dyn Fn(ExprId) -> Asserted,
) {
    match body.node(node) {
        Node::Element {
            attrs, children, ..
        } => {
            let mut here = held.clone();
            // `hidden={!P}` shows it only where `P` holds.
            for a in attrs.iter().filter(|a| a.name == "hidden") {
                if let AttrValue::Expr(e) = &a.value {
                    here.extend(asked(*e).when_false);
                }
            }
            // A control marked `|refusable` is shown to every reader, and its
            // refusal told: nothing is asked of where it stands.
            for a in attrs.iter().filter(|a| {
                a.event()
                    .is_some_and(|(_, modifiers)| !modifiers.contains(&"refusable"))
            }) {
                if let AttrValue::Expr(e) = &a.value {
                    out.push((node, *e, here.clone()));
                }
            }
            for c in children {
                walk(body, *c, &here, out, asked);
            }
        }
        Node::Block {
            directive,
            subject,
            children,
            ..
        } => {
            // An `{#if}`'s first branch: what its condition asserts. Its
            // `{:else}` and `{:else if}` branches assert nothing of it.
            let mut here = held.clone();
            let conditional = opens(directive, "{#if");
            if conditional && let Some(s) = subject {
                here.extend(asked(*s).when_true);
            }
            for c in children {
                if let Node::Branch { condition, .. } = body.node(*c) {
                    here = held.clone();
                    if conditional && let Some(cond) = condition {
                        here.extend(asked(*cond).when_true);
                    }
                    continue;
                }
                walk(body, *c, &here, out, asked);
            }
        }
        Node::Branch { .. } | Node::Text(_) | Node::Interpolation(_) => {}
    }
}

/// What a condition asserts of the predicates it asks.
#[derive(Debug, Default)]
struct Asserted {
    /// Those that hold wherever it is true: its conjuncts.
    when_true: BTreeSet<String>,
    /// Those that hold wherever it is false: each `!P` among its disjuncts.
    when_false: BTreeSet<String>,
}

fn predicates_in(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    unit: usize,
    body: &Body,
    e: ExprId,
    bound: &dyn Fn(ExprId) -> bool,
) -> Asserted {
    let is_asked = |x: ExprId| match body.expr(x) {
        Expr::Name(n) if !bound(x) => {
            matches!(named(hirs, ws, sigs, unit, n), Named::Asked(_)).then(|| n.clone())
        }
        _ => None,
    };
    let mut out = Asserted::default();
    // `a && b`: each conjunct holds where it is true.
    let mut conjuncts = vec![e];
    while let Some(x) = conjuncts.pop() {
        match body.expr(x) {
            Expr::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => conjuncts.extend([*lhs, *rhs]),
            _ => out.when_true.extend(is_asked(x)),
        }
    }
    // `!a || b`: where it is false, each negated disjunct's predicate holds.
    let mut disjuncts = vec![e];
    while let Some(x) = disjuncts.pop() {
        match body.expr(x) {
            Expr::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } => disjuncts.extend([*lhs, *rhs]),
            Expr::Unary {
                op: UnOp::Not,
                operand,
            } => {
                // `!(a && b)` is false where both hold.
                let mut inner = vec![*operand];
                while let Some(y) = inner.pop() {
                    match body.expr(y) {
                        Expr::Binary {
                            op: BinOp::And,
                            lhs,
                            rhs,
                        } => inner.extend([*lhs, *rhs]),
                        _ => out.when_false.extend(is_asked(y)),
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// **The predicates with no parameters each command a handler sends
/// requires**, as (command, predicate): the deployment's and the program's.
fn required(
    hirs: &[&Hir],
    ws: &Workspace,
    unit: usize,
    body: &Body,
    handler: ExprId,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in body.walk_from(handler) {
        let Expr::Call { callee, .. } = body.expr(e) else {
            continue;
        };
        let path = crate::infer::path_of(body, *callee);
        let def = match ws.resolve_path_in(unit, Namespace::Term, &path) {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
            _ => continue,
        };
        let Some(command) = crate::resolve::declaration(hirs, def) else {
            continue;
        };
        if command.kind != DeclKind::Command {
            continue;
        }
        let Some(requires) = command.policy("requires") else {
            continue;
        };
        let Ok(predicates) = crate::policy::predicates(&requires.value) else {
            continue;
        };
        // `Anyone` holds for every reader, so no page asks it (the requires
        // ADR): a control whose command anyone may run needs no guard.
        for p in predicates
            .into_iter()
            .filter(|p| p.arguments.is_empty() && p.name != crate::policy::ANYONE)
        {
            if !out.iter().any(|(c, q)| c == &command.name && q == &p.name) {
                out.push((command.name.clone(), p.name));
            }
        }
    }
    out
}
