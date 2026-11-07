//! **E9-V1..V6 — the ordinary value relations, each with its discriminator.**
//!
//! Architect ruling, 2026-08-21, reopening E9:
//!
//! > A milestone called *permanent value type checker* cannot honestly remain
//! > complete while `takes_str(42)`, `fn wrong_return() -> String { 42 }` and
//! > `takes_store(makes_cart())` all pass.
//!
//! Every refusal here has an accepted neighbour in the same shape, because a
//! rule that refuses everything passes every "is refused" assertion. And every
//! gate is scored on what a program is REFUSED for, never on what the
//! representation could express — the distinction the reopening was about.
//!
//! ```text
//! V1  every typed call checks arity and argument compatibility
//! V2  generic calls instantiate/unify through resolved types
//! V3  opaque nominal identity is DefId-based at a call
//! V4  declared function result agrees with body result
//! V5  the relations cannot operate on unresolved or partly resolved types
//! V6  ABI-equal nominal types stay semantically distinct
//! ```

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_core::values::{Outcome, RelationKind};
use pw_syntax::parse_tree;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The standard library, for programs that call `List.*`.
fn std_lib() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root().join("packages/pw-std")).expect("pw-std") {
        let p = e.expect("entry").path();
        if p.extension().is_some_and(|x| x == "pw") {
            out.push((
                p.file_name().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.sort();
    out
}

/// Codes reported for the file named `t.pw` in a program of `files`.
fn codes_in(files: &[(String, String)]) -> Vec<String> {
    check_sources(files)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| d.code.to_string()))
        .collect()
}

fn codes(src: &str) -> Vec<String> {
    codes_in(&[("t.pw".to_string(), src.to_string())])
}

fn codes_with_std(src: &str) -> Vec<String> {
    let mut files = std_lib();
    files.push(("t.pw".to_string(), src.to_string()));
    codes_in(&files)
}

fn messages(src: &str) -> Vec<String> {
    check_sources(&[("t.pw".to_string(), src.to_string())])
        .into_iter()
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

// --- V1: arguments ----------------------------------------------------------------

#[test]
fn v1_a_literal_of_the_wrong_primitive_is_refused_and_the_right_one_is_not() {
    let bad = "module z\n\nfn takes_str(s: String) -> String { s }\n\nfn caller() -> String { takes_str(42) }\n";
    assert_eq!(codes(bad), ["PW0605"], "{:?}", messages(bad));
    let good = "module z\n\nfn takes_str(s: String) -> String { s }\n\nfn caller() -> String { takes_str(\"x\") }\n";
    assert!(codes(good).is_empty(), "{:?}", messages(good));
}

#[test]
fn v1_the_reopening_case_is_refused_a_record_where_another_is_declared() {
    // `canonical_abi::a_call_site_is_not_type_checked` pinned exactly this
    // until 2026-09-24: two records, both resolvable, one module.
    let src = |arg: &str| {
        format!(
            "module r\n\ntype Store = Store {{ id: Int }}\ntype Cart = Cart {{ n: Int }}\n\n\
             fn takes_store(s: Store) -> Int {{ 0 }}\n\nfn makes_cart() -> Cart {{ Cart(1) }}\n\n\
             fn makes_store() -> Store {{ Store(1) }}\n\nfn breaks() -> Int {{\n    takes_store({arg})\n}}\n"
        )
    };
    assert_eq!(
        codes(&src("makes_cart()")),
        ["PW0605"],
        "{:?}",
        messages(&src("makes_cart()"))
    );
    assert!(
        codes(&src("makes_store()")).is_empty(),
        "{:?}",
        messages(&src("makes_store()"))
    );
}

#[test]
fn v1_a_member_call_checks_its_receiver_and_its_arguments() {
    let src = |arg: &str| {
        format!(
            "module m\n\ntype Cart = Cart {{ n: Int }}\n\nfn add(c: Cart, k: Int) -> Cart {{ c }}\n\n\
             fn f(c: Cart) -> Cart {{ c.add({arg}) }}\n"
        )
    };
    assert_eq!(
        codes(&src("\"two\"")),
        ["PW0605"],
        "{:?}",
        messages(&src("\"two\""))
    );
    assert!(codes(&src("2")).is_empty(), "{:?}", messages(&src("2")));
    // And its arity, which `PW0604` did not see for member calls before.
    assert_eq!(
        codes(&src("2, 3")),
        ["PW0604"],
        "{:?}",
        messages(&src("2, 3"))
    );
}

#[test]
fn v1_a_piped_value_is_the_first_argument() {
    let src = |lhs: &str| {
        format!(
            "module p\n\nfn double(n: Int) -> Int {{ n * 2 }}\n\nfn f() -> Int {{ {lhs} |> double() }}\n"
        )
    };
    assert_eq!(
        codes(&src("\"x\"")),
        ["PW0605"],
        "{:?}",
        messages(&src("\"x\""))
    );
    assert!(codes(&src("21")).is_empty(), "{:?}", messages(&src("21")));
}

#[test]
fn v1_a_construction_checks_each_field_by_name_and_by_position() {
    let named = |v: &str| {
        format!(
            "module c\n\ntype Cart = Cart {{ n: Int }}\n\nfn f() -> Cart {{ Cart {{ n: {v} }} }}\n"
        )
    };
    assert_eq!(
        codes(&named("\"one\"")),
        ["PW0605"],
        "{:?}",
        messages(&named("\"one\""))
    );
    assert!(codes(&named("1")).is_empty(), "{:?}", messages(&named("1")));

    let positional = |v: &str| {
        format!("module c\n\ntype Cart = Cart {{ n: Int }}\n\nfn f() -> Cart {{ Cart({v}) }}\n")
    };
    assert_eq!(codes(&positional("\"one\"")), ["PW0605"]);
    assert!(codes(&positional("1")).is_empty());
    // A construction is a call: its arity is checked too.
    assert_eq!(
        codes(&positional("1, 2")),
        ["PW0604"],
        "{:?}",
        messages(&positional("1, 2"))
    );
}

#[test]
fn v1_an_annotated_binding_is_initialised_with_its_type() {
    let src =
        |v: &str| format!("module b\n\nfn f() -> Int {{\n    let x: String = {v}\n    0\n}}\n");
    assert_eq!(codes(&src("42")), ["PW0607"], "{:?}", messages(&src("42")));
    assert!(
        codes(&src("\"x\"")).is_empty(),
        "{:?}",
        messages(&src("\"x\""))
    );
}

#[test]
fn v1_a_query_invocation_is_a_typed_call() {
    let src = |arg: &str| {
        format!(
            "module q\n\nquery Menu(store: Int) -> Int {{ store }}\n\n\
             page P(id: Int) {{\n    let menu = query Menu({arg})\n    view {{ <main /> }}\n}}\n"
        )
    };
    assert_eq!(
        codes(&src("\"x\"")),
        ["PW0605"],
        "{:?}",
        messages(&src("\"x\""))
    );
    assert!(codes(&src("id")).is_empty(), "{:?}", messages(&src("id")));
}

#[test]
fn v1_a_call_inside_a_policy_term_is_checked_with_its_binder_typed() {
    // `optimistic Cart(..) as cart => ..` binds `cart` to the target's value
    // (ADR-0025). The transition's call was checked by nothing until
    // 2026-09-24: term roots are trees the body walk does not reach. The
    // command invalidates the entry it speculates on (ADR-0105).
    let src = |arg: &str| {
        format!(
            "module o\n\ntype Cart = Cart {{ n: Int }}\ntype E = | Bad\n\n\
             query Current(k: Int) -> Result<Cart, E> {{ todo }}\n\n\
             fn bump(c: Cart, by: Int) -> Cart {{ c }}\n\n\
             command add(k: Int) -> Result<Cart, E>\n    requires Anyone\n    \
             optimistic Current(k) as cart => bump({arg}, 1)\n    \
             invalidates Current(k)\n{{\n    todo\n}}\n"
        )
    };
    assert!(
        codes(&src("cart")).is_empty(),
        "{:?}",
        messages(&src("cart"))
    );
    let bad = messages(&src("k"));
    assert!(
        bad.iter()
            .any(|m| m.starts_with("PW0605") && m.contains("bump")),
        "an Int where the binder's Cart is declared: {bad:?}"
    );
}

#[test]
fn v1_a_listeners_wildcard_is_decided_and_its_parameters_are_checked() {
    // `invalidates_on Changed(id, _)`: `_` is matched by every value at its
    // position (ADR-0091), so its relation agrees. Until ADR-0178 it was
    // typed as a name nothing declares, and stayed undecided: silent, which
    // is also what an analysis that never ran is.
    let src = |key: &str| {
        format!(
            "module l\n\nevent Changed(store: Int, item: String)\n\n\
             query Menu(id: Int) -> Int\n    invalidates_on {key}\n{{\n    id\n}}\n"
        )
    };
    let outcomes_of = |s: &str, target: &str| {
        let hir = lower_file(s, &parse_tree(s).green);
        let units = vec![Unit {
            path: "t.pw".into(),
            src: s.into(),
            hir,
        }];
        pw_core::values::analysis(&units)
            .remove(0)
            .1
            .into_iter()
            .filter(|r| r.kind == RelationKind::Argument && r.target.ends_with(target))
            .map(|r| r.outcome)
            .collect::<Vec<_>>()
    };
    let outcomes = |s: &str| outcomes_of(s, "Changed");
    assert_eq!(
        outcomes(&src("Changed(id, _)")),
        [Outcome::Agree, Outcome::Agree]
    );
    // A parameter of another type than the event's value at its position is
    // compared with nothing it can equal.
    let bad = outcomes(&src("Changed(_, id)"));
    assert_eq!(bad[0], Outcome::Agree);
    assert!(matches!(bad[1], Outcome::Disagree { .. }), "{bad:?}");
    assert_eq!(codes(&src("Changed(_, id)")), ["PW0605"]);
    assert!(codes(&src("Changed(id, _)")).is_empty());
    // Where a key is evaluated, `_` decides nothing: only a listener binds.
    // In an `invalidates` key it is every entry at the rest (ADR-0256), and
    // in an `emits` key no value, refused (PW0021).
    let evaluated = |clause: &str| {
        format!(
            "module l\n\ntype C = C {{ n: Int }}\ntype E = | Bad\n\nevent Moved(k: Int)\n\n\
             query Current(k: Int) -> Result<C, E> {{ todo }}\n\n\
             command add(k: Int) -> Result<C, E>\n    requires Anyone\n    {clause}\n{{\n    todo\n}}\n"
        )
    };
    let every = evaluated("invalidates Current(_)");
    assert_eq!(
        outcomes_of(&every, "Current"),
        [Outcome::Undecided(pw_core::values::Undecided::Unknown)]
    );
    assert!(codes(&every).is_empty(), "{:?}", messages(&every));
    let none = evaluated("emits Moved(_)");
    assert_eq!(
        outcomes_of(&none, "Moved"),
        [Outcome::Undecided(pw_core::values::Undecided::Unknown)]
    );
    assert_eq!(codes(&none), ["PW0021"]);
}

// --- V2: generic calls ------------------------------------------------------------

#[test]
fn v2_the_architects_discriminator_identity_of_a_store_is_a_store() {
    // `identity<T>(T); identity(Store) → infer T = Store`.
    let src = |ret: &str| {
        format!(
            "module g\n\ntype Store = Store {{ id: Int }}\ntype Cart = Cart {{ n: Int }}\n\n\
             fn identity<T>(x: T) -> T {{ x }}\n\nfn f(s: Store) -> {ret} {{ identity(s) }}\n"
        )
    };
    assert!(
        codes(&src("Store")).is_empty(),
        "{:?}",
        messages(&src("Store"))
    );
    assert_eq!(
        codes(&src("Cart")),
        ["PW0606"],
        "{:?}",
        messages(&src("Cart"))
    );
}

#[test]
fn v2_one_type_parameter_is_one_type_across_arguments() {
    let src = |b: &str| {
        format!(
            "module g\n\nfn pair<T>(a: T, b: T) -> T {{ a }}\n\nfn f() -> Int {{ pair(1, {b}) }}\n"
        )
    };
    let bad = messages(&src("\"x\""));
    assert!(
        bad.iter()
            .any(|m| m.starts_with("PW0605") && m.contains("argument 2")),
        "the first argument fixes T; the second disagrees: {bad:?}"
    );
    assert!(codes(&src("2")).is_empty(), "{:?}", messages(&src("2")));
}

#[test]
fn v2_each_call_instantiates_afresh() {
    // Two calls, two different `T`s. A substitution that leaked across calls
    // would refuse the second.
    let src = "module g\n\nfn identity<T>(x: T) -> T { x }\n\n\
               fn f() -> String {\n    let n: Int = identity(1)\n    identity(\"s\")\n}\n";
    assert!(codes(src).is_empty(), "{:?}", messages(src));
}

#[test]
fn v2_inside_a_generic_body_its_own_parameter_is_rigid() {
    // Nothing makes `42` a `T`.
    let bad = "module g\n\nfn id<T>(x: T) -> T { 42 }\n";
    assert_eq!(codes(bad), ["PW0606"], "{:?}", messages(bad));
    let good = "module g\n\nfn id<T>(x: T) -> T { x }\n";
    assert!(codes(good).is_empty(), "{:?}", messages(good));
}

#[test]
fn v2_renaming_a_type_parameter_changes_nothing() {
    let a = "module g\n\nfn id<T>(x: T) -> T { x }\n\nfn f() -> Int { id(1) }\n";
    let b = "module g\n\nfn id<U>(x: U) -> U { x }\n\nfn f() -> Int { id(1) }\n";
    assert_eq!(codes(a), codes(b));
    assert!(codes(a).is_empty());
}

#[test]
fn v2_list_map_infers_its_result_from_the_callback() {
    // `T` from the list, `U` from the lambda's body — typed against `fn(T) -> U`.
    let src = |ann: &str| {
        format!(
            "module l\n\nimport List\n\nfn f(xs: List<Int>) -> Int {{\n    \
             let ys: List<{ann}> = List.map(xs, fn(x) x + 1)\n    0\n}}\n"
        )
    };
    assert!(
        codes_with_std(&src("Int")).is_empty(),
        "{:?}",
        codes_with_std(&src("Int"))
    );
    assert_eq!(codes_with_std(&src("String")), ["PW0607"]);
}

#[test]
fn v2_an_unannotated_binding_of_a_generic_call_is_instantiated() {
    // `let ys = List.filter(xs, ..)` is a `List<P>`. The pre-E9 inference
    // typed it by `filter`'s declared result, `List<T>`, and the value
    // relations kept that binding: so `List.sort_by(ys, compare)` was refused
    // for a `T` nothing had instantiated. Found 2026-09-25, writing kiokun's
    // ranking in Pleris.
    let src = |compare: &str| {
        format!(
            "module l\n\nimport List\n\ntype P = P {{\n    n: Int,\n}}\n\n\
             type Q = Q {{\n    m: Int,\n}}\n\n\
             fn by_p(a: P, b: P) -> Int {{ a.n - b.n }}\n\n\
             fn by_q(a: Q, b: Q) -> Int {{ a.m - b.m }}\n\n\
             fn f(xs: List<P>) -> List<P> {{\n    \
             let ys = List.filter(xs, fn(p) p.n > 0)\n    \
             List.sort_by(ys, {compare})\n}}\n"
        )
    };
    assert!(
        codes_with_std(&src("by_p")).is_empty(),
        "{:?}",
        codes_with_std(&src("by_p"))
    );
    // And the binding is typed, not merely silent: the wrong comparison is
    // refused.
    assert_eq!(codes_with_std(&src("by_q")), ["PW0605"]);
}

#[test]
fn v2_a_declared_function_is_a_value_of_its_function_type() {
    let src = |f: &str| {
        format!(
            "module l\n\nimport List\n\nfn double(n: Int) -> Int {{ n * 2 }}\n\n\
             fn shout(s: String) -> String {{ s }}\n\n\
             fn f(xs: List<Int>) -> List<Int> {{ List.map(xs, {f}) }}\n"
        )
    };
    assert!(
        codes_with_std(&src("double")).is_empty(),
        "{:?}",
        codes_with_std(&src("double"))
    );
    assert_eq!(
        codes_with_std(&src("shout")),
        ["PW0605"],
        "`shout` takes a String and the list holds Ints"
    );
}

#[test]
fn v2_a_callback_of_the_wrong_arity_is_a_different_function_type() {
    let src = |lam: &str| {
        format!(
            "module l\n\nimport List\n\nfn f(xs: List<Int>) -> Int {{\n    let ys = List.map(xs, {lam})\n    0\n}}\n"
        )
    };
    assert_eq!(codes_with_std(&src("fn(a, b) a")), ["PW0605"]);
    assert!(codes_with_std(&src("fn(a) a")).is_empty());
}

// --- V3: nominal identity -----------------------------------------------------------

#[test]
fn v3_an_ordinary_program_cannot_pass_one_modules_tag_where_anothers_is_required() {
    // The closing question, verbatim: *can an ordinary program pass an
    // `alpha.Tag` where `beta.Tag` is required?*
    let alpha = (
        "alpha.pw".to_string(),
        "module alpha\n\nopaque type Tag = String\n\nfn make() -> Tag { todo }\n".to_string(),
    );
    let beta = ("beta.pw".to_string(), "module beta\n\nopaque type Tag = String\n\nfn take(t: Tag) -> Int { 0 }\n\nfn make() -> Tag { todo }\n".to_string());
    let t = |call: &str| {
        (
            "t.pw".to_string(),
            format!(
                "module t\n\nimport alpha\nimport beta\n\nfn f() -> Int {{ beta.take({call}) }}\n"
            ),
        )
    };
    let bad = codes_in(&[alpha.clone(), beta.clone(), t("alpha.make()")]);
    assert_eq!(bad, ["PW0605"]);
    let good = codes_in(&[alpha, beta, t("beta.make()")]);
    assert!(good.is_empty(), "{good:?}");
}

#[test]
fn v3_an_opaque_type_is_not_its_representation_in_either_direction() {
    let src = |sig: &str, arg: &str| {
        format!(
            "module o\n\nopaque type Tag = String\n\nfn make() -> Tag {{ todo }}\n\nfn take({sig}) -> Int {{ 0 }}\n\nfn f() -> Int {{ take({arg}) }}\n"
        )
    };
    assert_eq!(
        codes(&src("s: String", "make()")),
        ["PW0605"],
        "a Tag is not a String"
    );
    assert_eq!(
        codes(&src("t: Tag", "\"x\"")),
        ["PW0605"],
        "a String is not a Tag"
    );
    assert!(codes(&src("t: Tag", "make()")).is_empty());
    // Its own constructor is how one is made from the representation.
    assert!(codes(&src("t: Tag", "Tag(\"x\")")).is_empty());
    assert_eq!(
        codes(&src("t: Tag", "Tag(1)")),
        ["PW0605"],
        "and it checks what it is built from"
    );
}

#[test]
fn v3_a_privacy_qualifier_is_not_the_type_it_qualifies() {
    // ABI transparency is not semantic identity (ruling, 2026-08-20): the WIT
    // of `Session<SessionId>` is the WIT of `SessionId`, and the types differ.
    let cap = (
        "capability.pw".to_string(),
        "module capability\n\nopaque type Session<S> = String\n\nopaque type SessionId = String\n"
            .to_string(),
    );
    let t = |param: &str| {
        (
            "t.pw".to_string(),
            format!(
                "module t\n\nimport capability.{{ Session, SessionId }}\n\n\
                 fn current() -> Session<SessionId> {{ todo }}\n\nfn take(s: {param}) -> Int {{ 0 }}\n\n\
                 fn f() -> Int {{ take(current()) }}\n"
            ),
        )
    };
    assert_eq!(codes_in(&[cap.clone(), t("SessionId")]), ["PW0605"]);
    assert!(codes_in(&[cap, t("Session<SessionId>")]).is_empty());
}

// --- V4: results ---------------------------------------------------------------------

#[test]
fn v4_the_reopening_case_wrong_return_is_refused() {
    let bad = "module r\n\nfn wrong_return() -> String { 42 }\n";
    assert_eq!(codes(bad), ["PW0606"], "{:?}", messages(bad));
    let good = "module r\n\nfn right_return() -> String { \"42\" }\n";
    assert!(codes(good).is_empty());
}

#[test]
fn v4_every_branch_and_every_return_is_a_result() {
    let src = |a: &str, b: &str| {
        format!(
            "module r\n\nfn f(c: Bool) -> Int {{\n    if c {{\n        return {a}\n    }}\n    if c {{ 1 }} else {{ {b} }}\n}}\n"
        )
    };
    assert!(
        codes(&src("0", "2")).is_empty(),
        "{:?}",
        messages(&src("0", "2"))
    );
    assert_eq!(
        codes(&src("\"early\"", "2")),
        ["PW0606"],
        "the early return"
    );
    assert_eq!(codes(&src("0", "\"else\"")), ["PW0606"], "the else branch");
}

#[test]
fn v4_each_match_arm_is_a_result() {
    let src = |arm: &str| {
        format!(
            "module r\n\ntype S = | A | B\n\nfn f(s: S) -> Int {{\n    match s {{\n        A => 1,\n        B => {arm},\n    }}\n}}\n"
        )
    };
    assert!(codes(&src("2")).is_empty(), "{:?}", messages(&src("2")));
    assert_eq!(codes(&src("\"b\"")), ["PW0606"]);
}

#[test]
fn v4_a_question_mark_returns_its_error_and_the_error_types_must_agree() {
    let src = |err: &str| {
        format!(
            "module q\n\ntype E1 = | One\ntype E2 = | Two\n\n\
             fn inner() -> Result<Int, {err}> {{ todo }}\n\n\
             fn outer() -> Result<Int, E1> {{\n    let n = inner()?\n    Ok(n)\n}}\n"
        )
    };
    assert!(codes(&src("E1")).is_empty(), "{:?}", messages(&src("E1")));
    assert_eq!(codes(&src("E2")), ["PW0606"], "{:?}", messages(&src("E2")));
}

#[test]
fn v4_the_success_value_of_a_question_mark_is_what_flows_on() {
    let src = |ann: &str| {
        format!(
            "module q\n\ntype E = | Bad\n\nfn inner() -> Result<Int, E> {{ todo }}\n\n\
             fn outer() -> Result<Int, E> {{\n    let n: {ann} = inner()?\n    Ok(0)\n}}\n"
        )
    };
    assert!(codes(&src("Int")).is_empty(), "{:?}", messages(&src("Int")));
    assert_eq!(codes(&src("String")), ["PW0607"]);
}

#[test]
fn v4_a_unit_body_discards_its_last_value() {
    // Recorded in docs/ASSUMPTIONS.md: with no statement terminator, a `()`
    // declaration's last expression is a statement.
    let src = "module u\n\nfn g() -> Int { 1 }\n\nfn f() -> () { g() }\n";
    assert!(codes(src).is_empty(), "{:?}", messages(src));
}

// --- V5: unresolved types ---------------------------------------------------------------

#[test]
fn v5_a_written_type_that_names_nothing_is_reported_once() {
    let src = "module u\n\nfn f(x: Stroe) -> Lisst<Int> { todo }\n";
    assert_eq!(codes(src), ["PW0026", "PW0026"], "{:?}", messages(src));
}

#[test]
fn v5_no_relation_runs_on_an_unresolved_type() {
    // `f(1)` is not an argument-type error: the parameter's type is not known,
    // and a relation that compared against it would be comparing a spelling.
    let src = "module u\n\nfn f(x: Stroe) -> Int { 0 }\n\nfn g() -> Int { f(1) }\n";
    assert_eq!(codes(src), ["PW0026"], "{:?}", messages(src));
}

#[test]
fn v5_a_declared_constructor_is_applied_to_exactly_its_parameters() {
    let src = |ty: &str| {
        format!("module a\n\ntype Box<T> = Box {{ v: T }}\n\nfn f(b: {ty}) -> Int {{ 0 }}\n")
    };
    assert!(
        codes(&src("Box<Int>")).is_empty(),
        "{:?}",
        messages(&src("Box<Int>"))
    );
    for wrong in ["Box", "Box<Int, Int>"] {
        let m = messages(&src(wrong));
        assert!(
            m.iter()
                .any(|m| m.starts_with("PW0026") && m.contains("takes 1 type argument")),
            "{wrong}: {m:?}"
        );
    }
}

#[test]
fn v5_a_type_from_a_missing_import_is_the_imports_diagnostic_not_a_second_one() {
    let src = "module m\n\nimport nowhere.{ Thing }\n\nfn f(t: Thing) -> Int { 0 }\n";
    let got = codes(src);
    assert!(got.contains(&"PW0020".to_string()), "{got:?}");
    assert!(!got.contains(&"PW0026".to_string()), "a cascade: {got:?}");
}

// --- V6: ABI-equal types stay distinct ------------------------------------------------------

#[test]
fn v6_two_opaque_types_with_one_representation_are_two_types() {
    // Identical on the wire, `string` both; refused in the program.
    let src = |arg: &str| {
        format!(
            "module v\n\nopaque type OrderId = String\nopaque type StoreId = String\n\n\
             fn order() -> OrderId {{ todo }}\nfn store() -> StoreId {{ todo }}\n\n\
             fn take(s: StoreId) -> Int {{ 0 }}\n\nfn f() -> Int {{ take({arg}) }}\n"
        )
    };
    assert_eq!(codes(&src("order()")), ["PW0605"]);
    assert!(codes(&src("store()")).is_empty());
}

#[test]
fn v6_a_phantom_parameter_distinguishes_values_with_one_layout() {
    let src = |cur: &str| {
        format!(
            "module v\n\ntype USD = USD {{ code: String }}\ntype EUR = EUR {{ code: String }}\n\
             type Money<C> = Money {{ minor_units: Int }}\n\n\
             fn usd() -> Money<{cur}> {{ todo }}\n\nfn pay(m: Money<USD>) -> Int {{ 0 }}\n\n\
             fn f() -> Int {{ pay(usd()) }}\n"
        )
    };
    assert!(codes(&src("USD")).is_empty());
    assert_eq!(codes(&src("EUR")), ["PW0605"]);
}

// --- the analysis is queryable and non-vacuous ------------------------------------------------

fn units_of(files: &[&str]) -> Vec<Unit> {
    files
        .iter()
        .map(|f| {
            let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{f}: {e}"));
            let hir = lower_file(&src, &parse_tree(&src).green);
            Unit {
                path: f.to_string(),
                src,
                hir,
            }
        })
        .collect()
}

fn glob(dir: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root().join(dir))
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "pw"))
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

