//! **The conformance suite, over the `DataLayer` seam** (ADR-0338's decision
//! 5.5 and the integrator's ruling 7 of 2026-10-15: one suite, written once
//! against the seam; track `data-path`): generated for every table a
//! program's tables.json declares, and run for the layer under test, which
//! it is handed ([`Layer`]: the memory adapter's here, PostgreSQL's by D3),
//! through the seam a server serves it through: a query's reads
//! ([`DataLayer::table_reads`]), a command's reads and writes
//! ([`Staged::table_ops`]), and the server's own commit
//! (`Server::commit_staged`), the platform's transaction with them.
//!
//! The suite reads tables.json itself, apart from any adapter, and its
//! answers are an oracle's: each read's rows filtered from the rows it wrote
//! and ordered as D1 rules (`sorted_by`). Its parts that apply to every
//! adapter, memory's among them, at two sizes of generated data:
//!
//! 1. **answers**: every read function of every table, for present keys,
//!    a missing one, and set forms given duplicates, misses and nothing,
//!    answers exactly the oracle's rows, in their order;
//! 2. **costs**: each read is at least one statement of the layer's
//!    adapter and at most what its source's `sets` states (one for
//!    `one_call`, one a key otherwise), at both sizes, counted below the
//!    layer, by the adapter's own count ([`DataLayer::statements`]): a layer
//!    that read twice for one read would be seen. (Until 2026-11-02 the
//!    suite counted the calls of a wrapper it called once itself, which
//!    could not fail.) Parts 1 and 2, and the programs' query pins, read
//!    both outside a render and in one, through the layer's snapshot, as
//!    every page's query reads (until the re-review of 2026-11-02, outside
//!    a render alone), a render's reads counted once its transaction is
//!    begun ([`begun`]);
//! 3. **staging**: a command's write is read by that command, and by no other
//!    read until it commits; each write is one statement of the adapter,
//!    counted as a read's cost is, once the command's transaction is begun;
//!    and a row referring to a row its own command staged commits with it,
//!    as the guestbook's first signing does;
//! 4. **refused commands**: a command dropped keeps nothing; and so does
//!    each refusal any adapter must give, provoked through what the tables
//!    declare: a row referring to no row, a row deleted while another refers
//!    to it, and a table written past its bound, each refused at its write
//!    or at its commit, whichever the adapter refuses at. (Until 2026-11-02
//!    a table with no reference was refused by a hook in the memory
//!    adapter's layer, which no other adapter has.)
//! 5. **idempotency**: a command whose interaction is sent again is answered
//!    as it was, and writes nothing more (each command of the program that
//!    declares `idempotent_by`, through the server's command route).
//!
//! Not here, since no part of them applies to memory: a database's
//! guarantees and their negative controls, anomalies, and delivery to a
//! second host (ADR-0338 5.5's parts 2, 3 and 6), which D3 runs on
//! PostgreSQL through this same seam.

use super::tables::Asserted;
use super::*;
use crate::data::DataLayer;
use std::collections::BTreeSet;

use serde_json::{Map, Value as Json};

/// **A table as tables.json declares it**, read by the suite alone.
#[derive(Debug, Clone)]
pub(super) struct Table {
    pub name: String,
    /// Each field: its Pleris name and how it is stored.
    pub fields: Vec<(String, Json)>,
    pub key: Vec<String>,
    /// Each index: its fields, and whether it is unique.
    pub indexes: Vec<(Vec<String>, bool)>,
    /// Each reference: the field, and the table it refers to.
    pub references: Vec<(String, String)>,
    /// The most rows it holds, where it states `bounded`.
    pub bounded: Option<usize>,
    pub source: String,
    pub functions: Vec<Function>,
}

/// **A table's function as tables.json declares it.**
#[derive(Debug, Clone)]
pub(super) struct Function {
    pub import: String,
    pub primitive: String,
    pub by: Vec<String>,
    pub sorted_by: Vec<String>,
}

impl Function {
    fn writes(&self) -> bool {
        matches!(
            self.primitive.as_str(),
            "insert" | "put" | "delete" | "delete-prefix"
        )
    }
    fn many(&self) -> bool {
        self.primitive.ends_with("-many")
    }
}

impl Table {
    fn function(&self, primitive: &str) -> &Function {
        self.functions
            .iter()
            .find(|f| f.primitive == primitive)
            .unwrap_or_else(|| panic!("{} has a `{primitive}`", self.name))
    }

    /// Whether a function's read answers one row or none: a `get`, or a
    /// unique index's read.
    fn one(&self, function: &Function) -> bool {
        function.primitive == "get"
            || (function.primitive == "index"
                && self
                    .indexes
                    .iter()
                    .any(|(fields, unique)| *unique && *fields == function.by))
    }

    fn stored(&self, field: &str) -> &Json {
        &self
            .fields
            .iter()
            .find(|(name, _)| name == field)
            .unwrap_or_else(|| panic!("{} has a field `{field}`", self.name))
            .1
    }
}

/// **The tables a build declares**, from its tables.json.
pub(super) fn tables_of(build: &std::path::Path) -> Vec<Table> {
    let text = std::fs::read_to_string(build.join("tables.json")).expect("tables.json");
    let json: Json = serde_json::from_str(&text).expect("tables.json is JSON");
    let strings = |value: &Json| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| s.as_str().expect("a name").to_string())
            .collect()
    };
    json["tables"]
        .as_array()
        .expect("its tables")
        .iter()
        .map(|t| Table {
            name: t["name"].as_str().expect("a name").to_string(),
            fields: t["fields"]
                .as_array()
                .expect("fields")
                .iter()
                .map(|f| {
                    (
                        f["name"].as_str().expect("a field").to_string(),
                        f["stored"].clone(),
                    )
                })
                .collect(),
            key: strings(&t["key"]),
            indexes: t["indexes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| (strings(&i["fields"]), i["unique"].as_bool() == Some(true)))
                .collect(),
            references: t["references"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    (
                        r["field"].as_str().expect("a field").to_string(),
                        r["table"].as_str().expect("a table").to_string(),
                    )
                })
                .collect(),
            bounded: t["bounded"].as_u64().map(|b| b as usize),
            source: t["source"].as_str().unwrap_or_default().to_string(),
            functions: t["functions"]
                .as_array()
                .expect("functions")
                .iter()
                .map(|f| Function {
                    import: f["import"].as_str().expect("an import").to_string(),
                    primitive: f["primitive"].as_str().expect("a primitive").to_string(),
                    by: strings(&f["by"]),
                    sorted_by: strings(&f["sorted_by"]),
                })
                .collect(),
        })
        .collect()
}

