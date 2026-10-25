//! E8 — the host decides whether authority physically exists.
//!
//! Architect ruling, 2026-08-07:
//!
//! > The compiler decides what authority code needs. The host decides whether
//! > that authority physically exists. Neither should reconstruct the other's
//! > answer.
//!
//! This crate answers two host-boundary questions:
//!
//! ```text
//! may this component be instantiated on this node?
//!   1. does the node grant every capability the contract requires?   topology
//!   2. is the node's world one the contract allows?                  placement
//!   3. does the artifact import only what the contract allows?       audit
//!
//! may this particular export be invoked for this caller?
//!   4. did the deployment approve every declared precondition?       authorization
//! ```
//!
//! It does not infer effects, solve placements, invent authorization predicates,
//! or decide what a component needs. Those answers arrive from the compiler or
//! the deployment and this boundary refuses execution when one is missing.
//!
//! # Placement is not capability
//!
//! Two checks, not one, because they can disagree in both directions. A node in
//! the right world may lack a capability (an edge node with no cache attached);
//! a node with every capability may be in the wrong world (an origin process
//! asked to run browser-only DOM code). Collapsing them would make one of those
//! two mistakes silently allowed, and which one depends on which way the
//! collapse went.
//!
//! # No ambient authority
//!
//! `docs/evidence/E0/spike-wasmtime-component.txt` measured what this is
//! defending against. A guest built with Rust `std` for `wasm32-wasip2`
//! declares ONE import in its WIT world and the compiled component demands
//! **fifteen** — fourteen `wasi:*` interfaces the world never mentioned,
//! injected by std's runtime initialization. Every one is authority nobody
//! granted, and it arrives without a single line of application code asking
//! for it.
//!
//! That is why the audit is on the ARTIFACT rather than on the source. The
//! compiler can only tell you what the program says it needs.
//!
//! # Handles, not secrets
//!
//! A granted capability yields a [`Handle`] — an opaque reference the host can
//! revoke and account for. The value behind it never enters the guest's memory
//! through this crate.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub mod plan;

// --- the compiler's artifact, mirrored --------------------------------------
//
// ADR-0018/ADR-0020: field-name mirrors of `pw_core::contract`. This crate does
// not link the compiler, and the compiler does not link this crate. A rename on
// either side fails in `tests/contract_mirror.rs` rather than becoming an empty
// capability set at instantiation time.

/// One capability a component needs, as the compiler derived it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Capability {
    pub family: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub argument: Option<String>,
}

impl Capability {
    /// The canonical text form. Must match `pw_core::contract::Capability::name`
    /// exactly — a second spelling is a second capability.
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

/// Where an import's implementation comes from, and therefore what constrains
/// it.
///
/// Architect ruling, 2026-08-07:
///
/// > A component import isn't automatically "authority". It is a dependency on
/// > another component whose own authority is independently constrained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    #[default]
    HostCapability,
    Component,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Import {
    pub interface: String,
    pub name: String,
    #[serde(default)]
    pub capability: String,
    #[serde(default)]
    pub kind: ImportKind,
    /// Where its answer must hold an invariant (ADR-0179). Mirrored by field
    /// name, ADR-0018.
    #[serde(default)]
    pub bounded: Vec<Bounded>,
    /// **The event this import writes to the outbox** (ADR-0208), by its
    /// declaration's path. Mirrored by field name, ADR-0018.
    #[serde(default)]
    pub event: Option<String>,
    /// **The query whose entry this import drops** (ADR-0209), by its path.
    #[serde(default)]
    pub invalidates: Option<String>,
    /// **The positions it leaves to every value** (ADR-0256), by parameter:
    /// it is given the value at each other position. Mirrored by field
    /// name, ADR-0018.
    #[serde(default)]
    pub every: Vec<usize>,
    /// **The resource whose value it answers** (ADR-0277), by its path: a
    /// materialization's read. Mirrored by field name, ADR-0018.
    #[serde(default)]
    pub reads: Option<String>,
    /// **The run it schedules, or cancels** (the scheduled-commands track):
    /// the command's component, and how. Mirrored by field name, ADR-0018.
    #[serde(default)]
    pub schedules: Option<Schedules>,
}

/// **What a schedules' import does** (the scheduled-commands track): for the
/// component `command`, `at` an instant or `after` a duration, each given
/// after the command's arguments, or `cancel` the run pending for them.
/// Mirrored by field name, ADR-0018.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Schedules {
    pub command: String,
    pub how: String,
}

/// **One place a value from outside must hold an invariant** (ADR-0179), as
/// the compiler states it: the value at `argument`, each step into it, and
/// the bounds. Mirrored by field name, ADR-0018. A step is a record's field,
/// `*` for each item of a list, `some`, `ok` or `err`, a case by its name, or
/// a field of a case's payload by its position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Bounded {
    pub argument: usize,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(rename = "type")]
    pub ty: String,
    pub holds: String,
    /// What the bounds are on: the value, or a `String`'s length in code
    /// points (ADR-0225).
    #[serde(default)]
    pub measure: Measure,
    #[serde(default)]
    pub at_least: Option<i64>,
    #[serde(default)]
    pub at_most: Option<i64>,
}

/// **What a bound is on** (ADR-0225): a value, or a `String`'s length, in
/// code points, as the language's `String.length` counts them (ADR-0040).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    #[default]
    Value,
    Length,
}

/// What class an ACTUAL Wasm import falls into.
///
/// The audit classifies rather than flattening, because each class is
/// constrained by a different thing: a host capability by the contract's
/// `required_capabilities` and the node's grants, a component interface by that
/// component's own contract, and a runtime import by nothing at all — which is
/// why the third class exists and is always refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportClass {
    Host,
    Component,
    /// `wasi:*` and anything else the toolchain injected. Never allowed: see
    /// `docs/evidence/E0/spike-wasmtime-component.txt` F-2.
    Runtime,
}

/// Which class an import falls into, given the contract that should describe it.
///
/// MEMBERSHIP decides, not spelling. The compiler emits `pw:host/…` and
/// `pw:app/…`, but a component built against a hand-written WIT world uses
/// whatever names that world declares — and refusing those on a prefix would
/// make the audit a check on naming rather than on authority.
///
/// The one exception is [`is_runtime`], which no contract can override.
pub fn classify(contract: &ComponentContract, import: &str) -> ImportClass {
    if is_runtime(import) {
        return ImportClass::Runtime;
    }
    for i in &contract.imports {
        if i.key() == import {
            return match i.kind {
                ImportKind::HostCapability => ImportClass::Host,
                ImportKind::Component => ImportClass::Component,
            };
        }
    }
    ImportClass::Runtime
}

/// **What one instance may consume.**
///
/// E8 gate item: *"fuel and memory limits per instance, driven by policy rather
/// than a constant."* E0's `check:fuel` proved wasmtime enforces both; what a
/// spike cannot say is where the number comes from. This is that, and it lives
/// beside `Topology` — part of what a DEPLOYMENT declares, because a budget
/// compiled into the host is a budget nobody can raise for a component that
/// legitimately needs more, and nobody can lower for one that does not.
///
/// `None` is unbounded, and it is a deliberate value rather than a default:
/// `Limits::unbounded()` has to be written, so a caller that wants no ceiling
/// says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Execution budget. Exhausting it traps, which is what makes a runaway
    /// guest a bounded cost rather than an outage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
    /// Ceiling on linear memory, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<usize>,
    /// Ceiling on table elements — the other growable resource, and the one
    /// that gets forgotten. A guest denied memory can still exhaust a host
    /// through indirect-call tables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_elements: Option<usize>,
}

impl Limits {
    /// No ceiling on anything. Written out, never defaulted into.
    pub fn unbounded() -> Limits {
        Limits::default()
    }

    pub fn is_bounded(&self) -> bool {
        self.fuel.is_some() || self.memory_bytes.is_some() || self.table_elements.is_some()
    }
}

/// The store data a limited instance runs with.
///
/// Public because `Store<T>`'s data type is part of the API surface once a
/// caller holds one; the fields are wasmtime's own limiter, unchanged.
#[derive(Debug)]
pub struct Meter {
    pub limits: MemoryLimits,
}

/// Wasmtime's `ResourceLimiter`, driven by [`Limits`].
///
/// The fields are public and readable without the `engine` feature, because a
/// host that has decided a ceiling should be able to report it — the alternative
/// is a limiter whose contents exist only inside the engine, which is the shape
/// that makes "what was this instance allowed?" unanswerable after the fact.
#[derive(Debug, Default)]
pub struct MemoryLimits {
    pub memory_bytes: Option<usize>,
    pub table_elements: Option<usize>,
    /// The largest linear memory any instance in the store was granted, in
    /// bytes: its initial size, or a growth the limiter allowed. What E10
    /// task 4's evaluation measures (ADR-0046).
    pub peak_memory: usize,
}

impl From<&Limits> for Meter {
    fn from(l: &Limits) -> Meter {
        Meter {
            limits: MemoryLimits {
                memory_bytes: l.memory_bytes,
                table_elements: l.table_elements,
                peak_memory: 0,
            },
        }
    }
}

#[cfg(feature = "engine")]
impl wasmtime::ResourceLimiter for MemoryLimits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Refusing growth rather than erroring: the guest sees an allocation
        // failure it can handle, which is a different and better thing than
        // the host aborting. Charter §7.10's direction — a boundary reports
        // rather than crashes.
        let allowed = self.memory_bytes.is_none_or(|max| desired <= max);
        if allowed {
            self.peak_memory = self.peak_memory.max(desired);
        }
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(self.table_elements.is_none_or(|max| desired <= max))
    }
}

/// Is this an interface the toolchain injected rather than the program asked
/// for?
///
/// `docs/evidence/E0/spike-wasmtime-component.txt` F-2: Rust `std` on
/// `wasm32-wasip2` adds fourteen `wasi:*` interfaces during runtime
/// initialization. No effect row asks for them and no node grants them, so a
/// contract cannot authorise one by listing it — this check runs BEFORE
/// membership for exactly that reason.
pub fn is_runtime(import: &str) -> bool {
    import.starts_with("wasi:")
}

impl Import {
    pub fn key(&self) -> String {
        format!("{}#{}", self.interface, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Export {
    pub name: String,
    pub kind: String,
    /// How this edge may be bound. Mirrored by field name, ADR-0018.
    ///
    /// `#[serde(default)]` so a contract written before the compiler emitted
    /// this parses as what the host assumed when there was no field: bindable
    /// either way.
    #[serde(default)]
    pub binding: BindingSupport,
    /// Where the export is in the compiled component, as the compiler named
    /// it. Mirrored by field name, ADR-0018; absent in a contract written
    /// before E10-I.
    #[serde(default)]
    pub component: Option<ComponentExport>,
}

/// One authorization precondition the deployment must approve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorizationRequirement {
    pub predicate: String,
    #[serde(default)]
    pub arguments: Vec<usize>,
}

/// An export's place in a component: its interface, function, and invocation
/// preconditions. Mirrored by field name from the compiler, ADR-0018.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentExport {
    pub interface: String,
    pub function: String,
    #[serde(default)]
    pub authorization: Vec<AuthorizationRequirement>,
    /// The key type a retried invocation is recognised by (ADR-0121). An
    /// idempotent export's invocation without a key is refused by the host
    /// that runs it.
    #[serde(default)]
    pub idempotent_by: Option<String>,
    /// Who invokes it where no reader does (NEXT 6c): `Payments`. A host
    /// answers a reader's invocation of such an export as one that is not
    /// there. Mirrored by field name, ADR-0018.
    #[serde(default)]
    pub invoked_by: Option<String>,
    /// Where an argument must hold an invariant (ADR-0179). Mirrored by field
    /// name, ADR-0018.
    #[serde(default)]
    pub bounded: Vec<Bounded>,
}

/// **What binding modes an interface edge supports**, as the compiler derived
/// it from the signature's types.
///
/// Two independent answers rather than one enum, on the architect's ruling of
/// 2026-08-07: `Either` is just both, and an enum forecloses "remote ✓ only
/// through a host-mediated handle proxy".
///
/// A host may narrow this and must not widen it. Whether an edge SHOULD be
/// remote is the planner's, from placement; whether it CAN be is this, from the
/// types, and the compiler is the only thing that can see them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BindingSupport {
    pub local: LocalSupport,
    pub remote: RemoteSupport,
}

