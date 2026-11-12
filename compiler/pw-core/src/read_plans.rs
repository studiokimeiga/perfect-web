//! Experimental proof-guided planning for declared-table reads.
//!
//! This is a *logical* plan and a checkable transformation contract, not a new
//! SQL optimizer. The generated table ABI already exposes keyed and set reads.
//! Planning keeps their semantics (including missing keys, duplicates, order,
//! privacy and errors) separate from the physical cost of calling an adapter.
//!
//! The inventory is inspectable at build time. Applying a rewrite to arbitrary
//! source-language loops remains a subsequent compiler milestone: this module
//! must not be used to waive PW0405 before the executable lowering exists.

use serde::Serialize;

use crate::tables::{FunctionJson, Primitive, TablesJson};

/// A small, storage-independent algebra. The output of either plan is a
/// per-input-key sequence of rows, in input order, preserving duplicate keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum ReadOp {
    InputKeys,
    KeyedRead {
        function: String,
    },
    SetRead {
        function: String,
    },
    /// A set read may return one row per *distinct* key. Restore exactly the
    /// original sequence of key groups, including missing and duplicate keys.
    ReassembleByInputKey {
        fields: Vec<String>,
    },
}

/// Why a fact may be relied on, versus a claim that has no enforcement.
/// These are compiler-side origins, not strings supplied by an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// A unique key or an index's order in the declared storage schema.
    SchemaEnforced,
    /// A function and its set form generated from the same table declaration.
    CompilerDerived,
    /// Required from the table adapter's parity/conformance tests; the
    /// generated manifest alone does not establish actual runtime behavior.
    AdapterContract,
    /// Not evidence for an automatic rewrite.
    Assumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    pub property: &'static str,
    pub provenance: Provenance,
    pub witness: String,
}

impl Fact {
    fn trusted(&self) -> bool {
        self.provenance != Provenance::Assumed && !self.witness.is_empty()
    }
}

/// A pair of generated operations that can represent the same logical read.
/// The fields are not constructible outside this module: only compiler-generated
/// operation pairs may create a rewrite certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadFamily {
    table: String,
    source: Option<String>,
    keyed: String,
    set: String,
    fields: Vec<String>,
    order: Vec<String>,
    facts: Vec<Fact>,
}

impl ReadFamily {
    pub fn table(&self) -> &str {
        &self.table
    }
    pub fn keyed(&self) -> &str {
        &self.keyed
    }
    pub fn set(&self) -> &str {
        &self.set
    }
    pub fn fields(&self) -> &[String] {
        &self.fields
    }
    pub fn order(&self) -> &[String] {
        &self.order
    }
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    fn from_generated(
        table: &crate::tables::TableJson,
        keyed: &FunctionJson,
        set: &FunctionJson,
    ) -> Self {
        let schema = &table.schema;
        let mut facts = vec![
            Fact {
                property: "same_lookup_fields",
                provenance: Provenance::CompilerDerived,
                witness: format!(
                    "{}: {} and {} both read by {:?}",
                    schema.name, keyed.name, set.name, keyed.by
                ),
            },
            Fact {
                property: "set_read_grouping",
                provenance: Provenance::AdapterContract,
                witness: format!(
                    "{} uses the generated table read ABI; ordered by {:?}",
                    set.name, set.sorted_by
                ),
            },
            Fact {
                property: "read_only",
                provenance: Provenance::CompilerDerived,
                witness: format!("{} and {} have read effects", keyed.name, set.name),
            },
        ];
        if keyed.primitive == Primitive::Get {
            facts.push(Fact {
                property: "unique_key",
                provenance: Provenance::SchemaEnforced,
                witness: format!("{} key {:?}", schema.name, schema.key),
            });
        } else if keyed.primitive == Primitive::Index
            && schema
                .indexes
                .iter()
                .any(|index| index.unique && index.fields == keyed.by)
        {
            facts.push(Fact {
                property: "unique_index",
                provenance: Provenance::SchemaEnforced,
                witness: format!("{} unique index on {:?}", schema.name, keyed.by),
            });
        }
        Self {
            table: schema.name.clone(),
            source: schema.source.clone(),
            keyed: keyed.name.clone(),
            set: set.name.clone(),
            fields: keyed.by.clone(),
            order: set.sorted_by.clone(),
            facts,
        }
    }

    /// All plans start from a reference path which performs exactly the keyed
    /// calls the application requested. No transformation may change effects.
    pub fn reference(&self) -> Vec<ReadOp> {
        vec![
            ReadOp::InputKeys,
            ReadOp::KeyedRead {
                function: self.keyed.clone(),
            },
        ]
    }

