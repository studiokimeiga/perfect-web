//! **A case, from a browser** (the integrator's, 2026-10-13; W9's finding in
//! NEXT 6b).
//!
//! A page could not send a command a value of a sum type: the handler's
//! compiler refused it ("which a browser cannot send"), and a host refused
//! one ("is not accepted from a browser"). W9's tip took a percent as an
//! `Int`, refused at run time where its type should refuse it at check. A
//! browser now writes a case as a signal's value is written, `{ "$case":
//! <its WIT name>, "value": <its payload> }`, a case of several fields an
//! array, and a host reads it by the parameter's own type: a declared case
//! (a WIT variant, whether or not its cases carry a payload), `Option`'s
//! `some` and `none`, and `Result`'s `ok` and `err`.
//!
//! Each test states one case, with its control.

use pw_conformance::{Runnable, compile, units};
use serde_json::json;
use wasmtime::component::Val;

const CASES: &str = "module v

type Tip =
    | NoTip
    | Fifteen
    | Amount(Int)
    | Split(Int, String)

type Size =
    | Small
    | Large

command tipped(t: Tip) -> Result<Int, String>
    requires Anyone
{
    Ok(match t {
        NoTip => 0,
        Fifteen => 15,
        Amount(n) => n,
        Split(n, _) => n * 2,
    })
}

command sized(s: Size) -> Result<Int, String>
    requires Anyone
{
    Ok(match s {
        Small => 1,
        Large => 2,
    })
}

command maybe(n: Option<Int>) -> Result<Int, String>
    requires Anyone
{
    Ok(match n {
        Some(x) => x,
        None => 0 - 1,
    })
}

command either(r: Result<Int, String>) -> Result<Int, String>
    requires Anyone
{
    Ok(match r {
        Ok(x) => x,
        Err(_) => 0 - 2,
    })
}
";

fn runnable(id: &str) -> Runnable {
    Runnable::new(compile(&units(&[("v.pw", CASES)]), id))
}

/// What the component answers for `sent`, a browser's one argument.
fn answered(r: &Runnable, sent: serde_json::Value) -> Result<i64, String> {
    let args = r.arguments(&[sent])?;
    match r.call(&Default::default(), &args)?.as_slice() {
        [Val::Result(Ok(Some(n)))] => match **n {
            Val::S64(n) => Ok(n),
            ref other => Err(format!("answered {other:?}")),
        },
        other => Err(format!("answered {other:?}")),
    }
}

/// **A declared case, with a payload, without, and of several fields**, read
/// as the browser writes it, and the component given the case it was sent.
#[test]
fn a_declared_case_is_read_by_its_name_and_payload() {
    let r = runnable("v.tipped");
    assert_eq!(answered(&r, json!({ "$case": "no-tip" })), Ok(0));
    assert_eq!(answered(&r, json!({ "$case": "fifteen" })), Ok(15));
    assert_eq!(
        answered(&r, json!({ "$case": "amount", "value": 7 })),
        Ok(7)
    );
    assert_eq!(
        answered(&r, json!({ "$case": "split", "value": [4, "even"] })),
        Ok(8)
    );
}

/// **A type whose cases carry nothing**, read the same way: a WIT variant
/// still, as every Pleris sum type is.
#[test]
fn a_case_of_a_type_whose_cases_carry_nothing_is_read_by_its_name() {
    let r = runnable("v.sized");
    assert_eq!(answered(&r, json!({ "$case": "small" })), Ok(1));
    assert_eq!(answered(&r, json!({ "$case": "large" })), Ok(2));
}

/// **`Option` and `Result`**, by `some` and `none`, `ok` and `err`.
#[test]
fn options_and_results_are_read_by_their_cases() {
    let r = runnable("v.maybe");
    assert_eq!(answered(&r, json!({ "$case": "some", "value": 9 })), Ok(9));
    assert_eq!(answered(&r, json!({ "$case": "none" })), Ok(-1));
    let r = runnable("v.either");
    assert_eq!(answered(&r, json!({ "$case": "ok", "value": 3 })), Ok(3));
    assert_eq!(
        answered(&r, json!({ "$case": "err", "value": "no" })),
        Ok(-2)
    );
}

