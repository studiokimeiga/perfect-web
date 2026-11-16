//! E2D — source effect inference.
//!
//! Pulled forward out of E9 by architect ruling, 2026-08-06, because it is the
//! single highest-leverage missing capability: roughly fifteen rejected corpus
//! programs need it, and so does the headline P0 claim.
//!
//! # What it does
//!
//! Given a body, work out which effects it actually performs, and compare that
//! against the row it declares. `RQ-2` measured that Koka propagates effects
//! through unannotated higher-order code; this is `pw` owning the same property
//! over its own source, with its own spans.
//!
//! ```text
//! view StorePage(id: StoreId) !{} {      declared: {}
//!     let store = Stores.get(id)         Stores.get : !{ database.read }
//!     ...                                inferred: { database.read }
//! }                                      -> the row is not satisfied
//! ```
//!
//! # Where the effects come from
//!
//! [`crate::signatures::Signatures`], and nowhere else. There is no table of
//! function names here — E2C's deletion gate forbids it, and the reason is that
//! a table can describe a function that does not exist.
//!
//! # The three shapes that matter
//!
//! `docs/RISK_QUEUE.md` RQ-2 names them, and the corpus tests each:
//!
//! - **direct** — the body calls the effectful thing itself;
//! - **helper-hidden** — the body calls a local helper that does;
//! - **callback-hidden** — the body passes a lambda to a generic function, and
//!   the effect is inside the lambda.
//!
//! The third is the one an approximation gets wrong, which is why the body
//! grammar parses lambdas properly rather than skipping them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::hir::{Body, Decl, Expr, ExprId, Hir, Span};
use crate::ontology::Facet;
use crate::resolve::{DefId, Resolution, Workspace};
use crate::signatures::Signatures;

/// An effect, as written in a row: `database.read`, `layout.measure`.
pub type Effect = String;

/// Why a body has an effect. The material for a diagnostic that explains the
/// chain rather than announcing a mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub effect: Effect,
    /// The call that introduced it.
    pub span: Span,
    /// How it got here.
    pub via: Via,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// The body calls it directly.
    Direct { callee: String },
    /// Through a helper declared in the same program.
    Helper { helper: String, callee: String },
    /// Inside a lambda handed to another function.
    Callback { passed_to: String, callee: String },
    /// Named as a value, `List.map(xs, stamp)`: it may be called wherever
    /// the value goes (ADR-0078).
    Value { callee: String },
}

impl Via {
    /// The thing that actually performs the effect, whatever route it took to
    /// get here. A diagnostic about an escape hatch has to name the hatch.
    pub fn callee(&self) -> &str {
        match self {
            Via::Direct { callee }
            | Via::Helper { callee, .. }
            | Via::Callback { callee, .. }
            | Via::Value { callee } => callee,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Via::Direct { callee } => format!("`{callee}` performs it"),
            // A helper that IS the callee is a direct call that arrived by the
            // helper route. "`raw_html` calls `raw_html`" reads as a bug in the
            // compiler rather than a fact about the program.
            Via::Helper { helper, callee } if helper == callee => {
                format!("`{callee}` performs it")
            }
            Via::Helper { helper, callee } => {
                format!("`{helper}` calls `{callee}`, which performs it")
            }
            Via::Callback { passed_to, callee } => {
                format!("the callback passed to `{passed_to}` calls `{callee}`")
            }
            Via::Value { callee } => format!(
                "`{callee}` is named as a value here, and performs it wherever the value is called"
            ),
        }
    }
}

/// What a body was found to do.
#[derive(Debug, Clone, Default)]
pub struct Inferred {
    pub effects: BTreeSet<Effect>,
    pub sources: Vec<Source>,
    /// Which resolved facts produced these effects — ADR-0022.
    ///
    /// Recorded, not recomputed: every entry is a value this walk already
    /// concluded. `docs/RISK_QUEUE.md`'s coincidental-correctness section is
    /// what it is for — `R-037` produced exactly the right effect set through
    /// a mechanism unrelated to what its fixture claimed, and no assertion on
    /// `effects` or `sources` could see that.
    pub evidence: crate::provenance::Evidence,
    /// **Each call the walk resolved to a declaration, and what it charged
    /// the call** (4c, the bounded reads): the call graph rows are inferred
    /// over, kept where the walk resolves each callee rather than resolved a
    /// second time (ADR-0022). A pure call is here too, charged nothing, and a
    /// call through a local is not: it calls a value (ADR-0063, ADR-0078).
    pub calls: Vec<Called>,
}

/// **One call, as the walk resolved it** (4c): the call, the declaration it
/// calls, and the effects it charged the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Called {
    pub call: ExprId,
    pub callee: DefId,
    pub effects: Vec<Effect>,
}

/// **A lambda written as a call's argument**: the call, the argument's
/// position as written, and the callee as written. What the walk reads to say
/// which function a callback was handed to, and 4c's loops to say which
/// callbacks a combinator applies to a list's elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handed {
    pub call: ExprId,
    pub arg: usize,
    pub to: String,
}

/// **Every lambda written as a call's argument** under `root`, by the lambda.
pub fn callbacks(body: &Body, root: ExprId) -> BTreeMap<ExprId, Handed> {
    let mut out = BTreeMap::new();
    for id in body.walk_from(root) {
        if let Expr::Call { callee, args } = body.expr(id) {
            let to = path_of(body, *callee);
            for (arg, a) in args.iter().enumerate() {
                if matches!(body.expr(a.value), Expr::Lambda { .. }) {
                    out.insert(
                        a.value,
                        Handed {
                            call: id,
                            arg,
                            to: to.clone(),
                        },
                    );
                }
            }
        }
    }
    out
}

impl Inferred {
    /// Effects performed but not declared. Empty means the row is honest.
    ///
    /// An **open row** — one the author did not write at all — is not checked:
    /// `None` means "not stated", and inferring a row for a declaration that
    /// never claimed one would reject working programs.
    pub fn undeclared(&self, declared: Option<&[String]>) -> Vec<&Source> {
        let Some(declared) = declared else {
            return Vec::new();
        };
        let declared: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        self.sources
            .iter()
            .filter(|s| !covered(&declared, &s.effect))
            .filter(|s| seen.insert(s.effect.clone()))
            .collect()
    }
}

/// Does a declared row cover an inferred effect?
///
/// `database` covers `database.read`: declaring a family accepts its members.
/// The reverse is not true — declaring `database.read` does not permit
/// `database.write`, which is what makes a query that writes detectable.
pub fn row_covers(declared: &[String], effect: &str) -> bool {
    let set: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
    covered(&set, effect)
}

fn covered(declared: &BTreeSet<&str>, effect: &str) -> bool {
    if declared.contains(effect) {
        return true;
    }
    let want = family_of(effect);
    declared
        .iter()
        .any(|d| family_of(d) == want && is_broader(d, effect))
}

/// An effect's family: `style.mutate<LayoutAffect>` and `style.mutate` are both
/// `style`.
///
/// Type arguments are stripped **before** splitting on `.`, because
/// `secret<Payments>` has no dot at all — comparing families without stripping
/// made a row declaring `secret<Payments>` fail to cover `secret.read`, and two
/// corpus fixtures were reported for an effect they had declared.
pub fn family_of(effect: &str) -> &str {
    let base = effect.split('<').next().unwrap_or(effect);
    base.split('.').next().unwrap_or(base)
}

/// Is `declared` at least as permissive as `effect`?
///
/// A family covers its members: declaring `database` accepts `database.read`.
/// A member does not cover a sibling — declaring `database.read` must not
/// permit `database.write`, which is what makes a query that writes detectable.
fn is_broader(declared: &str, effect: &str) -> bool {
    let d = declared.split('<').next().unwrap_or(declared);
    let e = effect.split('<').next().unwrap_or(effect);
    d == e || !d.contains('.')
}

