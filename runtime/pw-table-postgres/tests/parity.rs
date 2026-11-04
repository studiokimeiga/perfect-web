//! **The same tests on memory and on PostgreSQL give the same answers**
//! (ADR-0338 5.5, part 1: "every operation of the slice answers what the
//! in-memory layer answers, before and after the same writes").
//!
//! - **A workload, seeded and random**: commands of random writes and
//!   reads over every primitive of the things' program (whose fields are of
//!   every kind the adapters store), each committed or dropped at random;
//!   every write's and read's answer, every command's `written` and commit,
//!   and after each command every read of every primitive, compared.
//! - **The compiled programs** (`examples/tables`): `app.pw`'s queries and
//!   commands and `writer.pw`'s commands, run by the engine through the
//!   table gate on each adapter, as the platform runs them.
//!
//! The memory adapter is `pw-table-memory`, unchanged: this file adapts the
//! two to one shape ([`Adapter`]) and compares; it owns no rule.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use pw_host::engine::{HostFn, Val};
use pw_table_memory::MemoryTables;
use pw_table_postgres::PgTables;
use serde_json::{Value, json};

/// **An answer, as both adapters can give it**: a value, as its JSON text
/// (a float's sign and kind kept), or an error by its kind and words.
#[derive(Debug, Clone, PartialEq)]
enum Answer {
    Value(String),
    Input(String),
    Conflict(String, String),
    Schema(String),
    Ended,
    Other(String),
}

fn from_memory<T>(
    answer: Result<T, pw_table_memory::Error>,
    value: impl Fn(T) -> String,
) -> Answer {
    use pw_table_memory::Error as E;
    match answer {
        Ok(v) => Answer::Value(value(v)),
        Err(E::Input(why)) => Answer::Input(why),
        Err(E::Conflict { table, existing }) => Answer::Conflict(table, existing.to_string()),
        Err(E::Schema(why)) => Answer::Schema(why),
        Err(E::Ended) => Answer::Ended,
        Err(other) => Answer::Other(format!("{other:?}")),
    }
}

fn from_postgres<T>(
    answer: Result<T, pw_table_postgres::Error>,
    value: impl Fn(T) -> String,
) -> Answer {
    use pw_table_postgres::Error as E;
    match answer {
        Ok(v) => Answer::Value(value(v)),
        Err(E::Input(why)) => Answer::Input(why),
        Err(E::Conflict { table, existing }) => Answer::Conflict(table, existing.to_string()),
        Err(E::Schema(why)) => Answer::Schema(why),
        Err(E::Ended) => Answer::Ended,
        Err(other) => Answer::Other(format!("{other:?}")),
    }
}

fn text(v: Value) -> String {
    v.to_string()
}

/// xorshift64*: the workload's choices, the same on every run.
struct Choices(u64);

impl Choices {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len() as u64) as usize]
    }
}

const OWNERS_POOL: &[&str] = &["a", "b", "B", "é", "z", "a\u{0}", "nobody"];
const SLUGS: &[&str] = &["s0", "s1", "s2", "", "é漢", "toolong!!"];
const SCORES: &[f64] = &[0.0, -0.0, 1.5, 5e-324, -2.25];
/// A nested float's choices: each of the last three is read back from its
/// shortest digits one or two units in the last place off by serde_json's
/// parser without `float_roundtrip` (the review of 2026-11-02), which the
/// first two, like every score above, are not.
const POINTS: &[f64] = &[
    -0.0,
    0.5,
    1.5666666666666667,
    9.011111111111111,
    -9.643915712060552e-234,
];

fn random_thing(c: &mut Choices) -> Value {
    let owner = *c.pick(OWNERS_POOL);
    let n = c.below(5) as i64;
    let mut row = thing_row(
        owner,
        n,
        c.below(4) as i64 * 10,
        c.pick(SLUGS),
        c.below(2) == 0,
    );
    row["score"] = json!(*c.pick(SCORES));
    if c.below(3) == 0 {
        row["note"] = json!(format!("note\u{0}{n}"));
    }
    if c.below(3) == 0 {
        row["maybe"] = json!([n, -n]);
    }
    if c.below(4) == 0 {
        row["count"] = json!(c.below(3) as i64);
    }
    row["point"]["x"] = json!(*c.pick(POINTS));
    if c.below(9) == 0 {
        row["rank"] = json!(5000);
    }
    row
}