/// **A value of a stored type**, numbered by `seed`: distinct seeds give
/// distinct values wherever the type has that many, each within its bounds.
/// A stored type the suite generates no value of fails the suite, named:
/// never a table left out.
fn value(stored: &Json, seed: usize) -> Json {
    if stored == "bool" {
        return Json::Bool(seed % 2 == 1);
    }
    if let Some(string) = stored.get("string") {
        let least = string["min_length"].as_u64().unwrap_or(0) as usize;
        let most = string["max_length"].as_u64().map(|m| m as usize);
        let mut text = format!("v{seed}");
        while text.chars().count() < least {
            text.push('x');
        }
        if let Some(most) = most {
            assert!(
                text.chars().count() <= most,
                "the suite numbers no {seed}th value within {stored}"
            );
        }
        return Json::String(text);
    }
    if let Some(int) = stored.get("int") {
        let least = int["at_least"].as_i64().unwrap_or(0);
        let number = least + seed as i64;
        if let Some(most) = int["at_most"].as_i64() {
            assert!(
                number <= most,
                "the suite numbers no {seed}th value within {stored}"
            );
        }
        return Json::from(number);
    }
    if let Some(inner) = stored.get("option") {
        return match seed % 3 {
            0 => Json::Null,
            _ => value(inner, seed),
        };
    }
    panic!("the suite generates no value stored as {stored}: add it before serving such a table")
}

/// **Each table's rows, `n` of each, generated**: a key's leading fields
/// from a few values each (so a prefix holds several rows), its last field
/// one a row; a unique index's fields one a row, another index's a few; a
/// field that refers to a table one of that table's keys. Tables are
/// generated after the tables they refer to.
pub(super) fn generated(
    tables: &[Table],
    n: usize,
    from: usize,
) -> BTreeMap<String, Vec<Map<String, Json>>> {
    let mut rows: BTreeMap<String, Vec<Map<String, Json>>> = BTreeMap::new();
    let mut left: Vec<&Table> = tables.iter().collect();
    while !left.is_empty() {
        let ready = left
            .iter()
            .position(|t| {
                t.references
                    .iter()
                    .all(|(_, to)| to == &t.name || rows.contains_key(to))
            })
            .expect("tables whose references can be generated in order");
        let table = left.remove(ready);
        let mut made = Vec::new();
        for i in from..from + n {
            let mut row = Map::new();
            for (field, stored) in &table.fields {
                let unique_index = table
                    .indexes
                    .iter()
                    .any(|(fields, unique)| *unique && fields.contains(field));
                let other_index = table
                    .indexes
                    .iter()
                    .any(|(fields, unique)| !*unique && fields.contains(field));
                let leading =
                    table.key.len() > 1 && table.key[..table.key.len() - 1].contains(field);
                let refers = table.references.iter().find(|(f, _)| f == field);
                let made_value = match refers {
                    Some((_, to)) => {
                        let target = tables.iter().find(|t| &t.name == to).expect("its target");
                        let keys = &rows[to];
                        let referred = &keys[i % keys.len()];
                        referred[&target.key[0]].clone()
                    }
                    None if unique_index => value(stored, i),
                    None if leading || other_index => value(stored, i % 3),
                    None => value(stored, i),
                };
                row.insert(field.clone(), made_value);
            }
            made.push(row);
        }
        rows.insert(table.name.clone(), made);
    }
    rows
}

/// A JSON value as the WIT `Val` of its stored type.
fn wit(stored: &Json, value: &Json) -> Val {
    if let Some(inner) = stored.get("option") {
        return Val::Option(match value {
            Json::Null => None,
            some => Some(Box::new(wit(inner, some))),
        });
    }
    match value {
        Json::Bool(b) => Val::Bool(*b),
        Json::String(s) => Val::String(s.clone()),
        Json::Number(n) => Val::S64(n.as_i64().expect("an integer")),
        other => panic!("no WIT value for {other} stored as {stored}"),
    }
}

/// A WIT `Val` as JSON, the adapter's form: an option's none is null.
fn json(value: &Val) -> Json {
    match value {
        Val::Bool(b) => Json::Bool(*b),
        Val::String(s) => Json::String(s.clone()),
        Val::S64(n) => Json::from(*n),
        Val::Option(None) => Json::Null,
        Val::Option(Some(inner)) => json(inner),
        other => panic!("no stored value is {other:?}"),
    }
}

/// A row as its WIT record: its fields in declared order, by WIT names.
fn wit_row(table: &Table, row: &Map<String, Json>) -> Val {
    Val::Record(
        table
            .fields
            .iter()
            .map(|(field, stored)| (pw_host::tables::wit_name(field), wit(stored, &row[field])))
            .collect(),
    )
}

/// A WIT record as a row, by the table's Pleris names.
fn row_of(table: &Table, record: &Val) -> Map<String, Json> {
    assert!(
        matches!(record, Val::Record(_)),
        "a row is a record: {record:?}"
    );
    let Val::Record(fields) = record else {
        unreachable!("asserted above")
    };
    assert_eq!(fields.len(), table.fields.len(), "a row of {}", table.name);
    fields
        .iter()
        .zip(&table.fields)
        .map(|((wit_name, value), (field, _))| {
            assert_eq!(*wit_name, pw_host::tables::wit_name(field));
            (field.clone(), json(value))
        })
        .collect()
}

