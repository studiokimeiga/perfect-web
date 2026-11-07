//! **What a policy head's value MEANS, as data.**
//!
//! Architect ruling, 2026-08-10:
//!
//! > Give policy/CSS call-shaped values contextual semantic resolution; don't
//! > merely exclude them. […] Have each policy operator define the semantic
//! > kind of each argument. […] Being inside a policy never exempts an
//! > executable term from resolution, but not every call-shaped policy argument
//! > is an executable term.
//!
//! # The hole this closes
//!
//! `pw-syntax`'s `POLICY_KEYWORDS` says which heads exist, and an unrecognised
//! head is already `PW0322`. Nothing said what a head's VALUE is. So
//! `hir::Policy` carries `value: String` — the characters after the keyword —
//! and every consumer that wanted meaning did its own `trim()` and `==`.
//!
//! Two consequences, both measured before this file existed:
//!
//! ```text
//! optimistic cart.add(item, quantity)   executable code, never parsed,
//! rollback   cart.remove(item)          never resolved, never checked
//! ```
//!
//! `cart` names nothing in `add_to_cart`'s scope. Two real calls into the void,
//! invisible to every analysis, because the text was never anything but text.
//!
//! And the inverse, from `tests/policy_consumers.rs`: where a policy value IS
//! lowered into the body — a page's statements, a `handler_policy` block — the
//! analyses cannot tell it from executable code, and a page's declared
//! authority moves because of what a policy value spells.
//!
//! One table answers both. A position this table calls `Term` gets ordinary
//! resolution, including the unresolved-name error. A position it calls
//! anything else does not, and the reason is recorded rather than implied.
//!
//! # What this file is NOT
//!
//! It is not the `PolicyExpr`/`TermExpr` split. That changes the HIR and moves
//! every consumer; this is the table that split has to be driven by, landed
//! first so the split is a mechanical change against a reviewed classification
//! rather than a judgement call per policy.

use std::collections::BTreeSet;

/// What kind of value a policy head takes.
///
/// The variants are semantic categories, not syntactic ones: `Word` and
/// `Strategy` are both bare identifiers in the source and are different facts
/// about the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// A word from a closed set: `cache shared`, `consistency snapshot`. The
    /// set is the domain's whole content — an unlisted word is an error, not
    /// an extension point.
    Word(&'static [&'static str]),
    /// **Where a value rests** (track `device-store`, ADR-0366): `device` or
    /// `origin`, as a replicated value says it, and, for a signal the device
    /// keeps, the name it is kept under: `storage device "todos-pleris"`.
    /// What the name may be is `device_store`'s (PW6805).
    Storage,
    /// Words from a closed set, one or more, by commas: `reads snapshot,
    /// read_your_writes` (ADR-0207).
    Words(&'static [&'static str]),
    /// **What a data source holds**, by the names its effects give them:
    /// `holds Carts, Orders` (ADR-0207). Each is resolved as an effect's
    /// argument is, where the program's sources are read (`check.rs`).
    Held,
    /// `30.seconds`, `0.seconds`.
    Duration,
    /// A duration greater than zero: `2.seconds` (ADR-0109).
    Budget,
    /// A type the program declares: `idempotent_by InteractionId`.
    TypeRef,
    /// A parameter of the declaration being written: `key id`. A cache key
    /// may also name a partition it separates, `key store, user` (PW5004):
    /// one of `privacy::PARTITIONS`.
    ///
    /// **Not** a term. `id` here selects a cache-key dimension; it is not
    /// evaluated, and resolving it as a term would make a cache key a function
    /// call.
    ParamRef,
    /// A declared resource, with its key arguments: `depends_on Menu(id)`.
    ResourceRef,
    /// A declared event and the values it carries: `emits
    /// CartChanged(current_session())`. Each value is a term, evaluated where
    /// the declaration runs (ADR-0088).
    EventRef,
    /// A declared event a declaration listens for, and what each of its
    /// values must equal: `invalidates_on InventoryChanged(id, _)`. Each
    /// argument is one of the declaration's parameters, binding that part of
    /// its key, or `_`, any value (ADR-0091). Not an `EventRef`: nothing in it
    /// is evaluated.
    Listener,
    /// A predicate over the caller: `requires SignedIn, OwnsOrder(order)`.
    PredicateRef,
    /// **A layout a page is shown in**: `layout StoreLayout` (ADR-0303).
    /// Resolved, and held to name one layout, by `layouts::check` (PW0352).
    LayoutRef,
    /// **A case of a declared sum type**: `not_found_on StoreError.NotFound`
    /// (ADR-0163). Resolved, and held to the page's queries, by
    /// `routes::not_found_case`.
    CaseRef,
    /// **An optimistic transition.** `optimistic Cart(current_session()) as
    /// cart => cart.add(item, quantity)` — a resource ENTRY, a binder for its
    /// current value, and a pure expression producing the speculative one.
    ///
    /// Architect ruling, 2026-08-11 (ADR-0025): it identifies an entry rather
    /// than a value of a type, because two entries can share a type; and it has
    /// no written inverse, because the platform restores the value it held and
    /// a hand-written inverse is generally false.
    Transition,
    /// **Written by nobody.** `rollback` was a policy until 2026-08-11 and is
    /// now derived: the runtime restores the resource value it held before the
    /// speculative one. A source that writes it is reported rather than
    /// ignored — an author who describes an inverse is describing something the
    /// platform will not use.
    Derived,
    /// A named operator with a signature: `retry bounded_exponential(max = 3)`.
    /// The words beside it are the values it may be instead: `retry forever`
    /// (refused, PW0313) and `retry none`.
    Operator(&'static [Op], &'static [&'static str]),
    /// A word from a closed set that may carry a condition: `impact
    /// layout_write when LayoutAffect`. The condition names a type, and is
    /// the effect ontology's (PW5204).
    ConditionedWord(&'static [&'static str]),
    /// An effect as written, type argument included: `capability
    /// database.read<T>`.
    EffectRef,
    /// A quoted interface name: `host "pw:host/database#read"`.
    Str,
    /// **An operation a host provides** (ADR-0262), quoted:
    /// `"namespace:package/interface#name"`, each part a WIT identifier.
    /// Until ADR-0262 any string was taken, and one with no interface,
    /// `feed:uploads#claim`, broke WIT generation for every component with
    /// an error naming no line.
    HostOp,
    /// One or more worlds: `placement browser, edge, origin`.
    Worlds,
    /// A privacy label constructor applied to a term: `privacy User(consumer)`.
    LabelCtor,
    /// A list of the declaration's own parameters: `inputs values, width,
    /// height`.
    ParamList,
    /// A bare word with no value: `affine`. Present or absent is the whole
    /// fact.
    Flag,
    /// **A block of executable code**, optionally binding a parameter:
    /// `release(handle) { Maps.destroy(handle) }`, `draw(ctx) { .. }`.
    ///
    /// The second `Term`-carrying domain, and the one most easily missed: it
    /// does not look like a policy value at all, and its contents are already
    /// walked by every body analysis. A resource's `release` block is where
    /// cleanup lives — R-044's whole subject — so a rule that skipped policy
    /// values wholesale would stop seeing it.
    Body,
    /// A CSS length: `intrinsic_height 24.px`.
    Length,
    /// A route pattern: `route "/stores/{id}"`. Not a `Str`: its holes name the
    /// declaration's parameters, and charter §8.2 checks every internal link
    /// against the set of these.
    RoutePattern,
    /// **A signal bound to the address's fragment, and the forms it takes**
    /// (track `todomvc`): `fragment filter "/" => "all", "/active" =>
    /// "active"`. A signal of the page, then each form the fragment takes
    /// and the value the signal takes with it, each a quoted literal. Read
    /// and held by `address_fragment::check`.
    Fragment,
    /// A declaration this program defines, named for attribution:
    /// `attributes_forced_layout_to VendorMap`.
    DeclRef,
    /// **A whole number greater than zero, as digits** (track `uploads`):
    /// `max_bytes 5_000_000`, `max_width 4096`. A literal, so a limit is
    /// what the program states and nothing computes it.
    Count,
    /// **A keyed read's set form** (4c, a read is bounded by construction):
    /// `set hours_of_stores` on `fn hours_of(store: StoreId)`, a function of
    /// the same source given a list of the keyed read's key and answering a
    /// list of what it answers, with its key. Resolved, and held to that
    /// shape, by `bounded::check` (PW0407).
    SetRead,
    /// **A table's row** (track `tables`, ADR-0338): `row Todo`, a record
    /// type declared in the table's module. Resolved, and held to that, by
    /// `tables::check` (PW0361).
    RowType,
    /// **Fields of a table's row**, by their names: `key owner, id`, `order
    /// added`. Held to the row by `tables::check` (PW0361). A query's `key`
    /// names its parameters instead (`ParamRef`): see [`domain_on`].
    Fields,
    /// **An index on a table's row**: `index by_owner on owner`, `unique
    /// by_email on email`, its name and its fields. Held by `tables::check`.
    Index,
    /// **A reference from a field of a table's row to a table**: `refers
    /// owner to Users`. Held by `tables::check`.
    Reference,
    /// **A table's change from the schema last released**: `migrate title
    /// renamed from name`, `migrate notes dropped`, `migrate rank backfilled
    /// with 0`. Its words are held by `tables::check` (PW0361), and its use by
    /// the schema's diff (PW0366).
    Migration,
}

