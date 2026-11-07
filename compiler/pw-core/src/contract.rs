//! E8-0 — what the compiler hands the host, frozen.
//!
//! Architect ruling, 2026-08-07:
//!
//! > The compiler decides what authority code needs. The host decides whether
//! > that authority physically exists. Neither should reconstruct the other's
//! > answer.
//!
//! and, on the shape of the boundary:
//!
//! > Freeze the compiler→host `ComponentContract` — component_id, abi_schema,
//! > required_capabilities, allowed_placements, imports, exports. Rule: actual
//! > Wasm imports ⊆ statically allowed set, never a superset. E8 consumes
//! > capabilities; it does not define effect semantics.
//!
//! # What this module is careful NOT to do
//!
//! It does not say what a capability *means*. `database.read<Stores>` is a
//! name derived from an effect row; whether a `database.read` exists, what it
//! connects to, and whether this deployment has one are the host's questions.
//! A compiler that answered them would be a second deployment topology, and
//! the two would disagree the first time one of them changed.
//!
//! It also does not invent placements. `allowed_placements` is E5's solver
//! output — the worlds that can satisfy what this component does — and nothing
//! here re-derives it from the capability list.
//!
//! # The rule, and why it is a subset
//!
//! ```text
//! actual Wasm imports  ⊆  the contract's allowed imports
//! ```
//!
//! **Subset, never superset.** A component that imports fewer host functions
//! than it is allowed to is fine: dead code, a branch never compiled in, a
//! capability declared for a sibling. A component that imports even one it was
//! not allowed is not a component with a small mistake — it is a component
//! whose authority the compiler never approved, and the host has no basis for
//! deciding whether it should have it.
//!
//! The audit is **fail-closed**. An import the audit cannot parse counts as
//! undeclared, because "I could not tell what this asks for" and "this asks
//! for nothing" must never produce the same verdict.
//!
//! # A data artifact
//!
//! ADR-0018: the contract is JSON. The compiler emits it and the host reads
//! it; neither links the other. The host mirrors these types by field name,
//! exactly as `pw-materialize` mirrors the graph and `pw-resource` mirrors
//! `EntryIdentitySpec`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::effects::Inference;
use crate::hir::{DeclKind, Expr, Hir};
use crate::ontology::Ontology;
use crate::placement::{Demand, solve};
use crate::resolve::Workspace;
use crate::signatures::Signatures;

/// One capability a component needs, as the compiler derived it.
///
/// Structured rather than a string, because the host has to make decisions on
/// the parts. `database.read<Stores>` and `database.write<Stores>` differ in
/// the operation; `database.read<Stores>` and `database.read<Payments>` differ
/// in what they reach. A host that had to re-parse a string to see that would
/// be reimplementing this derivation, badly, at the point where it matters
/// most.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Capability {
    /// `database`, `secret`, `dom`. The restricted family E5's world table
    /// keys on.
    pub family: String,
    /// `read`, `write`, `mutate`. Empty when the effect names only a family.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub operation: String,
    /// The type argument, where the effect carries one: `Stores` in
    /// `database.read<Stores>`.
    ///
    /// Part of the capability's IDENTITY, not a decoration. E2D recorded what
    /// happens when it is dropped: `secret<Payments>` and `secret<Sessions>`
    /// collapse into one, and a component authorised for one reaches the
    /// other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
}

/// The version of the capability→representation mapping this build used.
///
/// Architect ruling, 2026-08-07:
///
/// > E9 is free later to change the inference algorithm, source notation,
/// > internal effect-row representation and polymorphism machinery without
/// > changing `CapabilityId(DatabaseRead, Stores)` — unless E9 actually proves
/// > our semantic capability ontology itself was wrong.
///
/// Recorded so a host can tell a mapping change from an authority change. Two
/// contracts with different mapping versions are not comparable; two with the
/// same version and different capabilities are a real difference.
pub const CAPABILITY_MAPPING: u32 = 1;

/// Why an effect could not be turned into a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotACapability {
    /// The family is not one this platform restricts. Not an error — an
    /// unrestricted effect simply needs no host authority.
    Unrestricted { family: String },
    /// The type argument names nothing the program declares.
    ///
    /// Refused rather than kept as a string: `database.read<Stores>` and
    /// `database.read<Stroes>` would otherwise be two different capabilities,
    /// one of which nothing will ever grant, and the failure would appear at
    /// deployment as "the node does not grant `database.read<Stroes>`".
    UnknownArgument { family: String, argument: String },
    /// **Nothing in the program declares this effect at all.**
    ///
    /// Distinct from `Unrestricted`, and the distinction is the point: one is
    /// "declared, and needs no authority", the other is "there is no such
    /// effect here". Collapsing them is what a fallback does — it turns *I do
    /// not know* into *no restriction*, which is the shape that made
    /// `secret<Payments>` placeable in the browser.
    Undeclared { effect: String },
}

impl Capability {
    /// **Build a capability from an effect, against what the program declares.**
    ///
    /// The architect's requirement that capability identity come "from
    /// resolved platform declarations, not from parsing strings out of
    /// effect-row syntax".
    ///
    /// There was a second entry point — `Capability::resolve(effect, types)`,
    /// which passed `Ontology::default()` — and deleting the `worlds_for`
    /// fallback made it vacuous: an empty ontology declares nothing, so every
    /// call could only answer [`NotACapability::Undeclared`]. It survived
    /// exactly as long as there was a table underneath to answer for it, which
    /// is the whole argument against having had one.
    ///
    /// **Whether an effect needs host authority is a DECLARED fact.** Architect
    /// ruling, 2026-08-07:
    ///
    /// > An ordinary Pleris operation like `layout.measure` doesn't necessarily
    /// > need to give arbitrary application code a raw DOM capability. […]
    /// > rather than granting every browser component unrestricted DOM
    /// > authority merely because it performs UI work.
    ///
    /// `World::worlds_for` answered a different question — where an effect is
    /// *meaningful* — and reading its restricted-family list as "needs a
    /// capability" is what made `layout.measure` ask a host to grant the layout
    /// engine. **The declaration is now the only thing that decides.**
    ///
    /// There was a fallback here, and deleting it is the substance of the
    /// architect's step 6:
    ///
    /// > ontology doesn't know effect → diagnostic already exists → contract
    /// > generation is Blocked. No legacy interpretation.
    ///
    /// The fallback read `worlds_for` when nothing declared the effect, so an
    /// undeclared spelling the old table happened to recognise still produced a
    /// capability. It was the conservative direction and it was still split
    /// semantic authority: an effect this program never declared got authority
    /// from a table in the compiler. An unresolved effect now yields
    /// [`NotACapability::Undeclared`], which a caller must handle as a refusal
    /// rather than as "unrestricted".
    pub fn resolve(
        effect: &str,
        declared_types: &BTreeSet<String>,
        ontology: &Ontology,
    ) -> Result<Capability, NotACapability> {
        let c = Capability::parse(effect);
        match ontology.declared_for(effect) {
            Some(decl) if decl.capability.is_none() => {
                return Err(NotACapability::Unrestricted { family: c.family });
            }
            Some(_) => {}
            None => {
                return Err(NotACapability::Undeclared {
                    effect: effect.to_string(),
                });
            }
        }
        if let Some(a) = &c.argument
            && !declared_types.contains(a)
        {
            return Err(NotACapability::UnknownArgument {
                family: c.family.clone(),
                argument: a.clone(),
            });
        }
        Ok(c)
    }

    /// Parse `family.operation<Argument>` as an effect row writes it.
    ///
    /// Not the construction path a contract uses — see [`Capability::resolve`].
    /// Kept public because a HOST reading a contract has only the canonical
    /// text and must be able to recover the parts.
    pub fn parse(effect: &str) -> Capability {
        let (head, argument) = match effect.split_once('<') {
            Some((h, rest)) => (h, Some(rest.trim_end_matches('>').to_string())),
            None => (effect, None),
        };
        let (family, operation) = match head.split_once('.') {
            Some((f, o)) => (f.to_string(), o.to_string()),
            None => (head.to_string(), String::new()),
        };
        Capability {
            family,
            operation,
            argument,
        }
    }

    /// The canonical text form, and the one an import names.
    pub fn name(&self) -> String {
        let mut out = self.family.clone();
        if !self.operation.is_empty() {
            out.push('.');
            out.push_str(&self.operation);
        }
        if let Some(a) = &self.argument {
            out.push('<');
            out.push_str(a);
            out.push('>');
        }
        out
    }
}

