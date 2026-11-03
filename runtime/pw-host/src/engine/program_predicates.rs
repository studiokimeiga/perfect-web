//! **A predicate the program gives a body, asked by the engine itself**
//! (track `row-ownership`; ADR-0115 as amended by ADR-0384's H2; the review
//! of the track, findings 3 and 5).
//!
//! A requirement naming a `component` is the program's authorization: its
//! body, compiled as a component of its own (kind `predicate`). Until the
//! review the engine handed it to its embedder (`authorize_export_with`'s
//! `program` callback), which chose the caller it ran as, the transaction it
//! read and its limits: the track's own harness ran it unbounded, and an
//! exponentially recursive body held the memory adapter's one open command
//! past five seconds. Now the engine asks it, and nothing else may:
//!
//! - **as the export's caller**: the same [`Caller`], so its owned reads are
//!   held to the same reader (`table_owners::admit`), and the same platform
//!   operations, so `current_user()` answers the same user;
//! - **its reads on what the export's are**: [`CallTables`] gives the
//!   export's table functions and the predicate's reads from one place, a
//!   command's own transaction or the tables a query reads, so what the
//!   predicate read is what the export reads and writes against;
//! - **under the export's limits**: the same memory and table bounds, and its
//!   fuel spent from the invocation's one budget, what it leaves the export
//!   (`run_metered`): a body that never ends traps out of fuel, as the
//!   export would. `Limits` bounds no call by wall-clock time; fuel is what
//!   bounds a predicate's running, as it bounds every call's;
//! - **from the program's compiled predicates** ([`Predicates`]): a call given
//!   none for a requirement naming one is refused, and no embedder callback
//!   answers one. A requirement naming none is the deployment's, as before;
//!   and [`super::authorize_export`], the deployment's path, refuses one that
//!   names a component.

use std::collections::BTreeMap;

use super::{HostFn, Prepared, Val};
use crate::tables::{Caller, Manifest};
use crate::{ComponentContract, Granted, Limits};

/// **The tables one call acts on** (track `row-ownership`; ADR-0386): a
/// command's transaction, or the tables a query reads. The engine takes the
/// export's table functions and each of its program predicates' reads from
/// one, so a predicate reads what the export reads and writes against: a
/// command's in its own transaction (ADR-0115's `OwnsPost` reasoning, kept),
/// a query's in its read. Each function is admitted by the gate on
/// [`CallTables::manifest`], which the engine holds every table import to.
pub trait CallTables {
    /// The manifest each table import of the call is held to.
    fn manifest(&self) -> &Manifest;

    /// **The export's table functions**, as the gate admits them for its
    /// contract: a command's reads and writes, staged in its transaction; a
    /// query's reads.
    fn functions(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<BTreeMap<String, HostFn>, String>;

    /// **A program predicate's reads**, admitted as a query's for its
    /// contract (one `predicate` export), each acting on what
    /// [`CallTables::functions`] act on.
    fn predicate_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<BTreeMap<String, HostFn>, String>;
}

/// **A predicate the program gives a body, compiled** (ADR-0384's H2): its
/// component, its contract, one `predicate` export requiring nothing of its
/// own, and what its node grants it. The engine runs it for each
/// requirement naming its component, and nothing answers in its place.
/// Like the [`Granted`] it holds, it is not copied.
pub struct Predicate {
    prepared: Prepared,
    contract: ComponentContract,
    granted: Granted,
}

impl Predicate {
    /// A program predicate, or why `contract` is none: one export, of kind
    /// `predicate`, placed in its component, and requiring nothing itself (a
    /// predicate asking another is ADR-0384's, not built).
    pub fn new(
        prepared: Prepared,
        contract: ComponentContract,
        granted: Granted,
    ) -> Result<Predicate, String> {
        let id = &contract.component_id;
        let [export] = contract.exports.as_slice() else {
            return Err(format!(
                "`{id}` is no program predicate: its contract is not one export"
            ));
        };
        if export.kind != "predicate" {
            return Err(format!(
                "`{id}` is no program predicate: its export is a {}",
                export.kind
            ));
        }
        let Some(place) = &export.component else {
            return Err(format!(
                "`{id}`'s contract does not place its predicate in its component"
            ));
        };
        if !place.authorization.is_empty() {
            return Err(format!(
                "`{id}` requires a predicate itself, and a predicate asking another is not built"
            ));
        }
        Ok(Predicate {
            prepared,
            contract,
            granted,
        })
    }

