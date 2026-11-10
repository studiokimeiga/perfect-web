//! **HIR → Backend IR.**
//!
//! E10-A's first stage. Architect ruling, 2026-08-09:
//!
//! > For the first slice, support only the language constructs `add_to_cart`
//! > genuinely needs […] Unsupported constructs should produce an explicit
//! > "backend unsupported" diagnostic, never silently lower differently.
//!
//! and the rule that decides what this file is allowed to compute:
//!
//! > The backend consumes semantics already established upstream. It must not
//! > rediscover effects, capabilities, placement, privacy, or ABI types.
//!
//! # What it consumes rather than derives
//!
//! ```text
//! which calls need authority     the CONTRACT's required_capabilities
//! what a name resolves to        the WORKSPACE
//! what a call returns            SIGNATURES
//! what a type is                 the program's declarations, by DefId
//! ```
//!
//! The one thing it decides is shape: which HIR expression becomes which
//! instruction. Everything a decision depends on arrives already answered, and
//! `no_backend_decision_is_made_from_a_name` is the test that says so.
//!
//! # Why `add_to_cart` gets no special case
//!
//! > Compile exactly one real Pleris command end-to-end first, but do not build
//! > any stage specifically around `add_to_cart`.
//!
//! So the supported set is described as a set of CONSTRUCTS — a call, a name, a
//! literal, a field — and `add_to_cart` is simply a program that stays inside
//! it. `a_declaration_outside_the_supported_set_is_refused_by_name` is what
//! keeps that honest: the refusal names the construct, so widening the backend
//! is a visible act rather than a fixture that started passing.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::ir::{
    BinaryOp, Block, BlockId, BuiltinCase, CallableImport, CapabilityId, Case, Closure, Const,
    EachKind, Function, ImportId, Instr, Intrinsic, Lowering, MatchArm, Operation, Program, Region,
    Shape, Terminator, Type, TypeDef, UnaryOp, ValueId, all_instrs,
};
use crate::contract::ComponentContract;
use crate::hir::{Body, Decl, DeclKind, Expr, ExprId, Hir, Literal, Pattern, Span};
use crate::resolve::{DefId, Namespace, Resolution, Workspace};
use crate::resolved::{Builtin, Primitive, ResolvedType, TypeResolution};
use crate::signatures::Signatures;

/// **Proof that the program being lowered resolved and checked.**
///
/// Architect ruling, 2026-08-10, step 10: *enforce resolved/checked program
/// input before E10.*
///
/// The reason is E10-A's founding finding. `examples/store/app.pw` called
/// `current_session()` and imported nothing, every upstream analysis produced
/// an answer for it — no effects, no label, no constraint, no capability — and
/// each of those answers is what a harmless call produces too. The backend was
/// the first consumer for which the absence was fatal, and it found out by
/// failing to emit rather than by being told.
///
/// So the precondition is carried by a **type with a private field**. The only
/// public way to obtain a `Checked` is `program::with_semantic_program`,
/// which constructs its own resolution/signature context, and `program`
/// takes the proof it lends. A caller cannot hand the backend an
/// unresolved program by forgetting a step: there is no step to forget.
///
/// It is deliberately not a `bool` on `Context`, and not a documented
/// convention. Both are things a caller can be wrong about, which is the shape
/// of every entry in `docs/RISK_QUEUE.md`.
///
/// A caller cannot combine source with a foreign resolution/signature context:
/// ```compile_fail
/// use pw_core::{check::Unit, backend::lower::{Checked, Context}};
/// fn forge<'a>(units: &'a [Unit], context: Context<'a>) {
///     let _ = Checked::of(units, context);
/// }
/// ```
pub struct Checked<'a> {
    cx: Context<'a>,
    units: &'a [crate::check::Unit],
    pages: std::cell::OnceCell<crate::page_values::Analysis>,
    privacy: std::rc::Rc<crate::check::PrivacyFacts>,
}

impl<'a> Checked<'a> {
    pub(crate) fn diagnose_inferred(
        units: &'a [crate::check::Unit],
        cx: Context<'a>,
        inference: &crate::effects::Inference<'_>,
    ) -> Result<(Checked<'a>, crate::program::Diagnostics), crate::program::Diagnostics> {
        assert!(
            units.len() == cx.hirs.len()
                && units
                    .iter()
                    .zip(cx.hirs)
                    .all(|(unit, hir)| std::ptr::eq(&unit.hir, *hir)),
            "the checked units and the lowering context describe different programs"
        );
        let syntax: crate::program::Diagnostics = units
            .iter()
            .map(|u| {
                (
                    u.path.clone(),
                    pw_syntax::parse_tree(&u.src)
                        .errors
                        .into_iter()
                        .map(crate::check::syntax_error)
                        .collect(),
                )
            })
            .collect();
        if syntax.iter().any(|(_, ds)| !ds.is_empty()) {
            return Err(syntax);
        }
        let (diagnostics, privacy) =
            crate::check::check_inferred_prepared(units, cx.hirs, cx.ws, cx.sigs, inference);
        if diagnostics
            .iter()
            .any(|(_, ds)| ds.iter().any(|d| d.is_error()))
        {
            Err(diagnostics)
        } else {
            Ok((
                Checked {
                    cx,
                    units,
                    pages: std::cell::OnceCell::new(),
                    privacy: std::rc::Rc::new(privacy),
                },
                diagnostics,
            ))
        }
    }
    pub fn context(&self) -> &Context<'a> {
        &self.cx
    }
    pub(crate) fn with_contracts<'b>(&'b self, contracts: &'b [ComponentContract]) -> Checked<'b> {
        Checked {
            cx: Context {
                hirs: self.cx.hirs,
                ws: self.cx.ws,
                sigs: self.cx.sigs,
                contracts,
            },
            units: self.units,
            pages: std::cell::OnceCell::from(self.pages().clone()),
            privacy: std::rc::Rc::clone(&self.privacy),
        }
    }
    pub(crate) fn privacy(&self) -> &crate::check::PrivacyFacts {
        &self.privacy
    }
    pub(crate) fn set_pages(&self, pages: crate::page_values::Analysis) {
        assert!(
            self.pages.set(pages).is_ok(),
            "page analysis was already prepared"
        );
    }
    pub(crate) fn pages(&self) -> &crate::page_values::Analysis {
        self.pages
            .get_or_init(|| crate::page_values::analyze(self.cx.hirs, self.cx.ws, self.cx.sigs))
    }
    pub(crate) fn sources(&self) -> Vec<&str> {
        self.units.iter().map(|u| u.src.as_str()).collect()
    }
}

/// What the lowering needs from upstream, gathered once.
pub struct Context<'a> {
    pub hirs: &'a [&'a Hir],
    pub ws: &'a Workspace,
    pub sigs: &'a Signatures,
    /// The contracts, which already decided which capabilities each declaration
    /// requires. **Read, never re-derived.**
    pub contracts: &'a [ComponentContract],
}