/// A host function this component is allowed to import, and what authorises it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Import {
    /// The interface, in the Wasm component naming the host will use.
    pub interface: String,
    /// The function within it.
    pub name: String,
    /// Which required capability makes this import legitimate — for a HOST
    /// import only.
    ///
    /// Present so the audit's refusal can say *why*: "this module imports
    /// `pw:host/database#read`, and nothing in its effect row asks for
    /// `database.read`" is actionable; "undeclared import" is not.
    ///
    /// **One of possibly several.** See `capabilities` — this field is the
    /// first of them, kept because the audit's message reads better with one
    /// name and because a host that only ever saw one should not have to
    /// change to keep working.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub capability: String,
    /// **Every capability invoking this operation requires.**
    ///
    /// Architect ruling, 2026-08-20:
    ///
    /// > Don't encode `HostCall { capability }` as though every operation has
    /// > one authority. […] operation → zero or more capabilities; capability →
    /// > zero or more operations. No 1:1 assumption in either direction.
    ///
    /// May be empty: an operation can be placement-constrained and need no
    /// authority at all, which is already true of the browser-semantic effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,
    /// **The ABI, for a host operation.**
    ///
    /// Carried so that one answer reaches the whole chain — contract → Backend
    /// IR → core Wasm import types → WIT → the E8 artifact audit. The encoder
    /// derived it from CALL SITES until 2026-08-20, and `Carts.add` and
    /// `Carts.clear` sharing `database.write<Carts>` is what proved that could
    /// not work: the two have different ABIs and there was no single arity to
    /// derive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
    /// **Who defines this operation**, as distinct from who implements it
    /// today.
    ///
    /// Architect ruling, 2026-08-20: *"the host process currently provides the
    /// implementation" is a deployment fact; it doesn't need to collapse their
    /// semantic ownership.* `pw:host/session#read` is a platform facility with
    /// platform ABI-stability expectations; `store:data/carts#add` is the
    /// application's own, externally implemented for now.
    #[serde(default)]
    pub owner: Ownership,
    /// What kind of dependency this is.
    ///
    /// Architect ruling, 2026-08-07:
    ///
    /// > A component import isn't automatically "authority". It is a dependency
    /// > on another component whose own authority is independently constrained.
    ///
    /// A page reading `query Store(id)` depends on the `Store` component; it
    /// does not thereby acquire that component's `database.read`. Flattening
    /// the two into one set would make every caller of a privileged component
    /// look privileged.
    #[serde(default)]
    pub kind: ImportKind,
    /// **Where its answer must hold an invariant** (ADR-0179): a data
    /// layer's answer comes from outside the program, and a host checks it
    /// before the component reads it. `argument` is 0, the answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bounded: Vec<Bounded>,
    /// **The event this import writes to the outbox** (ADR-0208), by its
    /// declaration's path, `Events.CartChanged`: the host commits what it is
    /// given as that event, with the command's writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// **The query whose entry this import drops** (ADR-0209), by its path,
    /// `store.page.Cart`: the host drops the entry it is given once the
    /// command's writes commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidates: Option<String>,
    /// **The positions this import leaves to every value** (ADR-0256): an
    /// `invalidates` key's `_`, `[1]` for `Timeline(current_session(), _)`.
    /// The host is given the value at each other position, in order, and
    /// drops every entry at them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub every: Vec<usize>,
    /// **The resource whose value this import answers** (ADR-0277), by its
    /// path, `store.page.Menu`: a materialization's read, which the host
    /// answers from that resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reads: Option<String>,
    /// **The run this import schedules, or cancels** (the
    /// scheduled-commands track): the command's component, which the host
    /// runs, and how. The host writes it with the scheduling command's
    /// writes, or not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules: Option<Schedules>,
}

/// **What a schedules' import does** (the scheduled-commands track): for
/// the command `command`, a component id, `at` an instant or `after` a
/// duration, each given after the command's arguments as the `Int` it is,
/// or `cancel` the one run pending for those arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Schedules {
    pub command: String,
    pub how: String,
}

/// **One place a value from outside must hold an invariant** (ADR-0179):
/// the value at `argument`, then each step into it, and the bounds the
/// opaque type there states. A step is a record's field, `*` for each item
/// of a list, `some`, `ok` or `err`, a case by its name, or a field of a
/// case's payload by its position. All as the component names them, so a
/// host checks a value without knowing the program's types.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Bounded {
    pub argument: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    /// The opaque type whose invariant it is: `domain.PositiveInt`.
    #[serde(rename = "type")]
    pub ty: String,
    /// The invariant as written: `value >= 1`.
    pub holds: String,
    /// What the bounds are on: the value, or a `String`'s length in code
    /// points (ADR-0225). A value's is not written, so a contract that
    /// bounds none is as it was.
    #[serde(default, skip_serializing_if = "Measure::is_value")]
    pub measure: Measure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<i64>,
}

/// **What a bound is on** (ADR-0225): a value, or a `String`'s length, in
/// code points, as `String.length` counts them (ADR-0040).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    #[default]
    Value,
    Length,
}

impl Measure {
    fn is_value(&self) -> bool {
        *self == Measure::Value
    }
}

/// **Every invariant a value of `t` holds, wherever it is in the value**
/// (ADR-0179), for the value at `argument`. A type that contains itself
/// crosses as its nodes (ADR-0194), so its parts are each node's: the path
/// goes through every node, `*`, once, and a list of itself in a node is
/// indices, which hold nothing.
pub(crate) fn bounded_in(
    sigs: &Signatures,
    t: &crate::resolved::ResolvedType,
    argument: usize,
) -> Vec<Bounded> {
    let mut out = Vec::new();
    bounds_within(
        sigs,
        &t.semantic_key(),
        argument,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut out,
    );
    out
}

/// **Every invariant a declaration's parameters hold** (ADR-0179), each
/// parameter's [`bounded_in`] at its position: what a host holds a value
/// from outside to wherever one is given to the declaration, an export's
/// arguments and a page's parameters alike (ADR-0346, argument invariants).
pub(crate) fn bounded_params(sigs: &Signatures, def: crate::resolve::DefId) -> Vec<Bounded> {
    sigs.by_def(def)
        .map(|s| {
            s.params
                .iter()
                .enumerate()
                .filter_map(|(i, p)| Some(bounded_in(sigs, p.as_ref()?.resolved()?, i)))
                .flatten()
                .collect()
        })
        .unwrap_or_default()
}

fn bounds_within(
    sigs: &Signatures,
    key: &crate::resolved::TypeKey,
    argument: usize,
    path: &mut Vec<String>,
    seen: &mut Vec<crate::resolve::DefId>,
    out: &mut Vec<Bounded>,
) {
    use crate::resolved::{Builtin, TypeKey};
    let step = |name: String,
                key: &TypeKey,
                path: &mut Vec<String>,
                seen: &mut Vec<_>,
                out: &mut Vec<_>| {
        path.push(name);
        bounds_within(sigs, key, argument, path, seen, out);
        path.pop();
    };
    match key {
        // A set crosses as a list does (`list<t>`, ADR-0057).
        TypeKey::Builtin(Builtin::List | Builtin::Set, args) => {
            if let Some(item) = args.first() {
                step("*".into(), item, path, seen, out);
            }
        }
        // A map crosses as its entries, each a key and its value (`list<tuple
        // <k, v>>`, ADR-0057): each entry's key at `0` and its value at `1`.
        TypeKey::Builtin(Builtin::Map, args) => {
            path.push("*".into());
            for (i, arg) in args.iter().take(2).enumerate() {
                step(i.to_string(), arg, path, seen, out);
            }
            path.pop();
        }
        TypeKey::Builtin(Builtin::Option, args) => {
            if let Some(some) = args.first() {
                step("some".into(), some, path, seen, out);
            }
        }
        TypeKey::Builtin(Builtin::Result, args) => {
            for (case, arg) in ["ok", "err"].iter().zip(args) {
                step((*case).into(), arg, path, seen, out);
            }
        }
        TypeKey::Nominal(def, args) => {
            if seen.contains(def) {
                return;
            }
            let Some(facts) = sigs.type_decl(*def) else {
                return;
            };
            if let Some(inv) = facts.invariant.as_ref().filter(|i| i.unread.is_empty()) {
                out.push(Bounded {
                    argument,
                    path: path.clone(),
                    ty: sigs.path_of(*def).unwrap_or_default().to_string(),
                    holds: inv.written.clone(),
                    measure: match inv.measure {
                        crate::hir::Measure::Value => Measure::Value,
                        crate::hir::Measure::Length => Measure::Length,
                    },
                    at_least: inv.at_least.and_then(|b| i64::try_from(b).ok()),
                    at_most: inv.at_most.and_then(|b| i64::try_from(b).ok()),
                });
            }
            // A field's or a case's type, under the arguments `def` is applied
            // to: `Money<USD>`'s fields as `USD`'s.
            let applied = |t: &crate::resolved::TypeResolution| {
                t.resolved()
                    .map(|r| substituted_key(&r.semantic_key(), *def, args))
            };
            // As its nodes: each one's parts, every node at once.
            let nodes = crate::recursion::contains_itself(sigs, *def);
            if nodes {
                path.push("*".into());
            }
            seen.push(*def);
            for (name, t) in facts.record.iter().flatten() {
                if let Some(k) = applied(t) {
                    step(crate::wit::ident(name), &k, path, seen, out);
                }
            }
            for (case, fields) in facts.variants.iter().flatten() {
                path.push(crate::wit::ident(case));
                match fields.as_slice() {
                    [only] => {
                        if let Some(k) = applied(only) {
                            bounds_within(sigs, &k, argument, path, seen, out);
                        }
                    }
                    several => {
                        for (i, t) in several.iter().enumerate() {
                            if let Some(k) = applied(t) {
                                step(i.to_string(), &k, path, seen, out);
                            }
                        }
                    }
                }
                path.pop();
            }
            if let Some(k) = facts.representation.as_ref().and_then(applied) {
                bounds_within(sigs, &k, argument, path, seen, out);
            }
            seen.pop();
            if nodes {
                path.pop();
            }
        }
        _ => {}
    }
}