/// Infer the effects of every declaration in a program.
///
/// Bodies are revisited when a row they read grows, including declarations
/// named as values and non-invoked members. Propagation reaches a fixed point;
/// the set of source effects is finite.
pub struct Inference<'a> {
    sigs: &'a Signatures,
    /// Declaration name → the effects its body performs.
    /// Inferred facts, keyed by RESOLVED IDENTITY.
    ///
    /// `docs/RISK_QUEUE.md` 34: this was keyed by the bare declaration name, so
    /// `Resources.Cart` and `store.page.Cart` shared one entry and one of them
    /// silently received facts belonging to the other. A query whose body is
    /// `todo` was reported as requiring `database.read`, which is exactly what
    /// a cart query *should* require — the wrongness was invisible in the
    /// output because the borrowed fact was plausible for its new owner.
    ///
    /// Architect ruling, 2026-08-07:
    ///
    /// > It should be `DefId → inferred facts` […] never `"Cart" → inferred
    /// > facts`. This is not "implementing the permanent E9 type system early".
    /// > It is repairing the current compiler so every existing analysis
    /// > consumes the name resolution machinery E2B already established.
    known: BTreeMap<DefId, BTreeSet<Effect>>,
    // NOTE: `known` keyed by a `String` would be RISK_QUEUE 34 again. See
    // `name-keyed-allow.txt` and `tests/name_keyed_maps.rs`.
    /// **What each declaration's own body performs** (ADR-0283), as its row
    /// is checked against: its calls, the members it reads and the
    /// declarations it names as values (ADR-0078), from `run`'s last pass.
    /// Its declared row is not in it: a row permits, and does not perform.
    performed: BTreeMap<DefId, Inferred>,
    /// **Each body's names, as its scopes bind them**, by its unit and its
    /// owner, from `run`: a call through a local is that local's, never a
    /// declaration of its spelling's (ADR-0063; ADR-0283's limitation).
    lexicals: BTreeMap<(usize, crate::hir::DeclId), crate::lexical::Lexical>,
    /// **Each body's types, as `run` built them**, by its unit and its owner
    /// (ADR-0351, the generic-effects track): so a walk over one of its roots,
    /// a handler, a painter's `draw`, a resource's `acquire`, an optimistic
    /// transition, reads the members it reads by their receivers' types, as
    /// the walk over the body does. Until then such a walk read calls alone,
    /// and `el.offsetWidth` in a `draw` performed nothing.
    types: BTreeMap<(usize, crate::hir::DeclId), crate::infer::Types<'a>>,
    /// Which unit each `Hir` in the last `run` was, so a declaration can be
    /// given the same `DefId` the workspace gave it.
    workspace: &'a Workspace,
}

impl<'a> Inference<'a> {
    pub fn new(sigs: &'a Signatures, workspace: &'a Workspace) -> Self {
        Self {
            sigs,
            known: BTreeMap::new(),
            performed: BTreeMap::new(),
            lexicals: BTreeMap::new(),
            types: BTreeMap::new(),
            workspace,
        }
    }

    /// The identity of a declaration, as the workspace assigns it.
    ///
    /// Built the same way `Signatures::build` builds its `by_def` key, because
    /// two constructions of one identity is how this defect gets reintroduced.
    fn def_of(&self, unit: usize, id: crate::hir::DeclId) -> DefId {
        DefId { unit, decl: id.0 }
    }

    /// Seed from every declared row in the program, then iterate to a fixed
    /// point over local helpers.
    pub fn run(&mut self, hirs: &[&Hir]) {
        self.run_observing(hirs, || {});
    }