/// **The callables this program imports, with the ABI to call each.**
///
/// One per `ImportId` the IR actually references, and the signature comes from
/// the CALLEE's declaration — not from a call site, and not from the capability.
/// Architect ruling, 2026-08-20:
///
/// > A capability authorizes an operation. It does not identify the operation.
///
/// `Carts.add` and `Carts.clear` share `database.write<Carts>` and have
/// different ABIs, which is what made the previous capability-keyed model
/// unencodable. Here they are two imports, both valid, each with its own
/// signature and both requiring the same authority.
///
/// `required_capabilities` is a SET and may be empty: an operation can need
/// several authorities, one authority can serve many operations, and an
/// operation can be placement-constrained while needing none.
fn host_imports(cx: &Context<'_>, p: &Program) -> Vec<CallableImport> {
    // Every import the lowered code actually calls, inside a match arm or an
    // `if` as much as at the top of a body.
    let mut wanted: BTreeSet<ImportId> = BTreeSet::new();
    for f in &p.functions {
        for b in &f.blocks {
            for i in all_instrs(&b.instrs) {
                if let Instr::ImportCall { import, .. } = i {
                    wanted.insert(import.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for (unit, hir) in cx.hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            let def = crate::resolve::DefId { unit, decl: id.0 };
            // **The one canonical definition.** `contract.rs` calls the same
            // function on the same declaration, so the artifact a host reads
            // and the IR the encoder consumes cannot disagree about a
            // callable's ABI or its authority.
            let Some(signature) = cx.sigs.by_def(def) else {
                continue;
            };
            let Some(callable) = crate::backend::callable_of(
                decl,
                def,
                signature,
                |ty| match ty_resolved(cx.sigs, ty, &decl.name_span) {
                    Lowering::Lowered(t) => Some(t),
                    _ => None,
                },
                // The CONTRACT's answer, not a second derivation: whether an
                // effect needs authority is the ontology's, and the contracts
                // already asked. Read off the capability sets they carry.
                |effect| {
                    cx.contracts
                        .iter()
                        .flat_map(|c| c.imports.iter())
                        .flat_map(|i| i.capabilities.iter())
                        .find(|c| c.name() == effect)
                        .cloned()
                },
            ) else {
                continue;
            };
            if !wanted.contains(&callable.id) {
                continue;
            }
            out.push(callable);
        }
    }
    // **And the invalidations' functions** (ADR-0209): one for each query a
    // command invalidates, taking the query's parameters and answering
    // nothing, under the outbox's authority.
    let outbox = cx
        .contracts
        .iter()
        .flat_map(|c| c.imports.iter())
        .flat_map(|i| i.capabilities.iter())
        .find(|c| c.name() == crate::signatures::OUTBOX_WRITE)
        .cloned();
    for (unit, hir) in cx.hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            let def = crate::resolve::DefId { unit, decl: id.0 };
            let Some(signature) = cx.sigs.by_def(def) else {
                continue;
            };
            let invalidable = matches!(
                decl.kind,
                crate::hir::DeclKind::Query
                    | crate::hir::DeclKind::Subscription
                    | crate::hir::DeclKind::Resource
            );
            if !invalidable {
                continue;
            }
            // Each of its functions the code calls, by the positions its key
            // leaves to every value (ADR-0256): the rest are its parameters.
            let shapes: Vec<(ImportId, Vec<usize>)> = wanted
                .iter()
                .filter(|w| w.interface == crate::backend::INVALIDATIONS_INTERFACE)
                .filter_map(|w| {
                    let every = crate::backend::invalidation_shape(&signature.path, &w.name)?;
                    Some((w.clone(), every))
                })
                .collect();
            for (import, every) in shapes {
                let params: Option<Vec<Type>> = signature
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !every.contains(i))
                    .map(|(_, p)| {
                        match ty_resolved(cx.sigs, p.as_ref()?.resolved()?, &decl.name_span) {
                            Lowering::Lowered(t) => Some(t),
                            _ => None,
                        }
                    })
                    .collect();
                let Some(params) = params else {
                    continue;
                };
                out.push(CallableImport {
                    id: import,
                    callee: def,
                    binding: crate::backend::ir::ImportBinding::PlatformHost,
                    signature: crate::backend::ir::BackendSignature {
                        params,
                        result: Type::Unit,
                    },
                    required_capabilities: outbox.iter().cloned().map(CapabilityId).collect(),
                });
            }
        }
    }
    // **And the reads' functions** (ADR-0277): one for each resource a
    // materialization's body reads, taking its parameters and answering its
    // value, the platform's to answer, under no authority of the reader's: R
    // holds its own.
    for (unit, hir) in cx.hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            let def = crate::resolve::DefId { unit, decl: id.0 };
            let Some(signature) = cx.sigs.by_def(def) else {
                continue;
            };
            let import = crate::backend::read_binding(&signature.path);
            if !wanted.contains(&import) {
                continue;
            }
            let params: Option<Vec<Type>> = signature
                .params
                .iter()
                .map(
                    |p| match ty_resolved(cx.sigs, p.as_ref()?.resolved()?, &decl.name_span) {
                        Lowering::Lowered(t) => Some(t),
                        _ => None,
                    },
                )
                .collect();
            let result = signature
                .returns
                .as_ref()
                .and_then(|r| r.resolved())
                .and_then(|r| match ty_resolved(cx.sigs, r, &decl.name_span) {
                    Lowering::Lowered(Type::Result(ok, _)) => Some(*ok),
                    Lowering::Lowered(t) => Some(t),
                    _ => None,
                });
            let (Some(params), Some(result)) = (params, result) else {
                continue;
            };
            out.push(CallableImport {
                id: import,
                callee: def,
                binding: crate::backend::ir::ImportBinding::PlatformHost,
                signature: crate::backend::ir::BackendSignature { params, result },
                required_capabilities: Vec::new(),
            });
        }
    }
    // **And the schedules' functions** (the scheduled-commands track): for
    // each command a body schedules or cancels, `-at` and `-after` take its
    // parameters and then the time, as the `Int` it is, and `-cancel` its
    // parameters; each answers nothing, under `clock.schedule`.
    let scheduling = cx
        .contracts
        .iter()
        .flat_map(|c| c.imports.iter())
        .flat_map(|i| i.capabilities.iter())
        .find(|c| c.name() == crate::signatures::CLOCK_SCHEDULE)
        .cloned();
    for (unit, hir) in cx.hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != crate::hir::DeclKind::Command {
                continue;
            }
            let def = crate::resolve::DefId { unit, decl: id.0 };
            let Some(signature) = cx.sigs.by_def(def) else {
                continue;
            };
            let params: Option<Vec<Type>> = signature
                .params
                .iter()
                .map(
                    |p| match ty_resolved(cx.sigs, p.as_ref()?.resolved()?, &decl.name_span) {
                        Lowering::Lowered(t) => Some(t),
                        _ => None,
                    },
                )
                .collect();
            let Some(params) = params else {
                continue;
            };
            for how in crate::backend::SCHEDULE_HOWS {
                let import = crate::backend::schedule_binding(&signature.path, how);
                if !wanted.contains(&import) {
                    continue;
                }
                let mut params = params.clone();
                if how != "cancel" {
                    params.push(Type::Int);
                }
                out.push(CallableImport {
                    id: import,
                    callee: def,
                    binding: crate::backend::ir::ImportBinding::PlatformHost,
                    signature: crate::backend::ir::BackendSignature {
                        params,
                        result: Type::Unit,
                    },
                    required_capabilities: scheduling.iter().cloned().map(CapabilityId).collect(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Lower one declaration.
pub fn function(cx: &Context<'_>, unit: usize, decl: &Decl, span: Span) -> Lowering<Function> {
    let def = match decl_def(cx, unit, decl) {
        Some(d) => d,
        None => {
            return Lowering::Blocked {
                why: format!("`{}` has no resolved identity", decl.name),
                span,
            };
        }
    };

    let Some(body_id) = decl.body else {
        return Lowering::Unsupported {
            construct: "a declaration with no body",
            span,
            reason: "there is nothing to lower; a bodyless declaration is an \
                     interface, and E10-A compiles implementations"
                .to_string(),
        };
    };

    // The contract for this declaration, by the id `contracts()` built. Its
    // `required_capabilities` are what this function may call across — derived
    // once, upstream, from inference.
    let component_id = component_id(cx, unit, decl);
    let capabilities: Vec<CapabilityId> = cx
        .contracts
        .iter()
        .find(|c| c.component_id == component_id)
        .map(|c| {
            c.required_capabilities
                .iter()
                .cloned()
                .map(CapabilityId)
                .collect()
        })
        .unwrap_or_default();

    let internal = RefCell::new(Internal::default());
    let mut f = Lower {
        cx,
        unit,
        next_value: 0,
        instrs: Vec::new(),
        locals: BTreeMap::new(),
        lexicals: BTreeMap::new(),
        types: BTreeMap::new(),
        inlining: vec![def],
        subst: BTreeMap::new(),
        internal: &internal,
        vars: BTreeSet::new(),
        in_lambda: 0,
        ret: Type::Unit,
        captured: BTreeMap::new(),
        handler: false,
        signals: BTreeMap::new(),
        reads: decl.kind == DeclKind::Materialize,
    };

    // Parameters first, so a body naming one finds it.
    let Some(signature) = cx.sigs.by_def(def) else {
        return Lowering::Blocked {
            why: "missing resolved signature".into(),
            span,
        };
    };
    let mut params = Vec::new();
    for (index, p) in decl.params.iter().enumerate() {
        let Some(declared) = signature.params.get(index).and_then(Option::as_ref) else {
            return Lowering::Unsupported {
                construct: "an unannotated parameter",
                span: p.span.clone(),
                reason: format!("`{}` has no declared type, and the ABI needs one", p.name),
            };
        };
        let ty = match ty_resolution(cx.sigs, declared, &p.span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let v = f.fresh();
        f.locals.insert(p.name.clone(), v);
        f.types.insert(v, ty.clone());
        params.push((v, ty));
    }

    // **A map or set from outside is sorted on entry** (ADR-0057, ruling
    // 0057-c, ADR-0259): by its keys, stably, and a key twice stops the
    // invocation. A binary search over anything else answers wrongly. One
    // inside another value is refused: nothing sorts it.
    for (p, (v, ty)) in decl.params.iter().zip(&params) {
        let op = match ty {
            Type::Map(..) => Intrinsic::MapCheck,
            Type::Set(..) => Intrinsic::SetCheck,
            t if holds_collection(cx, t, &mut Vec::new()) => {
                return Lowering::Unsupported {
                    construct: "a map or set inside a parameter's value",
                    span: p.span.clone(),
                    reason: format!(
                        "`{}` would reach the body unchecked; only a map or set that is \
                         itself the parameter is checked on entry (ADR-0057)",
                        p.name
                    ),
                };
            }
            _ => continue,
        };
        let checked = f.fresh();
        f.push(Instr::Intrinsic {
            result: checked,
            op,
            args: vec![*v],
            ty: ty.clone(),
        });
        f.types.insert(checked, ty.clone());
        f.locals.insert(p.name.clone(), checked);
    }

    let ret = match &signature.returns {
        Some(resolution) => match ty_resolution(cx.sigs, resolution, &span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        },
        None => Type::Unit,
    };
    f.ret = ret.clone();

    let body = cx.hirs[unit].body(body_id);
    // **A command's events and the entries it invalidates, computed by the
    // command** (ADR-0208, ADR-0209). Each `emits` and `invalidates` key's
    // values are evaluated before its body, on what it was given, in the
    // order they are written, and handed to the platform, which commits them
    // with the command's writes or not at all. The server never reads a
    // key's text.
    for policy in decl
        .policies
        .iter()
        .filter(|p| p.name == "emits" || p.name == "invalidates")
    {
        for key in &policy.keys {
            match f.outboxed(body, &policy.name, key) {
                Lowering::Lowered(()) => {}
                other => return other.map(|_| unreachable!()),
            }
        }
    }
    let result = match f.expr(body, body.root, Some(&ret)) {
        Lowering::Lowered(v) => v,
        other => return other.map(|_| unreachable!()),
    };

    let instrs = std::mem::take(&mut f.instrs);
    drop(f);
    let internal = internal.into_inner();
    Lowering::Lowered(Function {
        def,
        export: decl.name.clone(),
        params,
        ret,
        blocks: vec![Block {
            id: BlockId(0),
            instrs,
            terminator: Terminator::Return(result),
        }],
        capabilities,
        instance: Vec::new(),
        callees: internal.done,
        closures: internal
            .closures
            .into_iter()
            .map(|c| c.expect("every closure slot is filled when its code lowers"))
            .collect(),
    })
}

/// **A resumable handler's body, lowered** (ADR-0058): a function of the
/// values it captured, one for each path the document carries for it, in the
/// order `resume::capture_paths` gives them. A command it calls is
/// `Instr::Command`, awaited by its module in the browser; everything else is
/// what a query's body lowers to.
pub fn handler(
    cx: &Context<'_>,
    unit: usize,
    decl_id: crate::hir::DeclId,
    lambda: ExprId,
    span: Span,
) -> Lowering<(Function, Vec<String>, bool)> {
    let hir = cx.hirs[unit];
    let decl = hir.decl(decl_id);
    let Some(def) = decl_def(cx, unit, decl) else {
        return Lowering::Blocked {
            why: format!("`{}` has no resolved identity", decl.name),
            span,
        };
    };
    let Some(body_id) = decl.body else {
        return Lowering::Blocked {
            why: format!("`{}` holds a handler and has no body", decl.name),
            span,
        };
    };
    let body = hir.body(body_id);
    // A lambda's body, or a declaration named as the handler, which is
    // `(e) => save(e)`, or `() => save()` for one that takes nothing
    // (ADR-0199, ADR-0195's ruling 12).
    let (inner, takes): (Option<ExprId>, usize) = match body.expr(lambda) {
        Expr::Lambda {
            params,
            body: inner,
            ..
        } => (
            Some(*inner),
            crate::values::lambda_params(body, params).len(),
        ),
        Expr::Name(_) | Expr::Field { .. } => {
            let types = crate::infer::Types::of_decl(cx.sigs, hir, decl_id, body);
            match types.named_handler(body, lambda) {
                Some(sig) => (None, sig.params.len()),
                None => {
                    return Lowering::Blocked {
                        why: "a handler named by what is no function or command".to_string(),
                        span,
                    };
                }
            }
        }
        _ => {
            return Lowering::Blocked {
                why: "a handler identity that is neither a lambda nor a name".to_string(),
                span,
            };
        }
    };
    let param = match body.expr(lambda) {
        Expr::Lambda { params, .. } => crate::values::lambda_params(body, params).first().copied(),
        _ => None,
    };
    // The event, read in the listener and given to the module (ADR-0138):
    // an `on:` handler's one parameter, typed by its event.
    let event = match takes {
        0 => None,
        1 => {
            let Some(ty) = crate::resume::handler_events(body)
                .into_iter()
                .find(|(_, e)| *e == lambda)
                .and_then(|(event, _)| crate::annotations::event_type(cx.sigs, &event).cloned())
            else {
                return Lowering::Unsupported {
                    construct: "a handler parameter that is not an event",
                    span,
                    reason: "only an `on:` attribute's handler is given anything: its event"
                        .to_string(),
                };
            };
            let name = match param.map(|p| body.pat(p)) {
                Some(crate::hir::Pattern::Bind { name, .. }) => Some(name.clone()),
                Some(crate::hir::Pattern::Wild) | None => None,
                _ => {
                    return Lowering::Unsupported {
                        construct: "an event taken apart in a handler's parameter",
                        span,
                        reason: "name the event, `(e) =>`, and read its fields".to_string(),
                    };
                }
            };
            Some((name, ty))
        }
        _ => {
            return Lowering::Unsupported {
                construct: "a handler with several parameters",
                span,
                reason: "an event gives its handler one record (ADR-0131)".to_string(),
            };
        }
    };
    // What each captured path is: its root's type, then each field's. The
    // root is typed by the binding its name means where the descriptor
    // writes it (ADR-0063).
    let types = crate::infer::Types::of_decl(cx.sigs, hir, decl_id, body);
    let roots = crate::resume::capture_roots(body, types.lexical(), lambda);
    let internal = RefCell::new(Internal {
        in_handler: true,
        ..Internal::default()
    });
    let mut f = Lower {
        cx,
        unit,
        next_value: 0,
        instrs: Vec::new(),
        locals: BTreeMap::new(),
        lexicals: BTreeMap::new(),
        types: BTreeMap::new(),
        inlining: vec![def],
        subst: BTreeMap::new(),
        internal: &internal,
        vars: BTreeSet::new(),
        in_lambda: 0,
        ret: Type::Unit,
        captured: BTreeMap::new(),
        handler: true,
        signals: BTreeMap::new(),
        reads: false,
    };
    // The page's signals, each by the type its declaration writes
    // (ADR-0130). A signal is a `let` of the page's body, so the lexical
    // rules that would hide one behind a local do so here too: a local of
    // the same name is in `locals`, and wins.
    for (name, _, declared) in crate::page_values::signals_of(body) {
        let Expr::Let { ty: Some(t), .. } = body.expr(declared) else {
            continue;
        };
        let Some(ty) = f.annotation(body, *t, &span) else {
            return Lowering::Unsupported {
                construct: "a signal whose type the backend cannot lower",
                span,
                reason: format!("`{name}`'s written type"),
            };
        };
        f.signals.insert(name, ty);
    }
    // The module signals it names, provided to its body (ADR-0144), each by
    // the type its declaration writes. The browser says which instance each
    // is, where the handler is attached.
    for (name, signal) in crate::template_ir::signals_named(cx.hirs, cx.ws, unit, body) {
        if f.signals.contains_key(&name) {
            continue;
        }
        let resolved = cx.sigs.signal_type(signal).and_then(|t| t.resolved());
        let ty = match resolved.map(|t| backend_type(cx, t, &span)) {
            Some(Lowering::Lowered(t)) => t,
            _ => {
                return Lowering::Unsupported {
                    construct: "a signal whose type the backend cannot lower",
                    span,
                    reason: format!("`{name}`'s declared type"),
                };
            }
        };
        f.signals.insert(name, ty);
    }
    let (mut captured, mut paths) = (Vec::new(), Vec::new());
    for path in crate::resume::capture_paths(body, types.lexical(), lambda) {
        let mut parts = path.split('.');
        let root = parts.next().unwrap_or_default();
        let mut ty = roots.get(root).and_then(|e| types.of(body, *e));
        for field in parts {
            ty = ty.and_then(|t| types.member(&t, field).and_then(|s| s.result().cloned()));
        }
        let Some(ty) = ty else {
            return Lowering::Unsupported {
                construct: "a captured value whose type nothing states",
                span,
                reason: format!("`{path}` has no type the handler can decode it by"),
            };
        };
        let ty = match ty_resolved(cx.sigs, &ty, &span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let v = f.fresh();
        f.types.insert(v, ty.clone());
        f.captured.insert(path.clone(), v);
        internal
            .borrow_mut()
            .captured_roots
            .insert(path.split('.').next().unwrap_or_default().to_string());
        captured.push((v, ty));
        paths.push(path);
    }
    // The event, after the captures: the module decodes it from what the
    // listener read (ADR-0138).
    let takes_event = event.is_some();
    let mut event_value = None;
    if let Some((name, ty)) = event {
        let ty = match ty_resolved(cx.sigs, &ty, &span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let v = f.fresh();
        f.types.insert(v, ty.clone());
        if let Some(name) = name {
            f.locals.insert(name, v);
        }
        captured.push((v, ty));
        event_value = Some(v);
    }
    let lowered = match inner {
        Some(inner) => f.expr(body, inner, None),
        // The declaration, called with the event where it takes one.
        None => f.call(body, lambda, &[], event_value, None, span.clone()),
    };
    let result = match lowered {
        Lowering::Lowered(v) => v,
        other => return other.map(|_| unreachable!()),
    };
    let ret = f.types.get(&result).cloned().unwrap_or(Type::Unit);
    let instrs = std::mem::take(&mut f.instrs);
    drop(f);
    let internal = internal.into_inner();
    let function = Function {
        def,
        export: crate::contract::component_id(hir, decl_id),
        params: captured,
        ret,
        blocks: vec![Block {
            id: BlockId(0),
            instrs,
            terminator: Terminator::Return(result),
        }],
        capabilities: Vec::new(),
        instance: Vec::new(),
        callees: internal.done,
        closures: internal
            .closures
            .into_iter()
            .map(|c| c.expect("every closure slot is filled when its code lowers"))
            .collect(),
    };
    Lowering::Lowered((function, paths, takes_event))
}

/// **A handler's program** (ADR-0058): its function, and the nominal types
/// it reaches, which its module decodes captures and encodes arguments by.
pub(crate) fn handler_program(cx: &Context<'_>, function: Function) -> Program {
    let mut p = Program {
        functions: vec![function],
        ..Program::default()
    };
    p.types = type_defs(cx, &p);
    p
}

/// **Several functions' program** (ADR-0122): a speculation module's, whose
/// decoders and functions read the nominal types they reach.
pub(crate) fn program_of(cx: &Context<'_>, functions: Vec<Function>) -> Program {
    let mut p = Program {
        functions,
        ..Program::default()
    };
    p.types = type_defs(cx, &p);
    p
}

/// **An expression of a declaration's body, as a pure function of named
/// values** (ADR-0122): an optimistic transition of its binder and its
/// command's parameters, or a template hole of the binding it reads. Each
/// name in `inputs` is a parameter, in order, of the type given; the
/// expression sees those and nothing else of the body. A command it would
/// call is refused: a speculation runs before the round trip, and performs
/// nothing (ADR-0025).
pub fn pure_expr(
    cx: &Context<'_>,
    unit: usize,
    decl_id: crate::hir::DeclId,
    root: ExprId,
    inputs: &[(String, Type)],
    export: &str,
    span: Span,
) -> Lowering<Function> {
    pure_expr_as(cx, unit, decl_id, root, inputs, None, export, span)
}

/// [`pure_expr`], where the expression must be of type `expected`: a
/// signal's first value, whose declaration writes its type (ADR-0130), so
/// `None` and `[]` have one.
#[allow(clippy::too_many_arguments)]
pub fn pure_expr_as(
    cx: &Context<'_>,
    unit: usize,
    decl_id: crate::hir::DeclId,
    root: ExprId,
    inputs: &[(String, Type)],
    expected: Option<&Type>,
    export: &str,
    span: Span,
) -> Lowering<Function> {
    let hir = cx.hirs[unit];
    let decl = hir.decl(decl_id);
    let Some(def) = decl_def(cx, unit, decl) else {
        return Lowering::Blocked {
            why: format!("`{}` has no resolved identity", decl.name),
            span,
        };
    };
    let Some(body_id) = decl.body else {
        return Lowering::Blocked {
            why: format!("`{}` has no body to read the expression from", decl.name),
            span,
        };
    };
    let body = hir.body(body_id);
    let internal = RefCell::new(Internal::default());
    let mut f = Lower {
        cx,
        unit,
        next_value: 0,
        instrs: Vec::new(),
        locals: BTreeMap::new(),
        lexicals: BTreeMap::new(),
        types: BTreeMap::new(),
        inlining: vec![def],
        subst: BTreeMap::new(),
        internal: &internal,
        vars: BTreeSet::new(),
        in_lambda: 0,
        ret: Type::Unit,
        captured: BTreeMap::new(),
        handler: false,
        signals: BTreeMap::new(),
        reads: false,
    };
    let mut params = Vec::new();
    for (name, ty) in inputs {
        let v = f.fresh();
        f.locals.insert(name.clone(), v);
        f.types.insert(v, ty.clone());
        params.push((v, ty.clone()));
    }
    let result = match f.expr(body, root, expected) {
        Lowering::Lowered(v) => v,
        other => return other.map(|_| unreachable!()),
    };
    let ret = f.types.get(&result).cloned().unwrap_or(Type::Unit);
    let instrs = std::mem::take(&mut f.instrs);
    drop(f);
    let internal = internal.into_inner();
    Lowering::Lowered(Function {
        def,
        export: export.to_string(),
        params,
        ret,
        blocks: vec![Block {
            id: BlockId(0),
            instrs,
            terminator: Terminator::Return(result),
        }],
        capabilities: Vec::new(),
        instance: Vec::new(),
        callees: internal.done,
        closures: internal
            .closures
            .into_iter()
            .map(|c| c.expect("every closure slot is filled when its code lowers"))
            .collect(),
    })
}

/// **A computed part's function** (ADR-0226, ruling 0073-a): its expression,
/// lifted into a pure function of the one value it reads, typed as the
/// expression reads it there. The browser's encoder and a component's lower
/// the same function.
pub fn hole(cx: &Context<'_>, d: &crate::page_values::Derived) -> Lowering<Function> {
    let hir = cx.hirs[d.origin.0];
    let decl = hir.decl(d.origin.1);
    let span = match decl.body {
        Some(b) => hir.body(b).expr_span(d.expr),
        None => hir.decl_span(d.origin.1),
    };
    // Each input, in order (track `device-store`: several, where the browser
    // computes it from several values it holds).
    let mut inputs = Vec::new();
    for (name, ty) in &d.inputs {
        match backend_type(cx, ty, &span) {
            Lowering::Lowered(t) => inputs.push((name.clone(), t)),
            other => return other.map(|_| unreachable!()),
        }
    }
    pure_expr(
        cx,
        d.origin.0,
        d.origin.1,
        d.expr,
        &inputs,
        &d.export(),
        span,
    )
}

/// **A map's key** (ADR-0248, ruling 0057-a): an `Int`, a `String`, a
/// `Bool`, or an opaque type over one, which crosses and compares as its
/// representation (ADR-0033 §4).
fn ordered_key(sigs: &Signatures, t: &Type) -> bool {
    match t {
        Type::Int | Type::Str | Type::Bool => true,
        Type::Nominal(def, _) => sigs
            .type_decl(*def)
            .and_then(|d| d.representation.as_ref())
            .is_some_and(|r| {
                matches!(
                    ty_resolution(sigs, r, &Span::default()),
                    Lowering::Lowered(Type::Int | Type::Str | Type::Bool)
                )
            }),
        _ => false,
    }
}

/// A declared type, as the backend's [`Type`] (ADR-0122).
pub(crate) fn backend_type(cx: &Context<'_>, ty: &ResolvedType, span: &Span) -> Lowering<Type> {
    ty_resolved(cx.sigs, ty, span)
}

/// Can a browser send a value of this type to a command? A primitive, or an
/// opaque type over one, which crosses as its representation (ADR-0033 §4); a
/// list or a record of what it can send (ADR-0172).
fn sendable(cx: &Context<'_>, t: &Type) -> bool {
    sendable_within(cx, t, &mut BTreeSet::new())
}

/// [`sendable`], each record on the way remembered, so one that holds itself
/// through a list is not walked for ever.
fn sendable_within(cx: &Context<'_>, t: &Type, on_the_way: &mut BTreeSet<DefId>) -> bool {
    match t {
        Type::Int | Type::Float | Type::Bool | Type::Str => true,
        // A list of what a browser can send (ADR-0172).
        Type::List(t) => sendable_within(cx, t, on_the_way),
        // **A case of what a browser can send** (the integrator's,
        // 2026-10-13; W9's finding in NEXT 6b): `Option`'s and `Result`'s, a
        // case as its name and payload, `{ $case, value }`.
        Type::Option(t) => sendable_within(cx, t, on_the_way),
        Type::Result(t, e) => {
            sendable_within(cx, t, on_the_way) && sendable_within(cx, e, on_the_way)
        }
        Type::Nominal(def, _) => {
            let Some(d) = cx.sigs.type_decl(*def) else {
                return false;
            };
            if let Some(r) = d.representation.as_ref() {
                return matches!(
                    ty_resolution(cx.sigs, r, &Span::default()),
                    Lowering::Lowered(Type::Int | Type::Float | Type::Bool | Type::Str)
                );
            }
            // And a declared case's, each field of each case sent. A type
            // that contains itself through a case is not: a host reads no
            // such case from a browser.
            if let Some(cases) = d.variants.as_ref() {
                if !on_the_way.insert(*def) {
                    return false;
                }
                let all = cases.iter().all(|(_, fields)| {
                    fields.iter().all(|f| {
                        matches!(
                            ty_resolution(cx.sigs, f, &Span::default()),
                            Lowering::Lowered(field) if sendable_within(cx, &field, on_the_way)
                        )
                    })
                });
                on_the_way.remove(def);
                return all;
            }
            // **A record of what a browser can send** (ADR-0172): the item a
            // page showed, which `add_to_cart` makes its new line from.
            let Some(fields) = d.record.as_ref() else {
                return false;
            };
            if !on_the_way.insert(*def) {
                return true;
            }
            // A field crosses by its WIT name, and a host finds it in what a
            // browser wrote by that name with each `-` a `_`. A name that
            // would come back as another, `opensMinute` as `opens_minute`,
            // is not sent: the host would not find it.
            let all = fields.iter().all(|(name, f)| {
                crate::wit::ident(name).replace('-', "_") == *name
                    && matches!(
                        ty_resolution(cx.sigs, f, &Span::default()),
                        Lowering::Lowered(field) if sendable_within(cx, &field, on_the_way)
                    )
            });
            on_the_way.remove(def);
            all
        }
        _ => false,
    }
}

/// **The functions compiled beside one export** (ADR-0050), shared by every
/// `Lower` working on it: each instance begun, so a recursive call finds its
/// own, and each one finished.
#[derive(Default)]
struct Internal {
    started: BTreeSet<(DefId, Vec<Type>)>,
    done: Vec<Function>,
    /// The function values' code (ADR-0052), by index. A slot is reserved
    /// before its code is lowered, so a closure made inside another's code
    /// has its own index.
    closures: Vec<Option<Closure>>,
    /// The export is a resumable handler (ADR-0058). A function lowered
    /// beside it, or a function value's code, is not its body, and a command
    /// called in one is refused.
    in_handler: bool,
    /// The names at the roots of the handler's captured paths: read in a
    /// function value, one is refused, since its code does not receive them.
    captured_roots: BTreeSet<String>,
}

/// **One declaration, compiled beside the export that reaches it**, at the
/// type arguments `instance` (ADR-0050). Its body is lowered as an export's
/// is, with its own values; a call inside it to a declaration it is already
/// lowering is a call, to its own instance.
fn lower_internal(
    cx: &Context<'_>,
    internal: &RefCell<Internal>,
    def: DefId,
    instance: Vec<Type>,
    subst: BTreeMap<(DefId, u32), Type>,
    span: Span,
) -> Lowering<Function> {
    let Some(decl) = crate::resolve::declaration(cx.hirs, def) else {
        return Lowering::Blocked {
            why: "a callee no unit holds".to_string(),
            span,
        };
    };
    let Some(body_id) = decl.body else {
        return Lowering::Unsupported {
            construct: "a call to a declaration with no body",
            span,
            reason: format!("`{}` has neither a body nor a `host` binding", decl.name),
        };
    };
    let Some(sig) = cx.sigs.by_def(def) else {
        return Lowering::Blocked {
            why: format!("`{}` has no resolved signature", decl.name),
            span,
        };
    };
    let mut f = Lower {
        cx,
        unit: def.unit,
        next_value: 0,
        instrs: Vec::new(),
        locals: BTreeMap::new(),
        lexicals: BTreeMap::new(),
        types: BTreeMap::new(),
        inlining: vec![def],
        subst,
        internal,
        vars: BTreeSet::new(),
        in_lambda: 0,
        ret: Type::Unit,
        captured: BTreeMap::new(),
        handler: false,
        signals: BTreeMap::new(),
        reads: false,
    };
    let mut params = Vec::new();
    for (index, p) in decl.params.iter().enumerate() {
        let Some(declared) = sig.params.get(index).and_then(Option::as_ref) else {
            return Lowering::Unsupported {
                construct: "an unannotated parameter",
                span: p.span.clone(),
                reason: format!("`{}` has no declared type", p.name),
            };
        };
        let ty = match f.ty(declared, &p.span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let v = f.fresh();
        f.locals.insert(p.name.clone(), v);
        f.types.insert(v, ty.clone());
        params.push((v, ty));
    }
    let ret = match &sig.returns {
        Some(r) => match f.ty(r, &span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        },
        None => Type::Unit,
    };
    f.ret = ret.clone();
    let body = cx.hirs[def.unit].body(body_id);
    let result = match f.expr(body, body.root, Some(&ret)) {
        Lowering::Lowered(v) => v,
        other => return other.map(|_| unreachable!()),
    };
    if f.types.get(&result) != Some(&ret) {
        return Lowering::Blocked {
            why: format!(
                "`{}` produces a {:?} and declares {ret:?}",
                decl.name,
                f.types.get(&result)
            ),
            span,
        };
    }
    let instrs = std::mem::take(&mut f.instrs);
    Lowering::Lowered(Function {
        def,
        export: decl.name.clone(),
        params,
        ret,
        blocks: vec![Block {
            id: BlockId(0),
            instrs,
            terminator: Terminator::Return(result),
        }],
        capabilities: Vec::new(),
        instance,
        callees: Vec::new(),
        closures: Vec::new(),
    })
}

/// **A lambda's code, compiled as a function** (ADR-0052): its captures,
/// then its parameters, and its body lowered against its result. It lowers
/// under the declarations being lowered where it is made, so a call back
/// into one of them is a call, not an inlining without end.
#[allow(clippy::too_many_arguments)]
fn lower_closure(
    cx: &Context<'_>,
    internal: &RefCell<Internal>,
    owner: DefId,
    unit: usize,
    body: &Body,
    captures: Vec<(String, Type)>,
    params: Vec<(String, Type)>,
    inner: ExprId,
    ret: Type,
    subst: BTreeMap<(DefId, u32), Type>,
    inlining: Vec<DefId>,
    span: Span,
) -> Lowering<Function> {
    let mut f = Lower {
        cx,
        unit,
        next_value: 0,
        instrs: Vec::new(),
        locals: BTreeMap::new(),
        lexicals: BTreeMap::new(),
        types: BTreeMap::new(),
        inlining,
        subst,
        internal,
        vars: BTreeSet::new(),
        in_lambda: 0,
        ret: ret.clone(),
        captured: BTreeMap::new(),
        handler: false,
        signals: BTreeMap::new(),
        reads: false,
    };
    let mut values = Vec::new();
    for (name, ty) in captures.into_iter().chain(params) {
        let v = f.fresh();
        f.locals.insert(name, v);
        f.types.insert(v, ty.clone());
        values.push((v, ty));
    }
    let result = match f.expr(body, inner, Some(&ret)) {
        Lowering::Lowered(v) => v,
        other => return other.map(|_| unreachable!()),
    };
    if f.types.get(&result) != Some(&ret) {
        return Lowering::Blocked {
            why: format!(
                "a lambda produces a {:?} where a {ret:?} is its result",
                f.types.get(&result)
            ),
            span,
        };
    }
    let instrs = std::mem::take(&mut f.instrs);
    Lowering::Lowered(Function {
        def: owner,
        export: "lambda".to_string(),
        params: values,
        ret,
        blocks: vec![Block {
            id: BlockId(0),
            instrs,
            terminator: Terminator::Return(result),
        }],
        capabilities: Vec::new(),
        instance: Vec::new(),
        callees: Vec::new(),
        closures: Vec::new(),
    })
}

/// Lower every component declaration in a checked program.
///
/// Returns the functions that lowered and the refusals, both — a program where
/// half the declarations are outside E10-A's supported set is the ordinary
/// case, and reporting only the successes would make the backend look finished.
/// **Lower a program that checked.**
///
/// Takes `Checked`, not `Context`. See the type's docstring: the precondition
/// is the parameter, so it cannot be skipped.
pub fn program(checked: &Checked<'_>) -> (Program, Vec<Lowering<Function>>) {
    let (program, refusals) = program_by_declaration(checked);
    (program, refusals.into_iter().map(|(_, r)| r).collect())
}

/// The construct a `todo` body is refused as: a declaration its author has
/// not written yet, which is not the backend declining something it could
/// be asked to compile.
pub const PLACEHOLDER: &str = "a `todo` body";

/// [`program`], with each refusal paired with the declaration it refuses, so
/// a caller can say why one component did not build without quoting every
/// other refusal in the program.
pub fn program_by_declaration(
    checked: &Checked<'_>,
) -> (Program, Vec<(DefId, Lowering<Function>)>) {
    let cx = checked.context();
    let mut out = Program::default();
    let mut refusals = Vec::new();
    // And each function a page's template reads as a member (ADR-0125).
    let members = &checked.pages().members;
    for (unit, hir) in cx.hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            let member = members.contains(&DefId { unit, decl: id.0 });
            // And each materialization that derives its value (ADR-0277), a
            // component the host runs to keep it.
            let derives = decl.kind == DeclKind::Materialize && decl.ret.is_some();
            if !member && !derives && !matches!(decl.kind, DeclKind::Command | DeclKind::Query) {
                continue;
            }
            match function(cx, unit, decl, hir.decl_span(id)) {
                Lowering::Lowered(f) => out.functions.push(f),
                other => refusals.push((DefId { unit, decl: id.0 }, other)),
            }
        }
    }
    // And each computed part a host computes (ADR-0226): the function its
    // expression is lifted into.
    for d in &checked.pages().derived {
        match hole(cx, d) {
            Lowering::Lowered(f) => out.functions.push(f),
            other => refusals.push((d.declaration(), other)),
        }
    }
    out.types = type_defs(cx, &out);
    out.imports = host_imports(cx, &out);
    (out, refusals)
}

/// Every nominal type the lowered functions reach, resolved to its shape.
///
/// A field's type comes from the signatures, resolved where the declaration is
/// written. Until 2026-09-25 it was the head of the written type read as a
/// primitive, with `String` for anything else: `redirect: Option<String>` was
/// a `String`. Nothing read this table then. The encoder reads it now, for a
/// record that never crosses the boundary (ADR-0039), so a type that does not
/// resolve leaves its declaration out rather than guessing.
fn type_defs(cx: &Context<'_>, p: &Program) -> Vec<TypeDef> {
    let mut wanted: Vec<(DefId, Vec<Type>)> = Vec::new();
    for f in &p.functions {
        for (_, t) in &f.params {
            wanted.extend(t.nominals());
        }
        wanted.extend(f.ret.nominals());
        for b in &f.blocks {
            for i in all_instrs(&b.instrs) {
                wanted.extend(i.ty().nominals());
            }
        }
    }
    let mut out: Vec<TypeDef> = Vec::new();
    let mut seen: BTreeSet<(DefId, Vec<Type>)> = BTreeSet::new();
    // Transitively: a record's field may be another record. One definition
    // per instance, each field under the instance's arguments (ADR-0062).
    while let Some((def, args)) = wanted.pop() {
        if !seen.insert((def, args.clone())) {
            continue;
        }
        let Some(decl) = crate::resolve::declaration(cx.hirs, def) else {
            continue;
        };
        let at = decl.name_span.clone();
        let subst: BTreeMap<(DefId, u32), Type> = args
            .iter()
            .enumerate()
            .map(|(i, t)| ((def, i as u32), t.clone()))
            .collect();
        let resolved = |r: &TypeResolution| match r.resolved() {
            Some(t) => match ty_resolved_with(cx.sigs, t, &at, &subst) {
                Lowering::Lowered(t) => Some(t),
                _ => None,
            },
            None => None,
        };
        let declared = cx.sigs.type_decl(def);
        let shape = if let Some(rep) = declared.and_then(|t| t.representation.as_ref()) {
            match resolved(rep) {
                Some(t) => Shape::Alias(Box::new(t)),
                None => continue,
            }
        } else if let Some(cases) = declared
            .and_then(|t| t.variants.as_ref())
            .filter(|cases| !cases.is_empty())
        {
            // A sum type's cases, each payload's fields resolved (ADR-0059),
            // whatever their number: a union of one case is a variant too.
            // One that does not resolve leaves the declaration out.
            let mut out_cases = Vec::new();
            let mut whole = true;
            for (name, fields) in cases {
                let mut types = Vec::new();
                for r in fields {
                    match resolved(r) {
                        Some(t) => types.push(t),
                        None => whole = false,
                    }
                }
                out_cases.push((name.clone(), types));
            }
            if !whole {
                continue;
            }
            Shape::Variant { cases: out_cases }
        } else if let Some(fields) = declared.and_then(|t| t.record.as_ref()) {
            let mut out_fields = Vec::new();
            let mut whole = true;
            for (name, r) in fields {
                match resolved(r) {
                    Some(t) => out_fields.push((name.clone(), t)),
                    None => whole = false,
                }
            }
            if !whole {
                continue;
            }
            Shape::Record { fields: out_fields }
        } else {
            continue;
        };
        if let Shape::Record { fields } = &shape {
            for (_, t) in fields {
                wanted.extend(t.nominals());
            }
        }
        if let Shape::Alias(t) = &shape {
            wanted.extend(t.nominals());
        }
        if let Shape::Variant { cases } = &shape {
            for (_, fields) in cases {
                for t in fields {
                    wanted.extend(t.nominals());
                }
            }
        }
        out.push(TypeDef {
            def,
            args,
            name: decl.name.clone(),
            shape,
        });
    }
    out.sort_by(|a, b| (a.def, &a.args).cmp(&(b.def, &b.args)));
    out
}

fn component_id(cx: &Context<'_>, unit: usize, decl: &Decl) -> String {
    let hir = cx.hirs[unit];
    hir.all_decls()
        .find(|(_, d)| std::ptr::eq(*d, decl))
        .map(|(id, _)| crate::contract::component_id(hir, id))
        .unwrap_or_else(|| decl.name.clone())
}

fn decl_def(cx: &Context<'_>, unit: usize, decl: &Decl) -> Option<DefId> {
    cx.hirs[unit]
        .all_decls()
        .find(|(_, d)| std::ptr::eq(*d, decl))
        .map(|(id, _)| DefId { unit, decl: id.0 })
}

struct Lower<'a> {
    cx: &'a Context<'a>,
    unit: usize,
    next_value: u32,
    instrs: Vec<Instr>,
    locals: BTreeMap<String, ValueId>,
    /// Cached canonical lexical views, including bodies currently inlined.
    lexicals: BTreeMap<(usize, crate::hir::DeclId), std::rc::Rc<crate::lexical::Lexical>>,
    /// Every value's type, recorded where the value is made. A `match` asks
    /// what its scrutinee is, and a field access what record it reads.
    types: BTreeMap<ValueId, Type>,
    /// The declarations whose bodies are being lowered, outermost first: the
    /// export, then each callee inlined into it (ADR-0039 §4). A call to one
    /// of them is a recursion, compiled as a call (ADR-0050).
    inlining: Vec<DefId>,
    /// The type arguments of every generic declaration being lowered, by
    /// the parameter's identity: what `T` is in this instance (ADR-0050).
    subst: BTreeMap<(DefId, u32), Type>,
    /// The functions compiled beside this export.
    internal: &'a RefCell<Internal>,
    /// The values that name mutable bindings, `let mut` (ADR-0051): a use
    /// of one reads what it holds there.
    vars: BTreeSet<ValueId>,
    /// How many function values being lowered enclose this point: the
    /// lambdas a list operation runs (ADR-0040). A `return`, a `?` or an
    /// assignment inside one would leave or change something outside it.
    in_lambda: usize,
    /// What the function being lowered returns: what `return` and `?` give.
    ret: Type,
    /// A resumable handler's captures (ADR-0058): each path it reads, by the
    /// value the document carries for it. A read of the path is that value.
    captured: BTreeMap<String, ValueId>,
    /// A handler's body is being lowered: a command it calls is
    /// `Instr::Command`, which its module awaits.
    handler: bool,
    /// **The page's signals a handler reaches, by name** (ADR-0130), with
    /// their types: a read of one is `Instr::SignalGet`, an assignment to one
    /// `Instr::SignalSet`. Empty outside a handler.
    signals: BTreeMap<String, Type>,
    /// **A materialization's body is being lowered** (ADR-0277): a `query
    /// R(..)` in it is a read the host answers, `pw:host/reads`.
    reads: bool,
}

/// **Instantiate a declared type against the type a value has** (ADR-0050):
/// bind each type parameter it mentions to what stands there, and say whether
/// the two agree. `List<T>` against `List<Int>` binds `T` to `Int`.
fn instantiate(
    sigs: &Signatures,
    declared: &ResolvedType,
    actual: &Type,
    subst: &mut BTreeMap<(DefId, u32), Type>,
) -> bool {
    if let Some(key) = declared.parameter_binding() {
        return match subst.get(&key) {
            Some(t) => t == actual,
            None => {
                subst.insert(key, actual.clone());
                true
            }
        };
    }
    if let Some(p) = declared.as_primitive() {
        return matches!(
            (p, actual),
            (Primitive::Int, Type::Int)
                | (Primitive::Float, Type::Float)
                | (Primitive::Bool, Type::Bool)
                | (Primitive::Str, Type::Str)
                | (Primitive::Unit, Type::Unit)
        );
    }
    let args = declared.args();
    if let Some(b) = declared.as_builtin() {
        return match (b, actual) {
            (Builtin::List, Type::List(t)) | (Builtin::Option, Type::Option(t)) => {
                args.first().is_some_and(|a| instantiate(sigs, a, t, subst))
            }
            (Builtin::Result, Type::Result(o, e)) => {
                args.len() == 2
                    && instantiate(sigs, &args[0], o, subst)
                    && instantiate(sigs, &args[1], e, subst)
            }
            (Builtin::Function, Type::Function(ps, r)) => match args.split_last() {
                Some((dr, dps)) => {
                    dps.len() == ps.len()
                        && dps
                            .iter()
                            .zip(ps)
                            .all(|(d, p)| instantiate(sigs, d, p, subst))
                        && instantiate(sigs, dr, r, subst)
                }
                None => false,
            },
            _ => false,
        };
    }
    if sigs.privacy_qualifier(declared).is_some() && args.len() == 1 {
        return instantiate(sigs, &args[0], actual, subst);
    }
    // `Box<T>` against `Box<Int>` binds `T` (ADR-0062).
    match (declared.def_id(), actual) {
        (Some(def), Type::Nominal(d, acts)) if def == *d && acts.len() == args.len() => args
            .iter()
            .zip(acts)
            .all(|(a, t)| instantiate(sigs, a, t, subst)),
        _ => false,
    }
}

impl<'a> Lower<'a> {
    fn lexical(&mut self, body: &Body) -> Option<std::rc::Rc<crate::lexical::Lexical>> {
        let key = (self.unit, body.owner);
        if let Some(found) = self.lexicals.get(&key) {
            return Some(found.clone());
        }
        let lexical = std::rc::Rc::new(crate::lexical::Lexical::build_in(
            self.cx.hirs.get(self.unit)?,
            body.owner,
        )?);
        self.lexicals.insert(key, lexical.clone());
        Some(lexical)
    }

    fn declaration_path(&mut self, body: &Body, expr: ExprId) -> bool {
        self.lexical(body)
            .is_some_and(|lexical| lexical.unbound_path(body, expr))
    }

    fn expression_case(&mut self, body: &Body, expr: ExprId) -> Option<crate::values::CaseNamed> {
        let lexical = self.lexical(body)?;
        crate::values::case_named_at(self.cx.sigs, self.cx.ws, self.unit, body, &lexical, expr)
    }

    /// A declared type, under the type arguments this body was instantiated
    /// at (ADR-0050).
    fn ty(&self, resolution: &TypeResolution, span: &Span) -> Lowering<Type> {
        match resolution.resolved() {
            Some(t) => ty_resolved_with(self.cx.sigs, t, span, &self.subst),
            None => Lowering::Blocked {
                why: resolution.to_string(),
                span: span.clone(),
            },
        }
    }

    fn fresh(&mut self) -> ValueId {
        let v = ValueId(self.next_value);
        self.next_value += 1;
        v
    }

    /// Append an instruction, recording its value's type.
    fn push(&mut self, instr: Instr) -> ValueId {
        let v = instr.result();
        self.types.insert(v, instr.ty().clone());
        self.instrs.push(instr);
        v
    }

    /// Is `name` the language's own, here: `Some`, `None`, `Ok`, `Err` where
    /// the program declares no term of that name? The typer's rule, so the
    /// backend cannot read a constructor the checker read as a declaration.
    fn builtin(&self, name: &str) -> Option<BuiltinCase> {
        let case = match name {
            "Some" => BuiltinCase::Some,
            "None" => BuiltinCase::None,
            "Ok" => BuiltinCase::Ok,
            "Err" => BuiltinCase::Err,
            _ => return None,
        };
        if self.locals.contains_key(name) {
            return None;
        }
        matches!(
            self.cx.ws.resolve_in(self.unit, Namespace::Term, name),
            Resolution::Unresolved
        )
        .then_some(case)
    }
}

fn ty_resolution(sigs: &Signatures, resolution: &TypeResolution, span: &Span) -> Lowering<Type> {
    match resolution.resolved() {
        Some(ty) => ty_resolved(sigs, ty, span),
        None => Lowering::Blocked {
            why: resolution.to_string(),
            span: span.clone(),
        },
    }
}

/// ABI projection: semantic identity arrives resolved. Privacy qualification
/// is erased only for the selected qualifier definition, not for its spelling.
fn ty_resolved(sigs: &Signatures, ty: &ResolvedType, span: &Span) -> Lowering<Type> {
    ty_resolved_with(sigs, ty, span, &BTreeMap::new())
}

/// [`ty_resolved`], where a type parameter is the type it was instantiated
/// at (ADR-0050).
fn ty_resolved_with(
    sigs: &Signatures,
    ty: &ResolvedType,
    span: &Span,
    subst: &BTreeMap<(DefId, u32), Type>,
) -> Lowering<Type> {
    if let Some(key) = ty.parameter_binding() {
        return match subst.get(&key) {
            Some(t) => Lowering::Lowered(t.clone()),
            None => Lowering::Unsupported {
                construct: "a type parameter no call instantiates",
                span: span.clone(),
                reason: format!("`{ty}` is not fixed by what reaches it here"),
            },
        };
    }
    if let Some(p) = ty.as_primitive() {
        return Lowering::Lowered(match p {
            Primitive::Int => Type::Int,
            Primitive::Float => Type::Float,
            Primitive::Bool => Type::Bool,
            Primitive::Str => Type::Str,
            Primitive::Unit => Type::Unit,
        });
    }
    let arg = |i: usize| match ty.args().get(i) {
        Some(t) => ty_resolved_with(sigs, t, span, subst),
        None => Lowering::Blocked {
            why: "resolved constructor is missing an argument".into(),
            span: span.clone(),
        },
    };
    if let Some(b) = ty.as_builtin() {
        return match b {
            Builtin::Result => match (arg(0), arg(1)) {
                (Lowering::Lowered(a), Lowering::Lowered(b)) => {
                    Lowering::Lowered(Type::Result(Box::new(a), Box::new(b)))
                }
                (Lowering::Lowered(_), other) | (other, _) => other,
            },
            Builtin::Option => arg(0).map(|a| Type::Option(Box::new(a))),
            Builtin::List => arg(0).map(|a| Type::List(Box::new(a))),
            // A map's key and a set's element are ordered: an `Int`, a
            // `String`, a `Bool`, or an opaque type over one (ADR-0057,
            // ADR-0248). The checker refuses another where it is written
            // (PW0627); this is the backend's own guard.
            Builtin::Map | Builtin::Set => {
                let key = match arg(0) {
                    Lowering::Lowered(k) if ordered_key(sigs, &k) => k,
                    Lowering::Lowered(other) => {
                        return Lowering::Unsupported {
                            construct: "a map or set keyed by a type other than Int, String or Bool",
                            span: span.clone(),
                            reason: format!(
                                "a {other:?} key has no order the backends share (ADR-0248)"
                            ),
                        };
                    }
                    other => return other,
                };
                match b {
                    Builtin::Set => Lowering::Lowered(Type::Set(Box::new(key))),
                    _ => arg(1).map(|v| Type::Map(Box::new(key), Box::new(v))),
                }
            }
            // `fn(A, B) -> R`: its parameters, then its result (ADR-0052).
            Builtin::Function => {
                let mut all = Vec::new();
                for i in 0..ty.args().len() {
                    match arg(i) {
                        Lowering::Lowered(t) => all.push(t),
                        other => return other,
                    }
                }
                match all.pop() {
                    Some(r) => Lowering::Lowered(Type::Function(all, Box::new(r))),
                    None => Lowering::Blocked {
                        why: "a function type with no result".to_string(),
                        span: span.clone(),
                    },
                }
            }
        };
    }
    if sigs.privacy_qualifier(ty).is_some() && ty.args().len() == 1 {
        return arg(0);
    }
    if let Some(def) = ty.def_id() {
        // A type that contains itself through a list is laid out by its type
        // (ADR-0194): a list's elements are where it points. One that holds
        // itself in place is boxed by the encoder (ADR-0202): its value is the
        // address of its cell.
        // An instance, by its arguments (ADR-0062): `Box<Int>` is laid out
        // with an `Int` where `Box` declares its `T`.
        let mut args = Vec::new();
        for i in 0..ty.args().len() {
            match arg(i) {
                Lowering::Lowered(t) => args.push(t),
                other => return other,
            }
        }
        return Lowering::Lowered(Type::Nominal(def, args));
    }
    Lowering::Blocked {
        why: format!("`{ty}` names no declaration"),
        span: span.clone(),
    }
}

impl<'a> Lower<'a> {
    /// **An event a command emits, or an entry it invalidates** (ADR-0208,
    /// ADR-0209): its values, at the parameters the event or the query
    /// declares, then the platform's call.
    fn outboxed(&mut self, body: &Body, clause: &str, key: &crate::hir::ClauseKey) -> Lowering<()> {
        let blocked = |why: String| Lowering::Blocked {
            why,
            span: key.span.clone(),
        };
        let target = match clause {
            "emits" => crate::backend::emitted_event(self.cx.ws, self.unit, key),
            _ => crate::backend::invalidated_query(self.cx.ws, self.cx.sigs, self.unit, key),
        };
        let Some(target) = target else {
            return blocked(format!("`{clause} {}` names nothing it may", key.name));
        };
        let (Some(decl), Some(sig)) = (
            crate::resolve::declaration(self.cx.hirs, target),
            self.cx.sigs.by_def(target),
        ) else {
            return blocked(format!("`{}` has no resolved signature", key.name));
        };
        // Each value at its parameter: by its name where it is named, and
        // otherwise in order, as the checker related them (ADR-0088).
        let mut given: Vec<Option<ExprId>> = vec![None; sig.params.len()];
        let mut next = 0;
        for a in &key.args {
            let at = match &a.name {
                Some(name) => sig.names.iter().position(|n| n == name),
                None => {
                    next += 1;
                    Some(next - 1)
                }
            };
            match at.and_then(|i| given.get_mut(i)) {
                Some(slot) => *slot = Some(a.value),
                None => {
                    return blocked(format!("`{}` is given a value it does not carry", key.name));
                }
            }
        }
        // A position written `_` is every value there (ADR-0256): the
        // function takes the rest, and the host drops every entry at them.
        let every: Vec<usize> = match clause {
            "invalidates" => given
                .iter()
                .enumerate()
                .filter(|(_, e)| e.is_some_and(|e| crate::check::every_value(body, e)))
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
        };
        let import = match clause {
            "emits" => match crate::backend::host_binding(decl) {
                Some(import) => import,
                None => return blocked(format!("`{}` is not the outbox's", key.name)),
            },
            _ => crate::backend::invalidation_binding(&sig.path, &every),
        };
        let mut args = Vec::new();
        for (i, (slot, declared)) in given.into_iter().zip(&sig.params).enumerate() {
            if every.contains(&i) {
                continue;
            }
            let (Some(expr), Some(declared)) = (slot, declared.as_ref()) else {
                return blocked(format!(
                    "`{}` is not given every value it carries",
                    key.name
                ));
            };
            let ty = match ty_resolution(self.cx.sigs, declared, &key.span) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            match self.expr(body, expr, Some(&ty)) {
                Lowering::Lowered(v) => args.push(v),
                other => return other.map(|_| unreachable!()),
            }
        }
        let result = self.fresh();
        self.push(Instr::ImportCall {
            result,
            import,
            args,
            ty: Type::Unit,
        });
        Lowering::Lowered(())
    }

    /// Whether the function being lowered is a command's export, whose body
    /// may schedule (PW6502).
    fn lowering_a_command(&self) -> bool {
        self.inlining
            .first()
            .and_then(|d| crate::resolve::declaration(self.cx.hirs, *d))
            .is_some_and(|d| d.kind == DeclKind::Command)
    }

    /// **A run a command schedules, or cancels** (the scheduled-commands
    /// track): `schedule cmd(args) at when`, a call of the platform's
    /// schedules' function for `cmd` and how, given `cmd`'s parameters, as a
    /// call gives them, and then the time as the `Int` it is; `unschedule
    /// cmd(args)`, its cancel, given the parameters. The host writes each with
    /// the command's writes, or not at all.
    fn scheduled(
        &mut self,
        body: &Body,
        keyword: &str,
        modifiers: &[String],
        args: &[ExprId],
        span: Span,
    ) -> Lowering<ValueId> {
        let blocked = |why: String| Lowering::Blocked {
            why,
            span: span.clone(),
        };
        let (how, given, time) = match (keyword, modifiers.get(1).map(String::as_str)) {
            ("schedule", Some(how @ ("at" | "after"))) if !args.is_empty() => {
                (how, &args[..args.len() - 1], args.last().copied())
            }
            ("unschedule", None) => ("cancel", args, None),
            _ => return blocked(format!("`{keyword}` is written `{keyword} cmd(..)`")),
        };
        let Some(name) = modifiers.first() else {
            return blocked(format!("`{keyword}` names no command"));
        };
        let (crate::resolve::Resolution::Local(def)
        | crate::resolve::Resolution::Imported { def, .. }) =
            self.cx
                .ws
                .resolve_in(self.unit, crate::resolve::Namespace::Term, name)
        else {
            return blocked(format!("`{name}` names no command"));
        };
        let Some(sig) = self.cx.sigs.by_def(def) else {
            return blocked(format!("`{name}` has no resolved signature"));
        };
        if given.len() != sig.params.len() {
            return blocked(format!(
                "`{name}` is given {} values for its {} parameters",
                given.len(),
                sig.params.len()
            ));
        }
        let mut values = Vec::new();
        for (expr, declared) in given.iter().zip(&sig.params) {
            let Some(declared) = declared.as_ref() else {
                return blocked(format!("`{name}` has a parameter of no declared type"));
            };
            let ty = match self.ty(declared, &span) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            match self.expr(body, *expr, Some(&ty)) {
                Lowering::Lowered(v) => values.push(v),
                other => return other,
            }
        }
        // The time, an `Instant` or a `Duration`, crosses as the `Int` it is
        // (ADR-0054): its value retyped to its representation.
        if let Some(time) = time {
            let at = match self.expr(body, time, None) {
                Lowering::Lowered(v) => v,
                other => return other,
            };
            let value = match self.types.get(&at).cloned() {
                Some(Type::Int) => at,
                Some(Type::Nominal(d, instance))
                    if self.representation(d, &instance, &span) == Some(Type::Int) =>
                {
                    let result = self.fresh();
                    self.push(Instr::Retype {
                        result,
                        value: at,
                        ty: Type::Int,
                    })
                }
                _ => return blocked(format!("`{keyword} {name}`'s time is no `Int`")),
            };
            values.push(value);
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::ImportCall {
            result,
            import: crate::backend::schedule_binding(&sig.path, how),
            args: values,
            ty: Type::Unit,
        }))
    }

    /// **A read in a materialization's body** (ADR-0277): `query R(args)`, a
    /// call of the platform's read of R, given R's parameters and answering
    /// R's value, its `Ok` where R answers a `Result`. The host answers it from
    /// R; where R answers an error the regeneration fails, and the
    /// materialization's `fallback` decides what is served.
    /// **`navigate Page(args)`** (ADR-0280): the page's route, and each
    /// argument lowered as the parameter it gives, each text (PW0621), by
    /// the parameter's name, which is its segment's.
    fn navigate(
        &mut self,
        body: &Body,
        modifiers: &[String],
        args: &[ExprId],
        span: Span,
    ) -> Lowering<ValueId> {
        let blocked = |why: String| Lowering::Blocked {
            why,
            span: span.clone(),
        };
        let Some(name) = modifiers.first() else {
            return blocked("a navigation names no page".to_string());
        };
        let resolution = match name.contains('.') {
            true => self.cx.ws.resolve_path(self.unit, name),
            false => self.cx.ws.resolve_in(self.unit, Namespace::Ui, name),
        };
        let (Resolution::Local(def) | Resolution::Imported { def, .. }) = resolution else {
            return blocked(format!("`{name}` does not resolve to a page"));
        };
        let hir = self.cx.hirs[def.unit];
        let decl = hir.decl(crate::hir::DeclId(def.decl));
        if decl.kind != DeclKind::Page {
            return blocked(format!("`{name}` is no page"));
        }
        let Some(route) = crate::routes::declared_route(hir, decl) else {
            return blocked(format!("`{name}` declares no route"));
        };
        let Some(sig) = self.cx.sigs.by_def(def) else {
            return blocked(format!("`{name}` has no signature"));
        };
        if sig.params.len() != args.len() || decl.params.len() != args.len() {
            return blocked(format!(
                "`{name}` takes {} values, and is given {}",
                sig.params.len(),
                args.len()
            ));
        }
        let mut given = Vec::new();
        let queried = crate::routes::route_parts(&route).1;
        for ((arg, declared), param) in args.iter().zip(&sig.params).zip(&decl.params) {
            // A query's parameter is an `Option`, which a navigation does not
            // give yet: a link or a GET form does (ADR-0309's Not claimed).
            if queried.contains(&param.name.as_str()) {
                return blocked(format!(
                    "`{name}`'s parameter `{}` is its route's query's, which a navigation \
                     does not give yet: a link or a GET form gives it",
                    param.name
                ));
            }
            let Some(declared) = declared.as_ref() else {
                return blocked(format!("`{name}`'s parameter `{}` has no type", param.name));
            };
            if !declared
                .resolved()
                .is_some_and(|t| crate::routes::is_text(self.cx.sigs, t))
            {
                return blocked(format!(
                    "`{name}`'s parameter `{}` is not text, which an address gives",
                    param.name
                ));
            }
            let ty = match self.ty(declared, &span) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            match self.expr(body, *arg, Some(&ty)) {
                Lowering::Lowered(v) => given.push((param.name.clone(), v)),
                other => return other,
            }
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Navigate {
            result,
            route,
            args: given,
            ty: Type::Unit,
        }))
    }

    fn query_read(
        &mut self,
        body: &Body,
        modifiers: &[String],
        args: &[ExprId],
        span: Span,
    ) -> Lowering<ValueId> {
        let blocked = |why: String| Lowering::Blocked {
            why,
            span: span.clone(),
        };
        let Some(name) = modifiers.first() else {
            return blocked("a read names no resource".to_string());
        };
        let resolution = match name.contains('.') {
            true => self.cx.ws.resolve_path(self.unit, name),
            false => self.cx.ws.resolve_in(self.unit, Namespace::Term, name),
        };
        let (Resolution::Local(def) | Resolution::Imported { def, .. }) = resolution else {
            return blocked(format!("`{name}` does not resolve"));
        };
        let Some(sig) = self.cx.sigs.by_def(def) else {
            return blocked(format!("`{name}` has no signature"));
        };
        if sig.params.len() != args.len() {
            return blocked(format!(
                "`{name}` takes {} values, and is given {}",
                sig.params.len(),
                args.len()
            ));
        }
        let mut values = Vec::new();
        for (arg, declared) in args.iter().zip(&sig.params) {
            let Some(declared) = declared.as_ref() else {
                return blocked(format!("a parameter of `{name}` has no type"));
            };
            let ty = match self.ty(declared, &span) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            match self.expr(body, *arg, Some(&ty)) {
                Lowering::Lowered(v) => values.push(v),
                other => return other,
            }
        }
        let Some(returns) = sig.returns.as_ref() else {
            return blocked(format!("`{name}` declares no type"));
        };
        let ty = match self.ty(returns, &span) {
            Lowering::Lowered(Type::Result(ok, _)) => *ok,
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        // A map or set it answers is sorted as a host's answer is (ADR-0057).
        let check = match &ty {
            Type::Map(..) => Some(Intrinsic::MapCheck),
            Type::Set(..) => Some(Intrinsic::SetCheck),
            t if holds_collection(self.cx, t, &mut Vec::new()) => {
                return Lowering::Unsupported {
                    construct: "a map or set inside a read's answer",
                    span,
                    reason: "only a map or set that is itself the answer is checked when it \
                             arrives (ADR-0057)"
                        .to_string(),
                };
            }
            _ => None,
        };
        let result = self.fresh();
        let read = self.push(Instr::ImportCall {
            result,
            import: crate::backend::read_binding(&sig.path),
            args: values,
            ty: ty.clone(),
        });
        match check {
            Some(op) => {
                let checked = self.fresh();
                Lowering::Lowered(self.push(Instr::Intrinsic {
                    result: checked,
                    op,
                    args: vec![read],
                    ty,
                }))
            }
            None => Lowering::Lowered(read),
        }
    }

    /// Lower one expression. `expected` is the type its context fixes, if any:
    /// the declaration's result, a parameter's type, the arms' common type.
    /// It is how `None` knows what it is `None` of.
    fn expr(&mut self, body: &Body, e: ExprId, expected: Option<&Type>) -> Lowering<ValueId> {
        let span = body.expr_span(e);
        // **A path a handler captured** (ADR-0058) is the value the document
        // carries for it, unless a binding in the body shadows its root.
        if !self.captured.is_empty()
            && let Some(path) = crate::resume::field_chain(body, e)
            && let Some(v) = self.captured.get(&path).copied()
            && !self
                .locals
                .contains_key(path.split('.').next().unwrap_or_default())
        {
            return Lowering::Lowered(v);
        }
        match body.expr(e) {
            Expr::Literal(l) => {
                // The HIR keeps a literal as its SOURCE TEXT, so a number that
                // does not fit is a refusal here rather than a silent wrap —
                // the backend is where "as written" stops being enough.
                let (value, ty) = match l {
                    Literal::Int(n) => match n.parse::<i64>() {
                        Ok(v) => (Const::Int(v), Type::Int),
                        Err(_) => {
                            return Lowering::Unsupported {
                                construct: "an integer literal this backend cannot represent",
                                span,
                                reason: format!("`{n}` does not fit in the ABI's `s64`"),
                            };
                        }
                    },
                    Literal::Float(n) => match n.parse::<f64>() {
                        Ok(v) => (Const::Float(v), Type::Float),
                        Err(_) => {
                            return Lowering::Unsupported {
                                construct: "a float literal this backend cannot represent",
                                span,
                                reason: format!("`{n}` is not an `f64`"),
                            };
                        }
                    },
                    // The string's VALUE, not its token: until 2026-09-25 this
                    // held the token, quotes included, which nothing noticed
                    // because the encoder refuses constants that need memory.
                    Literal::Str(s) => match l.string_value() {
                        Some(v) => (Const::Str(v), Type::Str),
                        None => {
                            return Lowering::Blocked {
                                why: format!("`{s}` has no value; the grammar refuses it (PW0014)"),
                                span,
                            };
                        }
                    },
                    Literal::UnterminatedStr(_) => {
                        return Lowering::Blocked {
                            why: "an unterminated string; the program did not parse".to_string(),
                            span,
                        };
                    }
                    // `()` (ADR-0200).
                    Literal::Unit => (Const::Unit, Type::Unit),
                };
                let result = self.fresh();
                Lowering::Lowered(self.push(Instr::Const {
                    result,
                    value,
                    ty: ty.clone(),
                }))
            }
            // A placeholder body. `Unsupported`, not `Blocked`: the program is
            // perfectly well-formed and there is simply nothing to compile.
            Expr::Name(n) if n == "todo" => Lowering::Unsupported {
                construct: PLACEHOLDER,
                span,
                reason: "the declaration is a placeholder".to_string(),
            },
            Expr::Name(n) if self.builtin(n) == Some(BuiltinCase::None) => {
                self.variant(BuiltinCase::None, None, expected, span)
            }
            // The language's own truth values, where nothing else has the
            // name: the typer's rule.
            Expr::Name(n) if (n == "true" || n == "false") && self.own_term(n) => {
                let result = self.fresh();
                Lowering::Lowered(self.push(Instr::Const {
                    result,
                    value: Const::Bool(n == "true"),
                    ty: Type::Bool,
                }))
            }
            // A `return` with nothing after it in its block (ADR-0051).
            Expr::Name(n) if n == "return" => self.early_return(body, None, expected, span),
            Expr::Name(n) => match self.locals.get(n).copied() {
                // A mutable binding is read as what it holds here (ADR-0051).
                Some(v) if self.vars.contains(&v) => Lowering::Lowered(self.read(v)),
                Some(v) => Lowering::Lowered(v),
                // A page's signal, read through the handler's context
                // (ADR-0130).
                None if self.signals.contains_key(n) => {
                    let ty = self.signals[n].clone();
                    let result = self.fresh();
                    Lowering::Lowered(self.push(Instr::SignalGet {
                        result,
                        signal: n.clone(),
                        ty,
                    }))
                }
                // `Empty` alone: the case of that name of the one sum type
                // this unit sees with one, or of the one expected where it is
                // written, by the typer's rule (ADR-0059, ADR-0201).
                None if self.case_written_alone(body, e).is_some() => {
                    let (def, index) = self.case_written_alone(body, e).expect("checked");
                    self.case_value(def, index, expected, span)
                }
                // A declaration's name where a function is wanted: its code,
                // as a value (ADR-0052).
                None if matches!(expected, Some(Type::Function(..))) => {
                    self.declaration_value(body, e, expected.cloned(), span)
                }
                None if self.internal.borrow().captured_roots.contains(n) => {
                    Lowering::Unsupported {
                        construct: "a captured value read inside a function value",
                        span,
                        reason: format!(
                            "`{n}` is what the handler captured; a function value's code does \
                             not receive it (ADR-0058)"
                        ),
                    }
                }
                None => Lowering::Blocked {
                    why: format!("`{n}` is not bound here"),
                    span,
                },
            },
            Expr::Lambda {
                descriptor: None,
                params,
                body: inner,
            } => self.closure(body, params, *inner, expected, span),
            Expr::Block { stmts } => {
                let mut last = None;
                for (i, s) in stmts.iter().enumerate() {
                    let tail = i + 1 == stmts.len();
                    // **`return e`** (ADR-0051): the grammar keeps `return` as a
                    // statement and its value as the next one. Nothing after
                    // them in the block runs.
                    if matches!(body.expr(*s), Expr::Name(n) if n == "return") {
                        return self.early_return(body, stmts.get(i + 1).copied(), expected, span);
                    }
                    if let Expr::Let { pat, init, ty } = body.expr(*s) {
                        // A block ending in a binding is the unit value
                        // (ADR-0159): `{ let _ = .. }` discards, as ruling
                        // 0099-a has it (ADR-0250), and a handler may end so.
                        // Where a value of another type is wanted, it cannot
                        // be given.
                        if tail && !matches!(expected, None | Some(Type::Unit)) {
                            return Lowering::Unsupported {
                                construct: "a block ending in a binding",
                                span,
                                reason: "the block's value would be the unit value".to_string(),
                            };
                        }
                        let annotated = ty.and_then(|t| self.annotation(body, t, &span));
                        match self.bind(body, *pat, *init, annotated, span.clone()) {
                            Lowering::Lowered(()) if tail => {
                                return Lowering::Lowered(self.unit());
                            }
                            Lowering::Lowered(()) => continue,
                            other => return other.map(|_| unreachable!()),
                        }
                    }
                    match self.expr(body, *s, if tail { expected } else { None }) {
                        Lowering::Lowered(v) => last = Some(v),
                        other => return other,
                    }
                }
                match last {
                    Some(v) => Lowering::Lowered(v),
                    None => Lowering::Unsupported {
                        construct: "an empty block",
                        span,
                        reason: "a function must produce a value".to_string(),
                    },
                }
            }
            Expr::Call { callee, args } => match body.expr(*callee) {
                Expr::Name(n) if self.builtin(n).is_some_and(BuiltinCase::has_payload) => {
                    let case = self.builtin(n).expect("checked");
                    let [arg] = args.as_slice() else {
                        return Lowering::Blocked {
                            why: format!("`{n}` takes one value"),
                            span,
                        };
                    };
                    let payload_expected = match (case, expected) {
                        (BuiltinCase::Some, Some(Type::Option(t))) => Some((**t).clone()),
                        (BuiltinCase::Ok, Some(Type::Result(t, _))) => Some((**t).clone()),
                        (BuiltinCase::Err, Some(Type::Result(_, e))) => Some((**e).clone()),
                        _ => None,
                    };
                    let payload = match self.expr(body, arg.value, payload_expected.as_ref()) {
                        Lowering::Lowered(v) => v,
                        other => return other,
                    };
                    self.variant(case, Some(payload), expected, span)
                }
                _ => self.call(body, *callee, args, None, expected, span),
            },
            Expr::Match { scrutinee, arms } => self.matched(body, *scrutinee, arms, expected, span),
            Expr::Field { base, name } => {
                // `Shape.Empty`, a case through its type (ADR-0059).
                if let Some(crate::values::CaseNamed::Case(def, index)) =
                    self.expression_case(body, e)
                {
                    return self.case_value(def, index, expected, span);
                }
                self.field(body, *base, name, span)
            }
            Expr::Binary { op, lhs, rhs } => self.binary(body, op, *lhs, *rhs, expected, span),
            Expr::Unary { op, operand } => self.unary(body, op, *operand, expected, span),
            Expr::If {
                cond,
                then,
                els: Some(els),
            } => self.branch(body, *cond, *then, *els, expected, span),
            // A statement: its value is the unit value, whichever way it goes
            // (ADR-0051). Where a value is needed it has none when the
            // condition is false, and is refused.
            Expr::If {
                cond,
                then,
                els: None,
            } if expected.is_none_or(|t| *t == Type::Unit) => {
                self.statement_if(body, *cond, *then, span)
            }
            Expr::If { els: None, .. } => Lowering::Unsupported {
                construct: "an `if` without `else`",
                span,
                reason: "it has no value when its condition is false".to_string(),
            },
            Expr::For {
                pat,
                iterable,
                body: inner,
            } => self.for_loop(body, *pat, *iterable, *inner, span),
            Expr::Try { value } => self.propagate(body, *value, span),
            Expr::Interpolated { text, parts } => self.interpolated(body, text, parts, span),
            Expr::Record {
                name: Some(name),
                fields,
            } => self.record(body, name, fields, expected, span),
            Expr::List { items } => self.list(body, items, expected, span),
            // **A read in a materialization's body** (ADR-0277).
            Expr::Keyword {
                keyword,
                modifiers,
                args,
                ..
            } if self.reads && keyword == "query" => self.query_read(body, modifiers, args, span),
            // **A handler goes to a page once its command commits** (ADR-0280).
            Expr::Keyword {
                keyword,
                modifiers,
                args,
                ..
            } if self.handler && keyword == "navigate" => {
                self.navigate(body, modifiers, args, span)
            }
            // **A run a command schedules, or cancels** (the
            // scheduled-commands track): only in a command's own body, which
            // PW6502 keeps it to.
            Expr::Keyword {
                keyword,
                modifiers,
                args,
                ..
            } if matches!(keyword.as_str(), "schedule" | "unschedule")
                && self.lowering_a_command() =>
            {
                self.scheduled(body, keyword, modifiers, args, span)
            }
            other => Lowering::Unsupported {
                construct: construct_name(other),
                span,
                reason: "the backend lowers calls, names, literals, blocks, bindings, \
                         field reads, matches over `Option` and `Result`, arithmetic, \
                         comparisons, `if`, interpolated strings and records"
                    .to_string(),
            },
        }
    }

    /// Is `name` a term the program leaves to the language, here?
    fn own_term(&self, name: &str) -> bool {
        !self.locals.contains_key(name)
            && matches!(
                self.cx.ws.resolve_in(self.unit, Namespace::Term, name),
                Resolution::Unresolved
            )
    }

    /// Lower `e` into a region of its own: its instructions, and its value.
    /// Names it binds stay inside.
    fn region(&mut self, body: &Body, e: ExprId, expected: Option<&Type>) -> Lowering<Region> {
        let outer = std::mem::take(&mut self.instrs);
        let scope = self.locals.clone();
        let value = self.expr(body, e, expected);
        self.locals = scope;
        let instrs = std::mem::replace(&mut self.instrs, outer);
        value.map(|value| Region { instrs, value })
    }

    /// A region holding one constant.
    fn constant(&mut self, value: Const, ty: Type) -> Region {
        let result = self.fresh();
        self.types.insert(result, ty.clone());
        Region {
            instrs: vec![Instr::Const { result, value, ty }],
            value: result,
        }
    }

    /// Lower `e` and require the type `want`.
    fn typed(&mut self, body: &Body, e: ExprId, want: &Type) -> Lowering<ValueId> {
        let span = body.expr_span(e);
        let v = match self.expr(body, e, Some(want)) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        match self.types.get(&v) {
            Some(t) if t == want => Lowering::Lowered(v),
            other => Lowering::Blocked {
                why: format!("a {want:?} is needed here and this is {other:?}"),
                span,
            },
        }
    }

    /// **Arithmetic, a comparison, `&` or `|`** (ADR-0039 §1, §2).
    fn binary(
        &mut self,
        body: &Body,
        op: &crate::hir::BinOp,
        lhs: ExprId,
        rhs: ExprId,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        use crate::hir::BinOp as B;
        // `a & b` is `if a { b } else { false }`, and `a | b` is
        // `if a { true } else { b }`: the right side runs only when it decides.
        if matches!(op, B::And | B::Or) {
            let cond = match self.typed(body, lhs, &Type::Bool) {
                Lowering::Lowered(v) => v,
                other => return other,
            };
            let right = match self.region(body, rhs, Some(&Type::Bool)) {
                Lowering::Lowered(r) => r,
                other => return other.map(|_| unreachable!()),
            };
            if self.types.get(&right.value) != Some(&Type::Bool) {
                return Lowering::Blocked {
                    why: "the right side of a logical operator is not a Bool".to_string(),
                    span,
                };
            }
            let (then, els) = match op {
                B::And => (right, self.constant(Const::Bool(false), Type::Bool)),
                _ => (self.constant(Const::Bool(true), Type::Bool), right),
            };
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::If {
                result,
                cond,
                then,
                els,
                ty: Type::Bool,
            }));
        }
        let op = match op {
            B::Add => BinaryOp::Add,
            B::Sub => BinaryOp::Sub,
            B::Mul => BinaryOp::Mul,
            B::Div => BinaryOp::Div,
            B::Rem => BinaryOp::Rem,
            B::Cmp(c) => match c.as_str() {
                "==" => BinaryOp::Eq,
                "!=" => BinaryOp::Ne,
                "<" => BinaryOp::Lt,
                "<=" => BinaryOp::Le,
                ">" => BinaryOp::Gt,
                ">=" => BinaryOp::Ge,
                other => {
                    return Lowering::Blocked {
                        why: format!("`{other}` is not a comparison"),
                        span,
                    };
                }
            },
            // `a |> f(b)` is `f(a, b)`, and `a |> f` is `f(a)`.
            B::Pipe => {
                let piped = match self.expr(body, lhs, None) {
                    Lowering::Lowered(v) => v,
                    other => return other,
                };
                return match body.expr(rhs) {
                    Expr::Call { callee, args } => {
                        self.call(body, *callee, args, Some(piped), expected, span)
                    }
                    Expr::Name(_) | Expr::Field { .. } => {
                        self.call(body, rhs, &[], Some(piped), expected, span)
                    }
                    _ => Lowering::Unsupported {
                        construct: "a pipeline into something that is not a call",
                        span,
                        reason: "`a |> f(..)` feeds `a` to a call".to_string(),
                    },
                };
            }
            B::And | B::Or => unreachable!("handled above"),
            B::Assign => return self.assign(body, lhs, rhs, span),
            B::Transition => {
                return Lowering::Unsupported {
                    construct: "a keyframe transition",
                    span,
                    reason: "`a -> b` is an animation's, and nothing here computes it".to_string(),
                };
            }
        };
        // An arithmetic operator's operands have its result's type; a
        // comparison's have whatever type they share.
        let operand = if op.compares() { None } else { expected };
        let l = match self.expr(body, lhs, operand) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let Some(lt) = self.types.get(&l).cloned() else {
            return Lowering::Blocked {
                why: "an operand has no type".to_string(),
                span,
            };
        };
        let r = match self.expr(body, rhs, Some(&lt)) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let rt = self.types.get(&r).cloned();
        // **An opaque type compares as its representation** (ADR-0228): two
        // `PostId`s are their `String`s. Its values are its representation's,
        // retyped (ADR-0054), so a comparison retypes them back. Until
        // ADR-0228 `i.id == post` checked, and did not build.
        let (l, r, lt, rt) = match (&lt, op.compares()) {
            (Type::Nominal(def, instance), true) if rt.as_ref() == Some(&lt) => {
                match self.representation(*def, instance, &span) {
                    Some(rep) => {
                        let (lr, rr) = (self.fresh(), self.fresh());
                        let lv = self.push(Instr::Retype {
                            result: lr,
                            value: l,
                            ty: rep.clone(),
                        });
                        let rv = self.push(Instr::Retype {
                            result: rr,
                            value: r,
                            ty: rep.clone(),
                        });
                        (lv, rv, rep.clone(), Some(rep))
                    }
                    None => (l, r, lt, rt),
                }
            }
            _ => (l, r, lt, rt),
        };
        if rt.as_ref() != Some(&lt) {
            // `pw check` refuses operands of two known types (PW0609,
            // ADR-0043). This is reached by operands it could not type.
            return Lowering::Blocked {
                why: format!(
                    "`{op:?}` on a {lt:?} and a {}: the operands differ in type",
                    rt.map_or("value of no type".to_string(), |t| format!("{t:?}"))
                ),
                span,
            };
        }
        let ty = match (op, &lt) {
            (BinaryOp::Rem, Type::Float) => {
                return Lowering::Unsupported {
                    construct: "`%` on a Float",
                    span,
                    reason: "its semantics are not decided, and Wasm has no instruction for it"
                        .to_string(),
                };
            }
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
                Type::Int | Type::Float,
            ) => lt.clone(),
            (BinaryOp::Eq | BinaryOp::Ne, Type::Int | Type::Float | Type::Bool | Type::Str) => {
                Type::Bool
            }
            (
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge,
                Type::Int | Type::Float | Type::Str,
            ) => Type::Bool,
            (op, t) => {
                return Lowering::Unsupported {
                    construct: "an operator on a value of this type",
                    span,
                    reason: format!("`{op:?}` on a {t:?}"),
                };
            }
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Binary {
            result,
            op,
            lhs: l,
            rhs: r,
            ty,
        }))
    }

    /// `-x` and `!b`.
    fn unary(
        &mut self,
        body: &Body,
        op: &crate::hir::UnOp,
        operand: ExprId,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let (op, v) = match op {
            crate::hir::UnOp::Not => match self.typed(body, operand, &Type::Bool) {
                Lowering::Lowered(v) => (UnaryOp::Not, v),
                other => return other,
            },
            crate::hir::UnOp::Neg => match self.expr(body, operand, expected) {
                Lowering::Lowered(v) => (UnaryOp::Neg, v),
                other => return other,
            },
        };
        let ty = match (op, self.types.get(&v)) {
            (UnaryOp::Not, _) => Type::Bool,
            (UnaryOp::Neg, Some(t @ (Type::Int | Type::Float))) => t.clone(),
            (UnaryOp::Neg, other) => {
                return Lowering::Unsupported {
                    construct: "`-` on a value that is not a number",
                    span,
                    reason: format!("the operand is {other:?}"),
                };
            }
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Unary {
            result,
            op,
            operand: v,
            ty,
        }))
    }

    /// **`if c { a } else { b }`**: two regions, one type.
    fn branch(
        &mut self,
        body: &Body,
        cond: ExprId,
        then: ExprId,
        els: ExprId,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let cond = match self.typed(body, cond, &Type::Bool) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let then = match self.region(body, then, expected) {
            Lowering::Lowered(r) => r,
            other => return other.map(|_| unreachable!()),
        };
        let then_ty = self.types.get(&then.value).cloned();
        let fixed = expected.cloned().or(then_ty.clone());
        let els = match self.region(body, els, fixed.as_ref()) {
            Lowering::Lowered(r) => r,
            other => return other.map(|_| unreachable!()),
        };
        let els_ty = self.types.get(&els.value).cloned();
        let (mut then, mut els) = (then, els);
        // A branch that returns has no value of its own, and takes the
        // other's type (ADR-0051).
        let ty = match (then_ty, els_ty) {
            (Some(a), Some(b)) if a == b => a,
            (_, Some(b)) if diverges(&then) => b,
            (Some(a), _) if diverges(&els) => a,
            (a, b) => {
                return Lowering::Blocked {
                    why: format!("the branches produce {a:?} and {b:?}"),
                    span,
                };
            }
        };
        for r in [&mut then, &mut els] {
            self.retype_divergent(r, &ty);
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::If {
            result,
            cond,
            then,
            els,
            ty,
        }))
    }

    /// **`"han-{n}"`**: the literal pieces and each hole's value as text,
    /// concatenated (ADR-0039 §3). The holes are the ones `lower.rs` found in
    /// the token, in order: each non-empty `{..}` is the next part.
    fn interpolated(
        &mut self,
        body: &Body,
        text: &str,
        parts: &[ExprId],
        span: Span,
    ) -> Lowering<ValueId> {
        // The string decoder's pieces (ADR-0049): its text, escapes decoded,
        // and its holes in order. Until 2026-09-25 a string with a backslash,
        // or a `"""` one, was refused here (A-023), and this split the token
        // on its own.
        use pw_syntax::strings::Piece;
        let pieces = match pw_syntax::strings::pieces(text) {
            Ok(p) => p,
            Err(e) => {
                return Lowering::Blocked {
                    why: format!("the string has no value ({}); PW0014 refuses it", e.message),
                    span,
                };
            }
        };
        let holes = pieces
            .iter()
            .filter(|p| matches!(p, Piece::Hole { .. }))
            .count();
        if holes != parts.len() {
            return Lowering::Blocked {
                why: format!(
                    "the string has {holes} holes and {} of them parsed",
                    parts.len()
                ),
                span,
            };
        }
        let mut values = Vec::new();
        let mut next = 0;
        for piece in pieces {
            match piece {
                Piece::Text(t) if t.is_empty() => {}
                Piece::Text(t) => {
                    let result = self.fresh();
                    values.push(self.push(Instr::Const {
                        result,
                        value: Const::Str(t),
                        ty: Type::Str,
                    }));
                }
                Piece::Hole { .. } => {
                    let i = next;
                    next += 1;
                    let v = match self.expr(body, parts[i], None) {
                        Lowering::Lowered(v) => v,
                        other => return other,
                    };
                    let v = match self.types.get(&v) {
                        Some(Type::Str) => v,
                        Some(Type::Int | Type::Bool) => {
                            let result = self.fresh();
                            self.push(Instr::Format {
                                result,
                                value: v,
                                ty: Type::Str,
                            })
                        }
                        other => {
                            return Lowering::Unsupported {
                                construct: "an interpolated value of this type",
                                span: body.expr_span(parts[i]),
                                reason: format!(
                                    "{other:?} has no text form here; a `Float`'s \
                                     formatting is not decided"
                                ),
                            };
                        }
                    };
                    values.push(v);
                }
            }
        }
        match values.as_slice() {
            [] => {
                let result = self.fresh();
                Lowering::Lowered(self.push(Instr::Const {
                    result,
                    value: Const::Str(String::new()),
                    ty: Type::Str,
                }))
            }
            [one] => Lowering::Lowered(*one),
            _ => {
                let result = self.fresh();
                Lowering::Lowered(self.push(Instr::Concat {
                    result,
                    parts: values,
                    ty: Type::Str,
                }))
            }
        }
    }

    /// **`Store { id: 1 }`**: a declared record, every field given once, in
    /// declaration order (ADR-0039 §6).
    fn record(
        &mut self,
        body: &Body,
        name: &str,
        fields: &[crate::hir::FieldInit],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let found = match name.contains('.') {
            true => self.cx.ws.resolve_path_in(self.unit, Namespace::Type, name),
            false => self.cx.ws.resolve_in(self.unit, Namespace::Type, name),
        };
        let def = match found {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
            _ => {
                return Lowering::Blocked {
                    why: format!("`{name}` names no type here"),
                    span,
                };
            }
        };
        let Some(declared) = self.cx.sigs.type_decl(def).and_then(|t| t.record.clone()) else {
            return Lowering::Unsupported {
                construct: "building a value of a type with no record fields",
                span,
                reason: format!("`{name}` is not a record"),
            };
        };
        if let Some(extra) = fields
            .iter()
            .find(|f| !declared.iter().any(|(n, _)| *n == f.name))
        {
            return Lowering::Blocked {
                why: format!("`{name}` has no field `{}`", extra.name),
                span,
            };
        }
        let mut given = Vec::new();
        for (field, _) in &declared {
            let inits: Vec<&crate::hir::FieldInit> =
                fields.iter().filter(|f| f.name == *field).collect();
            let [init] = inits.as_slice() else {
                return Lowering::Blocked {
                    why: format!(
                        "`{name}` is built with its field `{field}` {} times",
                        inits.len()
                    ),
                    span,
                };
            };
            given.push(match init.value {
                Some(e) => Given::Expr(e),
                // `Point { x, y }`: the shorthand names a binding, or, in a
                // handler, what it captured (ADR-0111), unless a binding
                // shadows it, as for a path.
                None => match self.locals.get(field).or_else(|| self.captured.get(field)) {
                    Some(v) => Given::Value(*v),
                    None => {
                        return Lowering::Blocked {
                            why: format!("`{field}` is not bound here"),
                            span,
                        };
                    }
                },
            });
        }
        let resolutions: Vec<TypeResolution> = declared.into_iter().map(|(_, r)| r).collect();
        let (args, instance) =
            match self.instance_fields(body, def, &resolutions, given, expected, &span) {
                Lowering::Lowered(x) => x,
                other => return other.map(|_| unreachable!()),
            };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Construct {
            result,
            ctor: def,
            args,
            ty: Type::Nominal(def, instance),
        }))
    }

    /// **Values for an instance's fields, and the instance they make**
    /// (ADR-0062): each field lowered against its declared type under what
    /// the context and the fields before it fixed. A field whose type
    /// mentions a parameter nothing has fixed yet is lowered alone, and its
    /// type fixes that parameter, as a generic callee's arguments fix its
    /// own (ADR-0050). A parameter nothing fixes is refused by name.
    fn instance_fields(
        &mut self,
        body: &Body,
        def: DefId,
        declared: &[TypeResolution],
        given: Vec<Given>,
        expected: Option<&Type>,
        span: &Span,
    ) -> Lowering<(Vec<ValueId>, Vec<Type>)> {
        let mut subst = self.subst.clone();
        if let Some(Type::Nominal(d, args)) = expected
            && *d == def
        {
            for (i, a) in args.iter().enumerate() {
                subst.insert((def, i as u32), a.clone());
            }
        }
        let mut values = Vec::new();
        for (r, g) in declared.iter().zip(given) {
            let Some(t) = r.resolved() else {
                return Lowering::Blocked {
                    why: r.to_string(),
                    span: span.clone(),
                };
            };
            let v = if !mentions_unbound(t, &subst) {
                let want = match ty_resolved_with(self.cx.sigs, t, span, &subst) {
                    Lowering::Lowered(w) => w,
                    other => return other.map(|_| unreachable!()),
                };
                match g {
                    Given::Expr(e) => match self.typed(body, e, &want) {
                        Lowering::Lowered(v) => v,
                        other => return other.map(|_| unreachable!()),
                    },
                    Given::Value(v) if self.types.get(&v) == Some(&want) => v,
                    Given::Value(v) => {
                        return Lowering::Blocked {
                            why: format!(
                                "a {:?} where a {want:?} is the field",
                                self.types.get(&v)
                            ),
                            span: span.clone(),
                        };
                    }
                }
            } else {
                let v = match g {
                    Given::Expr(e) => match self.expr(body, e, None) {
                        Lowering::Lowered(v) => v,
                        other => return other.map(|_| unreachable!()),
                    },
                    Given::Value(v) => v,
                };
                let actual = self.types.get(&v).cloned();
                if !actual.is_some_and(|a| instantiate(self.cx.sigs, t, &a, &mut subst)) {
                    return Lowering::Blocked {
                        why: format!("a value of another type where the field is `{t}`"),
                        span: span.clone(),
                    };
                }
                v
            };
            values.push(v);
        }
        match self.instance_args(def, &subst) {
            Some(args) => Lowering::Lowered((values, args)),
            None => Lowering::Unsupported {
                construct: "a type parameter no use instantiates",
                span: span.clone(),
                reason: "a generic type's arguments are fixed by its fields or where it is used, \
                         and nothing here fixes one"
                    .to_string(),
            },
        }
    }

    /// The arguments an instance of `def` is applied to, from `subst`:
    /// `None` if a parameter is not in it.
    fn instance_args(&self, def: DefId, subst: &BTreeMap<(DefId, u32), Type>) -> Option<Vec<Type>> {
        let n = crate::resolve::declaration(self.cx.hirs, def).map_or(0, |d| d.type_params.len());
        (0..n as u32)
            .map(|i| subst.get(&(def, i)).cloned())
            .collect()
    }

    /// **The instance a case's fields' types fix** (ADR-0062): `Maybe.Just`
    /// given an `Int` is a `Maybe<Int>`. `None` where they fix none, or
    /// disagree with its fields.
    fn case_instance(&self, def: DefId, index: usize, fields: &[Type]) -> Option<Vec<Type>> {
        let (_, declared) = self
            .cx
            .sigs
            .type_decl(def)?
            .variants
            .as_ref()?
            .get(index)?
            .clone();
        if declared.len() != fields.len() {
            return None;
        }
        let mut subst = self.subst.clone();
        for (d, f) in declared.iter().zip(fields) {
            if !instantiate(self.cx.sigs, d.resolved()?, f, &mut subst) {
                return None;
            }
        }
        self.instance_args(def, &subst)
    }

    /// The function's substitution, with `def`'s own parameters bound to
    /// `args`: what a field of that instance is.
    fn instance_subst(&self, def: DefId, args: &[Type]) -> BTreeMap<(DefId, u32), Type> {
        let mut subst = self.subst.clone();
        for (i, a) in args.iter().enumerate() {
            subst.insert((def, i as u32), a.clone());
        }
        subst
    }

    /// **`[a, b, c]`**: every element of one type, which the context fixes
    /// for an empty list.
    fn list(
        &mut self,
        body: &Body,
        items: &[ExprId],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let mut element = match expected {
            Some(Type::List(t)) => Some((**t).clone()),
            _ => None,
        };
        let mut values = Vec::new();
        for item in items {
            let v = match self.expr(body, *item, element.as_ref()) {
                Lowering::Lowered(v) => v,
                other => return other,
            };
            let t = self.types.get(&v).cloned();
            match (&element, t) {
                (None, Some(t)) => element = Some(t),
                (Some(e), Some(t)) if *e == t => {}
                (e, t) => {
                    return Lowering::Blocked {
                        why: format!("a list of {e:?} holds a {t:?}"),
                        span,
                    };
                }
            }
            values.push(v);
        }
        let Some(element) = element else {
            return Lowering::Unsupported {
                construct: "an empty list whose element type nothing fixes",
                span,
                reason: "`[]` needs the type its context expects".to_string(),
            };
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::MakeList {
            result,
            items: values,
            ty: Type::List(Box::new(element)),
        }))
    }

    /// **A standard-library operation** (ADR-0040): a loop whose body is the
    /// function argument, or an operation on values.
    fn intrinsic(
        &mut self,
        body: &Body,
        op: Operation,
        args: &[crate::hir::Arg],
        piped: Option<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        // The checker gives a named argument to the parameter of its name
        // (ADR-0081); an operation takes its arguments in order, and is not
        // given them by name yet.
        if args.iter().any(|a| a.name.is_some()) {
            return Lowering::Unsupported {
                construct: "a named argument to a standard-library operation",
                span,
                reason: "an operation takes its arguments in order".to_string(),
            };
        }
        // The arguments as written, the piped value first.
        let given: Vec<Given> = piped
            .into_iter()
            .map(Given::Value)
            .chain(args.iter().map(|a| Given::Expr(a.value)))
            .collect();
        match op {
            Operation::Intrinsic(i) => {
                let mut values: Vec<ValueId> = Vec::new();
                for (k, g) in given.iter().enumerate() {
                    let prior: Vec<Type> = values
                        .iter()
                        .filter_map(|v| self.types.get(v).cloned())
                        .collect();
                    let want = intrinsic_argument(i, k, &prior);
                    match self.given(body, g, want.as_ref()) {
                        Lowering::Lowered(v) => values.push(v),
                        other => return other,
                    }
                }
                self.apply(i, values, expected, span)
            }
            Operation::Each(kind) => self.each(body, kind, &given, expected, span),
        }
    }

    /// A value given to an intrinsic: already lowered when it was piped.
    fn given(&mut self, body: &Body, g: &Given, want: Option<&Type>) -> Lowering<ValueId> {
        match g {
            Given::Value(v) => Lowering::Lowered(*v),
            Given::Expr(e) => self.expr(body, *e, want),
        }
    }

    /// **The type of the fold a seed is given to, as the checker solved
    /// it** (ADR-0283), where every part of it is one the backend lays out.
    /// `None` where the seed is no expression of the body being lowered, or
    /// the checker leaves a part of any type or unknown.
    fn solved_seed(&self, body: &Body, seed: &Given) -> Option<Type> {
        let Given::Expr(s) = seed else {
            return None;
        };
        let def = *self.inlining.last()?;
        let decl = crate::resolve::declaration(self.cx.hirs, def)?;
        let hir = self.cx.hirs[def.unit];
        if !std::ptr::eq(hir.body(decl.body?), body) {
            return None;
        }
        let call = body.walk().into_iter().find(|e| {
            matches!(body.expr(*e), Expr::Call { args, .. } if args.iter().any(|a| a.value == *s))
        })?;
        let module = hir.module_of(crate::hir::DeclId(def.decl));
        let (t, _) =
            crate::values::type_of(self.cx.sigs, self.cx.ws, def.unit, module, decl, body, call);
        self.solved(&t)
    }

    /// **A type the checker solved, as the backend lays it out**: `None`
    /// where a part holds at every type, or the program does not say.
    fn solved(&self, t: &crate::values::Ty) -> Option<Type> {
        use crate::values::Ty;
        let each = |args: &[Ty]| {
            args.iter()
                .map(|a| self.solved(a))
                .collect::<Option<Vec<_>>>()
        };
        Some(match t {
            Ty::Primitive(p) => match p {
                Primitive::Int => Type::Int,
                Primitive::Float => Type::Float,
                Primitive::Bool => Type::Bool,
                Primitive::Str => Type::Str,
                Primitive::Unit => Type::Unit,
            },
            Ty::Builtin(b, args) => match (b, each(args)?.as_slice()) {
                (Builtin::List, [e]) => Type::List(Box::new(e.clone())),
                (Builtin::Option, [e]) => Type::Option(Box::new(e.clone())),
                (Builtin::Result, [o, e]) => Type::Result(Box::new(o.clone()), Box::new(e.clone())),
                (Builtin::Map, [k, v]) if ordered_key(self.cx.sigs, k) => {
                    Type::Map(Box::new(k.clone()), Box::new(v.clone()))
                }
                (Builtin::Set, [e]) if ordered_key(self.cx.sigs, e) => {
                    Type::Set(Box::new(e.clone()))
                }
                _ => return None,
            },
            Ty::Nominal(def, args) => Type::Nominal(*def, each(args)?),
            Ty::Parameter { binder, index } => self.subst.get(&(*binder, *index))?.clone(),
            Ty::Var(_) | Ty::Unknown | Ty::Any => return None,
        })
    }

    /// **A loop over a list**, its body the function argument.
    fn each(
        &mut self,
        body: &Body,
        kind: EachKind,
        given: &[Given],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some((first, rest)) = given.split_first() else {
            return Lowering::Blocked {
                why: "a list operation with no list".to_string(),
                span,
            };
        };
        let list = match self.given(body, first, None) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let Some(Type::List(element)) = self.types.get(&list).cloned() else {
            return Lowering::Blocked {
                why: format!(
                    "`{kind:?}` over {:?}, which is not a list",
                    self.types.get(&list)
                ),
                span,
            };
        };
        let element = *element;
        let (seed, function) = match (kind, rest) {
            (EachKind::Fold, [seed, f]) => (Some(seed), f),
            (EachKind::Fold, _) => {
                return Lowering::Blocked {
                    why: "a fold takes a list, a seed and a function".to_string(),
                    span,
                };
            }
            (_, [f]) => (None, f),
            _ => {
                return Lowering::Blocked {
                    why: format!("`{kind:?}` takes a list and a function"),
                    span,
                };
            }
        };
        // **A seed its context does not type** (ADR-0283): `let all =
        // List.fold(xs, [], f)` expects nothing of `[]`, and the fold's own
        // type, as the checker solved it, is the seed's.
        let solved = match (kind, expected, seed) {
            (EachKind::Fold, None, Some(g)) => self.solved_seed(body, g),
            _ => None,
        };
        let seed = match seed {
            Some(g) => match self.given(body, g, expected.or(solved.as_ref())) {
                Lowering::Lowered(v) => Some(v),
                other => return other,
            },
            None => None,
        };
        let seed_ty = seed.and_then(|v| self.types.get(&v).cloned());
        let params = match (kind, &seed_ty) {
            (EachKind::Fold, Some(a)) => vec![a.clone(), element.clone()],
            (EachKind::Fold, None) => {
                return Lowering::Blocked {
                    why: "the fold's seed has no type".to_string(),
                    span,
                };
            }
            (EachKind::SortBy, _) => vec![element.clone(), element.clone()],
            _ => vec![element.clone()],
        };
        let body_expected = match kind {
            EachKind::Map => match expected {
                Some(Type::List(u)) => Some((**u).clone()),
                _ => None,
            },
            EachKind::Fold => seed_ty.clone(),
            EachKind::SortBy => Some(Type::Int),
            EachKind::GroupBy => Some(Type::Str),
            _ => Some(Type::Bool),
        };
        let Given::Expr(f) = function else {
            return Lowering::Blocked {
                why: "a piped value is not the function".to_string(),
                span,
            };
        };
        let (bound, region) =
            match self.function_region(body, *f, &params, body_expected.as_ref(), span.clone()) {
                Lowering::Lowered(x) => x,
                other => return other.map(|_| unreachable!()),
            };
        let produced = self.types.get(&region.value).cloned();
        let need = |t: Type| -> Result<(), String> {
            match &produced {
                Some(p) if *p == t => Ok(()),
                other => Err(format!(
                    "the function produces {other:?} where {t:?} is needed"
                )),
            }
        };
        let ty = match kind {
            EachKind::Map => match produced.clone() {
                Some(u) => Ok(Type::List(Box::new(u))),
                None => Err("the function's value has no type".to_string()),
            },
            EachKind::Filter => need(Type::Bool).map(|_| Type::List(Box::new(element))),
            EachKind::Any | EachKind::All => need(Type::Bool).map(|_| Type::Bool),
            EachKind::Find => need(Type::Bool).map(|_| Type::Option(Box::new(element))),
            EachKind::SortBy => need(Type::Int).map(|_| Type::List(Box::new(element))),
            EachKind::GroupBy => {
                need(Type::Str).map(|_| Type::List(Box::new(Type::List(Box::new(element)))))
            }
            EachKind::Fold => match seed_ty.clone() {
                Some(a) => need(a.clone()).map(|_| a),
                None => Err("the fold's seed has no type".to_string()),
            },
            // A `for` loop, which no list operation is (ADR-0051).
            EachKind::For => Err("`for` is a loop, not a list operation".to_string()),
        };
        let ty = match ty {
            Ok(t) => t,
            Err(why) => return Lowering::Blocked { why, span },
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Each {
            result,
            kind,
            list,
            seed,
            params: bound,
            body: region,
            ty,
        }))
    }

    /// An operation on values, typed by its arguments, or, for an empty map
    /// or set, by the type its context expects.
    fn apply(
        &mut self,
        op: Intrinsic,
        args: Vec<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        use Intrinsic as I;
        let types: Vec<Option<Type>> = args.iter().map(|v| self.types.get(v).cloned()).collect();
        let ty = match (op, types.as_slice()) {
            (I::MapEmpty, []) => match expected {
                Some(t @ Type::Map(..)) => t.clone(),
                _ => {
                    return Lowering::Unsupported {
                        construct: "an empty map whose types nothing fixes",
                        span,
                        reason: "`Map.empty()` needs the type its context expects".to_string(),
                    };
                }
            },
            (I::SetEmpty, []) => match expected {
                Some(t @ Type::Set(..)) => t.clone(),
                _ => {
                    return Lowering::Unsupported {
                        construct: "an empty set whose type nothing fixes",
                        span,
                        reason: "`Set.empty()` needs the type its context expects".to_string(),
                    };
                }
            },
            (I::MapSize, [Some(Type::Map(..))]) | (I::SetSize, [Some(Type::Set(..))]) => Type::Int,
            (I::MapGet, [Some(Type::Map(k, v)), Some(key)]) if **k == *key => {
                Type::Option(v.clone())
            }
            (I::MapContains, [Some(Type::Map(k, _)), Some(key)]) if **k == *key => Type::Bool,
            (I::MapInsert, [Some(m @ Type::Map(k, v)), Some(key), Some(value)])
                if **k == *key && **v == *value =>
            {
                m.clone()
            }
            (I::MapRemove, [Some(m @ Type::Map(k, _)), Some(key)]) if **k == *key => m.clone(),
            (I::MapKeys, [Some(Type::Map(k, _))]) => Type::List(k.clone()),
            (I::MapValues, [Some(Type::Map(_, v))]) => Type::List(v.clone()),
            (I::MapFromLists, [Some(Type::List(k)), Some(Type::List(v))])
                if ordered_key(self.cx.sigs, k) =>
            {
                Type::Map(k.clone(), v.clone())
            }
            (I::MapCheck, [Some(m @ Type::Map(..))]) | (I::SetCheck, [Some(m @ Type::Set(..))]) => {
                m.clone()
            }
            (I::SetFromList, [Some(Type::List(t))]) if ordered_key(self.cx.sigs, t) => {
                Type::Set(t.clone())
            }
            (I::SetContains, [Some(Type::Set(t)), Some(x)]) if **t == *x => Type::Bool,
            (I::SetInsert | I::SetRemove, [Some(s @ Type::Set(t)), Some(x)]) if **t == *x => {
                s.clone()
            }
            (I::SetToList, [Some(Type::Set(t))]) => Type::List(t.clone()),
            (
                I::SetUnion | I::SetIntersection | I::SetDifference,
                [Some(a @ Type::Set(_)), Some(b)],
            ) if a == b => a.clone(),
            (I::ListLength, [Some(Type::List(_))]) => Type::Int,
            (I::ListGet, [Some(Type::List(t)), Some(Type::Int)]) => Type::Option(t.clone()),
            (I::ListTake | I::ListDrop, [Some(t @ Type::List(_)), Some(Type::Int)]) => t.clone(),
            (I::ListSlice, [Some(t @ Type::List(_)), Some(Type::Int), Some(Type::Int)]) => {
                t.clone()
            }
            (I::ListReverse, [Some(t @ Type::List(_))]) => t.clone(),
            (I::ListRange, [Some(Type::Int), Some(Type::Int)]) => Type::List(Box::new(Type::Int)),
            (I::StrSlice, [Some(Type::Str), Some(Type::Int), Some(Type::Int)]) => Type::Str,
            (I::ListConcat, [Some(a @ Type::List(_)), Some(b)]) if a == b => a.clone(),
            (I::StrLength, [Some(Type::Str)]) => Type::Int,
            (I::StrCodepoints, [Some(Type::Str)]) => Type::List(Box::new(Type::Int)),
            (I::StrFromCodepoints, [Some(Type::List(t))]) if **t == Type::Int => Type::Str,
            (
                I::StrStartsWith | I::StrEndsWith | I::StrContains,
                [Some(Type::Str), Some(Type::Str)],
            ) => Type::Bool,
            (I::StrJoin, [Some(Type::List(t)), Some(Type::Str)]) if **t == Type::Str => Type::Str,
            (
                I::StrTrim | I::StrToLowerAscii | I::StrToLower | I::StrToUpper,
                [Some(Type::Str)],
            ) => Type::Str,
            (I::FloatFromInt, [Some(Type::Int)]) => Type::Float,
            (op, types) => {
                return Lowering::Blocked {
                    why: format!("`{op:?}` receives {types:?}"),
                    span,
                };
            }
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Intrinsic {
            result,
            op,
            args,
            ty,
        }))
    }

    /// **A function argument, as a region** (ADR-0040 §3): a lambda's body, or
    /// a named declaration's, with its parameters bound to fresh values of
    /// `params`' types. Known where it is written, so nothing is a closure.
    fn function_region(
        &mut self,
        body: &Body,
        f: ExprId,
        params: &[Type],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<(Vec<ValueId>, Region)> {
        match body.expr(f).clone() {
            Expr::Lambda {
                descriptor: None,
                params: patterns,
                body: inner,
            } => {
                // The typer's reading of the parameters, so the two agree.
                let Some(names) = crate::values::lambda_names(body, &patterns) else {
                    return Lowering::Unsupported {
                        construct: "a lambda parameter that is not a name",
                        span,
                        reason: "a function's parameters are names here".to_string(),
                    };
                };
                if names.len() != params.len() {
                    return Lowering::Blocked {
                        why: format!(
                            "the function takes {} parameters and receives {}",
                            names.len(),
                            params.len()
                        ),
                        span,
                    };
                }
                let bound = self.fresh_typed(params);
                let scope = self.locals.clone();
                for (name, v) in names.iter().zip(&bound) {
                    self.locals.insert(name.clone(), *v);
                }
                self.in_lambda += 1;
                let region = self.region(body, inner, expected);
                self.in_lambda -= 1;
                self.locals = scope;
                region.map(|r| (bound, r))
            }
            Expr::Lambda { .. } => Lowering::Unsupported {
                construct: "a resumable lambda as a function argument",
                span,
                reason: "its descriptor belongs to a handler".to_string(),
            },
            // A function value a binding holds (ADR-0052): called, once per
            // element.
            Expr::Name(n) if self.locals.contains_key(&n) => {
                let f = self.locals[&n];
                let Some(Type::Function(ps, r)) = self.types.get(&f).cloned() else {
                    return Lowering::Blocked {
                        why: format!("`{n}` is passed as a function and holds none"),
                        span,
                    };
                };
                if ps.as_slice() != params {
                    return Lowering::Blocked {
                        why: format!("`{n}` takes {ps:?}, and is given {params:?}"),
                        span,
                    };
                }
                let bound = self.fresh_typed(params);
                let outer = std::mem::take(&mut self.instrs);
                let f = if self.vars.contains(&f) {
                    self.read(f)
                } else {
                    f
                };
                let result = self.fresh();
                self.push(Instr::Apply {
                    result,
                    function: f,
                    function_ty: Type::Function(ps, r.clone()),
                    args: bound.clone(),
                    ty: *r,
                });
                let instrs = std::mem::replace(&mut self.instrs, outer);
                Lowering::Lowered((
                    bound,
                    Region {
                        instrs,
                        value: result,
                    },
                ))
            }
            Expr::Name(_) | Expr::Field { .. } => {
                if !self.declaration_path(body, f) {
                    return Lowering::Unsupported {
                        construct: "a local member used as a callable",
                        span,
                        reason: "this callable value needs receiver lowering".to_string(),
                    };
                }
                let path = crate::infer::path_of(body, f);
                // A sum type's case (ADR-0059): `List.map(radii,
                // Shape.Circle)` builds one per element.
                let case = match path.contains('.') {
                    true => match self.expression_case(body, f) {
                        Some(crate::values::CaseNamed::Case(def, index)) => Some((def, index)),
                        _ => None,
                    },
                    false => crate::values::bare_case(self.cx.sigs, self.cx.ws, self.unit, &path),
                };
                if let Some((def, index)) = case {
                    // The instance the elements' types fix (ADR-0062).
                    let instance = match self.case_instance(def, index, params) {
                        Some(i) => i,
                        None => {
                            return Lowering::Blocked {
                                why: format!(
                                    "`{path}` is passed where a function of {params:?} is"
                                ),
                                span,
                            };
                        }
                    };
                    let cases = match self.declared_cases(def, &instance, &span) {
                        Lowering::Lowered(c) => c,
                        other => return other.map(|_| unreachable!()),
                    };
                    if cases.get(index).map(|(_, fields)| fields.as_slice()) != Some(params) {
                        return Lowering::Blocked {
                            why: format!("`{path}` is passed where a function of {params:?} is"),
                            span,
                        };
                    }
                    let ty = Type::Nominal(def, instance);
                    let bound = self.fresh_typed(params);
                    let result = self.fresh();
                    let instrs = vec![Instr::Case {
                        result,
                        case: index as u32,
                        fields: bound.clone(),
                        ty: ty.clone(),
                    }];
                    self.types.insert(result, ty);
                    return Lowering::Lowered((
                        bound,
                        Region {
                            instrs,
                            value: result,
                        },
                    ));
                }
                let resolution = match path.contains('.') {
                    true => self.cx.ws.resolve_path(self.unit, &path),
                    false => self.cx.ws.resolve_in(self.unit, Namespace::Term, &path),
                };
                let def = match resolution {
                    Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
                    _ => {
                        return Lowering::Blocked {
                            why: format!("`{path}` names no declaration here"),
                            span,
                        };
                    }
                };
                let bound = self.fresh_typed(params);
                let outer = std::mem::take(&mut self.instrs);
                let value = match crate::resolve::declaration(self.cx.hirs, def)
                    .and_then(crate::backend::intrinsic_binding)
                {
                    Some(Ok(Operation::Intrinsic(i))) => self.apply(i, bound.clone(), None, span),
                    Some(_) => Lowering::Unsupported {
                        construct: "a list operation passed as a function",
                        span,
                        reason: format!("`{path}` takes a function itself"),
                    },
                    None => self.inline(def, bound.clone(), None, span),
                };
                let instrs = std::mem::replace(&mut self.instrs, outer);
                value.map(|v| (bound, Region { instrs, value: v }))
            }
            _ => Lowering::Unsupported {
                construct: "a function argument that is not a lambda or a declaration's name",
                span,
                reason: "a function is known where it is passed (ADR-0040 §3)".to_string(),
            },
        }
    }

    /// Fresh values of these types.
    fn fresh_typed(&mut self, types: &[Type]) -> Vec<ValueId> {
        types
            .iter()
            .map(|t| {
                let v = self.fresh();
                self.types.insert(v, t.clone());
                v
            })
            .collect()
    }

    /// **A call to another Pleris declaration, inlined** (ADR-0039 §4): its
    /// body, lowered with its parameters bound to the arguments.
    fn inline(
        &mut self,
        callee: DefId,
        args: Vec<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some(decl) = crate::resolve::declaration(self.cx.hirs, callee) else {
            return Lowering::Blocked {
                why: "a callee no unit holds".to_string(),
                span,
            };
        };
        let Some(body_id) = decl.body else {
            return Lowering::Unsupported {
                construct: "a call to a declaration with no body",
                span,
                reason: format!(
                    "`{}` has neither a body nor a `host` binding to call",
                    decl.name
                ),
            };
        };
        let Some(sig) = self.cx.sigs.by_def(callee) else {
            return Lowering::Blocked {
                why: format!("`{}` has no resolved signature", decl.name),
                span,
            };
        };
        if args.len() != decl.params.len() {
            return Lowering::Blocked {
                why: format!(
                    "`{}` takes {} arguments and receives {}",
                    decl.name,
                    decl.params.len(),
                    args.len()
                ),
                span,
            };
        }

        // **The instance**: each type parameter the callee declares, bound by
        // what its arguments are (ADR-0050). Until 2026-09-25 a generic callee
        // was refused, because nothing specialized it.
        let mut subst = BTreeMap::new();
        for (i, (p, a)) in decl.params.iter().zip(&args).enumerate() {
            let Some(declared) = sig
                .params
                .get(i)
                .and_then(Option::as_ref)
                .and_then(TypeResolution::resolved)
            else {
                return Lowering::Unsupported {
                    construct: "an unannotated parameter",
                    span,
                    reason: format!("`{}`'s `{}` has no declared type", decl.name, p.name),
                };
            };
            let Some(actual) = self.types.get(a).cloned() else {
                return Lowering::Blocked {
                    why: format!("`{}`'s argument `{}` has no type", decl.name, p.name),
                    span,
                };
            };
            if !instantiate(self.cx.sigs, declared, &actual, &mut subst) {
                return Lowering::Blocked {
                    why: format!(
                        "`{}` receives a {actual:?} as `{}`, declared `{declared}`",
                        decl.name, p.name
                    ),
                    span,
                };
            }
        }
        // A parameter only the result mentions is what the use needs:
        // `fn empty<T>() -> List<T>` where a `List<Int>` is wanted.
        let returns = sig.returns.as_ref().and_then(TypeResolution::resolved);
        if let (Some(r), Some(want)) = (returns, expected) {
            let mut trial = subst.clone();
            if instantiate(self.cx.sigs, r, want, &mut trial) {
                subst = trial;
            }
        }
        let mut instance = Vec::new();
        for index in 0..decl.type_params.len() as u32 {
            match subst.get(&(callee, index)) {
                Some(t) => instance.push(t.clone()),
                None => {
                    return Lowering::Unsupported {
                        construct: "a type parameter no call instantiates",
                        span,
                        reason: format!(
                            "`{}`'s `{}` is fixed by neither its arguments nor its use",
                            decl.name, decl.type_params[index as usize]
                        ),
                    };
                }
            }
        }
        let ret = match returns {
            Some(r) => match ty_resolved_with(self.cx.sigs, r, &span, &subst) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            },
            None if sig.returns.is_some() => {
                return Lowering::Blocked {
                    why: format!("`{}`'s result does not resolve", decl.name),
                    span,
                };
            }
            None => Type::Unit,
        };

        // **A recursion is a call** (ADR-0050): the callee is compiled once,
        // beside the export, at this instance, and called. Until 2026-09-25
        // it was refused: calls are inlined (ADR-0039 §4), and an inlined
        // recursion has no end. So is a callee with a `return` or a `?`
        // (ADR-0051), which leaves the callee and would, inlined, leave its
        // caller.
        let callee_body = self.cx.hirs[callee.unit].body(body_id);
        if self.inlining.contains(&callee) || exits_early(callee_body) {
            let key = (callee, instance.clone());
            let begun = !self.internal.borrow_mut().started.insert(key);
            if !begun {
                match lower_internal(
                    self.cx,
                    self.internal,
                    callee,
                    instance.clone(),
                    subst,
                    span.clone(),
                ) {
                    Lowering::Lowered(f) => self.internal.borrow_mut().done.push(f),
                    other => return other.map(|_| unreachable!()),
                }
            }
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::Call {
                result,
                callee,
                instance,
                args,
                ty: ret,
            }));
        }

        let mut bound = BTreeMap::new();
        for (p, a) in decl.params.iter().zip(&args) {
            bound.insert(p.name.clone(), *a);
        }
        let body = self.cx.hirs[callee.unit].body(body_id);
        let caller = (
            std::mem::replace(&mut self.locals, bound),
            self.unit,
            std::mem::replace(&mut self.subst, subst),
        );
        self.unit = callee.unit;
        self.inlining.push(callee);
        let out = self.expr(body, body.root, Some(&ret));
        self.inlining.pop();
        (self.locals, self.unit, self.subst) = caller;
        let v = match out {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        match self.types.get(&v) {
            Some(t) if *t == ret => Lowering::Lowered(v),
            other => Lowering::Blocked {
                why: format!("`{}` produces a {other:?} and declares {ret:?}", decl.name),
                span,
            },
        }
    }

    /// **A sum type's case, built** (ADR-0059): `Shape.Circle(3)`, from its
    /// payload's fields positionally, each lowered against its declared
    /// type. A piped value is the first field. A generic type's instance is
    /// the one the context names, or the one its fields fix (ADR-0062).
    #[allow(clippy::too_many_arguments)]
    fn case(
        &mut self,
        body: &Body,
        def: DefId,
        index: usize,
        args: &[crate::hir::Arg],
        piped: Option<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some((_, fields)) = self
            .cx
            .sigs
            .type_decl(def)
            .and_then(|t| t.variants.as_ref())
            .and_then(|cases| cases.get(index))
            .cloned()
        else {
            return Lowering::Blocked {
                why: "a case its type does not declare; the checker refuses it (PW0608)".into(),
                span,
            };
        };
        if args.iter().any(|a| a.name.is_some()) {
            return Lowering::Unsupported {
                construct: "a case built with named fields",
                span,
                reason: "a case's fields are positional".to_string(),
            };
        }
        let given_count = usize::from(piped.is_some()) + args.len();
        if given_count != fields.len() {
            return Lowering::Blocked {
                why: format!(
                    "the case takes {} fields and is given {given_count}; the checker refuses it \
                     (PW0604)",
                    fields.len()
                ),
                span,
            };
        }
        let given: Vec<Given> = piped
            .map(Given::Value)
            .into_iter()
            .chain(args.iter().map(|a| Given::Expr(a.value)))
            .collect();
        let (values, instance) =
            match self.instance_fields(body, def, &fields, given, expected, &span) {
                Lowering::Lowered(x) => x,
                other => return other.map(|_| unreachable!()),
            };
        // The instance's cases resolve, or it is refused here, by name.
        if let other @ (Lowering::Unsupported { .. } | Lowering::Blocked { .. }) =
            self.declared_cases(def, &instance, &span)
        {
            return other.map(|_| unreachable!());
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Case {
            result,
            case: index as u32,
            fields: values,
            ty: Type::Nominal(def, instance),
        }))
    }

    /// **A case named as a value** (ADR-0059): one without a payload is its
    /// value, `Shape.Empty`; one with a payload, where a function is wanted,
    /// is a function building it: `List.map(radii, Shape.Circle)`.
    fn case_value(
        &mut self,
        def: DefId,
        index: usize,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        // A generic type's instance is the one its use names (ADR-0062):
        // `Maybe.Nothing` where a `Maybe<Int>` is wanted, or the case's
        // function where a `fn(Int) -> Maybe<Int>` is.
        let instance = match expected {
            Some(Type::Nominal(d, args)) if *d == def => args.clone(),
            Some(Type::Function(_, r)) => match &**r {
                Type::Nominal(d, args) if *d == def => args.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let cases = match self.declared_cases(def, &instance, &span) {
            Lowering::Lowered(c) => c,
            other => return other.map(|_| unreachable!()),
        };
        let Some((_, fields)) = cases.get(index).cloned() else {
            return Lowering::Blocked {
                why: "a case its type does not declare; the checker refuses it (PW0608)".into(),
                span,
            };
        };
        let ty = Type::Nominal(def, instance);
        if fields.is_empty() {
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::Case {
                result,
                case: index as u32,
                fields: Vec::new(),
                ty,
            }));
        }
        let Some(Type::Function(ps, r)) = expected.cloned() else {
            return Lowering::Unsupported {
                construct: "a case with a payload named without its fields",
                span,
                reason: "it is the function that builds the case, and nothing here wants a \
                         function; call it with its fields"
                    .to_string(),
            };
        };
        if ps != fields || *r != ty {
            return Lowering::Blocked {
                why: format!("the case is not a {:?}", Type::Function(ps, r)),
                span,
            };
        }
        // Its code: its parameters, built into the case. It captures nothing.
        let slot = self.reserve_closure();
        let params: Vec<(ValueId, Type)> = fields
            .iter()
            .enumerate()
            .map(|(i, t)| (ValueId(i as u32), t.clone()))
            .collect();
        let built = ValueId(params.len() as u32);
        let owner = self
            .inlining
            .last()
            .copied()
            .unwrap_or(DefId { unit: 0, decl: 0 });
        let code = Function {
            def: owner,
            export: "case".to_string(),
            params: params.clone(),
            ret: ty.clone(),
            blocks: vec![Block {
                id: BlockId(0),
                instrs: vec![Instr::Case {
                    result: built,
                    case: index as u32,
                    fields: params.iter().map(|(v, _)| *v).collect(),
                    ty: ty.clone(),
                }],
                terminator: Terminator::Return(built),
            }],
            capabilities: Vec::new(),
            instance: Vec::new(),
            callees: Vec::new(),
            closures: Vec::new(),
        };
        self.internal.borrow_mut().closures[slot] = Some(Closure {
            captures: 0,
            function: code,
        });
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Closure {
            result,
            index: slot as u32,
            captures: Vec::new(),
            ty: Type::Function(ps, r),
        }))
    }

    /// **`PositiveInt(1)`** (ADR-0054): the representation, lowered as what
    /// it is, and given the opaque type.
    #[allow(clippy::too_many_arguments)]
    fn opaque(
        &mut self,
        body: &Body,
        def: DefId,
        rep: &TypeResolution,
        args: &[crate::hir::Arg],
        piped: Option<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let given = match (piped, args) {
            (Some(v), []) => Given::Value(v),
            (None, [a]) => Given::Expr(a.value),
            _ => {
                return Lowering::Blocked {
                    why: "an opaque type is built from one value".to_string(),
                    span,
                };
            }
        };
        // A generic one's instance is the one the representation's type
        // fixes, or its use names (ADR-0062).
        let (values, instance) = match self.instance_fields(
            body,
            def,
            std::slice::from_ref(rep),
            vec![given],
            expected,
            &span,
        ) {
            Lowering::Lowered(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Retype {
            result,
            value: values[0],
            ty: Type::Nominal(def, instance),
        }))
    }

    /// **A lambda as a value** (ADR-0052): its code compiled as a function of
    /// its captures and its parameters, and a value holding the captures.
    /// Its type is the one its use gives it: an annotation, or the parameter
    /// it is passed as.
    fn closure(
        &mut self,
        body: &Body,
        params: &[crate::hir::PatternId],
        inner: ExprId,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some(Type::Function(param_types, ret)) = expected.cloned() else {
            return Lowering::Unsupported {
                construct: "a lambda whose type nothing fixes",
                span,
                reason: "a function value's parameter types come from where it is used: \
                         an annotation, or the parameter it is passed as"
                    .to_string(),
            };
        };
        let Some(names) = crate::values::lambda_names(body, params) else {
            return Lowering::Unsupported {
                construct: "a lambda parameter that is not a name",
                span,
                reason: "a function's parameters are names here".to_string(),
            };
        };
        if names.len() != param_types.len() {
            return Lowering::Blocked {
                why: format!(
                    "a lambda of {} parameters where {} are taken",
                    names.len(),
                    param_types.len()
                ),
                span,
            };
        }
        // What it captures: every name its body reads that a binding here
        // holds, as it holds it now.
        let mut captured: BTreeMap<String, ValueId> = BTreeMap::new();
        for id in body.walk_from(inner) {
            let read: Vec<&String> = match body.expr(id) {
                Expr::Name(n) => vec![n],
                Expr::Record { fields, .. } => fields
                    .iter()
                    .filter(|f| f.value.is_none())
                    .map(|f| &f.name)
                    .collect(),
                _ => Vec::new(),
            };
            for n in read {
                if !names.contains(n)
                    && let Some(v) = self.locals.get(n)
                {
                    captured.insert(n.clone(), *v);
                }
            }
        }
        let mut values = Vec::new();
        let mut captures = Vec::new();
        for (n, v) in captured {
            let v = if self.vars.contains(&v) {
                self.read(v)
            } else {
                v
            };
            let Some(t) = self.types.get(&v).cloned() else {
                return Lowering::Blocked {
                    why: format!("`{n}` is captured and has no type"),
                    span,
                };
            };
            values.push(v);
            captures.push((n, t));
        }
        let index = self.reserve_closure();
        let owner = self
            .inlining
            .last()
            .copied()
            .unwrap_or(DefId { unit: 0, decl: 0 });
        let code = match lower_closure(
            self.cx,
            self.internal,
            owner,
            self.unit,
            body,
            captures,
            names.into_iter().zip(param_types.iter().cloned()).collect(),
            inner,
            (*ret).clone(),
            self.subst.clone(),
            self.inlining.clone(),
            span,
        ) {
            Lowering::Lowered(f) => f,
            other => return other.map(|_| unreachable!()),
        };
        self.internal.borrow_mut().closures[index] = Some(Closure {
            captures: values.len(),
            function: code,
        });
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Closure {
            result,
            index: index as u32,
            captures: values,
            ty: Type::Function(param_types, ret),
        }))
    }

    /// **A binding's written type**, resolved where the declaration being
    /// lowered is written, under its instance (ADR-0052). `None` where it
    /// does not resolve, and the binding is lowered as if unwritten; the
    /// checker reports a written type that names nothing (PW0026).
    fn annotation(&self, body: &Body, ty: crate::hir::TypeRefId, span: &Span) -> Option<Type> {
        let written = crate::resolved::written_in_body(body, ty)?;
        let def = *self.inlining.last()?;
        let decl = crate::resolve::declaration(self.cx.hirs, def)?;
        let module = self.cx.hirs[def.unit].module_of(crate::hir::DeclId(def.decl));
        let resolution = self
            .cx
            .sigs
            .resolve_type(module, decl, &written, span.clone());
        match self.ty(&resolution, span) {
            Lowering::Lowered(t) => Some(t),
            _ => None,
        }
    }

    /// A slot for a function value's code, filled once it lowers.
    fn reserve_closure(&mut self) -> usize {
        let mut i = self.internal.borrow_mut();
        i.closures.push(None);
        i.closures.len() - 1
    }

    /// **A declaration's name, where a function value is wanted**
    /// (ADR-0052): its body compiled as the value's code, at the instance
    /// the wanted type gives it. It captures nothing.
    fn declaration_value(
        &mut self,
        body: &Body,
        e: ExprId,
        expected: Option<Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some(Type::Function(ps, r)) = expected else {
            unreachable!("asked for a function value")
        };
        let path = crate::infer::path_of(body, e);
        let def = match self.cx.ws.resolve_in(self.unit, Namespace::Term, &path) {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
            _ => {
                return Lowering::Blocked {
                    why: format!("`{path}` names no declaration here"),
                    span,
                };
            }
        };
        let (Some(decl), Some(sig)) = (
            crate::resolve::declaration(self.cx.hirs, def),
            self.cx.sigs.by_def(def),
        ) else {
            return Lowering::Blocked {
                why: format!("`{path}` has no resolved signature"),
                span,
            };
        };
        if crate::backend::intrinsic_binding(decl).is_some()
            || crate::backend::host_binding(decl).is_some()
        {
            return Lowering::Unsupported {
                construct: "a standard-library or host operation as a function value",
                span,
                reason: format!("`{path}` is supplied by the compiler or the host, not compiled"),
            };
        }
        let mut subst = BTreeMap::new();
        let declared: Vec<Option<&ResolvedType>> = sig
            .params
            .iter()
            .map(|p| p.as_ref().and_then(TypeResolution::resolved))
            .collect();
        let fits = declared.len() == ps.len()
            && declared
                .iter()
                .zip(&ps)
                .all(|(d, p)| d.is_some_and(|d| instantiate(self.cx.sigs, d, p, &mut subst)));
        let returns = sig.returns.as_ref().and_then(TypeResolution::resolved);
        if !fits || !returns.is_some_and(|d| instantiate(self.cx.sigs, d, &r, &mut subst)) {
            return Lowering::Blocked {
                why: format!("`{path}` is not a {:?}", Type::Function(ps, r)),
                span,
            };
        }
        let mut instance = Vec::new();
        for index in 0..decl.type_params.len() as u32 {
            match subst.get(&(def, index)) {
                Some(t) => instance.push(t.clone()),
                None => {
                    return Lowering::Unsupported {
                        construct: "a type parameter no use instantiates",
                        span,
                        reason: format!("`{path}` is generic, and its use fixes no instance"),
                    };
                }
            }
        }
        let index = self.reserve_closure();
        let code = match lower_internal(self.cx, self.internal, def, instance, subst, span.clone())
        {
            Lowering::Lowered(f) => f,
            other => return other.map(|_| unreachable!()),
        };
        self.internal.borrow_mut().closures[index] = Some(Closure {
            captures: 0,
            function: code,
        });
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Closure {
            result,
            index: index as u32,
            captures: Vec::new(),
            ty: Type::Function(ps, r),
        }))
    }

    /// **`f(a, b)`, where `f` holds a function value** (ADR-0052).
    #[allow(clippy::too_many_arguments)]
    fn call_value(
        &mut self,
        body: &Body,
        f: ValueId,
        params: &[Type],
        ret: Type,
        args: &[crate::hir::Arg],
        piped: Option<ValueId>,
        span: Span,
    ) -> Lowering<ValueId> {
        let mut lowered: Vec<ValueId> = piped.into_iter().collect();
        for a in args {
            let i = lowered.len();
            match self.expr(body, a.value, params.get(i)) {
                Lowering::Lowered(v) => lowered.push(v),
                other => return other,
            }
        }
        if lowered.len() != params.len() {
            return Lowering::Blocked {
                why: format!(
                    "a function of {} parameters is given {}",
                    params.len(),
                    lowered.len()
                ),
                span,
            };
        }
        for (v, p) in lowered.iter().zip(params) {
            if self.types.get(v) != Some(p) {
                return Lowering::Blocked {
                    why: format!("a function taking {p:?} is given a {:?}", self.types.get(v)),
                    span,
                };
            }
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Apply {
            result,
            function: f,
            function_ty: Type::Function(params.to_vec(), Box::new(ret.clone())),
            args: lowered,
            ty: ret,
        }))
    }

    /// **`return e`** (ADR-0051): `e`, of the function's result type, and the
    /// function left with it. The value the enclosing region stands for is a
    /// placeholder of the type it expects, never read.
    fn early_return(
        &mut self,
        body: &Body,
        value: Option<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        if self.in_lambda > 0 {
            return Lowering::Unsupported {
                construct: "a `return` inside a function value",
                span,
                reason: "the lambda a list operation runs is compiled into its loop; a \
                         `return` there would leave the function around it"
                    .to_string(),
            };
        }
        let ret = self.ret.clone();
        let v = match value {
            Some(e) => match self.expr(body, e, Some(&ret)) {
                Lowering::Lowered(v) => v,
                other => return other,
            },
            None => self.unit(),
        };
        if self.types.get(&v) != Some(&ret) {
            return Lowering::Blocked {
                why: format!(
                    "`return` gives a {:?}, and the function returns {ret:?}",
                    self.types.get(&v)
                ),
                span,
            };
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Return {
            result,
            value: v,
            ty: expected.cloned().unwrap_or(Type::Unit),
        }))
    }

    /// **The case a case written alone names, at `e`** (ADR-0201): the one
    /// type with it, or the one the checker's typer expects where it is
    /// written, so the backend builds the case the checker typed.
    fn case_written_alone(&self, body: &Body, e: ExprId) -> Option<(DefId, usize)> {
        let def = *self.inlining.last()?;
        let hir = self.cx.hirs.get(def.unit)?;
        let id = crate::hir::DeclId(def.decl);
        crate::values::case_at(
            self.cx.sigs,
            self.cx.ws,
            def.unit,
            hir.module_of(id),
            hir.decl(id),
            body,
            e,
        )
    }

    /// The unit value.
    fn unit(&mut self) -> ValueId {
        let result = self.fresh();
        self.push(Instr::Const {
            result,
            value: Const::Unit,
            ty: Type::Unit,
        })
    }

    /// What the variable `local` holds here.
    fn read(&mut self, local: ValueId) -> ValueId {
        let ty = self.types.get(&local).cloned().unwrap_or(Type::Unit);
        let result = self.fresh();
        self.push(Instr::Get { result, local, ty })
    }

    /// A region that returns, retyped to what its siblings produce: its
    /// placeholder stands for a value of that type (ADR-0051).
    fn retype_divergent(&mut self, r: &mut Region, ty: &Type) {
        if let Some(Instr::Return { result, ty: t, .. }) = r.instrs.last_mut()
            && *result == r.value
        {
            *t = ty.clone();
            self.types.insert(*result, ty.clone());
        }
    }

    /// **`x = e`** (ADR-0051): `e`, of the variable's type, held by it from
    /// here on.
    fn assign(&mut self, body: &Body, lhs: ExprId, rhs: ExprId, span: Span) -> Lowering<ValueId> {
        let Expr::Name(n) = body.expr(lhs) else {
            return Lowering::Unsupported {
                construct: "an assignment to something that is not a binding",
                span,
                reason: "a variable is assigned; a field of a value is not".to_string(),
            };
        };
        // A page's signal, changed through the handler's context (ADR-0130).
        if !self.locals.contains_key(n)
            && let Some(ty) = self.signals.get(n).cloned()
        {
            if self.in_lambda > 0 {
                return Lowering::Unsupported {
                    construct: "a signal changed inside a function value",
                    span,
                    reason: "a lambda a list operation runs is not the handler's own body"
                        .to_string(),
                };
            }
            let v = match self.expr(body, rhs, Some(&ty)) {
                Lowering::Lowered(v) => v,
                other => return other,
            };
            if self.types.get(&v) != Some(&ty) {
                return Lowering::Blocked {
                    why: format!(
                        "the signal `{n}` holds a {ty:?}, and is assigned a {:?}",
                        self.types.get(&v)
                    ),
                    span,
                };
            }
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::SignalSet {
                result,
                signal: n.clone(),
                value: v,
                ty: Type::Unit,
            }));
        }
        let Some(var) = self
            .locals
            .get(n)
            .copied()
            .filter(|v| self.vars.contains(v))
        else {
            return Lowering::Blocked {
                why: format!("`{n}` is not a mutable binding; the checker refuses it (PW0611)"),
                span,
            };
        };
        if self.in_lambda > 0 {
            return Lowering::Unsupported {
                construct: "an assignment inside a function value",
                span,
                reason: "a lambda a list operation runs does not change the bindings \
                         around it"
                    .to_string(),
            };
        }
        let ty = self.types.get(&var).cloned().unwrap_or(Type::Unit);
        let v = match self.expr(body, rhs, Some(&ty)) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        if self.types.get(&v) != Some(&ty) {
            return Lowering::Blocked {
                why: format!(
                    "`{n}` holds a {ty:?}, and is assigned a {:?}",
                    self.types.get(&v)
                ),
                span,
            };
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Set {
            result,
            local: var,
            value: v,
            ty: Type::Unit,
        }))
    }

    /// **`if c { .. }` as a statement** (ADR-0051): the branch runs for what
    /// it does, and the `if`'s value is the unit value either way.
    fn statement_if(
        &mut self,
        body: &Body,
        cond: ExprId,
        then: ExprId,
        span: Span,
    ) -> Lowering<ValueId> {
        let cond = match self.typed(body, cond, &Type::Bool) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let outer = std::mem::take(&mut self.instrs);
        let v = self.expr(body, then, None);
        let value = match v {
            Lowering::Lowered(v) if self.types.get(&v) == Some(&Type::Unit) => v,
            Lowering::Lowered(v)
                if diverges(&Region {
                    instrs: self.instrs.clone(),
                    value: v,
                }) =>
            {
                self.retype_last_return();
                v
            }
            Lowering::Lowered(_) => self.unit(),
            other => {
                self.instrs = outer;
                return other;
            }
        };
        let instrs = std::mem::replace(&mut self.instrs, outer);
        let then = Region { instrs, value };
        let outer = std::mem::take(&mut self.instrs);
        let unit = self.unit();
        let els = Region {
            instrs: std::mem::replace(&mut self.instrs, outer),
            value: unit,
        };
        let _ = span;
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::If {
            result,
            cond,
            then,
            els,
            ty: Type::Unit,
        }))
    }

    /// The last instruction is a `return` standing for a unit value.
    fn retype_last_return(&mut self) {
        if let Some(Instr::Return { result, ty, .. }) = self.instrs.last_mut() {
            *ty = Type::Unit;
            self.types.insert(*result, Type::Unit);
        }
    }

    /// **`for x in xs { .. }`** (ADR-0051): the body once per element, with
    /// `x` bound; its value discarded, and the loop's the unit value.
    fn for_loop(
        &mut self,
        body: &Body,
        pat: Option<crate::hir::PatternId>,
        iterable: ExprId,
        inner: ExprId,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some(Pattern::Bind { name, .. }) = pat.map(|p| body.pat(p)).cloned() else {
            return Lowering::Unsupported {
                construct: "a `for` pattern that is not a name",
                span,
                reason: "`for x in xs` binds one name here".to_string(),
            };
        };
        let list = match self.expr(body, iterable, None) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let Some(Type::List(et)) = self.types.get(&list).cloned() else {
            return Lowering::Blocked {
                why: "`for` over a value that is not a list".to_string(),
                span,
            };
        };
        let x = self.fresh();
        self.types.insert(x, *et);
        let scope = self.locals.clone();
        self.locals.insert(name, x);
        let outer = std::mem::take(&mut self.instrs);
        let v = self.expr(body, inner, None);
        let unit = self.unit();
        let instrs = std::mem::replace(&mut self.instrs, outer);
        self.locals = scope;
        if let other @ (Lowering::Unsupported { .. } | Lowering::Blocked { .. }) = v {
            return other;
        }
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Each {
            result,
            kind: EachKind::For,
            list,
            seed: None,
            params: vec![x],
            body: Region {
                instrs,
                value: unit,
            },
            ty: Type::Unit,
        }))
    }

    /// **`e?`** (ADR-0051): `e`'s success, or the function left with its
    /// failure: `None` from an `Option`, `Err(x)` from a `Result`.
    fn propagate(&mut self, body: &Body, value: ExprId, span: Span) -> Lowering<ValueId> {
        if self.in_lambda > 0 {
            return Lowering::Unsupported {
                construct: "a `?` inside a function value",
                span,
                reason: "the lambda a list operation runs is compiled into its loop; a \
                         `?` there would leave the function around it"
                    .to_string(),
            };
        }
        let v = match self.expr(body, value, None) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let vt = self.types.get(&v).cloned();
        let ret = self.ret.clone();
        let (ok, fail, inner, carries) = match (&vt, &ret) {
            (Some(Type::Option(t)), Type::Option(_)) => {
                (BuiltinCase::Some, BuiltinCase::None, (**t).clone(), None)
            }
            (Some(Type::Result(t, e)), Type::Result(_, f)) if e == f => (
                BuiltinCase::Ok,
                BuiltinCase::Err,
                (**t).clone(),
                Some((**e).clone()),
            ),
            _ => {
                return Lowering::Blocked {
                    why: format!(
                        "`?` on a {vt:?} in a function returning {ret:?}; the checker refuses it"
                    ),
                    span,
                };
            }
        };
        let x = self.fresh();
        self.types.insert(x, inner.clone());
        let succeeded = MatchArm {
            cases: vec![Case::Builtin(ok)],
            bindings: vec![Some(x)],
            body: Region {
                instrs: Vec::new(),
                value: x,
            },
        };
        let outer = std::mem::take(&mut self.instrs);
        let e = carries.map(|t| {
            let e = self.fresh();
            self.types.insert(e, t);
            e
        });
        let failure = self.fresh();
        self.push(Instr::Variant {
            result: failure,
            case: fail,
            payload: e,
            ty: ret,
        });
        let left = self.fresh();
        self.push(Instr::Return {
            result: left,
            value: failure,
            ty: inner.clone(),
        });
        let failed = MatchArm {
            cases: vec![Case::Builtin(fail)],
            bindings: e.into_iter().map(Some).collect(),
            body: Region {
                instrs: std::mem::replace(&mut self.instrs, outer),
                value: left,
            },
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Match {
            result,
            scrutinee: v,
            arms: vec![succeeded, failed],
            ty: inner,
        }))
    }

    /// `let x = e` and `let _ = e`: the value bound for the rest of the block.
    fn bind(
        &mut self,
        body: &Body,
        pat: Option<crate::hir::PatternId>,
        init: Option<ExprId>,
        annotated: Option<Type>,
        span: Span,
    ) -> Lowering<()> {
        let Some(init) = init else {
            return Lowering::Unsupported {
                construct: "a binding with no value",
                span,
                reason: "`let x` without `= e` has nothing to bind".to_string(),
            };
        };
        // The written type, where there is one, is what the value must be:
        // the type a lambda takes its parameters from (ADR-0052).
        let v = match self.expr(body, init, annotated.as_ref()) {
            Lowering::Lowered(v) => v,
            other => return other.map(|_| unreachable!()),
        };
        if let Some(t) = &annotated
            && self.types.get(&v) != Some(t)
        {
            return Lowering::Blocked {
                why: format!(
                    "a binding written `{t:?}` is initialised with a {:?}",
                    self.types.get(&v)
                ),
                span,
            };
        }
        match pat.map(|p| body.pat(p)) {
            // `let mut x = e`: a variable, holding `e` until assigned
            // (ADR-0051).
            Some(Pattern::Bind {
                name,
                mutable: true,
            }) => {
                let Some(ty) = self.types.get(&v).cloned() else {
                    return Lowering::Blocked {
                        why: format!("`{name}`'s first value has no type"),
                        span,
                    };
                };
                let var = self.fresh();
                self.push(Instr::Local {
                    result: var,
                    init: v,
                    ty,
                });
                self.vars.insert(var);
                self.locals.insert(name.clone(), var);
                Lowering::Lowered(())
            }
            Some(Pattern::Bind { name, .. }) => {
                self.locals.insert(name.clone(), v);
                Lowering::Lowered(())
            }
            Some(Pattern::Wild) | None => Lowering::Lowered(()),
            Some(_) => Lowering::Unsupported {
                construct: "a destructuring binding",
                span,
                reason: "`let` binds a name or `_` here".to_string(),
            },
        }
    }

    /// `Some(x)`, `None`, `Ok(x)`, `Err(e)`, typed by what the context
    /// expects, or by the payload when nothing does.
    fn variant(
        &mut self,
        case: BuiltinCase,
        payload: Option<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let payload_ty = payload.and_then(|v| self.types.get(&v).cloned());
        let ty = match (case, expected, payload_ty) {
            (BuiltinCase::Some | BuiltinCase::None, Some(t @ Type::Option(_)), _) => t.clone(),
            (BuiltinCase::Ok | BuiltinCase::Err, Some(t @ Type::Result(..)), _) => t.clone(),
            (BuiltinCase::Some, None, Some(p)) => Type::Option(Box::new(p)),
            _ => {
                return Lowering::Unsupported {
                    construct: "a variant whose type nothing here fixes",
                    span,
                    reason: format!(
                        "`{case:?}` needs the type its context expects, and the context \
                         names none this backend can read"
                    ),
                };
            }
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Variant {
            result,
            case,
            payload,
            ty,
        }))
    }

    /// **An opaque type's representation** (ADR-0054), under its instance's
    /// arguments, where it is one a comparison reads: an `Int`, a `Float`, a
    /// `Bool` or a `String`. `None` for any other type.
    fn representation(&self, def: DefId, instance: &[Type], span: &Span) -> Option<Type> {
        let rep = self.cx.sigs.type_decl(def)?.representation.clone()?;
        let subst = self.instance_subst(def, instance);
        let ty = ty_resolved_with(self.cx.sigs, rep.resolved()?, span, &subst).lowered()?;
        matches!(ty, Type::Int | Type::Float | Type::Bool | Type::Str).then_some(ty)
    }

    /// A field of a record value, by the index its declaration gives it.
    fn field(&mut self, body: &Body, base: ExprId, name: &str, span: Span) -> Lowering<ValueId> {
        let of = match self.expr(body, base, None) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        let Some(Type::Nominal(def, instance)) = self.types.get(&of).cloned() else {
            return Lowering::Unsupported {
                construct: "a field of something that is not a record",
                span,
                reason: format!("`.{name}` is read from a value of no declared record type"),
            };
        };
        // A field's type under the instance's arguments (ADR-0062).
        let subst = self.instance_subst(def, &instance);
        let under = |r: &TypeResolution| match r.resolved() {
            Some(t) => ty_resolved_with(self.cx.sigs, t, &span, &subst),
            None => Lowering::Blocked {
                why: r.to_string(),
                span: span.clone(),
            },
        };
        // **An opaque type's `.value`** (ADR-0048): its representation. The
        // checker allows it only in the module that declares the type.
        if name == "value"
            && let Some(rep) = self
                .cx
                .sigs
                .type_decl(def)
                .and_then(|t| t.representation.clone())
        {
            let ty = match under(&rep) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::Retype {
                result,
                value: of,
                ty,
            }));
        }
        let fields = self.cx.sigs.type_decl(def).and_then(|t| t.record.as_ref());
        let field = fields.and_then(|fs| fs.iter().enumerate().find(|(_, (n, _))| n == name));
        // **A member function read as a member** (ADR-0048): `cart.line_count`
        // is `line_count(cart)` where `Cart` has no field of that name, by the
        // member table the checker typed it with (ADR-0122). Until 2026-10-02
        // the backend knew only fields, and the page's own count did not build.
        if field.is_none()
            && let Some(member) = self.cx.sigs.member_by_in(
                self.cx.sigs.module_name(self.unit),
                crate::signatures::Receiver::Nominal(def),
                name,
            )
        {
            let callee = member.definition;
            return self.inline(callee, vec![of], None, span);
        }
        if fields.is_none() {
            return Lowering::Unsupported {
                construct: "a field of something that is not a record",
                span,
                reason: format!("`.{name}` is read from a type with no record fields"),
            };
        }
        let Some((index, (_, declared))) = field else {
            return Lowering::Blocked {
                why: format!(
                    "the record has no field `{name}`; the checker should have refused it"
                ),
                span,
            };
        };
        let ty = match under(declared) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Project {
            result,
            of,
            field: index as u32,
            ty,
        }))
    }

    /// **A match over a variant**, as a structured [`Instr::Match`]: over
    /// `Option`, `Result`, or a declared sum type (ADR-0059). Each case is
    /// taken by exactly one arm: a case pattern takes its case, `A | B` each
    /// of its alternatives, and `_` or a name every case no arm before it
    /// takes. The encoder emits no fallthrough, so a case no arm takes is
    /// refused here rather than compiled to a trap.
    fn matched(
        &mut self,
        body: &Body,
        scrutinee: ExprId,
        arms: &[crate::hir::MatchArm],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let scrutinee = match self.expr(body, scrutinee, None) {
            Lowering::Lowered(v) => v,
            other => return other,
        };
        // A pattern nested in another, a literal, or a scrutinee that is not
        // a variant: a decision tree (ADR-0060).
        if !self.flat(body, arms, self.types.get(&scrutinee)) {
            return self.matched_tree(body, scrutinee, arms, expected, span);
        }
        // The scrutinee's cases, each with its payload's fields.
        let (cases, declared): (Vec<(Case, Vec<Type>)>, Option<DefId>) =
            match self.types.get(&scrutinee).cloned() {
                Some(Type::Option(t)) => (
                    vec![
                        (Case::Builtin(BuiltinCase::Some), vec![*t]),
                        (Case::Builtin(BuiltinCase::None), Vec::new()),
                    ],
                    None,
                ),
                Some(Type::Result(t, e)) => (
                    vec![
                        (Case::Builtin(BuiltinCase::Ok), vec![*t]),
                        (Case::Builtin(BuiltinCase::Err), vec![*e]),
                    ],
                    None,
                ),
                Some(Type::Nominal(def, instance))
                    if self
                        .cx
                        .sigs
                        .type_decl(def)
                        .is_some_and(|t| t.variants.is_some()) =>
                {
                    match self.declared_cases(def, &instance, &span) {
                        Lowering::Lowered(cases) => (cases, Some(def)),
                        other => return other.map(|_| unreachable!()),
                    }
                }
                other => {
                    return Lowering::Unsupported {
                        construct: "a match over something other than a variant",
                        span,
                        reason: match other {
                            Some(t) => format!("the scrutinee's type is {t:?}"),
                            None => "the scrutinee's type is not known here".to_string(),
                        },
                    };
                }
            };

        let mut lowered: Vec<MatchArm> = Vec::new();
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        let mut ty: Option<Type> = expected.cloned();
        for arm in arms {
            let takes = match self.arm_takes(body, arm.pat, &cases, declared, &span) {
                Lowering::Lowered(t) => t,
                other => return other.map(|_| unreachable!()),
            };
            let (indices, whole): (Vec<usize>, Option<String>) = match &takes {
                Takes::Case(i, _) => (vec![*i], None),
                Takes::Cases(is) => (is.clone(), None),
                Takes::Rest(name) => (
                    (0..cases.len()).filter(|i| !taken.contains(i)).collect(),
                    name.clone(),
                ),
            };
            if indices.is_empty() {
                return Lowering::Unsupported {
                    construct: "an arm no case reaches",
                    span,
                    reason: "every case it could take is taken by an arm before it, so it \
                             can never run"
                        .to_string(),
                };
            }
            if let Some(i) = indices.iter().find(|i| taken.contains(i)) {
                return Lowering::Unsupported {
                    construct: "a case matched twice",
                    span,
                    reason: format!("`{:?}` has two arms; the second can never run", cases[*i].0),
                };
            }
            taken.extend(indices.iter().copied());

            // The arm's body, in a region of its own, with what its pattern
            // binds: each named field of its one case, or the whole value.
            let outer = std::mem::take(&mut self.instrs);
            let mut bindings: Vec<Option<ValueId>> = Vec::new();
            let mut names: Vec<(String, ValueId)> = Vec::new();
            if let Takes::Case(i, fields) = &takes {
                let (case, types) = &cases[*i];
                for (k, t) in types.iter().enumerate() {
                    let name = fields.get(k).cloned().flatten();
                    // A builtin case's payload is bound whether or not a
                    // name takes it: the numbering every artifact was built
                    // with before ADR-0059.
                    if name.is_none() && matches!(case, Case::Declared(_)) {
                        bindings.push(None);
                        continue;
                    }
                    let v = self.fresh();
                    self.types.insert(v, t.clone());
                    bindings.push(Some(v));
                    if let Some(n) = name {
                        names.push((n, v));
                    }
                }
            }
            if let Some(n) = whole {
                names.push((n, scrutinee));
            }
            let shadowed: Vec<(String, Option<ValueId>)> = names
                .iter()
                .map(|(n, v)| (n.clone(), self.locals.insert(n.clone(), *v)))
                .collect();
            let value = self.expr(body, arm.body, ty.as_ref());
            for (name, before) in shadowed.into_iter().rev() {
                match before {
                    Some(v) => self.locals.insert(name, v),
                    None => self.locals.remove(&name),
                };
            }
            let instrs = std::mem::replace(&mut self.instrs, outer);
            let value = match value {
                Lowering::Lowered(v) => v,
                other => return other,
            };
            let region = Region { instrs, value };
            let value_ty = self.types.get(&value).cloned();
            // An arm that returns has no value of its own (ADR-0051).
            match (&ty, value_ty) {
                _ if diverges(&region) => {}
                (None, Some(t)) => ty = Some(t),
                (Some(t), Some(v)) if *t != v => {
                    return Lowering::Blocked {
                        why: format!("the arms produce {t:?} and {v:?}"),
                        span,
                    };
                }
                _ => {}
            }
            lowered.push(MatchArm {
                cases: indices.iter().map(|i| cases[*i].0).collect(),
                bindings,
                body: region,
            });
        }
        if ty.is_none() && lowered.iter().all(|a| diverges(&a.body)) {
            ty = Some(Type::Unit);
        }
        if let Some(t) = &ty {
            for arm in &mut lowered {
                self.retype_divergent(&mut arm.body, t);
            }
        }
        if taken.len() != cases.len() {
            return Lowering::Unsupported {
                construct: "a match that does not cover every case",
                span,
                reason: format!(
                    "{} of {} cases have arms, and a missing case would have no code",
                    taken.len(),
                    cases.len()
                ),
            };
        }
        let Some(ty) = ty else {
            return Lowering::Blocked {
                why: "a match whose arms produce no typed value".to_string(),
                span,
            };
        };
        let result = self.fresh();
        Lowering::Lowered(self.push(Instr::Match {
            result,
            scrutinee,
            arms: lowered,
            ty,
        }))
    }

    /// A declared sum type's cases, each with its payload's fields resolved
    /// (ADR-0059), under the instance's arguments (ADR-0062): `Maybe<Int>`'s
    /// `Just` holds an `Int`. An instance missing an argument its declaration
    /// takes is refused by name.
    fn declared_cases(&self, def: DefId, args: &[Type], span: &Span) -> Lowering<Cases> {
        let params =
            crate::resolve::declaration(self.cx.hirs, def).map_or(0, |d| d.type_params.len());
        if args.len() != params {
            return Lowering::Unsupported {
                construct: "a type parameter no use instantiates",
                span: span.clone(),
                reason: "a generic sum type's arguments are fixed by its fields or where it is \
                         used, and nothing here fixes one"
                    .to_string(),
            };
        }
        let Some(declared) = self.cx.sigs.type_decl(def).and_then(|t| t.variants.clone()) else {
            return Lowering::Blocked {
                why: "a sum type with no cases".to_string(),
                span: span.clone(),
            };
        };
        let subst = self.instance_subst(def, args);
        let mut out = Vec::new();
        for (index, (_, fields)) in declared.iter().enumerate() {
            let mut types = Vec::new();
            for f in fields {
                let Some(t) = f.resolved() else {
                    return Lowering::Blocked {
                        why: f.to_string(),
                        span: span.clone(),
                    };
                };
                match ty_resolved_with(self.cx.sigs, t, span, &subst) {
                    Lowering::Lowered(t) => types.push(t),
                    other => return other.map(|_| unreachable!()),
                }
            }
            out.push((Case::Declared(index as u32), types));
        }
        Lowering::Lowered(out)
    }

    /// **Which of `cases` an arm's pattern takes** (ADR-0059), and what it
    /// binds. A nested pattern is refused, and a literal one.
    fn arm_takes(
        &self,
        body: &Body,
        pat: crate::hir::PatternId,
        cases: &[(Case, Vec<Type>)],
        declared: Option<DefId>,
        span: &Span,
    ) -> Lowering<Takes> {
        let unsupported = |construct: &'static str, reason: String| Lowering::Unsupported {
            construct,
            span: span.clone(),
            reason,
        };
        match body.pat(pat) {
            Pattern::Wild => Lowering::Lowered(Takes::Rest(None)),
            // A binding, always: `None` and `Empty`, a case without a payload
            // written alone, are constructor patterns by their capital
            // (ADR-0195, ruling 2).
            Pattern::Bind { name, .. } => Lowering::Lowered(Takes::Rest(Some(name.clone()))),
            Pattern::Ctor { path, args } => {
                let Some(i) = self.case_index(path, cases, declared) else {
                    return unsupported(
                        "a pattern this backend does not lower",
                        format!("`{path}(..)` is not a case of the scrutinee"),
                    );
                };
                if args.len() != cases[i].1.len() {
                    return Lowering::Blocked {
                        why: format!(
                            "`{path}` binds {} fields of {}; the checker refuses it (PW0603)",
                            args.len(),
                            cases[i].1.len()
                        ),
                        span: span.clone(),
                    };
                }
                let mut names = Vec::new();
                for a in args {
                    match body.pat(*a) {
                        Pattern::Bind { name, .. } => names.push(Some(name.clone())),
                        Pattern::Wild => names.push(None),
                        _ => {
                            return unsupported(
                                "a nested pattern",
                                format!("`{path}(..)` binds a name or `_` here"),
                            );
                        }
                    }
                }
                Lowering::Lowered(Takes::Case(i, names))
            }
            // `A | B`: each alternative's cases, none of them binding.
            Pattern::Or(alternatives) => {
                let mut taken = Vec::new();
                for p in alternatives {
                    match self.arm_takes(body, *p, cases, declared, span) {
                        Lowering::Lowered(Takes::Case(i, names))
                            if names.iter().all(Option::is_none) =>
                        {
                            taken.push(i)
                        }
                        Lowering::Lowered(Takes::Cases(is)) => taken.extend(is),
                        Lowering::Lowered(Takes::Rest(None)) => {
                            return Lowering::Lowered(Takes::Rest(None));
                        }
                        Lowering::Lowered(_) => {
                            return unsupported(
                                "an or-pattern that binds a name",
                                "each alternative would bind it from a different case".to_string(),
                            );
                        }
                        other => return other,
                    }
                }
                taken.sort_unstable();
                taken.dedup();
                Lowering::Lowered(Takes::Cases(taken))
            }
            Pattern::Literal(_) => unsupported(
                "a literal pattern",
                "an arm takes a case of the scrutinee's type".to_string(),
            ),
            Pattern::Error => Lowering::Blocked {
                why: "a pattern that did not parse".to_string(),
                span: span.clone(),
            },
        }
    }

    /// **Is this match one level deep**: a variant, each arm a case whose
    /// fields are names or `_`, `_`, a name, or `A | B` of those? Such a match
    /// is one `Instr::Match` whose arms are the program's, each body lowered
    /// once. A field written as a case's name, `Some(Empty)`, is a pattern
    /// nested in another: until 2026-09-26 it was read as a binding of that
    /// name, which took every payload (ADR-0060).
    fn flat(&self, body: &Body, arms: &[crate::hir::MatchArm], ty: Option<&Type>) -> bool {
        let variant = match ty {
            Some(Type::Option(_) | Type::Result(..)) => true,
            Some(Type::Nominal(def, _)) => self
                .cx
                .sigs
                .type_decl(*def)
                .is_some_and(|t| t.variants.is_some()),
            _ => false,
        };
        fn one_level(body: &Body, p: crate::hir::PatternId) -> bool {
            match body.pat(p) {
                Pattern::Wild | Pattern::Bind { .. } => true,
                Pattern::Ctor { args, .. } => args
                    .iter()
                    .all(|a| matches!(body.pat(*a), Pattern::Wild | Pattern::Bind { .. })),
                Pattern::Or(alternatives) => alternatives.iter().all(|a| one_level(body, *a)),
                Pattern::Literal(_) | Pattern::Error => false,
            }
        }
        variant && arms.iter().all(|a| one_level(body, a.pat))
    }

    /// **A match compiled to a decision tree** (ADR-0060): nested patterns,
    /// literals, and a `Bool`, an `Int` or a `String` taken apart. Each node
    /// tests one value: a variant's discriminant, as a structured match; a
    /// `Bool`, as an `if`; an `Int` or a `String`, as an `if` per literal the
    /// arms name. A leaf is the first arm whose pattern every test so far
    /// agrees with, its body lowered where its names are bound, so an arm
    /// reached by several paths has several copies. A path no arm takes is
    /// refused, never compiled to a trap.
    fn matched_tree(
        &mut self,
        body: &Body,
        scrutinee: ValueId,
        arms: &[crate::hir::MatchArm],
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let Some(st) = self.types.get(&scrutinee).cloned() else {
            return Lowering::Blocked {
                why: "a match over a value with no type".to_string(),
                span,
            };
        };
        let mut rows = Vec::new();
        for (i, arm) in arms.iter().enumerate() {
            match self.tree_pat(body, arm.pat, &st, &span) {
                Lowering::Lowered(p) => rows.push(TreeRow {
                    pats: vec![p],
                    arm: i,
                    binds: Vec::new(),
                }),
                other => return other.map(|_| unreachable!()),
            }
        }
        let mut leaves = 0usize;
        let mut tree = match self.tree_build(rows, vec![(scrutinee, st)], &mut leaves, &span) {
            Lowering::Lowered(t) => t,
            other => return other.map(|_| unreachable!()),
        };
        let mut ty: Option<Type> = expected.cloned();
        match self.tree_leaves(body, arms, &mut tree, &mut ty, &span) {
            Lowering::Lowered(()) => {}
            other => return other.map(|_| unreachable!()),
        }
        let ty = ty.unwrap_or(Type::Unit);
        let region = self.tree_emit(tree, &ty);
        self.instrs.extend(region.instrs);
        Lowering::Lowered(region.value)
    }

    /// A pattern as the tree reads it, against the type of the value it is
    /// tested on.
    fn tree_pat(
        &self,
        body: &Body,
        id: crate::hir::PatternId,
        ty: &Type,
        span: &Span,
    ) -> Lowering<TreePat> {
        let refused = |construct: &'static str, reason: String| Lowering::Unsupported {
            construct,
            span: span.clone(),
            reason,
        };
        match body.pat(id) {
            Pattern::Wild => Lowering::Lowered(TreePat::Any(None)),
            // A binding, always, by its capital (ADR-0195, ruling 2).
            Pattern::Bind { name, .. } => Lowering::Lowered(TreePat::Any(Some(name.clone()))),
            // `true` and `false`, `Bool`'s two cases.
            Pattern::Ctor { path, args }
                if *ty == Type::Bool
                    && args.is_empty()
                    && matches!(path.as_str(), "true" | "false") =>
            {
                Lowering::Lowered(TreePat::Bool(path == "true"))
            }
            Pattern::Ctor { path, args } => {
                let Some((cases, declared)) = self.variant_cases(ty, span) else {
                    return refused(
                        "a pattern this backend does not lower",
                        format!("`{path}(..)` against a {ty:?}"),
                    );
                };
                let Some(i) = self.case_index(path, &cases, declared) else {
                    return refused(
                        "a pattern this backend does not lower",
                        format!("`{path}(..)` is not a case of the scrutinee"),
                    );
                };
                let fields = cases[i].1.clone();
                if args.len() != fields.len() {
                    return Lowering::Blocked {
                        why: format!("`{path}` binds {} fields of {}", args.len(), fields.len()),
                        span: span.clone(),
                    };
                }
                let mut subs = Vec::new();
                for (a, t) in args.iter().zip(&fields) {
                    match self.tree_pat(body, *a, t, span) {
                        Lowering::Lowered(p) => subs.push(p),
                        other => return other,
                    }
                }
                Lowering::Lowered(TreePat::Case(cases[i].0, subs))
            }
            Pattern::Literal(l) => {
                let value = match (ty, l) {
                    (Type::Int, Literal::Int(n)) => {
                        n.replace('_', "").parse::<i64>().ok().map(Const::Int)
                    }
                    (Type::Str, Literal::Str(_)) => l.string_value().map(Const::Str),
                    (Type::Float, Literal::Float(_)) => {
                        return refused(
                            "a `Float` literal pattern",
                            "equality on floats decides no case".to_string(),
                        );
                    }
                    _ => None,
                };
                match value {
                    Some(v) => Lowering::Lowered(TreePat::Lit(v)),
                    None => Lowering::Blocked {
                        why: format!("a literal pattern against a {ty:?}; the checker refuses it"),
                        span: span.clone(),
                    },
                }
            }
            Pattern::Or(alternatives) => {
                let mut out = Vec::new();
                for a in alternatives {
                    match self.tree_pat(body, *a, ty, span) {
                        Lowering::Lowered(p) if p.binds() => {
                            return refused(
                                "an or-pattern that binds a name",
                                "each alternative would bind it from a different case".to_string(),
                            );
                        }
                        Lowering::Lowered(p) => out.push(p),
                        other => return other,
                    }
                }
                Lowering::Lowered(TreePat::Or(out))
            }
            Pattern::Error => Lowering::Blocked {
                why: "a pattern that did not parse".to_string(),
                span: span.clone(),
            },
        }
    }

    /// A variant type's cases, each with its payload's fields, and the
    /// declaration of a declared one. `None` for any other type.
    fn variant_cases(&self, ty: &Type, span: &Span) -> Option<(Cases, Option<DefId>)> {
        match ty {
            Type::Option(t) => Some((
                vec![
                    (Case::Builtin(BuiltinCase::Some), vec![(**t).clone()]),
                    (Case::Builtin(BuiltinCase::None), Vec::new()),
                ],
                None,
            )),
            Type::Result(t, e) => Some((
                vec![
                    (Case::Builtin(BuiltinCase::Ok), vec![(**t).clone()]),
                    (Case::Builtin(BuiltinCase::Err), vec![(**e).clone()]),
                ],
                None,
            )),
            Type::Nominal(def, instance) => match self.declared_cases(*def, instance, span) {
                Lowering::Lowered(cases) if !cases.is_empty() => Some((cases, Some(*def))),
                _ => None,
            },
            _ => None,
        }
    }

    /// **The tree for `rows` over the values `occs`**, one pattern per value
    /// in each row. The first row whose patterns are all `_` or names is a
    /// leaf; otherwise the first value the first row tests is tested, and
    /// the rows are divided by what it is.
    fn tree_build(
        &mut self,
        rows: Vec<TreeRow>,
        occs: Vec<(ValueId, Type)>,
        leaves: &mut usize,
        span: &Span,
    ) -> Lowering<Tree> {
        // `A | B` in any row, one row per alternative, in order.
        let mut rows = expand_or(rows);
        let Some(first) = rows.first() else {
            return Lowering::Unsupported {
                construct: "a match that does not cover every case",
                span: span.clone(),
                reason: "a value reaches no arm, and a missing case would have no code".to_string(),
            };
        };
        let Some(col) = first
            .pats
            .iter()
            .position(|p| !matches!(p, TreePat::Any(_)))
        else {
            *leaves += 1;
            if *leaves > 1024 {
                return Lowering::Unsupported {
                    construct: "a match whose decision tree is too large",
                    span: span.clone(),
                    reason: "its arms' bodies would be copied more than 1,024 times".to_string(),
                };
            }
            let row = rows.swap_remove(0);
            let mut binds = row.binds;
            for (p, (v, _)) in row.pats.iter().zip(&occs) {
                if let TreePat::Any(Some(name)) = p {
                    binds.push((name.clone(), *v));
                }
            }
            return Lowering::Lowered(Tree::Leaf {
                arm: row.arm,
                binds,
                region: None,
            });
        };
        let (occ, ty) = occs[col].clone();
        // Each row's pattern for `col`, out of the row: a name there binds
        // the value tested.
        let split: Vec<(TreePat, TreeRow)> = rows
            .into_iter()
            .map(|mut r| {
                let p = r.pats.remove(col);
                if let TreePat::Any(Some(name)) = &p {
                    r.binds.push((name.clone(), occ));
                }
                (p, r)
            })
            .collect();
        let mut rest = occs.clone();
        rest.remove(col);

        if let Some((cases, _)) = self.variant_cases(&ty, span) {
            let mut arms = Vec::new();
            let mut others: Vec<Case> = Vec::new();
            for (case, fields) in &cases {
                let named = split
                    .iter()
                    .any(|(p, _)| matches!(p, TreePat::Case(c, _) if c == case));
                if !named {
                    others.push(*case);
                    continue;
                }
                // The case's fields, each a value of its own, tested next.
                let values: Vec<ValueId> = fields
                    .iter()
                    .map(|t| {
                        let v = self.fresh();
                        self.types.insert(v, t.clone());
                        v
                    })
                    .collect();
                let sub_rows: Vec<TreeRow> = split
                    .iter()
                    .filter_map(|(p, r)| {
                        let subs = match p {
                            TreePat::Case(c, subs) if c == case => subs.clone(),
                            TreePat::Any(_) => vec![TreePat::Any(None); fields.len()],
                            _ => return None,
                        };
                        let mut pats = subs;
                        pats.extend(r.pats.iter().cloned());
                        Some(TreeRow {
                            pats,
                            arm: r.arm,
                            binds: r.binds.clone(),
                        })
                    })
                    .collect();
                let mut sub_occs: Vec<(ValueId, Type)> =
                    values.iter().copied().zip(fields.iter().cloned()).collect();
                sub_occs.extend(rest.iter().cloned());
                let sub = match self.tree_build(sub_rows, sub_occs, leaves, span) {
                    Lowering::Lowered(t) => t,
                    other => return other,
                };
                arms.push((vec![*case], values.into_iter().map(Some).collect(), sub));
            }
            if !others.is_empty() {
                let sub_rows: Vec<TreeRow> = split
                    .iter()
                    .filter(|(p, _)| matches!(p, TreePat::Any(_)))
                    .map(|(_, r)| r.clone())
                    .collect();
                let sub = match self.tree_build(sub_rows, rest.clone(), leaves, span) {
                    Lowering::Lowered(t) => t,
                    other => return other,
                };
                arms.push((others, Vec::new(), sub));
            }
            return Lowering::Lowered(Tree::Switch { occ, arms });
        }

        if ty == Type::Bool {
            let mut branch = |value: bool, me: &mut Self| {
                let sub_rows: Vec<TreeRow> = split
                    .iter()
                    .filter(|(p, _)| matches!(p, TreePat::Any(_)) || *p == TreePat::Bool(value))
                    .map(|(_, r)| r.clone())
                    .collect();
                me.tree_build(sub_rows, rest.clone(), leaves, span)
            };
            let then = match branch(true, self) {
                Lowering::Lowered(t) => t,
                other => return other,
            };
            let els = match branch(false, self) {
                Lowering::Lowered(t) => t,
                other => return other,
            };
            return Lowering::Lowered(Tree::Test {
                occ,
                then: Box::new(then),
                els: Box::new(els),
            });
        }

        if matches!(ty, Type::Int | Type::Str) {
            // Each literal the column names, in the order the arms name it,
            // then what no literal names.
            let mut values: Vec<Const> = Vec::new();
            for (p, _) in &split {
                if let TreePat::Lit(v) = p
                    && !values.contains(v)
                {
                    values.push(v.clone());
                }
            }
            let mut cases = Vec::new();
            for v in values {
                let sub_rows: Vec<TreeRow> = split
                    .iter()
                    .filter(|(p, _)| matches!(p, TreePat::Any(_)) || *p == TreePat::Lit(v.clone()))
                    .map(|(_, r)| r.clone())
                    .collect();
                match self.tree_build(sub_rows, rest.clone(), leaves, span) {
                    Lowering::Lowered(t) => cases.push((v, t)),
                    other => return other,
                }
            }
            let sub_rows: Vec<TreeRow> = split
                .iter()
                .filter(|(p, _)| matches!(p, TreePat::Any(_)))
                .map(|(_, r)| r.clone())
                .collect();
            let otherwise = match self.tree_build(sub_rows, rest, leaves, span) {
                Lowering::Lowered(t) => t,
                other => return other,
            };
            return Lowering::Lowered(Tree::Literals {
                occ,
                ty,
                cases,
                otherwise: Box::new(otherwise),
            });
        }
        Lowering::Unsupported {
            construct: "a pattern this backend does not lower",
            span: span.clone(),
            reason: format!("a pattern that tests a {ty:?}"),
        }
    }

    /// Lower each leaf's arm body, in the order the tree reaches them, with
    /// the names its path binds. The first body that does not leave early
    /// fixes the match's type.
    fn tree_leaves(
        &mut self,
        body: &Body,
        arms: &[crate::hir::MatchArm],
        tree: &mut Tree,
        ty: &mut Option<Type>,
        span: &Span,
    ) -> Lowering<()> {
        match tree {
            Tree::Leaf { arm, binds, region } => {
                let outer = std::mem::take(&mut self.instrs);
                let shadowed: Vec<(String, Option<ValueId>)> = binds
                    .iter()
                    .map(|(n, v)| (n.clone(), self.locals.insert(n.clone(), *v)))
                    .collect();
                let value = self.expr(body, arms[*arm].body, ty.as_ref());
                for (name, before) in shadowed.into_iter().rev() {
                    match before {
                        Some(v) => self.locals.insert(name, v),
                        None => self.locals.remove(&name),
                    };
                }
                let instrs = std::mem::replace(&mut self.instrs, outer);
                let value = match value {
                    Lowering::Lowered(v) => v,
                    other => return other.map(|_| ()),
                };
                let r = Region { instrs, value };
                let value_ty = self.types.get(&value).cloned();
                match (&*ty, value_ty) {
                    _ if diverges(&r) => {}
                    (None, Some(t)) => *ty = Some(t),
                    (Some(t), Some(v)) if *t != v => {
                        return Lowering::Blocked {
                            why: format!("the arms produce {t:?} and {v:?}"),
                            span: span.clone(),
                        };
                    }
                    _ => {}
                }
                *region = Some(r);
                Lowering::Lowered(())
            }
            Tree::Switch { arms: subs, .. } => {
                for (_, _, t) in subs {
                    match self.tree_leaves(body, arms, t, ty, span) {
                        Lowering::Lowered(()) => {}
                        other => return other,
                    }
                }
                Lowering::Lowered(())
            }
            Tree::Test { then, els, .. } => {
                match self.tree_leaves(body, arms, then, ty, span) {
                    Lowering::Lowered(()) => {}
                    other => return other,
                }
                self.tree_leaves(body, arms, els, ty, span)
            }
            Tree::Literals {
                cases, otherwise, ..
            } => {
                for (_, t) in cases {
                    match self.tree_leaves(body, arms, t, ty, span) {
                        Lowering::Lowered(()) => {}
                        other => return other,
                    }
                }
                self.tree_leaves(body, arms, otherwise, ty, span)
            }
        }
    }

    /// The tree as instructions, of the match's type `ty`.
    fn tree_emit(&mut self, tree: Tree, ty: &Type) -> Region {
        match tree {
            Tree::Leaf { region, .. } => {
                let mut r = region.expect("every leaf is lowered first");
                self.retype_divergent(&mut r, ty);
                r
            }
            Tree::Switch { occ, arms } => {
                let arms: Vec<MatchArm> = arms
                    .into_iter()
                    .map(|(cases, bindings, t)| MatchArm {
                        cases,
                        bindings,
                        body: self.tree_emit(t, ty),
                    })
                    .collect();
                let result = self.fresh();
                self.types.insert(result, ty.clone());
                Region {
                    instrs: vec![Instr::Match {
                        result,
                        scrutinee: occ,
                        arms,
                        ty: ty.clone(),
                    }],
                    value: result,
                }
            }
            Tree::Test { occ, then, els } => {
                let then = self.tree_emit(*then, ty);
                let els = self.tree_emit(*els, ty);
                let result = self.fresh();
                self.types.insert(result, ty.clone());
                Region {
                    instrs: vec![Instr::If {
                        result,
                        cond: occ,
                        then,
                        els,
                        ty: ty.clone(),
                    }],
                    value: result,
                }
            }
            Tree::Literals {
                occ,
                ty: of,
                cases,
                otherwise,
            } => {
                // `if v == a { .. } else if v == b { .. } else { .. }`, built
                // from the last test outward.
                let mut region = self.tree_emit(*otherwise, ty);
                for (value, t) in cases.into_iter().rev() {
                    let then = self.tree_emit(t, ty);
                    let (literal, equal, result) = (self.fresh(), self.fresh(), self.fresh());
                    self.types.insert(literal, of.clone());
                    self.types.insert(equal, Type::Bool);
                    self.types.insert(result, ty.clone());
                    region = Region {
                        instrs: vec![
                            Instr::Const {
                                result: literal,
                                value,
                                ty: of.clone(),
                            },
                            Instr::Binary {
                                result: equal,
                                op: BinaryOp::Eq,
                                lhs: occ,
                                rhs: literal,
                                ty: Type::Bool,
                            },
                            Instr::If {
                                result,
                                cond: equal,
                                then,
                                els: region,
                                ty: ty.clone(),
                            },
                        ],
                        value: result,
                    };
                }
                region
            }
        }
    }

    /// The position in `cases` of the case a pattern names: the language's
    /// own by `builtin`'s rule, a declared type's through its type
    /// (`Shape.Circle`) or alone (`Circle`), as the checker reads it.
    fn case_index(
        &self,
        path: &str,
        cases: &[(Case, Vec<Type>)],
        declared: Option<DefId>,
    ) -> Option<usize> {
        let case = match declared {
            None => Case::Builtin(self.builtin(path)?),
            Some(def) if path.contains('.') => {
                match crate::values::case_named(self.cx.sigs, self.cx.ws, self.unit, path)? {
                    crate::values::CaseNamed::Case(d, index) if d == def => {
                        Case::Declared(index as u32)
                    }
                    _ => return None,
                }
            }
            Some(def) => {
                let declared = self.cx.sigs.type_decl(def)?.variants.as_ref()?;
                Case::Declared(declared.iter().position(|(n, _)| n == path)? as u32)
            }
        };
        cases.iter().position(|(c, _)| *c == case)
    }

    /// A call — the one place the host boundary is decided.
    ///
    /// **From the contract, not from the callee's name.** If the enclosing
    /// declaration requires a capability and this call is the thing that needs
    /// it, it is a `HostCall`. The alternative — matching `Carts.add` against a
    /// list — is exactly the by-spelling resolution E2C deleted.
    fn call(
        &mut self,
        body: &Body,
        callee: ExprId,
        args: &[crate::hir::Arg],
        piped: Option<ValueId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Lowering<ValueId> {
        let path = crate::infer::path_of(body, callee);

        // **A call through a function value** (ADR-0052): a binding that
        // holds one, called.
        if let Expr::Name(n) = body.expr(callee)
            && let Some(f) = self.locals.get(n).copied()
            && let Some(Type::Function(ps, r)) = self.types.get(&f).cloned()
        {
            let f = if self.vars.contains(&f) {
                self.read(f)
            } else {
                f
            };
            return self.call_value(body, f, &ps, *r, args, piped, span);
        }

        if !self.declaration_path(body, callee) {
            return Lowering::Unsupported {
                construct: "a local member used as a callable",
                span,
                reason: "this callable value needs receiver lowering".to_string(),
            };
        }

        // **Through `Signatures`, keyed by the path a call site writes.**
        // `Carts.add` is a qualified path across a module boundary, and
        // `Workspace::resolve_in` answers about a single name — asking it for a
        // dotted path returned nothing and blocked every real command. `by_path`
        // is the answer E2C built for exactly this question, and `by_def` is
        // what a caller holding a `DefId` uses instead.
        // A BARE name resolves through the workspace, which knows this unit's
        // imports and the platform prelude. A DOTTED path is already qualified,
        // and `by_path` is keyed by exactly the form a call site writes.
        //
        // Neither is a spelling match: one goes through resolution and the
        // other through a map E2C builds from resolved declarations. Reading
        // the last segment would be the third thing, and it is the one
        // `last_segment.rs` forbids.
        //
        // A DOTTED path goes through `resolve_path`, which checks the module is
        // visible and the member exists. It was not asked before 2026-08-20,
        // because the old classification keyed on the effect row and did not
        // need the callee's identity — so `Menus.for_store` had a signature and
        // no `DefId`, and nothing noticed until the identity became the thing
        // that decides.
        let resolution = match path.contains('.') {
            true => self.cx.ws.resolve_path(self.unit, &path),
            false => self.cx.ws.resolve_in(self.unit, Namespace::Term, &path),
        };
        let resolved = match resolution {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => Some(d),
            _ => None,
        };
        // **An opaque type built from its representation** (ADR-0054):
        // `PositiveInt(1)`. The type is in the Type namespace, where a call's
        // callee is not looked for.
        if resolved.is_none() {
            let as_type = match path.contains('.') {
                true => self
                    .cx
                    .ws
                    .resolve_path_in(self.unit, Namespace::Type, &path),
                false => self.cx.ws.resolve_in(self.unit, Namespace::Type, &path),
            };
            if let Resolution::Local(def) | Resolution::Imported { def, .. } = as_type
                && let Some(rep) = self
                    .cx
                    .sigs
                    .type_decl(def)
                    .and_then(|t| t.representation.clone())
            {
                return self.opaque(body, def, &rep, args, piped, expected, span);
            }
            // **A sum type's case** (ADR-0059): `Shape.Circle(3)`, through
            // its type, by the rule the checker typed it with.
            if let Some(crate::values::CaseNamed::Case(def, index)) =
                self.expression_case(body, callee)
            {
                return self.case(body, def, index, args, piped, expected, span);
            }
            // `Circle(3)` alone: the case of the one type this unit sees that
            // has it, or of the one the call's expected type is, by the rule a
            // case without a payload has (ADR-0198, ADR-0201, ADR-0195's
            // ruling 5).
            if !path.contains('.')
                && let Some((def, index)) = self.case_written_alone(body, callee)
            {
                return self.case(body, def, index, args, piped, expected, span);
            }
        }
        // **An operation the compiler supplies** (ADR-0040): read from the
        // declaration's `intrinsic` clause, before its arguments are lowered,
        // because a function argument is compiled where it is called.
        if let Some(decl) = resolved.and_then(|d| crate::resolve::declaration(self.cx.hirs, d))
            && let Some(op) = crate::backend::intrinsic_binding(decl)
        {
            return match op {
                Ok(op) => self.intrinsic(body, op, args, piped, expected, span),
                Err(name) => Lowering::Unsupported {
                    construct: "an intrinsic this backend does not know",
                    span,
                    reason: format!("`{path}` is declared `intrinsic \"{name}\"`"),
                },
            };
        }
        let Some(sig) = resolved
            .and_then(|d| self.cx.sigs.by_def(d))
            .or_else(|| self.cx.sigs.by_path(&path))
            .cloned()
        else {
            return Lowering::Blocked {
                why: format!("`{path}` names no declaration this program contains"),
                span,
            };
        };

        // The arguments, each expecting its parameter's declared type. A piped
        // value is the first. A named argument is given to the parameter of
        // its name (ADR-0081): each is lowered in the order written, and
        // passed in the order declared. Until 2026-09-26 they were passed as
        // written, so `g(b = 1, a = 10)` computed `g(1, 10)`.
        let offset = usize::from(piped.is_some());
        let written: Vec<Option<&str>> = args.iter().map(|a| a.name.as_deref()).collect();
        let Ok(order) = crate::signatures::arrange(&sig.names, offset, &written) else {
            return Lowering::Blocked {
                why: format!(
                    "a named argument in this call to `{path}` names no parameter, or one twice"
                ),
                span,
            };
        };
        let mut slots: Vec<Option<ValueId>> = vec![None; offset + args.len()];
        if let Some(p) = piped {
            slots[0] = Some(p);
        }
        for (a, i) in args.iter().zip(order) {
            let expected = match sig.params.get(i).and_then(Option::as_ref) {
                Some(p) => match ty_resolution(self.cx.sigs, p, &span) {
                    Lowering::Lowered(t) => Some(t),
                    _ => None,
                },
                None => None,
            };
            // A lambda passed to a declaration is a function value
            // (ADR-0052), typed by the parameter it is passed as.
            match self.expr(body, a.value, expected.as_ref()) {
                Lowering::Lowered(v) => match slots.get_mut(i) {
                    Some(slot) => *slot = Some(v),
                    None => {
                        return Lowering::Blocked {
                            why: format!("`{path}` is given more arguments than it declares"),
                            span,
                        };
                    }
                },
                other => return other,
            }
        }
        let Some(lowered) = slots.into_iter().collect::<Option<Vec<ValueId>>>() else {
            return Lowering::Blocked {
                why: format!("a parameter of `{path}` is given no argument"),
                span,
            };
        };

        // **A command a handler calls** (ADR-0058): through its context, in
        // the browser, by the contract's component id. Its arguments are sent
        // as JSON, so each is one a browser can send (ADR-0033 §4).
        // A query answers on the server, and a handler runs in the browser.
        let in_handler = self.handler || self.internal.borrow().in_handler;
        if in_handler
            && let Some(d) = resolved
            && let Some(decl) = crate::resolve::declaration(self.cx.hirs, d)
            && decl.kind == DeclKind::Query
        {
            return Lowering::Unsupported {
                construct: "a query called from a handler",
                span,
                reason: format!(
                    "`{path}` answers on the server; a handler reaches the server through a \
                     command"
                ),
            };
        }
        if in_handler
            && let Some(d) = resolved
            && let Some(decl) = crate::resolve::declaration(self.cx.hirs, d)
            && decl.kind == DeclKind::Command
        {
            if self.in_lambda > 0 || !self.handler {
                return Lowering::Unsupported {
                    construct: "a command called inside a function",
                    span,
                    reason: format!(
                        "`{path}` would be awaited inside a function value or a function \
                         compiled beside the handler; call it in the handler's own body, a \
                         `for` loop's included"
                    ),
                };
            }
            for (i, v) in lowered.iter().enumerate() {
                let t = self.types.get(v).cloned().unwrap_or(Type::Unit);
                if !sendable(self.cx, &t) {
                    let named = match &t {
                        Type::Nominal(def, _) => crate::resolve::declaration(self.cx.hirs, *def)
                            .map(|d| d.name.clone())
                            .unwrap_or_else(|| format!("{t:?}")),
                        other => format!("{other:?}"),
                    };
                    return Lowering::Unsupported {
                        construct: "a command parameter a browser cannot send",
                        span,
                        reason: format!(
                            "argument {} of `{path}` is `{named}`; a handler sends primitives, \
                             opaque types over them, and lists, records and cases of what it \
                             sends, each field named in snake case",
                            i + 1
                        ),
                    };
                }
            }
            let command =
                crate::contract::component_id(self.cx.hirs[d.unit], crate::hir::DeclId(d.decl));
            // What the handler is answered (ADR-0157): whether the command
            // committed, and its declared error if not, `Result<Unit, E>`. It
            // was `Unit` until 2026-10-03, so a handler could not tell a
            // refusal from a commit. Never the value, which reaches the page
            // from the resource.
            let ty = match self
                .cx
                .sigs
                .by_def(d)
                .and_then(|s| s.result())
                .map(crate::resolved::ResolvedType::answered)
            {
                Some(t) => match ty_resolved(self.cx.sigs, &t, &span) {
                    Lowering::Lowered(t) => t,
                    other => return other.map(|_| unreachable!()),
                },
                None => Type::Unit,
            };
            // How the handler sends it again when no answer came (ADR-0173),
            // as its `retry` clause says. `retry forever` is refused before
            // anything is compiled (PW0313), and `retry none` sends it once.
            let resend = decl
                .policy("retry")
                .and_then(|p| crate::manifest::parse_retry(&p.value))
                .and_then(|r| match r {
                    crate::manifest::Retry::Bounded { max, jitter, .. } => {
                        Some(super::ir::Resend { max, jitter })
                    }
                    crate::manifest::Retry::None | crate::manifest::Retry::Forever => None,
                });
            let result = self.fresh();
            return Lowering::Lowered(self.push(Instr::Command {
                result,
                command,
                args: lowered,
                ty,
                resend,
            }));
        }

        let def = resolved;

        // **Where does this callee's implementation come from?**
        //
        // Architect ruling, 2026-08-20, deleting the rule that used to be here:
        //
        // > "The callee's declared effect row contains a capability required by
        // > this component, therefore it is a HostCall." Delete that
        // > classification. An effectful function can perfectly well be
        // > ordinary compiled Pleris.
        //
        // Three independent facts, and the old rule collapsed the first into
        // the third:
        //
        // ```text
        // implementation   local Pleris / another component / the host
        // effects          database.write<Carts>, session.read, ..
        // authority        the capabilities those effects require
        // ```
        //
        // So the question is the DECLARATION's, not the effect row's: a `fn`
        // with a `host "pw:host/carts#add"` policy is supplied externally, and
        // one without is compiled here however effectful it is.
        let binding = resolved
            .and_then(|d| crate::resolve::declaration(self.cx.hirs, d))
            .and_then(crate::backend::host_binding);

        // Allocated whichever way the call goes, as it was before a compiled
        // call learned its type from its instance (ADR-0050): an inlined
        // call leaves it unused, and every value numbered after it, and so
        // every artifact, is what it was.
        let result = self.fresh();
        match binding {
            Some(import) => {
                // What the host returns, from the signature. Not inferred here.
                let ty = match &sig.returns {
                    Some(resolution) => match self.ty(resolution, &span) {
                        Lowering::Lowered(ty) => ty,
                        other => return other.map(|_| unreachable!()),
                    },
                    None => Type::Unit,
                };
                // A map or set the host answers is sorted as a query's
                // parameter is (ADR-0057, ADR-0259); one inside another value
                // is refused, since nothing sorts it.
                let check = match &ty {
                    Type::Map(..) => Some(Intrinsic::MapCheck),
                    Type::Set(..) => Some(Intrinsic::SetCheck),
                    t if holds_collection(self.cx, t, &mut Vec::new()) => {
                        return Lowering::Unsupported {
                            construct: "a map or set inside a host's answer",
                            span,
                            reason: "only a map or set that is itself the answer is checked \
                                     when it arrives (ADR-0057)"
                                .to_string(),
                        };
                    }
                    _ => None,
                };
                self.push(Instr::ImportCall {
                    result,
                    import,
                    args: lowered,
                    ty: ty.clone(),
                });
                match check {
                    Some(op) => {
                        let checked = self.fresh();
                        Lowering::Lowered(self.push(Instr::Intrinsic {
                            result: checked,
                            op,
                            args: vec![result],
                            ty,
                        }))
                    }
                    None => Lowering::Lowered(result),
                }
            }
            None => match def {
                // Compiled Pleris: inlined, so the component still exports
                // one function and imports only the host (ADR-0039 §4); a
                // recursion is a call to an instance compiled beside it
                // (ADR-0050).
                Some(callee) => self.inline(callee, lowered, expected, span),
                // A call that needs no authority and whose callee has no
                // resolved identity here — a platform declaration reached
                // through the prelude. Refused rather than emitted as a call to
                // nothing, which is what an invented `DefId` would be.
                None => Lowering::Unsupported {
                    construct: "a call to a declaration with no resolved identity",
                    span,
                    reason: format!("`{path}` has a signature but no `DefId` visible here"),
                },
            },
        }
    }
}

