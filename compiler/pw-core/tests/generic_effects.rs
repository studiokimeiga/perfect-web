//! **A member read is a call wherever its receiver comes from** (ADR-0351,
//! the generic-effects track).
//!
//! A member read, `item.label` where `label` is a function of `Item`'s, is a
//! call (ADR-0048), and the effect inference charges what it performs by the
//! receiver's TYPE (ADR-0022). On 2026-10-16 every one of these checked clean
//! at master 22a0508e, each a body declared to perform nothing reading the
//! database through such a member:
//! - a receiver a GENERIC call computes: `let all = List.reverse(given)` then
//!   `for x in all { x.label }`, in a function and in a view's loop, through a
//!   user-written `fn same<T>(x: T) -> T`, a nested generic, a pipe, a lambda
//!   over a computed list and a match arm. `infer.rs` kept the call's declared
//!   result, `List<T>`, with the callee's `T` unsolved, and a member of `T` is
//!   none;
//! - and, not generic, a `for` loop's binder and an expression `match` arm's,
//!   which `infer.rs` typed not at all; a receiver that is not a bare name,
//!   `o.item.label`, which the member walk skipped; a generic record's field,
//!   `b.v.label`, typed as its declared `T`; a member with no declared row,
//!   charged its row (nothing) rather than what its body performs; and a
//!   member read on a value of a rigid type parameter, which the value
//!   relations left undecided.
//!
//! The same receivers carried a secret into markup unseen (PW5003), since a
//! member's label is read through the same types, and left a query's
//! contract without the capability its body needs.
//!
//! Each test states one case with its control: the same program reading a
//! record field, which performs nothing.

use pw_core::check::check_sources;

fn program(src: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.push(("t.pw".to_string(), src.to_string()));
    out
}

/// Each diagnostic on `t.pw`, as its code and the text its primary span
/// covers: where it is reported is part of what is claimed.
fn reported(src: &str) -> Vec<(String, String)> {
    check_sources(&program(src))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| {
            ds.into_iter().map(|d| {
                let at = src.get(d.primary_span.clone()).unwrap_or("?").to_string();
                (d.code.to_string(), at)
            })
        })
        .collect()
}

fn one(code: &str, at: &str) -> Vec<(String, String)> {
    vec![(code.to_string(), at.to_string())]
}

const NOTHING: Vec<(String, String)> = Vec::new();

/// A module with an `Item` whose member `label` reads the database, and
/// `rest` after it.
fn reading(rest: &str) -> String {
    format!(
        "module t\n\n\
         import List\n\n\
         type Item = Item {{ name: String }}\n\n\
         fn label(i: Item) -> String !{{ database.read<Item> }}\n    host \"shop:data/items#label\"\n\n\
         {rest}\n"
    )
}

/// `src` refused once with `code` at `at`, and its control, the same
/// program reading the record's field `name` instead, accepted.
fn refused_with_control(src: &str, code: &str, at: &str) {
    assert_eq!(reported(src), one(code, at), "{src}");
    let control = src.replace(at, &at.replace(".label", ".name"));
    assert_ne!(control, src, "the control reads a field");
    assert_eq!(reported(&control), NOTHING, "{control}");
}

/// These functions intentionally read the database per row as well as
/// omitting its effect row. Both independent protections must be present.
fn refused_with_bounded_control(src: &str, at: &str) {
    let mut got = reported(src);
    got.sort();
    assert_eq!(
        got,
        [one("PW0400", at), one("PW0405", at)].concat(),
        "{src}"
    );
    let control = src.replace(at, &at.replace(".label", ".name"));
    assert_ne!(control, src);
    assert_eq!(reported(&control), NOTHING, "{control}");
}

#[test]
fn a_member_read_through_a_generic_calls_result_in_a_function_is_seen() {
    let src = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    let all = List.reverse(given)\n    for x in all {\n        return x.label\n    }\n    \"none\"\n}",
    );
    refused_with_bounded_control(&src, "x.label");
}

#[test]
fn a_member_read_through_a_generic_calls_result_in_a_view_is_refused() {
    let src = reading(
        "view Menu(given: List<Item>) !{} {\n    let all = List.reverse(given)\n    <ul>\n        {#each all as item (item.name)}\n            <li>{item.label}</li>\n        {/each}\n    </ul>\n}",
    );
    refused_with_control(&src, "PW0401", "item.label");
}

#[test]
fn a_member_read_through_a_user_written_generic_function_is_seen() {
    let same = "fn same<T>(x: T) -> T !{} {\n    x\n}\n\n";
    let through_let = reading(&format!(
        "{same}fn first_label(given: Item) -> String !{{}} {{\n    let y = same(given)\n    y.label\n}}"
    ));
    refused_with_control(&through_let, "PW0400", "y.label");
    // Read on the call itself: charged until now by the member's SPELLING
    // (`value_effects` read `same(given).label` as a declaration `label`
    // named as a value), and now by the receiver's type.
    let direct = reading(&format!(
        "{same}fn first_label(given: Item) -> String !{{}} {{\n    same(given).label\n}}"
    ));
    refused_with_control(&direct, "PW0400", "same(given).label");
}

#[test]
fn a_member_read_through_a_nested_generic_is_seen() {
    let nested = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    let all = List.reverse(List.take(given, 2))\n    for x in all {\n        return x.label\n    }\n    \"none\"\n}",
    );
    refused_with_bounded_control(&nested, "x.label");
    // A lambda over a list a call computes: its parameter is typed by the
    // solved call, as one over a named list is.
    let lambda = reading(
        "fn labels(given: List<Item>) -> List<String> !{} {\n    List.map(List.reverse(given), i => i.label)\n}",
    );
    refused_with_bounded_control(&lambda, "i.label");
    // Piped, and read as a member.
    let piped = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    let all = given |> List.reverse()\n    for x in all {\n        return x.label\n    }\n    \"none\"\n}",
    );
    refused_with_bounded_control(&piped, "x.label");
    let member = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    let all = given.reverse\n    for x in all {\n        return x.label\n    }\n    \"none\"\n}",
    );
    refused_with_bounded_control(&member, "x.label");
}