    /// Keep visit-count witnesses on the same inference path without storing
    /// instrumentation in the inference state or changing its public API.
    fn run_observing(&mut self, hirs: &[&Hir], mut visit: impl FnMut()) {
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, d) in hir.all_decls() {
                if let Some(row) = &d.declared_effects {
                    self.known.insert(
                        self.def_of(unit, id),
                        row.iter().map(|e| e.written.clone()).collect(),
                    );
                }
            }
        }

        // A helper that does not declare a row still has effects. Propagate
        // until nothing changes — the effect set is finite, so this terminates.
        //
        // Everything a body performs, as a declaration's own row is checked
        // against: its calls, the members it reads, and the declarations it
        // names as values (ADR-0078). Until 2026-09-26 this counted calls
        // alone, so a helper that declares no row and reads `el.offsetWidth`,
        // or calls `clock.now` through a local, gave a view that called it
        // no effect.
        let sigs = self.sigs;
        // Every arena a declaration's terms are in: its body's, and an
        // interface's own (ADR-0240), whose roots are walked too. Only an
        // implemented body is inferred into `known`: a declaration with no
        // body performs what its row promises.
        let typed: Vec<(
            usize,
            crate::hir::DeclId,
            &Body,
            bool,
            crate::infer::Types<'a>,
        )> = hirs
            .iter()
            .enumerate()
            .flat_map(|(unit, hir)| {
                hir.all_decls().filter_map(move |(id, d)| {
                    let body = hir.body(d.terms_body()?);
                    Some((
                        unit,
                        id,
                        body,
                        d.body.is_some(),
                        crate::infer::Types::of_decl(sigs, hir, id, body),
                    ))
                })
            })
            .collect();
        for (unit, id, _, _, types) in &typed {
            self.lexicals.insert((*unit, *id), types.lexical().clone());
        }
        // Observe dependencies at the canonical row reads, including empty
        // and absent rows. The call graph alone omits named function values
        // and members that are read without being invoked. Register readers
        // before publishing growth so a recursive body can enqueue itself.
        let mut readers: BTreeMap<DefId, BTreeSet<usize>> = BTreeMap::new();
        let mut pending: VecDeque<usize> = typed
            .iter()
            .enumerate()
            .filter_map(|(index, (_, _, _, implemented, _))| implemented.then_some(index))
            .collect();
        let mut queued: Vec<bool> = typed
            .iter()
            .map(|(_, _, _, implemented, _)| *implemented)
            .collect();
        while let Some(index) = pending.pop_front() {
            queued[index] = false;
            visit();
            let (unit, id, body, _, types) = &typed[index];
            let mut reads = BTreeSet::new();
            let found = self.infer_in_reading(*unit, body, types, Some(&mut reads));
            for read in reads {
                readers.entry(read).or_default().insert(index);
            }
            let def = self.def_of(*unit, *id);
            let entry = self.known.entry(def).or_default();
            let before = entry.len();
            entry.extend(found.effects.iter().cloned());
            if entry.len() != before {
                for &reader in readers.get(&def).into_iter().flatten() {
                    if !queued[reader] {
                        queued[reader] = true;
                        pending.push_back(reader);
                    }
                }
            }
            // A declared row may already contain an arriving effect. Its
            // body's sources, calls and provenance still need this refresh.
            self.performed.insert(def, found);
        }
        // Rows only grow by union with finite source/signature effects. Only
        // row growth schedules readers, so cycles converge without a cap.
        for (unit, id, _, _, types) in typed {
            self.types.insert((unit, id), types);
        }
    }

    /// One canonical declared-plus-inferred row for a resolved member:
    /// effect charging and invocation accounting must agree (ADR-0351).
    fn member_performs(
        &self,
        sig: &crate::signatures::Signature,
        reads: Option<&mut BTreeSet<DefId>>,
    ) -> BTreeSet<Effect> {
        if let Some(reads) = reads {
            reads.insert(sig.definition);
        }
        sig.effects
            .iter()
            .cloned()
            .chain(
                self.known
                    .get(&sig.definition)
                    .into_iter()
                    .flatten()
                    .cloned(),
            )
            .collect()
    }

    /// What one body performs, given the types its declaration makes known.
    pub fn infer_in(&self, body: &Body, types: &crate::infer::Types<'_>) -> Inferred {
        self.infer_in_at(usize::MAX, body, types)
    }

    /// `infer_in`, resolving unqualified calls within `unit`.
    pub fn infer_in_at(
        &self,
        unit: usize,
        body: &Body,
        types: &crate::infer::Types<'_>,
    ) -> Inferred {
        self.infer_in_reading(unit, body, types, None)
    }

    fn infer_in_reading(
        &self,
        unit: usize,
        body: &Body,
        types: &crate::infer::Types<'_>,
        mut reads: Option<&mut BTreeSet<DefId>>,
    ) -> Inferred {
        let mut out =
            self.calls_with_types(unit, body, body.root, Some(types), reads.as_deref_mut());
        self.member_effects(body, body.root, types, &mut out, reads.as_deref_mut());
        self.value_effects(unit, body, body.root, types, &mut out, reads);
        out
    }

    /// **A declaration named as a value** (ADR-0078): `List.map(xs, stamp)`,
    /// `let f = clock.now`, `Box { f: stamp }`. What it names may be called
    /// wherever the value goes, so its effects are performed here, as a
    /// lambda's are where it is written. Until 2026-09-26 only a call
    /// counted, and an effect travelled through a function value unseen: a
    /// view declared `!{}` read the clock through `List.map(xs, stamp)`,
    /// which R-037's invariant forbids, and passed.
    fn value_effects(
        &self,
        unit: usize,
        body: &Body,
        root: ExprId,
        types: &crate::infer::Types<'_>,
        out: &mut Inferred,
        mut reads: Option<&mut BTreeSet<DefId>>,
    ) {
        // What a handler of the body's markup holds: a predicate named there
        // is no page's ask (`holds::asked`).
        let handlers = crate::holds::handler_exprs(body);
        // A callee is the call walk's, and a path's head is not a value.
        let mut not_values: BTreeSet<ExprId> = BTreeSet::new();
        for id in body.walk_from(root) {
            match body.expr(id) {
                Expr::Call { callee, .. } => {
                    not_values.insert(*callee);
                }
                Expr::Field { base, .. } => {
                    // Only a module/type path has a non-value prefix. A
                    // function used as a receiver, stamp.apply, is still
                    // named as a value and carries stamp's effects (ADR-0078).
                    if matches!(
                        types.member_read(body, id),
                        Some(crate::infer::MemberRead::Path)
                    ) {
                        not_values.insert(*base);
                    }
                }
                _ => {}
            }
        }
        for id in body.walk_from(root) {
            if not_values.contains(&id) {
                continue;
            }
            let path = match body.expr(id) {
                // A binding in scope is its value, not a declaration's.
                Expr::Name(_) if types.lexical().binder(id).is_some() => continue,
                Expr::Name(n) => n.clone(),
                // A field of a value in scope is the value's, which
                // `member_effects` reads by its type. Only a module's path
                // names a declaration. Until ADR-0228 its last segment was
                // matched against the declarations in scope, so the feed's
                // `i.author`, a field of an `Item`, was charged `fn author`'s
                // `database.read<User>`.
                Expr::Field { .. } => {
                    let mut head = id;
                    while let Expr::Field { base, .. } = body.expr(head) {
                        head = *base;
                    }
                    if matches!(body.expr(head), Expr::Name(_))
                        && types.lexical().binder(head).is_some()
                    {
                        continue;
                    }
                    // A field of a value a call or any expression computes,
                    // `same(x).label`, is that value's member: `member_effects`
                    // reads it by its type (ADR-0351). Its path here was the
                    // bare `label`, resolved by its SPELLING to whatever
                    // declaration in scope bears it.
                    if !matches!(body.expr(head), Expr::Name(_)) {
                        continue;
                    }
                    path_of(body, id)
                }
                _ => continue,
            };
            if path.is_empty() {
                continue;
            }
            // **A predicate a page asks is asked, never called where the
            // value goes**: a page's `{#if P}` asks the deployment's `P`
            // (ADR-0307); a body the program gives `P` reads for itself.
            // Until 2026-11-16 a page asking one whose body reads a table was
            // charged that read (PW0401) beside PW6925, which says what is
            // wrong: one defect told twice (ADR-0395's "Not claimed"). Only
            // what `holds::asked` counts as asked is passed over (a bare
            // name, outside a handler, of a predicate with no parameters),
            // so a predicate PW6925 tells nothing of is charged as before.
            if matches!(body.expr(id), Expr::Name(_))
                && !handlers.contains(&id)
                && self.asks_a_predicate(unit, &path)
            {
                continue;
            }
            let effects: Vec<Effect> = match self.sigs.by_path(&path) {
                Some(sig) => sig.effects.clone(),
                None => {
                    let def = self.resolved(unit, &path);
                    if let (Some(def), Some(reads)) = (def, reads.as_deref_mut()) {
                        reads.insert(def);
                    }
                    def.and_then(|def| self.known.get(&def))
                        .map(|s| s.iter().cloned().collect())
                        .unwrap_or_default()
                }
            };
            for e in effects {
                out.effects.insert(e.clone());
                out.sources.push(Source {
                    effect: e,
                    span: body.expr_span(id),
                    via: Via::Value {
                        callee: path.clone(),
                    },
                });
            }
        }
    }

    /// Effects reached through a member: `anchor.offsetWidth`,
    /// `el.getBoundingClientRect()`.
    ///
    /// Charter §7.5A bans reading geometry directly from ordinary code. That ban
    /// lives in the accessors' declared rows (`packages/pw-platform-web/browser.pw`),
    /// not in a list of property names here — which is what E2C's deletion gate
    /// requires and what lets a new accessor be added without touching a checker.
    fn member_effects(
        &self,
        body: &Body,
        root: ExprId,
        types: &crate::infer::Types<'_>,
        out: &mut Inferred,
        mut reads: Option<&mut BTreeSet<DefId>>,
    ) {
        // Which lambdas were handed to which function, so a member call inside
        // a callback names the callback rather than only itself.
        //
        // The same map the call walk builds, for the same reason: R-037's
        // claim is that `layout.measure` propagates through `List.map`'s
        // callback, and a diagnostic that named only
        // `el.getBoundingClientRect` would be true and would not say that.
        let inside_callback = callbacks(body, root);

        for id in body.walk_from(root) {
            // Both shapes: a property read, and a method call on a value.
            let (field, span) = match body.expr(id) {
                Expr::Field { .. } => (id, body.expr_span(id)),
                Expr::Call { callee, .. } if matches!(body.expr(*callee), Expr::Field { .. }) => {
                    (*callee, body.expr_span(id))
                }
                _ => continue,
            };
            let Expr::Field { base, name: member } = body.expr(field) else {
                continue;
            };
            let (base, member) = (*base, member.clone());

            // By the receiver's TYPE, or not at all. The by-name fallback that
            // used to sit here is gone: it made whether an effect was seen
            // depend on no other type declaring a member with the same
            // spelling, and it had already borrowed `Carts.add`'s row for
            // `Money.add` once, reporting a pure calculation as writing to the
            // database.
            //
            // **Whatever the receiver is** (ADR-0351, the generic-effects
            // track): a name, a field's value, a call's result, read as
            // [`crate::infer::Types::member_read`] reads it. Until then only a
            // bare name whose type `infer.rs` stated was read, so
            // `o.item.label`, `List.reverse(xs)`'s elements' `.label` and a
            // loop's binder's performed nothing here, unseen. A module's path,
            // `Stores.get`, is the call walk's; a member a type lacks, or a
            // value of a type parameter, is refused by the value relations
            // (PW0610); and a value whose type nothing states is refused by
            // the effect rows (PW0408). `el.getBoundingClientRect().width`
            // reads a field of the call's result, which performs nothing.
            let Some(crate::infer::MemberRead::Member(Some(sig))) = types.member_read(body, field)
            else {
                continue;
            };
            let receiver = path_of(body, base);
            // The fact `R-037` claimed and did not have: this member was
            // found because the RECEIVER'S TYPE declares it.
            let receiver_type = types
                .of(body, base)
                .or_else(|| sig.params.first()?.as_ref()?.resolved().cloned())
                .map(|t| t.display_name())
                .unwrap_or_default();
            let resolved = out.evidence.record(
                crate::provenance::FactKind::ResolvedMember {
                    receiver_type,
                    member: member.clone(),
                    via: crate::provenance::Route::ReceiverType,
                },
                span.clone(),
                vec![],
            );

            let callee = match receiver.is_empty() {
                true => member.clone(),
                false => format!("{receiver}.{member}"),
            };
            let via = match enclosing_callback(body, id, &inside_callback) {
                Some(passed_to) => Via::Callback {
                    passed_to,
                    callee: callee.clone(),
                },
                None => Via::Direct {
                    callee: callee.clone(),
                },
            };
            // **What the member performs** (ADR-0351): its declared row, and
            // what its body performs, as a call to it is charged. Until then
            // its row alone, so a member that declares none, read as a
            // property, performed nothing whatever its body did.
            let performs = self.member_performs(sig, reads.as_deref_mut());
            for e in &performs {
                // Through the callback, if it is inside one — and depending on
                // the resolution, so the chain is walkable rather than three
                // facts that happen to be present.
                let mut cause = resolved;
                if let Via::Callback { passed_to, .. } = &via {
                    cause = out.evidence.record(
                        crate::provenance::FactKind::ThroughCallback {
                            passed_to: passed_to.clone(),
                        },
                        span.clone(),
                        vec![resolved],
                    );
                }
                out.evidence.record(
                    crate::provenance::FactKind::Effect { effect: e.clone() },
                    span.clone(),
                    vec![cause],
                );

                out.effects.insert(e.clone());
                out.sources.push(Source {
                    effect: e.clone(),
                    span: span.clone(),
                    via: via.clone(),
                });
            }
        }
    }

    /// What one body performs, with a reason for each effect.
    /// What one body performs, EXCLUDING the given subtrees.
    ///
    /// E8-0 needs this. A page that renders `on:press={.. => add_to_cart(..)}`
    /// contains a call to a command that writes the database — but the page
    /// does not perform that write. The handler does, when someone presses the
    /// button, and E7-L already makes the handler a separately loaded unit
    /// with its own identity.
    ///
    /// Without the exclusion the page's contract requires `database.write`,
    /// and a host granting it would give the render path authority it never
    /// uses. That is the exact over-granting the capability model exists to
    /// prevent, arriving through the front door.
    pub fn infer_excluding(&self, unit: usize, body: &Body, exclude: &[ExprId]) -> Inferred {
        subtracted(self.infer_at(unit, body), body, exclude)
    }

    /// **What a declaration's own body performs** (ADR-0283): `run`'s
    /// finding, or, for a body `run` did not see, the same walk now.
    fn performed(&self, unit: usize, hir: &Hir, id: crate::hir::DeclId, body: &Body) -> Inferred {
        match self.performed.get(&self.def_of(unit, id)) {
            Some(found) => found.clone(),
            None => {
                let types = crate::infer::Types::of_decl(self.sigs, hir, id, body);
                self.infer_in_at(unit, body, &types)
            }
        }
    }

    /// `infer`, without a unit to resolve names in.
    ///
    /// Kept for callers that have a body and no workspace position. It can
    /// still resolve a call through `Signatures::by_path`, which is a
    /// fully-qualified lookup; what it cannot do is find a SIBLING's inferred
    /// row, because "which sibling" is a question only a unit can answer.
    pub fn infer(&self, body: &Body) -> Inferred {
        self.infer_at(usize::MAX, body)
    }

    /// What one body performs, resolving unqualified calls within `unit`.
    pub fn infer_at(&self, unit: usize, body: &Body) -> Inferred {
        self.infer_rooted(unit, body, body.root)
    }

    /// The same inference, over one subtree.
    ///
    /// For an embedded transition. `optimistic Cart() as cart => ..` is a
    /// separate execution root (ADR-0025) with its own effect row, checked
    /// against its own context's restrictions — so it needs to be inferred
    /// independently of the declaration's body rather than as part of it.
    ///
    /// **Everything the subtree performs** (ADR-0351, the generic-effects
    /// track): its calls, and the members it reads and the declarations it
    /// names as values, by its body's types as `run` built them. Until then
    /// its calls alone: a member read in a handler, a painter's `draw` or an
    /// optimistic transition performed nothing here. A body `run` did not see
    /// is read by its calls alone, as before.
    pub fn infer_rooted(&self, unit: usize, body: &Body, root: ExprId) -> Inferred {
        let mut out = self.calls_from(unit, body, root);
        if let Some(types) = self.types.get(&(unit, body.owner)) {
            self.member_effects(body, root, types, &mut out, None);
            self.value_effects(unit, body, root, types, &mut out, None);
        }
        out
    }

    /// **The calls a subtree makes**, each charged its callee's row: the
    /// call walk, which [`Inference::infer_rooted`] and
    /// [`Inference::infer_in_at`] each complete with the members and the
    /// values the subtree reads.
    fn calls_from(&self, unit: usize, body: &Body, root: ExprId) -> Inferred {
        self.calls_with_types(unit, body, root, self.types.get(&(unit, body.owner)), None)
    }

    fn calls_with_types(
        &self,
        unit: usize,
        body: &Body,
        root: ExprId,
        types: Option<&crate::infer::Types<'_>>,
        mut reads: Option<&mut BTreeSet<DefId>>,
    ) -> Inferred {
        let mut out = Inferred::default();
        // A member Call evaluates its callee Field once. That Field is not
        // also a receiver-only property invocation (ADR-0351).
        let callees: BTreeSet<ExprId> = body
            .walk_from(root)
            .into_iter()
            .filter_map(|id| match body.expr(id) {
                Expr::Call { callee, .. } => Some(*callee),
                _ => None,
            })
            .collect();
        // Lambdas are visited through their enclosing call, so the reason can
        // say *which* function the callback was handed to.
        let inside_callback = callbacks(body, root);

        for id in body.walk_from(root) {
            // **A frame phase says WHEN work runs, not WHAT it does.**
            //
            // There was a synthesis here: `measure { .. }` contributed
            // `layout.measure` and `mutate { .. }` contributed `style.mutate`,
            // whatever the block contained. Architect ruling, 2026-08-07:
            //
            // > A frame-phase block says when this body's work executes. An
            // > effect says what that work actually does. Those are
            // > independent.
            //
            // So `mutate { pure_computation() }` no longer claims it mutated
            // style. The phase still constrains placement and still decides
            // what is legal inside it — `contexts::elsewhere` and
            // `forbidden_in_phase` — but those read the EXECUTION CONTEXT and
            // the body's real effects, which they already did.
            //
            // **A schedule performs `clock.schedule`** (the scheduled-commands
            // track): a statement, not a call, charged where it is written, as
            // a call to the host is, so a command that schedules declares it.
            if let Expr::Keyword { keyword, .. } = body.expr(id)
                && matches!(keyword.as_str(), "schedule" | "unschedule")
            {
                let effect = crate::signatures::CLOCK_SCHEDULE.to_string();
                out.effects.insert(effect.clone());
                out.sources.push(Source {
                    effect,
                    span: body.expr_span(id),
                    via: Via::Direct {
                        callee: keyword.clone(),
                    },
                });
                continue;
            }
            // The same typed member decision used by values, effects and
            // labels owns the invocation identity. A local receiver must not
            // borrow a module path with the same spelling.
            let field = match body.expr(id) {
                Expr::Call { callee, .. } if matches!(body.expr(*callee), Expr::Field { .. }) => {
                    Some(*callee)
                }
                Expr::Field { .. } if !callees.contains(&id) => Some(id),
                _ => None,
            };
            if let (Some(types), Some(field)) = (types, field) {
                match types.member_read(body, field) {
                    Some(crate::infer::MemberRead::Member(Some(sig))) => {
                        let invokes = match body.expr(id) {
                            Expr::Call { .. } => self.sigs.by_def(sig.definition).is_some(),
                            _ => self.sigs.property_receiver(sig).is_some(),
                        };
                        if invokes {
                            let effects = self.member_performs(sig, reads.as_deref_mut());
                            out.calls.push(Called {
                                call: id,
                                callee: sig.definition,
                                effects: effects.into_iter().collect(),
                            });
                        }
                        // member_effects charges this member's row. Structural
                        // fields and function values do not become invocations.
                        continue;
                    }
                    Some(
                        crate::infer::MemberRead::Member(None)
                        | crate::infer::MemberRead::Unknown
                        | crate::infer::MemberRead::Never
                        | crate::infer::MemberRead::Parameter,
                    ) => continue,
                    Some(crate::infer::MemberRead::Path) | None => {}
                }
            }
            let Expr::Call { callee, .. } = body.expr(id) else {
                continue;
            };
            let path = path_of(body, *callee);
            if path.is_empty() {
                continue;
            }
            // **A call through a local is that local's** (ADR-0063): `found()`
            // where a `let`, a parameter or a pattern binds `found` calls the
            // value, never a `fn found` the module declares, and what the
            // value names was charged where it was named (ADR-0078). Until
            // then a local was charged its namesake's row (ADR-0283).
            if matches!(body.expr(*callee), Expr::Name(_)) && self.local(unit, body, *callee) {
                continue;
            }

            // A signature first — that is E2C's single source of truth — then a
            // local declaration's own inferred effects.
            //
            // Resolved, not spelled. `Cart` in one module and `Cart` in another
            // are two declarations, and matching on the last segment of a path
            // gave one of them the other's effects — `RISK_QUEUE` 34.
            let (called, effects): (Option<DefId>, Vec<Effect>) = match self.sigs.by_path(&path) {
                Some(sig) => (Some(sig.definition), sig.effects.clone()),
                None => {
                    let def = self.resolved(unit, &path);
                    if let (Some(def), Some(reads)) = (def, reads.as_deref_mut()) {
                        reads.insert(def);
                    }
                    let effects = def
                        .and_then(|def| self.known.get(&def))
                        .map(|s| s.iter().cloned().collect())
                        .unwrap_or_default();
                    (def, effects)
                }
            };
            // The call graph, as this walk resolved it (4c), a pure call too.
            if let Some(callee) = called {
                out.calls.push(Called {
                    call: id,
                    callee,
                    effects: effects.clone(),
                });
            }
            if effects.is_empty() {
                continue;
            }

            let short = path.rsplit('.').next().unwrap_or(&path).to_string();
            let via = match enclosing_callback(body, id, &inside_callback) {
                Some(passed_to) => Via::Callback {
                    passed_to,
                    callee: path.clone(),
                },
                None if self.sigs.by_path(&path).is_none() => Via::Helper {
                    helper: short,
                    callee: path.clone(),
                },
                None => Via::Direct {
                    callee: path.clone(),
                },
            };

            for e in effects {
                out.effects.insert(e.clone());
                out.sources.push(Source {
                    effect: e,
                    span: body.expr_span(id),
                    via: via.clone(),
                });
            }
        }
        out
    }

    /// **What the declaration `def`'s own body performs, as `run` found it**
    /// (ADR-0283): its sources, and each call the walk resolved (4c). `None`
    /// for a declaration with no body, or one `run` did not see.
    pub fn performed_of(&self, def: DefId) -> Option<&Inferred> {
        self.performed.get(&def)
    }

    /// **Which binding each local name means in the declaration `decl` of
    /// `unit`**, as `run` resolved it for the walk (ADR-0063). `None` for a
    /// body `run` did not see.
    pub fn lexical(
        &self,
        unit: usize,
        decl: crate::hir::DeclId,
    ) -> Option<&crate::lexical::Lexical> {
        self.lexicals.get(&(unit, decl))
    }

    /// **Whether `name`, in `body` of `unit`, is a local's**: a binding in
    /// scope there, as the body's declaration's scopes resolve it. `false`
    /// for a body `run` did not see, which resolves as before.
    pub fn local(&self, unit: usize, body: &Body, name: ExprId) -> bool {
        self.lexicals
            .get(&(unit, body.owner))
            .is_some_and(|l| l.binder(name).is_some())
    }

    /// Which declaration a path names, from inside `unit`.
    ///
    /// `usize::MAX` means "no unit", which resolves nothing — the honest
    /// answer for a caller that did not say where it was standing. Falling back
    /// to a name match would reintroduce exactly the defect this replaces.
    /// [`Inference::resolved`], exposed for callers that need the same answer.
    ///
    /// Public so contract derivation resolves component calls the SAME way
    /// effect propagation does. A second resolver would agree until it did not.
    pub fn resolved_from(&self, unit: usize, path: &str) -> Option<DefId> {
        self.resolved(unit, path)
    }

    /// What a CALL names, preferring the namespace calls live in.
    ///
    /// `store.page` can import `domain.{ Cart }` — a type — and
    /// `cart.queries`, which declares `query Cart`. The general resolver tries
    /// the type namespace first, so `query Cart(s)` resolved to the record
    /// definition: `docs/RISK_QUEUE.md` records that shape from E6, where every
    /// page's dependency on its own query pointed at a type of the same name.
    ///
    /// A call is a term. `Ui` is tried after it because a page may render a
    /// `view` or a `component` by name, and neither is callable.
    pub fn called_from(&self, unit: usize, path: &str) -> Option<DefId> {
        use crate::resolve::Namespace;
        if path.contains('.') {
            return self.resolved(unit, path);
        }
        for ns in [Namespace::Term, Namespace::Ui] {
            if let Resolution::Local(def) | Resolution::Imported { def, .. } =
                self.workspace.resolve_in(unit, ns, path)
            {
                return Some(def);
            }
        }
        self.resolved(unit, path)
    }

    /// **Whether `name` is a predicate a page asks**, as `holds::asked`
    /// counts one (the caller holds that it is named alone, outside a
    /// handler): a predicate with no parameters, the declaration the
    /// predicate namespace gives `name`, and no other a name of that
    /// spelling resolves to.
    fn asks_a_predicate(&self, unit: usize, name: &str) -> bool {
        use crate::resolve::Namespace;
        if unit == usize::MAX {
            return false;
        }
        let predicate = match self.workspace.resolve_in(unit, Namespace::Predicate, name) {
            Resolution::Local(def) | Resolution::Imported { def, .. } => def,
            _ => return false,
        };
        let unparameterised = self
            .sigs
            .by_def(predicate)
            .is_none_or(|sig| sig.params.is_empty());
        unparameterised && self.resolved(unit, name) == Some(predicate)
    }

    fn resolved(&self, unit: usize, path: &str) -> Option<DefId> {
        if unit == usize::MAX {
            return None;
        }
        if let Resolution::Local(def) | Resolution::Imported { def, .. } =
            self.workspace.resolve_path(unit, path)
        {
            return Some(def);
        }
        // A MEMBER call: `it.style.set_padding`, whose head is a value rather
        // than a module, so `resolve_path` cannot see it. Its last segment is
        // matched against the declarations THIS UNIT CAN SEE — its own module
        // and the modules it imports — and accepted only when exactly one
        // matches.
        //
        // This replaces the old fallback, which matched the last segment
        // against every declaration in the program by name. That is
        // `docs/RISK_QUEUE.md` 34: `Resources.Cart` and `store.page.Cart`
        // collided, and one silently received the other's effects.
        //
        // Scoped-and-unique keeps what the old fallback was load-bearing for —
        // R-035 depends on `set_padding`'s row reaching a call written as a
        // member — while making the cross-module borrow impossible: an
        // unrelated module is either not imported, or it produces two matches
        // and the answer is none.
        let last = path.rsplit('.').next()?;
        let module = self.workspace.module_of(unit)?;
        let visible = std::iter::once(module.name.as_str())
            .chain(module.imports.iter().map(|i| i.module.as_str()));

        let mut found: Option<DefId> = None;
        for m in visible {
            let candidate = match self.workspace.resolve_path(unit, &format!("{m}.{last}")) {
                Resolution::Local(def) | Resolution::Imported { def, .. } => def,
                _ => continue,
            };
            match found {
                // Ambiguous: two visible declarations share the spelling, so
                // there is no answer rather than an arbitrary one.
                Some(seen) if seen != candidate => return None,
                _ => found = Some(candidate),
            }
        }
        // And that is all. There is deliberately NO program-wide fallback.
        //
        // There was one — a spelling that named exactly one declaration
        // anywhere in the program — and the architect's ruling of 2026-08-07
        // ordered it deleted:
        //
        // > Delete program-wide-unique resolution completely. Add a structural
        // > test proving no correctness path performs global last-segment
        // > uniqueness lookup.
        //
        // What it was load-bearing for was `el.getBoundingClientRect()`, and
        // that now resolves through the RECEIVER's type: `infer.rs` gives a
        // callback's parameter the element type of the collection it is applied
        // to, and `member_effects` looks the member up on that type. A member
        // is visible because its receiver declares it, not because nothing else
        // in the program happens to share its spelling.
        found
    }

    /// **What this definition actually requires at run time.**
    ///
    /// Architect ruling, 2026-08-07, and the reason no caller may decide this
    /// for itself:
    ///
    /// ```text
    /// DeclaredEffects       what this interface permits/promises
    /// InferredEffects       what this implementation actually performs
    /// RequiredCapabilities  what this compiled artifact currently needs
    ///
    /// local code      RequiredCapabilities <- InferredEffects
    /// bodyless code   RequiredCapabilities <- DeclaredEffects
    /// ```
    ///
    /// So for
    ///
    /// ```text
    /// fn foo() !{ database.read, trace } { trace("hello") }
    /// ```
    ///
    /// the host must not grant database access merely because the annotation
    /// permitted it. The declaration still CONSTRAINS: a later implementation
    /// that adds `database.write` is outside its contract and the checker says
    /// so — that check lives with the row rules and is unaffected by this.
    ///
    /// A definition with no body is an interface or separately compiled code.
    /// Its declared row is a promise, and a promise is all there is.
    pub fn effective_effects(&self, unit: usize, hir: &Hir, id: crate::hir::DeclId) -> Vec<String> {
        let decl = hir.decl(id);
        match decl.body {
            Some(body) => {
                let b = hir.body(body);
                let mut out: Vec<String> = self
                    .performed(unit, hir, id, b)
                    .effects
                    .into_iter()
                    .collect();
                // **Plus the named roots that ARE this declaration's work.**
                //
                // A painter's `draw` block and a resource's `acquire`/`release`
                // blocks were body statements until 2026-08-11 and their
                // effects were the declaration's. They are roots now — so that
                // their binders scope and their contents are attributed — and
                // this is what keeps the migration faithful.
                //
                // An optimistic clause is deliberately excluded: it runs on the
                // client at a different time, and a command whose row absorbed
                // it would be indistinguishable from one performing the
                // transition itself.
                self.roots_and_events(unit, b, decl, &mut out);
                out.sort();
                out.dedup();
                out
            }
            None => decl
                .declared_effects
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|e| e.written.clone())
                .collect(),
        }
    }

    /// `effective_effects`, minus what only happens inside `exclude`.
    ///
    /// E8-0 needs this for a page whose handler lambdas are separately loaded
    /// units — see `infer_excluding`. The declared-versus-inferred rule is the
    /// same one; only the scope narrows.
    /// **The roots that are a declaration's work, and its events**: what
    /// `effective_effects` adds to its body's row, and a contract's
    /// capabilities with it (ADR-0208).
    fn roots_and_events(
        &self,
        unit: usize,
        b: &crate::hir::Body,
        decl: &crate::hir::Decl,
        out: &mut Vec<String>,
    ) {
        for (_, root) in decl.term_roots() {
            if !root.context.contributes_to_declaration() {
                continue;
            }
            out.extend(self.infer_rooted(unit, b, root.root).effects);
        }
        // **And the events it emits and the entries it invalidates**
        // (ADR-0208, ADR-0209), each written to the outbox with its writes.
        if ["emits", "invalidates"]
            .iter()
            .any(|c| decl.policy(c).is_some_and(|p| !p.keys.is_empty()))
        {
            out.push(crate::signatures::OUTBOX_WRITE.to_string());
        }
    }

    pub fn effective_effects_excluding(
        &self,
        unit: usize,
        hir: &Hir,
        id: crate::hir::DeclId,
        exclude: &[ExprId],
    ) -> Vec<String> {
        let decl = hir.decl(id);
        match decl.body {
            Some(body) => {
                let b = hir.body(body);
                let mut out: Vec<String> = subtracted(self.performed(unit, hir, id, b), b, exclude)
                    .effects
                    .into_iter()
                    .collect();
                self.roots_and_events(unit, b, decl, &mut out);
                out.sort();
                out.dedup();
                out
            }
            None => self.effective_effects(unit, hir, id),
        }
    }

    /// The effects a declaration performs, after propagation.
    pub fn effects_of_def(&self, def: DefId) -> Option<&BTreeSet<Effect>> {
        self.known.get(&def)
    }

    /// The effects of the declaration `path` names, from inside `unit`.
    ///
    /// There is deliberately no bare-name form. A caller that has only a
    /// spelling does not have enough information to be answered, and the
    /// previous version answered anyway.
    pub fn effects_of(&self, unit: usize, path: &str) -> Option<&BTreeSet<Effect>> {
        self.resolved(unit, path).and_then(|d| self.known.get(&d))
    }
}

