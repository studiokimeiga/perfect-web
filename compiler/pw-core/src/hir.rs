//! HIR — the representation every semantic analysis consumes.
//!
//! ADR-0012 drew the boundary: checkers see **ids and spans, never syntax
//! nodes**. ADR-0014 fixed the shape: id-indexed arenas, per body, with a span
//! stored beside every node.
//!
//! Nothing in this module knows that Rowan exists. That is the point of it.
//! `crate::lower` is the single module allowed to bridge the two, and
//! `hir::tests::only_lower_touches_syntax_nodes` enforces it.

use std::ops::Range;

pub type Span = Range<usize>;

macro_rules! id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(
    /// A source file's module declaration.
    ModuleId
);
id!(
    /// A top-level or nested declaration: `fn`, `view`, `component`, `query`, ...
    DeclId
);
id!(
    /// A declaration's body. The unit of reanalysis (ADR-0014).
    BodyId
);
id!(
    /// An expression, scoped to one [`Body`].
    ExprId
);
id!(
    /// A pattern, scoped to one [`Body`].
    PatternId
);
id!(
    /// A type reference, scoped to one [`Body`].
    TypeRefId
);
id!(
    /// A markup node, scoped to one [`Body`].
    NodeId
);

/// An id-indexed arena that keeps a span beside every node.
///
/// The span is not optional and not recomputed: charter §16.3 requires every
/// diagnostic to carry an origin span, so "I hold an id" must imply "I can
/// point at source".
#[derive(Debug, Clone)]
pub struct Arena<T> {
    nodes: Vec<T>,
    spans: Vec<Span>,
}

// Hand-written: `derive(Default)` would demand `T: Default`, which no HIR node
// has or should have.
impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            spans: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Push a node with its span, returning its raw index.
    pub fn alloc(&mut self, node: T, span: Span) -> u32 {
        let i = self.nodes.len() as u32;
        self.nodes.push(node);
        self.spans.push(span);
        i
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.nodes.get(i)
    }

    /// Move every span from `from` onward by `offset` bytes.
    ///
    /// For a sub-parse. A policy value whose domain is executable code is
    /// parsed from its own text, so its spans start at zero; without this a
    /// diagnostic about `optimistic cart.add(..)` would underline the first
    /// few columns of the file.
    pub fn shift_spans_from(&mut self, from: usize, offset: usize) {
        for s in self.spans.iter_mut().skip(from) {
            *s = (s.start + offset)..(s.end + offset);
        }
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.nodes.get_mut(i)
    }

    pub fn span_at(&self, i: usize) -> Option<&Span> {
        self.spans.get(i)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &T, &Span)> {
        self.nodes
            .iter()
            .zip(self.spans.iter())
            .enumerate()
            .map(|(i, (n, s))| (i, n, s))
    }
}

// --- declarations --------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Fn,
    View,
    Component,
    Page,
    /// `layout StoreLayout { .. }`: the markup and the bindings the pages
    /// that name it share (`layout StoreLayout` in a page), the page shown in
    /// its `<slot />`. Composed into each page, its parts numbered first.
    Layout,
    Query,
    Command,
    Subscription,
    Resource,
    /// E6 — a materialized page or fragment.
    Materialize,
    /// E6 — a typed event a command emits and a materialization listens for.
    Event,
    /// E8 — `effect database.read<T> { capability .. host .. }`.
    ///
    /// The ONE declaration whose name is a dotted path, so that
    /// `database.read` resolves to a declaration rather than being split at
    /// the dot by whoever needs it.
    Effect,
    /// `source StoreData` (ADR-0207): a database the program's effects name,
    /// and what it guarantees. Named by nothing a program writes.
    Source,
    /// `table Todos` (track `tables`, ADR-0338): an app's data, declared: its
    /// row, key, order, indexes, references and bound. Named by nothing a
    /// program writes: its module, which has its name, holds the reads and
    /// writes generated from it (`tables::generate`).
    Table,
    /// `upload PostImage` (track `uploads`, ADR-0260): a file a form posts to
    /// its `route`, served once committed under `serves`, and its limits.
    /// Named by nothing a program writes; `pw build` writes it for the host.
    Upload,
    /// `predicate OwnsPost(post: PostId)  says "…"` (ADR-0302): a predicate a
    /// command's `requires` names, its parameters, and the words a refusal
    /// by it is told in. Its meaning is the deployment's (ADR-0115).
    Predicate,
    /// `prelude Effect` — this module exports its declarations in one
    /// namespace to every unit in the program. The namespace is the `name`.
    Prelude,

    Task,
    Type,
    Opaque,
    Let,
    /// `signal drawer: Bool` at module level (ADR-0144): a signal a page or
    /// view provides to what its body contains. Its type is `ret`.
    Signal,
    Import,
    /// A declaration form the lowering does not model yet. Recorded rather
    /// than dropped, so a checker can see that something was there.
    Other,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub decls: Vec<DeclId>,
}

/// A declaration parameter. The type is the name **as written**: resolving it
/// is a later layer's job, and a signature has no body arena to hold a
/// `TypeRefId`.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    /// The declared type, complete. `None` where the parameter carries no
    /// annotation.
    pub ty: Option<DeclaredType>,
    pub span: Span,
}

pub use type_ref::DeclaredType;