/// One random operation: (import, arguments).
fn random_write(c: &mut Choices) -> (String, Vec<Value>) {
    match c.below(9) {
        0 | 1 => (thing("insert"), vec![random_thing(c)]),
        2 | 3 => (thing("put"), vec![random_thing(c)]),
        4 => (
            thing("delete"),
            vec![json!(*c.pick(OWNERS_POOL)), json!(c.below(5) as i64)],
        ),
        5 => (thing("delete-of-owner"), vec![json!(*c.pick(OWNERS_POOL))]),
        6 => (owner("insert"), vec![owner_row(c.pick(OWNERS_POOL))]),
        7 => (
            owner("put"),
            vec![json!({"id": *c.pick(OWNERS_POOL), "name": format!("n{}", c.below(3))})],
        ),
        _ => (owner("delete"), vec![json!(*c.pick(OWNERS_POOL))]),
    }
}

/// Every read of every primitive the things' program has, over the pools.
fn every_read() -> Vec<(String, Vec<Value>)> {
    let mut reads = vec![(owner("all"), vec![])];
    let keys: Vec<Value> = OWNERS_POOL
        .iter()
        .flat_map(|o| (0..5).map(move |n| json!({"owner": o, "n": n})))
        .collect();
    reads.push((thing("get-many"), vec![Value::Array(keys)]));
    reads.push((owner("get-many"), vec![json!(OWNERS_POOL)]));
    reads.push((thing("of-owner-many"), vec![json!(OWNERS_POOL)]));
    reads.push((thing("by-flag-many"), vec![json!([true, false])]));
    reads.push((thing("by-slug-many"), vec![json!(SLUGS)]));
    for o in OWNERS_POOL {
        reads.push((owner("get"), vec![json!(o)]));
        reads.push((thing("of-owner"), vec![json!(o)]));
        reads.push((thing("get"), vec![json!(o), json!(1)]));
        reads.push((thing("by-owner-rank"), vec![json!(o), json!(10)]));
    }
    for flag in [true, false] {
        reads.push((thing("by-flag"), vec![json!(flag)]));
    }
    for slug in SLUGS {
        reads.push((thing("by-slug"), vec![json!(slug)]));
    }
    reads.push((
        thing("by-owner-rank-many"),
        vec![json!([{"owner": "a", "rank": 0}, {"owner": "b", "rank": 10}, {"owner": "a", "rank": 0}])],
    ));
    // Every corresponding count, over the identical valid/invalid selectors,
    // before/after each staged write, commit and rollback in the workload.
    let counts: Vec<_> = reads
        .iter()
        .filter_map(|(import, args)| {
            let (table, function) = import.split_once('#')?;
            let fields = match function {
                "all" => return None,
                "get" if table == "pw:tables/owners" => "by-id",
                "get-many" if table == "pw:tables/owners" => "by-id-many",
                "get" => "by-owner-n",
                "get-many" => "by-owner-n-many",
                "of-owner" => "by-owner",
                "of-owner-many" => "by-owner-many",
                other => other,
            };
            Some((format!("{table}#count-{fields}"), args.clone()))
        })
        .collect();
    let windows = counts
        .iter()
        .flat_map(|(import, args)| {
            ["first", "last"].map(|word| {
                let mut args = args.clone();
                args.push(json!(2));
                (import.replace("#count-", &format!("#{word}-")), args)
            })
        })
        .collect::<Vec<_>>();
    reads.extend(windows);
    reads.extend(counts);
    reads
}