/// **A selector's values as the function is given them**: one scalar a
/// field, or, in a set form, one value a selector (a record of its fields,
/// by WIT names, where it has several).
fn selector(table: &Table, function: &Function, values: &[Json]) -> Val {
    match function.by.as_slice() {
        [field] => wit(table.stored(field), &values[0]),
        fields => Val::Record(
            fields
                .iter()
                .zip(values)
                .map(|(f, v)| (pw_host::tables::wit_name(f), wit(table.stored(f), v)))
                .collect(),
        ),
    }
}

fn arguments(table: &Table, function: &Function, selectors: &[Vec<Json>]) -> Vec<Val> {
    if function.many() {
        vec![Val::List(
            selectors
                .iter()
                .map(|values| selector(table, function, values))
                .collect(),
        )]
    } else if function.by.is_empty() {
        Vec::new()
    } else {
        function
            .by
            .iter()
            .zip(&selectors[0])
            .map(|(field, value)| wit(table.stored(field), value))
            .collect()
    }
}

/// A scalar's order, as D1 rules it: by its stored value's own order.
fn compare(a: &Json, b: &Json) -> std::cmp::Ordering {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => x.as_i64().cmp(&y.as_i64()),
        (Json::String(x), Json::String(y)) => x.cmp(y),
        (Json::Bool(x), Json::Bool(y)) => x.cmp(y),
        (Json::Null, Json::Null) => std::cmp::Ordering::Equal,
        (Json::Null, _) => std::cmp::Ordering::Less,
        (_, Json::Null) => std::cmp::Ordering::Greater,
        _ => panic!("two values of different types compared: {a}, {b}"),
    }
}