/// Does this region end by leaving the function (ADR-0051)?
/// Does a value of this type hold a map or a set, anywhere inside it: in a
/// list, an option, a result or a record's field?
fn holds_collection(cx: &Context<'_>, t: &Type, seen: &mut Vec<DefId>) -> bool {
    match t {
        Type::Map(..) | Type::Set(..) => true,
        Type::List(a) | Type::Option(a) => holds_collection(cx, a, seen),
        Type::Result(a, b) => holds_collection(cx, a, seen) || holds_collection(cx, b, seen),
        Type::Nominal(def, instance) => {
            if seen.contains(def) {
                return false;
            }
            seen.push(*def);
            let Some(decl) = cx.sigs.type_decl(*def) else {
                return false;
            };
            // Each field under the instance's arguments: a `Box<Map<..>>`
            // holds a map (ADR-0062).
            let subst: BTreeMap<(DefId, u32), Type> = instance
                .iter()
                .enumerate()
                .map(|(i, a)| ((*def, i as u32), a.clone()))
                .collect();
            let fields = decl.record.iter().flatten().map(|(_, r)| r);
            let rep = decl.representation.iter();
            // A sum type's payloads too (ADR-0059).
            let cases = decl.variants.iter().flatten().flat_map(|(_, fs)| fs.iter());
            instance.iter().any(|a| holds_collection(cx, a, seen))
                || fields.chain(rep).chain(cases).any(|r| {
                    r.resolved().is_some_and(|t| {
                        matches!(
                            ty_resolved_with(cx.sigs, t, &Span::default(), &subst),
                            Lowering::Lowered(ft) if holds_collection(cx, &ft, seen)
                        )
                    })
                })
        }
        Type::Int | Type::Float | Type::Bool | Type::Str | Type::Unit | Type::Function(..) => false,
    }
}

