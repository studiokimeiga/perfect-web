//! **A command runs when the clock passes its time** (the scheduled-commands
//! track's PW65xx, and `invoked_by Clock` under PW64xx).
//!
//! `schedule cmd(args) at when` and `schedule cmd(args) after duration` name
//! a command `invoked_by Clock` and when it runs; `unschedule cmd(args)`
//! cancels its pending run. Each test states one rule, with its control.

use pw_core::hir::Hir;
use pw_core::resolve::Workspace;
use pw_core::signatures::Signatures;

/// The platform modules a program that schedules reads.
fn platform() -> Vec<(String, String)> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/pw-platform-web");
    ["capability.pw", "clock.pw", "effects.pw"]
        .iter()
        .map(|f| {
            let src = std::fs::read_to_string(dir.join(f)).expect("a platform module");
            (f.to_string(), src)
        })
        .collect()
}

/// Program `t`, `decls` after its imports, beside the platform's clock.
fn files(decls: &str) -> Vec<(String, String)> {
    let mut files = platform();
    files.push((
        "t.pw".to_string(),
        format!(
            "module t\n\nimport clock.{{ Instant, Duration, minutes }}\n\n\
             opaque type InteractionId = String\n\n{decls}"
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

/// A command the clock invokes, given an order.
const EXPIRE: &str = "command expire(order: String) -> Result<Int, String>\n    \
     invoked_by    Clock\n    idempotent_by InteractionId\n{\n    Ok(1)\n}\n\n";

/// A command a reader presses, which runs `statement`, its row `row`.
fn scheduler(statement: &str, row: &str) -> String {
    format!(
        "command place(order: String, when: Instant) -> Result<Int, String> {row}\n    requires Anyone\n{{\n    \
         {statement}\n    Ok(1)\n}}\n\n"
    )
}

const ROW: &str = "!{ clock.schedule }";

/// **A command the clock invokes checks clean, says so in its contract, and
/// a program may have several**, each with its own parameters: PW6402's one
/// `Settled` and PW6403's one command are a payment's alone.
#[test]
fn a_command_the_clock_invokes_checks_clean_and_says_so_in_its_contract() {
    let two = format!(
        "{EXPIRE}command remind(order: String, times: Int) -> Result<Int, String>\n    \
         invoked_by    Clock\n    idempotent_by InteractionId\n{{\n    Ok(1)\n}}\n\n"
    );
    assert!(reported(&two).is_empty(), "{:?}", reported(&two));
    let sources = files(EXPIRE);
    let hirs: Vec<Hir> = sources
        .iter()
        .map(|(_, s)| pw_core::lower::lower_file(s, &pw_syntax::parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    let export = pw_core::contract::contracts(&refs, &sigs, &ws)
        .into_iter()
        .find(|c| c.component_id == "t.expire")
        .and_then(|c| c.exports.into_iter().next())
        .and_then(|e| e.component)
        .expect("the command's export");
    assert_eq!(export.invoked_by.as_deref(), Some("Clock"));
}

/// **A command the clock invokes holds no reader to a predicate and is
/// idempotent**, as one a payment invokes, said in the clock's words.
#[test]
fn a_command_the_clock_invokes_requires_nothing_and_is_idempotent() {
    let requiring = "command expire(order: String) -> Result<Int, String>\n    \
         invoked_by    Clock\n    requires      SignedIn\n    idempotent_by InteractionId\n\
         {\n    Ok(1)\n}\n\n";
    let found = with("PW6401", &reported(requiring));
    assert_eq!(found.len(), 1, "{:?}", reported(requiring));
    assert!(
        found[0].contains("is invoked by Clock, not by a reader"),
        "{found:?}"
    );
    let unkeyed = "command expire(order: String) -> Result<Int, String>\n    \
         invoked_by    Clock\n{\n    Ok(1)\n}\n\n";
    assert_eq!(
        with("PW6405", &reported(unkeyed)).len(),
        1,
        "{:?}",
        reported(unkeyed)
    );
    // The control: as written above, neither.
    assert!(reported(EXPIRE).is_empty(), "{:?}", reported(EXPIRE));
}

/// **A schedule at an instant, one after a duration, and an unschedule check
/// clean** in a command that declares `clock.schedule`.
#[test]
fn a_schedule_at_an_instant_or_after_a_duration_checks_clean() {
    for statement in [
        "schedule expire(order) at when",
        "schedule expire(order) after minutes(30)",
        "unschedule expire(order)",
    ] {
        let src = format!("{EXPIRE}{}", scheduler(statement, ROW));
        assert!(
            reported(&src).is_empty(),
            "{statement}: {:?}",
            reported(&src)
        );
    }
}

/// **A schedule names a command the clock invokes** (PW6501): not a command
/// a reader presses, not a name nothing declares, and written as a call.
#[test]
fn a_schedule_names_a_command_the_clock_invokes() {
    let pressed = format!(
        "{EXPIRE}{}",
        scheduler("schedule place(order, when) at when", ROW)
    );
    let found = with("PW6501", &reported(&pressed));
    assert_eq!(found.len(), 1, "{:?}", reported(&pressed));
    assert!(
        found[0].contains("names a command the clock does not invoke"),
        "{found:?}"
    );
    let nothing = format!(
        "{EXPIRE}{}",
        scheduler("schedule vanish(order) at when", ROW)
    );
    let found = with("PW6501", &reported(&nothing));
    assert_eq!(found.len(), 1, "{:?}", reported(&nothing));
    assert!(
        found[0].contains("names no command this module sees"),
        "{found:?}"
    );
    let unshaped = format!(
        "{EXPIRE}{}",
        scheduler("unschedule expire(order) at when", ROW)
    );
    assert_eq!(
        with("PW6501", &reported(&unshaped)).len(),
        1,
        "{:?}",
        reported(&unshaped)
    );
}

/// **A schedule is a command's statement** (PW6502): a function writes
/// nothing for it to be written with.
#[test]
fn a_schedule_is_a_commands_statement() {
    let src = format!(
        "{EXPIRE}fn plan(order: String, when: Instant) -> Int {ROW} {{\n    \
         schedule expire(order) at when\n    1\n}}\n\n"
    );
    let found = with("PW6502", &reported(&src));
    assert_eq!(found.len(), 1, "{:?}", reported(&src));
    assert!(
        found[0].contains("in `plan`, which is no command"),
        "{found:?}"
    );
}

/// **A run's time is `at` an `Instant` or `after` a `Duration`** (PW6503),
/// and a schedule says one, in those words.
#[test]
fn a_runs_time_is_an_instant_or_a_duration() {
    for (statement, said) in [
        (
            "schedule expire(order) at minutes(30)",
            "is given a `clock.Duration`, where it takes a `clock.Instant`",
        ),
        (
            "schedule expire(order) after when",
            "is given a `clock.Instant`, where it takes a `clock.Duration`",
        ),
        ("schedule expire(order)", "says no time"),
        // A word where the time's goes is no time, and the command's own
        // argument is not taken for one.
        (
            "schedule expire(order) later",
            "`schedule expire(..) later` in `place` says no time",
        ),
    ] {
        let src = format!("{EXPIRE}{}", scheduler(statement, ROW));
        let found = with("PW6503", &reported(&src));
        assert_eq!(found.len(), 1, "{statement}: {:?}", reported(&src));
        assert!(found[0].contains(said), "{statement}: {found:?}");
    }
}

/// **The time's type is the platform's by its declaration**: a program's
/// own type spelt `Instant` is another (RISK_QUEUE 34), and refused.
#[test]
fn a_programs_own_instant_is_not_the_clocks() {
    let mut sources = platform();
    sources.push((
        "t.pw".to_string(),
        format!(
            "module t\n\nopaque type InteractionId = String\n\nopaque type Instant = Int\n\n\
             {EXPIRE}{}",
            scheduler("schedule expire(order) at when", ROW)
        ),
    ));
    let found: Vec<String> = pw_core::check::check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .filter(|d| d.starts_with("PW6503"))
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("is given a `t.Instant`"), "{found:?}");
}

/// **A schedule gives the command its arguments, related as a call's**:
/// the wrong type and the wrong number are refused as a call's are.
#[test]
fn a_schedule_relates_its_arguments_as_a_call() {
    for statement in [
        "schedule expire(1) at when",
        "schedule expire(order, order) at when",
    ] {
        let src = format!("{EXPIRE}{}", scheduler(statement, ROW));
        let found: Vec<String> = reported(&src)
            .into_iter()
            .filter(|d| d.starts_with("PW06"))
            .collect();
        assert!(!found.is_empty(), "{statement}: {:?}", reported(&src));
    }
}

/// **A schedule performs `clock.schedule`**: a command that schedules and
/// does not say so is refused as one calling an effect it does not declare.
#[test]
fn a_schedule_performs_clock_schedule() {
    let src = format!(
        "{EXPIRE}{}",
        scheduler("schedule expire(order) at when", "!{}")
    );
    let found = with("PW0400", &reported(&src));
    assert_eq!(found.len(), 1, "{:?}", reported(&src));
    assert!(found[0].contains("clock.schedule"), "{found:?}");
}