/// One policy operator, with the semantic kind of each argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Op {
    /// Stable identity. Never derived from the spelling at a use site — the
    /// spelling is how the operator is FOUND, and this is what it IS.
    pub id: &'static str,
    pub name: &'static str,
    pub args: &'static [Arg],
}

/// What one argument of a policy operator is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// `max = 3` — a named literal of one kind. The name is part of the
    /// signature, and an argument that is not `required` may be left out.
    Named {
        name: &'static str,
        kind: Literal,
        required: bool,
    },
    /// A field of the declaration's result, paired with a strategy:
    /// `merge_by_field(notes = last_write_wins)`. Repeatable.
    FieldStrategy(&'static [&'static str]),
    /// A dimension of the thing being identified: `content_address(code,
    /// captures)`. A word from a closed set, not a value.
    Dimension(&'static [&'static str]),
    /// **A real executable term.** Receives ordinary resolution.
    Term,
    /// **One literal, written alone**: `chunked(100)`. Its name is for a
    /// message; the operator takes it and nothing else.
    Positional { name: &'static str, kind: Literal },
}

/// **What a named literal argument is** (ADR-0089).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Literal {
    /// A count from 1: `max = 3`. The manifest holds it as a `u32`.
    Count,
    /// `true` or `false`: `jitter = true`.
    Truth,
}

impl Literal {
    /// Is `written` a literal of this kind?
    pub fn fits(self, written: &str) -> bool {
        match self {
            Literal::Count => written.parse::<u32>().is_ok_and(|n| n >= 1),
            Literal::Truth => matches!(written, "true" | "false"),
        }
    }

    /// What a literal of this kind is, for a message.
    pub fn describe(self) -> &'static str {
        match self {
            Literal::Count => "a count from 1",
            Literal::Truth => "`true` or `false`",
        }
    }
}

/// A retry's bound and whether its delays vary. The bound is what makes a
/// retry bounded (PW0313), so it is required.
const RETRY_ARGS: &[Arg] = &[
    Arg::Named {
        name: "max",
        kind: Literal::Count,
        required: true,
    },
    Arg::Named {
        name: "jitter",
        kind: Literal::Truth,
        required: false,
    },
];

/// The strategies `merge_by_field` accepts per field.
const MERGE: &[&str] = &["last_write_wins", "maximum", "minimum", "union"];

/// What `content_address` may hash over.
const IDENTITY_DIMENSIONS: &[&str] = &["code", "captures", "document"];

const RETRY_OPS: &[Op] = &[
    Op {
        id: "policy.retry.bounded_exponential",
        name: "bounded_exponential",
        args: RETRY_ARGS,
    },
    Op {
        id: "policy.retry.transport_only",
        name: "transport_only",
        args: RETRY_ARGS,
    },
    // `fixed` was a third until ADR-0215 (ruling 0089-b): a query's runtime
    // never honoured it, and delays that do not grow keep clients that failed
    // together retrying together.
];

const CONFLICT_OPS: &[Op] = &[Op {
    id: "policy.conflict.merge_by_field",
    name: "merge_by_field",
    args: &[Arg::FieldStrategy(MERGE)],
}];

const IDENTITY_OPS: &[Op] = &[Op {
    id: "policy.identity.content_address",
    name: "content_address",
    args: &[Arg::Dimension(IDENTITY_DIMENSIONS)],
}];

/// **What one set read costs a source** (4c): `chunked(k)`, at least ⌈n/k⌉
/// calls for n keys (DynamoDB's `BatchGetItem`, k = 100; Firestore's `in`,
/// k = 30). Beside it the words `one_call` (PostgreSQL's `= ANY($1)`) and
/// `per_key` (a source that cannot batch).
const SETS_OPS: &[Op] = &[Op {
    id: "policy.sets.chunked",
    name: "chunked",
    args: &[Arg::Positional {
        name: "size",
        kind: Literal::Count,
    }],
}];

/// **The kinds an upload may be** (track `uploads`): each one a server
/// sniffs from a file's magic number and measures from its header.
pub const UPLOAD_TYPES: &[&str] = &["png", "jpeg", "webp", "gif"];