/// **What is not a case of the type, as sent, is refused**, before the
/// component runs, naming the argument: a case it does not declare, a
/// payload a case has none of, one it lacks or of another type, a key a
/// case does not carry, and a value that is no case.
#[test]
fn what_is_not_a_case_of_the_type_is_refused_by_the_host() {
    let r = runnable("v.tipped");
    for sent in [
        json!({ "$case": "twenty" }),
        json!({ "$case": "fifteen", "value": 15 }),
        json!({ "$case": "amount" }),
        json!({ "$case": "amount", "value": "seven" }),
        json!({ "$case": "split", "value": [4] }),
        json!({ "$case": "fifteen", "percent": 15 }),
        json!("fifteen"),
        json!(15),
    ] {
        let refused = r.arguments(std::slice::from_ref(&sent));
        assert!(refused.is_err(), "{sent} was read as {refused:?}");
        assert!(
            refused.as_ref().unwrap_err().contains("argument 1"),
            "{sent}: {refused:?}"
        );
    }
    let r = runnable("v.sized");
    assert!(r.arguments(&[json!({ "$case": "medium" })]).is_err());
    assert!(
        r.arguments(&[json!({ "$case": "small", "value": 1 })])
            .is_err()
    );
    let r = runnable("v.maybe");
    assert!(r.arguments(&[json!({ "$case": "some" })]).is_err());
    assert!(
        r.arguments(&[json!({ "$case": "none", "value": 1 })])
            .is_err()
    );
    let r = runnable("v.either");
    assert!(
        r.arguments(&[json!({ "$case": "maybe", "value": 1 })])
            .is_err()
    );
}

/// A page whose handlers press `commands`, each with its argument, and the
/// program's commands beside it.
fn pressing(presses: &[&str]) -> String {
    let buttons: String = presses
        .iter()
        .map(|call| {
            format!(
                "            <button type=\"button\" on:press={{() => match {call} {{\n                \
                 Ok(_) => notice = \"\",\n                Err(_) => notice = \"no\",\n            \
                 }}}}>press</button>\n"
            )
        })
        .collect();
    format!(
        "{CASES}
opaque type InteractionId = String

type Tree =
    | Leaf
    | Branch(Int, List<Tree>)

command choose(t: Tip) -> Result<Int, String>
    requires      Anyone
    idempotent_by InteractionId
{{
    Ok(0)
}}

command maybe_pressed(n: Option<Int>) -> Result<Int, String>
    requires      Anyone
    idempotent_by InteractionId
{{
    Ok(0)
}}

command either_pressed(r: Result<Int, String>) -> Result<Int, String>
    requires      Anyone
    idempotent_by InteractionId
{{
    Ok(0)
}}

command planted(t: Tree) -> Result<Int, String>
    requires      Anyone
    idempotent_by InteractionId
{{
    Ok(0)
}}

page P() {{
    cache private
    signal notice: String = \"\"

    view {{
        <main>
            <p>{{notice}}</p>
{buttons}        </main>
    }}
}}
"
    )
}

/// Each of the page's handlers, as the browser's compiler encodes it.
fn handlers(
    program: &str,
) -> Vec<pw_core::backend::wasm::Encoding<pw_core::backend::js::HandlerModule>> {
    pw_core::build::build(&units(&[("v.pw", program)]))
        .unwrap_or_else(|e| panic!("builds: {e}"))
        .handlers
        .into_iter()
        .filter(|h| h.declaration == "v.P~ui")
        .map(|h| h.module)
        .collect()
}

/// **A page sends a command a case**: a declared case, with a payload and
/// without, an `Option`'s and a `Result`'s. Its handler, compiled for the
/// browser, writes each as a case, where it was refused ("a command
/// parameter a browser cannot send").
#[test]
fn a_pages_handler_sends_a_command_a_case() {
    let encoded = handlers(&pressing(&[
        "choose(Fifteen)",
        "choose(Amount(5))",
        "maybe_pressed(Some(3))",
        "either_pressed(Ok(1))",
    ]));
    assert_eq!(encoded.len(), 4, "each press's handler");
    for module in encoded {
        let pw_core::backend::wasm::Encoding::Encoded(m) = module else {
            panic!("{module:?}");
        };
        assert!(m.source.contains("$case"), "{}", m.source);
    }
}

/// **A case of a type that contains itself is not sent**: a host reads none
/// from a browser, so the handler is refused where it is compiled, by name.
#[test]
fn a_case_of_a_type_that_contains_itself_is_not_sent() {
    let refused = handlers(&pressing(&["planted(Leaf)"]));
    let [pw_core::backend::wasm::Encoding::Unsupported { construct, .. }] = refused.as_slice()
    else {
        panic!("{refused:?}");
    };
    assert_eq!(*construct, "a command parameter a browser cannot send");
}