/// **A seeded random workload, on both** (part 1's answers): 160 commands,
/// each of one to five writes and reads, committed or dropped.
#[test]
fn a_random_workload_answers_alike_on_memory_and_postgresql() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "workload");
    let manifest = things();
    let memory = MemoryTables::new(manifest.clone()).assert_ok("memory");
    let postgres = PgTables::open(&url, scratch.schema(), manifest).assert_ok("postgres");
    let reads = every_read();
    let mut c = Choices(0x9e37_79b9_7f4a_7c15);
    let mut compared = 0usize;
    let mut met: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let meet = |answer: &Answer, met: &mut std::collections::BTreeSet<&str>| match answer {
        Answer::Value(v) if v.starts_with("[{") => {
            met.insert("rows");
        }
        Answer::Conflict(..) => {
            met.insert("conflict");
        }
        Answer::Input(why) if why.contains("references a missing") => {
            met.insert("reference");
        }
        Answer::Input(_) => {
            met.insert("input");
        }
        Answer::Value(v) if v.contains(": 1") || v.contains(": 2") => {
            met.insert("committed");
        }
        _ => {}
    };
    for step in 0..160 {
        let mut on_memory = memory.begin().assert_ok("a memory command");
        let mut on_postgres = postgres.begin().assert_ok("a postgres command");
        for _ in 0..=c.below(5) {
            let (import, args) = if c.below(4) == 0 {
                let (import, args) = c.pick(&reads).clone();
                let m = from_memory(on_memory.read(&import, &args), text);
                let p = from_postgres(on_postgres.read(&import, &args), text);
                assert_eq!(m, p, "step {step}: {import} {args:?}, in the command");
                meet(&m, &mut met);
                compared += 1;
                continue;
            } else {
                random_write(&mut c)
            };
            let m = from_memory(on_memory.write(&import, &args), |()| String::new());
            let p = from_postgres(on_postgres.write(&import, &args), |()| String::new());
            assert_eq!(m, p, "step {step}: {import} {args:?}");
            meet(&m, &mut met);
            compared += 1;
        }
        let m = from_memory(on_memory.written(), |w| format!("{w:?}"));
        let p = from_postgres(on_postgres.written(), |w| format!("{w:?}"));
        assert_eq!(m, p, "step {step}: written");
        if c.below(4) != 0 {
            let m = from_memory(on_memory.commit(), |k| format!("{:?}", k.written));
            let p = from_postgres(on_postgres.commit(), |k| format!("{:?}", k.written));
            assert_eq!(m, p, "step {step}: the commit");
            meet(&m, &mut met);
        } else {
            drop(on_memory);
            drop(on_postgres);
        }
        for (import, args) in &reads {
            let m = from_memory(memory.read(import, args), text);
            let p = from_postgres(postgres.read(import, args), text);
            assert_eq!(m, p, "step {step}: {import} {args:?}, published");
            meet(&m, &mut met);
            compared += 1;
        }
    }
    // The workload reached every kind of answer: rows, refusals by a key
    // and by a unique value, values refused, references refused at a
    // commit, commits that wrote.
    assert!(compared > 6000, "{compared} answers compared");
    for kind in ["rows", "conflict", "input", "reference", "committed"] {
        assert!(met.contains(kind), "the workload met no {kind}: {met:?}");
    }
}

// --- the compiled programs ---------------------------------------------------

/// What the app's and the writers' node grants.
const NODE: &[&str] = &[
    "database.read<Lists>",
    "database.read<Items>",
    "database.write<Lists>",
    "database.write<Items>",
    "outbox.write",
];

struct Compiled {
    bytes: Vec<u8>,
    contract: pw_host::ComponentContract,
    imports: Vec<String>,
}

impl Compiled {
    fn granted(&self) -> pw_host::Granted {
        let grants: Vec<String> = NODE.iter().map(|g| format!("{g:?}")).collect();
        let topology = pw_host::Topology::from_json(&format!(
            r#"{{"nodes":[{{"name":"origin","world":"origin","grants":[{}]}}]}}"#,
            grants.join(",")
        ))
        .assert_ok("topology");
        let admission = pw_host::admit(&self.contract, &topology, "origin", &self.imports);
        assert!(admission.is_admitted(), "{admission:?}");
        pw_host::Granted::from(&admission, &Default::default()).assert_ok("granted")
    }

    fn export(&self) -> [String; 2] {
        let export = self.contract.exports[0]
            .component
            .as_ref()
            .assert_ok("a component path");
        [export.interface.clone(), export.function.clone()]
    }