/// **A written type, kept whole.**
///
/// Its own module so the fields are private to everything, including the rest
/// of `hir.rs`. Within one crate a `pub` field is visible everywhere, so a
/// module boundary is the only thing that makes "you must ask" enforceable.
///
/// Architect ruling, 2026-08-08, after the third consumer in one day read a
/// type's head and dropped its arguments:
///
/// > Three independent consumers have now made the same mistake […] and two
/// > failed permissively. That means the representation itself is too easy to
/// > misuse. […] Preserving generic arguments should be effortless. Discarding
/// > them should require an explicit choice.
///
/// The three were `Interface::of`, where `List<OpenTransaction>` looked up
/// `List`, found no resource, and came out remotely transferable; `wit.rs`'s
/// record fields, which emitted `list` with nothing in it and which
/// `wit-parser` caught; and `koka.rs`'s `type_name`, which Koka caught. Each
/// read a `ty` beside a `ty_args` it did not read.
mod type_ref {
    /// A type as the program wrote it.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct DeclaredType {
        head: String,
        args: Vec<DeclaredType>,
    }

    impl DeclaredType {
        pub fn new(head: impl Into<String>, args: Vec<DeclaredType>) -> DeclaredType {
            DeclaredType {
                head: head.into(),
                args,
            }
        }

        /// **The whole type, as written.** `List<MenuItem>`.
        ///
        /// The default path, and the one every consumer should reach for.
        pub fn written(&self) -> String {
            // `fn(A, B) -> R`, as the grammar spells a function type: the
            // last argument is the result.
            if self.head == "fn"
                && let Some((result, params)) = self.args.split_last()
            {
                return format!(
                    "fn({}) -> {}",
                    params
                        .iter()
                        .map(Self::written)
                        .collect::<Vec<_>>()
                        .join(", "),
                    result.written()
                );
            }
            if self.args.is_empty() {
                self.head.clone()
            } else {
                format!(
                    "{}<{}>",
                    self.head,
                    self.args
                        .iter()
                        .map(Self::written)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }

        /// The complete argument types, recursively, in source order.
        ///
        /// `{#each xs as x}` needs exactly this — without it a loop binding has
        /// no type, which means a resumable handler inside a loop has no
        /// capture schema (`PW5016`), so the store demo could not use one.
        pub fn args(&self) -> &[DeclaredType] {
            &self.args
        }

        pub fn is_generic(&self) -> bool {
            !self.args.is_empty()
        }

        /// **The head, DISCARDING every argument.**
        ///
        /// Named to be unpleasant. `List<OpenTransaction>` gives `List`, which
        /// is not a type and answers no question about what the value carries —
        /// three separate consumers reached for a field that did this quietly,
        /// and two of them failed in the permissive direction.
        ///
        /// Legitimate only where the CONSTRUCTOR itself is the question: is
        /// this a `Result`, is this an `Option`. Every call site is listed in
        /// `compiler/pw-core/head-only-allow.txt` with a reason, and
        /// `tests/head_only.rs` fails on a new one.
        pub fn constructor_head_only(&self) -> &str {
            &self.head
        }
    }

    impl std::fmt::Display for DeclaredType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.written())
        }
    }
}

/// One entry of a declaration's policy block: `freshness 30.seconds`.
///
/// The value is kept **as written**. E4 gives individual policies meaning one
/// at a time, and a policy whose meaning is not yet modelled must still be
/// visible — charter §14 M4's gate says *all* query and command policies appear
/// in `pw explain`, and a policy the compiler silently dropped would not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub name: String,
    /// Everything after the keyword, trimmed. Empty when the policy is a bare
    /// word such as `offline`.
    pub value: String,
    /// **The named execution roots this policy owns.**
    ///
    /// `optimistic Cart(..) as cart => ..` owns two: the target selector and
    /// the transition. They are separate computations with separate contexts
    /// (ADR-0025), so they are separate roots rather than one tree with a
    /// special case inside it.
    ///
    /// Architect ruling, 2026-08-11, generalizing what the special case had
    /// already proved:
    ///
    /// > Your special-cased transition has already proved the abstraction. Make
    /// > it the first instance rather than leaving it exceptional.
    ///
    /// Architect ruling, 2026-08-10:
    ///
    /// > `optimistic` and `rollback` are different: their contents are actual
    /// > Pleris programs and must go through the full normal semantic
    /// > pipeline.
    ///
    /// and 2026-08-11, on what the value must identify:
    ///
    /// > An optimistic clause identifies a resource entry and binds its current
    /// > value; its body is an ordinary Pleris transition expression.
    ///
    /// Every root points into the OWNING DECLARATION'S body arena, and none is
    /// reachable from that body's root. They are terms, so name resolution must
    /// see them; they run in a different execution context, so the
    /// declaration's effect row must not absorb them. Reachable-from-root would
    /// have given the second for free along with the first.
    pub roots: Vec<TermRoot>,
    /// **The declarations this clause names, each with its key** (ADR-0088):
    /// `depends_on Store(id), Menu(id)` names two. Only a clause
    /// `crate::policy::keyed` describes has any. Each key's arguments are
    /// also roots, in the `Key` context: they are terms, and the name is not.
    pub keys: Vec<ClauseKey>,
    pub span: Span,
}

/// **A declaration a clause names, and its key** (ADR-0088):
/// `Cart(current_session())` in `invalidates Cart(current_session())`. The
/// name is looked up by the clause's kind, never as a term; the arguments are
/// terms. A name written alone has no arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClauseKey {
    pub name: String,
    pub name_span: Span,
    /// The name and its arguments, as written.
    pub span: Span,
    pub args: Vec<Arg>,
    /// Each argument as written, its whitespace collapsed as a policy's
    /// value's is: what reads a key as text, the resource graph's edge
    /// labels, reads it here (ADR-0240). The graph split the clause's text
    /// at its commas until then, a second reading of it.
    pub written: Vec<String>,
}

/// **What every value of an opaque type holds** (ADR-0179): `where value >=
/// 1`, read as bounds on its `value`, each end included; or, of a `String`,
/// `where String.length(value) <= 280`, bounds on its length in code points
/// (ADR-0225). Bounds alone: the build decides a construction against them,
/// and a host checks a value it is given against them without running the
/// program's code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    /// What the bounds are on: the value, or its length (ADR-0225).
    pub measure: Measure,
    /// The least value, where the predicate states one.
    pub at_least: Option<i128>,
    /// The greatest value, where the predicate states one.
    pub at_most: Option<i128>,
    /// The predicate as written: `value >= 1`.
    pub written: String,
    pub span: Span,
    /// Each part of the predicate that is no bound on `value`, and why
    /// (PW0623).
    pub unread: Vec<(Span, String)>,
}

/// **What an invariant bounds** (ADR-0225): an `Int`'s value, or a
/// `String`'s length, in code points, as `String.length` counts them
/// (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord, Hash)]
pub enum Measure {
    /// `value >= 1`.
    #[default]
    Value,
    /// `String.length(value) <= 280`.
    Length,
}

impl Invariant {
    /// Does `n` hold it?
    pub fn holds(&self, n: i128) -> bool {
        self.at_least.is_none_or(|lo| n >= lo) && self.at_most.is_none_or(|hi| n <= hi)
    }
}