/// `key` with `binder`'s type parameters replaced by `args`.
fn substituted_key(
    key: &crate::resolved::TypeKey,
    binder: crate::resolve::DefId,
    args: &[crate::resolved::TypeKey],
) -> crate::resolved::TypeKey {
    use crate::resolved::TypeKey;
    match key {
        TypeKey::Parameter { binder: b, index } if *b == binder => args
            .get(*index as usize)
            .cloned()
            .unwrap_or_else(|| key.clone()),
        TypeKey::Builtin(c, xs) => TypeKey::Builtin(
            *c,
            xs.iter()
                .map(|x| substituted_key(x, binder, args))
                .collect(),
        ),
        TypeKey::Nominal(d, xs) => TypeKey::Nominal(
            *d,
            xs.iter()
                .map(|x| substituted_key(x, binder, args))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Who defines an operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// The Pleris platform. `pw:host/…`.
    Platform,
    /// The application or its deployment. Everything else — and the default,
    /// because assuming an unlabelled operation is the platform's would be the
    /// generous direction.
    #[default]
    External,
}

/// A host operation's ABI, as the checker established it.
///
/// Written types rather than resolved ids: the contract is a DATA ARTIFACT
/// (ADR-0018) that a host reads without linking this compiler, and a `DefId`
/// means nothing outside it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Signature {
    /// Stable resolved parameter identities, preserving nominal and generic distinctions.
    pub params: Vec<crate::resolved::StableTypeId>,
    /// The resolved result identity, not a source spelling.
    pub result: crate::resolved::StableTypeId,
}

impl Signature {
    /// `(SessionId, MenuItemId) -> Cart`. For a refusal message, so a
    /// disagreement about an ABI reads as two shapes rather than two debug
    /// dumps.
    pub fn render(&self) -> String {
        format!(
            "({}) -> {}",
            self.params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            self.result
        )
    }
}

/// Where an import's implementation comes from, and therefore what constrains
/// it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// Supplied by the host or environment. Constrained by
    /// `required_capabilities` and by what the node grants.
    #[default]
    HostCapability,
    /// An export of another application component. Constrained by that
    /// component's own contract, which the host checks independently.
    Component,
}

impl Import {
    /// The wire form the audit compares: `interface#name`.
    pub fn key(&self) -> String {
        format!("{}#{}", self.interface, self.name)
    }
}

/// Something the component provides to the host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Export {
    pub name: String,
    /// The declaration kind it came from: `query`, `command`, `page`.
    pub kind: String,
    /// **How this edge may be bound.** Architect ruling, 2026-08-07:
    ///
    /// > Remote capability is a property of an interface edge, not a component:
    /// > `get(StoreId) -> Result<Store, StoreError>` is probably remotable and
    /// > `with_transaction(fn(OpenTransaction) -> T)` is not, in the same
    /// > component.
    ///
    /// Which is why it is here and not a seventh field on the contract. A
    /// per-component answer would have to be the conjunction over its exports,
    /// making a whole component local because one function holds a handle.
    ///
    /// `#[serde(default)]` so a contract written before this field parses: it
    /// reads as `local: direct, remote: transferable`, which is what the host
    /// assumed when there was no field at all.
    #[serde(default)]
    pub binding: crate::binding::BindingSupport,
    /// **Where this export is in the compiled component**: the exported
    /// interface and the function, as the component names them. E10-I,
    /// 2026-09-24: a host calling a compiled command must find it, and the
    /// naming is the compiler's projection (`wit::component_export`) — a host
    /// that re-derived it from `name` would be the second answer ADR-0020
    /// exists to prevent. Optional and not part of `abi_schema`, like
    /// `binding`: a contract written before it parses, and its identity is
    /// unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ComponentExport>,
}

/// One precondition the deployment must approve before invoking an export.
///
/// The predicate name belongs to the deployment's authorization vocabulary.
/// Arguments are zero-based indices into the export's already-typed command
/// parameters, so authorization receives values without reparsing source text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorizationRequirement {
    pub predicate: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<usize>,
}

/// An export's place in a component: `pw:app/store-page-add-to-cart-api@0.1.0`
/// and `add-to-cart`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentExport {
    pub interface: String,
    pub function: String,
    /// Invocation preconditions from the declaration's `requires` clause.
    ///
    /// Kept on the export rather than the component because authorization is a
    /// property of one invocation edge. A component may eventually expose more
    /// than one operation with different preconditions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authorization: Vec<AuthorizationRequirement>,
    /// The type of the key a retried invocation is recognised by, from the
    /// declaration's `idempotent_by` clause (ADR-0121).
    ///
    /// On the export for the reason `authorization` is: the host must refuse
    /// an invocation of an idempotent command that carries no key, and run
    /// one key's invocation at most once. A clause read by nothing was the
    /// defect: `idempotent_by InteractionId` was checked and never held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_by: Option<String>,
    /// **Who invokes it, where no reader does** (NEXT 6c): `Payments`, for
    /// the command a payment's outcome invokes. On the export for the reason
    /// `authorization` is: the host answers a press naming it as a command
    /// that is not there, and invokes it only on its provider's word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoked_by: Option<String>,
    /// **Where an argument must hold an invariant** (ADR-0179): a browser's
    /// request is a claim, and a host checks it before the component runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bounded: Vec<Bounded>,
}

/// **What the compiler tells the host about one component.**
///
/// Frozen at E8-0: E8 may consume every field and must add none. A host that
/// needed a seventh field would be asking the compiler a question the compiler
/// has no business answering, or answering one itself that it should be asking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentContract {
    /// The component's semantic identity: its module path.
    pub component_id: String,
    /// Which capability→representation mapping produced this contract.
    ///
    /// Six semantic fields plus one about the artifact itself. A host reading a
    /// mapping version it does not know must refuse rather than interpret the
    /// capabilities under its own — see [`CAPABILITY_MAPPING`].
    #[serde(default = "default_mapping")]
    pub capability_mapping: u32,
    /// A hash over the component's INTERFACE — its exports, their kinds and
    /// invocation preconditions, its capabilities and its placements. Not over
    /// source text: a comment must
    /// not change it, and a new capability must.
    pub abi_schema: String,
    pub required_capabilities: Vec<Capability>,
    /// The worlds E5's solver found can satisfy this component.
    ///
    /// Empty means *nowhere* — a component that cannot run anywhere, which the
    /// host must refuse rather than place somewhere and hope.
    pub allowed_placements: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

fn default_mapping() -> u32 {
    CAPABILITY_MAPPING
}

/// What an audit of a built artifact concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audit {
    /// Every actual import is in the allowed set.
    Satisfied,
    /// At least one is not. Carries all of them, because a build that fixes
    /// the first and rediscovers the second is a slow way to learn the shape
    /// of a problem.
    Undeclared(Vec<String>),
}

impl Audit {
    pub fn is_satisfied(&self) -> bool {
        matches!(self, Audit::Satisfied)
    }
}