#[test]
fn a_member_read_in_a_match_arm_over_a_generic_result_is_seen() {
    let expression = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    match List.get(given, 0) {\n        Some(x) => x.label,\n        None => \"none\",\n    }\n}",
    );
    refused_with_control(&expression, "PW0400", "x.label");
    let template = reading(
        "view Menu(given: List<Item>) !{} {\n    <div>\n        {#match List.get(given, 0)}\n            {:Some(x)}\n                <p>{x.label}</p>\n            {:None}\n                <p>none</p>\n        {/match}\n    </div>\n}",
    );
    refused_with_control(&template, "PW0401", "x.label");
}

#[test]
fn a_member_read_on_a_loops_or_an_arms_binder_is_seen() {
    // Not generic: `infer.rs` typed no `for` binder and no expression match
    // arm's.
    let for_loop = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    for x in given {\n        return x.label\n    }\n    \"none\"\n}",
    );
    refused_with_bounded_control(&for_loop, "x.label");
    let arm = reading(
        "fn first_label(given: Option<Item>) -> String !{} {\n    match given {\n        Some(x) => x.label,\n        None => \"none\",\n    }\n}",
    );
    refused_with_control(&arm, "PW0400", "x.label");
    // A loop over a name a `let` aliases, typed before the `let` was.
    let alias = reading(
        "view Menu(given: List<Item>) !{} {\n    let all = given\n    <ul>\n        {#each all as item (item.name)}\n            <li>{item.label}</li>\n        {/each}\n    </ul>\n}",
    );
    refused_with_control(&alias, "PW0401", "item.label");
}

#[test]
fn a_member_read_on_any_receiver_is_seen() {
    // A field's value, not a bare name: the member walk read only a name.
    let chain = reading(
        "type Order = Order { item: Item }\n\nfn first_label(o: Order) -> String !{} {\n    o.item.label\n}",
    );
    refused_with_control(&chain, "PW0400", "o.item.label");
    // A generic record's field is its declared `T` under the receiver's
    // arguments, not `T`.
    let boxed = reading(
        "type Box<T> = Box { v: T }\n\nfn first_label(b: Box<Item>) -> String !{} {\n    b.v.label\n}",
    );
    refused_with_control(&boxed, "PW0400", "b.v.label");
    let nested = reading(
        "type Box<T> = Box { v: Option<T> }\n\nfn first(b: Box<Item>) -> String !{} {\n    match b.v {\n        Some(x) => x.label,\n        None => \"none\",\n    }\n}",
    );
    refused_with_control(&nested, "PW0400", "x.label");
}

/// A module whose member `label` declares no row, and performs a read.
fn open_row(rest: &str) -> String {
    format!(
        "module t\n\n\
         type Item = Item {{ name: String }}\n\n\
         fn read_label(i: Item) -> String !{{ database.read<Item> }}\n    host \"shop:data/items#label\"\n\n\
         fn label(i: Item) -> String {{\n    read_label(i)\n}}\n\n\
         {rest}\n"
    )
}

#[test]
fn a_member_with_no_row_is_charged_what_its_body_performs() {
    let function = open_row("fn first_label(given: Item) -> String !{} {\n    given.label\n}");
    refused_with_control(&function, "PW0400", "given.label");
    let view = open_row("view Show(given: Item) !{} {\n    <p>{given.label}</p>\n}");
    refused_with_control(&view, "PW0401", "given.label");
}

#[test]
fn a_member_read_on_a_rigid_type_parameter_is_refused() {
    // `T` has no members: whatever it is instantiated with, the body does not
    // say which `label` it reads, nor what that performs.
    let src = reading("fn first_label<T>(x: T) -> String !{} {\n    x.label\n}");
    assert_eq!(reported(&src), one("PW0610", "x.label"));
    // The control: a value of `T` handed on whole.
    let control = reading("fn same<T>(x: T) -> T !{} {\n    x\n}");
    assert_eq!(reported(&control), NOTHING);
}

#[test]
fn a_member_read_whose_receivers_type_nothing_states_is_refused() {
    // A lambda's parameter that no call types: which `label` this is, and
    // what it performs, cannot be read, so the row cannot be checked over it.
    let src = reading(
        "fn first_label(given: List<Item>) -> String !{} {\n    let f = (x) => x.label\n    \"none\"\n}",
    );
    assert_eq!(reported(&src), one("PW0408", "x.label"));
    // Its type written: the member is read, and what it performs is the
    // row's to declare.
    let typed = src.replace("(x) =>", "(x: Item) =>");
    assert_eq!(reported(&typed), one("PW0400", "x.label"));
    // The control: a field of the typed value, which performs nothing.
    assert_eq!(reported(&typed.replace("x.label", "x.name")), NOTHING);
    // No argument can solve make's T: it must never escape as the callee's T.
    let unsolved = reading(
        "fn make<T>() -> T !{}\n    host \"shop:data/items#make\"\n\nfn first() -> String !{} {\n    let item = make()\n    item.label\n}",
    );
    assert_eq!(reported(&unsolved), one("PW0408", "item.label"));
    let annotated = unsolved.replace("let item = make()", "let item: Item = make()");
    assert_eq!(reported(&annotated), one("PW0400", "item.label"));
    assert_eq!(
        reported(&annotated.replace("item.label", "item.name")),
        NOTHING
    );
}