    pub fn batched(&self) -> Vec<ReadOp> {
        vec![
            ReadOp::InputKeys,
            ReadOp::SetRead {
                function: self.set.clone(),
            },
            ReadOp::ReassembleByInputKey {
                fields: self.fields.clone(),
            },
        ]
    }
}

/// Discover only pairs issued by the table compiler. Do not infer read
/// equivalence merely from similarly spelled application functions.
pub fn families(tables: &TablesJson) -> Vec<ReadFamily> {
    let mut result = Vec::new();
    for table in &tables.tables {
        for set in &table.functions {
            let keyed_kind = match set.primitive {
                Primitive::GetMany => Primitive::Get,
                Primitive::PrefixMany => Primitive::Prefix,
                Primitive::IndexMany => Primitive::Index,
                _ => continue,
            };
            if set.effect != "read" {
                continue;
            }
            let keyed = table
                .functions
                .iter()
                .find(|f| f.primitive == keyed_kind && f.by == set.by && f.effect == "read");
            if let Some(keyed) = keyed {
                // Incomplete/malformed adapter metadata must not authorize a
                // rewrite. Key fields must exist and agree with the schema.
                if keyed.by.is_empty()
                    || keyed
                        .by
                        .iter()
                        .any(|field| !table.schema.fields.iter().any(|f| f.name == *field))
                {
                    continue;
                }
                result.push(ReadFamily::from_generated(table, keyed, set));
            }
        }
    }
    result.sort_by(|a, b| (&a.table, &a.fields, &a.keyed).cmp(&(&b.table, &b.fields, &b.keyed)));
    result
}

/// Context the caller must establish from semantic analysis and runtime
/// guarantees, not infer from a function's name. Default denies rewriting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadContext {
    pub one_snapshot: bool,
    pub pure_key_projection: bool,
    pub no_observable_per_key_effects: bool,
    pub same_privacy_partition: bool,
    pub error_equivalent: bool,
}

/// Abstract *work units*, not microseconds. These values are calibrated by
/// a separate benchmark; they are not correctness assumptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Costs {
    pub request: u64,
    pub per_key: u64,
    pub batch_setup: u64,
    pub batch_per_key: u64,
}