/// **What `out` found, less what only happens inside `exclude`**: each
/// effect every one of whose sources lies inside an excluded subtree.
fn subtracted(mut out: Inferred, body: &Body, exclude: &[ExprId]) -> Inferred {
    if exclude.is_empty() {
        return out;
    }
    let inside = |span: &Span| {
        exclude.iter().any(|root| {
            let outer = body.expr_span(*root);
            span.start >= outer.start && span.end <= outer.end
        })
    };

    // SUBTRACTED, not rebuilt.
    //
    // An effect is dropped only when it has at least one source and every
    // one of them lies inside an excluded subtree. Rebuilding the set from
    // the surviving sources instead was wrong in the direction that
    // matters: an effect carried without a source vanished, and a query
    // that reads the database reported needing no authority at all.
    //
    // Keeping a sourceless effect is the conservative direction. An
    // over-stated capability is refused work; an under-stated one is
    // authority nobody approved.
    let mut dropped: BTreeSet<String> = BTreeSet::new();
    for effect in &out.effects {
        let mut seen = false;
        let mut all_inside = true;
        for s in out.sources.iter().filter(|s| &s.effect == effect) {
            seen = true;
            if !inside(&s.span) {
                all_inside = false;
            }
        }
        if seen && all_inside {
            dropped.insert(effect.clone());
        }
    }
    out.effects.retain(|e| !dropped.contains(e));
    out.sources.retain(|s| !inside(&s.span));
    out
}