/// A module whose `Item` has a member `token` that is a secret, and a view
/// `Menu` over `given` with `{markup}` as its body.
fn secret(markup: &str) -> String {
    format!(
        "module t\n\n\
         import List\n\
         import capability.{{ Secret, Payments }}\n\n\
         type Item = Item {{ name: String }}\n\n\
         fn token(i: Item) -> Secret<Payments> !{{}}\n    host \"shop:data/items#token\"\n\n\
         view Menu(given: List<Item>) !{{}} {{\n{markup}\n}}\n"
    )
}

#[test]
fn a_secret_read_through_a_generic_result_is_refused_in_markup() {
    for list in ["List.reverse(given)", "given"] {
        let src = secret(&format!(
            "    let all = {list}\n    <ul>\n        {{#each all as item (item.name)}}\n            <li>{{item.token}}</li>\n        {{/each}}\n    </ul>"
        ));
        assert_eq!(reported(&src), one("PW5003", "item.token"), "{src}");
        let control = src.replace("{item.token}", "{item.name}");
        assert_eq!(reported(&control), NOTHING, "{control}");
    }
    let arm = secret(
        "    <div>\n        {#match List.get(given, 0)}\n            {:Some(x)}\n                <p>{x.token}</p>\n            {:None}\n                <p>none</p>\n        {/match}\n    </div>",
    );
    assert_eq!(reported(&arm), one("PW5003", "x.token"));
    assert_eq!(reported(&arm.replace("{x.token}", "{x.name}")), NOTHING);
}

/// The capabilities `component` requires, its contract's.
fn required(src: &str, component: &str) -> Vec<String> {
    use pw_core::contract::{Capability, contracts};
    let files = program(src);
    let hirs: Vec<pw_core::hir::Hir> = files
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &refs);
    contracts(&refs, &sigs, &ws)
        .into_iter()
        .find(|c| c.component_id == component)
        .unwrap_or_else(|| panic!("no contract for {component}"))
        .required_capabilities
        .iter()
        .map(Capability::name)
        .collect()
}

#[test]
fn a_querys_contract_requires_what_a_member_of_a_generic_result_reads() {
    let src = reading(
        "query Labels(given: List<Item>) -> List<String> {\n    List.map(List.reverse(given), i => i.label)\n}",
    );
    assert_eq!(required(&src, "t.Labels"), ["database.read<Item>"]);
    assert!(required(&src.replace("i.label", "i.name"), "t.Labels").is_empty());
}

// --- every generic function of the standard packages ---------------------------

use pw_core::resolve::DefId;
use pw_core::resolved::{Builtin, ResolvedType};
use pw_core::signatures::{Signature, Signatures};
use std::collections::BTreeSet;

/// The standard packages, `packages/pw-std` and `packages/pw-platform-web`.
fn packages() -> Vec<(String, String)> {
    let mut all = program("");
    all.pop();
    all
}

/// `t` as a probe writes it: each type parameter `binder` binds as the
/// probe's own `P0`, `P1`.., each declaration by its module's path.
fn written(sigs: &Signatures, t: &ResolvedType, binder: Option<DefId>) -> String {
    if let Some((b, i)) = t.parameter_binding() {
        assert_eq!(Some(b), binder, "a parameter of another declaration in {t}");
        return format!("probe.P{i}");
    }
    if let Some(p) = t.as_primitive() {
        return p.name().to_string();
    }
    let args: Vec<String> = t.args().iter().map(|a| written(sigs, a, binder)).collect();
    match t.as_builtin() {
        Some(Builtin::Function) => {
            let (result, params) = args.split_last().expect("a function type has a result");
            format!("fn({}) -> {result}", params.join(", "))
        }
        Some(b) => format!("{}<{}>", b.name(), args.join(", ")),
        None => {
            let path = sigs
                .path_of(t.def_id().expect("a declared type"))
                .expect("a declared type's path")
                .to_string();
            match args.is_empty() {
                true => path,
                false => format!("{path}<{}>", args.join(", ")),
            }
        }
    }
}

/// Each module a written type names a declaration of.
fn modules_named(sigs: &Signatures, t: &ResolvedType, out: &mut BTreeSet<String>) {
    if let Some(def) = t.def_id()
        && let Some((module, _)) = sigs.path_of(def).and_then(|p| p.rsplit_once('.'))
    {
        out.insert(module.to_string());
    }
    for a in t.args() {
        modules_named(sigs, a, out);
    }
}

/// The type parameters `t` names, by position.
fn parameters_in(t: &ResolvedType, out: &mut BTreeSet<u32>) {
    if let Some((_, i)) = t.parameter_binding() {
        out.insert(i);
    }
    for a in t.args() {
        parameters_in(a, out);
    }
}

