//! **A command the platform invokes, which no reader presses** (NEXT 6c, the
//! payments track's PW64xx).
//!
//! `invoked_by Payments` names the command a payment's outcome invokes: the
//! host calls it on its provider's word, and no page sends it. Each test
//! states one rule, with its control.

use pw_core::hir::Hir;
use pw_core::resolve::Workspace;
use pw_core::signatures::Signatures;

/// The platform modules a program that settles payments reads.
fn platform() -> Vec<(String, String)> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/pw-platform-web");
    ["capability.pw", "payments.pw"]
        .iter()
        .map(|f| {
            let src = std::fs::read_to_string(dir.join(f)).expect("a platform module");
            (f.to_string(), src)
        })
        .collect()
}

/// Program `t`, `decls` after its imports, beside the platform's payments.
fn files(decls: &str) -> Vec<(String, String)> {
    let mut files = platform();
    files.push((
        "t.pw".to_string(),
        format!(
            "module t\n\nimport payments.{{ Settled }}\n\nopaque type InteractionId = String\n\n{decls}"
        ),
    ));
    files
}

/// Each `code message` the check reports for `t.pw`.
fn reported(decls: &str) -> Vec<String> {
    pw_core::check::check_sources(&files(decls))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn with(code: &str, reported: &[String]) -> Vec<String> {
    reported
        .iter()
        .filter(|d| d.starts_with(code))
        .cloned()
        .collect()
}

/// A command `name(params)` with `clauses`, which answers `Ok(1)`.
fn command(name: &str, params: &str, clauses: &str) -> String {
    format!("command {name}({params}) -> Result<Int, String>\n{clauses}{{\n    Ok(1)\n}}\n\n")
}

const BY: &str = "    invoked_by    Payments\n    idempotent_by InteractionId\n";

/// The contract of `id`.
fn contract_of(decls: &str, id: &str) -> pw_core::contract::ComponentContract {
    let sources = files(decls);
    let hirs: Vec<Hir> = sources
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    pw_core::contract::contracts(&refs, &sigs, &ws)
        .into_iter()
        .find(|c| c.component_id == id)
        .expect("the command's contract")
}

/// `id`'s export, as its contract carries it.
fn export_of(decls: &str, id: &str) -> pw_core::contract::ComponentExport {
    contract_of(decls, id)
        .exports
        .into_iter()
        .next()
        .and_then(|e| e.component)
        .expect("the command's export")
}

/// **Invoked by Payments, given a `Settled`, it checks clean**, and its
/// contract says who invokes it, as part of its interface.
#[test]
fn a_command_the_platform_invokes_says_so_in_its_contract() {
    let settle = command("settle", "settled: Settled", BY);
    assert!(reported(&settle).is_empty(), "{:?}", reported(&settle));
    let export = export_of(&settle, "t.settle");
    assert_eq!(export.invoked_by.as_deref(), Some("Payments"));
    // Control: the same command no platform invokes says nobody, and is
    // another interface. Its other clauses the same: a control that also
    // lacked `idempotent_by` was another interface for that too, and held
    // nothing of `invoked_by` (train-5's run found the mutant surviving).
    let plain = command(
        "settle",
        "settled: Settled",
        "    idempotent_by InteractionId\n",
    );
    assert_eq!(export_of(&plain, "t.settle").invoked_by, None);
    assert_ne!(
        contract_of(&settle, "t.settle").abi_schema,
        contract_of(&plain, "t.settle").abi_schema,
        "who may invoke a command is part of its interface"
    );
    // Written qualified, the platform's type is the same type.
    let qualified = command("settle", "settled: payments.Settled", BY);
    assert!(
        reported(&qualified).is_empty(),
        "{:?}",
        reported(&qualified)
    );
}

/// **A word the platform does not invoke by is refused**, and a clause that
/// is a command's is refused on a query.
#[test]
fn only_the_platforms_invokers_and_only_on_a_command() {
    let unknown = command(
        "settle",
        "settled: Settled",
        "    invoked_by    Stripe\n    idempotent_by InteractionId\n",
    );
    assert!(
        reported(&unknown).iter().any(|d| d.contains("Stripe")),
        "{:?}",
        reported(&unknown)
    );
    let query = "public query Q() -> Int\n    freshness 0.seconds\n    invoked_by    Payments\n{\n    1\n}\n";
    assert!(
        reported(query).iter().any(|d| d.contains("invoked_by")),
        "{:?}",
        reported(query)
    );
}

/// **PW6405: it declares `idempotent_by`**: the provider's word arrives more
/// than once, and the host keys each outcome so it commits once.
#[test]
fn a_command_the_platform_invokes_is_idempotent() {
    let once = command("settle", "settled: Settled", "    invoked_by    Payments\n");
    assert_eq!(
        with("PW6405", &reported(&once)),
        ["PW6405 `settle` is invoked by Payments, and declares no `idempotent_by`"],
    );
    // Control: keyed, it is clean.
    let keyed = command("settle", "settled: Settled", BY);
    assert!(with("PW6405", &reported(&keyed)).is_empty());
}

/// **PW6401: it declares no `requires`**, since no reader acts.
#[test]
fn a_command_the_platform_invokes_holds_no_reader_to_a_predicate() {
    let held = command(
        "settle",
        "settled: Settled",
        "    invoked_by    Payments\n    idempotent_by InteractionId\n    requires      SignedIn\n",
    );
    assert_eq!(
        with("PW6401", &reported(&held)),
        [
            "PW6401 `settle` is invoked by Payments, not by a reader: `requires SignedIn` has no one to hold"
        ],
    );
    // Control: a reader's command may hold its reader.
    let pressed = command("begin", "n: Int", "    requires      SignedIn\n");
    assert!(with("PW6401", &reported(&pressed)).is_empty());
}

/// **PW6402: it is given one `payments.Settled`**, and nothing else.
#[test]
fn a_command_the_platform_invokes_is_given_what_the_host_has() {
    for params in [
        "",
        "n: Int",
        "settled: Settled, n: Int",
        "settled: List<Settled>",
    ] {
        let wrong = command("settle", params, BY);
        assert_eq!(
            with("PW6402", &reported(&wrong)).len(),
            1,
            "({params}): {:?}",
            reported(&wrong)
        );
    }
    // A type of the program's own named `Settled` is not the platform's.
    let own = format!(
        "type Settled = Settled {{ n: Int }}\n\n{}",
        command("settle", "settled: Settled", BY)
    );
    assert_eq!(
        with("PW6402", &reported(&own)).len(),
        1,
        "{:?}",
        reported(&own)
    );
}

/// **PW6403: one command answers a payment's outcome**, the first in source
/// order; the second is refused where it is written, naming the first.
#[test]
fn one_command_answers_a_payments_outcome() {
    let two = format!(
        "{}{}",
        command("settle", "settled: Settled", BY),
        command("settle_again", "settled: Settled", BY)
    );
    assert_eq!(
        with("PW6403", &reported(&two)),
        ["PW6403 `settle` and `settle_again` are both invoked by Payments"],
    );
}

/// **PW6404: no page's handler sends it.**
#[test]
fn no_page_presses_a_command_the_platform_invokes() {
    let page = |command_name: &str| {
        format!(
            "page P() {{\n    cache private\n\n    view {{\n        <main>\n            \
             <button type=\"button\" on:press={{() => {command_name}(1)}}>Pay</button>\n        \
             </main>\n    }}\n}}\n"
        )
    };
    let pressed = format!(
        "{}{}",
        command("settle", "settled: Settled", BY),
        "page P() {\n    cache private\n\n    view {\n        <main>\n            \
         <button type=\"button\" on:press={() => settle(1)}>Pay</button>\n        \
         </main>\n    }\n}\n"
    );
    assert_eq!(
        with("PW6404", &reported(&pressed)),
        ["PW6404 `settle` is invoked by Payments, and a page's handler sends it"],
        "{:?}",
        reported(&pressed)
    );
    // Control: a page that presses the command that begins the payment.
    let begins = format!(
        "{}{}{}",
        command("settle", "settled: Settled", BY),
        command("begin", "n: Int", "    idempotent_by InteractionId\n"),
        page("begin")
    );
    assert!(
        with("PW6404", &reported(&begins)).is_empty(),
        "{:?}",
        reported(&begins)
    );
}
/// **PW6402 and PW6405 name the clause they hold the command to** (charter
/// §16.3, corpus C24): the `invoked_by` it declares, which is why it is
/// given a payment's outcome and keyed. Until C24 exercised them, both named
/// no boundary span. The control: a command a reader presses, keyed by
/// nothing, is told nothing.
#[test]
fn a_payments_command_is_told_at_its_invoked_by_clause() {
    let src_of = |decls: &str| files(decls).last().expect("t.pw").1.clone();
    let boundary = |decls: &str, code: &str| -> Vec<String> {
        let src = src_of(decls);
        pw_core::check::check_sources(&files(decls))
            .into_iter()
            .filter(|(n, _)| n == "t.pw")
            .flat_map(|(_, ds)| ds)
            .filter(|d| d.code == code)
            .flat_map(|d| d.related)
            .map(|r| src[r.span.start..r.span.end].trim().to_string())
            .collect()
    };
    let misgiven = command("settle", "order: String", BY);
    assert_eq!(
        boundary(&misgiven, "PW6402"),
        ["invoked_by    Payments"],
        "{:?}",
        reported(&misgiven)
    );
    let unkeyed = command("settle", "settled: Settled", "    invoked_by Payments\n");
    assert_eq!(
        boundary(&unkeyed, "PW6405"),
        ["invoked_by Payments"],
        "{:?}",
        reported(&unkeyed)
    );
    // A command a reader presses says who may (ADR-0406).
    let pressed = command("begin", "n: Int", "    requires Anyone\n");
    assert!(reported(&pressed).is_empty(), "{:?}", reported(&pressed));
}

/// A checked identity rotation has one parameter-free lifecycle command.
/// Its handles come from platform context, never browser arguments.
#[test]
fn an_identity_transition_has_one_parameter_free_idempotent_target() {
    let policy = "    invoked_by Identity\n    idempotent_by InteractionId\n";
    let joined = command("joined", "", policy);
    assert!(reported(&joined).is_empty(), "{:?}", reported(&joined));
    assert_eq!(
        export_of(&joined, "t.joined").invoked_by.as_deref(),
        Some("Identity")
    );
    let params = reported(&command("joined", "previous: String", policy));
    assert_eq!(with("PW6402", &params).len(), 1, "{params:?}");
    let twice = reported(&(joined + &command("another", "", policy)));
    assert_eq!(with("PW6403", &twice).len(), 1, "{twice:?}");
    let unkeyed = reported(&command("joined", "", "    invoked_by Identity\n"));
    assert_eq!(with("PW6405", &unkeyed).len(), 1, "{unkeyed:?}");
    let guarded = reported(&format!(
        "predicate SignedIn\n    says \"Sign in.\"\n{}",
        command(
            "joined",
            "",
            &(policy.to_string() + "    requires SignedIn\n")
        )
    ));
    assert_eq!(with("PW6401", &guarded).len(), 1, "{guarded:?}");
}