/// The domain of every policy head the parser recognises.
///
/// Exhaustive by test: `every_policy_keyword_has_a_domain` fails closed when
/// `pw_syntax::POLICY_KEYWORDS` grows. That is the countermeasure this project
/// has had to adopt three times — a central table that silently ignores what it
/// does not recognise is how `impact` went a whole commit without meaning.
pub fn domain_of(head: &str) -> Option<Domain> {
    Some(match head {
        // --- caching and freshness
        "cache" => Domain::Word(&["shared", "private"]),
        "partition" => Domain::Word(&["public", "private"]),
        "consistency" => Domain::Word(&["strong", "snapshot", "read_your_writes", "eventual"]),
        "freshness" => Domain::Duration,
        // A budget: a request is given it, and ends when it is spent. Zero
        // ends every request before it starts (ADR-0109).
        "timeout" => Domain::Budget,
        "fallback" => Domain::Word(&["last_known_good", "empty"]),
        "stampede" => Domain::Word(&["single_flight"]),
        "regenerate" => Domain::Word(&["on_invalidation"]),
        "locale" | "tenant" | "policy_version" | "code_version" => {
            Domain::Word(&["included_in_key"])
        }

        // --- keys and concurrency
        "key" | "dedupe_by" => Domain::ParamRef,
        // `parallel` is the manifest's second mode (ADR-0089).
        "concurrency" => Domain::Word(&["one_per_key", "parallel"]),
        // The three PW0325's repair names and R-027 expects; the charter's
        // "cancels or supersedes stale work" (ADR-0089).
        "on_key_change" => Domain::Word(&["cancel", "supersede", "keep"]),
        "idempotent_by" => Domain::TypeRef,

        // --- the dependency graph
        "depends_on" => Domain::ResourceRef,
        "invalidates" => Domain::ResourceRef,
        "invalidates_on" => Domain::Listener,
        "emits" => Domain::EventRef,

        // --- authority and placement
        "requires" => Domain::PredicateRef,
        // NEXT 6c: who invokes a command that no reader presses, a word of
        // the platform's: `Payments`, the host on its provider's word.
        "invoked_by" => Domain::Word(crate::invoked::INVOKERS),
        "placement" => Domain::Worlds,
        "privacy" => Domain::LabelCtor,
        "capability" => Domain::EffectRef,
        "host" => Domain::HostOp,
        // ADR-0040: the operation the compiler supplies, by its name.
        "intrinsic" => Domain::Str,
        "route" => Domain::RoutePattern,
        // Track `todomvc`: a page's fragment, bound to one of its signals.
        "fragment" => Domain::Fragment,
        "layout" => Domain::LayoutRef,
        "not_found_on" => Domain::CaseRef,
        // ADR-0295: `redirect_on KiokunError.Moved permanent`, a case and how
        // the move is answered, held by `routes::redirect_case`.
        "redirect_on" => Domain::CaseRef,
        "impact" => Domain::ConditionedWord(&[
            "layout_read",
            "layout_write",
            "dom_write",
            "paint_write",
            "compositor",
        ]),

        // --- executable
        "optimistic" => Domain::Transition,
        "rollback" => Domain::Derived,

        // --- operators
        "retry" | "reconnect" => Domain::Operator(RETRY_OPS, &["none", "forever"]),
        // A signal the device keeps states the only policy its tabs have
        // (track `device-store`): the whole value written last is every
        // tab's. Named, so that it is never silent (A-011).
        "conflict" => Domain::Operator(CONFLICT_OPS, &["last_write_wins"]),
        "identity" => Domain::Operator(IDENTITY_OPS, &[]),

        // --- transport, storage, lifecycle
        "transport" => Domain::Word(&["websocket", "sse", "poll"]),
        "delivery" => Domain::Word(&["streamed", "batched"]),
        "storage" => Domain::Storage,
        "offline" => Domain::Word(&["writable", "readable"]),
        "sync" => Domain::Word(&["on_reconnect", "immediate"]),
        "on_conflict_unresolved" => Domain::Word(&["surface_to_user", "refuse"]),
        // `application` is the scope a subscription may not claim inside a
        // component (R-028, PW2004): a word of the domain, refused where it
        // outlives its owner (ADR-0089).
        "scope" => Domain::Word(&["component", "page", "session", "application"]),
        "on_scope_exit" => Domain::Word(&["close", "detach"]),
        "transaction" => Domain::Word(&["serializable", "snapshot", "read_committed"]),

        // --- what a data source guarantees (ADR-0207)
        "holds" => Domain::Held,
        "transactions" => Domain::Word(&["serializable", "snapshot", "read_committed", "none"]),
        "reads" => Domain::Words(&["strong", "snapshot", "read_your_writes", "eventual"]),
        "changes" => Domain::Word(&["none", "feed"]),
        // 4c: what one set read costs it, and a keyed read's set form.
        "sets" => Domain::Operator(SETS_OPS, &["one_call", "per_key"]),
        "set" => Domain::SetRead,
        // --- a table's (track `tables`, ADR-0338): its row, its order, its
        // indexes, its references, its bound, and its change from the
        // schema released. Its `key` is `domain_on`'s.
        "row" => Domain::RowType,
        "order" => Domain::Fields,
        "index" | "unique" => Domain::Index,
        "refers" => Domain::Reference,
        "bounded" => Domain::Count,
        "migrate" => Domain::Migration,
        // The owned-rows ADR: who may change a table's rows. `user` and
        // `session` are the request's reader, whom the platform records with
        // each row it inserts; `anyone` says any reader may change any row.
        "owner" => Domain::Word(OWNERS),
        // Track `device-store` (ADR-0366): the version of a value the device
        // keeps; each earlier one is its `migrate`'s.
        "version" => Domain::Count,
        // An upload's (track `uploads`, ADR-0260): where its committed files
        // are served, its limits, and its kinds, a closed set each sniffed
        // from a file's bytes.
        "serves" => Domain::Str,
        // A predicate's (ADR-0302): the words a refusal by it is told in, one
        // string, held by `predicates::check`.
        "says" => Domain::Str,
        "max_bytes" | "max_width" | "max_height" => Domain::Count,
        "types" => Domain::Words(UPLOAD_TYPES),
        "captures" => Domain::Word(&["serializable_only"]),
        "on_version_mismatch" => Domain::Word(&["safe_refetch", "refuse"]),
        "load" => Domain::Word(&["on_first_interaction", "eager"]),
        "inputs" => Domain::ParamList,
        "isolated" => Domain::Word(&["true", "false"]),

        // --- resource lifecycle and painting: executable blocks
        "acquire" | "release" | "draw" => Domain::Body,
        // The hooks of an `unsafe.lifecycle { .. }` escape hatch (charter §7.5):
        // executable blocks, written inside the statement's block.
        "on_mount" | "on_unmount" => Domain::Body,
        "affine" => Domain::Flag,
        "intrinsic_height" => Domain::Length,
        "revision" => Domain::Word(&["content_hash"]),
        // Inside an `animate` block: the user preference the animation yields
        // to (charter §7.5A). A statement clause the grammar reads after the
        // keyframes, given its vocabulary here so its value is a word of this
        // domain and not a name to resolve (ADR-0047).
        "respects" => Domain::Word(&["prefers_reduced_motion"]),

        // --- the audit record on an escape hatch
        "because" => Domain::Str,
        "attributes_forced_layout_to" => Domain::DeclRef,

        _ => return None,
    })
}