    /// Its platform imports (the outbox's invalidations), each recording
    /// what it was handed.
    fn platform(&self) -> (BTreeMap<String, HostFn>, Arc<Mutex<Vec<String>>>) {
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let mut host = BTreeMap::new();
        for import in &self.contract.imports {
            if pw_host::tables::is_table_interface(&import.interface) {
                continue;
            }
            let (key, record) = (import.key(), Arc::clone(&calls));
            let named = key.clone();
            let callback: HostFn = Arc::new(move |args| {
                if let Ok(mut calls) = record.lock() {
                    calls.push(format!("{named} {args:?}"));
                }
                Ok(Vec::new())
            });
            host.insert(key, callback);
        }
        (host, calls)
    }

    fn call(
        &self,
        granted: &pw_host::Granted,
        host: &BTreeMap<String, HostFn>,
        tables: &pw_host::tables::Manifest,
        args: &[Val],
    ) -> Result<Vec<Val>, String> {
        let [interface, function] = self.export();
        pw_host::engine::call_with_tables(
            &self.bytes,
            &self.contract,
            granted,
            &pw_host::Limits::unbounded(),
            host,
            tables,
            &[interface.as_str(), function.as_str()],
            args,
        )
    }
}

/// `examples/tables`' program with `program`, compiled: tables.json and
/// each of `ids`.
fn compiled(
    program: &str,
    ids: &[&str],
) -> (pw_core::tables::TablesJson, BTreeMap<String, Compiled>) {
    let units = common::fixture_units(&["Lists.pw", "Items.pw", "ListData.pw", program]);
    let build = pw_core::build::build(&units).assert_ok("the program builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).assert_ok("contracts"),
    )
    .assert_ok("the host reads the contracts");
    let mut out = BTreeMap::new();
    for &id in ids {
        let artifact = build
            .components
            .iter()
            .find(|(component, _)| component == id)
            .map(|(_, artifact)| artifact);
        assert!(
            matches!(
                artifact,
                Some(pw_core::backend::component::Built::Component { .. })
            ),
            "{id} did not compile: {artifact:?}"
        );
        let Some(pw_core::backend::component::Built::Component { compiled, .. }) = artifact else {
            continue;
        };
        let bytes = compiled.component.bytes.clone();
        let imports = pw_host::engine::imports_of(&bytes).assert_ok("its imports");
        let contract = contracts
            .iter()
            .find(|c| c.component_id == id)
            .cloned()
            .assert_ok("its contract");
        out.insert(
            id.to_string(),
            Compiled {
                bytes,
                contract,
                imports,
            },
        );
    }
    (build.tables, out)
}

/// **One adapter, as the platform uses either**: a query's host functions,
/// and a command run in a transaction of its own, committed where it
/// answers and undone where it traps or answers a declared error.
trait Adapter {
    fn manifest(&self) -> &pw_host::tables::Manifest;
    fn query_fns(&self, c: &Compiled, granted: &pw_host::Granted) -> BTreeMap<String, HostFn>;
    fn command(
        &self,
        c: &Compiled,
        granted: &pw_host::Granted,
        args: &[Val],
    ) -> (Result<Vec<Val>, String>, Vec<String>, String);
}

fn declared_error(answer: &Result<Vec<Val>, String>) -> bool {
    matches!(answer.as_deref(), Ok([Val::Result(Err(_))]))
}

impl Adapter for MemoryTables {
    fn manifest(&self) -> &pw_host::tables::Manifest {
        self.host_tables()
    }
    fn query_fns(&self, c: &Compiled, granted: &pw_host::Granted) -> BTreeMap<String, HostFn> {
        self.read_host_fns(&c.contract, granted)
            .assert_ok("the gate admits its reads")
    }
    fn command(
        &self,
        c: &Compiled,
        granted: &pw_host::Granted,
        args: &[Val],
    ) -> (Result<Vec<Val>, String>, Vec<String>, String) {
        let staged = self.begin().assert_ok("its turn");
        let mut host = staged
            .host_fns(&c.contract, granted)
            .assert_ok("the gate admits it");
        let (platform, calls) = c.platform();
        host.extend(platform);
        let answer = c.call(granted, &host, self.host_tables(), args);
        let written = format!("{:?}", staged.written());
        let committed = if answer.is_ok() && !declared_error(&answer) {
            format!("{:?}", staged.commit().map(|c| c.written))
        } else {
            "not committed".to_string()
        };
        let calls = calls.lock().map(|c| c.clone()).unwrap_or_default();
        (answer, calls, format!("{written} {committed}"))
    }
}

