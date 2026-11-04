//! **A read is bounded by construction** (4c; the design note on
//! `track/bounded-reads`, which the integrator accepted on 2026-10-14).
//!
//! The owner asked that N+1 be impossible by construction, whatever the
//! database. Above the seam, a read whose key comes from a loop's element is
//! refused; below it, a source states what one set read costs it.
//!
//! - **PW0405**: a call that performs a source read (a `database.read<R>` in
//!   its inferred row, reached through a function a `host` clause binds) is
//!   refused where it is inside a loop over data and its key comes from the
//!   loop's element. A loop over data is a callback a combinator applies to a
//!   list's elements (`List.map`, `List.fold`, .., or a function of the
//!   program's that applies its callback so), a `for` body, or a recursion
//!   over data (a function on a cycle of the call graph whose recursive call
//!   is passed a part of its own argument). A key comes from the element
//!   through a name, a `let`, a field or a pure call. The repair is the set
//!   read, before the loop.
//! - **PW0406**, a warning: a set read on a source that cannot batch (`sets
//!   per_key`, or no `sets`) is a call per key, so the calls of the page or
//!   the command that makes it grow with its data.
//! - **`set`** on a keyed read names its set form, held to its shape
//!   (PW0407, the pair's own rule): a function of the same source, given a
//!   list of the keyed read's key, answering a list of what the keyed read
//!   answers, with its key.
//! - **`sets`** on a source: `one_call`, `chunked(k)` or `per_key`, which a
//!   source that states none is.
//!
//! Each half is read off what the effect walk already recorded (ADR-0022):
//! which call performs `database.read<R>` and which declaration it calls
//! (`Inferred::calls`), which callback a call sits in
//! (`effects::callbacks`), and which binding each name means (`lexical`).
//! What is new is small: which parameters of a declaration key a source read
//! inside it, which a callable applies to a list's elements, and which a
//! recursion passes a part of to itself again.

use std::collections::{BTreeMap, BTreeSet};

use crate::check::Source;
use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::effects::{Called, Inference, Inferred, database_domain};
use crate::hir::{Body, Decl, DeclId, DeclKind, Expr, ExprId, Hir, Pattern, PatternId, Span};
use crate::lexical::{Binder, Lexical};
use crate::resolve::{DefId, Namespace, Resolution, Workspace};
use crate::resolved::{Builtin, ResolvedType, TypeResolution};
use crate::signatures::{Signature, Signatures};

/// **What one set read costs a source** (`sets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sets {
    /// One call whatever its keys: PostgreSQL's `= ANY($1)`, Firestore's
    /// `batchGet` by name.
    OneCall,
    /// At least ⌈n/k⌉ calls for n keys: DynamoDB's `BatchGetItem` (k = 100),
    /// Firestore's `in` (k = 30).
    Chunked(u64),
    /// A call per key: a source that cannot batch.
    PerKey,
}

impl Sets {
    /// **A source's `sets`, as written.** A clause left out, or a value its
    /// domain does not have (which PW0335 refuses), guarantees nothing:
    /// `per_key`.
    pub fn of(written: Option<&str>) -> Sets {
        let Some(value) = written.map(str::trim) else {
            return Sets::PerKey;
        };
        if crate::policy::value_fault("sets", value).is_some() {
            return Sets::PerKey;
        }
        match value {
            "one_call" => Sets::OneCall,
            "per_key" => Sets::PerKey,
            _ => value
                .split_once('(')
                .and_then(|(_, k)| k.trim_end().strip_suffix(')'))
                .and_then(|k| k.trim().parse::<u64>().ok())
                .filter(|k| *k > 0)
                .map_or(Sets::PerKey, Sets::Chunked),
        }
    }

    /// As a `sets` clause writes it.
    pub fn written(self) -> String {
        match self {
            Sets::OneCall => "one_call".to_string(),
            Sets::Chunked(k) => format!("chunked({k})"),
            Sets::PerKey => "per_key".to_string(),
        }
    }

    /// **The calls one set read of `n` keys makes** (the design note's
    /// budget): one on `one_call`, ⌈n/k⌉ on `chunked(k)`, n on `per_key`.
    pub fn calls(self, n: u64) -> u64 {
        match self {
            Sets::OneCall => 1,
            Sets::Chunked(k) => n.div_ceil(k),
            Sets::PerKey => n,
        }
    }
}

/// **A source read a parameter keys**: the host read it reaches, which of
/// that read's parameters, and the resource it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reached {
    read: DefId,
    param: usize,
    resource: String,
}

/// **What binds a pattern's names**, by the expression that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Let(ExprId),
    Lambda(ExprId),
    For(ExprId),
    Arm(ExprId),
}

/// **A loop over data in one body**: what turns it, the span a read must be
/// inside to run on each turn, the names each turn binds, and the list it
/// walks where the program names one.
#[derive(Debug, Clone)]
struct Region {
    turn: Turn,
    /// The lambda, the `for`, or the body's root.
    owner: ExprId,
    inside: Span,
    bindings: BTreeSet<Binder>,
    collection: Option<ExprId>,
}

#[derive(Debug, Clone)]
enum Turn {
    /// A callback a combinator applies to a list's elements: the call, and
    /// the combinator as written.
    Callback { call: ExprId, combinator: String },
    /// A `for` loop.
    For,
    /// A recursion over data, at its recursive call.
    Recursion { at: ExprId },
}

/// **One body, as the effect walk saw it.**
struct BodyCx<'b> {
    unit: usize,
    def: DefId,
    decl: &'b Decl,
    body: &'b Body,
    lexical: &'b Lexical,
    performed: &'b Inferred,
    calls: BTreeMap<ExprId, &'b Called>,
    owners: BTreeMap<PatternId, Owner>,
    /// `xs |> f(..)`: the call, and the value piped into it.
    piped: BTreeMap<ExprId, ExprId>,
}

/// **The program's reads, by what keys them and what loops over them.**
pub(crate) struct Reads<'p, 'i> {
    hirs: &'p [&'p Hir],
    ws: &'p Workspace,
    sigs: &'p Signatures,
    inference: &'p Inference<'i>,
    sources: &'p [Source],
    /// Each declaration's parameters that key a source read inside it.
    keys: BTreeMap<DefId, BTreeMap<usize, Reached>>,
    /// Each callable's parameters it applies to a list's elements, and the
    /// parameter that list is, where one is.
    applies: BTreeMap<DefId, BTreeMap<usize, Option<usize>>>,
    /// Each declaration on a cycle of the call graph, by its component.
    cycles: BTreeMap<DefId, usize>,
    /// Each declaration that recurses over data: the parameters its
    /// recursive calls are passed a part of, and the first such call.
    recursive: BTreeMap<DefId, (BTreeSet<usize>, ExprId)>,
    /// Each keyed read whose `set` names its set form, and that set read.
    pairs: BTreeMap<DefId, DefId>,
    /// Each set read, and the keyed read that names it.
    set_reads: BTreeMap<DefId, DefId>,
    /// Each `set` whose shape does not fit, by its unit.
    faults: Vec<(usize, Diagnostic)>,
    /// **Every body the walk saw**, each read once (4c's passes revisit
    /// them until their facts settle), in unit and declaration order.
    bodies: Vec<BodyCx<'p>>,
}

/// The rounds a fixed point takes at most: each adds a parameter, of
/// finitely many.
const ROUNDS: usize = 16;