impl Default for BindingSupport {
    fn default() -> Self {
        BindingSupport {
            local: LocalSupport::Direct,
            remote: RemoteSupport::Transferable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalSupport {
    /// A direct call in one address space. Nothing about a type can forbid it;
    /// whether the two ends may SHARE a node is placement's question, which
    /// `plan` composes with this.
    Direct,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RemoteSupport {
    /// Every value in the signature can cross, with nothing owed.
    Transferable,
    /// Something in the signature cannot cross at all, and each one is named.
    Refused { positions: Vec<Untransferable> },
    /// **It can cross, IF the binding discharges these obligations.**
    ///
    /// The privacy property belongs to the binding, not to a node: an origin
    /// handles millions of sessions, so "this machine is Session A" is not a
    /// thing that can be true. A plan may keep such an edge as a candidate and
    /// must not call itself complete until something discharges what it owes.
    Conditional { obligations: Vec<Obligation> },
    /// The compiler had no basis to decide — a position whose type that build
    /// could not determine.
    ///
    /// Distinct from `Conditional`: there is nothing for a binding to prove,
    /// because nobody knows what crosses.
    Undetermined { positions: Vec<Untransferable> },
}

/// What a binding must prove before an edge may carry a signature.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Obligation {
    /// The invocation must reach the same principal it left.
    PreservePrincipal {
        principal: String,
        position: String,
        ty: Option<String>,
    },
}

impl RemoteSupport {
    pub fn is_transferable(&self) -> bool {
        matches!(self, RemoteSupport::Transferable)
    }

    /// Could this edge be remote at all, given something to discharge what it
    /// owes? `Conditional` is a yes with a condition, not a no.
    pub fn is_possible(&self) -> bool {
        !matches!(self, RemoteSupport::Refused { .. })
    }

    pub fn obligations(&self) -> &[Obligation] {
        match self {
            RemoteSupport::Conditional { obligations } => obligations,
            _ => &[],
        }
    }
}

/// One position in a signature that cannot cross, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Untransferable {
    /// `argument 0`, `result`, `error`.
    pub position: String,
    pub ty: Option<String>,
    pub reason: String,
}

/// The capability→representation mapping this host understands.
///
/// A contract produced under a different mapping is not comparable: the same
/// `database.read<Stores>` may mean something else. Refused rather than
/// interpreted — see [`Refusal::UnknownMapping`].
pub const CAPABILITY_MAPPING: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentContract {
    pub component_id: String,
    #[serde(default)]
    pub capability_mapping: u32,
    pub abi_schema: String,
    pub required_capabilities: Vec<Capability>,
    pub allowed_placements: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl ComponentContract {
    pub fn from_json(text: &str) -> Result<Vec<ComponentContract>, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

// --- the deployment, declared -----------------------------------------------

/// One place code can run, and what it actually has.
///
/// **Declarative, and the host's own.** The architect's E8 list calls for "a
/// declarative host topology replacing `worlds_for`". That table said which
/// worlds *could* grant a family — a statement about the shape of the web —
/// and it was deleted from the compiler on 2026-08-07 in favour of a
/// `placement` clause on each effect declaration. This is the other half: which
/// capabilities THIS deployment's node has, which is a statement about a
/// machine, and no compiler can know it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// The node's name, for diagnostics.
    pub name: String,
    /// Which world this node is: `build`, `browser`, `edge`, `origin`.
    pub world: String,
    /// The capabilities physically available here, in canonical form.
    ///
    /// `database.read<Stores>`, not `database`. A node with a read-only replica
    /// grants the first and not `database.write<Stores>`, and a topology that
    /// could only speak in families could not express that.
    pub grants: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Topology {
    pub nodes: Vec<Node>,
}

impl Topology {
    pub fn from_json(text: &str) -> Result<Topology, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

// --- the decision -----------------------------------------------------------

/// Why a component may not be instantiated here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The node's world is not one the contract allows.
    WrongWorld { node: String, world: String },
    /// The node does not physically have a capability the contract requires.
    Ungranted { node: String, capability: String },
    /// The artifact imports something the contract does not allow.
    ///
    /// Distinct from `Ungranted` on purpose: one says the deployment cannot
    /// supply what the program asked for, the other says the ARTIFACT asked for
    /// something the program never did. The second is far more serious — it
    /// means the built thing and the checked thing are not the same thing.
    UndeclaredImport { imports: Vec<String> },
    /// The contract can run nowhere at all.
    Unplaceable,
    /// The contract was produced under a capability mapping this host does not
    /// know, so its capabilities cannot be compared with this node's grants.
    ///
    /// Refused rather than interpreted. Two mappings can spell one capability
    /// the same way and mean different authority, and a host that guessed would
    /// be guessing about exactly the thing it exists to decide.
    UnknownMapping { found: u32, understood: u32 },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::WrongWorld { node, world } => {
                write!(
                    f,
                    "node `{node}` is a {world} node, which this component does not allow"
                )
            }
            Refusal::Ungranted { node, capability } => {
                write!(f, "node `{node}` does not grant `{capability}`")
            }
            Refusal::UndeclaredImport { imports } => write!(
                f,
                "the artifact imports {} which its contract does not allow: {}",
                imports.len(),
                imports.join(", ")
            ),
            Refusal::Unplaceable => {
                f.write_str("this component's demands can be satisfied by no world")
            }
            Refusal::UnknownMapping { found, understood } => write!(
                f,
                "the contract uses capability mapping {found}; this host understands {understood}"
            ),
        }
    }
}

/// What the host concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Instantiate it, granting exactly these capabilities and no others.
    Admit { grants: Vec<String> },
    /// Do not. Every reason, not the first — a deployment that fixes one and
    /// rediscovers the next is a slow way to learn the shape of a problem.
    Refuse(Vec<Refusal>),
}

impl Admission {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admit { .. })
    }

    pub fn refusals(&self) -> &[Refusal] {
        match self {
            Admission::Admit { .. } => &[],
            Admission::Refuse(r) => r,
        }
    }
}

/// **The whole decision.**
///
/// `actual` is what the built artifact really imports — read from the component
/// itself, not from anything the compiler said. Pass an empty slice only when
/// there is no artifact yet; an empty import list from a component that has
/// imports would silently pass the audit, so the caller reading them must fail
/// loudly rather than default to none.
pub fn admit(
    contract: &ComponentContract,
    topology: &Topology,
    node_name: &str,
    actual: &[String],
) -> Admission {
    let mut refusals = Vec::new();

    if contract.capability_mapping != CAPABILITY_MAPPING {
        refusals.push(Refusal::UnknownMapping {
            found: contract.capability_mapping,
            understood: CAPABILITY_MAPPING,
        });
    }

    if contract.allowed_placements.is_empty() {
        refusals.push(Refusal::Unplaceable);
    }

    let node = topology.node(node_name);
    match node {
        None => refusals.push(Refusal::WrongWorld {
            node: node_name.to_string(),
            world: "unknown".to_string(),
        }),
        Some(n) => {
            // 1. Placement.
            if !contract.allowed_placements.contains(&n.world) {
                refusals.push(Refusal::WrongWorld {
                    node: n.name.clone(),
                    world: n.world.clone(),
                });
            }
            // 2. Capability. A separate check on separate data — see the module
            //    header for why collapsing the two hides a real mistake.
            for cap in &contract.required_capabilities {
                if !n.grants.contains(&cap.name()) {
                    refusals.push(Refusal::Ungranted {
                        node: n.name.clone(),
                        capability: cap.name(),
                    });
                }
            }
        }
    }

    // 3. The artifact audit. ADR-0020's rule, applied to the built thing —
    //    and CLASSIFIED, so each class is compared against its own source.
    //
    //    A flattened set would let a component interface satisfy a host
    //    capability slot and vice versa, which is the one confusion the split
    //    exists to prevent.
    let host_allowed: BTreeSet<String> = contract
        .imports
        .iter()
        .filter(|i| i.kind == ImportKind::HostCapability)
        .map(Import::key)
        .collect();
    let component_allowed: BTreeSet<String> = contract
        .imports
        .iter()
        .filter(|i| i.kind == ImportKind::Component)
        .map(Import::key)
        .collect();

    let mut undeclared: Vec<String> = actual
        .iter()
        .filter(|i| {
            // A runtime import is never allowed, whatever a contract says.
            if is_runtime(i) {
                return true;
            }
            // Otherwise: allowed if the contract lists it, IN EITHER CLASS —
            // and the classes are separate sets, so a component interface
            // cannot fill a host capability slot or the reverse.
            !host_allowed.contains(*i) && !component_allowed.contains(*i)
        })
        .cloned()
        .collect();
    undeclared.sort();
    undeclared.dedup();
    if !undeclared.is_empty() {
        refusals.push(Refusal::UndeclaredImport {
            imports: undeclared,
        });
    }

    if refusals.is_empty() {
        // EXACTLY what the contract requires, and nothing the node happens to
        // have. A host that granted its whole capability set to every component
        // it admitted would make the contract advisory.
        Admission::Admit {
            grants: contract
                .required_capabilities
                .iter()
                .map(Capability::name)
                .collect(),
        }
    } else {
        Admission::Refuse(refusals)
    }
}

// --- capabilities, as handles ------------------------------------------------

/// An opaque reference to something the host holds.
///
/// The guest receives this, never the value behind it. A connection string, a
/// signing key or a session token placed in guest memory is authority the host
/// can no longer withdraw, cannot account for, and cannot keep out of a crash
/// dump — and E7's resume manifests already established that a capability must
/// never be a value the client can carry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle {
    capability: String,
    slot: u32,
}

impl Handle {
    pub fn capability(&self) -> &str {
        &self.capability
    }

    pub fn slot(&self) -> u32 {
        self.slot
    }
}

/// `Debug` never prints what a handle refers to, because a handle does not know.
impl std::fmt::Display for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "handle({}, slot {})", self.capability, self.slot)
    }
}

/// What one admitted instance may reach.
///
/// Built from an [`Admission`], so an instance cannot hold a capability the
/// decision did not grant: there is no constructor that takes a capability list
/// directly.
#[derive(Debug, Default)]
pub struct Granted {
    handles: BTreeMap<String, Handle>,
    /// The values, held HERE. Never handed out.
    values: BTreeMap<String, String>,
}

impl Granted {
    /// Build the instance's capability set from an admission and the node's
    /// backing values.
    ///
    /// Returns `None` for a refusal. There is deliberately no way to build a
    /// `Granted` from a refused admission — an "override" parameter is how a
    /// capability system becomes a logging system.
    pub fn from(admission: &Admission, backing: &BTreeMap<String, String>) -> Option<Granted> {
        let Admission::Admit { grants } = admission else {
            return None;
        };
        let mut out = Granted::default();
        for (slot, capability) in grants.iter().enumerate() {
            out.handles.insert(
                capability.clone(),
                Handle {
                    capability: capability.clone(),
                    slot: slot as u32,
                },
            );
            if let Some(v) = backing.get(capability) {
                out.values.insert(capability.clone(), v.clone());
            }
        }
        Some(out)
    }

    /// The handle for a capability, if this instance was granted it.
    pub fn handle(&self, capability: &str) -> Option<&Handle> {
        self.handles.get(capability)
    }

    /// Use a capability, by handle.
    ///
    /// The host performs the work and returns the result; the value behind the
    /// handle stays here. `None` when the handle is not one this instance
    /// holds — including a handle forged from another instance's, because
    /// membership is checked rather than the handle being trusted to describe
    /// itself.
    pub fn use_handle(&self, handle: &Handle) -> Option<&str> {
        let held = self.handles.get(&handle.capability)?;
        if held != handle {
            return None;
        }
        self.values.get(&handle.capability).map(String::as_str)
    }

    pub fn count(&self) -> usize {
        self.handles.len()
    }
}