/// The function a call sits inside the callback of, if any.
fn enclosing_callback(
    body: &Body,
    call: ExprId,
    callbacks: &BTreeMap<ExprId, Handed>,
) -> Option<String> {
    enclosing_callbacks(body, call, callbacks)
        .first()
        .map(|(_, handed)| handed.to.clone())
}

/// **Every callback a call sits inside**, innermost first: each lambda and
/// what it was handed to. 4c's loops read them all, since a read in a
/// callback nested in another's is in both loops.
pub fn enclosing_callbacks<'c>(
    body: &Body,
    call: ExprId,
    callbacks: &'c BTreeMap<ExprId, Handed>,
) -> Vec<(ExprId, &'c Handed)> {
    // A call is inside a callback when one of the lambdas handed to another
    // function contains it. Compared by span, because HIR has no parent links —
    // the check is containment, which is exactly what "inside" means.
    let span = body.expr_span(call);
    let mut found: Vec<(ExprId, &Handed)> = callbacks
        .iter()
        .filter(|(lambda, _)| {
            let l = body.expr_span(**lambda);
            l.start <= span.start && span.end <= l.end && l != span
        })
        .map(|(lambda, handed)| (*lambda, handed))
        .collect();
    found.sort_by_key(|(lambda, _)| {
        let l = body.expr_span(*lambda);
        l.end - l.start
    });
    found
}