impl<'p, 'i> Reads<'p, 'i> {
    /// **Every fact the rules read, over the whole program.**
    pub(crate) fn of(
        hirs: &'p [&'p Hir],
        ws: &'p Workspace,
        sigs: &'p Signatures,
        inference: &'p Inference<'i>,
        sources: &'p [Source],
    ) -> Reads<'p, 'i> {
        let mut out = Reads {
            hirs,
            ws,
            sigs,
            inference,
            sources,
            keys: BTreeMap::new(),
            applies: BTreeMap::new(),
            cycles: BTreeMap::new(),
            recursive: BTreeMap::new(),
            pairs: BTreeMap::new(),
            set_reads: BTreeMap::new(),
            faults: Vec::new(),
            bodies: Vec::new(),
        };
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, _) in hir.all_decls() {
                if let Some(cx) = out.cx(unit, id) {
                    out.bodies.push(cx);
                }
            }
        }
        out.cycles = out.call_cycles();
        // What a callable applies to a list's elements, and what recurses
        // over data, each reading the other: a callback applied in a
        // recursion, or a recursion through a combinator's callback.
        for _ in 0..ROUNDS {
            let recursive = out.recursion();
            let changed = recursive != out.recursive;
            out.recursive = recursive;
            let applies = out.applied();
            let changed = changed || applies != out.applies;
            out.applies = applies;
            if !changed {
                break;
            }
        }
        out.keys = out.keyed();
        out.pair();
        out
    }

    /// **What `unit` reads wrong**: its `set` clauses that do not fit
    /// (PW0407), its reads per element (PW0405), and its set reads on a
    /// source that cannot batch (PW0406).
    pub(crate) fn check(&self, unit: usize, src: &str) -> Vec<Diagnostic> {
        let mut out: Vec<Diagnostic> = self
            .faults
            .iter()
            .filter(|(u, _)| *u == unit)
            .map(|(_, d)| d.clone())
            .collect();
        for cx in self.bodies.iter().filter(|cx| cx.unit == unit) {
            let calls = &cx.performed.calls;
            if calls
                .iter()
                .any(|c| self.keys.contains_key(&c.callee) || self.applies.contains_key(&c.callee))
            {
                self.per_element(cx, src, &mut out);
            }
            if calls.iter().any(|c| self.set_reads.contains_key(&c.callee)) {
                self.per_key(cx, &mut out);
            }
        }
        out
    }

    // --- one body ----------------------------------------------------------

    fn def(unit: usize, id: DeclId) -> DefId {
        DefId { unit, decl: id.0 }
    }

    fn decl_of(&self, def: DefId) -> &'p Decl {
        self.hirs[def.unit].decl(DeclId(def.decl))
    }

    fn decl_span(&self, def: DefId) -> Span {
        self.hirs[def.unit].decl_span(DeclId(def.decl))
    }

    /// The declaration `id` of `unit`'s body, where the walk saw one.
    fn cx(&self, unit: usize, id: DeclId) -> Option<BodyCx<'p>> {
        let hir: &'p Hir = self.hirs.get(unit)?;
        let decl = hir.decl(id);
        let body = hir.body(decl.body?);
        let def = Self::def(unit, id);
        let performed = self.inference.performed_of(def)?;
        let lexical = self.inference.lexical(unit, id)?;
        let mut owners = BTreeMap::new();
        let mut piped = BTreeMap::new();
        for e in body.walk() {
            let mut own = |p: PatternId, owner: Owner| {
                for q in pattern_tree(body, p) {
                    owners.insert(q, owner);
                }
            };
            match body.expr(e) {
                Expr::Let { pat: Some(p), .. } => own(*p, Owner::Let(e)),
                Expr::Lambda { params, .. } => {
                    for p in params {
                        own(*p, Owner::Lambda(e));
                    }
                }
                Expr::For { pat: Some(p), .. } => own(*p, Owner::For(e)),
                Expr::Match { arms, .. } => {
                    for arm in arms {
                        own(arm.pat, Owner::Arm(e));
                    }
                }
                Expr::Binary {
                    op: crate::hir::BinOp::Pipe,
                    lhs,
                    rhs,
                } => {
                    piped.insert(*rhs, *lhs);
                }
                _ => {}
            }
        }
        Some(BodyCx {
            unit,
            def,
            decl,
            body,
            lexical,
            performed,
            calls: performed.calls.iter().map(|c| (c.call, c)).collect(),
            owners,
            piped,
        })
    }

    /// **Whether a member call's base is a value it is called on**, rather
    /// than a module's path: its head is a binding in scope, or no name.
    fn receives(cx: &BodyCx<'_>, base: ExprId) -> bool {
        let mut head = base;
        while let Expr::Field { base, .. } = cx.body.expr(head) {
            head = *base;
        }
        match cx.body.expr(head) {
            Expr::Name(_) => cx.lexical.binder(head).is_some(),
            _ => true,
        }
    }

    /// **The expression each parameter of the call's callee is given**, by
    /// the rule `arrange` gives them (ADR-0081): a member call's receiver and
    /// a piped value first. `None` where the callee is no declaration the
    /// walk resolved, or the arguments do not arrange.
    fn given(&self, cx: &BodyCx<'_>, call: ExprId) -> Option<Vec<Option<ExprId>>> {
        let called = cx.calls.get(&call)?;
        let sig = self.sigs.by_def(called.callee)?;
        let (callee, args) = match cx.body.expr(call) {
            Expr::Call { callee, args } => (*callee, args),
            Expr::Field { base, .. } if self.sigs.property_receiver(sig).is_some() => {
                return Some(vec![Some(*base)]);
            }
            _ => return None,
        };
        let mut leading: Vec<ExprId> = Vec::new();
        if let Expr::Field { base, .. } = cx.body.expr(callee)
            && Self::receives(cx, *base)
        {
            leading.push(*base);
        }
        if let Some(value) = cx.piped.get(&call) {
            leading.push(*value);
        }
        let written: Vec<Option<&str>> = args.iter().map(|a| a.name.as_deref()).collect();
        let order = crate::signatures::arrange(&sig.names, leading.len(), &written).ok()?;
        let mut out = vec![None; sig.names.len().max(leading.len() + args.len())];
        for (i, value) in leading.into_iter().enumerate() {
            out[i] = Some(value);
        }
        for (k, i) in order.into_iter().enumerate() {
            if let Some(slot) = out.get_mut(i) {
                *slot = Some(args[k].value);
            }
        }
        Some(out)
    }

    /// **Does `e` come from a binding `wanted` names?** Through a name, a
    /// `let`'s initializer, a match arm's scrutinee, a field, and a pure
    /// call's arguments (one the walk charged nothing, or one through a
    /// value); where `elements` is given, through a loop's element to the
    /// list it walks. The binding it reaches.
    fn depends(
        &self,
        cx: &BodyCx<'_>,
        e: ExprId,
        wanted: &dyn Fn(Binder) -> bool,
        elements: Option<&BTreeMap<ExprId, ExprId>>,
        seen: &mut BTreeSet<ExprId>,
    ) -> Option<Binder> {
        if !seen.insert(e) {
            return None;
        }
        let body = cx.body;
        match body.expr(e) {
            Expr::Name(_) => {
                let b = cx.lexical.binder(e)?;
                self.bound_depends(cx, b, wanted, elements, seen)
            }
            Expr::Field { base, .. } => self.depends(cx, *base, wanted, elements, seen),
            Expr::Call { callee, args } => {
                if cx.calls.get(&e).is_some_and(|c| !c.effects.is_empty()) {
                    return None;
                }
                std::iter::once(*callee)
                    .chain(args.iter().map(|a| a.value))
                    .find_map(|x| self.depends(cx, x, wanted, elements, seen))
            }
            Expr::Record { fields, .. } => {
                fields.iter().enumerate().find_map(|(i, f)| match f.value {
                    Some(v) => self.depends(cx, v, wanted, elements, seen),
                    None => cx
                        .lexical
                        .shorthand(e, i)
                        .and_then(|b| self.bound_depends(cx, b, wanted, elements, seen)),
                })
            }
            // A block's value is its last statement's.
            Expr::Block { stmts } => stmts
                .last()
                .and_then(|s| self.depends(cx, *s, wanted, elements, seen)),
            // A function, a binding and a loop are no key; nor what names a
            // page or runs a statement.
            Expr::Lambda { .. }
            | Expr::Let { .. }
            | Expr::For { .. }
            | Expr::Literal(_)
            | Expr::Error
            | Expr::Address { .. }
            | Expr::Keyword { .. }
            | Expr::Template { .. } => None,
            Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Cast { .. }
            | Expr::Try { .. }
            | Expr::Interpolated { .. }
            | Expr::List { .. }
            | Expr::If { .. }
            | Expr::Match { .. } => body
                .children(e)
                .into_iter()
                .find_map(|x| self.depends(cx, x, wanted, elements, seen)),
        }
    }

    /// `depends`, for what a binding is bound to.
    fn bound_depends(
        &self,
        cx: &BodyCx<'_>,
        b: Binder,
        wanted: &dyn Fn(Binder) -> bool,
        elements: Option<&BTreeMap<ExprId, ExprId>>,
        seen: &mut BTreeSet<ExprId>,
    ) -> Option<Binder> {
        if wanted(b) {
            return Some(b);
        }
        let Binder::Pattern(p) = b else {
            return None;
        };
        match *cx.owners.get(&p)? {
            Owner::Let(l) => match cx.body.expr(l) {
                Expr::Let {
                    init: Some(init), ..
                } => self.depends(cx, *init, wanted, elements, seen),
                _ => None,
            },
            Owner::Arm(m) => match cx.body.expr(m) {
                Expr::Match { scrutinee, .. } => {
                    self.depends(cx, *scrutinee, wanted, elements, seen)
                }
                _ => None,
            },
            Owner::Lambda(l) | Owner::For(l) => {
                let list = *elements?.get(&l)?;
                self.depends(cx, list, wanted, elements, seen)
            }
        }
    }

    /// The name a binding is written as.
    fn bound_name(cx: &BodyCx<'_>, b: Binder) -> String {
        match b {
            Binder::Param(i) => cx
                .decl
                .params
                .get(i)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            Binder::Pattern(p) => match cx.body.pat(p) {
                Pattern::Bind { name, .. } => name.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    // --- loops ---------------------------------------------------------------

    /// **The loops over data in a body**: each callback a combinator applies
    /// to a list's elements, written as an argument or bound by a `let` and
    /// passed; each `for`; and, where `recursion` asks, the body itself when
    /// its declaration recurses over data.
    fn regions(&self, cx: &BodyCx<'_>, recursion: bool) -> Vec<Region> {
        let body = cx.body;
        let mut out = Vec::new();
        let mut seen: BTreeSet<ExprId> = BTreeSet::new();
        // Written as an argument: the callbacks the walk reads
        // (`effects::callbacks`).
        for (lambda, handed) in crate::effects::callbacks(body, body.root) {
            let Some(called) = cx.calls.get(&handed.call) else {
                continue;
            };
            let Some(applied) = self.applies.get(&called.callee) else {
                continue;
            };
            let Some(given) = self.given(cx, handed.call) else {
                continue;
            };
            let Some(param) = given.iter().position(|g| *g == Some(lambda)) else {
                continue;
            };
            let Some(list) = applied.get(&param) else {
                continue;
            };
            let collection = list.and_then(|i| given.get(i).copied().flatten());
            if seen.insert(lambda) {
                out.push(Self::callback(
                    cx,
                    lambda,
                    handed.call,
                    &handed.to,
                    collection,
                ));
            }
        }
        // Bound by a `let`, then passed.
        for c in &cx.performed.calls {
            let Some(applied) = self.applies.get(&c.callee) else {
                continue;
            };
            let Some(given) = self.given(cx, c.call) else {
                continue;
            };
            for (param, list) in applied {
                let Some(Some(arg)) = given.get(*param) else {
                    continue;
                };
                let Some(lambda) = Self::let_lambda(cx, *arg) else {
                    continue;
                };
                let collection = list.and_then(|i| given.get(i).copied().flatten());
                let Expr::Call { callee, .. } = body.expr(c.call) else {
                    continue;
                };
                if seen.insert(lambda) {
                    out.push(Self::callback(
                        cx,
                        lambda,
                        c.call,
                        &crate::infer::path_of(body, *callee),
                        collection,
                    ));
                }
            }
        }
        for e in body.walk() {
            if let Expr::For {
                pat,
                iterable,
                body: turn,
            } = body.expr(e)
            {
                out.push(Region {
                    turn: Turn::For,
                    owner: e,
                    inside: body.expr_span(*turn),
                    bindings: pat.map(|p| binds(body, p)).unwrap_or_default(),
                    collection: Some(*iterable),
                });
            }
        }
        if recursion && let Some((params, at)) = self.recursive.get(&cx.def) {
            out.push(Region {
                turn: Turn::Recursion { at: *at },
                owner: body.root,
                inside: body.expr_span(body.root),
                bindings: params.iter().map(|i| Binder::Param(*i)).collect(),
                collection: None,
            });
        }
        out
    }

    fn callback(
        cx: &BodyCx<'_>,
        lambda: ExprId,
        call: ExprId,
        combinator: &str,
        collection: Option<ExprId>,
    ) -> Region {
        let Expr::Lambda { params, body, .. } = cx.body.expr(lambda) else {
            unreachable!("a callback is a lambda");
        };
        Region {
            turn: Turn::Callback {
                call,
                combinator: combinator.to_string(),
            },
            owner: lambda,
            inside: cx.body.expr_span(*body),
            bindings: params.iter().flat_map(|p| binds(cx.body, *p)).collect(),
            collection,
        }
    }

    /// **The lambda a name is bound to by a `let`**, through `let`s.
    fn let_lambda(cx: &BodyCx<'_>, e: ExprId) -> Option<ExprId> {
        let mut at = e;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(at) {
                return None;
            }
            match cx.body.expr(at) {
                Expr::Lambda { .. } => return Some(at),
                Expr::Name(_) => {
                    let Binder::Pattern(p) = cx.lexical.binder(at)? else {
                        return None;
                    };
                    let Owner::Let(l) = *cx.owners.get(&p)? else {
                        return None;
                    };
                    let Expr::Let {
                        pat: Some(bound),
                        init: Some(init),
                        ..
                    } = cx.body.expr(l)
                    else {
                        return None;
                    };
                    // The whole value, not a part a pattern takes apart.
                    if *bound != p {
                        return None;
                    }
                    at = *init;
                }
                _ => return None,
            }
        }
    }

    /// Each loop's element, by the lambda or the `for` that binds it, and the
    /// list it walks.
    fn elements(regions: &[Region]) -> BTreeMap<ExprId, ExprId> {
        regions
            .iter()
            .filter_map(|r| Some((r.owner, r.collection?)))
            .collect()
    }

    /// **Which parameters each callable applies to a list's elements.** A
    /// declaration with no body, as its signature says: a parameter
    /// `fn(.., T, ..) -> U` beside one `List<T>`, which by its type can only
    /// be given the list's elements (`List.map`, `List.fold`, `List.sort_by`,
    /// ..). One with a body, as the call graph shows: a parameter it calls in
    /// a loop over data, or passes on to be applied so.
    fn applied(&self) -> BTreeMap<DefId, BTreeMap<usize, Option<usize>>> {
        let mut out: BTreeMap<DefId, BTreeMap<usize, Option<usize>>> = BTreeMap::new();
        for (unit, hir) in self.hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                if decl.kind != DeclKind::Fn || decl.body.is_some() {
                    continue;
                }
                let def = Self::def(unit, id);
                if let Some(sig) = self.sigs.by_def(def) {
                    let found = by_signature(sig);
                    if !found.is_empty() {
                        out.insert(def, found);
                    }
                }
            }
        }
        for cx in &self.bodies {
            if !self.takes_a_function(cx) {
                continue;
            }
            let found = self.applied_in(cx);
            if !found.is_empty() {
                out.insert(cx.def, found);
            }
        }
        out
    }

    /// **Could the body's declaration apply a parameter at all?** One whose
    /// parameters are each written with a type, none of them a function's,
    /// applies none.
    fn takes_a_function(&self, cx: &BodyCx<'_>) -> bool {
        let Some(sig) = self.sigs.by_def(cx.def) else {
            return true;
        };
        sig.params.iter().any(|p| {
            p.as_ref()
                .and_then(TypeResolution::resolved)
                .is_none_or(|t| t.as_builtin() == Some(Builtin::Function))
        })
    }

    /// The parameters a body applies to a list's elements.
    fn applied_in(&self, cx: &BodyCx<'_>) -> BTreeMap<usize, Option<usize>> {
        let mut out = BTreeMap::new();
        let regions = self.regions(cx, true);
        let param = |e: ExprId| match (cx.body.expr(e), cx.lexical.binder(e)) {
            (Expr::Name(_), Some(Binder::Param(i))) => Some(i),
            _ => None,
        };
        let listed = |i: usize| {
            let sig = self.sigs.by_def(cx.def)?;
            let ty = sig.params.get(i)?.as_ref()?.resolved()?;
            (ty.as_builtin() == Some(Builtin::List)).then_some(i)
        };
        // Called in a loop over data.
        for e in cx.body.walk() {
            let Expr::Call { callee, .. } = cx.body.expr(e) else {
                continue;
            };
            let Some(j) = param(*callee) else {
                continue;
            };
            let at = cx.body.expr_span(e);
            if let Some(r) = regions
                .iter()
                .find(|r| r.inside.start <= at.start && at.end <= r.inside.end)
            {
                let list = match &r.turn {
                    Turn::Recursion { .. } => r.bindings.iter().find_map(|b| match b {
                        Binder::Param(i) => listed(*i),
                        _ => None,
                    }),
                    _ => r.collection.and_then(param).and_then(listed),
                };
                out.entry(j).or_insert(list);
            }
        }
        // Passed on to be applied so.
        for c in &cx.performed.calls {
            let Some(applied) = self.applies.get(&c.callee) else {
                continue;
            };
            let Some(given) = self.given(cx, c.call) else {
                continue;
            };
            for (k, list) in applied {
                let Some(j) = given.get(*k).copied().flatten().and_then(param) else {
                    continue;
                };
                let list = list
                    .and_then(|i| given.get(i).copied().flatten())
                    .and_then(param)
                    .and_then(listed);
                out.entry(j).or_insert(list);
            }
        }
        out
    }

    /// **Each declaration that recurses over data**: on a cycle of the call
    /// graph the walk recorded, with a recursive call passed a part of one of
    /// its parameters, through a field, a pure call, a `let` or a loop's
    /// element. A parameter passed whole is passed along, not walked: to the
    /// same position of the same declaration, or handed on through a cycle of
    /// several (`ping` and `pong` giving `name` to each other while a count
    /// falls), where it is walked only if the declaration it is handed to
    /// walks the position it lands in. A whole parameter makes no new key.
    fn recursion(&self) -> BTreeMap<DefId, (BTreeSet<usize>, ExprId)> {
        let components = &self.cycles;
        // Each declaration's parameters a recursive call is passed a part of,
        // and the first such call; and each whole hand-over, from a parameter
        // of one declaration to a position of another on its cycle.
        let mut out: BTreeMap<DefId, (BTreeSet<usize>, ExprId)> = BTreeMap::new();
        let mut handed: Vec<(DefId, usize, DefId, usize, ExprId)> = Vec::new();
        for cx in &self.bodies {
            let def = cx.def;
            let Some(own) = components.get(&def) else {
                continue;
            };
            let elements = Self::elements(&self.regions(cx, false));
            for c in &cx.performed.calls {
                if components.get(&c.callee) != Some(own) {
                    continue;
                }
                let Some(given) = self.given(cx, c.call) else {
                    continue;
                };
                for (k, arg) in given.iter().enumerate() {
                    let Some(arg) = arg else { continue };
                    for i in 0..cx.decl.params.len() {
                        let whole = matches!(cx.body.expr(*arg), Expr::Name(_))
                            && cx.lexical.binder(*arg) == Some(Binder::Param(i));
                        if whole {
                            handed.push((def, i, c.callee, k, c.call));
                            continue;
                        }
                        let wanted = |b: Binder| b == Binder::Param(i);
                        if self
                            .depends(cx, *arg, &wanted, Some(&elements), &mut BTreeSet::new())
                            .is_some()
                        {
                            let walked =
                                out.entry(def).or_insert_with(|| (BTreeSet::new(), c.call));
                            walked.0.insert(i);
                        }
                    }
                }
            }
        }
        // A parameter handed whole to a position another declaration walks is
        // walked: what lands there is a new key each turn.
        loop {
            let mut changed = false;
            for (def, i, callee, k, call) in &handed {
                let lands = out
                    .get(callee)
                    .is_some_and(|(walked, _)| walked.contains(k));
                let walked = out.get(def).is_some_and(|(walked, _)| walked.contains(i));
                if lands && !walked {
                    out.entry(*def)
                        .or_insert_with(|| (BTreeSet::new(), *call))
                        .0
                        .insert(*i);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        out
    }

    /// **The call graph's cycles**, as the walk recorded each call
    /// (`Inferred::calls`): each declaration on one, by its component.
    fn call_cycles(&self) -> BTreeMap<DefId, usize> {
        let edges: BTreeMap<DefId, BTreeSet<DefId>> = self
            .bodies
            .iter()
            .map(|cx| {
                (
                    cx.def,
                    cx.performed.calls.iter().map(|c| c.callee).collect(),
                )
            })
            .collect();
        components(&edges)
    }

    /// **Is `c` a recursive call of the body's declaration**: a call of one
    /// on its own cycle? The recursion's turn, whose reads are refused where
    /// they are written.
    fn recurses(&self, cx: &BodyCx<'_>, c: &Called) -> bool {
        self.cycles
            .get(&c.callee)
            .is_some_and(|n| self.cycles.get(&cx.def) == Some(n))
    }

    // --- keys --------------------------------------------------------------

    /// **Which parameters of each declaration key a source read.** A
    /// function a `host` clause binds, whose row reads `database.read<R>`:
    /// each of its parameters. One with a body: each parameter a key of a
    /// read it calls comes from, through a name, a `let`, a field or a pure
    /// call. A read keyed by a loop's element inside it is that loop's, and
    /// refused there.
    fn keyed(&self) -> BTreeMap<DefId, BTreeMap<usize, Reached>> {
        let mut out: BTreeMap<DefId, BTreeMap<usize, Reached>> = BTreeMap::new();
        for (unit, hir) in self.hirs.iter().enumerate() {
            for (id, decl) in hir.all_decls() {
                if decl.kind != DeclKind::Fn || decl.body.is_some() || decl.policy("host").is_none()
                {
                    continue;
                }
                let Some(resource) = decl
                    .declared_effects
                    .iter()
                    .flatten()
                    .find_map(|e| database_domain(&e.written, "read"))
                else {
                    continue;
                };
                let def = Self::def(unit, id);
                let keyed: BTreeMap<usize, Reached> = (0..decl.params.len())
                    .map(|i| {
                        (
                            i,
                            Reached {
                                read: def,
                                param: i,
                                resource: resource.to_string(),
                            },
                        )
                    })
                    .collect();
                if !keyed.is_empty() {
                    out.insert(def, keyed);
                }
            }
        }
        for _ in 0..ROUNDS {
            let mut changed = false;
            for cx in &self.bodies {
                for c in &cx.performed.calls {
                    let Some(keyed) = out.get(&c.callee).cloned() else {
                        continue;
                    };
                    let Some(given) = self.given(cx, c.call) else {
                        continue;
                    };
                    for (k, reached) in keyed {
                        let Some(Some(arg)) = given.get(k) else {
                            continue;
                        };
                        for i in 0..cx.decl.params.len() {
                            if out.get(&cx.def).is_some_and(|m| m.contains_key(&i)) {
                                continue;
                            }
                            let wanted = |b: Binder| b == Binder::Param(i);
                            if self
                                .depends(cx, *arg, &wanted, None, &mut BTreeSet::new())
                                .is_some()
                            {
                                out.entry(cx.def).or_default().insert(i, reached.clone());
                                changed = true;
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        out
    }

    // --- PW0405 --------------------------------------------------------------

    /// **Each read in a loop over data keyed by the loop's element** (PW0405):
    /// a call whose key comes from a binding of a loop it is inside, the
    /// innermost first; and a function named as a combinator's callback that
    /// keys a read by what it is given.
    fn per_element(&self, cx: &BodyCx<'p>, src: &str, out: &mut Vec<Diagnostic>) {
        let regions = self.regions(cx, true);
        for c in &cx.performed.calls {
            // A recursive call is the recursion's turn: its reads are refused
            // where they are written.
            if regions.is_empty()
                || self.recurses(cx, c)
                || !c
                    .effects
                    .iter()
                    .any(|e| database_domain(e, "read").is_some())
            {
                continue;
            }
            let Some(keyed) = self.keys.get(&c.callee) else {
                continue;
            };
            let Some(given) = self.given(cx, c.call) else {
                continue;
            };
            let at = cx.body.expr_span(c.call);
            let mut around: Vec<&Region> = regions
                .iter()
                .filter(|r| r.inside.start <= at.start && at.end <= r.inside.end)
                .collect();
            around.sort_by_key(|r| r.inside.end - r.inside.start);
            'read: for region in around {
                for (k, reached) in keyed {
                    let Some(Some(key)) = given.get(*k) else {
                        continue;
                    };
                    let wanted = |b: Binder| region.bindings.contains(&b);
                    if let Some(b) = self.depends(cx, *key, &wanted, None, &mut BTreeSet::new()) {
                        let found = self.read_per_element(cx, src, c, *k, region, *key, b, reached);
                        out.push(self.known(cx, c.callee, found));
                        break 'read;
                    }
                }
            }
        }
        // A function named as a callback, which reads by what it is given.
        for c in &cx.performed.calls {
            let Some(applied) = self.applies.get(&c.callee) else {
                continue;
            };
            let Some(given) = self.given(cx, c.call) else {
                continue;
            };
            for (param, list) in applied {
                let Some(Some(arg)) = given.get(*param) else {
                    continue;
                };
                let Some(named) = self.named(cx, *arg) else {
                    continue;
                };
                let Some((_, reached)) = self.keys.get(&named).and_then(|m| m.iter().next()) else {
                    continue;
                };
                let collection = list.and_then(|i| given.get(i).copied().flatten());
                let found = self.named_per_element(cx, src, c, *arg, named, reached, collection);
                out.push(self.known(cx, named, found));
            }
        }
    }

    /// **A read a reference program is known to make per element**
    /// (`KNOWN_READS_PER_ELEMENT`): reported as a warning, which says why it
    /// waits.
    fn known(&self, cx: &BodyCx<'_>, read: DefId, mut found: Diagnostic) -> Diagnostic {
        let (Some(at), Some(read)) = (self.sigs.path_of(cx.def), self.sigs.path_of(read)) else {
            return found;
        };
        if let Some((_, _, why)) = KNOWN_READS_PER_ELEMENT
            .iter()
            .find(|(a, r, _)| *a == at && *r == read)
        {
            found.severity = Severity::Warning;
            found.reason = "known_read_per_element";
            found.explanation = Some(format!(
                "{EXPLANATION} A read per element this program is known to make, a warning \
                 until it reads them together: {why}."
            ));
        }
        found
    }

    /// The declaration a function value names, where it names one: a path
    /// no binding in scope heads.
    fn named(&self, cx: &BodyCx<'_>, e: ExprId) -> Option<DefId> {
        let mut head = e;
        while let Expr::Field { base, .. } = cx.body.expr(head) {
            head = *base;
        }
        if !matches!(cx.body.expr(head), Expr::Name(_)) || cx.lexical.binder(head).is_some() {
            return None;
        }
        let path = crate::infer::path_of(cx.body, e);
        if path.is_empty() {
            return None;
        }
        let def = match self.sigs.by_path(&path) {
            Some(sig) => sig.definition,
            None => self.inference.resolved_from(cx.unit, &path)?,
        };
        (self.decl_of(def).kind == DeclKind::Fn).then_some(def)
    }

    /// The text of `span` in `src`, or nothing where it is not there.
    fn text(src: &str, span: &Span) -> String {
        src.get(span.clone()).unwrap_or_default().to_string()
    }

    /// What the read's key is named, `store` in `hours_of(store: StoreId)`.
    fn key_name(&self, reached: &Reached) -> String {
        self.decl_of(reached.read)
            .params
            .get(reached.param)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "key".to_string())
    }

    /// **A key as the set read's list would compute it**: a name a `let`
    /// binds, as its initializer.
    fn expanded(&self, cx: &BodyCx<'_>, src: &str, key: ExprId) -> String {
        let mut at = key;
        let mut seen = BTreeSet::new();
        while seen.insert(at) {
            let Expr::Name(_) = cx.body.expr(at) else {
                break;
            };
            let Some(Binder::Pattern(p)) = cx.lexical.binder(at) else {
                break;
            };
            let Some(Owner::Let(l)) = cx.owners.get(&p).copied() else {
                break;
            };
            let Expr::Let {
                pat: Some(bound),
                init: Some(init),
                ..
            } = cx.body.expr(l)
            else {
                break;
            };
            if *bound != p {
                break;
            }
            at = *init;
        }
        Self::text(src, &cx.body.expr_span(at))
    }

    /// Generated many reads answer flat rows and can take composite records.
    /// Their existing metadata supplies the pair and the key fields; a repair
    /// describes regrouping rather than replacing each read with the batch.
    fn generated_help(&self, read: DefId, place: &str) -> Option<String> {
        let hir = self.hirs[read.unit];
        let generated = hir.generated.get(&DeclId(read.decl))?;
        let crate::tables::Wrote::Function { by, .. } = &generated.what else {
            return None;
        };
        let set = *self.pairs.get(&read)?;
        let table = &hir.decl(generated.table).name;
        let many = self.decl_of(set);
        let fields = by
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let input = if by.len() == 1 {
            format!("scalar values for {fields}")
        } else {
            let record = many.params.first()?.ty.as_ref()?.args().first()?.written();
            format!("key records of `{table}.{record}` with fields {fields}")
        };
        Some(format!(
            "read them together, before the {place}, with `{table}.{}`: supply {input}; \
             then give each turn its matching rows from the flat result (or no row for a \
             missing key)",
            many.name
        ))
    }

    /// **The help a read per element is given**: the set read, before the
    /// loop, where the keyed read names one; where it names none, that it
    /// has none and its source can declare one.
    #[allow(clippy::too_many_arguments)]
    fn help(
        &self,
        cx: &BodyCx<'_>,
        src: &str,
        read: &str,
        at: &Span,
        direct: bool,
        region: &Region,
        key: ExprId,
        element: &str,
        reached: &Reached,
    ) -> String {
        let place = match region.turn {
            Turn::Recursion { .. } => "recursion",
            _ => "loop",
        };
        if let Some(help) = self.generated_help(reached.read, place) {
            return help;
        }
        let keyed = &self.decl_of(reached.read).name;
        let Some(set) = self.pairs.get(&reached.read) else {
            return format!("`{keyed}` has no set read; its source can declare one");
        };
        let set = &self.decl_of(*set).name;
        let collection = match (&region.turn, region.collection) {
            (Turn::Recursion { .. }, _) | (_, None) => None,
            (_, Some(c)) => Some(c),
        };
        let Some(collection) = collection.filter(|_| direct) else {
            let place = match region.turn {
                Turn::Recursion { .. } => "recursion",
                _ => "loop",
            };
            return format!(
                "read them together, before the {place}, with `{set}`, and give each turn what \
                 it answers"
            );
        };
        let qualified = match read.rsplit_once('.') {
            Some((module, _)) => format!("{module}.{set}"),
            None => set.clone(),
        };
        // What the set read answers is named for it: `hours` for
        // `hours_of_stores`, `prices` for `prices_of`.
        let var = [set, keyed]
            .iter()
            .find_map(|n| {
                n.split_once("_of")
                    .map(|(v, _)| v)
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or("found")
            .to_string();
        let list = Self::text(src, &cx.body.expr_span(collection));
        let key = self.expanded(cx, src, key);
        let mut help = format!(
            "read them together, before the loop:\n    let {var} = {qualified}(List.map({list}, \
             {element} => {key}))"
        );
        // The loop as written, the read's place taken by what the set read
        // answers, where it is one line.
        let whole = match region.turn {
            Turn::Callback { call, .. } => cx
                .piped
                .get(&call)
                .map(|lhs| cx.body.expr_span(*lhs).start..cx.body.expr_span(call).end)
                .unwrap_or_else(|| cx.body.expr_span(call)),
            _ => cx.body.expr_span(region.owner),
        };
        if whole.start <= at.start
            && at.end <= whole.end
            && let (Some(before), Some(after)) =
                (src.get(whole.start..at.start), src.get(at.end..whole.end))
        {
            let line = format!("{before}{var}{after}");
            if !line.contains('\n') {
                help.push_str(&format!("\n    {line}"));
            }
        }
        help
    }

    /// **PW0405, for a read written in the loop.**
    #[allow(clippy::too_many_arguments)]
    fn read_per_element(
        &self,
        cx: &BodyCx<'_>,
        src: &str,
        c: &Called,
        param: usize,
        region: &Region,
        key: ExprId,
        binding: Binder,
        reached: &Reached,
    ) -> Diagnostic {
        let body = cx.body;
        let read = match body.expr(c.call) {
            Expr::Call { callee, .. } => crate::infer::path_of(body, *callee),
            Expr::Field { .. } => crate::infer::path_of(body, c.call),
            _ => String::new(),
        };
        let bound = Self::bound_name(cx, binding);
        let collection = region
            .collection
            .map(|e| Self::text(src, &body.expr_span(e)));
        let over = match (&region.turn, &collection) {
            (Turn::Recursion { .. }, _) => format!(
                "once for each call of `{}`, which recurses over `{bound}`",
                cx.decl.name
            ),
            (_, Some(list)) => format!("once for each element of `{list}`"),
            (Turn::Callback { combinator, .. }, None) => {
                format!("once for each element `{combinator}` passes")
            }
            (Turn::For, None) => "once for each turn of its `for`".to_string(),
        };
        let key_text = Self::text(src, &body.expr_span(key));
        let from = match &region.turn {
            Turn::Callback { combinator, .. } => format!(
                "its key `{key_text}` comes from `{bound}`, the element `{combinator}` passes"
            ),
            Turn::For => {
                format!("its key `{key_text}` comes from `{bound}`, the element `for` binds")
            }
            Turn::Recursion { .. } => format!(
                "its key `{key_text}` comes from `{bound}`, a part of which `{}` passes to its \
                 recursive call",
                cx.decl.name
            ),
        };
        let mut related = vec![Related::new(body.expr_span(key), from)];
        if let Some(list) = region.collection {
            related.push(Related::new(
                body.expr_span(list),
                format!("one read per {}", self.key_name(reached)),
            ));
        }
        if let Turn::Recursion { at } = region.turn {
            related.push(Related::new(
                body.expr_span(at),
                format!("`{}` recurses here", cx.decl.name),
            ));
        }
        let direct = c.callee == reached.read;
        if !direct {
            related.push(Related::in_unit(
                c.callee.unit,
                self.decl_span(c.callee),
                format!(
                    "`{}` reads `{}` through `{}`, keyed by its parameter `{}`",
                    self.decl_of(c.callee).name,
                    reached.resource,
                    self.decl_of(reached.read).name,
                    self.decl_of(c.callee)
                        .params
                        .get(param)
                        .map_or("", |p| p.name.as_str())
                ),
            ));
        }
        Diagnostic {
            code: codes::READ_PER_ELEMENT.id,
            invariant: codes::READ_PER_ELEMENT.invariant,
            reason: "read_per_element",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!("`{read}` reads `{}` {over}", reached.resource),
            primary_span: body.expr_span(c.call),
            related,
            explanation: Some(EXPLANATION.to_string()),
            repairs: vec![Repair {
                description: self.help(
                    cx,
                    src,
                    &read,
                    &body.expr_span(c.call),
                    direct,
                    region,
                    key,
                    &bound,
                    reached,
                ),
                replacement: None,
            }],
        }
    }

    /// **PW0405, for a function named as a combinator's callback** that reads
    /// by what it is given.
    #[allow(clippy::too_many_arguments)]
    fn named_per_element(
        &self,
        cx: &BodyCx<'_>,
        src: &str,
        c: &Called,
        arg: ExprId,
        named: DefId,
        reached: &Reached,
        collection: Option<ExprId>,
    ) -> Diagnostic {
        let body = cx.body;
        let name = Self::text(src, &body.expr_span(arg));
        let combinator = match body.expr(c.call) {
            Expr::Call { callee, .. } => crate::infer::path_of(body, *callee),
            _ => String::new(),
        };
        let over = match collection {
            Some(list) => format!(
                "once for each element of `{}`",
                Self::text(src, &body.expr_span(list))
            ),
            None => format!("once for each element `{combinator}` passes"),
        };
        let keyed = &self.decl_of(reached.read).name;
        let mut related = vec![Related::in_unit(
            named.unit,
            self.decl_span(named),
            format!(
                "`{}` reads `{}` through `{keyed}`, keyed by what `{combinator}` passes it",
                self.decl_of(named).name,
                reached.resource
            ),
        )];
        if let Some(list) = collection {
            related.push(Related::new(
                body.expr_span(list),
                format!("one read per {}", self.key_name(reached)),
            ));
        }
        let help = self
            .generated_help(reached.read, "loop")
            .unwrap_or_else(|| match self.pairs.get(&reached.read) {
                Some(set) => format!(
                    "read them together, before the loop, with `{}`, and give each turn what it \
                 answers",
                    self.decl_of(*set).name
                ),
                None => format!("`{keyed}` has no set read; its source can declare one"),
            });
        Diagnostic {
            code: codes::READ_PER_ELEMENT.id,
            invariant: codes::READ_PER_ELEMENT.invariant,
            reason: "read_per_element",
            detector: Detector::DeclarationRule,
            severity: Severity::Error,
            message: format!("`{name}` reads `{}` {over}", reached.resource),
            primary_span: body.expr_span(arg),
            related,
            explanation: Some(EXPLANATION.to_string()),
            repairs: vec![Repair {
                description: help,
                replacement: None,
            }],
        }
    }

    // --- PW0406 --------------------------------------------------------------

    /// **Each set read on a source that cannot batch** (PW0406): a call per
    /// key, so the calls of what makes it grow with its data. A resource no
    /// source holds is the host's own database's, which gives every
    /// guarantee (ADR-0207): one call.
    fn per_key(&self, cx: &BodyCx<'_>, out: &mut Vec<Diagnostic>) {
        for c in &cx.performed.calls {
            let Some(keyed) = self.set_reads.get(&c.callee) else {
                continue;
            };
            let mut reads: Vec<&str> = c
                .effects
                .iter()
                .filter_map(|e| database_domain(e, "read"))
                .collect();
            reads.dedup();
            for resource in reads {
                let Some(source) = self
                    .sources
                    .iter()
                    .find(|s| s.holds.iter().any(|(h, _)| h == resource))
                else {
                    continue;
                };
                if Sets::of(source.sets.as_deref()) != Sets::PerKey {
                    continue;
                }
                let per = self
                    .decl_of(*keyed)
                    .params
                    .first()
                    .map_or_else(|| "key".to_string(), |p| p.name.clone());
                let stated = match &source.sets {
                    Some(written) => format!(
                        "`{}` states `sets {written}`: a set read is a call per key",
                        source.name
                    ),
                    None => format!(
                        "`{}` states no `sets`, so `per_key`: a set read is a call per key",
                        source.name
                    ),
                };
                out.push(Diagnostic {
                    code: codes::SET_READ_PER_KEY.id,
                    invariant: codes::SET_READ_PER_KEY.invariant,
                    reason: "set_read_per_key",
                    detector: Detector::DeclarationRule,
                    severity: Severity::Warning,
                    message: format!(
                        "`{}` reads `{resource}` once per {per} on `{}`, which cannot batch",
                        cx.decl.name, source.name
                    ),
                    primary_span: cx.body.expr_span(c.call),
                    related: vec![Related::in_unit(source.unit, source.span.clone(), stated)],
                    explanation: Some(
                        "A source states what one set read costs it (`sets`): one call whatever \
                         its keys (`one_call`), at least one call for each `k` keys \
                         (`chunked(k)`), or one call per key (`per_key`), which a source that \
                         states none is. A set read on a source that cannot batch makes a call \
                         per key, so the calls of the page or command that makes it grow with \
                         its data. A warning: which database holds the data is the \
                         deployment's choice, and the program is right as written."
                            .to_string(),
                    ),
                    repairs: vec![Repair {
                        description: format!(
                            "hold `{resource}` in a source that batches, or bound the list"
                        ),
                        replacement: None,
                    }],
                });
                break;
            }
        }
    }

    // --- the `set` pair --------------------------------------------------------

    /// **Each `set` clause, held to its shape** (PW0407, the pair's own rule
    /// since the integrator's ruling of 2026-10-15; it was PW0335's): it names
    /// a function of the same source, given a list of the keyed read's key,
    /// answering a list of what the keyed read answers, with its key.
    fn pair(&mut self) {
        let mut pairs = BTreeMap::new();
        let mut faults = Vec::new();
        for (unit, hir) in self.hirs.iter().enumerate() {
            // Generated pairs have the table declaration's shape, including
            // flat prefix/index results and composite arguments. Handwritten
            // pairs below still undergo the exact `set` shape check.
            use crate::tables::Primitive;
            for table in crate::tables::declared(hir) {
                let functions = crate::tables::functions_of(hir, table.decl);
                for (id, primitive, by) in &functions {
                    let many = match primitive {
                        Primitive::Get => Primitive::GetMany,
                        Primitive::Prefix => Primitive::PrefixMany,
                        Primitive::Index => Primitive::IndexMany,
                        Primitive::Count => Primitive::CountMany,
                        Primitive::First => Primitive::FirstMany,
                        Primitive::Last => Primitive::LastMany,
                        _ => continue,
                    };
                    if let Some((set, _, _)) = functions
                        .iter()
                        .find(|(_, p, fields)| *p == many && fields == by)
                    {
                        pairs.insert(Self::def(unit, *id), Self::def(unit, *set));
                    }
                }
            }
            for (id, decl) in hir.all_decls() {
                let Some(policy) = decl.policy("set") else {
                    continue;
                };
                if decl.kind != DeclKind::Fn {
                    // PW5105's: a clause on a declaration it is not for.
                    continue;
                }
                let def = Self::def(unit, id);
                let value = policy.value.trim();
                match self.fits(unit, def, value) {
                    Ok(set) => {
                        pairs.insert(def, set);
                    }
                    Err((why, at)) => faults.push((
                        unit,
                        Diagnostic {
                            code: codes::SET_PAIR_SHAPE.id,
                            invariant: codes::SET_PAIR_SHAPE.invariant,
                            reason: "set_pair_shape",
                            detector: Detector::DeclarationRule,
                            severity: Severity::Error,
                            message: format!("`set {value}`: {why}"),
                            primary_span: policy.span.clone(),
                            related: vec![match at {
                                Some(set) => Related::in_unit(
                                    set.unit,
                                    self.decl_span(set),
                                    format!("`{}` is declared here", self.decl_of(set).name),
                                ),
                                None => Related::new(
                                    hir.decl_span(id),
                                    format!("a policy of `{}`", decl.name),
                                ),
                            }],
                            explanation: Some(
                                "A keyed read names its set form with `set`: a function of the \
                                 same source, given a list of the keyed read's key, answering a \
                                 list of what the keyed read answers, each with its key. A loop's \
                                 reads are then one read before it, and its source's `sets` says \
                                 what that read costs. A `set` that names anything else pairs \
                                 the read with nothing."
                                    .to_string(),
                            ),
                            repairs: vec![Repair {
                                description: "name a function of the same source, given a list \
                                              of the key, answering a list of the result with \
                                              its key"
                                    .to_string(),
                                replacement: None,
                            }],
                        },
                    )),
                }
            }
        }
        self.set_reads = pairs.iter().map(|(k, s)| (*s, *k)).collect();
        self.pairs = pairs;
        self.faults = faults;
    }

    /// The resources a declaration reads, by its row: declared, for one with
    /// no body, and inferred through what it calls, for one with one.
    fn reads_of(&self, def: DefId) -> BTreeSet<String> {
        self.inference
            .effects_of_def(def)
            .into_iter()
            .flatten()
            .filter_map(|e| database_domain(e, "read"))
            .map(str::to_string)
            .collect()
    }

    /// The sources holding what a declaration reads, by name; the host's
    /// own database for what no source holds.
    fn sources_of(&self, reads: &BTreeSet<String>) -> BTreeSet<String> {
        reads
            .iter()
            .map(|r| {
                self.sources
                    .iter()
                    .find(|s| s.holds.iter().any(|(h, _)| h == r))
                    .map_or_else(
                        || "the host's database".to_string(),
                        |s| format!("`{}`", s.name),
                    )
            })
            .collect()
    }

    /// **Does `value`, the `set` of the keyed read `def`, name its set
    /// form?** The set read, or why not and where.
    fn fits(&self, unit: usize, def: DefId, value: &str) -> Result<DefId, (String, Option<DefId>)> {
        let keyed = self.decl_of(def);
        let resolution = if value.contains('.') {
            self.ws.resolve_path_in(unit, Namespace::Term, value)
        } else {
            self.ws.resolve_in(unit, Namespace::Term, value)
        };
        let (Resolution::Local(set) | Resolution::Imported { def: set, .. }) = resolution else {
            return Err((format!("`{value}` names no function visible here"), None));
        };
        let decl = self.decl_of(set);
        if decl.kind != DeclKind::Fn {
            return Err((format!("`{value}` is no function"), Some(set)));
        }
        if set == def {
            return Err((format!("`{value}` is `{}` itself", keyed.name), Some(set)));
        }
        let reads = self.reads_of(def);
        if reads.is_empty() {
            return Err((
                format!("`{}` reads no source, so it has no set read", keyed.name),
                None,
            ));
        }
        let theirs = self.reads_of(set);
        let (mine, other) = (self.sources_of(&reads), self.sources_of(&theirs));
        if theirs.is_empty() || mine != other {
            let listed = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
            return Err((
                if theirs.is_empty() {
                    format!("`{value}` reads no source, and a set read reads its keyed read's")
                } else {
                    format!(
                        "`{value}` reads from {}, and `{}` from {}: a set read is of its keyed \
                         read's source",
                        listed(&other),
                        keyed.name,
                        listed(&mine)
                    )
                },
                Some(set),
            ));
        }
        let (Some(mine), Some(theirs)) = (self.sigs.by_def(def), self.sigs.by_def(set)) else {
            return Err((format!("`{value}` has no signature to compare"), Some(set)));
        };
        let ty = |sig: &'p Signature, i: usize| -> Option<&'p ResolvedType> {
            sig.params.get(i)?.as_ref()?.resolved()
        };
        let Some(key) = ty(mine, 0) else {
            return Err((format!("`{}` takes no key", keyed.name), None));
        };
        let shown = |sig: &Signature| {
            sig.params
                .iter()
                .map(|p| {
                    p.as_ref()
                        .map_or_else(|| "_".to_string(), ToString::to_string)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let wanted = std::iter::once(format!("List<{}>", key.display_name()))
            .chain(mine.params.iter().skip(1).map(|p| {
                p.as_ref()
                    .map_or_else(|| "_".to_string(), ToString::to_string)
            }))
            .collect::<Vec<_>>()
            .join(", ");
        let listed_key = ty(theirs, 0)
            .filter(|t| t.as_builtin() == Some(Builtin::List))
            .and_then(|t| t.args().first())
            .is_some_and(|k| k.same_as(key));
        let rest = theirs.params.len() == mine.params.len()
            && (1..mine.params.len()).all(|i| match (ty(mine, i), ty(theirs, i)) {
                (Some(a), Some(b)) => a.same_as(b),
                _ => false,
            });
        if !listed_key || !rest {
            return Err((
                format!(
                    "`{value}` takes `({})`, and a set read of `{}` takes `({wanted})`",
                    shown(theirs),
                    keyed.name
                ),
                Some(set),
            ));
        }
        // What the keyed read answers when it finds what it reads.
        let Some(result) = mine.result() else {
            return Err((format!("`{}` answers nothing", keyed.name), None));
        };
        let found = match result.as_builtin() {
            Some(Builtin::Result | Builtin::Option) => result.args().first().unwrap_or(result),
            _ => result,
        };
        let answers = theirs
            .result()
            .filter(|t| t.as_builtin() == Some(Builtin::List))
            .and_then(|t| t.args().first());
        let holds = |record: &ResolvedType, t: &ResolvedType, field: Option<&str>| {
            record
                .def_id()
                .and_then(|d| self.sigs.type_decl(d))
                .and_then(|t| t.record.as_ref())
                .is_some_and(|fields| {
                    fields.iter().any(|(name, f)| {
                        field.is_none_or(|field| name.as_str() == field)
                            && f.resolved().is_some_and(|f| f.same_as(t))
                    })
                })
        };
        // ADR-0340's key-field ruling: an unrelated field of the key's
        // type does not identify the row. Payload fields need only carry the
        // result type; the key field also bears the key parameter's name.
        let key_field = keyed.params.first().map(|p| p.name.as_str());
        let fits = answers.is_some_and(|e| {
            // ADR-0348: keep the operation's exact Result/Option per named key,
            // as well as the existing successful-value projection.
            (e.same_as(found) && holds(e, key, key_field))
                || (holds(e, key, key_field) && (holds(e, found, None) || holds(e, result, None)))
        });
        if !fits {
            return Err((
                format!(
                    "`{value}` answers `{}`, and a set read of `{}` answers a list of `{}` with \
                     its `{}`",
                    theirs
                        .returns
                        .as_ref()
                        .map_or_else(|| "nothing".to_string(), TypeResolution::to_string),
                    keyed.name,
                    found.display_name(),
                    key.display_name()
                ),
                Some(set),
            ));
        }
        Ok(set)
    }
}

/// **The reads per element a reference program makes today** (4c's
/// finding, 2026-10-14), each reported as a warning until its program reads
/// them together: the declaration that reads, the read, and why it waits.
///
/// The design note's survey found no read per element in the store's or the
/// feed's programs, and none is there. PW0405, first run over every
/// reference program, found four in kiokun.com's (`examples/kiokun-site`),
/// each as kiokun.com makes it: one `?limit=` search per reading, and one
/// pitch file per word. Their set forms are its data layer's to add, which is
/// the integrator's to schedule (the track's question). The list shrinks as a
/// program states its set reads; `tests/bounded_reads.rs` holds each entry
/// to the reads it names, so a stale entry fails, and it grows only on the
/// integrator's ruling.
pub const KNOWN_READS_PER_ELEMENT: &[(&str, &str, &str)] = &[
    (
        "kiokun.site.Word",
        "kiokun.site.search_reading",
        "kiokun.com asks `search?reading=..&limit=` once for each reading; its layer has no set form of `search#reading`",
    ),
    (
        "kiokun.site.Word",
        "kiokun.site.read_pitch",
        "kiokun.com reads a word's pitch shard once for each word; its layer has no set form of `pitch#of`, whose key is two values",
    ),
    (
        "kiokun.site.Search",
        "kiokun.site.search_reading",
        "kiokun.com asks `search?reading=..&limit=` once for each reading; its layer has no set form of `search#reading`",
    ),
];

/// Why a read per element is refused, for every PW0405.
const EXPLANATION: &str = "A read in a loop over data runs once for each element, so the page \
     or command makes as many round trips as its list is long (N+1). A key that does not come \
     from the element is one key, which the host's cache answers again; a key that does is a \
     read per element. Read the list's keys together, in one set read before the loop, and \
     find each element's in what it answers.";

/// A pattern and every pattern inside it.
fn pattern_tree(body: &Body, p: PatternId) -> Vec<PatternId> {
    let mut out = Vec::new();
    let mut stack = vec![p];
    while let Some(q) = stack.pop() {
        out.push(q);
        match body.pat(q) {
            Pattern::Ctor { args, .. } => stack.extend(args.iter().copied()),
            Pattern::Or(alternatives) => stack.extend(alternatives.iter().copied()),
            _ => {}
        }
    }
    out
}

/// The bindings a pattern makes.
fn binds(body: &Body, p: PatternId) -> BTreeSet<Binder> {
    pattern_tree(body, p)
        .into_iter()
        .filter(|q| matches!(body.pat(*q), Pattern::Bind { .. }))
        .map(Binder::Pattern)
        .collect()
}

/// **The parameters a signature applies to a list's elements**: one of a
/// function type `fn(.., T, ..) -> U` beside one of `List<T>`, where `T` is
/// one type. By its type, such a function can give its callback no `T` but
/// the list's.
fn by_signature(sig: &Signature) -> BTreeMap<usize, Option<usize>> {
    let ty = |i: usize| -> Option<&ResolvedType> { sig.params.get(i)?.as_ref()?.resolved() };
    let mut out = BTreeMap::new();
    for j in 0..sig.params.len() {
        let Some(f) = ty(j).filter(|t| t.as_builtin() == Some(Builtin::Function)) else {
            continue;
        };
        let Some((_, takes)) = f.args().split_last() else {
            continue;
        };
        for i in (0..sig.params.len()).filter(|i| *i != j) {
            let Some(element) = ty(i)
                .filter(|t| t.as_builtin() == Some(Builtin::List))
                .and_then(|t| t.args().first())
            else {
                continue;
            };
            if takes.iter().any(|t| t.same_as(element)) {
                out.insert(j, Some(i));
                break;
            }
        }
    }
    out
}

/// **The call graph's strongly connected components** (Tarjan, 1972), each
/// declaration on a cycle with its component's number: one of several
/// declarations, or one that calls itself.
fn components(edges: &BTreeMap<DefId, BTreeSet<DefId>>) -> BTreeMap<DefId, usize> {
    struct Walk<'e> {
        edges: &'e BTreeMap<DefId, BTreeSet<DefId>>,
        index: BTreeMap<DefId, usize>,
        low: BTreeMap<DefId, usize>,
        stack: Vec<DefId>,
        on: BTreeSet<DefId>,
        next: usize,
        found: Vec<Vec<DefId>>,
    }
    impl Walk<'_> {
        fn visit(&mut self, v: DefId) {
            self.index.insert(v, self.next);
            self.low.insert(v, self.next);
            self.next += 1;
            self.stack.push(v);
            self.on.insert(v);
            let targets: Vec<DefId> = self
                .edges
                .get(&v)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            for w in targets {
                if !self.index.contains_key(&w) {
                    self.visit(w);
                    let lw = self.low[&w];
                    let lv = self.low.get_mut(&v).expect("visited");
                    *lv = (*lv).min(lw);
                } else if self.on.contains(&w) {
                    let iw = self.index[&w];
                    let lv = self.low.get_mut(&v).expect("visited");
                    *lv = (*lv).min(iw);
                }
            }
            if self.low[&v] == self.index[&v] {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on.remove(&w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                self.found.push(component);
            }
        }
    }
    let mut walk = Walk {
        edges,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on: BTreeSet::new(),
        next: 0,
        found: Vec::new(),
    };
    for v in edges.keys() {
        if !walk.index.contains_key(v) {
            walk.visit(*v);
        }
    }
    let mut out = BTreeMap::new();
    for (n, component) in walk.found.into_iter().enumerate() {
        let cyclic = component.len() > 1
            || component
                .first()
                .is_some_and(|v| edges.get(v).is_some_and(|s| s.contains(v)));
        if cyclic {
            for v in component {
                out.insert(v, n);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_component_is_a_cycle_or_a_call_of_itself() {
        let d = |n: u32| DefId { unit: 0, decl: n };
        let edges: BTreeMap<DefId, BTreeSet<DefId>> = [
            (d(0), [d(1)].into()),
            (d(1), [d(0)].into()),
            (d(2), [d(2)].into()),
            (d(3), [d(0)].into()),
        ]
        .into();
        let found = components(&edges);
        assert_eq!(found.get(&d(0)), found.get(&d(1)));
        assert!(found.contains_key(&d(2)));
        assert!(!found.contains_key(&d(3)), "a caller of a cycle is on none");
        assert_ne!(found.get(&d(0)), found.get(&d(2)));
    }
}