/// **What `head`'s value means on a declaration of `kind`**: [`domain_of`],
/// but for the one head two kinds of declaration share with two meanings. A
/// query's `key` names its parameters (`ParamRef`); a table's names its row's
/// fields (track `tables`).
pub fn domain_on(head: &str, kind: crate::hir::DeclKind) -> Option<Domain> {
    match (head, kind) {
        ("key", crate::hir::DeclKind::Table) => Some(Domain::Fields),
        _ => domain_of(head),
    }
}

/// **The execution context a block policy's contents run in.**
///
/// Architect ruling, 2026-08-11: the policy operator's identity selects the
/// form, and downstream resolution receives already-lowered lexical bindings.
/// Nothing recognises `"draw"` or `"release"` by spelling to decide what a
/// block introduces — this table does, once.
///
/// `Draw`, `Acquire` and `Release` get no new legality rules; they carry only
/// the semantics they already had. What changes is that their contents are
/// attributed to their own root rather than to whatever declaration happens to
/// enclose them.
pub fn execution_context(head: &str) -> Option<crate::hir::ExecutionContext> {
    use crate::hir::ExecutionContext as X;
    match domain_of(head)? {
        Domain::Body => Some(match head {
            "draw" => X::Draw,
            "acquire" => X::Acquire,
            "release" => X::Release,
            _ => return None,
        }),
        _ => None,
    }
}

/// Every policy head whose value is, or contains, **executable code**.
///
/// This is the list `PW0024` has to consult. A name written in one of these
/// positions is a term and must resolve; a name written anywhere else is not,
/// and reporting it would be the over-broad rule that was reverted twice.
///
/// The `Term` domains are direct. The rest carry terms in ARGUMENT positions:
/// `emits CartChanged(current_session())` names an event — which is not a term
/// — and passes it one.
pub fn carries_terms(head: &str) -> bool {
    matches!(
        domain_of(head),
        Some(Domain::Transition)
            | Some(Domain::Body)
            | Some(Domain::ResourceRef)
            | Some(Domain::EventRef)
            | Some(Domain::Listener)
            | Some(Domain::LabelCtor)
    )
}

/// **A clause that names a declaration and passes it its key** (ADR-0088):
/// the namespace its names are looked up in, and the kinds of declaration
/// it may name. `invalidates Cart(current_session())` names a resource, and
/// `emits CartChanged(current_session())` an event. Each key is a term.
///
/// `depends_on` names a resource, or a materialization (ADR-0255, ADR-0195's
/// ruling 10), a term since ADR-0273, as a query is.
pub fn keyed(
    head: &str,
) -> Option<&'static [(crate::resolve::Namespace, &'static [crate::hir::DeclKind])]> {
    use crate::hir::DeclKind as K;
    use crate::resolve::Namespace;
    const RESOURCES: &[K] = &[K::Query, K::Subscription, K::Resource];
    const READ: &[(Namespace, &[K])] = &[(
        Namespace::Term,
        &[K::Query, K::Subscription, K::Resource, K::Materialize],
    )];
    const RESOURCE: &[(Namespace, &[K])] = &[(Namespace::Term, RESOURCES)];
    const EVENT: &[(Namespace, &[K])] = &[(Namespace::Event, &[K::Event])];
    match domain_of(head)? {
        Domain::ResourceRef if head == "depends_on" => Some(READ),
        Domain::ResourceRef => Some(RESOURCE),
        Domain::EventRef | Domain::Listener => Some(EVENT),
        _ => None,
    }
}