impl Adapter for PgTables {
    fn manifest(&self) -> &pw_host::tables::Manifest {
        self.host_tables()
    }
    fn query_fns(&self, c: &Compiled, granted: &pw_host::Granted) -> BTreeMap<String, HostFn> {
        self.read_host_fns(&c.contract, granted)
            .assert_ok("the gate admits its reads")
    }
    fn command(
        &self,
        c: &Compiled,
        granted: &pw_host::Granted,
        args: &[Val],
    ) -> (Result<Vec<Val>, String>, Vec<String>, String) {
        let staged = self.begin().assert_ok("its turn");
        let mut host = staged
            .host_fns(&c.contract, granted)
            .assert_ok("the gate admits it");
        let (platform, calls) = c.platform();
        host.extend(platform);
        let answer = c.call(granted, &host, self.host_tables(), args);
        let written = format!("{:?}", staged.written());
        let committed = if answer.is_ok() && !declared_error(&answer) {
            format!("{:?}", staged.commit().map(|c| c.written))
        } else {
            "not committed".to_string()
        };
        let calls = calls.lock().map(|c| c.clone()).unwrap_or_default();
        (answer, calls, format!("{written} {committed}"))
    }
}

fn query(adapter: &dyn Adapter, c: &Compiled, args: &[Val]) -> Result<Vec<Val>, String> {
    let granted = c.granted();
    let host = adapter.query_fns(c, &granted);
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        c.imports
            .iter()
            .filter(|i| pw_host::tables::is_table_import(i))
            .collect::<Vec<_>>(),
        "exactly its own table imports"
    );
    c.call(&granted, &host, adapter.manifest(), args)
}

fn s(v: &str) -> Val {
    Val::String(v.into())
}

fn todo_list(id: &str, slug: &str, added: i64) -> Val {
    Val::Record(vec![
        ("id".into(), s(id)),
        ("name".into(), s(&format!("List {id}"))),
        ("slug".into(), s(slug)),
        ("added".into(), Val::S64(added)),
    ])
}

fn item(list: &str, id: &str, done: bool, added: i64, note: Option<&str>) -> Val {
    Val::Record(vec![
        ("list".into(), s(list)),
        ("id".into(), s(id)),
        ("title".into(), s(&format!("Item {id}"))),
        ("done".into(), Val::Bool(done)),
        ("count".into(), Val::S64(1)),
        ("added".into(), Val::S64(added)),
        ("note".into(), Val::Option(note.map(|n| Box::new(s(n))))),
    ])
}

const EVERY_LIST: &str = "tables.app.EveryList";
const LIST_AT: &str = "tables.app.ListAt";
const ITEMS_OF: &str = "tables.app.ItemsOf";
const OPEN_ITEMS: &str = "tables.app.OpenItems";
const EVERY_ITEM: &str = "tables.app.EveryItem";
const MAKE_LIST: &str = "tables.app.make_list";
const KEEP_ITEM: &str = "tables.app.keep_item";
const DROP_ITEM: &str = "tables.app.drop_item";
const CLEAR_LIST: &str = "tables.app.clear_list";

