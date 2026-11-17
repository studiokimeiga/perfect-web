//! **A call's arguments past the flat limit reach the host in memory**
//! (ADR-0266, amended 2026-11-17). The Canonical ABI passes a call's
//! arguments to a host's operation as flat core values up to
//! `MAX_FLAT_PARAMS`, 16, and past it the caller stores them in its own
//! memory, a tuple of them laid out as a record is, and passes one pointer,
//! aligned as the tuple is. Until the amendment the backend refused such a
//! call at build, so no compiled command could `put` a row wider than 16
//! flat values: a row of an id and two `Option<Ref>` is 17 (ADR-0440's asset
//! contract suite). Each test states one case, through the host, which
//! records what each operation received, with its control.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pw_conformance::{Calls, Runnable, compile, operation, units};
use pw_host::engine::{HostFn, Val};

const FIELDS: [&str; 17] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
];

fn typed(names: &[&str]) -> String {
    names
        .iter()
        .map(|f| format!("{f}: Int"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn listed(names: &[&str]) -> String {
    names.join(", ")
}

/// The program: three operations of the host past the limit (seventeen
/// `Int`s; a `Bool`, a record of 17 and a `String`; the record and a
/// `String`, answered with a `String`, its result through a pointer too),
/// one within it, and the queries that call them, with their arguments
/// held flat (`Spread`, `Built`: computed in the body) or in memory
/// (`Seventeen`, `Forwarded`, `Named`, whose own parameters arrive past the
/// limit).
fn program() -> String {
    let all = typed(&FIELDS);
    let sixteen = typed(&FIELDS[..16]);
    let built = FIELDS
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{f}: at + {i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let spread = (0..17)
        .map(|i| format!("at + {i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "module wa\n\ntype Wide = Wide {{ {all} }}\n\n\
         fn seventeen({all}) -> Int !{{ database.read<Wide> }}\n    host \"wa:data/wide#seventeen\"\n\n\
         fn sixteen({sixteen}) -> Int !{{ database.read<Wide> }}\n    host \"wa:data/wide#sixteen\"\n\n\
         fn placed(flag: Bool, w: Wide, name: String) -> Int !{{ database.read<Wide> }}\n    host \"wa:data/wide#placed\"\n\n\
         fn named(w: Wide, name: String) -> String !{{ database.read<Wide> }}\n    host \"wa:data/wide#named\"\n\n\
         public query Seventeen({all}) -> Int {{\n    seventeen({})\n}}\n\n\
         public query Sixteen({sixteen}) -> Int {{\n    sixteen({})\n}}\n\n\
         public query Spread(at: Int) -> Int {{\n    seventeen({spread})\n}}\n\n\
         public query Forwarded(flag: Bool, w: Wide, name: String) -> Int {{\n    placed(flag, w, name)\n}}\n\n\
         public query Built(flag: Bool, name: String, at: Int) -> Int {{\n    placed(flag, Wide {{ {built} }}, name)\n}}\n\n\
         public query Named(w: Wide, name: String) -> String {{\n    named(w, name)\n}}\n",
        listed(&FIELDS),
        listed(&FIELDS[..16]),
    )
}

fn compiled(id: &str) -> Runnable {
    Runnable::new(compile(&units(&[("wa.pw", &program())]), id))
}

/// The host: each operation records what it received, and answers how many
/// arguments it was given (a `String` for `named`: the name it was given,
/// marked).
fn host(calls: &Calls) -> BTreeMap<String, HostFn> {
    let mut ops: BTreeMap<String, HostFn> = BTreeMap::new();
    for op in ["seventeen", "sixteen", "placed"] {
        let (name, f) = operation(calls, &format!("wa:data/wide#{op}"), |args| {
            Val::S64(args.len() as i64)
        });
        ops.insert(name, f);
    }
    let (name, f) = operation(calls, "wa:data/wide#named", |args| match args.last() {
        Some(Val::String(name)) => Val::String(format!("named {name}")),
        other => Val::String(format!("no name: {other:?}")),
    });
    ops.insert(name, f);
    ops
}

/// Each value distinct, so a value read from another position's place is
/// another value.
fn values(n: usize, from: i64) -> Vec<i64> {
    (0..n as i64).map(|i| from + 7 * i).collect()
}

fn wide(v: &[i64]) -> Val {
    Val::Record(
        FIELDS
            .iter()
            .zip(v)
            .map(|(f, x)| (f.to_string(), Val::S64(*x)))
            .collect(),
    )
}

/// What `export` answered and what the host received, by `args`.
fn called(export: &str, args: &[Val]) -> (Result<Vec<Val>, String>, Vec<(String, Vec<Val>)>) {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let out = compiled(export).call(&host(&calls), args);
    let received = calls.lock().expect("calls").clone();
    (out, received)
}

fn ints(v: &[i64]) -> Vec<Val> {
    v.iter().map(|x| Val::S64(*x)).collect()
}

#[test]
fn seventeen_arguments_reach_the_host_each_in_its_place() {
    let v = values(17, 1000);
    let (out, received) = called("wa.Seventeen", &ints(&v));
    assert_eq!(out, Ok(vec![Val::S64(17)]));
    assert_eq!(received, [("wa:data/wide#seventeen".to_string(), ints(&v))]);
    // The same seventeen computed in the body, each held flat, each stored
    // at its own offset.
    let (out, received) = called("wa.Spread", &[Val::S64(100)]);
    assert_eq!(out, Ok(vec![Val::S64(17)]));
    let spread: Vec<i64> = (100..117).collect();
    assert_eq!(
        received,
        [("wa:data/wide#seventeen".to_string(), ints(&spread))]
    );
    // The control: sixteen are passed flat, as they were.
    let v = values(16, 2000);
    let (out, received) = called("wa.Sixteen", &ints(&v));
    assert_eq!(out, Ok(vec![Val::S64(16)]));
    assert_eq!(received, [("wa:data/wide#sixteen".to_string(), ints(&v))]);
}

/// A `Bool`, then the record at the next multiple of 8, its alignment, and
/// a `String` past its 136 bytes; each held flat but the record, which the
/// body builds in memory.
#[test]
fn a_record_built_in_the_body_is_passed_aligned_after_what_comes_before_it() {
    for flag in [true, false] {
        let (out, received) = called(
            "wa.Built",
            &[Val::Bool(flag), Val::String("built".into()), Val::S64(500)],
        );
        assert_eq!(out, Ok(vec![Val::S64(3)]));
        let expected: Vec<i64> = (0..17).map(|i| 500 + i).collect();
        assert_eq!(
            received,
            [(
                "wa:data/wide#placed".to_string(),
                vec![
                    Val::Bool(flag),
                    wide(&expected),
                    Val::String("built".into())
                ]
            )]
        );
    }
}

/// The same call, its arguments held in memory where the export's own
/// arrived past the limit, each copied to its place. The name is of an odd
/// length, so the host's copy of it leaves the region's top unaligned: the
/// tuple is allocated at its own alignment all the same, or the host refuses
/// its pointer.
#[test]
fn arguments_held_in_memory_are_copied_each_to_its_place() {
    let v = values(17, 3000);
    for flag in [true, false] {
        let name = Val::String("after the record.".into());
        let (out, received) = called("wa.Forwarded", &[Val::Bool(flag), wide(&v), name.clone()]);
        assert_eq!(out, Ok(vec![Val::S64(3)]));
        assert_eq!(
            received,
            [(
                "wa:data/wide#placed".to_string(),
                vec![Val::Bool(flag), wide(&v), name]
            )]
        );
    }
}

/// Past the limit both ways: the arguments at one pointer, and the
/// operation's `String` answered through another, after it.
#[test]
fn a_call_past_the_limit_takes_its_answer_through_a_pointer_too() {
    let v = values(17, 4000);
    let name = Val::String("odd".into());
    let (out, received) = called("wa.Named", &[wide(&v), name.clone()]);
    assert_eq!(out, Ok(vec![Val::String("named odd".into())]));
    assert_eq!(
        received,
        [("wa:data/wide#named".to_string(), vec![wide(&v), name])]
    );
}