/// **Statements that read `label{i}` of each `P{i}` a value of type `t`,
/// named `e`, holds**, through the lists, options, results, sets and maps it
/// is in, and which `P{i}` they read. A function's or a primitive's holds
/// none a statement reads.
fn reader(
    t: &ResolvedType,
    e: &str,
    n: &mut u32,
    read: &mut BTreeSet<u32>,
    per_row: &mut Vec<String>,
    looping: bool,
) -> Option<String> {
    *n += 1;
    let k = *n;
    if let Some((_, i)) = t.parameter_binding() {
        read.insert(i);
        if looping {
            per_row.push(format!("{e}.label{i}"));
        }
        return Some(format!("    let _read{k} = {e}.label{i}"));
    }
    let arg = |a: usize| t.args()[a].clone();
    match t.as_builtin()? {
        Builtin::List => {
            let x = format!("x{k}");
            let body = reader(&arg(0), &x, n, read, per_row, true)?;
            Some(format!("    for {x} in {e} {{\n{body}\n    }}"))
        }
        Builtin::Set => {
            let x = format!("x{k}");
            let body = reader(&arg(0), &x, n, read, per_row, true)?;
            Some(format!(
                "    for {x} in Set.to_list({e}) {{\n{body}\n    }}"
            ))
        }
        Builtin::Map => {
            let (key, value) = (format!("k{k}"), format!("v{k}"));
            let keys = reader(&arg(0), &key, n, read, per_row, true)
                .map(|b| format!("    for {key} in Map.keys({e}) {{\n{b}\n    }}"));
            let values = reader(&arg(1), &value, n, read, per_row, true)
                .map(|b| format!("    for {value} in Map.values({e}) {{\n{b}\n    }}"));
            match (keys, values) {
                (None, None) => None,
                (a, b) => Some(a.into_iter().chain(b).collect::<Vec<_>>().join("\n")),
            }
        }
        Builtin::Option => {
            let x = format!("x{k}");
            let body = reader(&arg(0), &x, n, read, per_row, looping)?;
            Some(format!(
                "    match {e} {{\n        Some({x}) => {{\n{body}\n        }},\n        None => (),\n    }}"
            ))
        }
        Builtin::Result => {
            let (ok, err) = (format!("x{k}"), format!("y{k}"));
            let good = reader(&arg(0), &ok, n, read, per_row, looping);
            let bad = reader(&arg(1), &err, n, read, per_row, looping);
            if good.is_none() && bad.is_none() {
                return None;
            }
            let arm = |case: String, b: Option<String>| match b {
                Some(b) => format!("        {case} => {{\n{b}\n        }},"),
                None => format!("        {case} => (),"),
            };
            Some(format!(
                "    match {e} {{\n{}\n{}\n    }}",
                arm(format!("Ok({ok})"), good),
                arm(format!("Err({err})"), bad)
            ))
        }
        Builtin::Function => None,
    }
}

/// One generic function of the standard packages, and the probe that calls
/// it.
struct Probe {
    src: String,
    /// The `P{i}` its result holds where a statement reads them, each given
    /// by an argument: what the probe must be refused for reading.
    reads: BTreeSet<u32>,
    per_row: Vec<String>,
    /// Its result as a probe writes it, each parameter as its argument gives
    /// it: `None` where an argument gives none.
    result: Option<String>,
}

/// **A probe for `sig`**: `fn probe(a0: .., a1: ..)`, each parameter of
/// `sig`'s type with `P{i}` for its `i`th type parameter, which calls it and
/// reads `label{i}`, a member that reads the database, of each `P{i}` its
/// result holds. Declared to perform what `sig` performs, and nothing else.
fn probe_for(sigs: &Signatures, sig: &Signature, arity: usize) -> Probe {
    let binder = Some(sig.definition);
    let params: Vec<&ResolvedType> = sig
        .params
        .iter()
        .map(|p| {
            p.as_ref()
                .and_then(|p| p.resolved())
                .expect("a resolved parameter")
        })
        .collect();
    let result = sig.result().expect("a resolved result");
    let mut modules = BTreeSet::from([
        sig.path.rsplit_once('.').expect("a module's").0.to_string(),
        "Map".to_string(),
        "Set".to_string(),
    ]);
    let mut given = BTreeSet::new();
    for p in &params {
        modules_named(sigs, p, &mut modules);
        parameters_in(p, &mut given);
    }
    let mut named = BTreeSet::new();
    parameters_in(result, &mut named);
    let (mut n, mut reads) = (0, BTreeSet::new());
    let mut per_row = Vec::new();
    let read = reader(result, "r", &mut n, &mut reads, &mut per_row, false);
    let solved = named.is_subset(&given);
    let mut src = String::from("module probe\n\n");
    for m in &modules {
        src.push_str(&format!("import {m}\n"));
    }
    // What the row's type arguments name, as the callee's module sees them:
    // `log<Public>`'s `Public` is `capability.Public`.
    let unit = sigs.unit_of(Some(sig.path.rsplit_once('.').expect("a module's").0));
    for effect in &sig.effects {
        let Some((_, arg)) = effect.split_once('<') else {
            continue;
        };
        let arg = arg.trim_end_matches('>').trim();
        use pw_core::resolve::{Namespace, Resolution};
        if let Some(u) = unit
            && let Resolution::Local(def) | Resolution::Imported { def, .. } =
                sigs.workspace().resolve_in(u, Namespace::Type, arg)
            && let Some((module, name)) = sigs.path_of(def).and_then(|p| p.rsplit_once('.'))
        {
            src.push_str(&format!("import {module}.{{ {name} }}\n"));
        }
    }
    for i in 0..arity {
        src.push_str(&format!(
            "\nopaque type P{i} = String\n\nfn label{i}(p: P{i}) -> String !{{ database.read<P{i}> }}\n    host \"probe:data/p{i}#label\"\n"
        ));
    }
    let args: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(k, p)| format!("a{k}: {}", written(sigs, p, binder).replace("probe.", "")))
        .collect();
    let names: Vec<String> = (0..params.len()).map(|k| format!("a{k}")).collect();
    let reading = match (&read, solved) {
        (Some(r), true) => format!("{r}\n"),
        _ => String::new(),
    };
    src.push_str(&format!(
        "\nfn probe({}) -> () !{{ {} }} {{\n    let r = {}({})\n{reading}    ()\n}}\n",
        args.join(", "),
        sig.effects.join(", "),
        sig.path,
        names.join(", "),
    ));
    Probe {
        src,
        reads: if solved { reads } else { BTreeSet::new() },
        per_row: if solved { per_row } else { Vec::new() },
        result: solved.then(|| written(sigs, result, binder)),
    }
}