/// A variant type's cases, each with its payload's fields' types.
type Cases = Vec<(Case, Vec<Type>)>;

/// Does `ty` mention a type parameter `subst` does not bind?
fn mentions_unbound(ty: &ResolvedType, subst: &BTreeMap<(DefId, u32), Type>) -> bool {
    if let Some(key) = ty.parameter_binding() {
        return !subst.contains_key(&key);
    }
    ty.args().iter().any(|a| mentions_unbound(a, subst))
}

/// **A pattern as a decision tree reads it** (ADR-0060).
#[derive(Debug, Clone, PartialEq)]
enum TreePat {
    /// `_`, or a name bound to the value tested.
    Any(Option<String>),
    /// A variant's case, and a pattern for each field of its payload.
    Case(Case, Vec<TreePat>),
    /// `true` or `false`.
    Bool(bool),
    /// An `Int` or a `String`, equal to this.
    Lit(Const),
    /// `A | B`, binding nothing.
    Or(Vec<TreePat>),
}

impl TreePat {
    /// Does any part of this pattern bind a name?
    fn binds(&self) -> bool {
        match self {
            TreePat::Any(name) => name.is_some(),
            TreePat::Case(_, subs) | TreePat::Or(subs) => subs.iter().any(TreePat::binds),
            TreePat::Bool(_) | TreePat::Lit(_) => false,
        }
    }
}