/// **The oracle**: the rows a read answers, from all the rows there are:
/// those whose `by` fields hold one of the selectors, each once, ordered by
/// `sorted_by`.
fn oracle(
    function: &Function,
    rows: &[Map<String, Json>],
    selectors: &[Vec<Json>],
) -> Vec<Map<String, Json>> {
    let mut found: Vec<Map<String, Json>> = rows
        .iter()
        .filter(|row| {
            function.by.is_empty()
                || selectors.iter().any(|values| {
                    function
                        .by
                        .iter()
                        .zip(values)
                        .all(|(field, value)| row[field] == *value)
                })
        })
        .cloned()
        .collect();
    found.sort_by(|a, b| {
        function
            .sorted_by
            .iter()
            .map(|field| compare(&a[field], &b[field]))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    found
}

/// **The suite's own contracts**: a query reading every read function of
/// every table, and a command reading and writing every function, each
/// admitted on a node granting each table's read and write; so the gate
/// admits each to the whole manifest, as it admits a program's components
/// each to theirs.
pub(super) fn suite_contracts(tables: &[Table]) -> [(ComponentContract, Granted); 2] {
    let contract = |id: &str, kind: &str, writes: bool| {
        let imports: Vec<pw_host::Import> = tables
            .iter()
            .flat_map(|t| t.functions.iter().map(move |f| (t, f)))
            .filter(|(_, f)| writes || !f.writes())
            .map(|(t, f)| {
                let (interface, name) = f.import.split_once('#').expect("an import's name");
                pw_host::Import {
                    interface: interface.to_string(),
                    name: name.to_string(),
                    capability: format!(
                        "database.{}<{}>",
                        if f.writes() { "write" } else { "read" },
                        t.name
                    ),
                    kind: pw_host::ImportKind::HostCapability,
                    bounded: Vec::new(),
                    event: None,
                    invalidates: None,
                    every: Vec::new(),
                    reads: None,
                    schedules: None,
                }
            })
            .collect();
        let required: BTreeSet<String> = imports.iter().map(|i| i.capability.clone()).collect();
        ComponentContract {
            component_id: id.to_string(),
            capability_mapping: pw_host::CAPABILITY_MAPPING,
            abi_schema: String::new(),
            required_capabilities: required
                .iter()
                .map(|c| {
                    let (operation, argument) = c
                        .strip_prefix("database.")
                        .and_then(|c| c.strip_suffix('>'))
                        .and_then(|c| c.split_once('<'))
                        .expect("a table's capability");
                    pw_host::Capability {
                        family: "database".into(),
                        operation: operation.into(),
                        argument: Some(argument.into()),
                    }
                })
                .collect(),
            allowed_placements: vec!["origin".into()],
            imports,
            exports: vec![pw_host::Export {
                name: id.to_string(),
                kind: kind.to_string(),
                binding: Default::default(),
                component: Some(pw_host::ComponentExport {
                    interface: format!("pw:app/{id}"),
                    function: id.to_string(),
                    authorization: Vec::new(),
                    idempotent_by: None,
                    invoked_by: None,
                    bounded: Vec::new(),
                }),
            }],
        }
    };
    let topology = Topology {
        nodes: vec![Node {
            name: "origin".into(),
            world: "origin".into(),
            grants: tables
                .iter()
                .flat_map(|t| {
                    [
                        format!("database.read<{}>", t.name),
                        format!("database.write<{}>", t.name),
                    ]
                })
                .collect(),
        }],
    };
    let admitted = |contract: ComponentContract| {
        let actual: Vec<String> = contract.imports.iter().map(|i| i.key()).collect();
        let admission = admit(&contract, &topology, "origin", &actual);
        assert!(admission.is_admitted(), "{admission:?}");
        let granted = Granted::from(&admission, &BTreeMap::new()).expect("granted");
        (contract, granted)
    };
    [
        admitted(contract("conformance.reads", "query", false)),
        admitted(contract("conformance.writes", "command", true)),
    ]
}

/// A host function called, and what it answered.
fn call(ops: &data::Ops, import: &str, args: &[Val]) -> Vec<Val> {
    let f = ops.get(import);
    assert!(f.is_some(), "the gate admitted `{import}`");
    let Some(f) = f else {
        unreachable!("asserted above")
    };
    f(args).asserted(import)
}

/// **Rows written through the seam and committed through the server's own
/// commit**: one command inserting each row, which the materializer's
/// transaction publishes.
pub(super) fn written(
    s: &Server,
    tables: &[Table],
    rows: &BTreeMap<String, Vec<Map<String, Json>>>,
) {
    let wrote = write_rows(s, tables, rows);
    assert_eq!(wrote, Ok(()), "the rows are inserted and commit");
}

/// [`written`], saying what went wrong rather than asserting it: for a
/// writer on a thread of the server's own (a test's hook in a render), whose
/// test asserts what it said.
pub(super) fn write_rows(
    s: &Server,
    tables: &[Table],
    rows: &BTreeMap<String, Vec<Map<String, Json>>>,
) -> Result<(), String> {
    let [_, (writes, granted)] = suite_contracts(tables);
    let mut staging = s.data.begin("conformance");
    let ops = staging.table_ops(&writes, &granted)?;
    for table in tables {
        let import = &table.function("insert").import;
        let insert = ops
            .get(import)
            .ok_or_else(|| format!("the gate admitted no `{import}`"))?;
        for row in &rows[&table.name] {
            let answer = insert(&[wit_row(table, row)])?;
            if answer != [Val::Result(Ok(None))] {
                return Err(format!("{}: a row refused: {answer:?}", table.name));
            }
        }
    }
    drop(ops);
    match s.commit_staged(&mut *staging, &[], &[], None, &[], None, None) {
        Ok(CommitOutcome::Applied(_)) => Ok(()),
        Ok(_) => Err("the rows committed nothing".into()),
        Err(why) => Err(why),
    }
}

/// The published row of `table` at `row`'s key, through the query's seam.
fn published(s: &Server, table: &Table, row: &Map<String, Json>) -> Vec<Val> {
    let [(reads, granted), _] = suite_contracts(std::slice::from_ref(table));
    let ops = s
        .data
        .table_reads(&reads, &granted, None)
        .asserted("its reads");
    let get = table.function("get");
    let key: Vec<Json> = get.by.iter().map(|f| row[f].clone()).collect();
    call(&ops, &get.import, &arguments(table, get, &[key]))
}

fn some_row(table: &Table, row: &Map<String, Json>) -> Vec<Val> {
    vec![Val::Option(Some(Box::new(wit_row(table, row))))]
}

/// **The statements the layer's adapter has run**, as the layer under test
/// counts them below itself ([`DataLayer::statements`]); a layer the suite
/// runs on must count them, or it holds no read to its cost.
fn statements(s: &Server) -> u64 {
    let counted = s.data.statements();
    assert!(
        counted.is_some(),
        "the layer under test counts its adapter's statements"
    );
    counted.unwrap_or_default()
}

/// **Parts 1 and 2 at one size**: every read function of every table
/// answers the oracle's rows, each read at least one statement of the
/// layer's adapter and at most what its source's `sets` states; both outside
/// a render and in one, through the layer's snapshot (NEXT 4c), which is
/// how every page's query reads. (Until the re-review of 2026-11-02 the
/// suite read outside a render alone, and a layer whose render reads called
/// its adapter three times passed it.)
fn answers_and_costs(
    s: &Server,
    tables: &[Table],
    sets: &BTreeMap<String, String>,
    n: usize,
) -> usize {
    let rows = generated(tables, n, 0);
    written(s, tables, &rows);
    let [(reads, granted), _] = suite_contracts(tables);
    let snapshot = s.data.snapshot();
    assert!(
        snapshot.is_some(),
        "the layer under test takes a render's snapshot"
    );
    let mut checked = 0;
    for (way, snapshot) in [
        ("outside a render", None),
        ("in a render", snapshot.as_ref()),
    ] {
        checked += answers_and_costs_read(
            s,
            tables,
            sets,
            n,
            &rows,
            (way, snapshot),
            (&reads, &granted),
        );
    }
    checked
}

/// [`answers_and_costs`], one way of reading: outside a render (no
/// snapshot), or in one (its snapshot).
fn answers_and_costs_read(
    s: &Server,
    tables: &[Table],
    sets: &BTreeMap<String, String>,
    n: usize,
    rows: &BTreeMap<String, Vec<Map<String, Json>>>,
    (way, snapshot): (&str, Option<&data::Snapshot>),
    (reads, granted): (&ComponentContract, &Granted),
) -> usize {
    let ops: data::Ops = s
        .data
        .table_reads(reads, granted, snapshot)
        .asserted("the reads");
    if snapshot.is_some() {
        begun(&ops, &tables[0], &rows[&tables[0].name][0]);
    }
    let mut checked = 0;
    for table in tables {
        let all = &rows[&table.name];
        for function in table.functions.iter().filter(|f| !f.writes()) {
            // Each value `by` takes in the rows, and one no row holds.
            let mut present: Vec<Vec<Json>> = Vec::new();
            for row in all {
                let values: Vec<Json> = function.by.iter().map(|f| row[f].clone()).collect();
                if !present.contains(&values) {
                    present.push(values);
                }
            }
            let missing: Vec<Json> = function
                .by
                .iter()
                .map(|f| value(table.stored(f), 10_000 + n))
                .collect();
            let cases: Vec<Vec<Vec<Json>>> = if function.by.is_empty() {
                vec![vec![]]
            } else if function.many() {
                let mut duplicated = present.clone();
                duplicated.push(present[0].clone());
                duplicated.push(missing.clone());
                duplicated.reverse();
                vec![present.clone(), duplicated, vec![missing.clone()], vec![]]
            } else {
                present
                    .iter()
                    .cloned()
                    .chain([missing.clone()])
                    .map(|s| vec![s])
                    .collect()
            };
            for selectors in cases {
                let before = statements(s);
                let answer = call(
                    &ops,
                    &function.import,
                    &arguments(table, function, &selectors),
                );
                let made = statements(s) - before;
                // Part 2: the adapter's statements, within the source's
                // `sets`: one where it states `one_call`, one a key
                // otherwise, and one for a read by one key or none; at least
                // one, but for a set read of no key, which an adapter may
                // answer without asking its database.
                let keys = selectors.len().max(1) as u64;
                let stated = match sets.get(&table.source).map(String::as_str) {
                    Some("one_call") => 1,
                    _ => keys,
                };
                let least = u64::from(!(function.many() && selectors.is_empty()));
                assert!(
                    (least..=stated).contains(&made),
                    "{} with {} selector(s), {way}: {made} statement(s) of the adapter, and \
                     its source states at most {stated}",
                    function.import,
                    selectors.len()
                );
                // Part 1: the oracle's rows, in its order.
                let expected = oracle(function, all, &selectors);
                let got: Option<Vec<Map<String, Json>>> = match answer.as_slice() {
                    [Val::Option(found)] if table.one(function) => {
                        Some(found.iter().map(|r| row_of(table, r)).collect())
                    }
                    [Val::List(found)] if !table.one(function) => {
                        Some(found.iter().map(|r| row_of(table, r)).collect())
                    }
                    _ => None,
                };
                assert!(
                    got.is_some(),
                    "{} answered {answer:?}, {way}",
                    function.import
                );
                let got = got.unwrap_or_default();
                assert_eq!(
                    got, expected,
                    "{} given {selectors:?}, at {n} rows a table, {way}",
                    function.import
                );
                checked += 1;
            }
        }
    }
    checked
}

/// A row of `table` no other has (numbered from `from`), each field that
/// refers to a table referring to a row of `base`'s.
fn fresh(
    tables: &[Table],
    table: &Table,
    base: &BTreeMap<String, Vec<Map<String, Json>>>,
    from: usize,
) -> Map<String, Json> {
    let mut row = generated(tables, 1, from)[&table.name][0].clone();
    for (field, to) in &table.references {
        let target = tables.iter().find(|t| &t.name == to).expect("its target");
        row.insert(field.clone(), base[to][0][&target.key[0]].clone());
    }
    row
}

/// **A command the tables must refuse, refused whole**: `write` makes its
/// writes through the seam, and the adapter refuses one of them (a write
/// refused traps the command, which then commits nothing) or its commit,
/// whichever it refuses at. Either way the command is undone; the caller
/// holds the tables to having kept nothing of it.
fn refused_whole(
    s: &Server,
    tables: &[Table],
    what: &str,
    write: impl FnOnce(&data::Ops) -> Result<(), String>,
) {
    let [_, (writes, granted)] = suite_contracts(tables);
    let mut staging = s.data.begin("conformance");
    let ops = staging
        .table_ops(&writes, &granted)
        .asserted("its functions");
    let wrote = write(&ops);
    drop(ops);
    if wrote.is_ok() {
        let commit = s.commit_staged(&mut *staging, &[], &[], None, &[], None, None);
        assert!(
            commit.is_err(),
            "{what}: the adapter refuses the write or its commit"
        );
    }
    drop(staging);
}

/// A write through the seam: its answer, `Err` where the adapter refused it.
fn write(ops: &data::Ops, import: &str, args: &[Val]) -> Result<(), String> {
    let f = ops.get(import);
    assert!(f.is_some(), "the gate admitted `{import}`");
    let Some(f) = f else {
        unreachable!("asserted above")
    };
    match f(args)?.as_slice() {
        [Val::Result(Err(conflict))] => Err(format!("refused by its row: {conflict:?}")),
        _ => Ok(()),
    }
}

/// How many rows `table` holds, as a read every reader makes (its `all`,
/// which a bounded table has).
fn held(s: &Server, table: &Table) -> usize {
    let [(reads, granted), _] = suite_contracts(std::slice::from_ref(table));
    let ops = s
        .data
        .table_reads(&reads, &granted, None)
        .asserted("its reads");
    match call(&ops, &table.function("all").import, &[]).as_slice() {
        [Val::List(rows)] => rows.len(),
        other => panic!("{}'s `all` answered {other:?}", table.name),
    }
}

/// **Parts 3 and 4**: a command's write is read by it alone until it
/// commits; a command dropped keeps nothing; and so does each refusal any
/// adapter must give, provoked through what the tables declare (a row
/// referring to no row, a row deleted while another refers to it, a table
/// written past its bound). Each table's refusals provoked, by its name.
fn staging_and_refusals(s: &Server, tables: &[Table]) -> BTreeMap<String, usize> {
    let base = generated(tables, 3, 0);
    written(s, tables, &base);
    let [_, (writes, granted)] = suite_contracts(tables);
    let mut provoked: BTreeMap<String, usize> = BTreeMap::new();
    for table in tables {
        // A row no other has, referring to rows that are there.
        let row = fresh(tables, table, &base, 50_000);
        let none = vec![Val::Option(None)];
        let get = table.function("get");
        let key: Vec<Json> = get.by.iter().map(|f| row[f].clone()).collect();
        // Part 3: staged, read by its command alone; committed, by every read.
        let mut staging = s.data.begin("conformance");
        let ops = staging
            .table_ops(&writes, &granted)
            .asserted("its functions");
        call(
            &ops,
            &table.function("insert").import,
            &[wit_row(table, &row)],
        );
        assert_eq!(
            call(
                &ops,
                &get.import,
                &arguments(table, get, std::slice::from_ref(&key))
            ),
            some_row(table, &row),
            "{}: a command reads its own write",
            table.name
        );
        assert_eq!(
            published(s, table, &row),
            none,
            "{}: no other read sees a staged write",
            table.name
        );
        drop(ops);
        assert!(matches!(
            s.commit_staged(&mut *staging, &[], &[], None, &[], None, None),
            Ok(CommitOutcome::Applied(_))
        ));
        drop(staging);
        assert_eq!(published(s, table, &row), some_row(table, &row));
        // Part 4: dropped without its commit, it keeps nothing.
        let other = fresh(tables, table, &base, 60_000);
        let staging = s.data.begin("conformance");
        let ops = staging
            .table_ops(&writes, &granted)
            .asserted("its functions");
        call(
            &ops,
            &table.function("insert").import,
            &[wit_row(table, &other)],
        );
        drop(ops);
        drop(staging);
        assert_eq!(published(s, table, &other), none, "{}: dropped", table.name);
        let insert = &table.function("insert").import;
        let refusals = provoked.entry(table.name.clone()).or_default();
        // Refused, by a reference to no row: the command's other row with it.
        if let Some((field, to)) = table.references.first() {
            let target = tables.iter().find(|t| &t.name == to).expect("its target");
            let mut orphan = fresh(tables, table, &base, 70_000);
            orphan.insert(field.clone(), value(target.stored(&target.key[0]), 70_000));
            refused_whole(
                s,
                tables,
                &format!("{} referring to no {to}", table.name),
                |ops| {
                    write(ops, insert, &[wit_row(table, &other)])?;
                    write(ops, insert, &[wit_row(table, &orphan)])
                },
            );
            assert_eq!(
                published(s, table, &orphan),
                none,
                "{}: an orphan",
                table.name
            );
            assert_eq!(
                published(s, table, &other),
                none,
                "{}: and its fellow",
                table.name
            );
            *refusals += 1;
        }
        // Refused, by a row deleted while another refers to it.
        for referring in tables
            .iter()
            .filter(|t| t.references.iter().any(|(_, to)| to == &table.name))
        {
            // The first base row of `table` is the one every referring
            // table's first base row refers to (`generated`).
            let referred = &base[&table.name][0];
            let delete = table.function("delete");
            let key: Vec<Json> = delete.by.iter().map(|f| referred[f].clone()).collect();
            refused_whole(
                s,
                tables,
                &format!(
                    "{} deleted while {} refers to it",
                    table.name, referring.name
                ),
                |ops| {
                    write(
                        ops,
                        &delete.import,
                        &arguments(table, delete, std::slice::from_ref(&key)),
                    )
                },
            );
            assert_eq!(
                published(s, table, referred),
                some_row(table, referred),
                "{}: still there, {} referring to it",
                table.name,
                referring.name
            );
            *refusals += 1;
        }
        // Refused, by its bound: filled to it in one command, and one more.
        if let Some(bound) = table.bounded {
            let before = held(s, table);
            let filling: Vec<Map<String, Json>> = (0..=bound.saturating_sub(before))
                .map(|k| fresh(tables, table, &base, 80_000 + k))
                .collect();
            refused_whole(
                s,
                tables,
                &format!("{} past its bound", table.name),
                |ops| {
                    for row in &filling {
                        write(ops, insert, &[wit_row(table, row)])?;
                    }
                    Ok(())
                },
            );
            assert_eq!(
                held(s, table),
                before,
                "{}: nothing past its bound",
                table.name
            );
            assert_eq!(published(s, table, &filling[0]), none, "{}", table.name);
            *refusals += 1;
        }
        if *refusals == 0 {
            eprintln!(
                "conformance: {} declares no reference and no bound, so no refusal is provoked",
                table.name
            );
        }
    }
    provoked
}

/// **A transaction begun before its statements are counted**: `table`'s
/// first read, of `row`, through `ops`, a command's or a render's, and its
/// statements not counted. An adapter that begins a transaction at its
/// first statement (PostgreSQL's `BEGIN` for a command, its read-only
/// snapshot for a render) spends that once a transaction, not once a read
/// or a write, as D3's own pins count it
/// (`runtime/pw-table-postgres/tests/primitives.rs`, "the transaction
/// begun"); the memory adapter's transaction takes no statement. Until
/// 2026-11-12 the suite counted it against the first read or write measured
/// (track `durable-tables`, ADR-0397): a command's first write was two
/// statements on PostgreSQL, and a render's first read passed only where
/// its source states more than one.
fn begun(ops: &data::Ops, table: &Table, row: &Map<String, Json>) {
    let first = table.functions.iter().find(|f| !f.writes());
    assert!(
        first.is_some(),
        "{} declares a read, which begins the transaction",
        table.name
    );
    let Some(read) = first else {
        unreachable!("asserted above")
    };
    let key: Vec<Json> = read.by.iter().map(|f| row[f].clone()).collect();
    call(ops, &read.import, &arguments(table, read, &[key]));
}

/// **Part 3, its writes**: in a command, each write function of every table
/// (`insert`, `put`, `delete`, `delete-prefix`) is one statement of the
/// layer's adapter, counted below the layer as a read's cost is, once the
/// command's transaction is [`begun`]; and a row referring to a row its own
/// command staged commits, with it, as the guestbook's first signing writes
/// its book and its entry in one command. The writes measured, and the
/// references to a staged row committed.
fn writes_and_staged_references(s: &Server, tables: &[Table]) -> (usize, usize) {
    let base = generated(tables, 3, 0);
    written(s, tables, &base);
    let [_, (writes, granted)] = suite_contracts(tables);
    let mut measured = 0;
    for table in tables {
        // Each write's statements, in a command dropped after: it keeps
        // nothing, so each is measured on the rows as committed.
        let staging = s.data.begin("conformance");
        let ops = staging
            .table_ops(&writes, &granted)
            .asserted("its functions");
        let row = fresh(tables, table, &base, 90_000);
        let there = &base[&table.name][0];
        begun(&ops, table, there);
        for function in table.functions.iter().filter(|f| f.writes()) {
            let args = match function.primitive.as_str() {
                "insert" | "put" => vec![wit_row(table, &row)],
                _ => {
                    let key: Vec<Json> = function.by.iter().map(|f| there[f].clone()).collect();
                    arguments(table, function, &[key])
                }
            };
            let before = statements(s);
            call(&ops, &function.import, &args);
            assert_eq!(
                statements(s) - before,
                1,
                "{}: a write is one statement of the adapter",
                function.import
            );
            measured += 1;
        }
        drop(ops);
        drop(staging);
    }
    // A row referring to a row its own command staged.
    let mut committed = 0;
    for table in tables {
        for (field, to) in &table.references {
            let target = tables.iter().find(|t| &t.name == to).expect("its target");
            let referred = fresh(tables, target, &base, 95_000);
            let mut referring = fresh(tables, table, &base, 95_000);
            referring.insert(field.clone(), referred[&target.key[0]].clone());
            let mut staging = s.data.begin("conformance");
            let ops = staging
                .table_ops(&writes, &granted)
                .asserted("its functions");
            call(
                &ops,
                &target.function("insert").import,
                &[wit_row(target, &referred)],
            );
            call(
                &ops,
                &table.function("insert").import,
                &[wit_row(table, &referring)],
            );
            drop(ops);
            let commit = s.commit_staged(&mut *staging, &[], &[], None, &[], None, None);
            assert!(
                matches!(commit, Ok(CommitOutcome::Applied(_))),
                "{} referring to a {to} its command staged commits: {:?}",
                table.name,
                commit.err()
            );
            drop(staging);
            assert_eq!(published(s, target, &referred), some_row(target, &referred));
            assert_eq!(
                published(s, table, &referring),
                some_row(table, &referring),
                "{} referring to the {to} staged with it",
                table.name
            );
            committed += 1;
        }
    }
    (measured, committed)
}

/// **The layer under test**, made afresh for a build: what serves its
/// declared tables, for each server the suite builds (ADR-0338's ruling 7:
/// one suite, every adapter run through it).
pub(super) type Layer<'a> = &'a dyn Fn(&std::path::Path) -> Arc<dyn DataLayer>;

/// **The memory adapter's layer** (`tables::TablesLayer`), for a build.
pub(super) fn memory(build: &std::path::Path) -> Arc<dyn DataLayer> {
    let contracts = std::fs::read_to_string(build.join("contracts.json"))
        .map_err(|e| e.to_string())
        .and_then(|text| ComponentContract::from_json(&text).map_err(|e| e.to_string()))
        .asserted("contracts.json");
    Arc::new(crate::tables::TablesLayer::from_build(build, &contracts).asserted("the memory layer"))
}

/// The build served by `layer`, afresh.
pub(super) fn served_by(build: &std::path::Path, layer: Layer) -> Server {
    Server::from_build_tables(build.to_path_buf(), build.to_path_buf(), layer(build))
        .asserted("served by the layer under test")
}

/// What [`conformance`] checked: the reads held to the oracle (outside a
/// render and in one), each table's refusals provoked, the writes measured
/// at the adapter, and the references to a staged row committed.
pub(super) struct Checked {
    pub reads: usize,
    pub refusals: BTreeMap<String, usize>,
    pub writes: usize,
    pub staged_references: usize,
}

/// **The suite, for a program's build and the layer under test, each part
/// on a server serving the build by a fresh layer**: parts 3 and 4 (the
/// writes' costs and a reference to a staged row among them), then parts 1
/// and 2 at two sizes.
pub(super) fn conformance(build: &std::path::Path, layer: Layer) -> Checked {
    let tables = tables_of(build);
    assert!(!tables.is_empty(), "the program declares tables");
    let sources: Json = serde_json::from_str(
        &std::fs::read_to_string(build.join("sources.json")).expect("sources.json"),
    )
    .expect("sources.json is JSON");
    let sets: BTreeMap<String, String> = sources
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or_default().to_string(),
                s["sets"].as_str().unwrap_or("per_key").to_string(),
            )
        })
        .collect();
    let refusals = staging_and_refusals(&served_by(build, layer), &tables);
    let (writes, staged_references) =
        writes_and_staged_references(&served_by(build, layer), &tables);
    let reads = answers_and_costs(&served_by(build, layer), &tables, &sets, 3)
        + answers_and_costs(&served_by(build, layer), &tables, &sets, 24);
    Checked {
        reads,
        refusals,
        writes,
        staged_references,
    }
}