/// **Which declarations a clause belongs to, and what they are, for a
/// message** (ADR-0092; ADR-0216, the owner's ruling 0092-b).
///
/// Every head the language has is here, and a head on a kind it is not
/// listed for is PW5105: the table fails closed. Until ADR-0216 only the
/// graph's four clauses were, and every other head checked wherever it was
/// written: `freshness` on a command, `retry` on a function, read by nothing.
///
/// A head another rule places is listed for every kind, so a defect is
/// reported once (ADR-0112): `requires` (PW0335), `not_found_on` (PW0342),
/// `rollback` where PW0327 refuses it, and `host` on an effect (PW0332).
/// A head written only in a statement is listed for no declaration, by the
/// statement it belongs to.
pub fn declared_by(head: &str) -> Option<(&'static [crate::hir::DeclKind], &'static str)> {
    use crate::hir::DeclKind as K;
    /// Every kind: a head whose place another rule decides.
    const EVERY: &[K] = &[
        K::Fn,
        K::View,
        K::Component,
        K::Page,
        K::Layout,
        K::Query,
        K::Command,
        K::Subscription,
        K::Resource,
        K::Materialize,
        K::Event,
        K::Effect,
        K::Source,
        K::Table,
        K::Upload,
        K::Prelude,
        K::Task,
        K::Type,
        K::Opaque,
        K::Let,
        K::Signal,
        K::Import,
        K::Other,
    ];
    /// What holds code, and so runs somewhere.
    const PLACED: &[K] = &[
        K::Fn,
        K::View,
        K::Component,
        K::Page,
        K::Query,
        K::Command,
        K::Subscription,
        K::Resource,
        K::Materialize,
        K::Effect,
        K::Task,
        K::Other,
    ];
    const READS: &[K] = &[K::Query, K::Subscription, K::Resource];
    Some(match head {
        // The dependency graph's clauses (ADR-0092).
        "emits" | "invalidates" => (&[K::Command], "a command"),
        "invalidates_on" => (
            &[K::Query, K::Subscription, K::Resource, K::Materialize],
            "a resource or a materialization",
        ),
        "depends_on" => (&[K::Materialize], "a materialization"),
        // A cached read's: what a host keeps of it, and for how long.
        "freshness" | "consistency" | "concurrency" | "dedupe_by" | "timeout" => {
            (READS, "a query, a subscription or a resource")
        }
        // A cached read's key, or a table's (track `tables`).
        "key" => (
            &[K::Query, K::Subscription, K::Resource, K::Table],
            "a query, a subscription, a resource or a table",
        ),
        "cache" => (
            &[K::Query, K::Subscription, K::Resource, K::Page],
            "a query, a subscription, a resource or a page",
        ),
        "on_key_change" => (&[K::Query, K::Resource], "a query or a resource"),
        "delivery" => (&[K::Query], "a query"),
        "retry" => (
            &[K::Query, K::Resource, K::Command],
            "a query, a resource or a command",
        ),
        "fallback" => (
            &[K::Query, K::Resource, K::Materialize],
            "a query, a resource or a materialization",
        ),
        "partition" => (
            &[
                K::Query,
                K::Subscription,
                K::Resource,
                K::Materialize,
                K::Page,
            ],
            "a cached read, a materialization or a page",
        ),
        "regenerate" | "stampede" | "locale" | "tenant" | "policy_version" | "code_version" => {
            (&[K::Materialize], "a materialization")
        }
        // A command's.
        "idempotent_by" | "transaction" | "optimistic" | "invoked_by" => {
            (&[K::Command], "a command")
        }
        // A subscription's connection.
        "transport" | "reconnect" | "on_scope_exit" => (&[K::Subscription], "a subscription"),
        "scope" => (
            &[K::Subscription, K::Resource],
            "a subscription or a resource",
        ),
        // A resource's life.
        "acquire" | "release" | "affine" => (&[K::Resource], "a resource"),
        // A page's.
        "revision" => (&[K::Page], "a page"),
        // A page's address, or where a form posts an upload (track `uploads`).
        "route" => (&[K::Page, K::Upload], "a page or an upload"),
        // A page's address's fragment, bound to a signal (track `todomvc`).
        "fragment" => (&[K::Page], "a page"),
        // The layout a page is shown in (ADR-0303): a page's, and not a
        // layout's, which names none.
        "layout" => (&[K::Page], "a page"),
        "privacy" => (
            &[K::Page, K::Other, K::Signal],
            "a page, a replicated value or a signal the device keeps",
        ),
        // An effect's.
        "capability" | "impact" => (&[K::Effect], "an effect"),
        // A function's binding to what supplies it.
        "intrinsic" => (&[K::Fn], "a function"),
        "host" => (&[K::Fn, K::Effect], "a function"),
        // A data source's (ADR-0207), and what a set read costs it (4c).
        "holds" | "transactions" | "reads" | "changes" | "sets" => (&[K::Source], "a data source"),
        // A keyed read's set form (4c): a function's.
        "set" => (&[K::Fn], "a function"),
        // A table's (track `tables`, ADR-0338), and who may change its rows
        // (the owned-rows ADR).
        "row" | "order" | "index" | "unique" | "refers" | "bounded" | "owner" => {
            (&[K::Table], "a table")
        }
        // A table's change from its schema released, or a device's value's
        // from its version released (track `device-store`).
        "migrate" => (
            &[K::Table, K::Signal],
            "a table or a signal the device keeps",
        ),
        "version" => (&[K::Signal], "a signal the device keeps"),
        // An upload's (track `uploads`, ADR-0260).
        "serves" | "max_bytes" | "types" | "max_width" | "max_height" => {
            (&[K::Upload], "an upload")
        }
        "says" => (&[K::Predicate], "a predicate"),
        // A painter's, a replicated value's, a handler policy's: forms the
        // grammar keeps as `Other`.
        "draw" | "inputs" | "isolated" => (&[K::Other], "a painter"),
        "offline" | "sync" | "on_conflict_unresolved" => (&[K::Other], "a replicated value"),
        // Where a value rests and what two copies' writes do: a replicated
        // value's, and a signal's the device keeps (track `device-store`).
        "storage" | "conflict" => (
            &[K::Other, K::Signal],
            "a replicated value or a signal the device keeps",
        ),
        "identity" | "captures" | "load" | "on_version_mismatch" => {
            (&[K::Other], "a handler policy")
        }
        // Where it runs.
        "placement" => (PLACED, "a declaration that holds code"),
        // Placed by their own rules.
        "requires" | "not_found_on" | "redirect_on" => (EVERY, "a command"),
        "rollback" => (
            &[
                K::Query,
                K::Command,
                K::Subscription,
                K::Resource,
                K::Materialize,
            ],
            "nothing: an optimistic clause is undone by restoring what it held",
        ),
        // Written only in statements.
        "intrinsic_height" => (&[], "a `subtree` block"),
        "because" | "attributes_forced_layout_to" => (&[], "an `unsafe` statement"),
        "on_mount" | "on_unmount" | "respects" => (&[], "a component's body"),
        _ => return None,
    })
}

/// **May `head` be written more than once in one declaration?** An
/// effect's `impact`: an effect has as many impacts as it has facets
/// (`impact dom_write`, `impact layout_write when LayoutAffect`); and a
/// table's `index`, `unique`, `refers` and `migrate`, each one of several
/// (track `tables`). Any other head written twice says two things, and every
/// reader took the first (ADR-0098).
pub fn repeats(head: &str) -> bool {
    matches!(
        domain_of(head),
        Some(
            Domain::ConditionedWord(_)
                // A table has as many indexes, references and statements of
                // its change as it states (track `tables`).
                | Domain::Index
                | Domain::Reference
                | Domain::Migration
        )
    )
}

/// The operator a spelling names, within a head's domain.
///
/// Contextual: `merge_by_field` is an operator under `conflict` and nothing
/// under `retry`. There is no global operator namespace to fall back to, which
/// is what makes an unknown operator an error rather than a silent pass.
pub fn operator(head: &str, spelling: &str) -> Option<&'static Op> {
    match domain_of(head)? {
        Domain::Operator(ops, _) => ops.iter().find(|o| o.name == spelling),
        _ => None,
    }
}

/// **A duration as written: `30.seconds`, `500.milliseconds`, `1.hours`.**
///
/// The one reading, in milliseconds: the manifest reads it, and a value it
/// cannot read is refused where it is written (ADR-0089).
/// **The units of a CSS length** (ADR-0247), as CSS Values and Units
/// Level 4 defines them: absolute, font-relative and viewport-percentage.
pub const LENGTH_UNITS: &[&str] = &[
    "cm", "mm", "Q", "in", "pt", "pc", "px", // absolute
    "em", "rem", "ex", "rex", "cap", "rcap", "ch", "rch", "ic", "ric", "lh", "rlh", // font
    "vw", "svw", "lvw", "dvw", "vh", "svh", "lvh", "dvh", "vi", "svi", "lvi", "dvi", "vb", "svb",
    "lvb", "dvb", "vmin", "svmin", "lvmin", "dvmin", "vmax", "svmax", "lvmax",
    "dvmax", // viewport
];

/// **Is `v` a length**: a count, whole or with a fraction, then `.` and a
/// CSS length unit, `24.px`, `1.5.rem` (ADR-0247). Until ADR-0247 no
/// length was judged, and `intrinsic_height bogus` checked.
pub fn length(v: &str) -> bool {
    let Some((count, unit)) = v.trim().rsplit_once('.') else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let count = match count.split_once('.') {
        Some((whole, fraction)) => digits(whole) && digits(fraction),
        None => digits(count),
    };
    count && LENGTH_UNITS.contains(&unit)
}