/// One arm's patterns, one per value still to test, and the names its path
/// has bound.
#[derive(Debug, Clone)]
struct TreeRow {
    pats: Vec<TreePat>,
    arm: usize,
    binds: Vec<(String, ValueId)>,
}

/// **A decision tree** (ADR-0060): a node per value tested, a leaf per arm
/// reached.
enum Tree {
    /// The arm, and the names its path binds; its body, once lowered.
    Leaf {
        arm: usize,
        binds: Vec<(String, ValueId)>,
        region: Option<Region>,
    },
    /// A variant's discriminant: for each case the arms name, its fields'
    /// values and the tree below it; then the other cases, together.
    Switch {
        occ: ValueId,
        arms: Vec<(Vec<Case>, Vec<Option<ValueId>>, Tree)>,
    },
    /// A `Bool`.
    Test {
        occ: ValueId,
        then: Box<Tree>,
        els: Box<Tree>,
    },
    /// An `Int` or a `String`, compared with each literal the arms name.
    Literals {
        occ: ValueId,
        ty: Type,
        cases: Vec<(Const, Tree)>,
        otherwise: Box<Tree>,
    },
}

/// The rows with each `A | B` replaced by one row per alternative, in
/// order, wherever it is in a row.
fn expand_or(rows: Vec<TreeRow>) -> Vec<TreeRow> {
    let mut out = Vec::new();
    for row in rows {
        match row.pats.iter().position(|p| matches!(p, TreePat::Or(_))) {
            None => out.push(row),
            Some(i) => {
                let TreePat::Or(alternatives) = row.pats[i].clone() else {
                    unreachable!("found above")
                };
                let expanded = alternatives
                    .into_iter()
                    .map(|a| {
                        let mut r = row.clone();
                        r.pats[i] = a;
                        r
                    })
                    .collect();
                out.extend(expand_or(expanded));
            }
        }
    }
    out
}