/// A build of `sources`, and a way to serve it afresh, as long as the
/// directory is held.
fn build_of(sources: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(sources, &[])
}

/// **The suite holds for the guestbook's tables** (`Books`, `Entries`), on
/// the memory adapter: every table refused at least once.
#[test]
fn the_suite_holds_for_the_guestbooks_tables() {
    let (_dir, out) = build_of(&["examples/guestbook"]);
    let checked = conformance(&out, &memory);
    assert!(checked.reads > 0);
    assert_eq!(
        checked.refusals,
        BTreeMap::from([("Books".to_string(), 1), ("Entries".to_string(), 2)]),
        "Books deleted while an entry refers to it; Entries referring to no book, and past \
         its bound"
    );
    assert_eq!(
        (checked.writes, checked.staged_references),
        (7, 1),
        "Books' three writes and Entries' four, each measured; an entry referring to the book \
         its command staged"
    );
    eprintln!(
        "conformance: the guestbook's tables, {} reads checked, refusals {:?}, {} writes \
         measured, {} reference to a staged row",
        checked.reads, checked.refusals, checked.writes, checked.staged_references
    );
}

/// **The suite holds for the lists app's tables** (`Lists`, `Items`), on
/// the memory adapter: every table refused at least once.
#[test]
fn the_suite_holds_for_the_lists_apps_tables() {
    let mut sources = super::tables::TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_dir, out) = build_of(&sources);
    let checked = conformance(&out, &memory);
    assert!(checked.reads > 0);
    assert_eq!(
        checked.refusals,
        BTreeMap::from([("Items".to_string(), 1), ("Lists".to_string(), 2)]),
        "Items referring to no list; Lists deleted while an item refers to it, and past its \
         bound"
    );
    assert_eq!(
        (checked.writes, checked.staged_references),
        (7, 1),
        "each table's writes measured; an item referring to the list its command staged"
    );
    eprintln!(
        "conformance: the lists app's tables, {} reads checked, refusals {:?}, {} writes \
         measured, {} reference to a staged row",
        checked.reads, checked.refusals, checked.writes, checked.staged_references
    );
}