/// Exactly what a host may install in a component's linker.
///
/// The bridge between a decision and an instantiation. `docs/evidence/E0//// spike-wasmtime-component.txt` already proved the engine half — a component
/// whose import is withheld from the linker fails to instantiate, with a
/// diagnostic naming the missing interface — so what remained was that the
/// linker's CONTENTS come from the admission rather than from the node.
///
/// Derived from the contract's imports filtered by what was granted, so a host
/// cannot install an interface for a capability the decision refused, and
/// cannot install one the node happens to have.
///
/// **A read is linked with no grant** (ADR-0277): a materialization's read of
/// a resource's value is a dependency, not authority, as a page's reading of
/// a query is. The resource holds its own authority, which its component is
/// admitted for when the host runs it to answer.
pub fn linkable(contract: &ComponentContract, granted: &Granted) -> Vec<String> {
    let mut out: Vec<String> = contract
        .imports
        .iter()
        .filter(|i| {
            granted.handle(&i.capability).is_some()
                || (i.reads.is_some() && i.capability.is_empty())
        })
        .map(Import::key)
        .collect();
    out.sort();
    out.dedup();
    out
}

// --- reading a real artifact -------------------------------------------------

#[cfg(feature = "engine")]
pub mod engine {
    //! What a built component actually imports.
    //!
    //! Read from the artifact with `wasmtime`, because the point of the audit
    //! is to catch what the source does not say — `docs/evidence/E0/spike-\
    //! wasmtime-component.txt` measured a guest whose WIT world declares one
    //! import and whose component demands fifteen.

    use std::collections::{BTreeMap, BTreeSet};

    use wasmtime::component::types::ComponentItem;

    /// A component-level value, as the engine passes one across the boundary.
    /// Re-exported so a host implementing operations names the same type the
    /// engine does, without depending on a second copy of the engine.
    pub use wasmtime::component::Val;

    /// Strict read-only table selector conversion, before manifest validation.
    pub mod table_selectors;

    /// Flat read-only table row values; no linker bindings are installed here.
    pub mod table_results;

    /// Compose an admitted table read's adapter closure into one host callback.
    pub mod table_read_host;

    /// **The configuration of every engine the host makes** (ADR-0244): the
    /// component model, and none of the proposals Pleris's components do not
    /// use, so a component that uses one is refused when it loads.
    ///
    /// Wasmtime 48 enables GC, exceptions and the component model's async by
    /// default, and the host made its engines with `Config::new()`: each was
    /// on for nothing. Three advisories against 48.0.3 were in them
    /// (RUSTSEC-2026-0325 to 0327). Each comes back when the compiler emits
    /// code that uses it, checked against that release's advisories.
    pub fn engine_config() -> wasmtime::Config {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        // Revisit when ADR-0008's move to WASI 0.3 (`wasm32-wasip3`) comes,
        // for concurrent or streamed host calls inside a query.
        config.wasm_component_model_async(false);
        // Revisit if the browser's Wasm (E10-T2) or the boxed recursive
        // types (ADR-0202) would be smaller or faster on GC references than
        // on linear memory and regions. The Canonical ABI carries no GC type
        // across a component's boundary yet.
        config.wasm_gc(false);
        // Revisit only if a lowering needs a non-local exit that `Result`
        // and a trap cannot express. Resumable effect handlers would need
        // stack switching, not exceptions.
        config.wasm_exceptions(false);
        // A trap's cause is read from the frame it stopped in (ADR-0267): a
        // backtrace captured, the youngest frame first, and no function
        // inlined into its caller, since a backtrace may omit an inlined
        // frame. Both are Wasmtime 48's defaults, stated so that a release
        // changing them does not drop the causes.
        config.wasm_backtrace_max_frames(std::num::NonZeroUsize::new(20));
        config.compiler_inlining(wasmtime::Inlining::No);
        config
    }

    /// Every instance a component imports, as `interface#name` where the name
    /// is known and `interface` alone otherwise.
    ///
    /// Errors rather than returning an empty list. An empty list means "imports
    /// nothing", which passes every audit — so a failure to read must never be
    /// able to produce one.
    ///
    /// Read from the component [`Prepared::compile`] compiled, once a process
    /// for its bytes (ADR-0317): until 2026-10-12 a server loading a build
    /// compiled each component twice, once to read its imports and once to
    /// run it, and every test's server did so again.
    pub fn imports_of(bytes: &[u8]) -> Result<Vec<String>, String> {
        Ok(Prepared::compile(bytes)?.imports())
    }