/// **Which cases one match arm takes** (ADR-0059), by their positions in
/// the scrutinee's cases.
enum Takes {
    /// One case, and the name each field of its payload is bound to, if any:
    /// `Some(y)`, `Rect(w, _)`, `Empty`.
    Case(usize, Vec<Option<String>>),
    /// Several cases, binding nothing: `A | B`.
    Cases(Vec<usize>),
    /// Every case no arm before it takes: `_`, or a name bound to the whole
    /// value.
    Rest(Option<String>),
}

fn diverges(r: &Region) -> bool {
    matches!(r.instrs.last(), Some(Instr::Return { result, .. }) if *result == r.value)
}

/// **Does a body leave early**: a `return` or a `?` outside every lambda
/// (ADR-0051)? Such a callee is compiled beside its export and called, since
/// inlined, its `return` would leave its caller.
fn exits_early(body: &Body) -> bool {
    let mut lambdas = BTreeSet::new();
    for id in body.walk() {
        if let Expr::Lambda { body: inner, .. } = body.expr(id) {
            lambdas.extend(body.walk_from(*inner));
        }
    }
    body.walk().into_iter().any(|id| {
        !lambdas.contains(&id)
            && match body.expr(id) {
                Expr::Name(n) => n == "return",
                Expr::Try { .. } => true,
                _ => false,
            }
    })
}