/// **Part 5, idempotency**: each command of the guestbook that declares
/// `idempotent_by`, sent again with its interaction, is answered as it was
/// and writes nothing more (the lists app declares none). Through the
/// server's command route, which answers a retry from the record its
/// commit kept with its rows; on the layer under test, memory's.
#[test]
fn each_idempotent_command_sent_again_is_answered_once() {
    let (_dir, out) = build_of(&["examples/guestbook"]);
    let s = served_by(&out, &memory);
    let idempotent: Vec<String> = s
        .contracts
        .iter()
        .filter(|c| {
            c.exports
                .first()
                .and_then(|e| e.component.as_ref())
                .is_some_and(|e| e.idempotent_by.is_some())
        })
        .map(|c| c.component_id.clone())
        .collect();
    assert_eq!(
        idempotent,
        ["guestbook.book.remove", "guestbook.book.sign"],
        "every idempotent command of the program is below"
    );
    s.identity.signed_in_for_test("a", "u-ada", "Ada");
    let [(reads, granted), _] = suite_contracts(&tables_of(&out));
    let entries = |s: &Server| {
        let ops = s
            .data
            .table_reads(&reads, &granted, None)
            .asserted("the entries' reads");
        call(
            &ops,
            "pw:tables/entries#of-book",
            &[Val::String("guestbook".into())],
        )
    };
    for (command, args) in [
        ("guestbook.book.sign", serde_json::json!(["Once"])),
        ("guestbook.book.remove", serde_json::json!([1])),
    ] {
        let args = args.as_array().expect("arguments").clone();
        let first = s
            .command_answer(command, "a", &args, Some("i-again"))
            .asserted("answered");
        assert!(first.committed, "{command}: {first:?}");
        let after_first = entries(&s);
        let again = s
            .command_answer(command, "a", &args, Some("i-again"))
            .asserted("answered");
        assert_eq!(again, first, "{command}: the retry is answered as it was");
        assert_eq!(
            entries(&s),
            after_first,
            "{command}: and writes nothing more"
        );
    }
}