/// A callee's dotted path as written.
fn path_of(body: &Body, id: ExprId) -> String {
    match body.expr(id) {
        Expr::Name(n) => n.clone(),
        Expr::Field { base, name } => {
            let base = path_of(body, *base);
            if base.is_empty() {
                name.clone()
            } else {
                format!("{base}.{name}")
            }
        }
        _ => String::new(),
    }
}

/// **The execution context a span sits in: every enclosing frame phase,
/// innermost first.**
///
/// Charter §7.5A gives the frame a shape — measure, then mutate, then paint,
/// then post-paint — and which phase code is in decides what it may do. That is
/// an ordering question, not a question of which effects exist. E2D's inference
/// answers the second; this answers the first.
///
/// A CHAIN rather than one phase, because phases nest and a nested one does not
/// escape its parent:
///
/// ```text
/// post_paint {
///     let h = measure { el.height() }     still in the post-paint frame
/// }
/// ```
///
/// There is deliberately no `phase_at` returning one phase. There was, every
/// caller used it, and using it is what made the rule narrow: an inner phase
/// looked like the whole context. A single-answer variant kept "for
/// convenience" is an invitation to reintroduce that.
///
/// `R-042` is that program. It was caught only because `measure { .. }`
/// synthesized `layout.measure` at the KEYWORD's own span, which `phase_at`
/// then attributed to `post_paint` — so the diagnostic was right and the reason
/// was an artefact of where a made-up effect was recorded. Deleting the
/// synthesis (architect ruling: a phase says WHEN work runs, not what it does)
/// made the fixture go silent, which is how the narrowness was found.
pub fn phases_at(body: &Body, span: &Span) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for id in body.walk() {
        let Expr::Keyword { keyword, .. } = body.expr(id) else {
            continue;
        };
        if !matches!(
            keyword.as_str(),
            "measure" | "mutate" | "post_paint" | "animate" | "frame" | "draw" | "subtree"
        ) {
            continue;
        }
        let k = body.expr_span(id);
        if k.start <= span.start && span.end <= k.end && k != *span {
            found.push((k.end - k.start, keyword.clone()));
        }
    }
    found.sort_by_key(|(w, _)| *w);
    found.into_iter().map(|(_, k)| k).collect()
}

/// May work with these FACETS happen in this frame phase?
///
/// Architect ruling, 2026-08-07:
///
/// ```text
/// animate phase forbids LayoutRead | LayoutWrite
/// measure phase forbids LayoutWrite
/// mutate  phase permits appropriate writes
/// ```
///
/// > rather than `family == "layout"`.
///
/// It was keyed on the family, and the family is a proxy for meaning that is
/// too coarse to be one: `style.mutate<LayoutAffect>` and
/// `style.mutate<PaintOnly>` share a family, so the animate rule could not
/// catch the exact case its own comment described. Each entry is charter §7.5A
/// stating what a phase is *for*; a phase that permitted everything would not
/// be a phase.
///
/// The facets come from the effect's own declaration — see
/// [`crate::ontology::Facet`] — so a new effect with layout consequences is
/// caught here without teaching this function its name.
pub fn forbidden_in_phase(phase: &str, facets: &BTreeSet<Facet>) -> Option<&'static str> {
    let has = |f: Facet| facets.contains(&f);
    match phase {
        // The measure phase reads. A write inside it invalidates the very
        // geometry the phase exists to read consistently.
        "measure" if has(Facet::LayoutWrite) || has(Facet::DomWrite) => Some(
            "the measure phase reads geometry; writing inside it invalidates what the rest of \
             the phase is about to read",
        ),
        // After paint, the frame is already on screen. Measuring forces the
        // browser to lay out again for a frame nobody will see.
        "post_paint" if has(Facet::LayoutRead) => Some(
            "the frame is already presented; measuring now forces a second layout for a frame \
             nobody will see",
        ),
        // A painter draws. Touching the document from inside one re-enters
        // layout from a phase that runs after it.
        "draw" if has(Facet::DomWrite) || has(Facet::LayoutWrite) || has(Facet::LayoutRead) => {
            Some(
                "a painter draws; reaching the document from inside one re-enters layout from a \
                 phase that runs after it",
            )
        }
        // A compositor animation runs off the main thread. An animated property
        // that invalidates layout drags it back on — and THAT is the case the
        // family-keyed version could not see, because a layout-affecting style
        // write is in the `style` family.
        "animate" if has(Facet::LayoutWrite) || has(Facet::LayoutRead) => Some(
            "a compositor animation runs without layout; animating a property that invalidates \
             it forces a layout every frame",
        ),
        _ => None,
    }
}

/// Declarations whose bodies must be pure regardless of what they declare.
///
/// Charter §7.5A and §8.5: a `view` renders; it does not fetch. The row it
/// writes is a claim about itself, and `!{}` on a view that reads the database
/// is the corpus's very first rejected case.
/// Why a declaration's output is reused rather than recomputed per reader.
///
/// Charter §7.9 and §9.4. Two different declarations reach the same place: a
/// `placement build` page is generated once and shipped as a file, and a
/// `partition public` materialization is computed once and served from one
/// cache entry to every reader. In both, output that varies with *when* it ran
/// is not a function of its declared inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reuse {
    /// Recomputed for its reader; nondeterminism is that reader's own.
    PerReader,
    /// Generated once at build time.
    Build,
    /// One cache entry serves every reader.
    SharedPartition,
}