/// An argument to an intrinsic, as it arrives: a piped value already lowered,
/// or an expression written in the call. A field of a record or a case
/// arrives so too, a record's shorthand as the binding it names (ADR-0062).
enum Given {
    Value(ValueId),
    Expr(ExprId),
}

/// The type an intrinsic's `k`th argument must have, where the operation
/// fixes it: what `[]` or `None` written there needs to know.
fn intrinsic_argument(op: Intrinsic, k: usize, prior: &[Type]) -> Option<Type> {
    use Intrinsic as I;
    match (op, k) {
        (I::ListGet | I::ListTake | I::ListDrop, 1) => Some(Type::Int),
        (I::ListSlice | I::StrSlice, 1 | 2) => Some(Type::Int),
        (I::ListRange, 0 | 1) => Some(Type::Int),
        (I::StrSlice, 0) => Some(Type::Str),
        // A key or an element where the map or set before it fixes it.
        (I::MapGet | I::MapContains | I::MapRemove | I::MapInsert, 1) => match prior.first() {
            Some(Type::Map(k, _)) => Some((**k).clone()),
            _ => None,
        },
        (I::MapInsert, 2) => match prior.first() {
            Some(Type::Map(_, v)) => Some((**v).clone()),
            _ => None,
        },
        (I::SetContains | I::SetInsert | I::SetRemove, 1) => match prior.first() {
            Some(Type::Set(t)) => Some((**t).clone()),
            _ => None,
        },
        (I::SetUnion | I::SetIntersection | I::SetDifference, 1) => prior.first().cloned(),
        (I::ListConcat, 1) => prior.first().cloned(),
        (I::StrFromCodepoints, 0) => Some(Type::List(Box::new(Type::Int))),
        (I::StrJoin, 0) => Some(Type::List(Box::new(Type::Str))),
        (I::FloatFromInt, 0) => Some(Type::Int),
        (
            I::StrLength
            | I::StrCodepoints
            | I::StrStartsWith
            | I::StrEndsWith
            | I::StrContains
            | I::StrTrim
            | I::StrToLowerAscii
            | I::StrToLower
            | I::StrToUpper,
            _,
        )
        | (I::StrJoin, 1) => Some(Type::Str),
        _ => None,
    }
}