/// **Non-vacuity, over the standard packages** (ADR-0351): every generic
/// function they declare, called with each type parameter a type of the
/// probe's own, gives a result `infer.rs` types with each parameter as its
/// argument gave it, and a read of a member of each element that result
/// holds is seen. Where no argument gives a parameter, the call has no type,
/// never the callee's `T`.
#[test]
fn every_generic_function_of_the_standard_packages_passes_its_element_types_through() {
    use pw_core::hir::{DeclKind, Expr, Hir};
    let files = packages();
    let lower = |files: &[(String, String)]| -> Vec<Hir> {
        files
            .iter()
            .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
            .collect()
    };
    let hirs = lower(&files);
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);

    // Every generic function the packages declare, by the HIR, and the same
    // count by their text: one the walk missed is one no probe checks.
    let mut generic: Vec<(String, usize)> = Vec::new();
    for hir in &refs {
        for (id, d) in hir.all_decls() {
            if d.kind == DeclKind::Fn && !d.type_params.is_empty() {
                let module = hir.module_of(id).expect("a package declares its module");
                generic.push((format!("{module}.{}", d.name), d.type_params.len()));
            }
        }
    }
    let by_text = files
        .iter()
        .flat_map(|(_, s)| s.lines())
        .filter(|l| {
            l.strip_prefix("fn ")
                .and_then(|r| r.split('(').next())
                .is_some_and(|name| name.contains('<'))
        })
        .count();
    assert_eq!(generic.len(), by_text, "{generic:?}");
    assert!(
        generic.len() >= 39,
        "the packages declared 39 on 2026-10-16: {generic:?}"
    );

    let mut refused = 0;
    let mut untyped = Vec::new();
    for (path, arity) in &generic {
        let sig = sigs
            .by_path(path)
            .unwrap_or_else(|| panic!("no signature for {path}"));
        let probe = probe_for(&sigs, sig, *arity);
        let mut all = files.clone();
        all.push(("probe.pw".to_string(), probe.src.clone()));

        // What `infer.rs` types the call.
        let hirs = lower(&all);
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = pw_core::resolve::Workspace::build(&refs);
        let with = Signatures::build(&ws, &refs);
        let hir = refs.last().expect("the probe");
        let (id, decl) = hir
            .all_decls()
            .find(|(_, d)| d.name == "probe")
            .expect("the probe function");
        let body = hir.body(decl.body.expect("a body"));
        let call = body
            .walk()
            .into_iter()
            .find_map(|e| match body.expr(e) {
                Expr::Let {
                    init: Some(init), ..
                } => Some(*init),
                _ => None,
            })
            .expect("`let r = ..`");
        let types = pw_core::infer::Types::of_decl(&with, hir, id, body);
        let typed = types.of(body, call).map(|t| written(&with, &t, None));
        assert_eq!(
            typed, probe.result,
            "{path}: the call's type\n{}",
            probe.src
        );
        if typed.is_none() {
            untyped.push(path.clone());
        }

        // And what the checker reports: each element read, which the probe
        // does not declare, and nothing else.
        let diagnostics: Vec<_> = check_sources(&all)
            .into_iter()
            .filter(|(n, _)| n == "probe.pw")
            .flat_map(|(_, ds)| ds)
            .collect();
        let mut bounded: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "PW0405")
            .map(|d| probe.src[d.primary_span.clone()].to_string())
            .collect();
        bounded.sort();
        let mut expected_bounded = probe.per_row.clone();
        expected_bounded.sort();
        assert_eq!(bounded, expected_bounded, "{path} per-row reads");
        let mut reported: Vec<(String, String)> = diagnostics
            .into_iter()
            .filter(|d| d.code != "PW0405")
            .map(|d| (d.code.to_string(), d.message))
            .collect();
        reported.sort();
        let expected: Vec<(String, String)> = probe
            .reads
            .iter()
            .map(|i| {
                (
                    "PW0400".to_string(),
                    format!("`probe` performs `database.read<P{i}>` but does not declare it"),
                )
            })
            .collect();
        assert_eq!(reported, expected, "{path}\n{}", probe.src);
        if !probe.reads.is_empty() {
            refused += 1;
        }
    }
    // 28 of the 39 hand an element on, and each probe of those was refused
    // for reading it. Nine answer a count, a truth or nothing (`length`,
    // `any`, `all`, the maps' and sets' `size` and `contains`, `log`'s two),
    // and the two that make one from nothing are typed as nothing.
    assert_eq!(refused, 28, "the probes that read an element");
    assert_eq!(untyped, ["Map.empty", "Set.empty"]);
}

#[test]
fn a_draw_binder_has_the_platforms_declared_context_and_arguments() {
    let src = "module t\n\npaint Marks(width: Float) !{ paint.custom } {\n    inputs width\n    draw(ctx) {\n        ctx.rect(0.0, 0.0, width, 2.0)\n    }\n}\n";
    assert_eq!(reported(src), NOTHING);
    assert_eq!(
        reported(&src.replace("ctx.rect", "ctx.missing")),
        one("PW0610", "ctx.missing")
    );
    assert_eq!(
        reported(&src.replace("width, 2.0", "width, \"bad\"")),
        one("PW0605", "\"bad\"")
    );
}