fn program(dirs: &[&str], files: &[&str]) -> Vec<Unit> {
    let mut all: Vec<String> = dirs.iter().flat_map(|d| glob(d)).collect();
    all.extend(
        files
            .iter()
            .map(|f| root().join(f).to_string_lossy().to_string()),
    );
    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
    units_of(&refs)
}

#[test]
fn the_store_program_is_decided_not_merely_silent() {
    // Silence is also what an analysis that never ran produces. The store's
    // calls are where E10-I's compiled command comes from, so every one of
    // them must be DECIDED and agree — not undecided and quiet.
    let units = program(
        &[
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/lib",
            "examples/store",
        ],
        &["examples/domain.pw"],
    );
    let store: Vec<_> = pw_core::values::analysis(&units)
        .into_iter()
        .filter(|(p, _)| p.ends_with("store/app.pw"))
        .flat_map(|(_, r)| r)
        .filter(|r| r.kind != RelationKind::Annotation)
        .collect();
    let decided = store.iter().filter(|r| r.outcome == Outcome::Agree).count();
    let undecided: Vec<_> = store
        .iter()
        .filter(|r| r.outcome != Outcome::Agree)
        .map(|r| {
            format!(
                "{} {:?} {} {:?}",
                r.declaration, r.kind, r.target, r.outcome
            )
        })
        .collect();
    assert!(
        undecided.is_empty(),
        "the store has undecided or disagreeing relations: {undecided:#?}"
    );
    // Arity, arguments and results of every call in the page, the queries and
    // the commands. A floor, so a regression to silence cannot pass.
    assert!(
        decided >= 25,
        "only {decided} relations decided in the store"
    );
    // Specifically the command E10-I compiles.
    assert!(
        store.iter().any(|r| r.declaration == "add_to_cart"
            && r.kind == RelationKind::Argument
            && r.target == "UserCarts.add"
            && r.outcome == Outcome::Agree),
        "`add_to_cart`'s call to `UserCarts.add` must be checked and agree (a user's cart since track `store-accounts`)"
    );
}