/// **Does the artifact's ABI for each operation match the one the contract
/// fixed?**
///
/// The second question in the artifact-audit layer, and deliberately not folded
/// into [`audit`]. That one decides *identity* — "did you import only the
/// operations your contract names?" — and it is satisfied by an artifact that
/// imports `store:data/carts#add` no matter what type it gives it.
///
/// Architect ruling, 2026-08-20:
///
/// > I would also mutate only the operation's signature while preserving its
/// > `ImportId`: `same interface#operation / wrong ABI → rejected`. That becomes
/// > important immediately for the Component Model.
///
/// It matters there because a component's import is a *name plus a type*, and
/// two builds can agree on every name while disagreeing about what crosses.
/// Nothing downstream would notice: the module validates, the world resolves,
/// and [`audit`] is satisfied — the arguments are simply read as the wrong
/// shape.
///
/// An operation the contract declares no signature for is **not** checked
/// rather than assumed to match. That is a contract that has not said, and a
/// silent pass here would make "the contract fixed this ABI" indistinguishable
/// from "the contract is quiet about it".
pub fn abi(contract: &ComponentContract, actual: &[(String, Signature)]) -> Audit {
    let declared: BTreeMap<String, &Signature> = contract
        .imports
        .iter()
        .filter_map(|i| i.signature.as_ref().map(|s| (i.key(), s)))
        .collect();
    let mut wrong: Vec<String> = actual
        .iter()
        .filter_map(|(key, sig)| match declared.get(key) {
            Some(want) if *want != sig => Some(format!(
                "{key}: the contract fixes {}, the artifact declares {}",
                want.render(),
                sig.render()
            )),
            // Present with no declared ABI, or absent from the contract
            // entirely — the second is `audit`'s refusal to make, and making it
            // here too would report one defect as two.
            _ => None,
        })
        .collect();
    wrong.sort();
    wrong.dedup();
    match wrong.is_empty() {
        true => Audit::Satisfied,
        false => Audit::Undeclared(wrong),
    }
}

/// **Does this contract possess the authority its own operations require?**
///
/// A separate layer from the artifact audit and from admission. Architect
/// ruling, 2026-08-20:
///
/// ```text
/// artifact audit        "Did you import only the operations your contract names?"
/// contract consistency  "Does your contract possess every authority those
///                        operations require?"
/// admission             "Does this node actually grant that authority?"
/// ```
///
/// > Do not combine those into one check.
///
/// The relation is **⊆, never equality**:
///
/// > because a component can have required authority whose use isn't
/// > represented by a particular callable import shape, and we shouldn't force
/// > the ABI to become the definition of semantic effects.
///
/// So a component may require more than its imports do — the semantic pipeline
/// is responsible for why — and it may never require less. An operation
/// requiring nothing is satisfied by a component holding nothing, which is the
/// case the browser-semantic effects already need.
pub fn consistent(contract: &ComponentContract) -> Audit {
    let held: BTreeSet<String> = contract
        .required_capabilities
        .iter()
        .map(|c| c.name())
        .collect();
    let mut missing = Vec::new();
    for i in &contract.imports {
        for c in &i.capabilities {
            if !held.contains(&c.name()) {
                missing.push(format!("{} requires {}", i.key(), c.name()));
            }
        }
    }
    missing.sort();
    missing.dedup();
    match missing.is_empty() {
        true => Audit::Satisfied,
        false => Audit::Undeclared(missing),
    }
}

/// **The rule.** Actual imports ⊆ allowed imports.
///
/// Fail-closed by construction: the allowed set is what the contract lists, and
/// anything not in it is undeclared. There is no "unknown interface" branch to
/// forget, because there is no lookup — only membership.
pub fn audit(contract: &ComponentContract, actual: &[String]) -> Audit {
    let allowed: BTreeSet<String> = contract.imports.iter().map(Import::key).collect();
    let mut undeclared: Vec<String> = actual
        .iter()
        .filter(|i| !allowed.contains(*i))
        .cloned()
        .collect();
    undeclared.sort();
    undeclared.dedup();
    if undeclared.is_empty() {
        Audit::Satisfied
    } else {
        Audit::Undeclared(undeclared)
    }
}

/// Is this declaration something a host runs?
///
/// A type or an import declaration has no authority to describe, and a plain
/// `fn` is compiled into its callers rather than instantiated on its own.
///
/// # `view` was missing, and its absence was reading as an answer
///
/// Architect ruling, 2026-08-10:
///
/// > A top-level `view` is executable semantic code: it can render, contain
/// > captures, contain deferred handlers, acquire effects, and have placement
/// > constraints. So I would not normalize `view → no ComponentContract` as
/// > intentional. Unless `view` were explicitly defined as nothing more than an
/// > inline syntactic macro — which it clearly isn't in the current language —
/// > it should have a semantic contract.
///
/// The evidence-reachability audit is what forced this. A-014 was being read as
/// evidence that a page does not inherit its handler's authority, and it
/// declares a `view` — so there was no contract, and its before-state was not
/// *requires nothing* but *there is nothing to ask*. An absence that reads as a
/// claim is worse than a wrong claim, because nothing disagrees with it.
fn component_kind(kind: DeclKind) -> Option<&'static str> {
    Some(match kind {
        DeclKind::Query => "query",
        DeclKind::Command => "command",
        DeclKind::Page => "page",
        DeclKind::Layout => "layout",
        DeclKind::Component => "component",
        DeclKind::View => "view",
        _ => return None,
    })
}

/// **What a call's or a function value's path names, as the backend
/// resolves it** (`backend::lower`): a dotted path through the modules, a
/// bare name among the terms in scope.
fn compiled_as(ws: &Workspace, unit: usize, path: &str) -> Option<crate::resolve::DefId> {
    use crate::resolve::{Namespace, Resolution};
    let found = match path.contains('.') {
        true => ws.resolve_path(unit, path),
        false => ws.resolve_in(unit, Namespace::Term, path),
    };
    match found {
        Resolution::Local(def) | Resolution::Imported { def, .. } => Some(def),
        _ => None,
    }
}

