//! **E10-A — the first backend slice.**
//!
//! Architect ruling, 2026-08-09:
//!
//! > **Narrow application surface, general backend spine.** Compile exactly one
//! > real Pleris command (`add_to_cart`) end-to-end first, but do not build any
//! > stage specifically around `add_to_cart`.
//!
//! ```text
//! checked Pleris HIR
//!       ↓
//! Backend IR                       <- ir.rs, general
//!       ↓
//! Wasm core module
//!       ↓
//! existing ComponentContract + generated WIT
//!       ↓
//! Wasm Component
//!       ↓
//! E8 host
//!       ↓
//! add_to_cart actually runs        <- E10-I
//! ```
//!
//! # What this must not do
//!
//! > The backend consumes semantics already established upstream. It must not
//! > rediscover effects, capabilities, placement, privacy, or ABI types.
//!
//! Every one of those has an owner: `effects.rs`, `ontology.rs`, `contract.rs`,
//! `placement.rs`, `boundary.rs`. A backend that re-derived any of them would
//! be a second answer to a question the project has already spent a milestone
//! making singular.
//!
//! # And no Rust in the middle
//!
//! > I would avoid `Pleris → Rust → Wasm` even temporarily. That would satisfy
//! > E10-I but leave the hardest backend questions outsourced to Rust,
//! > especially memory layout and ownership.
//!
//! `wasm-encoder` ENCODES the artifact; the lowering is ours. The distinction
//! is the same one ADR-0002 draws about Marko and ADR-0001 about Koka, and both
//! needed an explicit deletion condition to stay temporary. A Rust stage would
//! have needed one too, and would have been harder to tell apart from the real
//! backend afterwards.

pub mod case;
pub mod component;
pub mod computed;
pub mod ir;
pub mod js;
pub mod js_pure;
pub mod lower;
pub mod signals;
pub mod speculation;
pub mod wasm;

/// **Is this declaration's implementation supplied from outside, and by what?**
///
/// Architect ruling, 2026-08-20:
///
/// > Host implementation is explicit declaration metadata, never inferred from
/// > a missing/`todo` body or from the effect row.
///
/// So it is the `host` policy and nothing else. `todo` means *there is not an
/// implementation here*; it does not secretly mean *the platform supplies an
/// FFI implementation*, and the two are materially different semantics.
///
/// One reader, used by lowering and by the contract, because "which callables
/// does this component depend on" must have one answer all the way through:
/// contract → Backend IR → core Wasm imports → WIT → the E8 artifact audit.
///
/// # An `effect` declaration is not a callable
///
/// `effect session.read { host "pw:host/session#read" }` uses the same spelling,
/// and it means something else: it is the residue of deriving an operation from
/// a CAPABILITY, which the same ruling removed —
///
/// > A capability authorizes an operation. It does not identify the operation.
///
/// So an effect declaration is refused here. It matters because the effect and
/// the `fn` that performs it name the *same* operation today —
/// `pw:host/session#read` is claimed by both `effect session.read` and
/// `fn current_session()` — and a reader that accepted either would answer with
/// whichever it met last. It did: `wit::host_signatures` rendered the effect's
/// empty signature over the function's real one, and the wrong answer looked
/// like a right one.
///
/// The ontology reads that clause separately, and nothing consumes what it
/// reads. See `docs/RISK_QUEUE.md`.
pub fn host_binding(decl: &crate::hir::Decl) -> Option<ir::ImportId> {
    if decl.kind == crate::hir::DeclKind::Effect {
        return None;
    }
    // **An event is the platform's outbox's** (ADR-0208): a command emits
    // `CartChanged` by calling `pw:host/outbox#cart-changed` with its values,
    // and the host commits it with the command's writes or not at all.
    if decl.kind == crate::hir::DeclKind::Event {
        return Some(ir::ImportId {
            interface: OUTBOX_INTERFACE.to_string(),
            name: crate::wit::ident(&decl.name),
        });
    }
    let p = decl.policy("host")?;
    let raw = p.value.trim().trim_matches('"');
    let (interface, name) = raw.split_once('#')?;
    if interface.is_empty() || name.is_empty() {
        return None;
    }
    Some(ir::ImportId {
        interface: interface.to_string(),
        name: name.to_string(),
    })
}

/// **The event an `emits` key names** (ADR-0208), resolved as the checker
/// resolves it (ADR-0088): in the event namespace, from the declaring unit.
pub fn emitted_event(
    ws: &crate::resolve::Workspace,
    unit: usize,
    key: &crate::hir::ClauseKey,
) -> Option<crate::resolve::DefId> {
    use crate::resolve::{Namespace, Resolution};
    let found = match key.name.contains('.') {
        true => ws.resolve_path_in(unit, Namespace::Event, &key.name),
        false => ws.resolve_in(unit, Namespace::Event, &key.name),
    };
    match found {
        Resolution::Local(def) | Resolution::Imported { def, .. } => Some(def),
        _ => None,
    }
}