#[test]
fn the_accepted_corpus_disagrees_nowhere_and_decides_a_floor() {
    let units = program(
        &[
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/lib",
            "examples/accepted",
        ],
        &["examples/domain.pw"],
    );
    let all: Vec<_> = pw_core::values::analysis(&units)
        .into_iter()
        .flat_map(|(_, r)| r)
        .collect();
    let disagree: Vec<_> = all
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Disagree { .. }))
        .map(|r| {
            format!(
                "{} {:?} {} {:?}",
                r.declaration, r.kind, r.target, r.outcome
            )
        })
        .collect();
    assert!(disagree.is_empty(), "{disagree:#?}");
    let agree = all
        .iter()
        .filter(|r| r.kind != RelationKind::Annotation && r.outcome == Outcome::Agree)
        .count();
    let annotations = all
        .iter()
        .filter(|r| r.kind == RelationKind::Annotation && r.outcome == Outcome::Agree)
        .count();
    eprintln!("accepted corpus: {agree} value relations agree, {annotations} annotations resolve");
    assert!(
        agree >= 100,
        "only {agree} value relations decided over the accepted corpus"
    );
    assert!(
        annotations >= 200,
        "only {annotations} annotations resolved"
    );
}

#[test]
fn diagnostics_are_exactly_the_disagreements() {
    // A projection, not a second analysis: every Disagree is one diagnostic
    // and nothing else is.
    let src = "module p\n\nfn takes_str(s: String) -> String { s }\n\n\
               fn a() -> String { takes_str(42) }\n\nfn b() -> String { 42 }\n\n\
               fn c(x: Stroe) -> Int { 0 }\n";
    let hir = lower_file(src, &parse_tree(src).green);
    let units = vec![Unit {
        path: "t.pw".into(),
        src: src.into(),
        hir,
    }];
    let relations = pw_core::values::analysis(&units).remove(0).1;
    let disagreements = relations
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Disagree { .. }))
        .count();
    assert_eq!(disagreements, 3);
    assert_eq!(
        pw_core::values::diagnostics(&relations, 0).len(),
        disagreements
    );
}