pub fn duration(v: &str) -> Option<u64> {
    let (n, unit) = v.split_once('.')?;
    let n: u64 = n.trim().parse().ok()?;
    let per = match unit.trim() {
        "milliseconds" | "millisecond" | "ms" => 1,
        "seconds" | "second" => 1_000,
        "minutes" | "minute" => 60_000,
        "hours" | "hour" => 3_600_000,
        _ => return None,
    };
    n.checked_mul(per)
}

/// **The platform's predicate, which every reader holds** (the requires ADR):
/// `requires Anyone` says a command is anyone's to run. A command a reader can
/// run says who may (PW0367), and this is how it says "anyone". No program
/// declares it, it takes no argument and stands alone, and nothing asks it:
/// the contract carries no requirement for it, and a page needs no guard.
pub const ANYONE: &str = "Anyone";

/// **Who may change a table's rows** (the owned-rows ADR): `owner user` and
/// `owner session`, the reader who inserted each row, whom the platform
/// records; `owner anyone`, any reader. A table a command a reader can run
/// writes says one (PW0370), as such a command says who may run it.
pub const OWNERS: &[&str] = &["user", "session", "anyone"];

/// One deployment authorization predicate written by a command.
///
/// `requires SignedIn, OwnsOrder(order)` is declarative. The compiler records
/// the predicate names and which command parameters they receive; the deployment
/// supplies their meaning at invocation time. A predicate is therefore not a
/// Pleris term and cannot silently execute inside the component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Predicate {
    pub name: String,
    pub arguments: Vec<String>,
}

/// Parse a `requires` value.
///
/// Predicates may be bare or applied to command parameters. Their names belong
/// to the deployment's authorization vocabulary, not to the program's term
/// namespace. Arguments are deliberately only names: the host evaluates a
/// precondition over the authenticated caller and the already-typed command
/// arguments, rather than running arbitrary Pleris code before authorization.
pub fn predicates(value: &str) -> Result<Vec<Predicate>, String> {
    let items = split_top_level(value)?;
    if items.is_empty() {
        return Err("`requires` names no predicate".to_string());
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            return Err("an authorization predicate is empty".to_string());
        }
        let (name, arguments) = match item.split_once('(') {
            Some((name, rest)) => {
                let Some(inner) = rest.strip_suffix(')') else {
                    return Err(format!(
                        "`{}`'s arguments are not closed with `)`",
                        name.trim()
                    ));
                };
                let args = split_top_level(inner)?;
                (
                    name.trim(),
                    args.into_iter().map(str::trim).collect::<Vec<_>>(),
                )
            }
            None => (item, Vec::new()),
        };

        if !predicate_name(name) {
            return Err(format!("`{name}` is not an authorization predicate name"));
        }
        let mut args = Vec::new();
        for argument in arguments {
            if !predicate_name(argument) || argument.contains('.') {
                return Err(format!(
                    "`{name}` receives command parameter names; `{argument}` is not one"
                ));
            }
            args.push(argument.to_string());
        }

        let predicate = Predicate {
            name: name.to_string(),
            arguments: args,
        };
        if !seen.insert((predicate.name.clone(), predicate.arguments.clone())) {
            return Err(format!("`{item}` is required twice"));
        }
        out.push(predicate);
    }
    Ok(out)
}

/// Comma-separated items, without splitting commas inside an argument list.
fn split_top_level(value: &str) -> Result<Vec<&str>, String> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Err("an authorization predicate has an unmatched `)`".to_string());
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                out.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("an authorization predicate has an unclosed `(`".to_string());
    }
    out.push(&value[start..]);
    Ok(out)
}

fn predicate_name(name: &str) -> bool {
    fn segment(s: &str) -> bool {
        let mut chars = s.chars();
        chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
            && chars.all(|c| c == '_' || c.is_alphanumeric())
    }
    !name.is_empty() && name.split('.').all(segment)
}

/// **A count as written**: digits, `_` between them, greater than zero and
/// within `u64` (track `uploads`): `5_000_000`.
pub fn count(value: &str) -> Option<u64> {
    let v = value.trim();
    let ok = v.starts_with(|c: char| c.is_ascii_digit())
        && v.ends_with(|c: char| c.is_ascii_digit())
        && !v.contains("__")
        && v.chars().all(|c| c.is_ascii_digit() || c == '_');
    ok.then(|| v.replace('_', "").parse::<u64>().ok())
        .flatten()
        .filter(|n| *n > 0)
}