/// The table calls `query`, given `args`, made through the server's own
/// path (`Server::answer`, its reads counted as `/metrics` counts them),
/// and the statements its layer's adapter ran for them.
fn table_calls(s: &Server, query: &str, args: &[Val]) -> (BTreeMap<String, u64>, u64) {
    let before = s.calls.lock().expect("calls").clone();
    let ran = statements(s);
    s.answer(query, "conformance", args).asserted(query);
    let ran = statements(s) - ran;
    let after = s.calls.lock().expect("calls").clone();
    let calls = after
        .into_iter()
        .filter(|(op, _)| pw_host::tables::is_table_import(op))
        .map(|(op, n)| {
            let was = before.get(&op).copied().unwrap_or(0);
            (op, n - was)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    (calls, ran)
}

/// **Each query of a program reads its tables a fixed number of times,
/// whatever they hold** (part 2, for the programs' own reads: 4b's pins,
/// counted at two sizes): its calls of each table function at 3 rows a
/// table and at 24 are the same, one each, a set form's one call for every
/// key it is given, so no query reads once a row; and the statements its
/// layer's adapter runs for them, counted below the layer, are the same at
/// both sizes, one a call on memory's. Each outside a render and in one, as
/// a page reads it (NEXT 4c: the render's snapshot), so a layer whose render
/// reads cost more is seen (the re-review of 2026-11-02).
#[test]
fn each_query_reads_its_tables_a_fixed_number_of_times_at_two_sizes() {
    let mut sources = super::tables::TABLES.to_vec();
    sources.push("examples/tables/app.pw");
    let (_lists_dir, lists) = build_of(&sources);
    let (_book_dir, book) = build_of(&["examples/guestbook"]);
    let text = |s: &str| Val::String(s.into());
    // The reader's handle, as the platform makes it for the session the
    // pins read as: its guest's, from the session's digest.
    let reader = crate::identity::guest_user("conformance");
    let pins: Vec<(&std::path::Path, &str, Vec<Val>, Vec<&str>)> = vec![
        (
            &lists,
            "tables.app.EveryList",
            vec![],
            vec!["pw:tables/lists#all"],
        ),
        (
            &lists,
            "tables.app.ListAt",
            vec![text("v0")],
            vec!["pw:tables/lists#by-slug"],
        ),
        (
            &lists,
            "tables.app.ItemsOf",
            vec![text("v0")],
            vec!["pw:tables/items#of-list"],
        ),
        (
            &lists,
            "tables.app.OpenItems",
            vec![],
            vec!["pw:tables/items#by-done"],
        ),
        (
            &lists,
            "tables.app.EveryItem",
            vec![],
            vec!["pw:tables/items#of-list-many", "pw:tables/lists#all"],
        ),
        (
            &book,
            "guestbook.book.Latest",
            vec![text(&reader), Val::S64(20)],
            vec!["pw:tables/entries#of-book"],
        ),
        (&book, "guestbook.book.Me", vec![text(&reader)], vec![]),
    ];
    for n in [3, 24] {
        for in_a_render in [false, true] {
            let way = if in_a_render {
                "in a render"
            } else {
                "outside a render"
            };
            for (build, query, args, reads) in &pins {
                let s = served_by(build, &memory);
                let tables = tables_of(build);
                written(&s, &tables, &generated(&tables, n, 0));
                let expected: BTreeMap<String, u64> =
                    reads.iter().map(|op| (op.to_string(), 1)).collect();
                // A page's render: its snapshot, which its queries read.
                let render =
                    in_a_render.then(|| crate::render_merge::begin(&*s.data, s.queries.epoch()));
                let (calls, ran) = table_calls(&s, query, args);
                drop(render);
                assert_eq!(calls, expected, "{query} at {n} rows a table, {way}");
                assert_eq!(
                    ran,
                    reads.len() as u64,
                    "{query} at {n} rows a table, {way}: one statement of the adapter a call"
                );
            }
        }
    }
}