/// **The query an `invalidates` key names** (ADR-0209), resolved as the
/// checker resolves it (ADR-0088): a query, a subscription or a resource.
pub fn invalidated_query(
    ws: &crate::resolve::Workspace,
    sigs: &crate::signatures::Signatures,
    unit: usize,
    key: &crate::hir::ClauseKey,
) -> Option<crate::resolve::DefId> {
    use crate::resolve::Resolution;
    crate::policy::keyed("invalidates")?
        .iter()
        .find_map(|(ns, kinds)| {
            let found = match key.name.contains('.') {
                true => ws.resolve_path_in(unit, *ns, &key.name),
                false => ws.resolve_in(unit, *ns, &key.name),
            };
            let (Resolution::Local(def) | Resolution::Imported { def, .. }) = found else {
                return None;
            };
            sigs.kind_of(def)
                .is_some_and(|k| kinds.contains(&k))
                .then_some(def)
        })
}

/// **The platform's invalidations** (ADR-0209): each query a command
/// invalidates is a function of it, taking the query's parameters, by the
/// query's whole path, since two modules' queries may share a name.
pub const INVALIDATIONS_INTERFACE: &str = "pw:host/invalidations";

/// **The platform's schedules** (the scheduled-commands track): for each
/// command a body schedules, a function by the command's whole path and what
/// it does, `-at` an instant or `-after` a duration, taking the command's
/// parameters and then the time, and `-cancel`, taking its parameters. The
/// host writes each with the scheduling command's writes, or not at all.
pub const SCHEDULES_INTERFACE: &str = "pw:host/schedules";

/// What a schedules' function does: `at`, `after` or `cancel`.
pub const SCHEDULE_HOWS: [&str; 3] = ["at", "after", "cancel"];

/// The schedules' function that does `how` for the command at `path`.
pub fn schedule_binding(path: &str, how: &str) -> ir::ImportId {
    ir::ImportId {
        interface: SCHEDULES_INTERFACE.to_string(),
        name: format!("{}-{how}", crate::wit::ident(path)),
    }
}

/// **The platform's reads** (ADR-0277): each resource a materialization's
/// body reads, `query R(..)`, is a function of it, taking R's parameters and
/// answering R's value, its `Ok` where R answers a `Result`, by R's whole
/// path. The host answers it from R: a query run, or a materialization's
/// kept entry, regenerated first where it is not current.
pub const READS_INTERFACE: &str = "pw:host/reads";

/// The reads' function for the resource `path` (ADR-0277).
pub fn read_binding(path: &str) -> ir::ImportId {
    ir::ImportId {
        interface: READS_INTERFACE.to_string(),
        name: crate::wit::ident(path),
    }
}

/// The invalidations' function for the query `path` (ADR-0209), given the
/// value at each of its parameters but those `every` lists, which its key
/// writes `_` (ADR-0256): `feed-app-timeline-EVERY-1` takes a session, and
/// drops its entries at every limit. A WIT name's later words may be
/// uppercase, and `wit::ident` writes none, so no query's own name is
/// another's shape.
pub fn invalidation_binding(path: &str, every: &[usize]) -> ir::ImportId {
    let mut name = crate::wit::ident(path);
    if !every.is_empty() {
        name.push_str("-EVERY");
        for i in every {
            name.push_str(&format!("-{i}"));
        }
    }
    ir::ImportId {
        interface: INVALIDATIONS_INTERFACE.to_string(),
        name,
    }
}

/// **The positions an invalidations' function of `path` leaves to every
/// value**, where `name` is one of its (ADR-0256): none for
/// `feed-app-timeline`, the second for `feed-app-timeline-EVERY-1`, and no
/// answer for another query's.
pub fn invalidation_shape(path: &str, name: &str) -> Option<Vec<usize>> {
    let rest = name.strip_prefix(&crate::wit::ident(path))?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    rest.strip_prefix("-EVERY-")?
        .split('-')
        .map(|i| i.parse().ok())
        .collect()
}

