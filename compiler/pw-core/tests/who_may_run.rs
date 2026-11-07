//! **A command says who may run it** (PW0367, the requires ADR).
//!
//! The server runs a command for any request that names it, whether or not a
//! page shows its button. Until 2026-11-06 a command that said nothing of who
//! may run it compiled, and anyone could. Now a command a reader can run
//! declares `requires`: the predicates a request must hold, or `Anyone`,
//! alone, which every reader holds. Each test states one part, with controls.

use pw_core::check::check_sources;

fn library() -> Vec<(String, String)> {
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
    out
}

/// Each diagnostic of `src` with `code`, as `message | each related note`,
/// and the text its primary span covers.
fn reported(src: &str, code: &str) -> Vec<(String, String)> {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.code == code)
        .map(|d| {
            let notes: Vec<String> = d.related.iter().map(|r| r.label.clone()).collect();
            (
                format!("{} | {}", d.message, notes.join(" | ")),
                src[d.primary_span.clone()].to_string(),
            )
        })
        .collect()
}

/// A command `act`, its clauses `clauses` (each a line of its own), in a
/// program of its own.
fn command(clauses: &str) -> String {
    format!("module t\n\ncommand act(n: Int) -> Result<Int, String>\n{clauses}{{\n    Ok(n)\n}}\n")
}

#[test]
fn a_command_that_says_nothing_of_who_may_run_it_is_refused() {
    assert_eq!(
        reported(&command(""), "PW0367"),
        [(
            "`act` runs for any reader's request, and says nothing of who may run it | \
             a request that names `act` runs it, with or without a page"
                .to_string(),
            "act".to_string()
        )]
    );
}

/// The predicates a request must hold, the deployment's or the program's,
/// with the command's parameters, or `Anyone`.
#[test]
fn requires_says_who_may() {
    for clauses in [
        "    requires SignedIn\n",
        "    requires SignedIn, OwnsOrder(n)\n",
        "    requires Anyone\n",
    ] {
        assert_eq!(reported(&command(clauses), "PW0367"), [], "{clauses}");
    }
}

/// One the platform runs says `invoked_by`, and declares no `requires`
/// (PW6401): PW0367 does not ask it to.
#[test]
fn a_command_the_platform_runs_says_invoked_by_instead() {
    let program = "module t

opaque type InteractionId = String

command act(n: Int) -> Result<Int, String>
    invoked_by    Clock
    idempotent_by InteractionId
{
    Ok(n)
}
";
    assert_eq!(reported(program, "PW0367"), []);
}

/// `Anyone` is said alone: anyone may, or only some.
#[test]
fn anyone_stands_alone_and_takes_no_argument() {
    let got = reported(&command("    requires Anyone, SignedIn\n"), "PW0367");
    assert_eq!(got.len(), 1, "{got:#?}");
    assert_eq!(
        got[0].0,
        "`act` requires `Anyone` beside another predicate | \
         a request that names `act` runs it, with or without a page"
    );
    assert!(
        got[0].1.contains("Anyone, SignedIn"),
        "at the clause: {got:#?}"
    );
    assert_eq!(
        reported(&command("    requires Anyone(n)\n"), "PW0367").len(),
        1,
        "`Anyone` takes no argument"
    );
}

/// A program's own `Anyone` would be read as everyone's, since nothing asks
/// it: it is refused where it is declared.
#[test]
fn anyone_is_the_platforms() {
    let program = format!(
        "{}\npredicate Anyone\n    says \"Anyone.\"\n",
        command("    requires SignedIn\n")
    );
    let got = reported(&program, "PW0367");
    assert_eq!(got.len(), 1, "{got:#?}");
    assert!(
        got[0].0.starts_with("`predicate Anyone` is declared"),
        "{got:#?}"
    );
    assert_eq!(got[0].1, "Anyone");
}

/// A page that sends the command is named beside it, each press where it is.
#[test]
fn a_press_that_sends_it_is_named_beside_it() {
    let program = PAGE.replace("REQUIRES", "");
    let got = reported(&program, "PW0367");
    assert_eq!(got.len(), 1, "{got:#?}");
    assert!(
        got[0].0.ends_with("| a press sends it from here"),
        "{got:#?}"
    );
}

/// A page needs no guard for a control whose command anyone may run, and
/// still needs one for a command a predicate holds (PW5048, ADR-0307).
#[test]
fn a_control_anyone_may_press_needs_no_guard() {
    let anyone = PAGE.replace("REQUIRES", "    requires      Anyone\n");
    assert_eq!(reported(&anyone, "PW5048"), []);
    assert_eq!(reported(&anyone, "PW0367"), []);
    let signed_in = PAGE.replace("REQUIRES", "    requires      SignedIn\n")
        + "\npredicate SignedIn\n    says \"Sign in to post.\"\n";
    assert_eq!(reported(&signed_in, "PW5048").len(), 1, "the control");
}

/// A page whose button sends `act`, its `requires` at `REQUIRES`.
const PAGE: &str = "module t

opaque type InteractionId = String

command act(n: Int) -> Result<Int, String>
REQUIRES    idempotent_by InteractionId
{
    Ok(n)
}

page P() {
    route \"/\"

    signal sent: Int = 0

    view {
        <title>P</title>
        <button type=\"button\" on:press={() => match act(1) {
            Ok(_) => sent = sent + 1,
            Err(_) => (),
        }}>Go</button>
    }
}
";