impl Default for Costs {
    fn default() -> Self {
        Self {
            request: 100,
            per_key: 1,
            batch_setup: 10,
            batch_per_key: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchCapability {
    None,
    /// One request regardless of the number of input keys.
    OneCall,
    /// At most this many input keys per request. Zero is invalid.
    Chunked(u64),
}

impl BatchCapability {
    pub fn calls(self, n: u64) -> Option<u64> {
        match self {
            Self::None | Self::Chunked(0) => None,
            Self::OneCall => Some(u64::from(n > 0)),
            Self::Chunked(k) => Some(n / k + u64::from(n % k != 0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Keyed,
    Batched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    strategy: Strategy,
    input_keys: u64,
    /// Units under the provided cost model, *not* predicted wall time.
    reference_cost: u64,
    batch_cost: Option<u64>,
    blockers: Vec<&'static str>,
    operations: Vec<ReadOp>,
    /// A rewrite certificate is conditional on the checked semantic context.
    certificate: Vec<Fact>,
}

impl Decision {
    pub fn strategy(&self) -> Strategy {
        self.strategy
    }
    pub fn blockers(&self) -> &[&'static str] {
        &self.blockers
    }
    pub fn reference_cost(&self) -> u64 {
        self.reference_cost
    }
    pub fn batch_cost(&self) -> Option<u64> {
        self.batch_cost
    }
    pub fn operations(&self) -> &[ReadOp] {
        &self.operations
    }
    pub fn certificate(&self) -> &[Fact] {
        &self.certificate
    }
}

/// The reference always remains available. Apply the set rewrite only when
/// all semantic preconditions hold and its modeled work is strictly cheaper.
pub fn choose(
    family: &ReadFamily,
    context: ReadContext,
    capability: BatchCapability,
    keys: u64,
    costs: Costs,
) -> Decision {
    let mut blockers = Vec::new();
    if !context.one_snapshot {
        blockers.push("one_snapshot_not_proven");
    }
    if !context.pure_key_projection {
        blockers.push("key_projection_not_pure");
    }
    if !context.no_observable_per_key_effects {
        blockers.push("per_key_effects_may_be_observable");
    }
    if !context.same_privacy_partition {
        blockers.push("privacy_partition_not_preserved");
    }
    if !context.error_equivalent {
        blockers.push("error_or_retry_behavior_not_equivalent");
    }
    if family.facts.is_empty() || family.facts.iter().any(|f| !f.trusted()) {
        blockers.push("read_pair_not_certified");
    }
    if capability.calls(keys).is_none() {
        blockers.push("no_supported_batch_adapter");
    }
    let reference_cost = keys.saturating_mul(costs.request.saturating_add(costs.per_key));
    let batch_cost = capability.calls(keys).map(|calls| {
        calls
            .saturating_mul(costs.request)
            .saturating_add(costs.batch_setup)
            .saturating_add(keys.saturating_mul(costs.batch_per_key))
    });
    if keys == 0 {
        blockers.push("no_input_keys");
    }
    let batched = blockers.is_empty() && batch_cost.is_some_and(|c| c < reference_cost);
    Decision {
        strategy: if batched {
            Strategy::Batched
        } else {
            Strategy::Keyed
        },
        input_keys: keys,
        reference_cost,
        batch_cost,
        blockers,
        operations: if batched {
            family.batched()
        } else {
            family.reference()
        },
        certificate: if batched {
            family.facts.clone()
        } else {
            Vec::new()
        },
    }
}

/// Build-time inventory: available *legal primitives* and their provenance.
/// It never pretends to know the workload, the runtime snapshot, or which plan
/// will win. No execution decision is emitted without a checked ReadContext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub schema_hash: String,
    pub families: Vec<ReadFamily>,
    pub policy: &'static str,
}

pub fn inventory(tables: &TablesJson) -> Inventory {
    Inventory {
        schema_hash: tables.hash.clone(),
        families: families(tables),
        policy: "no automatic loop rewrite until snapshot/effect/error context and executable lowering are verified",
    }
}

/// Reference regrouping semantics for differential testing. A set read's
/// grouped answer can omit absent keys and return each distinct key once;
/// the per-key sequence must retain absence and duplicates, in input order.
pub fn reassemble<K: Ord + Clone, V: Clone>(
    input: &[K],
    grouped: &std::collections::BTreeMap<K, Vec<V>>,
) -> Vec<Vec<V>> {
    input
        .iter()
        .map(|key| grouped.get(key).cloned().unwrap_or_default())
        .collect()
}

/// Execute a checked plan against an adapter's two read primitives. The
/// caller must supply equivalent keyed and set operations from the same
/// generated table ABI and an appropriate transaction/snapshot. This is
/// intentionally *not* wired into arbitrary Pleris loops yet.
/// The batch primitive may deduplicate and sort its results: keys are
/// deduplicated before sending and the original input grouping is restored.
pub fn execute<K, V, E>(
    decision: &Decision,
    keys: &[K],
    capability: BatchCapability,
    mut keyed: impl FnMut(&K) -> Result<Vec<V>, E>,
    mut set: impl FnMut(&[K]) -> Result<Vec<V>, E>,
    key_of: impl Fn(&V) -> K,
) -> Result<Vec<Vec<V>>, E>
where
    K: Ord + Clone,
    V: Clone,
{
    if decision.strategy != Strategy::Batched || decision.input_keys != keys.len() as u64 {
        return keys.iter().map(&mut keyed).collect();
    }
    let limit = match capability {
        BatchCapability::OneCall => usize::MAX,
        BatchCapability::Chunked(n) if n > 0 => usize::try_from(n).unwrap_or(usize::MAX),
        _ => return keys.iter().map(&mut keyed).collect(),
    };
    let mut seen = std::collections::BTreeSet::new();
    let distinct: Vec<K> = keys
        .iter()
        .filter(|key| seen.insert((*key).clone()))
        .cloned()
        .collect();
    let mut grouped = std::collections::BTreeMap::<K, Vec<V>>::new();
    for chunk in distinct.chunks(limit) {
        for row in set(chunk)? {
            grouped.entry(key_of(&row)).or_default().push(row);
        }
    }
    Ok(reassemble(keys, &grouped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_pair() -> ReadFamily {
        ReadFamily {
            table: "Posts".into(),
            source: None,
            keyed: "get".into(),
            set: "get_many".into(),
            fields: vec!["id".into()],
            order: vec!["id".into()],
            facts: vec![Fact {
                property: "set_read_grouping",
                provenance: Provenance::AdapterContract,
                witness: "generated parity".into(),
            }],
        }
    }
    fn safe() -> ReadContext {
        ReadContext {
            one_snapshot: true,
            pure_key_projection: true,
            no_observable_per_key_effects: true,
            same_privacy_partition: true,
            error_equivalent: true,
        }
    }

    #[test]
    fn default_is_fail_closed_and_does_not_pretend_to_optimize() {
        let family = table_pair();
        let d = choose(
            &family,
            ReadContext::default(),
            BatchCapability::OneCall,
            20,
            Costs::default(),
        );
        assert_eq!(d.strategy, Strategy::Keyed);
        assert!(d.certificate.is_empty());
        assert_eq!(d.blockers.len(), 5);
    }

    #[test]
    fn one_call_wins_for_many_keys_with_a_checked_context() {
        let d = choose(
            &table_pair(),
            safe(),
            BatchCapability::OneCall,
            20,
            Costs::default(),
        );
        assert_eq!(d.strategy, Strategy::Batched);
        assert_eq!(d.reference_cost, 2020);
        assert_eq!(d.batch_cost, Some(150));
        assert_eq!(d.operations.len(), 3);
        assert_eq!(d.certificate.len(), 1);
    }

    #[test]
    fn effect_or_privacy_or_error_change_refuses_a_cheaper_plan() {
        let good = safe();
        for change in 0..5 {
            let mut c = good;
            match change {
                0 => c.one_snapshot = false,
                1 => c.pure_key_projection = false,
                2 => c.no_observable_per_key_effects = false,
                3 => c.same_privacy_partition = false,
                _ => c.error_equivalent = false,
            }
            let d = choose(
                &table_pair(),
                c,
                BatchCapability::OneCall,
                100,
                Costs::default(),
            );
            assert_eq!(d.strategy, Strategy::Keyed);
            assert_eq!(d.blockers.len(), 1);
        }
    }

    #[test]
    fn unverified_fact_or_missing_adapter_is_not_an_optimization_proof() {
        let mut f = table_pair();
        f.facts[0].provenance = Provenance::Assumed;
        assert!(
            choose(&f, safe(), BatchCapability::OneCall, 20, Costs::default())
                .blockers
                .contains(&"read_pair_not_certified")
        );
        assert_eq!(
            choose(
                &table_pair(),
                safe(),
                BatchCapability::Chunked(0),
                20,
                Costs::default()
            )
            .strategy,
            Strategy::Keyed
        );
    }

    #[test]
    fn cost_model_can_correctly_choose_the_reference() {
        let d = choose(
            &table_pair(),
            safe(),
            BatchCapability::OneCall,
            1,
            Costs::default(),
        );
        assert_eq!(d.strategy, Strategy::Keyed);
        let expensive = Costs {
            batch_setup: 1_000_000,
            ..Costs::default()
        };
        assert_eq!(
            choose(
                &table_pair(),
                safe(),
                BatchCapability::OneCall,
                20,
                expensive
            )
            .strategy,
            Strategy::Keyed
        );
        let d = choose(
            &table_pair(),
            safe(),
            BatchCapability::Chunked(3),
            20,
            Costs::default(),
        );
        assert_eq!(d.batch_cost, Some(750));
        assert_eq!(d.strategy, Strategy::Batched);
    }

    #[test]
    fn empty_and_extreme_cardinality_do_not_divide_by_zero_or_overflow() {
        assert_eq!(BatchCapability::OneCall.calls(0), Some(0));
        assert_eq!(BatchCapability::Chunked(0).calls(1), None);
        assert_eq!(
            BatchCapability::Chunked(3).calls(u64::MAX),
            Some(u64::MAX / 3)
        );
        let d = choose(
            &table_pair(),
            safe(),
            BatchCapability::Chunked(3),
            u64::MAX,
            Costs {
                request: u64::MAX,
                per_key: u64::MAX,
                batch_setup: u64::MAX,
                batch_per_key: u64::MAX,
            },
        );
        assert_eq!(d.strategy, Strategy::Keyed);
    }

    #[test]
    fn reassembly_preserves_missing_keys_duplicates_and_input_order() {
        let keys = [3, 1, 3, 2, 4, 1];
        let grouped =
            std::collections::BTreeMap::from([(1, vec![10, 11]), (2, vec![20]), (3, vec![30])]);
        let baseline: Vec<Vec<i32>> = keys
            .iter()
            .map(|k| grouped.get(k).cloned().unwrap_or_default())
            .collect();
        assert_eq!(reassemble(&keys, &grouped), baseline);
        assert_eq!(reassemble(&keys, &grouped)[4], Vec::<i32>::new());
    }

    #[test]
    fn serialized_inventory_has_no_claim_of_an_executed_optimization() {
        let family = table_pair();
        let data = serde_json::to_string(&family).unwrap();
        assert!(data.contains("adapter_contract"));
        assert!(data.contains("get_many"));
        assert!(!data.contains("fastest"));
    }
}