    /// The component that answers it, as a requirement names it.
    pub fn component_id(&self) -> &str {
        &self.contract.component_id
    }

    /// Its export's path in its component.
    fn path(&self) -> Result<[&str; 2], String> {
        let place = self.contract.exports[0]
            .component
            .as_ref()
            .ok_or_else(|| format!("`{}` has no export path", self.component_id()))?;
        Ok([&place.interface, &place.function])
    }
}

/// **The program's predicates, by the component that answers each**: what
/// a requirement's `component` names.
#[derive(Default)]
pub struct Predicates {
    by_component: BTreeMap<String, Predicate>,
}

impl Predicates {
    /// The program's predicates, each once.
    pub fn new(predicates: impl IntoIterator<Item = Predicate>) -> Result<Predicates, String> {
        let mut by_component = BTreeMap::new();
        for predicate in predicates {
            let id = predicate.component_id().to_string();
            if by_component.insert(id.clone(), predicate).is_some() {
                return Err(format!("the program predicate `{id}` is given twice"));
            }
        }
        Ok(Predicates { by_component })
    }

    /// None: a program that gives no predicate a body.
    pub fn none() -> Predicates {
        Predicates::default()
    }
}

/// **Every requirement of `export`, asked in the order of asking** (see the
/// module): the program's by the engine, the deployment's by `deployment`.
/// The first that does not hold, or cannot be asked, refuses the invocation.
/// Answers the limits the export runs under: the fuel its predicates left.
#[allow(clippy::too_many_arguments)]
pub(super) fn authorize(
    contract: &ComponentContract,
    export: &[&str],
    args: &[Val],
    limits: &Limits,
    platform: &BTreeMap<String, HostFn>,
    tables: &dyn CallTables,
    predicates: &Predicates,
    caller: &Caller,
    mut deployment: impl FnMut(&str, &[&Val]) -> Result<bool, String>,
) -> Result<Limits, String> {
    let mut budget = limits.clone();
    for requirement in super::authorization_for(contract, export)? {
        let selected = super::selected(requirement, args)?;
        let answer = match &requirement.component {
            None => deployment(&requirement.predicate, &selected),
            Some(component) => match predicates.by_component.get(component) {
                Some(predicate) => ask(predicate, &selected, &mut budget, platform, tables, caller),
                None => Err(format!(
                    "its body, component `{component}`, is not among the program's predicates \
                     this call was given"
                )),
            },
        };
        super::verdict(&requirement.predicate, answer)?;
    }
    Ok(budget)
}

/// **One program predicate, asked**: its arguments held to its own
/// contract's invariants, linked with the platform's operations and its
/// reads from `tables`, run as `caller` under `budget`, whose fuel it spends.
fn ask(
    predicate: &Predicate,
    selected: &[&Val],
    budget: &mut Limits,
    platform: &BTreeMap<String, HostFn>,
    tables: &dyn CallTables,
    caller: &Caller,
) -> Result<bool, String> {
    let path = predicate.path()?;
    let args: Vec<Val> = selected.iter().map(|v| (*v).clone()).collect();
    predicate
        .prepared
        .hold_arguments(&predicate.contract, &path, &args)?;
    let mut host = platform.clone();
    host.extend(tables.predicate_reads(&predicate.contract, &predicate.granted)?);
    let (answer, _, left) = super::run_metered(
        &predicate.prepared.engine,
        &predicate.prepared.component,
        &predicate.contract,
        &predicate.granted,
        budget,
        &host,
        Some(tables.manifest()),
        caller,
        &path,
        &args,
    )?;
    if budget.fuel.is_some() {
        budget.fuel = Some(left);
    }
    match answer.as_slice() {
        [Val::Bool(holds)] => Ok(*holds),
        other => Err(format!(
            "`{}` answered {other:?}, not whether it holds",
            predicate.component_id()
        )),
    }
}