// --- the syntax the relations need ----------------------------------------------------------------

#[test]
fn a_type_declaration_keeps_its_parameters() {
    // `type Box<T>` parsed and dropped `<T>` until 2026-09-24.
    let src = "module a\n\ntype Box<T> = Box { v: T }\n";
    let hir = lower_file(src, &parse_tree(src).green);
    let decl = hir
        .all_decls()
        .find(|(_, d)| d.name == "Box")
        .map(|(_, d)| d.clone())
        .expect("Box");
    assert_eq!(decl.type_params, ["T"]);
    assert!(
        codes(src).is_empty(),
        "`v: T` resolves to the parameter: {:?}",
        messages(src)
    );
}

#[test]
fn a_callable_declares_type_parameters_and_function_types_parse() {
    let src = "module g\n\nfn apply<T, U>(x: T, f: fn(T) -> U) -> U { f(x) }\n";
    assert!(
        parse_tree(src).errors.is_empty(),
        "{:?}",
        parse_tree(src).errors
    );
    let hir = lower_file(src, &parse_tree(src).green);
    let (_, d) = hir
        .all_decls()
        .find(|(_, d)| d.name == "apply")
        .expect("apply");
    assert_eq!(d.type_params, ["T", "U"]);
    assert_eq!(
        d.params[1].ty.as_ref().map(|t| t.written()),
        Some("fn(T) -> U".to_string()),
        "a function type is written back as the grammar spells it"
    );
    assert_eq!(
        pw_syntax::fmt::format_source(src),
        src,
        "and it is canonical"
    );
}