/// Effects that make an output depend on *when* it ran.
///
/// The wall clock is the corpus's case. It is `clock.wall` and not the whole
/// `clock` family on purpose: a monotonic read measures a duration and can be
/// deterministic in the sense that matters here, while a wall read cannot.
pub fn nondeterministic(effect: &str) -> bool {
    matches!(family_of(effect), "random") || effect.starts_with("clock.wall")
}

/// What a database effect reaches: `Carts` in `database.write<Carts>`, when
/// its operation is `operation` (ADR-0101).
///
/// The domain is the argument as written, as a capability's is, so
/// `database.read<Carts>` and `database.write<Carts>` reach one domain. An
/// effect with no argument reaches none this can name.
pub fn database_domain<'e>(effect: &'e str, operation: &str) -> Option<&'e str> {
    let (head, rest) = effect.split_once('<')?;
    let domain = rest.strip_suffix('>')?.trim();
    (head.trim().strip_prefix("database.") == Some(operation) && !domain.is_empty())
        .then_some(domain)
}

/// Does `effect` change the state a query reads? A write, or a transaction,
/// which exists to hold writes (ADR-0100).
fn writes(effect: &str) -> bool {
    matches!(
        effect.split('<').next().unwrap_or(effect).trim(),
        "database.write" | "database.transaction"
    )
}

pub fn forbidden_in(
    decl: &Decl,
    reuse: Reuse,
    world: Option<crate::placement::World>,
    // Where the program declares its effects are meaningful. Needed for one
    // question below — is this a world secrets exist in — which used to read a
    // hard-coded table in `placement.rs`.
    declared: &dyn crate::placement::Placements,
    effect: &str,
) -> Option<&'static str> {
    use crate::hir::DeclKind::*;
    let family = family_of(effect);

    // Charter §7.9, §9.4. An artifact computed once and reused is only correct
    // if it is a function of its declared inputs. `clock.wall` makes it a
    // function of when it ran as well, so two readers of one cache entry — or
    // one reader of a file generated last Tuesday — get an answer that was
    // never true for them.
    if reuse != Reuse::PerReader && nondeterministic(effect) {
        return Some(match reuse {
            Reuse::Build => {
                "static generation requires a deterministic effect row: the output is \
                 generated once and shipped as a file, so anything that varies with \
                 when it ran is baked in"
            }
            _ => {
                "a shared materialized fragment must be a pure function of its declared \
                 dependencies: one cache entry serves every reader, so a value that \
                 varies with when it was computed is served to readers it was never \
                 true for"
            }
        });
    }

    // ADR-0304: a value that compares the wall clock holds until the
    // earliest instant it compared, and a query's entry, shared or not, is
    // read again then. Nothing makes a file again, nor a materialization,
    // which is made again when an event reaches it: no event says the
    // instant passed.
    if reuse != Reuse::PerReader && effect.starts_with("clock.compare") {
        return Some(match reuse {
            Reuse::Build => {
                "static generation cannot compare the clock: the output is generated once \
                 and shipped as a file, and nothing tells a file when the instant it \
                 compared passes"
            }
            _ => {
                "a shared materialized fragment cannot compare the clock: it is made again \
                 when an event reaches it, and no event says the instant it compared has \
                 passed"
            }
        });
    }

    // A painter is identified by what it declares, not by a declaration kind:
    // `paint.custom` in the row *is* the statement "this runs inside the paint
    // pipeline". Charter §7.5A gives it inputs precisely so it can be replayed
    // and cached, which is only sound if it is a pure function of them.
    if decl
        .declared_effects
        .as_ref()
        .is_some_and(|r| r.iter().any(|e| e.path == "paint.custom"))
        && matches!(family, "dom" | "style" | "layout" | "database" | "network")
    {
        return Some(
            "a painter must be a pure function of its declared inputs, so that the \
             result can be replayed and cached — `paint.custom` may not use effect \
             `dom.mutate`, nor any other reach outside the inputs it names. A painter \
             that touches the document re-enters the pipeline that called it",
        );
    }

    match (decl.kind, family) {
        (View | Component | Page, "database") => {
            Some("a view renders; it cannot reach the database while doing so")
        }
        // Charter §7.5: a query is a keyed remote READ, cached, deduplicated
        // and retried as reads are; a mutation is a command's, with its
        // authorization, idempotency and invalidation. A write inside a query
        // happens once for every reader of a cache entry, and again on every
        // retry (ADR-0100).
        (Query | Subscription, "database") if writes(effect) => Some(
            "a query reads: a write inside one happens once for every reader of its \
             cache entry, and again on every retry, where a command declares its \
             idempotency and what it invalidates",
        ),
        (View | Component | Page, "network") => {
            Some("a view renders; fetching during render is what streaming exists to avoid")
        }
        // A view is *usually* rendered where secrets are not — but a
        // declaration that names a world where they ARE may read one while
        // rendering. R-003 is an origin-placed page, and reporting it here
        // said "a page may not do this" about something a page may do; the
        // defect is that the secret then reaches the MARKUP, which is
        // `secret_to_browser` and is reported separately.
        //
        // `Grant::Blocked` counts as "secrets are not here": if this program
        // declares no `secret` effect, `world` cannot be a world that holds
        // one. That is the conservative reading and it is also the true one —
        // the family the match arm named came from the effect's own spelling,
        // and a spelling is not a declaration.
        (View | Component | Page, "secret")
            if !world
                .is_some_and(|w| w.grants("secret", declared) == crate::placement::Grant::Yes) =>
        {
            Some("a view is rendered where secrets are not")
        }
        // Charter §7.5A. Reading geometry while rendering forces a synchronous
        // reflow, which is the cost the frame phases exist to make visible.
        // Measuring belongs in a `measure` phase, not in the render itself.
        (View | Component | Page, "layout") => {
            Some("reading geometry during render forces a synchronous reflow")
        }
        _ => None,
    }
}