/// **Where an expression tree runs.**
///
/// Architect ruling, 2026-08-11:
///
/// > A `TermRoot` should own an expression tree, its lexical binders, and its
/// > initial `ExecutionContext`. […] That gives the compiler one invariant:
/// > **every executable expression belongs to exactly one named execution
/// > root.**
///
/// Nested constructs — a frame phase — still push onto the context stack; they
/// do not become separate roots. A root is where a *tree* begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionContext {
    /// The declaration's own body: a command, a query, a rendered view.
    Declaration,
    /// Selecting which resource entry an optimistic clause targets.
    ///
    /// Its own context because it is not the state transformation. It may read
    /// invocation context to name the entry — `Cart(current_session())` — and
    /// that is exactly what the transformation may not do (ADR-0025).
    TargetSelection,
    /// `optimistic .. => ..` — on the client, before the round trip.
    OptimisticTransition,
    /// `draw(ctx) { .. }` — a painter.
    Draw,
    /// `acquire { .. }` — a resource's acquisition.
    Acquire,
    /// `release(h) { .. }` — a resource's release.
    Release,
    /// **A clause's key** (ADR-0088): an argument of `Cart(current_session())`
    /// in `invalidates Cart(current_session())`. It names an entry or an
    /// event where the declaration runs. Like an optimistic clause's target,
    /// it may read invocation context, and its effects are not the
    /// declaration's work.
    Key,
    /// **An event's key** (ADR-0208): an argument of
    /// `CartChanged(current_session())` in `emits CartChanged(..)`. The
    /// command evaluates it, before its body, and hands the event to the
    /// outbox: so what it performs is the command's work (ADR-0195, ruling
    /// 9).
    Emitted,
    /// **An invalidated entry's key** (ADR-0209): an argument of
    /// `Cart(current_session())` in `invalidates Cart(..)`. The command
    /// evaluates it, before its body, and hands the entry to the outbox, so
    /// what it performs is the command's work too.
    Invalidated,
    /// **A listener's argument** (ADR-0091): `id` in `invalidates_on
    /// MenuChanged(id)`. The declaration's parameter an event's value must
    /// equal, or `_`. Nothing in it is evaluated; it says which part of an
    /// entry's key a value is compared with.
    Listener,
}

impl ExecutionContext {
    /// The world this context runs in, where it has one.
    pub fn placement(self) -> Option<&'static str> {
        match self {
            ExecutionContext::OptimisticTransition | ExecutionContext::Draw => Some("browser"),
            _ => None,
        }
    }

    /// **May a tree in this context perform effects?**
    ///
    /// `false` for an optimistic transition, and the reason is the whole of
    /// ADR-0025: the platform abandons a speculative value by restoring the one
    /// it held, and an externally visible effect cannot be abandoned that way.
    ///
    /// A context policy rather than a rule keyed on the policy's spelling —
    /// architect ruling, 2026-08-11: *the existing verdict remains; the
    /// mechanism becomes general.* `Draw`, `Acquire` and `Release` deliberately
    /// get no new legality rules here; they carry only the semantics they
    /// already had.
    pub fn permits_effects(self) -> bool {
        !matches!(self, ExecutionContext::OptimisticTransition)
    }

    /// **Do this root's effects belong to the declaration that owns it?**
    ///
    /// `true` for a painter's `draw`, a resource's `acquire` and `release`:
    /// those blocks ARE the declaration's work, and they were statements in its
    /// body until they became roots. Preserving that is the migration being
    /// faithful — architect ruling, 2026-08-11: *for `Draw`, `Acquire`, and
    /// `Release`, don't invent new legality rules now; encode only the
    /// semantics/rules they already have.*
    ///
    /// `false` for an optimistic clause. It runs on the client at a different
    /// time, and a command whose row absorbed it would be indistinguishable
    /// from one that performed the transition itself.
    pub fn contributes_to_declaration(self) -> bool {
        matches!(
            self,
            ExecutionContext::Draw
                | ExecutionContext::Acquire
                | ExecutionContext::Release
                | ExecutionContext::Emitted
                | ExecutionContext::Invalidated
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            ExecutionContext::Declaration => "the declaration body",
            ExecutionContext::TargetSelection => "an optimistic clause's target",
            ExecutionContext::OptimisticTransition => "an optimistic transition",
            ExecutionContext::Draw => "a painter's draw block",
            ExecutionContext::Acquire => "a resource's acquire block",
            ExecutionContext::Release => "a resource's release block",
            ExecutionContext::Key => "a clause's key",
            ExecutionContext::Emitted => "an event's key",
            ExecutionContext::Invalidated => "an invalidated entry's key",
            ExecutionContext::Listener => "a listener's argument",
        }
    }
}

/// **One named execution root: a tree, its binders, and where it runs.**
///
/// The binders are the root's OWN — `optimistic Cart(..) as cart => ..` binds
/// `cart` for the transition and for nothing else. Downstream resolution
/// receives them already lowered; nothing recognises a policy by spelling to
/// decide what a block introduces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRoot {
    pub context: ExecutionContext,
    /// Names this root introduces, with their spans.
    pub binders: Vec<(String, Span)>,
    pub root: ExprId,
}