#[test]
fn a_question_mark_is_a_node_the_hir_keeps() {
    let src = "module q\n\nfn f() -> Result<Int, Int> {\n    let n = g()?\n    Ok(n)\n}\n";
    let hir = lower_file(src, &parse_tree(src).green);
    let (_, d) = hir.all_decls().find(|(_, d)| d.name == "f").expect("f");
    let body = hir.body(d.body.expect("body"));
    assert!(
        body.walk()
            .into_iter()
            .any(|e| matches!(body.expr(e), pw_core::hir::Expr::Try { .. })),
        "the `?` must survive lowering"
    );
}

/// **Every expression syntax kind is one lowering treats as an expression.**
///
/// `TryExpr` was added to the grammar on 2026-09-24 and not to `lower.rs`'s
/// `is_expr`, and every `let x = f()?` silently lost its initialiser — the
/// enumerated-kinds shape this project has now found about nine times. A scan,
/// with a non-vacuity floor so a renamed function cannot pass it vacuously.
#[test]
fn every_expression_kind_is_lowered_as_an_expression() {
    let kinds =
        std::fs::read_to_string(root().join("compiler/pw-syntax/src/kind.rs")).expect("kind.rs");
    let lower =
        std::fs::read_to_string(root().join("compiler/pw-core/src/lower.rs")).expect("lower.rs");
    let start = lower.find("fn is_expr(").expect("is_expr");
    let end = start + lower[start..].find("\n}\n").expect("end of is_expr");
    let is_expr = &lower[start..end];
    let enum_start = kinds.find("pub enum SyntaxKind").expect("SyntaxKind");
    let enum_end = enum_start + kinds[enum_start..].find("\n}\n").expect("end of enum");
    let expr_kinds: Vec<&str> = kinds[enum_start..enum_end]
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_suffix(','))
        .map(|l| l.split(['=', ' ']).next().unwrap_or(l).trim())
        .filter(|k| k.ends_with("Expr"))
        .collect();
    assert!(expr_kinds.len() >= 18, "the scan found only {expr_kinds:?}");
    let missing: Vec<&&str> = expr_kinds
        .iter()
        .filter(|k| !is_expr.contains(&format!("K::{k}")))
        .collect();
    assert!(
        missing.is_empty(),
        "expression kinds lowering does not treat as expressions: {missing:?}"
    );
}