/// **`app.pw`, compiled, on both adapters** (ADR-0386's acceptance program):
/// its five queries through every read primitive, its four commands through
/// every write primitive, answered alike, and what each command handed the
/// platform alike.
#[test]
fn the_compiled_app_answers_alike_on_memory_and_postgresql() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "app");
    let (manifest, components) = compiled(
        "app.pw",
        &[
            EVERY_LIST, LIST_AT, ITEMS_OF, OPEN_ITEMS, EVERY_ITEM, MAKE_LIST, KEEP_ITEM, DROP_ITEM,
            CLEAR_LIST,
        ],
    );
    let memory = MemoryTables::new(manifest.clone()).assert_ok("memory");
    let postgres = PgTables::open(&url, scratch.schema(), manifest).assert_ok("postgres");
    let adapters: [&dyn Adapter; 2] = [&memory, &postgres];
    let c = |id: &str| &components[id];
    let commands: Vec<(&str, Vec<Val>)> = vec![
        (MAKE_LIST, vec![todo_list("groceries", "food", 20)]),
        (MAKE_LIST, vec![todo_list("chores", "home", 10)]),
        (MAKE_LIST, vec![todo_list("other", "food", 5)]),
        (MAKE_LIST, vec![todo_list("groceries", "else", 1)]),
        (KEEP_ITEM, vec![item("groceries", "milk", false, 3, None)]),
        (
            KEEP_ITEM,
            vec![item("groceries", "eggs", true, 1, Some("brown\u{0}"))],
        ),
        (KEEP_ITEM, vec![item("chores", "sweep", false, 2, None)]),
        (KEEP_ITEM, vec![item("chores", "dust", false, 5, None)]),
        (KEEP_ITEM, vec![item("groceries", "milk", true, 3, None)]),
        (KEEP_ITEM, vec![item("nowhere", "x", false, 1, None)]),
        (DROP_ITEM, vec![s("chores"), s("dust")]),
        (DROP_ITEM, vec![s("chores"), s("none")]),
        (CLEAR_LIST, vec![s("groceries")]),
        (KEEP_ITEM, vec![item("groceries", "bread", false, 9, None)]),
    ];
    let queries = |adapter: &dyn Adapter| -> Vec<Result<Vec<Val>, String>> {
        vec![
            query(adapter, c(EVERY_LIST), &[]),
            query(adapter, c(LIST_AT), &[s("food")]),
            query(adapter, c(LIST_AT), &[s("none")]),
            query(adapter, c(ITEMS_OF), &[s("groceries")]),
            query(adapter, c(ITEMS_OF), &[s("chores")]),
            query(adapter, c(OPEN_ITEMS), &[]),
            query(adapter, c(EVERY_ITEM), &[]),
        ]
    };
    for (id, args) in &commands {
        let ran: Vec<_> = adapters
            .iter()
            .map(|a| a.command(c(id), &c(id).granted(), args))
            .collect();
        // The answer, what it handed the platform, and what it wrote.
        let on_memory = (&ran[0].0, &ran[0].1, &ran[0].2);
        let on_postgres = (&ran[1].0, &ran[1].1, &ran[1].2);
        assert_eq!(on_memory, on_postgres, "{id} {args:?}");
        assert_eq!(queries(&memory), queries(&postgres), "after {id} {args:?}");
    }
    // The program's commands reached their every kind of answer.
    let lists = queries(&postgres)[0].clone().assert_ok("every list");
    assert_eq!(
        format!("{lists:?}").matches("List ").count(),
        2,
        "{lists:?}"
    );
}

const INSERT_THEN_GET: &str = "tables.writer.insert_then_get";
const PUT_THEN_OPEN: &str = "tables.writer.put_then_open";
const INSERT_THEN_DIVIDE: &str = "tables.writer.insert_then_divide";
const RENAME: &str = "tables.writer.rename";