#[test]
fn rooted_effect_inference_sees_member_reads_and_function_values() {
    let src = "module t\n\nimport browser.{ ElementRef }\n\npaint Marks(el: ElementRef) !{ paint.custom } {\n    inputs el\n    draw(ctx) {\n        let x = el.offsetWidth\n    }\n}\n";
    assert_eq!(reported(src), one("PW0401", "el.offsetWidth"));
    assert_eq!(reported(&src.replace("el.offsetWidth", "1.0")), NOTHING);
    let named = src.replace("let x = el.offsetWidth", "let f = browser.offsetWidth");
    assert_eq!(reported(&named), one("PW0401", "browser.offsetWidth"));
}

#[test]
fn a_computed_receiver_is_never_resolved_by_its_members_spelling() {
    let src = "module t\n\nimport List\n\ntype Item = Item { name: String }\ntype Other = Other {}\n\nfn label(o: Other) -> String !{ database.read<Other> }\n    host \"shop:data/other#label\"\n\nfn same<T>(x: T) -> T !{} { x }\n\nfn field(i: Item) -> String !{} { same(i).name }\n";
    assert_eq!(reported(src), NOTHING);
    assert_eq!(
        reported(&src.replace("same(i).name", "same(i).label")),
        one("PW0610", "same(i).label")
    );
}