/// One constructor of a `type T = | A | B(X)` declaration.
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    /// Each payload's type, as written: a tree, so `Found(List<Entry>)`
    /// keeps its argument (ADR-0059). They were spellings, re-parsed by each
    /// reader, as an opaque type's representation was (ADR-0054).
    pub fields: Vec<DeclaredType>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    /// Where the name itself is written, not the whole declaration.
    ///
    /// A diagnostic that says "`Cart` is declared here, so its result is
    /// `Session<SessionId>`" wants to underline the name. Pointing at the whole
    /// declaration says the same words about a twenty-line span, which is how a
    /// precise message becomes a vague one.
    pub name_span: Span,
    pub kind: DeclKind,
    pub params: Vec<Param>,
    /// The entire declared return annotation, including nested arguments.
    /// `None` means no annotation, not an annotation that failed to resolve.
    pub ret: Option<DeclaredType>,
    /// The variants, when this declaration defines an algebraic data type.
    pub variants: Option<Vec<VariantDef>>,
    /// The fields, when this declaration defines a record.
    pub fields: Option<Vec<Param>>,
    /// The declaration's policy block, in source order.
    pub policies: Vec<Policy>,
    /// For an `import`, the names it brings into scope: `import domain.{ A, B }`
    /// yields `["A", "B"]`. Empty for a whole-module import.
    pub imports: Vec<String>,
    /// `public`, `session`, `private` — the visibility keyword as written.
    /// This is where a declaration's privacy label starts (charter §7.8).
    pub visibility: Option<String>,
    /// `opaque type StoreId = String` — the representation, as written: a
    /// tree, so `opaque type Names = List<String>` keeps its argument
    /// (ADR-0054). It was a spelling, and a generic one never resolved.
    pub opaque_of: Option<DeclaredType>,
    /// `opaque type PositiveInt = Int where value >= 1`: what every value of
    /// the type holds (ADR-0179), read as bounds on its `value`.
    pub invariant: Option<Invariant>,
    /// The type parameters this declaration binds: `["C"]` for
    /// `opaque type Secret<C>`, `["T"]` for `effect database.read<T>`.
    ///
    /// Recorded because ARITY is a checkable property and the tree already
    /// knows it. `database.read` written with no argument and
    /// `database.read<A, B>` written with two are both wrong against a
    /// declaration binding one, and a checker that had to count `<`s in a
    /// string would be parsing the source a second time.
    pub type_params: Vec<String>,
    /// The declared effect row, as written: `!{ database.read<Stores> }` yields
    /// `["database.read"]`. Empty for `!{}`; `None` when no row was written.
    pub declared_effects: Option<Vec<EffectRef>>,
    pub body: Option<BodyId>,
    /// **The arena an interface's clause terms are in** (ADR-0240). A
    /// declaration with a body lowers its clauses' terms into the body's
    /// arena; one with none, an interface, had nowhere to, and its keys were
    /// never lowered: not resolved, not counted, not typed. `None` where
    /// there is a body, or no clause with terms. Read through
    /// [`Decl::terms_body`].
    pub terms: Option<BodyId>,
    /// Declarations nested inside this one, e.g. a `fn` inside a `component`.
    pub children: Vec<DeclId>,
    /// `let mut x = ..` at module level: a binding a body may assign
    /// (ADR-0051). False for every other declaration.
    pub mutable: bool,
}

impl Decl {
    /// **The arena this declaration's clause terms are in** (ADR-0240): its
    /// body's, or an interface's own. What reads a term root or a key's
    /// argument reads it from here; `body` says whether the declaration is
    /// implemented, which an interface's terms do not make it.
    pub fn terms_body(&self) -> Option<BodyId> {
        self.body.or(self.terms)
    }

    /// The value of a named policy, if the declaration declares it.
    pub fn policy(&self, name: &str) -> Option<&Policy> {
        self.policies.iter().find(|p| p.name == name)
    }

    /// **Every named execution root this declaration owns**, other than its
    /// body.
    ///
    /// The explicit way to reach an embedded tree. A consumer that should see
    /// them asks; one that should not, does not — and neither finds them by
    /// accident, because they are not reachable from the body's root.
    pub fn term_roots(&self) -> impl Iterator<Item = (&Policy, &TermRoot)> {
        self.policies
            .iter()
            .flat_map(|p| p.roots.iter().map(move |r| (p, r)))
    }

    /// The roots of one context.
    pub fn roots_in(
        &self,
        context: ExecutionContext,
    ) -> impl Iterator<Item = (&Policy, &TermRoot)> {
        self.term_roots().filter(move |(_, r)| r.context == context)
    }