/// Every expression kind by name: a refusal says which construct it is,
/// never "this expression".
fn construct_name(e: &Expr) -> &'static str {
    match e {
        Expr::Name(_) => "a name",
        Expr::Literal(_) => "a literal",
        Expr::Field { .. } => "a field access",
        Expr::Call { .. } => "a call",
        Expr::Address { .. } => "a page's address",
        Expr::Lambda { .. } => "a lambda",
        Expr::Binary { .. } => "a binary operator",
        Expr::Cast { .. } => "a cast",
        Expr::Try { .. } => "a `?` propagation",
        Expr::Interpolated { .. } => "an interpolated string",
        Expr::Unary { .. } => "a unary operator",
        Expr::Block { .. } => "a block",
        Expr::If { .. } => "an `if`",
        Expr::Match { .. } => "a `match`",
        Expr::For { .. } => "a `for` loop",
        Expr::Record { name: None, .. } => "a record with no type name",
        Expr::Record { .. } => "a record",
        Expr::List { .. } => "a list",
        Expr::Let { .. } => "a binding where a value is needed",
        Expr::Keyword { .. } => "a body-level statement",
        Expr::Template { .. } => "markup",
        Expr::Error => "an expression that did not parse",
    }
}

#[cfg(test)]
mod checked_input_tests {
    use super::*;
    #[test]
    fn another_program_of_equal_length_cannot_receive_the_proof() {
        let units = |src: &str| {
            vec![crate::check::Unit {
                path: "app.pw".into(),
                src: src.into(),
                hir: crate::lower::lower_file(src, &pw_syntax::parse_tree(src).green),
            }]
        };
        let a = units("module a\nquery N() -> Int { 1 }\n");
        let b = units("module b\nquery N() -> Int { 2 }\n");
        let hirs: Vec<_> = b.iter().map(|u| &u.hir).collect();
        let ws = Workspace::build(&hirs);
        let sigs = Signatures::build(&ws, &hirs);
        let mut inference = crate::effects::Inference::new(&sigs, &ws);
        inference.run(&hirs);
        let make = || {
            Checked::diagnose_inferred(
                &a,
                Context {
                    hirs: &hirs,
                    ws: &ws,
                    sigs: &sigs,
                    contracts: &[],
                },
                &inference,
            )
        };
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(make)).is_err());
        assert!(
            Checked::diagnose_inferred(
                &b,
                Context {
                    hirs: &hirs,
                    ws: &ws,
                    sigs: &sigs,
                    contracts: &[]
                },
                &inference
            )
            .is_ok()
        );
    }
}