#[test]
fn a_member_with_additional_parameters_is_a_function_value_not_its_result() {
    use pw_core::hir::{Expr, Hir};
    let src = reading(
        "fn suffix(i: Item, s: String) -> String !{} { s }\n\nfn inspect(i: Item) -> () !{} {\n    let property = i.name\n    let function = i.suffix\n    let called = i.suffix(\"x\")\n    ()\n}",
    );
    assert_eq!(reported(&src), NOTHING);
    let files = program(&src);
    let hirs: Vec<Hir> = files
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    let hir = refs.last().unwrap();
    let (id, d) = hir.all_decls().find(|(_, d)| d.name == "inspect").unwrap();
    let body = hir.body(d.body.unwrap());
    let types = pw_core::infer::Types::of_decl(&sigs, hir, id, body);
    let results: Vec<Option<String>> = body
        .walk()
        .into_iter()
        .filter_map(|e| match body.expr(e) {
            Expr::Let { init: Some(i), .. } => Some(types.of(body, *i).map(|t| t.display_name())),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [Some("String".to_string()), None, Some("String".to_string())]
    );
}

#[test]
fn solved_loop_binders_are_available_to_every_type_reader() {
    use pw_core::hir::{Expr, Hir};
    let src = reading(
        "fn inspect(given: List<Item>) -> () !{} {\n    for item in List.reverse(given) {\n        let name = item.name\n    }\n    ()\n}",
    );
    let files = program(&src);
    let hirs: Vec<Hir> = files
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = pw_core::resolve::Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    let hir = refs.last().unwrap();
    let (id, d) = hir.all_decls().find(|(_, d)| d.name == "inspect").unwrap();
    let body = hir.body(d.body.unwrap());
    let types = pw_core::infer::Types::of_decl(&sigs, hir, id, body);
    let used = body
        .walk()
        .into_iter()
        .find(|e| matches!(body.expr(*e), Expr::Name(n) if n == "item"))
        .unwrap();
    let binding = types.lexical().binder(used).unwrap();
    assert_eq!(
        types.binding(binding).map(|t| t.display_name()),
        Some("Item".to_string())
    );
}

#[test]
fn an_interfaces_term_roots_refuse_untyped_member_receivers() {
    let src = reading(
        "event Changed(id: Int)\n\nquery Number(id: Int) -> Int\n    invalidates_on Changed(id)\n{ id }\n\nfn consume<T>(f: T, n: Int) -> Int !{} { n }\n\ncommand Put(id: Int) -> Result<Int, String>\n    requires Anyone\n    emits Changed(id)\n    optimistic Number(id) as n => consume((item) => item.name, n)\n",
    );
    assert_eq!(reported(&src), one("PW0408", "item.name"));
    assert_eq!(
        reported(&src.replace("(item) =>", "(item: Item) =>")),
        NOTHING
    );
}

#[test]
fn a_function_named_as_a_member_receiver_performs_its_effects() {
    let src = reading(
        "fn stamp() -> String !{ database.read<Item> }\n    host \"shop:data/items#stamp\"\n\nfn apply(f: fn() -> String) -> String !{} { f() }\n\nview Show() !{} { <p>{stamp.apply}</p> }\n",
    );
    assert_eq!(reported(&src), one("PW0401", "stamp"));
    assert_eq!(
        reported(&src.replace("stamp.apply", "t.stamp.apply")),
        one("PW0401", "t.stamp")
    );
    let pure = src.replace(
        "fn stamp() -> String !{ database.read<Item> }",
        "fn stamp() -> String !{}",
    );
    assert_eq!(reported(&pure), NOTHING);
}

#[test]
fn a_module_path_prefix_is_never_charged_as_a_function_value() {
    // The module List and function t.List occupy different namespaces.
    // List.reverse is the resolved module path, so t.List is never read.
    let src = reading(
        "fn List() -> String !{ database.read<Item> }\n    host \"shop:data/items#stamp\"\n\nview Show(given: List<Item>) !{} {\n    let xs = List.reverse(given)\n    <p>pure</p>\n}\n",
    );
    assert_eq!(reported(&src), NOTHING);
    assert_eq!(
        reported(&src.replace("List.reverse(given)", "given.reverse")),
        NOTHING
    );
}

#[test]
fn a_function_receiver_keeps_its_capability_and_privacy() {
    let capability = reading(
        "fn stamp() -> String !{ database.read<Item> }\n    host \"shop:data/items#stamp\"\n\nfn apply(f: fn() -> String) -> String !{} { f() }\n\nquery Answer() -> String { stamp.apply }\n",
    );
    assert_eq!(required(&capability, "t.Answer"), ["database.read<Item>"]);
    let pure = capability.replace(
        "fn stamp() -> String !{ database.read<Item> }",
        "fn stamp() -> String !{}",
    );
    assert!(required(&pure, "t.Answer").is_empty());
    // Labels already read the base's value. The function declares a public
    // String, but its body returns a secret-derived String; only its summary
    // and the field base's label preserve that fact through apply's public
    // signature. This does not infer privacy from an effect refusal.
    let private = "module t\n\nimport capability.{ Secret, Payments }\n\nfn token() -> Secret<Payments> !{}\n    host \"shop:data/secrets#token\"\n\nfn text<T>(value: T) -> String !{}\n    host \"shop:data/secrets#text\"\n\nfn stamp() -> String !{} { text(token()) }\n\nfn apply(f: fn() -> String) -> String !{} { f() }\n\nview Show() !{} { <p>{stamp.apply}</p> }\n";
    assert_eq!(reported(private), one("PW5003", "stamp.apply"));
    assert_eq!(
        reported(&private.replace("text(token())", "\"public\"")),
        NOTHING
    );
}

#[test]
fn a_recursive_call_instantiates_its_own_generic_result() {
    let src = reading(
        "fn first<T>(value: T, count: Int) -> T !{} {\n    if count > 0 {\n        let item = first(Item { name: \"x\" }, 0)\n        let text = item.label\n    }\n    value\n}\n",
    );
    // The recursive call is the first effectful operation once this
    // function's body has an inferred row, so PW0400 points there.
    assert_eq!(
        reported(&src),
        one("PW0400", "first(Item { name: \"x\" }, 0)")
    );
    assert_eq!(reported(&src.replace("item.label", "item.name")), NOTHING);
    // Keep the generic result nested too: identity must be checked below
    // List, and the loop binder reads the concrete recursive invocation.
    let nested = reading(
        "fn first<T>(values: List<T>, count: Int) -> List<T> !{} {\n    if count > 0 {\n        let items = first([Item { name: \"x\" }], 0)\n        for item in items { let text = item.label }\n    }\n    values\n}\n",
    );
    assert_eq!(
        reported(&nested),
        [
            one("PW0400", "first([Item { name: \"x\" }], 0)"),
            one("PW0405", "item.label")
        ]
        .concat()
    );
    assert_eq!(
        reported(&nested.replace("item.label", "item.name")),
        NOTHING
    );
}

#[test]
fn a_nested_function_instantiates_an_enclosing_generics_result() {
    let src = reading(
        "fn first<T>(value: T) -> T !{} {\n    fn inspect() -> String !{} {\n        let item = first(Item { name: \"x\" })\n        item.label\n    }\n    value\n}\n",
    );
    refused_with_control(&src, "PW0400", "item.label");
    // The caller's actual rigid T stays rigid when the argument IS that T.
    let rigid = reading(
        "fn first<T>(value: T, count: Int) -> T !{} {\n    if count > 0 {\n        let item = first(value, 0)\n        let text = item.label\n    }\n    value\n}\n",
    );
    assert_eq!(reported(&rigid), one("PW0610", "item.label"));
}

// Integrator additions after the generic-results/each-header merge. These use
// this suite's real packages and canonical checker, with pure neighbours.
#[test]
fn a_generic_each_header_types_members_in_both_its_key_and_body() {
    for markup in [
        r#"view Menu(given: List<Item>) !{} {
    <ul>
        {#each List.reverse(given) as item (item.name)}
            <li>{item.label}</li>
        {/each}
    </ul>
}"#,
        r#"view Menu(given: List<Item>) !{} {
    <ul>
        {#each List.reverse(given) as item (item.label)}
            <li>{item.name}</li>
        {/each}
    </ul>
}"#,
    ] {
        let src = reading(markup);
        assert_eq!(reported(&src), one("PW0401", "item.label"), "{src}");
        let field = src.replace("item.label", "item.name");
        assert_eq!(reported(&field), NOTHING, "{field}");
        let pure_member = src.replace(
            "!{ database.read<Item> }\n    host \"shop:data/items#label\"",
            "!{} {\n    i.name\n}",
        );
        assert_ne!(pure_member, src);
        assert_eq!(reported(&pure_member), NOTHING, "{pure_member}");
    }
}

#[test]
fn a_generic_each_header_keeps_its_members_privacy_in_key_and_body() {
    for markup in [
        r#"    <ul>
        {#each List.reverse(given) as item (item.name)}
            <li>{item.token}</li>
        {/each}
    </ul>"#,
        r#"    <ul>
        {#each List.reverse(given) as item (item.token)}
            <li>{item.name}</li>
        {/each}
    </ul>"#,
    ] {
        let src = secret(markup);
        assert_eq!(reported(&src), one("PW5003", "item.token"), "{src}");
        let public = src.replace("item.token", "item.name");
        assert_eq!(reported(&public), NOTHING, "{public}");
    }
}

#[test]
fn an_interface_term_local_call_never_calls_a_namesake_global() {
    let src = reading(
        "fn stamp() -> String !{ database.read<Item> }\n    host \"shop:data/items#stamp\"\n\nevent Changed(id: Int)\n\nquery Number(id: Int) -> Int\n    invalidates_on Changed(id)\n{ id }\n\nfn consume<T>(f: T, n: Int) -> Int !{} { n }\n\ncommand Put(id: Int) -> Result<Int, String>\n    requires Anyone\n    emits Changed(id)\n    optimistic Number(id) as n => consume((stamp: fn() -> String) => stamp(), n)\n",
    );
    assert_eq!(reported(&src), NOTHING, "{src}");
    assert_eq!(
        reported(&src.replace("fn stamp()", "fn global_stamp()")),
        NOTHING
    );
    let global = src.replace("=> stamp(), n)", "=> t.stamp(), n)");
    assert_ne!(global, src);
    assert_eq!(
        reported(&global),
        one("PW0330", "consume((stamp: fn() -> String) => t.stamp(), n)")
    );
}

#[test]
fn member_invocations_have_one_typed_callee_and_their_actual_effects() {
    let src = reading(
        "fn inferred(i: Item) -> String { i.label }\n\nfn reverse(i: Item) -> String !{ database.read<Item> }\n    host \"shop:data/items#reverse\"\n\nfn decorate(i: Item, prefix: String) -> String !{} { prefix }\n\nfn property(i: Item) -> String !{ database.read<Item> } { i.inferred }\nfn explicit(i: Item) -> String !{ database.read<Item> } { i.label() }\nfn shadow(List: Item) -> String !{ database.read<Item> } { List.reverse() }\nfn shadow_property(List: Item) -> String !{ database.read<Item> } { List.reverse }\nfn field(i: Item) -> String !{} { i.name }\nfn function_value(i: Item) -> String !{} { let f = i.decorate; i.name }\n",
    );
    assert_eq!(reported(&src), NOTHING, "{src}");
    let files = program(&src);
    let hirs: Vec<pw_core::hir::Hir> = files
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&pw_core::hir::Hir> = hirs.iter().collect();
    let workspace = pw_core::resolve::Workspace::build(&refs);
    let signatures = pw_core::signatures::Signatures::build(&workspace, &refs);
    let mut inference = pw_core::effects::Inference::new(&signatures, &workspace);
    inference.run(&refs);
    for (function, callee) in [
        ("property", "t.inferred"),
        ("explicit", "t.label"),
        ("shadow", "t.reverse"),
        ("shadow_property", "t.reverse"),
    ] {
        let definition = signatures
            .by_path(&format!("t.{function}"))
            .expect("function")
            .definition;
        let performed = inference.performed_of(definition).expect("performed");
        let actual: Vec<_> = performed
            .calls
            .iter()
            .map(|c| {
                (
                    signatures.path_of(c.callee).expect("callee"),
                    c.effects.clone(),
                )
            })
            .collect();
        assert_eq!(
            actual,
            [(callee, vec!["database.read<Item>".to_string()])],
            "{function}"
        );
    }
    for function in ["field", "function_value"] {
        let definition = signatures
            .by_path(&format!("t.{function}"))
            .expect("function")
            .definition;
        assert!(
            inference
                .performed_of(definition)
                .expect("performed")
                .calls
                .is_empty(),
            "{function}"
        );
    }
}

#[test]
fn a_local_receiver_never_borrows_a_type_qualified_constructor() {
    let src = reading(
        "type Shape = Circle | Empty\n\nfn Empty(i: Item) -> String !{} { i.name }\n\nfn property(Shape: Item) -> String !{} { Shape.Empty }\nfn explicit(Shape: Item) -> String !{} { Shape.Empty() }\nfn genuine() -> Shape !{} { Shape.Empty }\n",
    );
    assert_eq!(reported(&src), NOTHING, "{src}");
    let renamed = src
        .replace("(Shape: Item)", "(item: Item)")
        .replace("{ Shape.Empty() }", "{ item.Empty() }")
        .replace(
            "-> String !{} { Shape.Empty }",
            "-> String !{} { item.Empty }",
        );
    assert_eq!(reported(&renamed), NOTHING, "{renamed}");
    let wrong = src.replace(
        "fn explicit(Shape: Item) -> String",
        "fn explicit(Shape: Item) -> Shape",
    );
    assert_ne!(wrong, src);
    assert_eq!(reported(&wrong), one("PW0606", "Shape.Empty()"));
}

#[test]
fn a_computed_receiver_keeps_its_member_call_and_never_becomes_a_partial_path() {
    let src = reading(
        "import decode\n\nfn same<T>(x: T) -> T !{} { x }\n\nfn suffix(i: Item, text: String) -> String !{} { text }\n\nfn list(i: Item) -> String !{} { i.name }\n\ntype Holder = Holder { decode: Item }\n\nfn make(i: Item) -> Holder !{} { Holder { decode: i } }\n\nfn direct(i: Item) -> String !{} { i.suffix(\"x\") }\nfn computed(i: Item) -> String !{} { same(i).suffix(\"x\") }\nfn nested(i: Item) -> String !{} { make(i).decode.list() }\nfn field(i: Item) -> String !{} { same(i).name }\n",
    );
    assert_eq!(reported(&src), NOTHING, "{src}");
    let wrong = src.replace(
        "fn computed(i: Item) -> String",
        "fn computed(i: Item) -> Int",
    );
    assert_eq!(reported(&wrong), one("PW0606", "same(i).suffix(\"x\")"));
}

#[test]
fn member_and_local_function_callees_keep_their_actual_result_type() {
    for (parameters, expression) in [
        ("List: Item", "List.length().caption"),
        ("List: Item", "List.length.caption"),
        ("f: fn() -> Detail", "f().caption"),
    ] {
        let src = reading(&format!(
            "type Detail = Detail {{ text: String }}\n\nfn length(i: Item) -> Detail !{{}} {{ Detail {{ text: i.name }} }}\n\nfn f() -> Int !{{}} {{ 0 }}\n\nfn caption(d: Detail) -> String !{{ database.read<Detail> }}\n    host \"shop:data/details#caption\"\n\nfn shadow({parameters}) -> String !{{}} {{ {expression} }}\n"
        ));
        assert_eq!(reported(&src), one("PW0400", expression), "{src}");
        let pure = src.replace(".caption", ".text");
        assert_ne!(pure, src);
        assert_eq!(reported(&pure), NOTHING, "{pure}");
    }
}