/// **The positions an `invalidates` key leaves to every value** (ADR-0256,
/// ADR-0195's ruling 10): each argument written `_`, at its parameter, by
/// its name where it is named and otherwise in order, `names` the query's
/// parameters.
pub fn every_position(
    body: &crate::hir::Body,
    key: &crate::hir::ClauseKey,
    names: &[String],
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut next = 0;
    for a in &key.args {
        let at = match &a.name {
            Some(name) => names.iter().position(|n| n == name),
            None => {
                next += 1;
                Some(next - 1)
            }
        };
        if let Some(i) = at
            && crate::check::every_value(body, a.value)
        {
            out.push(i);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// **Is this declaration an operation the compiler supplies, and which?**
///
/// The `intrinsic` policy and nothing else (ADR-0040), for the reason
/// [`host_binding`] reads `host`: the backend decides by what a declaration
/// says it is, never by its name. `Some(Err(name))` is an `intrinsic` clause
/// naming an operation this backend does not know, which the lowering refuses
/// by that name rather than compiling a call to nothing.
pub fn intrinsic_binding(decl: &crate::hir::Decl) -> Option<Result<ir::Operation, String>> {
    let p = decl.policy("intrinsic")?;
    let name = p.value.trim().trim_matches('"');
    Some(ir::Operation::named(name).ok_or_else(|| name.to_string()))
}

/// **The WIT package the Pleris platform itself owns.**
///
/// Architect ruling, 2026-08-20:
///
/// > `pw:host/carts#add` looks like Pleris itself defines a universal carts
/// > API. It doesn't. […] Do not let `pw:host/carts` become the permanent
/// > standard-library design merely because it was the first thing that made
/// > the demo executable.
///
/// So ownership is read from the package the author WROTE, exactly as a WIT
/// package name is an ownership assertion everywhere else. `pw:host/session#read`
/// is a platform facility with platform ABI-stability expectations;
/// `store:data/carts#add` is the store's own data layer, externally implemented
/// today and eventually replaceable by compiled Pleris.
///
/// Not a spelling-based resolution: the package IS the identity, and this reads
/// it rather than guessing from a function's name.
pub const PLATFORM_PACKAGE: &str = "pw:host";

/// **The platform's table primitives** (track `tables`, ADR-0338, 1.3):
/// `pw:tables/todos#get`, a primitive one adapter per database implements
/// for every table, imported per table. The narrower platform data
/// primitives the architect's ruling of 2026-08-20 expects an app's data
/// operations to become: the platform's, as `pw:host` is.
pub const TABLES_PACKAGE: &str = "pw:tables";

/// Resource-only upload facilities, independent of application row writes.
pub const UPLOADS_PACKAGE: &str = "pw:uploads";

/// **The platform's outbox** (ADR-0208): each event a program declares is a
/// function of it, taking what the event carries.
pub const OUTBOX_INTERFACE: &str = "pw:host/outbox";

/// **One canonical callable definition, consumed by both the contract and the
/// backend.**
///
/// Architect ruling, 2026-08-20:
///
/// ```text
/// operation declaration
///        ↓
/// CallableImport
///       ↙   ↘
/// contract   backend
/// ```
///
/// > not: operation declaration → contract guess / backend guess. You've spent
/// > multiple milestones eliminating the second pattern.
///
/// It was the second pattern for one commit: `contract::host_calls` and
/// `backend::lower::host_imports` each built the facts from the declaration,
/// and they would have agreed until one of them learned something.
/// `ty` projects a resolved type onto the backend representation; `capability` says what authority an effect
/// requires, and returns `None` for one that requires none.
///
/// Both are passed in rather than decided here. Whether `dom.mutate` needs a
/// host capability is the ONTOLOGY's answer — `capability none` on the effect's
/// own declaration — and a second derivation in the backend is what put a
/// capability on a browser operation that needs none.
pub fn callable_of(
    decl: &crate::hir::Decl,
    callee: crate::resolve::DefId,
    signature: &crate::signatures::Signature,
    ty: impl Fn(&crate::resolved::ResolvedType) -> Option<ir::Type>,
    capability: impl Fn(&str) -> Option<crate::contract::Capability>,
) -> Option<ir::CallableImport> {
    let id = host_binding(decl)?;
    let binding = match [PLATFORM_PACKAGE, TABLES_PACKAGE, UPLOADS_PACKAGE]
        .iter()
        .any(|p| id.interface == *p || id.interface.starts_with(&format!("{p}/")))
    {
        true => ir::ImportBinding::PlatformHost,
        false => ir::ImportBinding::External,
    };
    let mut params = Vec::new();
    for p in &signature.params {
        params.push(ty(p.as_ref()?.resolved()?)?);
    }
    let result = match &signature.returns {
        Some(resolved) => ty(resolved.resolved()?)?,
        None => ir::Type::Unit,
    };
    Some(ir::CallableImport {
        id,
        callee,
        binding,
        signature: ir::BackendSignature { params, result },
        required_capabilities: signature
            .effects
            .iter()
            .filter_map(|e| capability(e).map(ir::CapabilityId))
            .collect(),
    })
}