    /// [`imports_of`], for a component `engine` compiled.
    fn imports_in(
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
    ) -> Vec<String> {
        let ty = component.component_type();

        let mut out = Vec::new();
        for (name, item) in ty.imports(engine) {
            match item.ty {
                ComponentItem::ComponentInstance(instance) => {
                    // An instance's OPERATIONS are what it asks for: its
                    // functions and resources. A type it names is not
                    // authority — `pw:types/types` holds only the shapes the
                    // operations mention, and reporting it as an import made
                    // every compiled component ask for something no contract
                    // grants (E10-I, 2026-09-24). A `wasi:*` instance is
                    // always reported, whatever it holds.
                    let mut any = false;
                    for (func, export) in instance.exports(engine) {
                        if matches!(
                            export.ty,
                            ComponentItem::ComponentFunc(_) | ComponentItem::Resource(_)
                        ) {
                            out.push(format!("{name}#{func}"));
                            any = true;
                        }
                    }
                    if !any && name.starts_with("wasi:") {
                        out.push(name.to_string());
                    }
                }
                ComponentItem::Type(_) => {}
                _ => out.push(name.to_string()),
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// **The exact export being invoked, as its contract declares it.**
    ///
    /// A contract may expose more than one operation later, so matching the
    /// component's interface and function is part of selecting its policy
    /// and its arguments' invariants.
    ///
    /// A one-segment path is a function the component's world exports
    /// directly, declared with an empty interface. `run` reaches such an
    /// export, and a hand-written guest has one; refusing the path shape made
    /// it unreachable rather than undeclared (2026-10-02).
    fn declared_export<'a>(
        contract: &'a crate::ComponentContract,
        export: &[&str],
    ) -> Result<&'a crate::ComponentExport, String> {
        let (interface, function) = match export {
            [function] => ("", *function),
            [interface, function] => (*interface, *function),
            _ => {
                return Err(format!(
                    "an export path is `function` or `interface`, `function`; got {export:?}"
                ));
            }
        };
        contract
            .exports
            .iter()
            .filter_map(|e| e.component.as_ref())
            .find(|e| e.interface == *interface && e.function == *function)
            .ok_or_else(|| {
                format!(
                    "`{}` is not an export declared by component contract `{}`",
                    export.join("#"),
                    contract.component_id
                )
            })
    }

    /// Authorization requirements for the exact export being invoked.
    fn authorization_for<'a>(
        contract: &'a crate::ComponentContract,
        export: &[&str],
    ) -> Result<&'a [crate::AuthorizationRequirement], String> {
        declared_export(contract, export).map(|e| e.authorization.as_slice())
    }

    /// Evaluate every `requires` predicate before an invocation.
    ///
    /// Predicate names are deployment vocabulary. The host never guesses their
    /// meaning. Arguments are selected from the already-typed component
    /// arguments by the indices the compiler recorded. Unknown predicates are
    /// therefore naturally fail-closed when the deployment evaluator returns an
    /// error.
    pub fn authorize_export<F>(
        contract: &crate::ComponentContract,
        export: &[&str],
        args: &[Val],
        mut evaluate: F,
    ) -> Result<(), String>
    where
        F: FnMut(&str, &[&Val]) -> Result<bool, String>,
    {
        for requirement in authorization_for(contract, export)? {
            let mut selected = Vec::with_capacity(requirement.arguments.len());
            for index in &requirement.arguments {
                let Some(value) = args.get(*index) else {
                    return Err(format!(
                        "authorization predicate `{}` refers to missing argument {}",
                        requirement.predicate, index
                    ));
                };
                selected.push(value);
            }
            match evaluate(&requirement.predicate, &selected) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(format!(
                        "authorization predicate `{}` denied the invocation",
                        requirement.predicate
                    ));
                }
                Err(why) => {
                    return Err(format!(
                        "authorization predicate `{}` could not be evaluated: {why}",
                        requirement.predicate
                    ));
                }
            }
        }
        Ok(())
    }

    fn refuse_unevaluated_authorization(
        contract: &crate::ComponentContract,
        export: &[&str],
    ) -> Result<(), String> {
        let requirements = authorization_for(contract, export)?;
        if requirements.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "`{}` has {} authorization precondition(s) that have not been evaluated; use `call_authorized_within`",
                export.join("#"),
                requirements.len()
            ))
        }
    }

    /// **Instantiate a component with exactly the capabilities it was granted.**
    ///
    /// E8 gate item: *"typed linking only from a `Granted`. A component whose
    /// contract omits an import fails to instantiate, with the engine's own
    /// diagnostic rather than ours."*
    ///
    /// The linker is populated from [`crate::linkable`] and from nothing else.
    /// That is the whole design:
    ///
    /// ```text
    /// admit()    decides whether this component may run here
    /// Granted    what it therefore holds
    /// linkable() which of its imports that satisfies
    /// this       the linker, and no other source of definitions
    /// ```
    ///
    /// **The engine refuses, not us.** A pre-flight check comparing lists would
    /// be a second implementation of instantiation's own rule, and the two
    /// would agree until a component imported something in a way the list did
    /// not model. An unlinked import is an unresolvable one, and wasmtime says
    /// so in its own words — which is also the better diagnostic, because it
    /// names what the artifact asked for rather than what we expected.
    ///
    /// Returns the linked import keys on success, so a caller can record what
    /// was actually supplied.
    ///
    /// Runs under [`crate::Limits::unbounded`]. Use [`instantiate_within`] to
    /// give an instance a budget.
    pub fn instantiate(
        bytes: &[u8],
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
    ) -> Result<Vec<String>, String> {
        instantiate_within(bytes, contract, granted, &crate::Limits::unbounded()).map(|o| o.linked)
    }

    /// What an instantiation produced, and what it cost.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Instantiated {
        /// The import keys that were linked, from the granted set and nothing
        /// else.
        pub linked: Vec<String>,
        /// Fuel consumed, where a budget was set.
        pub fuel_used: Option<u64>,
    }

    /// **One host operation**, as the deployment implements it: the values a
    /// component passed, in; the values it receives, out. A refusal is an
    /// error string the call returns to the host, never a value invented for
    /// the component.
    pub type HostFn = std::sync::Arc<
        dyn Fn(&[wasmtime::component::Val]) -> Result<Vec<wasmtime::component::Val>, String>
            + Send
            + Sync,
    >;

    /// **Instantiate and CALL, with the host answering from behind a handle.**
    ///
    /// The positive control the architect asked for on 2026-08-08:
    ///
    /// > If you don't yet have a positive guest that actually exercises one
    /// > granted host interface, I would add that as the last E8 control.
    ///
    /// Everything else about E8 shows authority being *refused* or *linked*.
    /// This shows it being **used**: the guest calls the host function its
    /// contract permitted, and the value it returns is one only the host could
    /// have supplied.
    ///
    /// `host` maps `interface#function` to the deployment's implementation of
    /// that operation. It was a map of canned answers until E10-I needed a
    /// compiled command to call a real data layer; now each granted import is
    /// linked to exactly one host function, and a granted import with none is
    /// a refusal rather than a stub.
    ///
    /// `export` is the exported function's path: its interface, then the
    /// function, as the component names them — `["pw:app/..-api@0.1.0",
    /// "add-to-cart"]` — or a single top-level name.
    ///
    /// Returns what the exported function produced. Its post-return runs
    /// inside the call, so the component's invocation region is reclaimed
    /// before this returns.
    #[allow(clippy::too_many_arguments)]
    pub fn call_within(
        bytes: &[u8],
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
        limits: &crate::Limits,
        host: &std::collections::BTreeMap<String, HostFn>,
        export: &[&str],
        args: &[wasmtime::component::Val],
    ) -> Result<Vec<wasmtime::component::Val>, String> {
        Prepared::compile(bytes)?.call_within(contract, granted, limits, host, export, args)
    }

    /// Invoke an export after the deployment explicitly evaluates its
    /// authorization preconditions.
    #[allow(clippy::too_many_arguments)]
    pub fn call_authorized_within<F>(
        bytes: &[u8],
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
        limits: &crate::Limits,
        host: &std::collections::BTreeMap<String, HostFn>,
        export: &[&str],
        args: &[wasmtime::component::Val],
        evaluate: F,
    ) -> Result<Vec<wasmtime::component::Val>, String>
    where
        F: FnMut(&str, &[&Val]) -> Result<bool, String>,
    {
        Prepared::compile(bytes)?
            .call_authorized_within(contract, granted, limits, host, export, args, evaluate)
    }

    /// **A component compiled once, run many times.**
    ///
    /// Compiling is the expensive half of a call: Cranelift translates the
    /// whole component. A server that compiled per request paid it on every
    /// press of Add — and under the browser suite's parallel load that CPU was
    /// visible as Firefox's timing-sensitive tests failing, which is how it
    /// was found (E10-I, 2026-09-24). Authority is NOT cached: every call
    /// builds its linker from that call's `Granted`, so a grant revoked
    /// between two calls is revoked for the second.
    ///
    /// **Compiled once a process for each component's bytes** (ADR-0317):
    /// a compiled component is immutable, and an engine and its component
    /// are each shared by reference, so every server a process loads with
    /// those bytes runs the one compiled code. Until 2026-10-12 each server
    /// compiled every component of its build, and a test process's servers
    /// each compiled kiokun's again: seven of a kiokun test's eight seconds
    /// were Wasmtime compiling (W6's finding, 2026-10-09), and the
    /// development server's suite, four at once, held more than CI's memory
    /// bound (ADR-0292).
    #[derive(Clone)]
    pub struct Prepared {
        engine: wasmtime::Engine,
        component: wasmtime::component::Component,
    }

    /// Each component compiled in this process, by its bytes. A cell each,
    /// so a second caller with the same bytes waits for the first's compile
    /// rather than compiling them again.
    type Compiled = std::sync::Arc<std::sync::OnceLock<Result<Prepared, String>>>;
    static COMPILED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<Vec<u8>, Compiled>>,
    > = std::sync::OnceLock::new();

    /// How many components this process has compiled, which a test reads.
    static COMPILES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    impl Prepared {
        /// Compile a component, or take the one this process compiled from
        /// the same bytes. The engine meters fuel, so a call can be given a
        /// budget; a call with none gets the whole range.
        pub fn compile(bytes: &[u8]) -> Result<Prepared, String> {
            let cell = {
                let mut compiled = COMPILED
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|_| "the compiled components' table is poisoned".to_string())?;
                match compiled.get(bytes) {
                    Some(cell) => cell.clone(),
                    None => compiled.entry(bytes.to_vec()).or_default().clone(),
                }
            };
            cell.get_or_init(|| Prepared::compiled_now(bytes)).clone()
        }

        fn compiled_now(bytes: &[u8]) -> Result<Prepared, String> {
            COMPILES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut config = engine_config();
            config.consume_fuel(true);
            let engine = wasmtime::Engine::new(&config).map_err(|e| e.to_string())?;
            let component =
                wasmtime::component::Component::new(&engine, bytes).map_err(|e| e.to_string())?;
            Ok(Prepared { engine, component })
        }

        /// How many components this process has compiled.
        pub fn compiles() -> usize {
            COMPILES.load(std::sync::atomic::Ordering::Relaxed)
        }

        /// Every instance the component imports ([`imports_of`]).
        pub fn imports(&self) -> Vec<String> {
            imports_in(&self.engine, &self.component)
        }

        /// **An export's arguments hold every invariant its contract states,
        /// at the component's boundary** (ADR-0179; ADR-0346, argument
        /// invariants).
        ///
        /// A value of an opaque type with a `where` invariant exists only if
        /// it passed the invariant, so a call carrying one that breaks it is
        /// refused before anything else: before a `requires` predicate reads
        /// it, before the component is instantiated, and so before its code
        /// or a data layer sees it. Every call API holds it, whoever built
        /// the values: a browser's JSON ([`Prepared::arguments_for`] holds
        /// them as it decodes, and the request is refused before anything is
        /// kept), a page's address, a stored run, a test. Held as the
        /// component is passed them, where the contract's paths are: a value
        /// of a type that contains itself as its nodes (ADR-0194), however it
        /// was given. Refused in a decoder's words: "argument refused: value
        /// 2 is 0, and `domain.PositiveInt` holds `value >= 1`".
        ///
        /// Only the compiler states what an argument holds, and its exports
        /// are an interface's: a world's export (a hand-written guest's)
        /// states nothing, and is called as before.
        pub fn hold_arguments(
            &self,
            contract: &crate::ComponentContract,
            export: &[&str],
            args: &[Val],
        ) -> Result<(), String> {
            let bounded = &declared_export(contract, export)?.bounded;
            if bounded.is_empty() {
                return Ok(());
            }
            let func = export_type(&self.engine, &self.component, export)?;
            let passed: Vec<Val> = args
                .iter()
                .cloned()
                .zip(func.params())
                .map(|(given, (_, ty))| graph::tangle(given, &ty))
                .collect::<Result<_, _>>()?;
            holds(bounded, &passed).map_err(|why| format!("argument refused: {why}"))
        }

        /// [`call_within`], on the compiled component.
        #[allow(clippy::too_many_arguments)]
        pub fn call_within(
            &self,
            contract: &crate::ComponentContract,
            granted: &crate::Granted,
            limits: &crate::Limits,
            host: &std::collections::BTreeMap<String, HostFn>,
            export: &[&str],
            args: &[wasmtime::component::Val],
        ) -> Result<Vec<wasmtime::component::Val>, String> {
            // Its arguments decoded first, before anything else.
            self.hold_arguments(contract, export, args)?;
            refuse_unevaluated_authorization(contract, export)?;
            run(
                &self.engine,
                &self.component,
                contract,
                granted,
                limits,
                host,
                export,
                args,
            )
        }

        /// `call_within`, with every invocation precondition evaluated first.
        #[allow(clippy::too_many_arguments)]
        pub fn call_authorized_within<F>(
            &self,
            contract: &crate::ComponentContract,
            granted: &crate::Granted,
            limits: &crate::Limits,
            host: &std::collections::BTreeMap<String, HostFn>,
            export: &[&str],
            args: &[wasmtime::component::Val],
            evaluate: F,
        ) -> Result<Vec<wasmtime::component::Val>, String>
        where
            F: FnMut(&str, &[&Val]) -> Result<bool, String>,
        {
            // Its arguments decoded before a predicate reads them.
            self.hold_arguments(contract, export, args)?;
            authorize_export(contract, export, args, evaluate)?;
            run(
                &self.engine,
                &self.component,
                contract,
                granted,
                limits,
                host,
                export,
                args,
            )
        }

        /// **An export's results, as the program holds them** (ADR-0194):
        /// each value of a type that contains itself made from its nodes, by
        /// the export's declared result types. A call returns the nodes, as
        /// the component passed them; a host that renders or compares the
        /// values reads them so.
        pub fn untangled(
            &self,
            export: &[&str],
            results: Vec<wasmtime::component::Val>,
        ) -> Result<Vec<wasmtime::component::Val>, String> {
            let func = export_type(&self.engine, &self.component, export)?;
            results
                .into_iter()
                .zip(func.results())
                .map(|(v, t)| graph::untangle(v, &t))
                .collect()
        }

        /// **A query's value as a browser's module reads it** (ADR-0233), by
        /// the export's first result type: a value of a type that contains
        /// itself as its nodes, anywhere in it (see
        /// [`graph::browser_json`]). `value` is the result's `Ok`, as a
        /// page's binding holds it, where the result is a `Result`.
        pub fn browser_value(
            &self,
            export: &[&str],
            value: wasmtime::component::Val,
            leaf: &dyn Fn(&wasmtime::component::Val) -> serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            let func = export_type(&self.engine, &self.component, export)?;
            let ty = func
                .results()
                .next()
                .ok_or_else(|| format!("{export:?} returns nothing"))?;
            let ty = match (&value, &ty) {
                (wasmtime::component::Val::Result(_), _) => ty,
                (_, wasmtime::component::types::Type::Result(r)) => r
                    .ok()
                    .ok_or_else(|| format!("{export:?}'s `Ok` holds nothing"))?,
                _ => ty,
            };
            graph::browser_json(value, &ty, leaf)
        }

        /// **Arguments that arrived as JSON, typed by the export's own
        /// parameters.**
        ///
        /// A compiled handler runs in the user's browser, so what reaches the
        /// server is a claim about arguments, not values this system produced.
        /// Each one is converted by the type the COMPONENT declares for its
        /// parameter, read from the artifact, and anything else is refused: the
        /// wrong count, the wrong kind, an integer outside its type's range, a
        /// number with a fraction where an integer is declared. Scalars and
        /// strings are accepted; every other parameter type is refused by name
        /// until a command needs it.
        ///
        /// This checks the ABI's types, not what an opaque type promises:
        /// [`Prepared::arguments_for`] holds them to the invariants the
        /// contract states as well (ADR-0179).
        pub fn arguments(
            &self,
            export: &[&str],
            json: &[serde_json::Value],
        ) -> Result<Vec<wasmtime::component::Val>, String> {
            let func = export_type(&self.engine, &self.component, export)?;
            let params: Vec<(&str, wasmtime::component::types::Type)> = func.params().collect();
            if params.len() != json.len() {
                return Err(format!(
                    "`{}` takes {} argument(s); {} were sent",
                    export.join("#"),
                    params.len(),
                    json.len()
                ));
            }
            params
                .iter()
                .zip(json)
                .enumerate()
                .map(|(i, ((name, ty), v))| {
                    from_json(ty, v, &format!("argument {} (`{name}`)", i + 1))
                })
                .collect()
        }

        /// **An export's arguments, as its contract states them** (ADR-0179):
        /// typed by its parameters, as [`Prepared::arguments`] types them,
        /// and each held to the invariants the contract names. A browser's
        /// quantity of 0, for a `PositiveInt`, is refused here, before the
        /// component runs, as a number with a fraction is.
        pub fn arguments_for(
            &self,
            export: &crate::ComponentExport,
            json: &[serde_json::Value],
        ) -> Result<Vec<wasmtime::component::Val>, String> {
            let args = self.arguments(&[&export.interface, &export.function], json)?;
            holds(&export.bounded, &args).map_err(|why| format!("argument refused: {why}"))?;
            Ok(args)
        }
    }

    /// The type of an exported function, found by its path in the component.
    fn export_type(
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
        export: &[&str],
    ) -> Result<wasmtime::component::types::ComponentFunc, String> {
        let [interface, function] = export else {
            return Err(format!(
                "an export path is `interface`, `function`; got {export:?}"
            ));
        };
        let ty = component.component_type();
        let instance = ty
            .exports(engine)
            .find(|(name, _)| name == interface)
            .and_then(|(_, item)| match item.ty {
                ComponentItem::ComponentInstance(instance) => Some(instance),
                _ => None,
            })
            .ok_or_else(|| format!("the component exports no instance `{interface}`"))?;
        instance
            .exports(engine)
            .find(|(name, _)| name == function)
            .and_then(|(_, item)| match item.ty {
                ComponentItem::ComponentFunc(func) => Some(func),
                _ => None,
            })
            .ok_or_else(|| format!("`{interface}` exports no function `{function}`"))
    }

    /// **A type that contains itself, as a host holds it** (ADR-0194).
    ///
    /// A component passes a value of such a type as its nodes: a
    /// `list<node>` in level order, node 0 the value, each `list<u32>` field
    /// of a node its children's indices. A host holds the value itself,
    /// nested, as the program's declaration shapes it: each of those fields a
    /// list of the node's children. These convert between the two by the
    /// type the component declares, through every type that holds one, and
    /// neither recurses on the value, so its depth is no stack's business.
    pub mod graph {
        use wasmtime::component::Val;
        use wasmtime::component::types::Type;

        /// **The deepest nested value [`untangle`] makes.** A nested value
        /// is cloned, compared, printed and dropped by recursion, which a
        /// deep enough one overflows; its nodes are not. 128 is serde_json's
        /// own default nesting limit, so a nested value a host holds is one it
        /// could have parsed from JSON. Measured in a debug build, where each
        /// level costs most: a clone or a comparison overflows half a 2 MiB
        /// thread stack near 330 levels, so 128 leaves more than twice the
        /// room (`pw-conformance`'s `recursive_types.rs`). A host that needs a
        /// deeper value reads its nodes, which have no depth to overflow.
        pub const NESTED_DEPTH: usize = 128;

        /// Where a node holds its children: each field (or case payload, or a
        /// payload tuple's element) that holds indices, and how many.
        #[derive(Debug, Clone, PartialEq)]
        enum Slot {
            /// A record's field, by name.
            Field(String, Kind),
            /// A case's payload, by the case's name.
            Payload(String, Kind),
            /// An element of a case's payload tuple.
            Element(String, usize, Kind),
        }

        /// How many children a slot holds: a `list<u32>` any number, a `u32`
        /// one, held in place, and an `option<u32>` one or none (ADR-0202).
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Kind {
            List,
            One,
            Maybe,
        }

        fn indices(ty: &Type) -> Option<Kind> {
            match ty {
                Type::List(l) if l.ty() == Type::U32 => Some(Kind::List),
                Type::Option(o) if o.ty() == Type::U32 => Some(Kind::Maybe),
                Type::U32 => Some(Kind::One),
                _ => None,
            }
        }

        /// **The node type, when `ty` is a type that contains itself, as
        /// its nodes**: `list<node>`, a record or a variant with a `list<u32>`
        /// of children. Its slots.
        fn node_of(ty: &Type) -> Option<(Type, Vec<Slot>)> {
            let Type::List(list) = ty else {
                return None;
            };
            let node = list.ty();
            let mut slots = Vec::new();
            match &node {
                Type::Record(r) => {
                    for f in r.fields() {
                        if let Some(k) = indices(&f.ty) {
                            slots.push(Slot::Field(f.name.to_string(), k));
                        }
                    }
                }
                Type::Variant(v) => {
                    for c in v.cases() {
                        match &c.ty {
                            Some(Type::Tuple(t)) => {
                                for (i, e) in t.types().enumerate() {
                                    if let Some(k) = indices(&e) {
                                        slots.push(Slot::Element(c.name.to_string(), i, k));
                                    }
                                }
                            }
                            Some(t) => {
                                if let Some(k) = indices(t) {
                                    slots.push(Slot::Payload(c.name.to_string(), k));
                                }
                            }
                            None => {}
                        }
                    }
                }
                _ => return None,
            }
            (!slots.is_empty()).then_some((node, slots))
        }

        /// The value at a slot of a node, where the node's case holds one.
        fn at<'v>(node: &'v mut Val, slot: &Slot) -> Option<Option<&'v mut Val>> {
            // Another case of a variant holds no children here.
            if let (Slot::Payload(case, _) | Slot::Element(case, _, _), Val::Variant(name, _)) =
                (slot, &*node)
                && name != case
            {
                return None;
            }
            Some(match (slot, node) {
                (Slot::Field(name, _), Val::Record(fields)) => {
                    fields.iter_mut().find(|(n, _)| n == name).map(|(_, v)| v)
                }
                (Slot::Payload(..), Val::Variant(_, Some(p))) => Some(&mut **p),
                (Slot::Element(_, i, _), Val::Variant(_, Some(p))) => match &mut **p {
                    Val::Tuple(items) => items.get_mut(*i),
                    _ => None,
                },
                _ => None,
            })
        }

        fn kind(slot: &Slot) -> Kind {
            match slot {
                Slot::Field(_, k) | Slot::Payload(_, k) | Slot::Element(_, _, k) => *k,
            }
        }

        /// A node's children, or their indices, in the order the encoding
        /// reads them, each slot's taken out of the node: a list's items, a
        /// box's one, an option's one or none.
        fn take_lists(node: &mut Val, slots: &[Slot]) -> Result<Vec<Vec<Val>>, String> {
            let mut out = Vec::new();
            for slot in slots {
                let Some(held) = at(node, slot) else {
                    continue;
                };
                let taken = match (kind(slot), held) {
                    (Kind::List, Some(Val::List(items))) => std::mem::take(items),
                    (Kind::One, Some(v)) => vec![std::mem::replace(v, Val::Bool(false))],
                    (Kind::Maybe, Some(Val::Option(o))) => {
                        o.take().map(|b| *b).into_iter().collect()
                    }
                    _ => return Err(format!("a node has no children at {slot:?}")),
                };
                out.push(taken);
            }
            Ok(out)
        }

        /// Put each slot's back, in the order [`take_lists`] took them.
        fn put_lists(node: &mut Val, slots: &[Slot], mut lists: Vec<Vec<Val>>) {
            lists.reverse();
            for slot in slots {
                let Some(Some(held)) = at(node, slot) else {
                    continue;
                };
                let mut values = lists.pop().unwrap_or_default();
                *held = match kind(slot) {
                    Kind::List => Val::List(values),
                    Kind::One => values.pop().unwrap_or(Val::Bool(false)),
                    Kind::Maybe => Val::Option(values.pop().map(Box::new)),
                };
            }
        }

        /// **A value of a type that contains itself, from a browser**
        /// (ADR-0205): `{ "$graph": [node, ...] }`, each node a record whose
        /// fields that hold the type are `{ "$node": k }`, a list of them, or
        /// a case holding one. Each is the index the component's node holds
        /// there; every other field is read by `field`, as any argument's.
        pub fn from_browser(
            ty: &Type,
            v: &serde_json::Value,
            at: &str,
            field: &dyn Fn(&Type, &serde_json::Value, &str) -> Result<Val, String>,
        ) -> Result<Val, String> {
            let Some((node, slots)) = node_of(ty) else {
                return Err(format!("{at}: not a type that contains itself"));
            };
            let Type::Record(record) = &node else {
                return Err(format!(
                    "{at}: a case of a type that contains itself is not accepted from a browser"
                ));
            };
            let nodes = v
                .get("$graph")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| format!("{at}: expected a `$graph`, got {v}"))?;
            let index = |r: &serde_json::Value, at: &str| -> Result<Val, String> {
                r.get("$node")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|k| u32::try_from(k).ok())
                    .map(Val::U32)
                    .ok_or_else(|| format!("{at}: expected a `$node`, got {r}"))
            };
            let mut out = Vec::with_capacity(nodes.len());
            for (k, n) in nodes.iter().enumerate() {
                let at = format!("{at}.$graph[{k}]");
                let o = n
                    .as_object()
                    .ok_or_else(|| format!("{at}: expected an object, got {n}"))?;
                let mut fields = Vec::new();
                for f in record.fields() {
                    let written = f.name.replace('-', "_");
                    let value = o
                        .get(&written)
                        .ok_or_else(|| format!("{at}: the node has no field `{written}`"))?;
                    let place = format!("{at}.{written}");
                    let slot = slots.iter().find_map(|s| match s {
                        Slot::Field(name, kind) if name == f.name => Some(*kind),
                        _ => None,
                    });
                    fields.push((
                        f.name.to_string(),
                        match slot {
                            Some(Kind::List) => Val::List(
                                value
                                    .as_array()
                                    .ok_or_else(|| format!("{place}: expected a list of nodes"))?
                                    .iter()
                                    .map(|r| index(r, &place))
                                    .collect::<Result<_, _>>()?,
                            ),
                            Some(Kind::One) => index(value, &place)?,
                            Some(Kind::Maybe) => {
                                match value.get("$case").and_then(serde_json::Value::as_str) {
                                    Some("some") => Val::Option(Some(Box::new(index(
                                        value.get("value").unwrap_or(&serde_json::Value::Null),
                                        &place,
                                    )?))),
                                    Some("none") => Val::Option(None),
                                    _ => return Err(format!("{place}: expected `some` or `none`")),
                                }
                            }
                            None => field(&f.ty, value, &place)?,
                        },
                    ));
                }
                out.push(Val::Record(fields));
            }
            Ok(Val::List(out))
        }

        /// Is `ty` a type that contains itself, as its nodes?
        pub fn is_nodes(ty: &Type) -> bool {
            node_of(ty).is_some()
        }

        /// **A value as the component passes it**: each value of a type
        /// that contains itself, nested, made its nodes. A value already
        /// given as its nodes is passed as it is: the component checks it.
        pub fn tangle(v: Val, ty: &Type) -> Result<Val, String> {
            if let Some((node, slots)) = node_of(ty) {
                return match v {
                    // Nodes already.
                    Val::List(items) => Ok(Val::List(items)),
                    root => encode(root, &node, &slots),
                };
            }
            walk(v, ty, &tangle)
        }

        /// **A value as the program holds it**: each value of a type that
        /// contains itself made from its nodes, which must be a tree in level
        /// order, as the component's own decoder requires, and no deeper
        /// than [`NESTED_DEPTH`].
        pub fn untangle(v: Val, ty: &Type) -> Result<Val, String> {
            if let Some((node, slots)) = node_of(ty) {
                let Val::List(nodes) = v else {
                    return Err("a value of a type that contains itself is not its nodes".into());
                };
                return decode(nodes, &node, &slots);
            }
            walk(v, ty, &untangle)
        }

        /// **A value as the browser's modules read it** (ADR-0233): each
        /// value of a type that contains itself as its nodes, a graph (see
        /// [`to_browser`]); a record an object by its fields' names, `-` as
        /// `_`; a list or a tuple an array; a case, an option or a result
        /// `{ "$case": name, "value": payload }`; and anything else as
        /// `leaf` writes it. A host's own writer of nested values writes the
        /// same, but knows no type: a value it nests is no graph.
        pub fn browser_json(
            v: Val,
            ty: &Type,
            leaf: &dyn Fn(&Val) -> serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            if node_of(ty).is_some() {
                return to_browser(v, ty, &|v, t| browser_json(v, t, leaf));
            }
            let case = |name: &str, payload: Option<serde_json::Value>| match payload {
                Some(p) => serde_json::json!({ "$case": name, "value": p }),
                None => serde_json::json!({ "$case": name }),
            };
            let inner = |v: Option<Box<Val>>, ty: Option<Type>| match (v, ty) {
                (Some(v), Some(ty)) => browser_json(*v, &ty, leaf).map(Some),
                (Some(v), None) => Ok(Some(leaf(&v))),
                (None, _) => Ok(None),
            };
            Ok(match (v, ty) {
                (Val::Record(fields), Type::Record(r)) => {
                    let types: Vec<(String, Type)> =
                        r.fields().map(|f| (f.name.to_string(), f.ty)).collect();
                    let mut o = serde_json::Map::new();
                    for (name, v) in fields {
                        let written = match types.iter().find(|(n, _)| *n == name) {
                            Some((_, t)) => browser_json(v, t, leaf)?,
                            None => leaf(&v),
                        };
                        o.insert(name.replace('-', "_"), written);
                    }
                    serde_json::Value::Object(o)
                }
                (Val::List(items), Type::List(l)) => {
                    let t = l.ty();
                    serde_json::Value::Array(
                        items
                            .into_iter()
                            .map(|v| browser_json(v, &t, leaf))
                            .collect::<Result<_, _>>()?,
                    )
                }
                (Val::Tuple(items), Type::Tuple(t)) => serde_json::Value::Array(
                    items
                        .into_iter()
                        .zip(t.types())
                        .map(|(v, t)| browser_json(v, &t, leaf))
                        .collect::<Result<_, _>>()?,
                ),
                (Val::Option(v), Type::Option(o)) => match v {
                    Some(v) => case("some", inner(Some(v), Some(o.ty()))?),
                    None => case("none", None),
                },
                (Val::Result(Ok(v)), Type::Result(r)) => case("ok", inner(v, r.ok())?),
                (Val::Result(Err(v)), Type::Result(r)) => case("err", inner(v, r.err())?),
                (Val::Variant(name, v), Type::Variant(t)) => {
                    let ty = t.cases().find(|c| c.name == name).and_then(|c| c.ty);
                    let payload = inner(v, ty)?;
                    case(&name, payload)
                }
                (Val::Enum(name), _) => case(&name, None),
                (v, _) => leaf(&v),
            })
        }

        /// **A value of a type that contains itself, for a browser**
        /// (ADR-0233): `{ "$graph": [node, ...] }` in the level order
        /// [`from_browser`] reads and a browser's module decodes, each
        /// node's children `{ "$node": k }`, a list of them, or a case
        /// holding one. Every other field of a node is written by `field`,
        /// with its type.
        pub fn to_browser(
            v: Val,
            ty: &Type,
            field: &dyn Fn(Val, &Type) -> Result<serde_json::Value, String>,
        ) -> Result<serde_json::Value, String> {
            let Some((node, slots)) = node_of(ty) else {
                return Err("not a type that contains itself".into());
            };
            let Val::List(nodes) = tangle(v, ty)? else {
                return Err("a value of a type that contains itself made no nodes".into());
            };
            let reference = |k: &Val| match k {
                Val::U32(k) => Ok(serde_json::json!({ "$node": k })),
                other => Err(format!("a node's child is no index: {other:?}")),
            };
            let children = |kind: Kind, v: Val| -> Result<serde_json::Value, String> {
                Ok(match (kind, v) {
                    (Kind::List, Val::List(ks)) => serde_json::Value::Array(
                        ks.iter().map(reference).collect::<Result<_, _>>()?,
                    ),
                    (Kind::One, k) => reference(&k)?,
                    (Kind::Maybe, Val::Option(Some(k))) => {
                        serde_json::json!({ "$case": "some", "value": reference(&k)? })
                    }
                    (Kind::Maybe, Val::Option(None)) => serde_json::json!({ "$case": "none" }),
                    (_, other) => return Err(format!("a node's children are {other:?}")),
                })
            };
            let mut out = Vec::with_capacity(nodes.len());
            for n in nodes {
                out.push(match (n, &node) {
                    (Val::Record(fields), Type::Record(r)) => {
                        let types: Vec<(String, Type)> =
                            r.fields().map(|f| (f.name.to_string(), f.ty)).collect();
                        let mut o = serde_json::Map::new();
                        for (name, v) in fields {
                            let slot = slots.iter().find_map(|s| match s {
                                Slot::Field(n, k) if *n == name => Some(*k),
                                _ => None,
                            });
                            let written = match (slot, types.iter().find(|(n, _)| *n == name)) {
                                (Some(k), _) => children(k, v)?,
                                (None, Some((_, t))) => field(v, t)?,
                                (None, None) => {
                                    return Err(format!("a node has no field `{name}`"));
                                }
                            };
                            o.insert(name.replace('-', "_"), written);
                        }
                        serde_json::Value::Object(o)
                    }
                    (Val::Variant(case, payload), Type::Variant(t)) => {
                        let ty = t.cases().find(|c| c.name == case).and_then(|c| c.ty);
                        let whole = slots.iter().find_map(|s| match s {
                            Slot::Payload(c, k) if *c == case => Some(*k),
                            _ => None,
                        });
                        let value = match (payload, ty, whole) {
                            (None, _, _) => None,
                            (Some(p), _, Some(k)) => Some(children(k, *p)?),
                            (Some(p), Some(Type::Tuple(tt)), None) => {
                                let Val::Tuple(items) = *p else {
                                    return Err(format!("the case `{case}` holds no tuple"));
                                };
                                let mut each = Vec::with_capacity(items.len());
                                for (i, (v, t)) in items.into_iter().zip(tt.types()).enumerate() {
                                    let slot = slots.iter().find_map(|s| match s {
                                        Slot::Element(c, j, k) if *c == case && *j == i => Some(*k),
                                        _ => None,
                                    });
                                    each.push(match slot {
                                        Some(k) => children(k, v)?,
                                        None => field(v, &t)?,
                                    });
                                }
                                Some(serde_json::Value::Array(each))
                            }
                            (Some(p), Some(t), None) => Some(field(*p, &t)?),
                            (Some(_), None, None) => {
                                return Err(format!("the case `{case}` holds no payload"));
                            }
                        };
                        match value {
                            Some(v) => serde_json::json!({ "$case": case, "value": v }),
                            None => serde_json::json!({ "$case": case }),
                        }
                    }
                    (other, _) => return Err(format!("a node is {other:?}")),
                });
            }
            Ok(serde_json::json!({ "$graph": out }))
        }

        /// `f` on every part of `v` its type says may hold such a value.
        fn walk(
            v: Val,
            ty: &Type,
            f: &dyn Fn(Val, &Type) -> Result<Val, String>,
        ) -> Result<Val, String> {
            let inner =
                |v: Option<Box<Val>>, ty: Option<Type>| -> Result<Option<Box<Val>>, String> {
                    match (v, ty) {
                        (Some(v), Some(ty)) => Ok(Some(Box::new(f(*v, &ty)?))),
                        (v, _) => Ok(v),
                    }
                };
            Ok(match (v, ty) {
                (Val::Record(fields), Type::Record(r)) => {
                    let types: Vec<(String, Type)> =
                        r.fields().map(|f| (f.name.to_string(), f.ty)).collect();
                    let mut out = Vec::with_capacity(fields.len());
                    for (name, v) in fields {
                        let v = match types.iter().find(|(n, _)| *n == name) {
                            Some((_, t)) => f(v, t)?,
                            None => v,
                        };
                        out.push((name, v));
                    }
                    Val::Record(out)
                }
                (Val::List(items), Type::List(l)) => {
                    let t = l.ty();
                    Val::List(
                        items
                            .into_iter()
                            .map(|v| f(v, &t))
                            .collect::<Result<_, _>>()?,
                    )
                }
                (Val::Tuple(items), Type::Tuple(t)) => Val::Tuple(
                    items
                        .into_iter()
                        .zip(t.types())
                        .map(|(v, t)| f(v, &t))
                        .collect::<Result<_, _>>()?,
                ),
                (Val::Option(v), Type::Option(o)) => Val::Option(inner(v, Some(o.ty()))?),
                (Val::Result(Ok(v)), Type::Result(r)) => Val::Result(Ok(inner(v, r.ok())?)),
                (Val::Result(Err(v)), Type::Result(r)) => Val::Result(Err(inner(v, r.err())?)),
                (Val::Variant(case, v), Type::Variant(t)) => {
                    let ty = t.cases().find(|c| c.name == case).and_then(|c| c.ty);
                    Val::Variant(case, inner(v, ty)?)
                }
                (v, _) => v,
            })
        }

        /// A nested value, as its nodes in level order: the node list is a
        /// queue, each node's children appended as it is reached.
        fn encode(root: Val, node: &Type, slots: &[Slot]) -> Result<Val, String> {
            let mut queue = std::collections::VecDeque::from([root]);
            let mut nodes = Vec::new();
            let mut next: u32 = 1;
            while let Some(mut v) = queue.pop_front() {
                let lists = take_lists(&mut v, slots)?;
                let mut indices = Vec::with_capacity(lists.len());
                for children in lists {
                    let k = u32::try_from(children.len()).map_err(|_| "too many nodes")?;
                    indices.push((next..next + k).map(Val::U32).collect());
                    next = next.checked_add(k).ok_or("too many nodes")?;
                    queue.extend(children);
                }
                put_lists(&mut v, slots, indices);
                // A node's other fields, by the node's type.
                nodes.push(walk(v, node, &tangle)?);
            }
            Ok(Val::List(nodes))
        }

        /// Nodes, checked as the component's decoder checks them, as the
        /// nested value they encode, built from the last node back: in level
        /// order each node's children lie after it, one run of indices per
        /// list, continuing the runs before it.
        fn decode(mut nodes: Vec<Val>, node: &Type, slots: &[Slot]) -> Result<Val, String> {
            let len = nodes.len();
            if len == 0 {
                return Err("a value of a type that contains itself has no nodes".into());
            }
            // Each node's runs, checked, and each node's depth.
            let mut runs: Vec<Vec<(usize, usize)>> = Vec::with_capacity(len);
            let mut depth = vec![0usize; len];
            depth[0] = 1;
            let mut next = 1usize;
            for (i, v) in nodes.iter_mut().enumerate() {
                let lists = take_lists(v, slots)?;
                let mut mine = Vec::with_capacity(lists.len());
                for list in lists {
                    let k = list.len();
                    if k > 0 && next <= i {
                        return Err(format!("node {i} holds a node before its own"));
                    }
                    if k > len - next {
                        return Err(format!("node {i} holds a node past the last"));
                    }
                    for (j, x) in list.iter().enumerate() {
                        if *x != Val::U32((next + j) as u32) {
                            return Err(format!(
                                "node {i}'s children are not the {k} indices from {next}"
                            ));
                        }
                    }
                    for c in next..next + k {
                        depth[c] = depth[i] + 1;
                        if depth[c] > NESTED_DEPTH {
                            return Err(format!(
                                "the value is deeper than {NESTED_DEPTH}, the deepest a host nests"
                            ));
                        }
                    }
                    mine.push((next, k));
                    next += k;
                }
                runs.push(mine);
            }
            if next != len {
                return Err(format!("{} node(s) no list holds", len - next));
            }
            let mut built: Vec<Option<Val>> = Vec::with_capacity(len);
            built.resize_with(len, || None);
            for i in (0..len).rev() {
                let mut v = std::mem::replace(&mut nodes[i], Val::Bool(false));
                let lists = runs[i]
                    .iter()
                    .map(|(at, k)| {
                        (*at..at + k)
                            .map(|c| built[c].take().expect("built"))
                            .collect()
                    })
                    .collect();
                put_lists(&mut v, slots, lists);
                built[i] = Some(walk(v, node, &untangle)?);
            }
            Ok(built[0].take().expect("the first node"))
        }
    }

    /// **A host's answer, read through the type the component declares**
    /// (ADR-0166). A data layer's row can hold more than a program asks for:
    /// a record's fields its type does not name are not passed in, and a
    /// field it names that the answer lacks is refused, by name. The rest is
    /// the engine's to check, as it was: a tuple of another length, a case
    /// the variant does not have, or a value of another kind is passed on,
    /// and refused there.
    pub fn project(
        v: wasmtime::component::Val,
        ty: &wasmtime::component::types::Type,
    ) -> Result<wasmtime::component::Val, String> {
        use wasmtime::component::Val;
        use wasmtime::component::types::Type;
        let inner = |v: Option<Box<Val>>, ty: Option<Type>| -> Result<Option<Box<Val>>, String> {
            match (v, ty) {
                (Some(v), Some(ty)) => Ok(Some(Box::new(project(*v, &ty)?))),
                (v, _) => Ok(v),
            }
        };
        let each = |items: Vec<Val>, ty: &Type| -> Result<Vec<Val>, String> {
            items.into_iter().map(|v| project(v, ty)).collect()
        };
        Ok(match (v, ty) {
            (Val::Record(fields), Type::Record(record)) => {
                let mut given: std::collections::BTreeMap<String, Val> =
                    fields.into_iter().collect();
                let mut out = Vec::new();
                for field in record.fields() {
                    // By its WIT name, or as the program writes it (ADR-0291):
                    // a data layer writes a record as Pleris names its
                    // fields, as a browser does (ADR-0172), and the program's
                    // `opens_minute` is the world's `opens-minute`.
                    let written = field.name.replace('-', "_");
                    let v = given
                        .remove(field.name)
                        .or_else(|| given.remove(&written))
                        .ok_or_else(|| match written == field.name {
                            true => format!("the host's record has no field `{}`", field.name),
                            false => format!(
                                "the host's record has no field `{}`, nor `{written}`",
                                field.name
                            ),
                        })?;
                    out.push((field.name.to_string(), project(v, &field.ty)?));
                }
                Val::Record(out)
            }
            (Val::List(items), Type::List(list)) => Val::List(each(items, &list.ty())?),
            (Val::FixedLengthList(items), Type::FixedLengthList(list)) => {
                Val::FixedLengthList(each(items, &list.ty())?)
            }
            (Val::Map(pairs), Type::Map(map)) => {
                let (key, value) = (map.key(), map.value());
                Val::Map(
                    pairs
                        .into_iter()
                        .map(|(k, v)| Ok((project(k, &key)?, project(v, &value)?)))
                        .collect::<Result<_, String>>()?,
                )
            }
            (Val::Tuple(items), Type::Tuple(tuple)) if items.len() == tuple.types().len() => {
                Val::Tuple(
                    items
                        .into_iter()
                        .zip(tuple.types())
                        .map(|(v, ty)| project(v, &ty))
                        .collect::<Result<_, _>>()?,
                )
            }
            (Val::Option(v), Type::Option(option)) => Val::Option(inner(v, Some(option.ty()))?),
            (Val::Result(Ok(v)), Type::Result(result)) => Val::Result(Ok(inner(v, result.ok())?)),
            (Val::Result(Err(v)), Type::Result(result)) => {
                Val::Result(Err(inner(v, result.err())?))
            }
            (Val::Variant(case, v), Type::Variant(variant)) => {
                let ty = variant.cases().find(|c| c.name == case).and_then(|c| c.ty);
                Val::Variant(case, inner(v, ty)?)
            }
            (v, _) => v,
        })
    }

    /// **Does each value hold every invariant `checks` states of it?**
    /// (ADR-0179). Refused by name otherwise: where the value is, what it is,
    /// and what its type holds. A value of another shape than the path says is
    /// refused too: the contract and the value disagree.
    pub fn holds(
        checks: &[crate::Bounded],
        values: &[wasmtime::component::Val],
    ) -> Result<(), String> {
        for check in checks {
            let Some(v) = values.get(check.argument) else {
                return Err(format!(
                    "no value {} to hold `{}`'s invariant",
                    check.argument + 1,
                    check.ty
                ));
            };
            held(check, v, &mut Vec::new())?;
        }
        Ok(())
    }

    fn held(
        check: &crate::Bounded,
        v: &wasmtime::component::Val,
        at: &mut Vec<String>,
    ) -> Result<(), String> {
        use wasmtime::component::Val;
        let Some(step) = check.path.get(at.len()) else {
            // An `Int`'s value, or a `String`'s length in code points
            // (ADR-0225), counted as the language counts it.
            let (n, said) = match (check.measure, v) {
                (crate::Measure::Value, Val::S64(n)) => (i128::from(*n), format!("{n}")),
                (crate::Measure::Length, Val::String(s)) => {
                    let n = s.chars().count();
                    (n as i128, format!("{n} code points long"))
                }
                (crate::Measure::Value, _) => {
                    return Err(format!(
                        "{} is no `Int`, which `{}` is",
                        place(check, at),
                        check.ty
                    ));
                }
                (crate::Measure::Length, _) => {
                    return Err(format!(
                        "{} is no `String`, which `{}` is",
                        place(check, at),
                        check.ty
                    ));
                }
            };
            let above = check.at_least.is_none_or(|b| n >= i128::from(b));
            let below = check.at_most.is_none_or(|b| n <= i128::from(b));
            return match above && below {
                true => Ok(()),
                false => Err(format!(
                    "{} is {said}, and `{}` holds `{}`",
                    place(check, at),
                    check.ty,
                    check.holds
                )),
            };
        };
        let inside = |v: &Val, at: &mut Vec<String>, name: String| {
            at.push(name);
            let r = held(check, v, at);
            at.pop();
            r
        };
        match (step.as_str(), v) {
            ("*", Val::List(items)) => {
                for (i, item) in items.iter().enumerate() {
                    inside(item, at, format!("[{i}]"))?;
                }
                Ok(())
            }
            ("some", Val::Option(o)) => match o {
                Some(v) => inside(v, at, step.clone()),
                None => Ok(()),
            },
            ("ok", Val::Result(Ok(v))) | ("err", Val::Result(Err(v))) => match v {
                Some(v) => inside(v, at, step.clone()),
                None => Ok(()),
            },
            ("ok", Val::Result(Err(_))) | ("err", Val::Result(Ok(_))) => Ok(()),
            (case, Val::Variant(name, payload)) => match (name == case, payload) {
                (true, Some(v)) => inside(v, at, step.clone()),
                _ => Ok(()),
            },
            (field, Val::Record(fields)) => match fields.iter().find(|(n, _)| n == field) {
                Some((_, v)) => inside(v, at, step.clone()),
                None => Err(format!("{} has no field `{field}`", place(check, at))),
            },
            (index, Val::Tuple(items)) => {
                match index.parse::<usize>().ok().and_then(|i| items.get(i)) {
                    Some(v) => inside(v, at, step.clone()),
                    None => Err(format!("{} has no field {index}", place(check, at))),
                }
            }
            _ => Err(format!(
                "{} is not what `{}`'s invariant is checked at",
                place(check, at),
                check.ty
            )),
        }
    }

    /// "argument 2", "argument 1's `lines[0].quantity`", as a refusal names it.
    fn place(check: &crate::Bounded, at: &[String]) -> String {
        let mut path = String::new();
        for step in at {
            if step.starts_with('[') {
                path.push_str(step);
            } else {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(step);
            }
        }
        match path.is_empty() {
            true => format!("value {}", check.argument + 1),
            false => format!("value {}'s `{path}`", check.argument + 1),
        }
    }

    /// **A case a browser sends**: its WIT name and its payload, if any.
    fn case<'a>(
        v: &'a serde_json::Value,
        at: &str,
    ) -> Result<(&'a str, Option<&'a serde_json::Value>), String> {
        let o = v
            .as_object()
            .ok_or_else(|| format!("{at}: expected a case, `{{ \"$case\": .. }}`, got {v}"))?;
        let name = o
            .get("$case")
            .and_then(|c| c.as_str())
            .ok_or_else(|| format!("{at}: a case names its `$case`, and {v} does not"))?;
        if let Some(other) = o.keys().find(|k| *k != "$case" && *k != "value") {
            return Err(format!(
                "{at}: a case carries `$case` and `value`, and not `{other}`"
            ));
        }
        Ok((name, o.get("value")))
    }

    /// One JSON value, as a value of this component type, or why not.
    fn from_json(
        ty: &wasmtime::component::types::Type,
        v: &serde_json::Value,
        at: &str,
    ) -> Result<wasmtime::component::Val, String> {
        use wasmtime::component::Val;
        use wasmtime::component::types::Type;
        let expected = |what: &str| format!("{at}: expected {what}, got {v}");
        let int = |min: i128, max: i128| -> Result<i128, String> {
            let n = v
                .as_i64()
                .map(i128::from)
                .or_else(|| v.as_u64().map(i128::from))
                .ok_or_else(|| expected("an integer"))?;
            if n < min || n > max {
                return Err(format!("{at}: {n} is outside {min}..={max}"));
            }
            Ok(n)
        };
        let float = || {
            v.as_f64()
                .filter(|f| f.is_finite())
                .ok_or_else(|| expected("a finite number"))
        };
        Ok(match ty {
            Type::Bool => Val::Bool(v.as_bool().ok_or_else(|| expected("a boolean"))?),
            Type::S8 => Val::S8(int(i8::MIN.into(), i8::MAX.into())? as i8),
            Type::U8 => Val::U8(int(0, u8::MAX.into())? as u8),
            Type::S16 => Val::S16(int(i16::MIN.into(), i16::MAX.into())? as i16),
            Type::U16 => Val::U16(int(0, u16::MAX.into())? as u16),
            Type::S32 => Val::S32(int(i32::MIN.into(), i32::MAX.into())? as i32),
            Type::U32 => Val::U32(int(0, u32::MAX.into())? as u32),
            Type::S64 => Val::S64(int(i64::MIN.into(), i64::MAX.into())? as i64),
            Type::U64 => Val::U64(int(0, u64::MAX.into())? as u64),
            Type::Float32 => Val::Float32(float()? as f32),
            Type::Float64 => Val::Float64(float()?),
            Type::Char => {
                let s = v.as_str().ok_or_else(|| expected("one character"))?;
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Val::Char(c),
                    _ => return Err(expected("one character")),
                }
            }
            Type::String => Val::String(v.as_str().ok_or_else(|| expected("a string"))?.into()),
            // **A record, field by field** (ADR-0172): the item a page showed,
            // which `add_to_cart` makes its new line from. A browser writes a
            // field by its Pleris name, `store_id`, and the component
            // declares it by its WIT name, `store-id`. A field the type
            // does not declare is not passed in; one it declares and the
            // value lacks is refused.
            Type::Record(record) => {
                let o = v.as_object().ok_or_else(|| expected("an object"))?;
                let mut fields = Vec::new();
                for field in record.fields() {
                    let written = field.name.replace('-', "_");
                    let value = o
                        .get(&written)
                        .ok_or_else(|| format!("{at}: the record has no field `{written}`"))?;
                    fields.push((
                        field.name.to_string(),
                        from_json(&field.ty, value, &format!("{at}.{written}"))?,
                    ));
                }
                Val::Record(fields)
            }
            // **As its nodes** (ADR-0205): the browser's `$graph`, each
            // `$node` the index the component reads where its node holds the
            // type. The component checks the nodes, as it checks a host's.
            Type::List(_) if graph::is_nodes(ty) => graph::from_browser(ty, v, at, &from_json)?,
            Type::List(list) => {
                let items = v.as_array().ok_or_else(|| expected("an array"))?;
                let element = list.ty();
                Val::List(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, x)| from_json(&element, x, &format!("{at}[{i}]")))
                        .collect::<Result<_, _>>()?,
                )
            }
            // **A case, by its WIT name, and its payload** (the integrator's,
            // 2026-10-13; W9's finding in NEXT 6b): `{ "$case": "some",
            // "value": .. }`, as a browser writes a case it sends. A case the
            // type does not declare, a payload a case has none of, or one it
            // lacks, is refused; a case of several fields carries an array.
            // A Pleris sum type is a WIT variant, its cases with no payload
            // included: no component of a program exports an enum.
            Type::Option(option) => match case(v, at)? {
                ("some", Some(x)) => Val::Option(Some(Box::new(from_json(
                    &option.ty(),
                    x,
                    &format!("{at}.some"),
                )?))),
                ("none", None) => Val::Option(None),
                (c, _) => return Err(format!("{at}: no case `{c}` of an option, as sent")),
            },
            Type::Result(result) => {
                let (c, x) = case(v, at)?;
                let (ty, ok) = match c {
                    "ok" => (result.ok(), true),
                    "err" => (result.err(), false),
                    c => return Err(format!("{at}: no case `{c}` of a result")),
                };
                let payload = match (ty, x) {
                    (Some(ty), Some(x)) => Some(Box::new(from_json(&ty, x, &format!("{at}.{c}"))?)),
                    (None, None) => None,
                    _ => {
                        return Err(format!(
                            "{at}: case `{c}`'s payload, as sent, is not its type's"
                        ));
                    }
                };
                Val::Result(if ok { Ok(payload) } else { Err(payload) })
            }
            Type::Variant(variant) => {
                let (c, x) = case(v, at)?;
                let Some(declared) = variant.cases().find(|k| k.name == c) else {
                    return Err(format!("{at}: no case `{c}` of its type"));
                };
                let payload = match (declared.ty, x) {
                    (Some(ty), Some(x)) => Some(Box::new(from_json(&ty, x, &format!("{at}.{c}"))?)),
                    (None, None) => None,
                    _ => {
                        return Err(format!(
                            "{at}: case `{c}`'s payload, as sent, is not its type's"
                        ));
                    }
                };
                Val::Variant(c.to_string(), payload)
            }
            // A case's several fields, in their order.
            Type::Tuple(tuple) => {
                let items = v.as_array().ok_or_else(|| expected("an array"))?;
                let types: Vec<_> = tuple.types().collect();
                if items.len() != types.len() {
                    return Err(format!(
                        "{at}: {} values sent, {} declared",
                        items.len(),
                        types.len()
                    ));
                }
                Val::Tuple(
                    types
                        .iter()
                        .zip(items)
                        .enumerate()
                        .map(|(i, (t, x))| from_json(t, x, &format!("{at}[{i}]")))
                        .collect::<Result<_, _>>()?,
                )
            }
            other => {
                return Err(format!(
                    "{at}: a parameter of type {other:?} is not accepted from a browser"
                ));
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
        limits: &crate::Limits,
        host: &std::collections::BTreeMap<String, HostFn>,
        export: &[&str],
        args: &[wasmtime::component::Val],
    ) -> Result<Vec<wasmtime::component::Val>, String> {
        run_measured(
            engine, component, contract, granted, limits, host, export, args,
        )
        .map(|(results, _)| results)
    }

    /// **What one call cost** (ADR-0046): what E10 task 4's evaluation of
    /// memory strategies measures, per call, through the same path every call
    /// takes.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Usage {
        /// Instructions executed by the call, as wasmtime's fuel counts them.
        pub fuel: u64,
        /// The largest linear memory the instance had, in bytes: pages, so a
        /// multiple of 64 KiB.
        pub peak_memory: usize,
        /// Linking and instantiating: the fresh instance each call gets.
        pub instantiate: std::time::Duration,
        /// The call itself, post-return included.
        pub call: std::time::Duration,
    }

    impl Prepared {
        /// [`Prepared::call_within`], and what the call cost.
        #[allow(clippy::too_many_arguments)]
        pub fn call_measured(
            &self,
            contract: &crate::ComponentContract,
            granted: &crate::Granted,
            limits: &crate::Limits,
            host: &std::collections::BTreeMap<String, HostFn>,
            export: &[&str],
            args: &[wasmtime::component::Val],
        ) -> Result<(Vec<wasmtime::component::Val>, Usage), String> {
            // Its arguments decoded as every call's are.
            self.hold_arguments(contract, export, args)?;
            refuse_unevaluated_authorization(contract, export)?;
            run_measured(
                &self.engine,
                &self.component,
                contract,
                granted,
                limits,
                host,
                export,
                args,
            )
        }
    }

    /// **What a trap's function is named before its cause** (ADR-0267): the
    /// compiler names each function a trap of its own calls `pw-trap: <its
    /// cause's words>` (`pw_core::backend::wasm::TRAP_PREFIX`, which a
    /// conformance test holds equal to this).
    pub const TRAP_PREFIX: &str = "pw-trap: ";

    /// **Why a call stopped** (ADR-0267): a trap of the component's own, by
    /// the function it stopped in, which the compiler names by its cause;
    /// Wasm's own, by its code; and after it, the error as it was. Until
    /// ADR-0267 every trap read as Wasm's "`unreachable` instruction
    /// executed", whatever stopped it.
    fn stopped(e: &wasmtime::Error) -> String {
        let named = e
            .downcast_ref::<wasmtime::WasmBacktrace>()
            .and_then(|trace| {
                trace
                    .frames()
                    .iter()
                    .find_map(|f| f.func_name()?.strip_prefix(TRAP_PREFIX))
                    .map(str::to_string)
            });
        let coded = || {
            use wasmtime::Trap;
            let words = match e.downcast_ref::<Trap>()? {
                Trap::IntegerDivisionByZero => "division by zero",
                Trap::IntegerOverflow => "Int overflow",
                Trap::OutOfFuel => "out of fuel",
                Trap::StackOverflow => "calls nested too deep",
                _ => return None,
            };
            Some(words.to_string())
        };
        match named.or_else(coded) {
            Some(cause) => format!("stopped: {cause}: {e:#}"),
            None => format!("{e:#}"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_measured(
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
        limits: &crate::Limits,
        host: &std::collections::BTreeMap<String, HostFn>,
        export: &[&str],
        args: &[wasmtime::component::Val],
    ) -> Result<(Vec<wasmtime::component::Val>, Usage), String> {
        use wasmtime::Store;
        let started = std::time::Instant::now();
        use wasmtime::component::{Linker, Val};
        let engine = engine.clone();
        let component = component.clone();

        let mut linker: Linker<crate::Meter> = Linker::new(&engine);
        let supplied = crate::linkable(contract, granted);
        let functions = imported_functions(&engine, &component);
        let wanted: BTreeSet<&str> = supplied.iter().map(String::as_str).collect();

        for (interface, funcs) in &functions {
            let here: Vec<&String> = funcs
                .iter()
                .filter(|f| wanted.contains(format!("{interface}#{f}").as_str()))
                .collect();
            if here.is_empty() {
                continue;
            }
            let mut instance = linker
                .instance(interface)
                .map_err(|e| format!("{interface}: {e}"))?;
            for func in here {
                let key = format!("{interface}#{func}");
                // Granted, and the deployment supplies nothing for it: a
                // refusal. Linking a stub would make "authorized" and
                // "implemented" the same word.
                let Some(implementation) = host.get(&key).cloned() else {
                    return Err(format!(
                        "`{key}` is granted and the host implements nothing for it"
                    ));
                };
                let named = key.clone();
                // What its answer must hold (ADR-0179), as the contract states.
                let bounded: Vec<crate::Bounded> = contract
                    .imports
                    .iter()
                    .find(|i| i.key() == key)
                    .map(|i| i.bounded.clone())
                    .unwrap_or_default();
                instance
                    .func_new(func, move |_, ty, args, results| {
                        // A type that contains itself reaches the host
                        // nested, as the program shapes it (ADR-0194).
                        let args: Vec<Val> = args
                            .iter()
                            .cloned()
                            .zip(ty.params())
                            .map(|(v, (_, t))| graph::untangle(v, &t))
                            .collect::<Result<_, _>>()
                            .map_err(|e| wasmtime::Error::msg(format!("`{named}`: {e}")))?;
                        let out = implementation(&args).map_err(wasmtime::Error::msg)?;
                        if out.len() != results.len() {
                            return Err(wasmtime::Error::msg(format!(
                                "the host returned {} values where the operation has {}",
                                out.len(),
                                results.len()
                            )));
                        }
                        // Read through the type the component declares
                        // (ADR-0166): a data layer's row may hold more than
                        // the program asks for.
                        // A nested value of a type that contains itself
                        // is made its nodes first, each then read so.
                        for ((slot, v), ty) in results.iter_mut().zip(out).zip(ty.results()) {
                            *slot = graph::tangle(v, &ty)
                                .and_then(|v| project(v, &ty))
                                .map_err(|e| wasmtime::Error::msg(format!("`{named}`: {e}")))?;
                        }
                        // A data layer's answer comes from outside the
                        // program: an invariant it breaks is a failed call,
                        // never a value the component reads (ADR-0179).
                        holds(&bounded, results).map_err(|why| {
                            wasmtime::Error::msg(format!(
                                "`{named}` answered what breaks an invariant: {why}"
                            ))
                        })?;
                        Ok(())
                    })
                    .map_err(|e| format!("{key}: {e}"))?;
            }
        }

        let mut store = Store::new(&engine, crate::Meter::from(limits));
        store.limiter(|m| &mut m.limits);
        // The engine always meters; an unbounded call is given all of it.
        store
            .set_fuel(limits.fuel.unwrap_or(u64::MAX))
            .map_err(|e| e.to_string())?;
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| e.to_string())?;
        let instantiated = std::time::Instant::now();
        let fuel_before = store.get_fuel().map_err(|e| e.to_string())?;

        // The export by its path, one segment at a time: an interface export
        // is an instance whose function is found inside it.
        let mut index = None;
        for segment in export {
            index = Some(
                instance
                    .get_export_index(&mut store, index.as_ref(), segment)
                    .ok_or_else(|| format!("no export `{}`", export.join("#")))?,
            );
        }
        let index = index.ok_or_else(|| "an empty export path".to_string())?;
        let func = instance
            .get_func(&mut store, index)
            .ok_or_else(|| format!("`{}` is not a function", export.join("#")))?;
        // The result arity is the function's type's, read from the artifact.
        let mut results = vec![Val::Bool(false); func.ty(&store).results().len()];
        // An argument of a type that contains itself may be given nested; it
        // is passed as its nodes (ADR-0194).
        let args: Vec<Val> = args
            .iter()
            .cloned()
            .zip(func.ty(&store).params())
            .map(|(v, (_, t))| graph::tangle(v, &t))
            .collect::<Result<_, _>>()?;
        func.call(&mut store, &args, &mut results)
            .map_err(|e| stopped(&e))?;
        let usage = Usage {
            fuel: fuel_before.saturating_sub(store.get_fuel().map_err(|e| e.to_string())?),
            peak_memory: store.data().limits.peak_memory,
            instantiate: instantiated - started,
            call: instantiated.elapsed(),
        };
        Ok((results, usage))
    }

    /// Each imported interface's FUNCTION exports, read from the component.
    ///
    /// One reader, used by both `instantiate_within` and `call_within`: what an
    /// interface exports is the artifact's answer, and asking it twice in two
    /// ways is how the linker and the caller would come to disagree.
    fn imported_functions(
        engine: &wasmtime::Engine,
        component: &wasmtime::component::Component,
    ) -> BTreeMap<String, Vec<String>> {
        let ty = component.component_type();
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, item) in ty.imports(engine) {
            let ComponentItem::ComponentInstance(instance) = item.ty else {
                continue;
            };
            for (func, kind) in instance.exports(engine) {
                if matches!(kind.ty, ComponentItem::ComponentFunc(_)) {
                    out.entry(name.to_string())
                        .or_default()
                        .push(func.to_string());
                }
            }
        }
        out
    }

    /// **[`instantiate`], within a declared budget.**
    ///
    /// E8 gate item: *"fuel and memory limits per instance, driven by policy
    /// rather than a constant."* E0's `check:fuel` proved wasmtime enforces
    /// both; what it could not say is where the number comes from. [`Limits`]
    /// is that, and it travels with the deployment rather than being compiled
    /// in — a limit hard-coded in the host is a limit nobody can tune for a
    /// component that legitimately needs more.
    ///
    /// [`Limits`]: crate::Limits
    pub fn instantiate_within(
        bytes: &[u8],
        contract: &crate::ComponentContract,
        granted: &crate::Granted,
        limits: &crate::Limits,
    ) -> Result<Instantiated, String> {
        use wasmtime::component::{Component, Linker};
        use wasmtime::{Engine, Store};

        let mut config = engine_config();
        // Metering is an ENGINE setting, so it is decided here from the policy
        // rather than being on always. An engine that consumed fuel for an
        // unbounded instance would charge for something nobody bounded.
        config.consume_fuel(limits.fuel.is_some());
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let component = Component::new(&engine, bytes).map_err(|e| e.to_string())?;

        let mut linker: Linker<crate::Meter> = Linker::new(&engine);
        let supplied = crate::linkable(contract, granted);

        // **Grouped by interface, and only the FUNCTIONS.**
        //
        // A linker instance is created once per interface — the spike's
        // `perfect-web:store/stores` exports both `read` and the `store` record,
        // and defining the instance twice is an error. So is defining a
        // function for `store`, which is a type: what an interface exports is
        // read from the COMPONENT's own type here, so the set of things to
        // define comes from the artifact rather than from the contract's idea
        // of it.
        let ty = component.component_type();
        let mut functions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, item) in ty.imports(&engine) {
            let ComponentItem::ComponentInstance(instance) = item.ty else {
                continue;
            };
            for (func, kind) in instance.exports(&engine) {
                if matches!(kind.ty, ComponentItem::ComponentFunc(_)) {
                    functions
                        .entry(name.to_string())
                        .or_default()
                        .push(func.to_string());
                }
            }
        }

        // One definition per linkable import, and the value behind a capability
        // is never handed over — the guest gets a function that calls back into
        // the host, exactly as `Handle` describes. The bodies are stubs here
        // because what this proves is the LINKING rule; `add_to_cart` running
        // for real is the next gate item and needs the command path.
        let wanted: BTreeSet<&str> = supplied.iter().map(String::as_str).collect();
        for (interface, funcs) in &functions {
            // Decided before the instance is created, so an interface with
            // nothing granted gets no instance at all rather than an empty one
            // the engine might accept.
            let here: Vec<&String> = funcs
                .iter()
                .filter(|f| wanted.contains(format!("{interface}#{f}").as_str()))
                .collect();
            if here.is_empty() {
                continue;
            }
            let mut instance = linker
                .instance(interface)
                .map_err(|e| format!("{interface}: {e}"))?;
            for func in here {
                let name = func.clone();
                instance
                    .func_new(func, move |_, _ty, _args, results| {
                        // **A stub body, and what it stands in for matters.**
                        // A capability's VALUE lives on the host and never
                        // enters the guest's memory — `Handle` is the whole
                        // design. What this proves is the LINKING rule: an
                        // import the granted set covers gets a definition, and
                        // one it does not gets none.
                        //
                        // `option<T>` because that is what the spike's
                        // `stores.read` returns. A differently shaped result
                        // fails when CALLED, not when instantiated, which is
                        // the right boundary for a rule about instantiation.
                        let _ = &name;
                        for slot in results.iter_mut() {
                            *slot = wasmtime::component::Val::Option(None);
                        }
                        Ok(())
                    })
                    .map_err(|e| format!("{interface}#{func}: {e}"))?;
            }
        }

        let mut store = Store::new(&engine, crate::Meter::from(limits));
        store.limiter(|m| &mut m.limits);
        if let Some(fuel) = limits.fuel {
            store.set_fuel(fuel).map_err(|e| e.to_string())?;
        }

        // **The refusal, and it is the engine's.** An import with no definition
        // is unresolvable here, in wasmtime's own words.
        linker
            .instantiate(&mut store, &component)
            .map_err(|e| e.to_string())?;

        let fuel_used = limits.fuel.and_then(|budget| {
            store
                .get_fuel()
                .ok()
                .map(|left| budget.saturating_sub(left))
        });
        Ok(Instantiated {
            linked: supplied,
            fuel_used,
        })
    }
}