/// **The host operations this declaration's code calls**: its body's, and
/// those of each function compiled into it with it (ADR-0283).
///
/// One import per CALLABLE, with the ABI and the authority read off the
/// operation's own declaration. Architect ruling, 2026-08-20: a capability
/// authorizes an operation and does not identify one, so the import cannot be
/// keyed on the capability — `Carts.add` and `Carts.clear` share
/// `database.write<Carts>` and differ in arity.
///
/// `backend::host_binding` is the one reader of the `host` policy, used here
/// and by lowering, so the contract and the IR cannot disagree about which
/// callables a component depends on.
#[allow(clippy::too_many_arguments)]
fn host_calls(
    inference: &Inference<'_>,
    sigs: &Signatures,
    hirs: &[&Hir],
    unit: usize,
    hir: &Hir,
    id: crate::hir::DeclId,
    declared_types: &BTreeSet<String>,
    ontology: &Ontology,
) -> Vec<Import> {
    let Some(body_id) = hir.decl(id).body else {
        return Vec::new();
    };
    let body = hir.body(body_id);
    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    // What the body calls, and what its events' and invalidated entries'
    // keys call: the command computes those (ADR-0208, ADR-0209).
    let mut exprs = body.walk();
    let emitted = hir.decl(id).policy("emits");
    let invalidated = hir.decl(id).policy("invalidates");
    for root in emitted.iter().chain(&invalidated).flat_map(|p| &p.roots) {
        exprs.extend(body.walk_from(root.root));
    }
    // **And what the code compiled with it calls** (ADR-0283). A function
    // with a body and no `host` or `intrinsic` binding is compiled into this
    // component, called (inlined, ADR-0039 §4, or beside the export where it
    // recurses, ADR-0050) or named as a value (ADR-0052): each is walked
    // once, in its own unit, resolved as the backend resolves it. A query or
    // a command it calls is another component, its dependency
    // (`component_calls`), and a handler's code is the handler's: an `on:`
    // attribute's lambda, or the declaration it names (ADR-0199). Until
    // ADR-0283 the declaration's own body was read alone, so a host call one
    // function down was in no world: `pw check` passed, and `pw build`
    // refused it.
    let handlers: Vec<crate::hir::Span> = crate::resume::handler_lambdas(body)
        .into_iter()
        .map(|e| match body.expr(e) {
            Expr::Lambda { body: inner, .. } => body.expr_span(*inner),
            _ => body.expr_span(e),
        })
        .collect();
    let in_handler = |e: crate::hir::ExprId| {
        let at = body.expr_span(e);
        handlers
            .iter()
            .any(|h| at.start >= h.start && at.end <= h.end)
    };
    let exprs: Vec<crate::hir::ExprId> = exprs.into_iter().filter(|e| !in_handler(*e)).collect();
    let root = crate::resolve::DefId { unit, decl: id.0 };
    let mut compiled: BTreeSet<crate::resolve::DefId> = BTreeSet::from([root]);
    let mut pending = vec![(root, exprs)];
    let mut callees: Vec<crate::resolve::DefId> = Vec::new();
    while let Some((at, exprs)) = pending.pop() {
        let h = hirs[at.unit];
        let at_id = crate::hir::DeclId(at.decl);
        let Some(b) = h.decl(at_id).body else {
            continue;
        };
        let body = h.body(b);
        // A name a binding in scope holds is that binding's, whatever
        // declaration shares it (ADR-0068).
        let lexical = crate::lexical::Lexical::build_in(h, at_id);
        let local = |e| lexical.as_ref().is_some_and(|l| l.binder(e).is_some());
        let called: BTreeSet<crate::hir::ExprId> = exprs
            .iter()
            .filter_map(|e| match body.expr(*e) {
                Expr::Call { callee, .. } => Some(*callee),
                _ => None,
            })
            .collect();
        for expr in exprs {
            let (named, is_call) = match body.expr(expr) {
                Expr::Call { callee, .. } => (*callee, true),
                Expr::Name(_) | Expr::Field { .. } if !called.contains(&expr) => (expr, false),
                _ => continue,
            };
            if local(named) {
                continue;
            }
            let path = crate::infer::path_of(body, named);
            if path.is_empty() {
                continue;
            }
            if is_call && let Some(def) = inference.resolved_from(at.unit, &path) {
                callees.push(def);
            }
            let Some(def) = compiled_as(sigs.workspace(), at.unit, &path) else {
                continue;
            };
            let Some(callee) = crate::resolve::declaration(hirs, def) else {
                continue;
            };
            if let Some(b) = callee.body
                && callee.kind == DeclKind::Fn
                && crate::backend::host_binding(callee).is_none()
                && crate::backend::intrinsic_binding(callee).is_none()
                && compiled.insert(def)
            {
                pending.push((def, hirs[def.unit].body(b).walk()));
            }
        }
    }
    // And the outbox's function for each event it emits.
    callees.extend(
        emitted
            .iter()
            .flat_map(|p| &p.keys)
            .filter_map(|k| crate::backend::emitted_event(sigs.workspace(), unit, k)),
    );
    for def in callees {
        let Some(decl) = crate::resolve::declaration(hirs, def) else {
            continue;
        };
        // **The one canonical definition**, the same one `backend::lower`
        // consumes. The types are carried as WRITTEN here because a contract is
        // a data artifact (ADR-0018) and a `DefId` means nothing outside this
        // compiler, so the closure returns `Type::Unit` and the written forms
        // are read off the declaration below.
        let Some(signature) = sigs.by_def(def) else {
            continue;
        };
        let Some(callable) = crate::backend::callable_of(
            decl,
            def,
            signature,
            |_| Some(crate::backend::ir::Type::Unit),
            |effect| match Capability::resolve(effect, declared_types, ontology) {
                Ok(c) => Some(c),
                // An unrestricted family needs no host authority. The ONE place
                // that decides, and both readers of a callable consult it.
                Err(NotACapability::Unrestricted { .. }) => None,
                Err(_) => Some(Capability::parse(effect)),
            },
        ) else {
            continue;
        };
        // Preserve every slot. An unavailable semantic identity must not
        // become a shorter signature or silently acquire a Unit result.
        let Some(params) = signature
            .params
            .iter()
            .map(|p| sigs.stable_type(p.as_ref()?.resolved()?))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let result = match &signature.returns {
            Some(r) => match r.resolved().and_then(|t| sigs.stable_type(t)) {
                Some(t) => t,
                None => continue,
            },
            None => crate::resolved::StableTypeId::Primitive("Unit".into()),
        };
        let semantic_signature = Signature { params, result };
        if !seen.insert((callable.id.interface.clone(), callable.id.name.clone())) {
            continue;
        }
        let caps: Vec<Capability> = callable
            .required_capabilities
            .iter()
            .map(|c| c.0.clone())
            .collect();
        out.push(Import {
            interface: callable.id.interface,
            name: callable.id.name,
            capability: caps.first().map(|c| c.name()).unwrap_or_default(),
            capabilities: caps,
            signature: Some(semantic_signature),
            owner: match callable.binding {
                crate::backend::ir::ImportBinding::PlatformHost => Ownership::Platform,
                _ => Ownership::External,
            },
            kind: ImportKind::HostCapability,
            bounded: signature
                .returns
                .as_ref()
                .and_then(|r| r.resolved())
                .map(|t| bounded_in(sigs, t, 0))
                .unwrap_or_default(),
            event: (decl.kind == crate::hir::DeclKind::Event).then(|| signature.path.clone()),
            invalidates: None,
            every: Vec::new(),
            reads: None,
            schedules: None,
        });
    }
    // **And the invalidations' function for each entry it invalidates**
    // (ADR-0209): the query's parameters, under the outbox's authority, since
    // the entry is dropped once the writes commit and not before.
    for key in invalidated.iter().flat_map(|p| &p.keys) {
        let Some(def) = crate::backend::invalidated_query(sigs.workspace(), sigs, unit, key) else {
            continue;
        };
        let Some(signature) = sigs.by_def(def) else {
            continue;
        };
        // A position written `_` is every value there (ADR-0256): the
        // function takes the rest.
        let every = crate::backend::every_position(body, key, &signature.names);
        let Some(params) = signature
            .params
            .iter()
            .enumerate()
            .filter(|(i, _)| !every.contains(i))
            .map(|(_, p)| sigs.stable_type(p.as_ref()?.resolved()?))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let import = crate::backend::invalidation_binding(&signature.path, &every);
        if !seen.insert((import.interface.clone(), import.name.clone())) {
            continue;
        }
        let outbox = Capability::parse(crate::signatures::OUTBOX_WRITE);
        out.push(Import {
            interface: import.interface,
            name: import.name,
            capability: outbox.name(),
            capabilities: vec![outbox],
            signature: Some(Signature {
                params,
                result: crate::resolved::StableTypeId::Primitive("Unit".into()),
            }),
            owner: Ownership::Platform,
            kind: ImportKind::HostCapability,
            bounded: Vec::new(),
            event: None,
            invalidates: Some(signature.path.clone()),
            every,
            reads: None,
            schedules: None,
        });
    }
    // **And the schedules' function for each run a command schedules or
    // cancels** (the scheduled-commands track): the scheduled command's
    // parameters, and its time, an instant or a duration, as the `Int` it
    // is; under `clock.schedule`, since the host writes it with the
    // command's writes, or not at all.
    if hir.decl(id).kind == crate::hir::DeclKind::Command {
        for e in body.walk() {
            let crate::hir::Expr::Keyword {
                keyword, modifiers, ..
            } = body.expr(e)
            else {
                continue;
            };
            let how = match (keyword.as_str(), modifiers.get(1).map(String::as_str)) {
                ("schedule", Some(how @ ("at" | "after"))) => how,
                ("unschedule", None) => "cancel",
                _ => continue,
            };
            let Some(name) = modifiers.first() else {
                continue;
            };
            let (crate::resolve::Resolution::Local(def)
            | crate::resolve::Resolution::Imported { def, .. }) =
                sigs.workspace()
                    .resolve_in(unit, crate::resolve::Namespace::Term, name)
            else {
                continue;
            };
            let Some(signature) = sigs.by_def(def) else {
                continue;
            };
            let Some(mut params) = signature
                .params
                .iter()
                .map(|p| sigs.stable_type(p.as_ref()?.resolved()?))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if how != "cancel" {
                params.push(crate::resolved::StableTypeId::Primitive("Int".into()));
            }
            let import = crate::backend::schedule_binding(&signature.path, how);
            if !seen.insert((import.interface.clone(), import.name.clone())) {
                continue;
            }
            let scheduling = Capability::parse(crate::signatures::CLOCK_SCHEDULE);
            out.push(Import {
                interface: import.interface,
                name: import.name,
                capability: scheduling.name(),
                capabilities: vec![scheduling],
                signature: Some(Signature {
                    params,
                    result: crate::resolved::StableTypeId::Primitive("Unit".into()),
                }),
                owner: Ownership::Platform,
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: Some(Schedules {
                    command: component_id(hirs[def.unit], crate::hir::DeclId(def.decl)),
                    how: how.to_string(),
                }),
            });
        }
    }
    // **And the reads' function for each resource a materialization's body
    // reads** (ADR-0277): its parameters, answering its value, the platform's
    // to answer from it, under no authority of the reader's: what it reads
    // holds its own, as a page's dependency does.
    let decl = hir.decl(id);
    if decl.kind == crate::hir::DeclKind::Materialize && decl.ret.is_some() {
        for (name, _) in crate::graph::queried(body) {
            let ws = sigs.workspace();
            let found = match name.contains('.') {
                true => ws.resolve_path(unit, &name),
                false => ws.resolve_in(unit, crate::resolve::Namespace::Term, &name),
            };
            let (crate::resolve::Resolution::Local(def)
            | crate::resolve::Resolution::Imported { def, .. }) = found
            else {
                continue;
            };
            let Some(signature) = sigs.by_def(def) else {
                continue;
            };
            let Some(params) = signature
                .params
                .iter()
                .map(|p| sigs.stable_type(p.as_ref()?.resolved()?))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            // Its value: the `Ok` of a `Result` it answers.
            let Some(result) = signature
                .returns
                .as_ref()
                .and_then(|r| r.resolved())
                .and_then(|r| match r.as_builtin() {
                    Some(crate::resolved::Builtin::Result) => r.args().first(),
                    _ => Some(r),
                })
                .and_then(|r| sigs.stable_type(r))
            else {
                continue;
            };
            let import = crate::backend::read_binding(&signature.path);
            if !seen.insert((import.interface.clone(), import.name.clone())) {
                continue;
            }
            out.push(Import {
                interface: import.interface,
                name: import.name,
                capability: String::new(),
                capabilities: Vec::new(),
                signature: Some(Signature { params, result }),
                owner: Ownership::Platform,
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: Some(signature.path.clone()),
                schedules: None,
            });
        }
    }
    out
}