/// **What is wrong with a policy's value, by its domain** (ADR-0089).
///
/// Architect ruling, 2026-08-07, on an unknown policy head: "Pleris may
/// reject authored semantics; it must never silently erase them." A value its
/// domain does not have is the same failure one level down. `cache Shared`
/// was read as no cache by the manifest, and as no shared cache by the rule
/// that keeps a session's data out of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueFault {
    /// Not one of the words the domain lists.
    Word(&'static [&'static str]),
    /// Not a duration.
    Duration,
    /// Not a length (ADR-0247).
    Length,
    /// A duration of zero, where the domain is a budget (ADR-0109).
    ZeroBudget,
    /// Not a world.
    World(String),
    /// A flag written with a value.
    Flag,
    /// Not a whole number greater than zero, as digits (track `uploads`).
    Count,
    /// Not an operation a host provides (ADR-0262), and why.
    HostOp(String),
    /// No operator of that name, where these are the domain's.
    Operator(String, Vec<&'static str>),
    /// An operator's arguments opened and never closed.
    Unclosed(String),
    /// An argument the operator does not take.
    UnknownArgument(&'static str, String),
    /// An argument given twice.
    ArgumentTwice(&'static str, String),
    /// A required argument left out.
    MissingArgument(&'static str, &'static str),
    /// An argument that is not of its kind: `max = "two"`.
    ArgumentKind(&'static str, &'static str, Literal, String),
    /// A value where every argument is written by name.
    Positional(&'static str, String),
    /// A word the operator's argument does not list: a strategy, a dimension.
    ArgumentWord(&'static str, String, &'static [&'static str]),
}

/// **The fault in `value`, as `head`'s domain reads it**, or `None` where the
/// domain has it or is not one this reads. A name the value mentions, a
/// parameter or a type, is resolved where scope is known (`check.rs`).
/// **An operation a host provides, as a binding writes it** (ADR-0262):
/// `"namespace:package/interface#name"`, quoted, its package, interface and
/// name each a WIT identifier. Answers the four parts, or why not.
pub fn host_op(value: &str) -> Result<[&str; 4], String> {
    let quoted = value
        .trim()
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or("it is not quoted")?;
    let (interface, name) = quoted
        .split_once('#')
        .ok_or("it names no operation after `#`")?;
    let (package, interface) = interface
        .split_once('/')
        .ok_or("it names no interface after its package, `/`")?;
    let (namespace, package) = package
        .split_once(':')
        .ok_or("its package has no namespace, `namespace:package`")?;
    let parts = [namespace, package, interface, name];
    for part in parts {
        wit_id(part)?;
    }
    Ok(parts)
}

/// **A WIT identifier**, as wit-parser 0.257.1's `validate_id` holds one:
/// words joined by `-`, each all lowercase or all uppercase ASCII letters,
/// digits among them, the first beginning with a letter.
fn wit_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("a part is empty".to_string());
    }
    for (i, word) in id.split('-').enumerate() {
        let Some(first) = word.chars().next() else {
            return Err(format!("`{id}` has an empty word"));
        };
        if i == 0 && !first.is_ascii_alphabetic() {
            return Err(format!("`{id}` begins with `{first}`, not a letter"));
        }
        let lower = word.chars().any(|c| c.is_ascii_lowercase());
        let upper = word.chars().any(|c| c.is_ascii_uppercase());
        if let Some(c) = word.chars().find(|c| !c.is_ascii_alphanumeric()) {
            return Err(format!("`{id}` has `{c}`, which no WIT identifier has"));
        }
        if lower && upper {
            return Err(format!("`{id}`'s `{word}` mixes cases"));
        }
    }
    Ok(())
}

/// **Where a value may rest** (`storage`'s words).
pub const STORAGES: &[&str] = &["device", "origin"];

/// **`storage`'s value, read** (track `device-store`): its word, and the
/// quoted name after it where one is written, `device "todos-pleris"`. `None`
/// where the word is no storage, or what follows is not one quoted text
/// without an escape.
pub fn storage(value: &str) -> Option<(&str, Option<&str>)> {
    let value = value.trim();
    let (word, rest) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
    if !STORAGES.contains(&word) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Some((word, None));
    }
    let name = rest.strip_prefix('"')?.strip_suffix('"')?;
    (!name.contains('"') && !name.contains('\\')).then_some((word, Some(name)))
}

pub fn value_fault(head: &str, value: &str) -> Option<ValueFault> {
    let value = value.trim();
    match domain_of(head)? {
        Domain::Word(words) => (!words.contains(&value)).then_some(ValueFault::Word(words)),
        Domain::Storage => storage(value)
            .is_none()
            .then_some(ValueFault::Word(STORAGES)),
        Domain::Words(words) => value
            .split(',')
            .map(str::trim)
            .any(|w| !words.contains(&w))
            .then_some(ValueFault::Word(words)),
        Domain::ConditionedWord(words) => {
            let word = value.split_once(" when ").map_or(value, |(w, _)| w).trim();
            (!words.contains(&word)).then_some(ValueFault::Word(words))
        }
        Domain::Duration => duration(value).is_none().then_some(ValueFault::Duration),
        Domain::Length => (!length(value)).then_some(ValueFault::Length),
        Domain::Budget => match duration(value) {
            None => Some(ValueFault::Duration),
            Some(0) => Some(ValueFault::ZeroBudget),
            Some(_) => None,
        },
        Domain::Worlds => value
            .split(',')
            .map(str::trim)
            .find(|w| !crate::placement::ALL_WORLDS.iter().any(|x| x.name() == *w))
            .map(|w| ValueFault::World(w.to_string())),
        Domain::Flag => (!value.is_empty()).then_some(ValueFault::Flag),
        Domain::Count => count(value).is_none().then_some(ValueFault::Count),
        Domain::HostOp => host_op(value).err().map(ValueFault::HostOp),
        Domain::Operator(ops, words) => {
            if words.contains(&value) {
                return None;
            }
            let (name, args) = match value.split_once('(') {
                Some((name, rest)) => match rest.trim_end().strip_suffix(')') {
                    Some(args) => (name.trim(), args),
                    None => return Some(ValueFault::Unclosed(name.trim().to_string())),
                },
                None => (value, ""),
            };
            let Some(op) = ops.iter().find(|o| o.name == name) else {
                return Some(ValueFault::Operator(
                    name.to_string(),
                    ops.iter().map(|o| o.name).collect(),
                ));
            };
            operator_fault(op, args)
        }
        _ => None,
    }
}

/// What is wrong with an operator's arguments: each written once, by the
/// name its signature gives it, and of its kind.
fn operator_fault(op: &'static Op, args: &str) -> Option<ValueFault> {
    let written: Vec<&str> = args
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .collect();
    // One literal, written alone: `chunked(100)` (4c).
    if let [Arg::Positional { name, kind }] = op.args {
        return match written.as_slice() {
            [] => Some(ValueFault::MissingArgument(op.name, name)),
            [v] if !kind.fits(v) => Some(ValueFault::ArgumentKind(
                op.name,
                name,
                *kind,
                v.to_string(),
            )),
            [_] => None,
            [..] => Some(ValueFault::ArgumentTwice(op.name, name.to_string())),
        };
    }
    let mut given: Vec<&str> = Vec::new();
    for a in &written {
        match op.args {
            [Arg::Dimension(words)] => {
                if !words.contains(a) {
                    return Some(ValueFault::ArgumentWord(op.name, a.to_string(), words));
                }
            }
            [Arg::FieldStrategy(strategies)] => {
                let Some((_, strategy)) = a.split_once('=') else {
                    return Some(ValueFault::Positional(op.name, a.to_string()));
                };
                let strategy = strategy.trim();
                if !strategies.contains(&strategy) {
                    return Some(ValueFault::ArgumentWord(
                        op.name,
                        strategy.to_string(),
                        strategies,
                    ));
                }
            }
            _ => {
                let Some((name, v)) = a.split_once('=') else {
                    return Some(ValueFault::Positional(op.name, a.to_string()));
                };
                let (name, v) = (name.trim(), v.trim());
                let Some((kind, _)) = named(op, name) else {
                    return Some(ValueFault::UnknownArgument(op.name, name.to_string()));
                };
                if given.contains(&name) {
                    return Some(ValueFault::ArgumentTwice(op.name, name.to_string()));
                }
                given.push(name);
                if !kind.fits(v) {
                    let arg = named(op, name).map_or("", |(_, n)| n);
                    return Some(ValueFault::ArgumentKind(op.name, arg, kind, v.to_string()));
                }
            }
        }
    }
    op.args.iter().find_map(|a| match a {
        Arg::Named {
            name,
            required: true,
            ..
        } if !given.contains(name) => Some(ValueFault::MissingArgument(op.name, name)),
        _ => None,
    })
}

/// The named argument `name` of `op`: its kind, and its name as the
/// signature spells it.
fn named(op: &Op, name: &str) -> Option<(Literal, &'static str)> {
    op.args.iter().find_map(|a| match a {
        Arg::Named { name: n, kind, .. } if *n == name => Some((*kind, *n)),
        _ => None,
    })
}

/// **The operator a value applies**: `transport_only` in `retry
/// transport_only(max = 2)`. How a rule asks which operator it is, rather
/// than reading the value's spelling.
pub fn applied(head: &str, value: &str) -> Option<&'static Op> {
    let name = value.split_once('(').map_or(value, |(n, _)| n).trim();
    operator(head, name)
}

/// Every operator id the registry defines, for the stability test.
pub fn all_operator_ids() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for ops in [RETRY_OPS, CONFLICT_OPS, IDENTITY_OPS, SETS_OPS] {
        for o in ops {
            out.insert(o.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every head `domain_of` gives a domain, in its order.
    const HEADS: &[&str] = &[
        "cache",
        "partition",
        "consistency",
        "freshness",
        "timeout",
        "fallback",
        "stampede",
        "regenerate",
        "locale",
        "tenant",
        "policy_version",
        "code_version",
        "key",
        "dedupe_by",
        "concurrency",
        "on_key_change",
        "idempotent_by",
        "depends_on",
        "invalidates",
        "invalidates_on",
        "emits",
        "requires",
        "invoked_by",
        "placement",
        "route",
        "fragment",
        "layout",
        "not_found_on",
        "redirect_on",
        "privacy",
        "capability",
        "host",
        "intrinsic",
        "impact",
        "optimistic",
        "rollback",
        "retry",
        "reconnect",
        "conflict",
        "identity",
        "transport",
        "delivery",
        "storage",
        "offline",
        "sync",
        "on_conflict_unresolved",
        "scope",
        "on_scope_exit",
        "transaction",
        "holds",
        "row",
        "order",
        "index",
        "unique",
        "refers",
        "bounded",
        "migrate",
        "owner",
        "transactions",
        "reads",
        "changes",
        "sets",
        "set",
        "captures",
        "on_version_mismatch",
        "load",
        "inputs",
        "isolated",
        "acquire",
        "release",
        "draw",
        "on_mount",
        "on_unmount",
        "affine",
        "intrinsic_height",
        "revision",
        "respects",
        "because",
        "attributes_forced_layout_to",
    ];

    /// **Fails closed when the parser learns a new policy head.**
    ///
    /// The countermeasure, stated once in `tests/unknown_policy.rs` and owed
    /// again here: a head the parser accepts and this table does not classify
    /// is a value with no meaning, and it would look exactly like a value
    /// nobody wrote.
    #[test]
    fn every_policy_keyword_has_a_domain() {
        let missing: Vec<&str> = pw_syntax::POLICY_KEYWORDS
            .iter()
            .copied()
            .filter(|k| domain_of(k).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "the parser recognises these policy heads and this table does not \
             say what their values mean: {missing:?}"
        );
    }

    /// And the other direction, so the table cannot drift into describing
    /// policies the language does not have.
    ///
    /// The exceptions are the heads written inside a `handler_policy`,
    /// `replicated` or `paint` BLOCK. Those lower as body statements rather
    /// than as `Policy` entries — which is precisely what the `PolicyExpr`
    /// split has to fix — so the parser's list does not contain them yet.
    #[test]
    fn the_table_describes_only_policies_the_language_has() {
        const IN_BLOCK_ONLY: &[&str] = &[
            "identity",
            "captures",
            "on_version_mismatch",
            "load",
            "on_conflict_unresolved",
            "inputs",
            "isolated",
            "tenant",
            "respects",
            "on_mount",
            "on_unmount",
        ];
        let invented: Vec<&str> = HEADS
            .iter()
            .copied()
            .filter(|h| !pw_syntax::POLICY_KEYWORDS.contains(h) && !IN_BLOCK_ONLY.contains(h))
            .collect();
        assert!(
            invented.is_empty(),
            "this table classifies heads the parser does not recognise as \
             policies at all: {invented:?}"
        );
    }

    /// **A clause's head takes its value on its line** (ADR-0243). The names
    /// check reads a head this table gives a domain, and what follows it on
    /// its line, as one clause (ADR-0047). The grammar separates a block's
    /// statements around the same heads, so a clause written in a block is
    /// never refused as two statements. A head whose value is code takes a
    /// block, which follows any statement.
    #[test]
    fn every_clause_head_takes_its_value_on_its_line() {
        let refused: Vec<&str> = HEADS
            .iter()
            .copied()
            .filter(|h| domain_of(h) != Some(Domain::Body) && !pw_syntax::heads_a_clause(h))
            .collect();
        assert!(
            refused.is_empty(),
            "the names check reads these heads with a value on their line, and \
             the grammar would refuse it as a second statement: {refused:?}"
        );
    }

    /// **Operator ids are stable and unique.**
    ///
    /// They are what a `PolicyApply` node will carry, and what an artifact will
    /// record. Two operators sharing one id would make a contract hash agree
    /// across a semantic difference.
    #[test]
    fn operator_ids_are_unique_and_namespaced() {
        let ids = all_operator_ids();
        let mut count = 0;
        for ops in [RETRY_OPS, CONFLICT_OPS, IDENTITY_OPS, SETS_OPS] {
            count += ops.len();
        }
        assert_eq!(ids.len(), count, "two operators share one id");
        for id in &ids {
            assert!(id.starts_with("policy."), "{id} is not namespaced");
        }
    }

    /// **An operator is resolved within its head's domain, never globally.**
    #[test]
    fn an_operator_belongs_to_one_domain() {
        assert!(operator("retry", "bounded_exponential").is_some());
        assert!(operator("reconnect", "bounded_exponential").is_some());
        assert!(
            operator("conflict", "bounded_exponential").is_none(),
            "a retry strategy is not a conflict strategy, and the only reason \
             to accept it here would be a global operator table"
        );
        assert!(operator("retry", "merge_by_field").is_none());
        assert!(
            operator("cache", "shared").is_none(),
            "`cache` takes a word, not an operator — asking for one must not \
             invent an operator from the word"
        );
    }

    /// **The term positions are exactly the ones that hold executable code.**
    ///
    /// The discriminator matters more than the list: `key id` and `cache
    /// shared` are the shapes the over-broad `PW0024` reported, twice, before
    /// being reverted.
    #[test]
    fn only_executable_positions_carry_terms() {
        for head in [
            "optimistic",
            "emits",
            "invalidates",
            "acquire",
            "release",
            "draw",
        ] {
            assert!(carries_terms(head), "{head} holds executable code");
        }
        assert!(
            !carries_terms("rollback"),
            "`rollback` is written by nobody since ADR-0025 — the platform \
             restores the value it held"
        );
        for head in [
            "cache",
            "key",
            "requires",
            "consistency",
            "freshness",
            "placement",
            "retry",
            "impact",
            "capability",
            "host",
            "concurrency",
            "transport",
            "storage",
        ] {
            assert!(
                !carries_terms(head),
                "{head} does not hold executable code, and reporting its value \
                 as an unresolved name is the rule that was reverted twice"
            );
        }
    }
}