    /// An optimistic clause as its two roots, paired.
    ///
    /// The target and the transition belong to one policy and are separate
    /// computations; every rule about them needs both, and pairing them here
    /// keeps a caller from matching them up by position.
    pub fn optimistic_clauses(&self) -> Vec<(&Policy, &TermRoot, &TermRoot)> {
        // Each arm's target and the transition after it (ADR-0238): a clause
        // may speculate on several entries.
        let mut out = Vec::new();
        for p in &self.policies {
            let mut target = None;
            for r in &p.roots {
                match r.context {
                    ExecutionContext::TargetSelection => target = Some(r),
                    ExecutionContext::OptimisticTransition => {
                        if let Some(t) = target.take() {
                            out.push((p, t, r));
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

/// One entry in an effect row, with the span of the entry itself so a
/// diagnostic can underline the specific effect rather than the whole row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRef {
    /// The effect's name without type arguments: `style.mutate`. Used for
    /// family grouping.
    pub path: String,
    /// The effect **as written**: `style.mutate<LayoutAffect>`. Charter §7.5A
    /// makes the type argument load-bearing — a write that can invalidate
    /// layout is a different effect from one that cannot — so the identity a
    /// checker compares and the text a developer reads must both keep it.
    pub written: String,
    /// The type arguments, **as the parser found them**: `["LayoutAffect"]`.
    ///
    /// Lowered from the `TypeArgList` the grammar already builds, not sliced
    /// out of `written` afterwards. Two arguments are two entries, which is
    /// what makes `database.read<A, B>` distinguishable from a one-argument
    /// effect whose argument happens to contain a comma.
    pub args: Vec<String>,
    pub span: Span,
}

// --- bodies --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Body {
    pub owner: DeclId,
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pattern>,
    pub types: Arena<TypeRef>,
    /// Markup nodes. Separate from `exprs` because a renderer walks markup and
    /// an effect checker walks expressions, and neither wants the other's tree.
    pub nodes: Arena<Node>,
    /// The block expression the body consists of.
    pub root: ExprId,
    /// **The `let`s that are signals** (ADR-0130): `signal x: T = e` lowers
    /// to the `let` it is, and this says which. A side set rather than a
    /// field on `Expr::Let`, so every rule about a binding reads a signal
    /// as the binding it is, and the rules about signals ask here.
    pub signals: std::collections::BTreeSet<ExprId>,
    /// **A lambda parameter's written type**, `e: InputEvent` in `(e:
    /// InputEvent) => ..`, by the pattern it annotates (ADR-0138). A side
    /// table, as `signals` is, so the many readers of a lambda's patterns are
    /// unchanged. Until 2026-10-02 the annotation was dropped, and a written
    /// type checked nothing.
    pub param_types: std::collections::BTreeMap<PatternId, TypeRefId>,
    /// **The handlers `bind:value` wrote** (ADR-0142): `bind:value={s}` is
    /// lowered to `value={s}` and an `on:input` handler setting `s`, and this
    /// says which handlers are those, for the rule on what a binding binds.
    pub bound: std::collections::BTreeSet<ExprId>,
    /// **The assignments that are `provide`s** (ADR-0144): `provide drawer =
    /// false` lowers to the assignment `drawer = false` of the signal its
    /// module declares, and this says which. So the name resolves, and the
    /// value is checked against the signal's type, as any assignment's is;
    /// the rules about signals ask here.
    pub provides: std::collections::BTreeSet<ExprId>,
}

/// The name a `bind:value` handler gives its event (ADR-0142): one no source
/// can write, so it hides nothing the page binds.
pub const BOUND_EVENT: &str = "event~bound";

impl Body {
    pub fn expr(&self, id: ExprId) -> &Expr {
        self.exprs
            .get(id.index())
            .expect("ExprId from another body")
    }

    pub fn expr_span(&self, id: ExprId) -> Span {
        self.exprs
            .span_at(id.index())
            .expect("ExprId from another body")
            .clone()
    }

    pub fn pat(&self, id: PatternId) -> &Pattern {
        self.pats
            .get(id.index())
            .expect("PatternId from another body")
    }

    pub fn node(&self, id: NodeId) -> &Node {
        self.nodes
            .get(id.index())
            .expect("NodeId from another body")
    }

    pub fn node_span(&self, id: NodeId) -> Span {
        self.nodes
            .span_at(id.index())
            .expect("NodeId from another body")
            .clone()
    }

    /// Every markup node under `id`, parents before children.
    pub fn walk_markup(&self, roots: &[NodeId]) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            let kids = match self.node(id) {
                Node::Element { children, .. } | Node::Block { children, .. } => children.clone(),
                _ => vec![],
            };
            stack.extend(kids.into_iter().rev());
        }
        out
    }

    pub fn pat_span(&self, id: PatternId) -> Span {
        self.pats
            .span_at(id.index())
            .expect("PatternId from another body")
            .clone()
    }

    /// Every expression in the body, in allocation order.
    pub fn exprs(&self) -> impl Iterator<Item = (ExprId, &Expr, &Span)> {
        self.exprs.iter().map(|(i, e, s)| (ExprId(i as u32), e, s))
    }

    /// The direct children of an expression. Written once here so no analysis
    /// has to re-derive the traversal and quietly miss a variant — the
    /// generic-callback case fails exactly that way.
    pub fn children(&self, id: ExprId) -> Vec<ExprId> {
        match self.expr(id) {
            Expr::Name(_) | Expr::Literal(_) | Expr::Error => vec![],
            Expr::Field { base, .. } => vec![*base],
            Expr::Call { callee, args } => {
                let mut v = vec![*callee];
                v.extend(args.iter().map(|a| a.value));
                v
            }
            // Its arguments, which are read; the page it names is no value.
            Expr::Address { args, .. } => args.iter().map(|a| a.value).collect(),
            Expr::Lambda {
                descriptor, body, ..
            } => {
                let mut v: Vec<ExprId> = descriptor.iter().copied().collect();
                v.push(*body);
                v
            }
            Expr::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            Expr::Cast { value, .. } => vec![*value],
            Expr::Try { value } => vec![*value],
            Expr::Interpolated { parts, .. } => parts.clone(),
            Expr::Unary { operand, .. } => vec![*operand],
            Expr::Block { stmts } => stmts.clone(),
            Expr::If { cond, then, els } => {
                let mut v = vec![*cond, *then];
                v.extend(els.iter().copied());
                v
            }
            Expr::Match { scrutinee, arms } => {
                let mut v = vec![*scrutinee];
                v.extend(arms.iter().map(|a| a.body));
                v
            }
            Expr::For { iterable, body, .. } => vec![*iterable, *body],
            Expr::Record { fields, .. } => fields.iter().filter_map(|f| f.value).collect(),
            Expr::List { items } => items.clone(),
            Expr::Let { init, .. } => init.iter().copied().collect(),
            Expr::Keyword { args, block, .. } => {
                let mut v = args.clone();
                v.extend(block.iter().copied());
                v
            }
            Expr::Template { parts, .. } => parts.clone(),
        }
    }

    /// Every expression reachable from `root`, parents before children.
    /// The source text of a string expression, whether or not it has holes.
    ///
    /// A route is written `route "/stores/{id}"`, which is a string WITH a
    /// hole — so a consumer that matched only `Literal::Str` stopped seeing
    /// routes the moment holes became expressions. Anything that wants the
    /// characters should ask for them here rather than match one variant.
    pub fn string_text(&self, id: ExprId) -> Option<&str> {
        match self.expr(id) {
            Expr::Literal(Literal::Str(s)) => Some(s),
            Expr::Interpolated { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn walk(&self) -> Vec<ExprId> {
        self.walk_from(self.root)
    }

    /// The same walk, rooted at an arbitrary expression. Used where a check is
    /// about one subtree — the value of a single attribute, say — rather than
    /// about the whole body.
    pub fn walk_from(&self, root: ExprId) -> Vec<ExprId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            out.push(id);
            let kids = self.children(id);
            stack.extend(kids.into_iter().rev());
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(String),
    Float(String),
    Str(String),
    /// A string whose closing quote is missing. Kept as a literal so a body
    /// containing one still lowers.
    UnterminatedStr(String),
    /// **`()`, the unit value** (ADR-0200), the one value of the type `()`.
    /// Until ADR-0200 it lowered to an error node, which nothing reported.
    Unit,
}

impl Literal {
    /// **The value a string literal denotes**: `"espresso"` is `espresso`,
    /// and `"a\nb"` is `a`, a line feed and `b` (ADR-0049).
    ///
    /// A `Literal::Str` holds its source TOKEN, quotes and escapes as written;
    /// `pw_syntax::strings` is the one reading of it, which every backend
    /// shares. Until 2026-09-25 a token with a backslash, or a `"""` string,
    /// had no value here (A-023), and backends refused it or passed it to
    /// their targets' rules. `None` now only for a token the grammar already
    /// refused (PW0014), or one with a hole, which lowers as
    /// `Expr::Interpolated` instead.
    pub fn string_value(&self) -> Option<String> {
        let Literal::Str(token) = self else {
            return None;
        };
        pw_syntax::strings::value(token).ok().flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// `==` `!=` `<=` `>=` `<` `>`
    Cmp(String),
    And,
    Or,
    /// `a |> f`. Not desugared to a call: a diagnostic pointing at a
    /// synthesised call the author never wrote is worse than one pointing at
    /// `|>` (ADR-0014).
    Pipe,
    /// `a -> b` in an animation keyframe sequence.
    Transition,
    Assign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    /// `max = 3` and `jitter: true` carry a name; positional arguments do not.
    pub name: Option<String>,
    pub value: ExprId,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pat: PatternId,
    pub body: ExprId,
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: String,
    /// `Point { x, y }` shorthand has no value expression.
    pub value: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// A bare identifier. `a.b` is `Field { base: Name("a"), name: "b" }` —
    /// lowering does not decide whether that is a module path (ADR-0014).
    Name(String),
    Literal(Literal),
    Field {
        base: ExprId,
        name: String,
    },
    Call {
        callee: ExprId,
        args: Vec<Arg>,
    },
    /// **A page's address** (ADR-0311): `{StorePage(id)}` in a canonical
    /// link's `href`, a page named by its declaration and given its
    /// parameters, as `navigate` names one (ADR-0280). Not a call: it names a
    /// page and runs nothing, so nothing that reads a call (what it performs,
    /// what it reads, the label it carries) reads it as one. Until it was its
    /// own node a shared page naming a session page's address was refused as
    /// the session's (PW5001).
    Address {
        /// The page, as written: `StorePage`, or `store.StorePage`.
        page: String,
        args: Vec<Arg>,
    },
    Lambda {
        /// An annotation in parameter position: `resumable(captures = { item })
        /// => ..`. It is call-shaped, so it is kept as an expression rather
        /// than forced into a pattern. R-010 and R-030 — a non-serializable
        /// capture and private data in a resume manifest — are checks *on this
        /// expression*, so dropping it would make them unimplementable.
        descriptor: Option<ExprId>,
        params: Vec<PatternId>,
        body: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    /// `raw as Store`. The target is a type, not an expression.
    Cast {
        value: ExprId,
        ty: TypeRefId,
    },
    /// `e?`: the success value of `e`, returning its failure — an `Err` or a
    /// `None` — from the enclosing declaration.
    Try {
        value: ExprId,
    },
    /// A string with `{expr}` holes: `"charging {token} now"`.
    ///
    /// The holes are real expressions with real spans, not text a checker
    /// searches. Before this existed, the privacy-sink rule read `{name}` out
    /// of the literal's characters, so `{token.value}` was invisible to it —
    /// a value could leave through a hole the analysis could not see into.
    Interpolated {
        text: String,
        parts: Vec<ExprId>,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    Block {
        stmts: Vec<ExprId>,
    },
    If {
        cond: ExprId,
        then: ExprId,
        els: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
    },
    /// `for x in xs { .. }`.
    ///
    /// **The pattern binds.** It was `Call { callee: Name("for") }` until
    /// 2026-08-10, which had two consequences: every analysis that walks calls
    /// saw a call to something no program declares, and the loop variable was
    /// introduced by nothing — so `for badge in badges { badge.width() }` made
    /// `badge` look like an undeclared name. Architect ruling: *syntax must
    /// remain syntax; terms must remain terms.*
    For {
        pat: Option<PatternId>,
        iterable: ExprId,
        body: ExprId,
    },
    Record {
        /// `None` for a bare `{ a: 1 }` with no type name in front.
        name: Option<String>,
        fields: Vec<FieldInit>,
    },
    List {
        items: Vec<ExprId>,
    },
    Let {
        pat: Option<PatternId>,
        ty: Option<TypeRefId>,
        init: Option<ExprId>,
    },
    /// The body-level statement family: `observe resize`, `frame { .. }`,
    /// `unsafe.imperative { .. }`, `use key: Secret<T> = ..`. One node kind
    /// because E2 recognises them and E4 gives them meaning.
    Keyword {
        /// `unsafe.imperative` keeps its qualification.
        keyword: String,
        /// A `because "…"` string, as written. Charter §14 M5 task 6 makes it
        /// mandatory on an escape hatch, so it is a field rather than an
        /// argument: a rule should not have to guess which argument it was.
        justification: Option<String>,
        /// The `attributes_…_to X` / `attributed_to X` target — who owns the
        /// consequence. An audit record needs both halves: a reason nobody is
        /// answerable for is a comment, not an audit.
        attribution: Option<String>,
        /// The bare modifier words between the keyword and any punctuation:
        /// `unsafe capability synchronous_geometry` yields
        /// `["capability", "synchronous_geometry"]`.
        modifiers: Vec<String>,
        args: Vec<ExprId>,
        block: Option<ExprId>,
    },
    /// A markup region: the roots of its element tree.
    ///
    /// `parts` keeps the interpolated expressions in source order so an effect
    /// or privacy walk can reach them without descending the markup, which is
    /// what every analysis before E3 actually wanted.
    Template {
        parts: Vec<ExprId>,
        roots: Vec<NodeId>,
    },
    /// Syntax that could not be lowered. Carries its span so a checker can
    /// still report position, and keeps lowering total (ADR-0014).
    Error,
}

// --- markup ---------------------------------------------------------------

/// An attribute's value.
#[derive(Debug, Clone)]
pub enum AttrValue {
    /// `class="card"`
    Static(String),
    /// `on:press={handler}` — a real expression, so a checker can look inside.
    Expr(ExprId),
    /// `disabled` — present with no value.
    None,
}

#[derive(Debug, Clone)]
pub struct Attr {
    /// The name as written, namespace included: `on:press`, `aria-label`.
    pub name: String,
    pub value: AttrValue,
    pub span: Span,
}

impl Attr {
    /// `on:press` -> `Some(("on", "press"))`.
    pub fn namespace(&self) -> Option<(&str, &str)> {
        self.name.split_once(':')
    }

    /// **An `on:` attribute's event and its modifiers** (ADR-0131):
    /// `on:submit|prevent` -> `Some(("submit", ["prevent"]))`. `None` for
    /// any other attribute.
    pub fn event(&self) -> Option<(&str, Vec<&str>)> {
        let rest = self.name.strip_prefix("on:")?;
        let mut parts = rest.split('|');
        let event = parts.next()?;
        Some((event, parts.collect()))
    }
}

/// **What an event's modifiers may say** (ADR-0131): what the runtime does
/// before any code loads, in the listener. `prevent` stops the browser's own
/// action, a form's submission or a link's navigation; `stop` keeps the event
/// from reaching an enclosing element's handler. And `refusable` (ADR-0307),
/// which the runtime does nothing for: the control is shown to every reader,
/// though its command requires a predicate the page does not ask (a part
/// served to everyone cannot), and a refusal is told where the press was
/// (ADR-0302).
pub const EVENT_MODIFIERS: &[&str] = &["prevent", "stop", "refusable"];

#[derive(Debug, Clone)]
pub enum Node {
    Element {
        /// `p`, or `store.Card` for a component.
        tag: String,
        attrs: Vec<Attr>,
        children: Vec<NodeId>,
        self_closing: bool,
    },
    /// Character data, as written.
    Text(String),
    /// `{expr}` between tags.
    Interpolation(ExprId),
    /// `{#each items as x (x.id)} .. {/each}`. The directive is kept verbatim
    /// because E3 does not yet own its semantics and inventing a structure for
    /// it now would be guessing.
    Block {
        directive: String,
        /// Everything between the opening and closing markers, in order, with
        /// each `{:..}` marker kept at its place as a [`Node::Branch`]
        /// (ADR-0042). Until 2026-09-25 the markers were dropped, so
        /// `{#if a}A{:else}B{/if}` rendered A and B together.
        children: Vec<NodeId>,
        /// `c` in `{#if c}`, `e` in `{#match e}`: a real expression, so the
        /// value relations and the privacy walks see it. `None` for `{#each}`,
        /// whose head is `each`.
        subject: Option<ExprId>,
        /// An `{#each}`'s head, parsed once by the grammar (ADR-0242), its
        /// list and key expressions as `subject` is (ADR-0345). `None` for
        /// any other block, or a head that did not parse, whose error is
        /// reported.
        each: Option<EachHead>,
        /// The closing marker as written, `{/if}`; empty when the block was
        /// never closed.
        close: String,
    },
    /// `{:else}`, `{:else if c}`, `{:Some(x)}`: one of a block's branch
    /// markers, at its place among the block's children (ADR-0042).
    Branch {
        /// As written.
        marker: String,
        /// `c` in `{:else if c}`.
        condition: Option<ExprId>,
        /// `{:Some(x)}`, `{:None}`, `{:Shape.Rect(w, h)}`: a `{#match}` arm.
        arm: Option<TemplateArm>,
    },
}

/// **An `{#each}`'s head** (ADR-0242): `menu as item (item.id)`, parsed once
/// by the grammar.
///
/// **Its list and its key are expressions in the body's arena** (ADR-0345),
/// as an `{#if}`'s condition is: what the template evaluates when it renders,
/// read by every walker of a view's expressions. Until 2026-10-15 they were
/// text, which only the readers that split it read, so no effect, privacy,
/// signal or value walk saw them: a view declared `!{}` read the database in
/// `{#each items() as item}`, and in `{#each store.items as item}` through an
/// effectful member, and checked clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EachHead {
    /// The list the rows are read from, read in the scope around the block.
    pub list: ExprId,
    /// The name each row binds.
    pub binder: String,
    pub binder_span: Span,
    /// What tells the rows apart, read in the row's scope, where `binder`
    /// is bound.
    pub key: Option<ExprId>,
    /// The list and the key as written, whitespace collapsed: what a
    /// message quotes, and nothing else. What the head means is `list` and
    /// `key`.
    pub written: EachWritten,
}

/// [`EachHead::written`]: `menu` and `item.id` in `{#each menu as item
/// (item.id)}`, for a message to quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EachWritten {
    pub list: String,
    pub key: Option<String>,
}

/// Declares [`Position`] and [`Position::ALL`] from one list, so a position
/// cannot be added to the one and left out of the other.
macro_rules! positions {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// **Where in markup an expression is written** (ADR-0345): every
        /// position a [`Node`] holds one at, each its own variant.
        /// [`Node::exprs`] is the one reading of them, and
        /// [`Expr::Template`]'s `parts` are what it reads in a region, so
        /// every walker of a body's expressions sees each position. A
        /// position an analysis reads differently is a variant it matches,
        /// without a wildcard.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Position {
            $($(#[$doc])* $name,)*
        }

        impl Position {
            /// Every position, in the order declared.
            pub const ALL: &'static [Position] = &[$(Position::$name,)*];
        }
    };
}

positions! {
    /// `{e}` between tags: written as text.
    Hole,
    /// `name={e}` or `name="..{e}.."`: an element's attribute other than an
    /// event's, or a component's argument.
    Attribute,
    /// `on:press={e}`: an event's handler, run when a person acts, not
    /// while the view renders.
    Handler,
    /// `c` in `{#if c}`, `e` in `{#match e}`: what a block decides by.
    Subject,
    /// `c` in `{:else if c}`.
    Condition,
    /// `xs` in `{#each xs as x}`: the list the rows are read from.
    List,
    /// `k` in `{#each xs as x (k)}`: what tells the rows apart, read where
    /// `x` is bound.
    Key,
}

impl Node {
    /// **Every expression this node holds, and where** (ADR-0345), in the
    /// order written. One exhaustive reading: each variant and each field
    /// named, and no wildcard, so a field that holds an expression cannot be
    /// added without deciding here where it is. A node's children are their
    /// own nodes, read on their own.
    pub fn exprs(&self) -> Vec<(Position, ExprId)> {
        match self {
            Node::Element {
                tag: _,
                attrs,
                children: _,
                self_closing: _,
            } => attrs
                .iter()
                .filter_map(|a| {
                    let Attr {
                        name: _,
                        value,
                        span: _,
                    } = a;
                    let position = match a.event() {
                        Some(_) => Position::Handler,
                        None => Position::Attribute,
                    };
                    match value {
                        AttrValue::Expr(e) => Some((position, *e)),
                        AttrValue::Static(_) | AttrValue::None => None,
                    }
                })
                .collect(),
            Node::Text(_) => Vec::new(),
            Node::Interpolation(e) => vec![(Position::Hole, *e)],
            Node::Block {
                directive: _,
                children: _,
                subject,
                each,
                close: _,
            } => {
                let mut out: Vec<(Position, ExprId)> =
                    subject.iter().map(|s| (Position::Subject, *s)).collect();
                if let Some(EachHead {
                    list,
                    binder: _,
                    binder_span: _,
                    key,
                    written: _,
                }) = each
                {
                    out.push((Position::List, *list));
                    out.extend(key.iter().map(|k| (Position::Key, *k)));
                }
                out
            }
            Node::Branch {
                marker: _,
                condition,
                arm,
            } => {
                // An arm binds names, and holds no expression.
                if let Some(TemplateArm {
                    case: _,
                    bindings: _,
                }) = arm
                {}
                condition
                    .iter()
                    .map(|c| (Position::Condition, *c))
                    .collect()
            }
        }
    }
}

/// **A `{#match}` arm's marker** (ADR-0042, ADR-0061): the case it takes,
/// and the name each field of the case's payload is bound to.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateArm {
    /// As written: `Some`, `Circle`, or `Shape.Circle` through its type.
    pub case: String,
    /// One name per field, in order: `x` in `{:Some(x)}`, `w` and `h` in
    /// `{:Rect(w, h)}`; none for `{:None}`.
    pub bindings: Vec<String>,
}

impl TemplateArm {
    /// The case's own name, without the type a qualified marker names.
    pub fn short(&self) -> &str {
        self.case.rsplit_once('.').map_or(&self.case, |(_, c)| c)
    }
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_`
    Wild,
    /// A binding: `x`, or `mut x`.
    Bind {
        name: String,
        mutable: bool,
    },
    /// A constructor pattern: `Some(x)`, `DecodeError.Invalid(a, b)`.
    Ctor {
        path: String,
        args: Vec<PatternId>,
    },
    Literal(Literal),
    /// `a | b`
    Or(Vec<PatternId>),
    Error,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    /// `List`, `store.Order`, `()` for unit.
    pub path: String,
    pub args: Vec<TypeRefId>,
}

// --- the program ---------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Hir {
    pub modules: Arena<Module>,
    pub decls: Arena<Decl>,
    pub bodies: Arena<Body>,
    /// **What lowering parsed and could not read** (ADR-0237): the errors of
    /// its standalone parses, of what the declaration grammar keeps as text —
    /// a clause's value, a string's hole, a block marker's expression — each
    /// span in the file. The checker reports them as it does the file's.
    pub syntax: Vec<pw_syntax::SyntaxError>,
    /// **The declarations the compiler wrote, and what for** (track
    /// `tables`, ADR-0338): each function and record a table's declaration
    /// gives its module (`tables::generate`), by its id. A declaration not
    /// here is the program's own.
    pub generated: std::collections::BTreeMap<DeclId, crate::tables::Generated>,
    /// Upload resource declarations synthesized by the same lowering pass.
    pub generated_uploads: std::collections::BTreeMap<DeclId, crate::uploads::Generated>,
}

impl Hir {
    pub fn decl(&self, id: DeclId) -> &Decl {
        self.decls.get(id.index()).expect("unknown DeclId")
    }

    pub fn decl_span(&self, id: DeclId) -> Span {
        self.decls
            .span_at(id.index())
            .expect("unknown DeclId")
            .clone()
    }

    pub fn body(&self, id: BodyId) -> &Body {
        self.bodies.get(id.index()).expect("unknown BodyId")
    }

    /// The module a declaration belongs to.
    ///
    /// Inference needs this, so it is an input to inference rather than
    /// something bolted on afterwards — see `Types::of_body`.
    pub fn module_of(&self, decl: DeclId) -> Option<&str> {
        if let Some((_, m, _)) = self
            .modules
            .iter()
            .find(|(_, m, _)| m.decls.contains(&decl))
        {
            return Some(m.name.as_str());
        }
        // A declaration nested in another is in that one's module. A module
        // lists only its top-level declarations, so until 2026-09-26 a nested
        // one had none, and every annotation it wrote was left unresolved
        // ("the annotation's declaring module is unavailable", ADR-0066).
        let parent = self
            .all_decls()
            .find(|(_, d)| d.children.contains(&decl))
            .map(|(p, _)| p)?;
        self.module_of(parent)
    }

    /// Every declaration, nested ones included.
    pub fn all_decls(&self) -> impl Iterator<Item = (DeclId, &Decl)> {
        self.decls.iter().map(|(i, d, _)| (DeclId(i as u32), d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arena_keeps_a_span_for_every_node() {
        let mut a: Arena<&str> = Arena::new();
        let i = a.alloc("x", 3..7);
        assert_eq!(a.get(i as usize), Some(&"x"));
        assert_eq!(a.span_at(i as usize), Some(&(3..7)));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn only_lower_touches_syntax_nodes() {
        // ADR-0014 property 3, enforced rather than trusted. A checker that
        // reaches into the syntax tree would compile and pass its own tests
        // while quietly re-coupling the layers.
        //
        // The check looks at `use` declarations, not at prose: a first version
        // searched the whole file for "SyntaxNode" and "green" and flagged
        // seven modules that only mentioned them in comments. A check that
        // cries wolf gets disabled, which is worse than not having one.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("src/ is readable") {
            let path = entry.expect("entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.extension().is_none_or(|e| e != "rs") || name == "lower.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read");
            for line in text.lines().map(str::trim) {
                if !line.starts_with("use ") {
                    continue;
                }
                for needle in [
                    "SyntaxNode",
                    "SyntaxToken",
                    "rowan",
                    "pw_syntax::kind",
                    "pw_syntax::tree",
                    "pw_syntax::grammar",
                ] {
                    if line.contains(needle) {
                        offenders.push(format!("{name}: {line}"));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "only lower.rs may import syntax nodes (ADR-0014): {offenders:#?}"
        );
    }

    #[test]
    fn the_syntax_node_guard_can_fail() {
        // Negative control: the guard must actually detect an import. Written
        // against the same matcher the test above uses.
        let bad = "use pw_syntax::kind::SyntaxNode;";
        let matched = bad.trim().starts_with("use ")
            && ["SyntaxNode", "rowan", "pw_syntax::kind"]
                .iter()
                .any(|n| bad.contains(n));
        assert!(matched, "the matcher must catch a real import");

        // And must not fire on prose that merely names the type.
        let prose = "// checkers never see a SyntaxNode (ADR-0014)";
        assert!(!prose.trim().starts_with("use "));
    }
}