/// The other components this declaration's body calls.
///
/// Resolved, so `Cart` in one module and `Cart` in another are two
/// dependencies — the same requirement `docs/RISK_QUEUE.md` 34 is about.
fn component_calls(
    inference: &Inference<'_>,
    unit: usize,
    hir: &Hir,
    id: crate::hir::DeclId,
    components: &BTreeMap<crate::resolve::DefId, (String, String)>,
) -> Vec<Import> {
    let Some(body_id) = hir.decl(id).body else {
        return Vec::new();
    };
    let body = hir.body(body_id);
    // Calls, and the resources a body READS.
    //
    // `let menu = query Menu(id)` is not a `Call` — it is the keyword form the
    // dependency graph already knows how to find, so `graph::queried` is reused
    // rather than reimplemented. A second finder would agree until one of them
    // learned about a new form.
    let mut paths: Vec<String> = body
        .walk()
        .into_iter()
        .filter_map(|expr| match body.expr(expr) {
            Expr::Call { callee, .. } => Some(crate::infer::path_of(body, *callee)),
            _ => None,
        })
        .collect();
    paths.extend(
        crate::graph::queried(body)
            .into_iter()
            .map(|(name, _)| name),
    );

    let mut out = Vec::new();
    for path in paths {
        if path.is_empty() {
            continue;
        }
        // The COMPONENT of that name, preferentially.
        //
        // `store.page` declares `query Store` and imports `domain.Store`, and
        // the general resolver tries the type namespace first — which is how
        // `docs/RISK_QUEUE.md` recorded every page dependency resolving to a
        // record definition. Here the question is specifically "which component
        // does this call", so the module-qualified form is tried first and the
        // general resolution is the fallback.
        let qualified = if path.contains('.') {
            None
        } else {
            hir.module_of(id)
                .map(|m| format!("{m}.{path}"))
                .and_then(|q| inference.resolved_from(unit, &q))
                .filter(|d| components.contains_key(d))
        };
        let Some(def) = qualified.or_else(|| inference.resolved_from(unit, &path)) else {
            continue;
        };
        // Itself is not a dependency: a recursive query does not import its
        // own export, and recording it would make every host wire a component
        // to itself.
        if def == (crate::resolve::DefId { unit, decl: id.0 }) {
            continue;
        }
        if let Some((component, name)) = components.get(&def) {
            out.push(Import {
                interface: format!("pw:app/{component}"),
                name: name.clone(),
                capability: String::new(),
                capabilities: Vec::new(),
                signature: None,
                // **Decided, not defaulted.** A dependency on another
                // application component is the application's, so it is
                // `External` for the same reason `store:data/carts#add` is —
                // Pleris did not define it. The two are told apart by `kind`,
                // which is the question `owner` is not answering.
                owner: Ownership::External,
                kind: ImportKind::Component,
                // Another component's answer is built by checked code.
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            });
        }
    }
    out.sort();
    out.dedup();
    out
}

/// **A declaration's component id**: `module.Name`, or `Name` in the root
/// module. Ui declarations append `~ui`, preserving their distinct namespace.
///
/// The one derivation. A contract is filed under it, a component dependency
/// names it, and a compiled handler calls a command by it (`backend::js`). A
/// second derivation that disagreed would send a handler's call to a component
/// the host has no contract for, which is a refusal at best.
pub fn component_id(hir: &Hir, id: crate::hir::DeclId) -> String {
    let decl = hir.decl(id);
    let name = &decl.name;
    let path = match hir.module_of(id) {
        Some(module) if !module.is_empty() => format!("{module}.{name}"),
        _ => name.clone(),
    };
    // Term and Ui declarations may share a source path (A-008). The marker
    // cannot be written in a source name; adding another declaration never
    // changes an existing identity. All projections consume this authority.
    if crate::resolve::Namespace::of(decl.kind) == Some(crate::resolve::Namespace::Ui) {
        format!("{path}~ui")
    } else {
        path
    }
}

/// Derive every component's contract from a checked program.
///
/// One pass, from the same signatures and the same placement solver every other
/// analysis uses. Nothing here re-derives an effect row or a world.
pub fn contracts(hirs: &[&Hir], sigs: &Signatures, ws: &Workspace) -> Vec<ComponentContract> {
    contracts_with_pages(hirs, sigs, ws, &crate::page_values::analyze(hirs, ws, sigs))
}

pub(crate) fn contracts_with_pages(
    hirs: &[&Hir],
    sigs: &Signatures,
    ws: &Workspace,
    pages: &crate::page_values::Analysis,
) -> Vec<ComponentContract> {
    // INFERRED effects, not declared ones.
    //
    // A query's authority is in its body: `Menu` writes no effect row and
    // calls a helper that reads the database, and the host has to be told
    // about that read. A contract built from declared rows would hand the most
    // ordinary component in the language an empty capability set and let it
    // import whatever it liked — the audit would pass, because the allowed set
    // it compared against was the wrong one.
    let mut inference = Inference::new(sigs, ws);
    inference.run(hirs);

    contracts_inferred(hirs, sigs, ws, pages, &inference)
}

pub(crate) fn contracts_inferred(
    hirs: &[&Hir],
    sigs: &Signatures,
    ws: &Workspace,
    pages: &crate::page_values::Analysis,
    inference: &Inference<'_>,
) -> Vec<ComponentContract> {
    let privacy = crate::check::privacy_facts(hirs, sigs, inference);
    contracts_with_privacy(hirs, sigs, ws, pages, inference, &privacy)
}

/// Complete validation lends only facts belonging to this sealed source proof.
pub(crate) fn contracts_checked(
    checked: &crate::backend::lower::Checked<'_>,
    inference: &Inference<'_>,
) -> Vec<ComponentContract> {
    let cx = checked.context();
    contracts_with_privacy(
        cx.hirs,
        cx.sigs,
        cx.ws,
        checked.pages(),
        inference,
        checked.privacy(),
    )
}

