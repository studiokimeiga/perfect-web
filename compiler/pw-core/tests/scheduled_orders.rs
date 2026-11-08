//! **An order placed for a time its reader chooses** (track
//! `scheduled-orders`): what the checker holds of the clock's arithmetic the
//! store's windows are computed with. Whether a window is offered is read by
//! comparing the clock (ADR-0304); moving an instant along the clock, or
//! ordering two, reads no clock, so a function that declares no effect may do
//! it, and one that compares the clock may not; and an instant so moved is
//! one a run may be scheduled at (ADR-0334), as an order is sent to its stores
//! its preparation before its window. Each test states one rule, with its
//! control.

use pw_core::check::check_sources;

/// The platform's modules, as a program is checked beside them.
fn platform() -> Vec<(String, String)> {
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

/// Each `code message` the check reports for program `t`, `decls` after its
/// imports.
fn reported(decls: &str) -> Vec<String> {
    let mut files = platform();
    files.push((
        "t.pw".to_string(),
        format!(
            "module t\n\nimport clock\nimport clock.{{ Instant, minutes, hours }}\n\n\
             opaque type InteractionId = String\n\n{decls}"
        ),
    ));
    check_sources(&files)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// Whether a window starts more than an hour after `from`, written with the
/// clock's arithmetic alone; `then` is where the hour is read.
fn window(then: &str) -> String {
    format!(
        "fn in_reach(at: Instant, from: Instant) -> Bool !{{}} {{\n    \
         {then}\n}}\n"
    )
}

/// **Moving an instant along the clock, and ordering two, reads no clock**:
/// a function that declares no effect computes a window's hour before it,
/// and its quarter hour after. The control: the same function comparing the
/// clock performs `clock.compare`, which it does not declare.
#[test]
fn moving_and_ordering_instants_reads_no_clock() {
    let found = reported(&window(
        "clock.is_before(clock.later(from, hours(1)), clock.earlier(clock.later(at, minutes(15)), minutes(15)))",
    ));
    assert!(found.is_empty(), "{found:#?}");
    let found = reported(&window("!clock.passed(clock.earlier(at, hours(1)))"));
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW0400") && d.contains("clock.compare")),
        "{found:#?}"
    );
}

/// A command the clock runs, given an order.
const SEND: &str = "command send(order: String) -> Result<Int, String>\n    \
     invoked_by    Clock\n    idempotent_by InteractionId\n{\n    Ok(1)\n}\n\n";

/// A command a reader presses, which schedules `send` `at`.
fn placing(at: &str) -> String {
    format!(
        "{SEND}command place(order: String, window: Instant) -> Result<Int, String>\n    requires Anyone\n    \
         idempotent_by InteractionId\n{{\n    schedule send(order) at {at}\n    Ok(1)\n}}\n"
    )
}

/// **An instant moved along the clock is one a run is scheduled at**: an
/// order sent to its stores forty minutes before its window. The control:
/// the same number of milliseconds as an `Int` is no instant (PW6503).
#[test]
fn a_run_is_scheduled_at_an_instant_moved_along_the_clock() {
    let found = reported(&placing("clock.earlier(window, minutes(40))"));
    assert!(found.is_empty(), "{found:#?}");
    let found = reported(&placing("2400000"));
    assert!(found.iter().any(|d| d.starts_with("PW6503")), "{found:#?}");
}