#[cfg(test)]
mod queue_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::lower_file;
    use crate::placement::{PlacementLookup, World};
    use crate::resolve::Workspace;
    use pw_syntax::parse_tree;

    /// The one declaration `forbidden_in` asks a [`crate::placement::Placements`]
    /// for: is `secret` an effect this world holds.
    ///
    /// Matched by whole spelling rather than by splitting a name, because a
    /// test double that took an effect name apart would be a second parser of
    /// the thing `ontology.rs` is the only module allowed to parse.
    struct Platform;

    impl crate::placement::Placements for Platform {
        fn placement_of(&self, effect: &str) -> PlacementLookup {
            match effect {
                "secret" | "secret.read" => PlacementLookup::Known(vec![World::Origin]),
                _ => PlacementLookup::Unrestricted,
            }
        }
    }

    /// [`super::forbidden_in`] against that vocabulary.
    fn forbidden_in(
        decl: &Decl,
        reuse: Reuse,
        world: Option<World>,
        effect: &str,
    ) -> Option<&'static str> {
        super::forbidden_in(decl, reuse, world, &Platform, effect)
    }

    fn program(sources: &[&str]) -> (Vec<Hir>, Signatures, Workspace) {
        let owned: Vec<Hir> = sources
            .iter()
            .map(|s| lower_file(s, &parse_tree(s).green))
            .collect();
        let refs: Vec<&Hir> = owned.iter().collect();
        let ws = Workspace::build(&refs);
        let sigs = Signatures::build(&ws, &refs);
        (owned, sigs, ws)
    }

    const LIB: &str = "module Stores\n\nfn get(id: Int) -> Int !{ database.read } { 0 }\n";

    fn effects_of(sources: &[&str], decl: &str) -> BTreeSet<Effect> {
        let (hirs, sigs, ws) = program(sources);
        let refs: Vec<&Hir> = hirs.iter().collect();
        let mut inf = Inference::new(&sigs, &ws);
        inf.run(&refs);
        for (unit, hir) in refs.iter().enumerate() {
            for (_, d) in hir.all_decls() {
                if d.name != decl {
                    continue;
                }
                let Some(b) = d.body else { continue };
                return inf.infer_at(unit, hir.body(b)).effects;
            }
        }
        BTreeSet::new()
    }

    #[test]
    fn a_direct_call_contributes_its_effect() {
        let e = effects_of(
            &[
                LIB,
                "module page\n\nimport Stores\n\nfn f() -> Int !{} { Stores.get(1) }\n",
            ],
            "f",
        );
        assert!(e.contains("database.read"), "{e:?}");
    }

    #[test]
    fn an_effect_hidden_behind_a_helper_still_propagates() {
        // RQ-2's second shape. A checker that only looked at direct calls would
        // report `f` as pure, which is the failure the corpus exists to catch.
        let src = "module page\n\nimport Stores\n\n\
                   fn helper(id: Int) -> Int !{} { Stores.get(id) }\n\
                   fn f() -> Int !{} { helper(1) }\n";
        let e = effects_of(&[LIB, src], "f");
        assert!(
            e.contains("database.read"),
            "the effect must reach `f` through `helper`: {e:?}"
        );
    }

    #[test]
    fn an_effect_inside_a_callback_still_propagates() {
        // RQ-2's third shape, and the one the architect called load-bearing: the
        // callee knows nothing about the effect it ends up performing.
        let src = "module page\n\nimport Stores\nimport List\n\n\
                   fn f(ids: List<Int>) -> Int !{} { List.map(ids, id => Stores.get(id)) }\n";
        let lib2 = "module List\n\nfn map(xs: Int, f: Int) -> Int !{} { 0 }\n";
        let e = effects_of(&[LIB, lib2, src], "f");
        assert!(
            e.contains("database.read"),
            "the effect must escape the callback: {e:?}"
        );
    }

    #[test]
    fn a_pure_body_infers_nothing() {
        // The control. If everything came back effectful the tests above would
        // pass while measuring nothing.
        let src = "module page\n\nfn f() -> Int !{} { 1 + 2 }\n";
        assert!(effects_of(&[src], "f").is_empty());
    }

    #[test]
    fn a_declared_family_covers_its_members_but_not_the_reverse() {
        let mut declared = BTreeSet::new();
        declared.insert("database");
        assert!(covered(&declared, "database.read"));
        assert!(covered(&declared, "database.write"));

        let mut narrow = BTreeSet::new();
        narrow.insert("database.read");
        assert!(covered(&narrow, "database.read"));
        assert!(
            !covered(&narrow, "database.write"),
            "declaring a read must not permit a write"
        );
    }

    #[test]
    fn an_undeclared_effect_is_reported_with_its_chain() {
        let src = "module page\n\nimport Stores\n\n\
                   fn helper(id: Int) -> Int !{} { Stores.get(id) }\n\
                   fn f() -> Int !{} { helper(1) }\n";
        let (hirs, sigs, ws) = program(&[LIB, src]);
        let refs: Vec<&Hir> = hirs.iter().collect();
        let mut inf = Inference::new(&sigs, &ws);
        inf.run(&refs);

        // Unit 1 — `helper` is a SIBLING in that unit, and finding it is the
        // point of the test. `infer` without a unit resolves no siblings, by
        // design: a caller that does not say where it stands cannot be told
        // which `helper` it means.
        let hir = &refs[1];
        let (_, f) = hir.all_decls().find(|(_, d)| d.name == "f").expect("f");
        let found = inf.infer_at(1, hir.body(f.body.unwrap()));
        let missing = found.undeclared(Some(&[]));
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(missing[0].effect, "database.read");
        assert!(
            missing[0].via.describe().contains("helper"),
            "the reason must name the chain, not just the effect: {}",
            missing[0].via.describe()
        );
    }

    #[test]
    fn a_row_that_is_not_written_is_not_checked() {
        // An open row means "not stated". Inferring one for a declaration that
        // never claimed anything would reject working programs, and E2D's job
        // is to check claims, not to invent them.
        let found = Inferred {
            evidence: crate::provenance::Evidence::default(),
            effects: BTreeSet::from(["database.read".to_string()]),
            sources: vec![Source {
                effect: "database.read".into(),
                span: 0..1,
                via: Via::Direct {
                    callee: "Stores.get".into(),
                },
            }],
            calls: Vec::new(),
        };
        assert!(found.undeclared(None).is_empty());
        assert_eq!(found.undeclared(Some(&[])).len(), 1);
    }

    /// **A query reads** (ADR-0100): a write, or the transaction that holds
    /// one, is a command's.
    #[test]
    fn a_query_may_read_and_not_write() {
        use crate::hir::DeclKind;
        let query = crate::hir::Decl {
            name: "Q".into(),
            name_span: 0..0,
            kind: DeclKind::Query,
            params: vec![],
            ret: None,
            variants: None,
            fields: None,
            opaque_of: None,
            invariant: None,
            type_params: vec![],
            policies: vec![],
            imports: vec![],
            visibility: None,
            declared_effects: Some(vec![]),
            body: None,
            terms: None,
            children: vec![],
            mutable: false,
        };
        for effect in ["database.write<Carts>", "database.transaction"] {
            assert!(
                forbidden_in(&query, Reuse::PerReader, None, effect).is_some(),
                "{effect}"
            );
        }
        assert!(forbidden_in(&query, Reuse::PerReader, None, "database.read<Carts>").is_none());
        let subscription = crate::hir::Decl {
            kind: DeclKind::Subscription,
            ..query.clone()
        };
        assert!(
            forbidden_in(
                &subscription,
                Reuse::PerReader,
                None,
                "database.write<Carts>"
            )
            .is_some()
        );
        let command = crate::hir::Decl {
            kind: DeclKind::Command,
            ..query
        };
        for effect in ["database.write<Carts>", "database.transaction"] {
            assert!(
                forbidden_in(&command, Reuse::PerReader, None, effect).is_none(),
                "a command writes: {effect}"
            );
        }
    }

    /// ADR-0101: a read and a write of one domain reach it, and an operation
    /// reaches only its own.
    #[test]
    fn a_database_effect_names_its_domain() {
        assert_eq!(
            database_domain("database.write<Carts>", "write"),
            Some("Carts")
        );
        assert_eq!(
            database_domain("database.read< Carts >", "read"),
            Some("Carts")
        );
        assert_eq!(database_domain("database.read<Carts>", "write"), None);
        assert_eq!(database_domain("database.write<Carts>", "read"), None);
        assert_eq!(database_domain("secret.write<Carts>", "write"), None);
        assert_eq!(database_domain("database.transaction", "write"), None);
        assert_eq!(database_domain("database.write<>", "write"), None);
    }

    #[test]
    fn a_view_may_not_reach_the_database_whatever_it_declares() {
        use crate::hir::DeclKind;
        let view = crate::hir::Decl {
            name: "V".into(),
            name_span: 0..0,
            kind: DeclKind::View,
            params: vec![],
            ret: None,
            variants: None,
            fields: None,
            opaque_of: None,
            invariant: None,
            type_params: vec![],
            policies: vec![],
            imports: vec![],
            visibility: None,
            declared_effects: Some(vec![]),
            body: None,
            terms: None,
            children: vec![],
            mutable: false,
        };
        assert!(forbidden_in(&view, Reuse::PerReader, None, "database.read").is_some());
        assert!(forbidden_in(&view, Reuse::PerReader, None, "network.fetch").is_some());
        // ...but rendering effects are exactly what a view is for.
        assert!(forbidden_in(&view, Reuse::PerReader, None, "dom.mutate").is_none());

        // A view placed where secrets live may read one while rendering. What
        // it may not do is put it in the markup, which is a different
        // invariant with a different code. R-003 was reported for both, and
        // "a page may not do this" was false about the one it may.
        assert!(
            forbidden_in(&view, Reuse::PerReader, None, "secret.read").is_some(),
            "a view with no declared world is rendered where secrets are not"
        );
        assert!(
            forbidden_in(
                &view,
                Reuse::PerReader,
                Some(crate::placement::World::Origin),
                "secret.read"
            )
            .is_none(),
            "an origin-placed view renders where secrets ARE"
        );
        assert!(
            forbidden_in(
                &view,
                Reuse::PerReader,
                Some(crate::placement::World::Browser),
                "secret.read"
            )
            .is_some(),
            "a browser-placed view still may not"
        );

        let f = crate::hir::Decl {
            kind: DeclKind::Fn,
            ..view
        };
        assert!(
            forbidden_in(&f, Reuse::PerReader, None, "database.read").is_none(),
            "an ordinary fn may read the database if it says so"
        );

        // (see below for the delegation control)

        // Reuse, not the declaration kind, is what makes a clock read wrong.
        // The same page is fine when it is rendered for its reader.
        let page = crate::hir::Decl {
            kind: DeclKind::Page,
            ..f
        };
        assert!(
            forbidden_in(&page, Reuse::PerReader, None, "clock.wall").is_none(),
            "a page rendered per request may read the clock"
        );
        assert!(forbidden_in(&page, Reuse::Build, None, "clock.wall").is_some());
        assert!(forbidden_in(&page, Reuse::SharedPartition, None, "clock.wall").is_some());

        // And the distinction inside the clock family is load-bearing: a
        // monotonic read measures a duration and does not make the output
        // depend on when it ran. Without this, the rule would be "a build-time
        // page may not use the clock", which is a different and wronger claim.
        assert!(
            forbidden_in(&page, Reuse::Build, None, "clock.read").is_none(),
            "a duration measurement does not make a build artifact irreproducible"
        );

        // A clock compared (ADR-0304) is true until a known instant: a
        // query's entry, read again then, may hold it. A file generated once,
        // and a materialization made again only when an event reaches it,
        // are told nothing when that instant passes.
        assert!(forbidden_in(&page, Reuse::PerReader, None, "clock.compare").is_none());
        assert!(forbidden_in(&page, Reuse::SharedPartition, None, "clock.compare").is_some());
        assert!(forbidden_in(&page, Reuse::Build, None, "clock.compare").is_some());
    }
}