// --- operands (ADR-0043) -----------------------------------------------------------

/// **An operator's operands, and an `if`'s condition, are typed** (PW0609).
/// Until 2026-09-25 a comparison was typed `Bool` whatever it compared:
/// `1 == "a"` checked, and the component backend was the first to refuse it.
#[test]
fn an_operator_takes_operands_of_the_types_it_takes() {
    let program = |body: &str| {
        format!(
            "module o\n\ntype Word = Word {{ text: String }}\n\n\
             fn f(n: Int, x: Float, s: String, b: Bool, w: Word) -> Bool {{\n    {body}\n}}\n"
        )
    };
    for bad in [
        "n == s",
        "s != n",
        "n < x",
        "s >= n",
        "(n + x) > 0.0",
        "(s + s) == s",
        "(w * 2) == w",
        "(b & n) == b",
        "(n & b) == b",
        "n == 1 | s",
        "s | b",
        "!n",
        "-s == s",
        "if n { true } else { false }",
    ] {
        let src = program(bad);
        assert!(
            codes(&src).contains(&"PW0609".to_string()),
            "{bad}: {:?}",
            messages(&src)
        );
    }
    // The control: the same operators, each on the types it takes.
    for good in [
        "n == 1",
        "s != \"a\"",
        "x < 1.5",
        "s >= \"a\"",
        "(n + n * 2 - n / 3 % 4) > 0",
        "(x + x * 2.0 - x / 3.0) > 0.0",
        "(b & n == 1) | !b",
        "-n < 0",
        "w == w",
        "if b { n > 0 } else { false }",
    ] {
        let src = program(good);
        assert!(codes(&src).is_empty(), "{good}: {:?}", messages(&src));
    }
}

/// An operand the program does not type is undecided, never reported: the
/// relation is about types the program states.
#[test]
fn an_untyped_operand_is_undecided_not_refused() {
    let src = "module o\n\nfn f(n: Int, u) -> Bool { u == n & u + 1 > n }\n";
    assert!(codes(src).is_empty(), "{:?}", messages(src));
    let units: Vec<Unit> = vec![Unit {
        path: "t.pw".to_string(),
        hir: lower_file(src, &parse_tree(src).green),
        src: src.to_string(),
    }];
    let operands: Vec<_> = pw_core::values::analysis(&units)
        .into_iter()
        .flat_map(|(_, r)| r)
        .filter(|r| r.kind == RelationKind::Operand)
        .collect();
    assert!(!operands.is_empty());
    assert!(
        operands
            .iter()
            .all(|r| !matches!(r.outcome, Outcome::Disagree { .. })),
        "{operands:?}"
    );
    assert!(
        operands
            .iter()
            .any(|r| matches!(r.outcome, Outcome::Undecided(_))),
        "{operands:?}"
    );
}