fn contracts_with_privacy(
    hirs: &[&Hir],
    sigs: &Signatures,
    ws: &Workspace,
    pages: &crate::page_values::Analysis,
    inference: &Inference<'_>,
    privacy: &crate::check::PrivacyFacts,
) -> Vec<ComponentContract> {
    let labels = privacy.labels();
    // What each effect DECLARES about itself: whether it needs host authority,
    // and which interface serves it. Built once, beside the inference, because
    // both answer questions about the same rows.
    let ontology = Ontology::build_with(hirs, ws);

    // What the program says about each TYPE — is it a resource, is it produced
    // only by a scoped declaration. The same facts `resume.rs` asks about a
    // capture, asked here about a signature: one boundary-transfer analysis,
    // two policies over it.
    let type_facts = crate::boundary::TypeFacts::build(hirs, sigs);

    // Every type the program declares, for resolving a capability's argument.
    // A misspelled `database.read<Stroes>` must be refused here rather than
    // becoming a capability nothing will ever grant.
    let declared_types: BTreeSet<String> = hirs
        .iter()
        .flat_map(|h| h.all_decls().map(|(_, d)| d).collect::<Vec<_>>())
        .filter(|d| matches!(d.kind, DeclKind::Type | DeclKind::Opaque))
        .map(|d| d.name.clone())
        .collect();

    // Which declarations ARE components, by resolved identity. A call to one
    // is a component dependency; a call to a plain `fn` is not — a helper is
    // compiled into its caller and has no separate contract to depend on.
    let components: BTreeMap<crate::resolve::DefId, (String, String)> = hirs
        .iter()
        .enumerate()
        .flat_map(|(unit, hir)| {
            hir.all_decls()
                .filter(|(_, d)| {
                    component_kind(d.kind).is_some()
                        || (d.kind == DeclKind::Materialize && d.ret.is_some())
                })
                .map(|(id, d)| {
                    (
                        crate::resolve::DefId { unit, decl: id.0 },
                        (component_id(hir, id), d.name.clone()),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Declaration identity → its privacy label, across every unit. Built by
    // `check::Reads`, the derivation the checker uses, so the two cannot
    // drift: a declaration's own label, joined with what it reads through
    // what it calls (ADR-0118). Keyed by RESOLVED IDENTITY: `Cart` in one
    // module and `Cart` in another are two declarations with two labels.
    //
    // And what each holds where it runs (ADR-0282), as the checker's
    // placement reads it: a secret it answers or reads, never one it only
    // uses.
    // These labels are the exact owned privacy facts for this preparation.

    // The functions a page's template reads as members (ADR-0125): a host
    // calls each to compute what the page shows, so each is a component with
    // a contract of its own. Not a dependency target: a call to one from
    // Pleris code is still compiled into its caller.
    let members = &pages.members;

    let mut out = Vec::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            // Components are the things a host runs: the units with behaviour.
            // A type or an import declaration has no authority to describe.
            let member = members.contains(&crate::resolve::DefId { unit, decl: id.0 });
            // A materialization that derives its value is a component the host
            // runs to keep it (ADR-0277).
            let derives = decl.kind == DeclKind::Materialize && decl.ret.is_some();
            let Some(kind) = component_kind(decl.kind)
                .or(derives.then_some("materialization"))
                .or(member.then_some("function"))
            else {
                continue;
            };

            // ONE CONTRACT PER DECLARATION, not per module.
            //
            // Least authority. A module holding a database query and a browser
            // component would otherwise give the component the query's
            // `database.read` and the query the component's `dom.mutate` — and
            // then E5's solver would find nowhere either could run, because the
            // union of their demands is unsatisfiable. Aggregating by module
            // over-grants and under-places at the same time.
            let component_id = component_id(hir, id);

            // What this component performs, EXCLUDING its resumable handlers.
            //
            // A handler is a separately loaded unit with its own identity —
            // E7-L made that concrete — so its authority is its own. A page
            // that renders `on:press={.. => add_to_cart(..)}` would otherwise
            // require `database.write` to RENDER, and a host granting it would
            // give the render path authority it never uses.
            //
            // The handler's own authority is not lost: the command it calls is
            // itself a component with its own contract, so `add_to_cart`'s
            // `database.write` is recorded once, against the thing that
            // performs it.
            let effects: Vec<String> = match decl.body {
                Some(body_id) => {
                    let body = hir.body(body_id);
                    // Only the DEFERRED body, not the whole lambda.
                    //
                    // Architect ruling, 2026-08-07:
                    //
                    //   effects of reading/building captures -> render context
                    //   effects inside handler invocation    -> handler
                    //
                    // A lambda's descriptor — `resumable(captures = { .. })` —
                    // is evaluated while the page renders: the captures are
                    // read, typed and serialized then. Excluding it would let a
                    // page perform an effect at render time and attribute it to
                    // a button nobody has pressed.
                    //
                    // **A handler's, and no other lambda's** (ADR-0283): an
                    // `on:` attribute's lambda, or the declaration it names
                    // (ADR-0199). A lambda handed to `List.map` runs where it
                    // is written. Until ADR-0283 every lambda's body was
                    // deferred, so a query that read the database inside one
                    // required no capability, and could be placed in the
                    // browser.
                    let mut deferred: Vec<_> = crate::resume::handler_lambdas(body)
                        .into_iter()
                        .map(|e| match body.expr(e) {
                            Expr::Lambda { body: inner, .. } => *inner,
                            _ => e,
                        })
                        .collect();
                    // And a `<stream>`'s query (ADR-0148), for the same reason:
                    // the host runs it as its own component, under its own
                    // contract, as it runs a query a `let` reads. Until
                    // 2026-10-03 a page that streamed `Recommendations` was
                    // asked `network.fetch` to render, which `pw diff` showed.
                    for e in body.walk() {
                        let Expr::Template { roots, .. } = body.expr(e) else {
                            continue;
                        };
                        for n in body.walk_markup(roots) {
                            if let crate::hir::Node::Element { tag, attrs, .. } = body.node(n)
                                && tag == "stream"
                                && let Some(query) = crate::streams::query_attr(attrs)
                            {
                                deferred.push(query);
                            }
                        }
                    }
                    inference.effective_effects_excluding(unit, hir, id, &deferred)
                }
                // No body: an interface, or platform-external code. One
                // operation decides, for both branches.
                None => inference.effective_effects(unit, hir, id),
            };

            let capabilities: Vec<Capability> = effects
                .iter()
                .filter_map(
                    |e| match Capability::resolve(e, &declared_types, &ontology) {
                        Ok(c) => Some(c),
                        // An unrestricted family needs no host authority. This is
                        // the only reason an effect may leave no capability behind.
                        Err(NotACapability::Unrestricted { .. }) => None,
                        // An unresolvable argument KEEPS the capability.
                        //
                        // Dropping it would remove authority the program asked for,
                        // and the whole point of the direction argument is that
                        // over-stating is refused work while under-stating is
                        // authority nobody approved. The mistake surfaces at
                        // deployment as "the node does not grant
                        // `database.read<Stroes>`", which is a bad diagnostic —
                        // making it a build-time one is the next step and is
                        // recorded in `docs/NEXT.md`, not silently absorbed here.
                        Err(NotACapability::UnknownArgument { .. }) => Some(Capability::parse(e)),
                        // An effect nothing declares KEEPS its capability too,
                        // for the same reason and one more: the contract must
                        // not come out looking like a component that needs
                        // nothing. It cannot be deployed regardless —
                        // `allowed_placements` is empty below, because
                        // placement is `Blocked` for the same effect — and the
                        // build has already reported the row.
                        Err(NotACapability::Undeclared { .. }) => Some(Capability::parse(e)),
                    },
                )
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();

            // Placements from E5's solver, given the demand `check.rs` builds
            // for the same declaration. Not re-derived from the capability
            // list: a second derivation would agree until an input mattered.
            //
            // It did not build the same demand. Until the evidence-reachability
            // audit asked which observer could witness A-013's build-time
            // claim, this passed `declared: None` — so the author's pinned
            // world reached the CHECKER and never reached the ARTIFACT. A page
            // written `placement build` shipped a contract permitting the
            // browser, the edge and the origin, and a host reading it would
            // have granted any of them. `declared_world` is `check.rs`'s own
            // derivation, called rather than repeated.
            //
            // The label half was `Label::public()` until 2026-08-11 — E10-P —
            // and the deferral was deliberate: the label is the join of what a
            // body READS, and until policy values left the executable body tree
            // a body walk could not tell one from a term. Wiring it in first
            // would have given the contract's placement a second channel from
            // policy values.
            //
            // `check::declaration_label` and `Reads::holds` are the
            // checker's own derivation, called rather than repeated, so the
            // artifact and the diagnostic cannot disagree about how private
            // a component is (ADR-0282: until 2026-10-08 the contract's page
            // reading the session through a query of its own was allowed at
            // build).
            let def = crate::resolve::DefId { unit, decl: id.0 };
            let demand = Demand {
                effects: effects.clone(),
                label: crate::check::declaration_label(hir, labels, inference, unit, decl)
                    .join(&privacy.holds(def)),
                declared: crate::check::declared_world(hir, decl),
            };
            // A `Blocked` effect leaves this EMPTY, and empty is the contract's
            // existing word for "no node may run this". The host refuses it
            // everywhere, which is what "contract generation is Blocked" means
            // in the artifact's own vocabulary — no new field, and no way to
            // read an undeclared effect as an unconstrained one.
            let allowed_placements: Vec<String> = solve(&demand, &ontology)
                .feasible
                .into_iter()
                .map(|w| w.name().to_string())
                .collect();

            // Host authority, and component dependencies, in one list but
            // never one CLASS. A page that calls a privileged query depends on
            // it; it does not acquire its authority.
            //
            // **A host import is a CALLABLE, not a capability.** Architect
            // ruling, 2026-08-20, after the Wasm encoder found the conflation:
            //
            // > A capability authorizes an operation. It does not identify the
            // > operation.
            //
            // `Carts.add` and `Carts.clear` both require `database.write<Carts>`
            // and have different ABIs, so an import keyed on the capability had
            // no signature it could carry. Each host operation the body calls
            // is its own import, with its own ABI and its own authority set.
            let mut imports: BTreeSet<Import> = host_calls(
                inference,
                sigs,
                hirs,
                unit,
                hir,
                id,
                &declared_types,
                &ontology,
            )
            .into_iter()
            .collect();
            for dep in component_calls(inference, unit, hir, id, &components) {
                imports.insert(dep);
            }
            let imports: Vec<Import> = imports.into_iter().collect();

            // What this component provides. One entry today, because a
            // declaration is the unit; the field is plural because grouping
            // several declarations into one instantiable component is E8's
            // decision to make, not something to foreclose here.
            //
            // The binding support is per EXPORT for the same reason: remote
            // capability is a property of an interface edge, so a component
            // with a remotable query and a handle-passing helper says so about
            // each rather than about itself.
            // Every executable declaration, including UI, has one resolved
            // signature. Do not re-resolve the same written annotations here.
            let binding = match sigs.by_def(crate::resolve::DefId { unit, decl: id.0 }) {
                Some(signature) => crate::binding::binding_support(signature, &type_facts),
                None => crate::binding::BindingSupport {
                    local: crate::binding::LocalSupport::Direct,
                    remote: crate::binding::RemoteSupport::Undetermined {
                        positions: vec![crate::binding::Untransferable {
                            position: "interface".into(),
                            ty: None,
                            reason: "the declaring module has no resolved signature".into(),
                        }],
                    },
                },
            };
            let mut component = crate::wit::component_export(&component_id, &decl.name);
            component.authorization = authorization_of(decl);
            component.idempotent_by = decl
                .policy("idempotent_by")
                .map(|p| p.value.trim().to_string());
            component.invoked_by = decl
                .policy("invoked_by")
                .map(|p| p.value.trim().to_string());
            component.bounded = bounded_params(sigs, crate::resolve::DefId { unit, decl: id.0 });
            let exports = vec![Export {
                name: decl.name.clone(),
                kind: kind.to_string(),
                binding,
                component: Some(component),
            }];

            let abi_schema = schema_of(&component_id, &exports, &capabilities, &allowed_placements);
            out.push(ComponentContract {
                component_id,
                capability_mapping: CAPABILITY_MAPPING,
                abi_schema,
                required_capabilities: capabilities,
                allowed_placements,
                imports,
                exports,
            });
        }
    }

    // **Each computed part a host computes** (ADR-0226): the function the
    // compiler lifts its expression into, a component of its own, of kind
    // `derived`. It performs no effect and imports nothing; it runs where its
    // page runs, so it is placed where the page is.
    for d in &pages.derived {
        let page = component_id(hirs[d.page.unit], crate::hir::DeclId(d.page.decl));
        let allowed_placements = out
            .iter()
            .find(|c| c.component_id == page)
            .map(|c| c.allowed_placements.clone())
            .unwrap_or_default();
        let name = d.export();
        let exports = vec![Export {
            name: name.clone(),
            kind: "derived".to_string(),
            binding: crate::binding::BindingSupport {
                local: crate::binding::LocalSupport::Direct,
                remote: crate::binding::remote_support(&d.interface(), &type_facts),
            },
            component: Some(crate::wit::component_export(&d.component_id, &name)),
        }];
        let abi_schema = schema_of(&d.component_id, &exports, &[], &allowed_placements);
        out.push(ComponentContract {
            component_id: d.component_id.clone(),
            capability_mapping: CAPABILITY_MAPPING,
            abi_schema,
            required_capabilities: Vec::new(),
            allowed_placements,
            imports: Vec::new(),
            exports,
        });
    }
    out.sort_by(|a, b| a.component_id.cmp(&b.component_id));
    out
}

/// The authorization requirements a declaration contributes to its export.
///
/// Checked builds only reach the first arm. The fallback is deliberately
/// impossible to satisfy rather than empty: callers of this lower-level API can
/// construct HIR without running `check`, and malformed authorization must not
/// turn into "no authorization required".
fn authorization_of(decl: &crate::hir::Decl) -> Vec<AuthorizationRequirement> {
    let Some(policy) = decl.policy("requires") else {
        return Vec::new();
    };
    match crate::policy::predicates(&policy.value) {
        Ok(predicates) => predicates
            .into_iter()
            // `Anyone` holds for every reader: nothing to ask (ADR-0406).
            // PW0367 refuses it beside another predicate, and with
            // arguments: unchecked, `Anyone(n)` is no `Anyone`, and asks
            // what no reader holds rather than nothing (train-14's review).
            .filter(|predicate| {
                !(predicate.name == crate::policy::ANYONE && predicate.arguments.is_empty())
            })
            .map(|predicate| {
                if predicate.name == crate::policy::ANYONE {
                    return AuthorizationRequirement {
                        predicate: "__invalid_requires__".to_string(),
                        arguments: vec![usize::MAX],
                    };
                }
                AuthorizationRequirement {
                    predicate: predicate.name,
                    arguments: predicate
                        .arguments
                        .into_iter()
                        .map(|name| {
                            decl.params
                                .iter()
                                .position(|parameter| parameter.name == name)
                                .unwrap_or(usize::MAX)
                        })
                        .collect(),
                }
            })
            .collect(),
        Err(_) => vec![AuthorizationRequirement {
            predicate: "__invalid_requires__".to_string(),
            arguments: vec![usize::MAX],
        }],
    }
}

/// A hash over the interface, and over nothing else.
///
/// Exports, their kinds and invocation preconditions, the capabilities and the
/// placements. Not the bodies:
/// a component whose implementation changed but whose authority and interface
/// did not is the same contract, and a host that reloaded on every edit would
/// be reacting to noise. Not the source text either — see
/// `resume_artifacts`'s scheme 1 for what that costs.
fn schema_of(
    module: &str,
    exports: &[Export],
    capabilities: &[Capability],
    placements: &[String],
) -> String {
    let mut text = String::from(module);
    for e in exports {
        text.push_str(&format!("|{}:{}", e.kind, e.name));
        if let Some(component) = &e.component {
            for requirement in &component.authorization {
                text.push_str("|requires:");
                text.push_str(&requirement.predicate);
                text.push('(');
                for (i, argument) in requirement.arguments.iter().enumerate() {
                    if i > 0 {
                        text.push(',');
                    }
                    text.push_str(&argument.to_string());
                }
                text.push(')');
            }
            if let Some(key) = &component.idempotent_by {
                text.push_str("|idempotent_by:");
                text.push_str(key);
            }
            // Who may invoke it is its interface, as who must hold what is.
            if let Some(invoker) = &component.invoked_by {
                text.push_str("|invoked_by:");
                text.push_str(invoker);
            }
            // What the host checks of an argument before the component runs
            // (ADR-0179), as authorization is.
            for b in &component.bounded {
                text.push_str(&format!(
                    "|bounded:{}:{}:{}:{}",
                    b.argument,
                    b.path.join("."),
                    b.ty,
                    b.holds
                ));
            }
        }
    }
    for c in capabilities {
        text.push_str(&format!("|cap:{}", c.name()));
    }
    for p in placements {
        text.push_str(&format!("|at:{p}"));
    }
    hash(&text)
}

fn hash(content: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in content.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_keeps_its_type_argument() {
        // E2D's lesson, at the boundary this time. Dropping the argument
        // collapses `secret<Payments>` and `secret<Sessions>` into one
        // capability, and a component authorised for one reaches the other.
        let a = Capability::parse("secret<Payments>");
        let b = Capability::parse("secret<Sessions>");
        assert_ne!(a, b);
        assert_eq!(a.family, "secret");
        assert_eq!(a.argument.as_deref(), Some("Payments"));
        assert_eq!(a.name(), "secret<Payments>");
    }

    #[test]
    fn an_operation_is_part_of_the_identity() {
        assert_ne!(
            Capability::parse("database.read<Stores>"),
            Capability::parse("database.write<Stores>")
        );
    }

    #[test]
    fn a_bare_family_parses() {
        let c = Capability::parse("secret");
        assert_eq!(c.family, "secret");
        assert!(c.operation.is_empty());
        assert_eq!(c.name(), "secret");
    }
}