/// **`writer.pw`, compiled, on both**: a command reading its own write
/// before it commits, one reading an index its put kept, a put refused by
/// the row holding its slug, and one that traps after its write, keeping
/// nothing.
#[test]
fn the_compiled_writers_answer_alike_on_memory_and_postgresql() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "writer");
    let (manifest, components) = compiled(
        "writer.pw",
        &[INSERT_THEN_GET, PUT_THEN_OPEN, INSERT_THEN_DIVIDE, RENAME],
    );
    let memory = MemoryTables::new(manifest.clone()).assert_ok("memory");
    let postgres = PgTables::open(&url, scratch.schema(), manifest).assert_ok("postgres");
    let adapters: [&dyn Adapter; 2] = [&memory, &postgres];
    let c = |id: &str| &components[id];
    let commands: Vec<(&str, Vec<Val>)> = vec![
        (INSERT_THEN_GET, vec![todo_list("a", "one", 1)]),
        (INSERT_THEN_GET, vec![todo_list("a", "two", 2)]),
        (INSERT_THEN_GET, vec![todo_list("b", "two", 3)]),
        (PUT_THEN_OPEN, vec![item("a", "x", false, 4, None)]),
        (PUT_THEN_OPEN, vec![item("b", "y", true, 5, Some("z"))]),
        (RENAME, vec![todo_list("a", "two", 1)]),
        (RENAME, vec![todo_list("a", "uno", 1)]),
        (
            INSERT_THEN_DIVIDE,
            vec![todo_list("c", "three", 6), Val::S64(0)],
        ),
        (
            INSERT_THEN_DIVIDE,
            vec![todo_list("c", "three", 6), Val::S64(4)],
        ),
        (INSERT_THEN_GET, vec![todo_list("c", "four", 7)]),
    ];
    for (id, args) in &commands {
        let ran: Vec<_> = adapters
            .iter()
            .map(|a| a.command(c(id), &c(id).granted(), args))
            .collect();
        let shape = |r: &(Result<Vec<Val>, String>, Vec<String>, String)| {
            // A trap's words are the engine's, which name no adapter; that
            // it trapped is the answer.
            (
                r.0.as_ref().map_err(|_| "trapped").cloned(),
                r.1.clone(),
                r.2.clone(),
            )
        };
        assert_eq!(shape(&ran[0]), shape(&ran[1]), "{id} {args:?}");
    }
    let kept_memory: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|id| {
            format!(
                "{:?}",
                memory
                    .read("pw:tables/lists#get", &[json!(id)])
                    .map_err(|e| format!("{e:?}"))
            )
        })
        .collect();
    let kept_postgres: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|id| {
            format!(
                "{:?}",
                postgres
                    .read("pw:tables/lists#get", &[json!(id)])
                    .map_err(|e| format!("{e:?}"))
            )
        })
        .collect();
    assert_eq!(kept_memory, kept_postgres);
}

/// **An ended command's functions act on nothing**, on either adapter: its
/// write through a host function after the command prepared, its write and
/// its read after it committed, each fail, and none is kept.
#[test]
fn an_ended_commands_functions_act_on_nothing_on_either_adapter() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "ended");
    let (manifest, components) = compiled("writer.pw", &[INSERT_THEN_GET]);
    let memory = MemoryTables::new(manifest.clone()).assert_ok("memory");
    let postgres = PgTables::open(&url, scratch.schema(), manifest).assert_ok("postgres");
    let c = &components[INSERT_THEN_GET];
    let granted = c.granted();
    let (insert, get) = ("pw:tables/lists#insert", "pw:tables/lists#get");
    let call = |host: &BTreeMap<String, HostFn>, import: &str, args: &[Val]| {
        host.get(import).map(|f| f(args).is_err()).unwrap_or(false)
    };
    let on_memory = {
        let staged = memory.begin().assert_ok("a command");
        let host = staged
            .host_fns(&c.contract, &granted)
            .assert_ok("its functions");
        let prepared = staged.prepare().assert_ok("prepared");
        let after_prepare = call(&host, insert, &[todo_list("x", "x", 1)]);
        prepared.publish().assert_ok("published");
        let after_commit = call(&host, insert, &[todo_list("y", "y", 1)]);
        let read_after = call(&host, get, &[s("x")]);
        (after_prepare, after_commit, read_after)
    };
    let on_postgres = {
        let staged = postgres.begin().assert_ok("a command");
        let host = staged
            .host_fns(&c.contract, &granted)
            .assert_ok("its functions");
        let prepared = staged.prepare().assert_ok("prepared");
        let after_prepare = call(&host, insert, &[todo_list("x", "x", 1)]);
        prepared.publish().assert_ok("published");
        let after_commit = call(&host, insert, &[todo_list("y", "y", 1)]);
        let read_after = call(&host, get, &[s("x")]);
        (after_prepare, after_commit, read_after)
    };
    assert_eq!(on_memory, (true, true, true));
    assert_eq!(on_postgres, (true, true, true));
    for id in ["x", "y"] {
        assert_eq!(
            postgres.read(get, &[json!(id)]),
            Ok(Value::Null),
            "nothing of an ended command kept"
        );
        assert_eq!(memory.read(get, &[json!(id)]), Ok(Value::Null));
    }
}
