//! **A row says who may change it** (PW0370, the owned-rows ADR).
//!
//! A command a reader can run is a request away, and its arguments are the
//! request's: `forget(shelf, id)` deletes whichever note a request names.
//! Until 2026-11-07 a table such a command wrote said nothing of whose its
//! rows were, and any reader changed any row. Now it declares `owner user`,
//! `owner session` or `owner anyone`, and for the first two the platform
//! records each row's owner and holds every change to it (the adapters'
//! tests, `pw-table-memory`'s `owned_rows.rs` and `pw-table-postgres`'s).
//! Each test states one part, with controls.

use pw_core::check::{Unit, check_sources, check_sources_with};
use pw_core::diagnostics::Diagnostic;
use pw_core::tables::{OwnerKind, Schema, Step};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The platform's packages.
fn platform() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root().join(dir))
            .expect("packages")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                format!("{dir}/{}", p.file_name().unwrap().to_string_lossy()),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out
}

fn program(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = platform();
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.to_string())));
    out
}

fn units(files: &[(String, String)]) -> Vec<Unit> {
    files
        .iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path: path.clone(),
                src: src.clone(),
                hir: pw_core::lower::lower_file(src, &parsed.green),
            }
        })
        .collect()
}

/// Each diagnostic of the files `files` names, by its file.
fn diagnostics(files: &[(&str, &str)]) -> Vec<(String, Diagnostic)> {
    let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
    check_sources(&program(files))
        .into_iter()
        .filter(|(n, _)| names.contains(&n.as_str()))
        .flat_map(|(n, ds)| ds.into_iter().map(move |d| (n.clone(), d)))
        .collect()
}

/// **PW0370 as a reader reads it**: the file it is told in, the text its
/// name points at, its message, and each note's file, text and words.
type Told = (String, String, String, Vec<(String, String, String)>);

fn who_may_change(files: &[(&str, &str)]) -> Vec<Told> {
    let text = |file: &str, span: &std::ops::Range<usize>| {
        let src = files.iter().find(|(n, _)| *n == file).expect("a file").1;
        src[span.clone()].to_string()
    };
    diagnostics(files)
        .into_iter()
        .filter(|(_, d)| d.code == "PW0370")
        .map(|(file, d)| {
            let notes = d
                .related
                .iter()
                .map(|r| {
                    let at = r.file.clone().unwrap_or_else(|| file.clone());
                    (at.clone(), text(&at, &r.span), r.label.clone())
                })
                .collect();
            (
                file.clone(),
                text(&file, &d.primary_span),
                d.message.clone(),
                notes,
            )
        })
        .collect()
}

/// A shelf's notes, keyed by the shelf first, so a shelf's notes are one
/// prefix (`delete_of_shelf`). `OWNER` is where its `owner` is written.
const NOTES: &str = "module Notes\n\
\n\
opaque type NoteId = String\n\
opaque type Shelf = String\n\
\n\
type Note = Note { shelf: Shelf, id: NoteId, text: String }\n\
\n\
table Notes\n    row   Note\n    key   shelf, id\nOWNER\
\n\
source NoteData\n    holds        Notes\n    transactions serializable\n    reads        strong\n    changes      none\n";

fn notes(owner: &str) -> String {
    NOTES.replace("OWNER", owner)
}

/// A program's own commands, in a module of their own: `BODY` is theirs.
const APP: &str = "module app\n\
\n\
import Notes.{ Note, NoteId, Shelf }\n\
import Notes\n\
\n\
BODY";

fn app(body: &str) -> String {
    APP.replace("BODY", body)
}

/// One reader's command, writing a note by its key.
const FORGET: &str = "command forget(shelf: Shelf, id: NoteId) -> ()\n    requires Anyone\n{\n    \
                      Notes.delete(shelf, id)\n}\n";

const SAYS: &str = "`Notes` is written by a command any reader's request runs, and says nothing \
                    of who may change its rows";

#[test]
fn a_table_a_readers_command_writes_says_who_may_change_its_rows() {
    let notes = notes("");
    let app = app(FORGET);
    assert_eq!(
        who_may_change(&[("Notes.pw", &notes), ("app.pw", &app)]),
        [(
            "Notes.pw".to_string(),
            "Notes".to_string(),
            SAYS.to_string(),
            vec![(
                "app.pw".to_string(),
                "Notes.delete(shelf, id)".to_string(),
                "`forget` writes it, and runs for any reader's request".to_string()
            )]
        )]
    );
}

/// `owner user`, `owner session` and `owner anyone` each say who; with one,
/// the program checks clean (the control: nothing else refuses it).
#[test]
fn owner_says_who() {
    for owner in [
        "    owner user\n",
        "    owner session\n",
        "    owner anyone\n",
    ] {
        let notes = notes(owner);
        let app = app(FORGET);
        let found = diagnostics(&[("Notes.pw", &notes), ("app.pw", &app)]);
        assert!(found.is_empty(), "{owner}: {found:#?}");
    }
}

/// Each generated write is a write: `insert`, `put`, `delete` and
/// `delete_of_<fields>`. A read changes no row.
#[test]
fn each_write_counts_and_a_read_does_not() {
    let notes = notes("");
    for (body, writes) in [
        (
            "command add(note: Note) -> Result<(), Note>\n    requires Anyone\n{\n    Notes.insert(note)\n}\n",
            Some("Notes.insert(note)"),
        ),
        (
            "command keep(note: Note) -> ()\n    requires Anyone\n{\n    Notes.put(note)\n}\n",
            Some("Notes.put(note)"),
        ),
        (FORGET, Some("Notes.delete(shelf, id)")),
        (
            "command clear(shelf: Shelf) -> ()\n    requires Anyone\n{\n    Notes.delete_of_shelf(shelf)\n}\n",
            Some("Notes.delete_of_shelf(shelf)"),
        ),
        (
            "command peek(shelf: Shelf, id: NoteId) -> Option<Note>\n    requires Anyone\n{\n    \
             Notes.get(shelf, id)\n}\n",
            None,
        ),
    ] {
        let app = app(body);
        let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &app)]);
        match writes {
            Some(at) => {
                assert_eq!(found.len(), 1, "{body}: {found:#?}");
                assert_eq!(found[0].3[0].1, at, "{body}");
            }
            None => assert!(found.is_empty(), "{body}: {found:#?}"),
        }
    }
}

/// A write in what the command calls is the command's: its effects are
/// inferred through each call, and the call is named.
#[test]
fn a_write_through_a_helper_is_the_commands() {
    let notes = notes("");
    let app = app(
        "fn drop_note(shelf: Shelf, id: NoteId) -> () {\n    Notes.delete(shelf, id)\n}\n\n\
         command forget(shelf: Shelf, id: NoteId) -> ()\n    requires Anyone\n{\n    \
         drop_note(shelf, id)\n}\n",
    );
    let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &app)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].3,
        [(
            "app.pw".to_string(),
            "drop_note(shelf, id)".to_string(),
            "`forget` writes it, and runs for any reader's request".to_string()
        )]
    );
}

/// An order a payment settles: keyed by the order a payment names.
const ORDERS: &str = "module Orders\n\
\n\
type Order = Order { id: String, total: Int }\n\
\n\
table Orders\n    row Order\n    key id\nOWNER";

/// A table only the platform's commands write needs none: no reader runs
/// them (PW6401). The program checks clean (the control: nothing else
/// refuses it). Beside a reader's command, only the reader's is named.
#[test]
fn a_table_only_the_platform_writes_needs_no_owner() {
    let orders = ORDERS.replace("OWNER", "");
    let settle = "module pay\n\nimport payments.{ Settled }\nimport Orders\n\n\
                  opaque type InteractionId = String\n\n\
                  command settle(settled: Settled) -> Result<Int, String>\n    invoked_by    Payments\n    \
                  idempotent_by InteractionId\n{\n    Orders.delete(settled.order)\n    Ok(1)\n}\n";
    let found = diagnostics(&[("Orders.pw", &orders), ("pay.pw", settle)]);
    assert!(found.is_empty(), "{found:#?}");
    let beside = format!(
        "{settle}\ncommand cancel(order: String) -> ()\n    requires Anyone\n{{\n    \
         Orders.delete(order)\n}}\n"
    );
    let found = who_may_change(&[("Orders.pw", &orders), ("pay.pw", &beside)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    let named: Vec<&str> = found[0].3.iter().map(|n| n.2.as_str()).collect();
    assert_eq!(
        named,
        ["`cancel` writes it, and runs for any reader's request"]
    );
}

/// A command the clock invokes runs on the behalf of the reader whose
/// command scheduled it, with the arguments that reader's request gave
/// (`invoked.rs`): what it writes, the reader's command writes, told at the
/// `schedule`. Through each schedule it makes in turn too. Scheduled by no
/// reader's command, it is the platform's alone.
#[test]
fn a_write_a_readers_command_schedules_is_the_readers() {
    const CLOCKED: &str = "opaque type InteractionId = String\n\n\
        command forget(shelf: Shelf, id: NoteId) -> Result<Int, String>\n    invoked_by    Clock\n    \
        idempotent_by InteractionId\n{\n    Notes.delete(shelf, id)\n    Ok(1)\n}\n";
    const LATER: &str = "command forget_later(shelf: Shelf, id: NoteId) -> Result<Int, String> \
        !{ clock.schedule }\n    requires Anyone\n{\n    schedule forget(shelf, id) after minutes(30)\n    \
        Ok(1)\n}\n";
    let notes = notes("");
    let program = |body: &str| {
        app(body).replace("import Notes\n", "import Notes\nimport clock.{ minutes }\n")
    };
    // Scheduled by no reader's command: the platform's alone, and clean.
    let alone = program(CLOCKED);
    let found = diagnostics(&[("Notes.pw", &notes), ("app.pw", &alone)]);
    assert!(found.is_empty(), "{found:#?}");
    // Scheduled by a reader's command: the reader's write.
    let scheduled = program(&format!("{CLOCKED}\n{LATER}"));
    let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &scheduled)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].3,
        [(
            "app.pw".to_string(),
            "schedule forget(shelf, id) after minutes(30)".to_string(),
            "`forget_later` schedules `forget`, which writes it on its behalf, and runs for any \
             reader's request"
                .to_string()
        )]
    );
    // Through a schedule the clocked command makes in turn.
    let relay = "command relay(shelf: Shelf, id: NoteId) -> Result<Int, String> !{ clock.schedule }\n    \
                 invoked_by    Clock\n    idempotent_by InteractionId\n{\n    \
                 schedule forget(shelf, id) after minutes(5)\n    Ok(1)\n}\n";
    let later = LATER.replace("schedule forget(", "schedule relay(");
    let chained = program(&format!("{CLOCKED}\n{relay}\n{later}"));
    let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &chained)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].3[0]
            .2
            .starts_with("`forget_later` schedules `relay`"),
        "{found:#?}"
    );
    // The control: owned, the same program checks clean.
    let owned = crate::notes("    owner session\n");
    let found = diagnostics(&[("Notes.pw", &owned), ("app.pw", &scheduled)]);
    assert!(found.is_empty(), "{found:#?}");
}

/// **A contract says for whom the clock runs a command** (the review of
/// 2026-11-07): `reader` where a reader's command schedules it, `platform`
/// where the command the platform invokes does, both where both do, each
/// through a schedule a run makes in turn; nothing where no command
/// schedules it, and nothing on a command the clock does not run. A host
/// holds each run's writer to it (`pw_host::tables::Writer::admits`).
#[test]
fn a_contract_says_for_whom_the_clock_runs_a_command() {
    let clocked = |name: &str, then: &str| {
        let effects = if then.is_empty() {
            ""
        } else {
            " !{ clock.schedule }"
        };
        format!(
            "command {name}(order: String) -> Result<Int, String>{effects}\n    invoked_by    \
             Clock\n    idempotent_by InteractionId\n{{\n{then}    Ok(1)\n}}\n\n"
        )
    };
    let source = format!(
        "module pay\n\nimport payments.{{ Settled }}\nimport clock.{{ minutes }}\n\n\
         opaque type InteractionId = String\n\n\
         command settle(settled: Settled) -> Result<Int, String> !{{ clock.schedule }}\n    \
         invoked_by    Payments\n    idempotent_by InteractionId\n{{\n    \
         schedule send(settled.order) after minutes(5)\n    \
         schedule refund(settled.order) after minutes(5)\n    Ok(1)\n}}\n\n\
         command place(order: String) -> Result<Int, String> !{{ clock.schedule }}\n    \
         requires Anyone\n{{\n    schedule send(order) after minutes(5)\n    \
         schedule expire(order) after minutes(30)\n    Ok(1)\n}}\n\n{}{}{}{}{}",
        clocked("send", ""),
        clocked("refund", ""),
        clocked("expire", "    schedule purge(order) after minutes(5)\n"),
        clocked("purge", ""),
        clocked("idle", ""),
    );
    let found = diagnostics(&[("pay.pw", &source)]);
    assert!(found.is_empty(), "the control: it checks clean: {found:#?}");
    let files = program(&[("pay.pw", &source)]);
    let build = pw_core::build::build(&units(&files)).expect("the program builds");
    let runs_for = |id: &str| -> Vec<String> {
        let export = build
            .contracts
            .iter()
            .find(|c| c.component_id == id)
            .and_then(|c| c.exports.first())
            .and_then(|e| e.component.clone());
        assert!(export.is_some(), "no command export for {id}");
        export.map(|c| c.runs_for).unwrap_or_default()
    };
    assert_eq!(runs_for("pay.send"), ["platform", "reader"]);
    assert_eq!(runs_for("pay.refund"), ["platform"]);
    assert_eq!(runs_for("pay.expire"), ["reader"]);
    assert_eq!(
        runs_for("pay.purge"),
        ["reader"],
        "through the schedule a reader's run makes"
    );
    assert!(runs_for("pay.idle").is_empty(), "no command schedules it");
    assert!(
        runs_for("pay.place").is_empty(),
        "the clock does not run it"
    );
    assert!(
        runs_for("pay.settle").is_empty(),
        "the clock does not run it"
    );
}

/// One refusal a table, beside every command that writes it; a table in the
/// command's own module is told there.
#[test]
fn one_refusal_a_table_beside_each_command() {
    let notes = notes("");
    let app = app(&format!(
        "{FORGET}\ncommand keep(note: Note) -> ()\n    requires Anyone\n{{\n    Notes.put(note)\n}}\n"
    ));
    let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &app)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    let named: Vec<&str> = found[0].3.iter().map(|n| n.2.as_str()).collect();
    assert_eq!(
        named,
        [
            "`forget` writes it, and runs for any reader's request",
            "`keep` writes it, and runs for any reader's request",
        ]
    );
    // In one module: the table's own file, the command's note in it.
    let own = format!("{notes}\n{FORGET}");
    let found = who_may_change(&[("Notes.pw", &own)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].3[0].0, "Notes.pw");
    assert_eq!(found[0].3[0].1, "Notes.delete(shelf, id)");
}

/// `owner` is a table's clause, of three words: on a command it is PW5105's,
/// and a word no owner is PW0335's, told once (no PW0370 beside it).
#[test]
fn owner_is_a_tables_clause_of_three_words() {
    let on_a_command = app(
        "command forget(shelf: Shelf, id: NoteId) -> ()\n    requires Anyone\n    owner user\n{\n    \
         Notes.delete(shelf, id)\n}\n",
    );
    let notes_ok = notes("    owner user\n");
    let found = diagnostics(&[("Notes.pw", &notes_ok), ("app.pw", &on_a_command)]);
    let codes: Vec<&str> = found.iter().map(|(_, d)| d.code).collect();
    assert_eq!(codes, ["PW5105"], "{found:#?}");

    let everyone = notes("    owner everyone\n");
    let app = app(FORGET);
    let found = diagnostics(&[("Notes.pw", &everyone), ("app.pw", &app)]);
    let codes: Vec<&str> = found.iter().map(|(_, d)| d.code).collect();
    assert_eq!(codes, ["PW0335"], "{found:#?}");
}

// --- what a database records ---------------------------------------------------

/// The schema the build declares, as `schema.lock` writes it.
fn released(notes: &str) -> Schema {
    let files = program(&[("Notes.pw", notes)]);
    let build = pw_core::build::build(&units(&files)).expect("the release builds");
    Schema::read(&build.schema.written()).expect("a lock reads back")
}

/// `owner user` and `owner session` are recorded in the schema, and in
/// tables.json, which flattens it; `owner anyone` records nothing, so a table
/// that says it keeps the schema, and the hash, it had before.
#[test]
fn the_schema_records_who_owns_each_row() {
    let owner = |o: &str| released(&notes(o)).tables[0].owner;
    assert_eq!(owner("    owner user\n"), Some(OwnerKind::User));
    assert_eq!(owner("    owner session\n"), Some(OwnerKind::Session));
    assert_eq!(owner("    owner anyone\n"), None);
    assert_eq!(
        released(&notes("    owner anyone\n")).hash,
        released(&notes("")).hash,
        "`owner anyone` changes no row's storage"
    );
    assert_ne!(
        released(&notes("    owner session\n")).hash,
        released(&notes("")).hash
    );
    let files = program(&[("Notes.pw", &notes("    owner session\n"))]);
    let build = pw_core::build::build(&units(&files)).expect("builds");
    let json = serde_json::to_value(&build.tables).expect("json");
    assert_eq!(json["tables"][0]["owner"], "session");
    let files = program(&[("Notes.pw", &notes("    owner anyone\n"))]);
    let build = pw_core::build::build(&units(&files)).expect("builds");
    let json = serde_json::to_value(&build.tables).expect("json");
    assert_eq!(json["tables"][0].get("owner"), None);
}

/// The steps from `before`, released, to `after`, and PW0366's refusals.
fn diff(before: &str, after: &str) -> (Vec<Step>, Vec<String>) {
    let lock = released(before);
    let refused = check_sources_with(&program(&[("Notes.pw", after)]), Some(&lock))
        .into_iter()
        .filter(|(n, _)| n == "Notes.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.code == "PW0366")
        .map(|d| d.message)
        .collect();
    let files = program(&[("Notes.pw", after)]);
    let units = units(&files);
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let (plan, _) = pw_core::tables::held_to(&hirs, &ws, &sigs, Some(&lock));
    (plan.steps, refused)
}

/// What PW0366 says of a table released `owner session` that records none.
const LOST: &str = "`Notes` was released `owner session` and records no owner now: the owner \
                    each row records would be lost";

/// A table released without owners comes to record them: a step, each row
/// released owned by no reader. One released with them gives them up only as
/// the program states it, `migrate owner session dropped` (the review of
/// 2026-11-07): the owners are lost for good, and any reader may change any
/// row, as a field's values are lost where it is dropped. One whose rows
/// recorded a user comes to record a session (or the other way): each owner
/// recorded would be read as another kind of reader, refused.
#[test]
fn a_change_of_owner_is_a_step_and_of_its_kind_refused() {
    let none = notes("");
    let user = notes("    owner user\n");
    let session = notes("    owner session\n");
    let anyone = notes("    owner anyone\n");
    let change = |from, to| Step::ChangeOwner {
        table: "Notes".to_string(),
        from,
        to,
    };
    assert_eq!(
        diff(&none, &session),
        (vec![change(None, Some(OwnerKind::Session))], Vec::new())
    );
    assert_eq!(
        diff(&anyone, &user),
        (vec![change(None, Some(OwnerKind::User))], Vec::new())
    );
    // Given up: no step, and refused, where nothing states it; a step where
    // the program does.
    assert_eq!(
        diff(&session, &anyone),
        (Vec::new(), vec![LOST.to_string()])
    );
    let (steps, refused) = diff(&user, &none);
    assert!(steps.is_empty(), "{steps:#?}");
    assert_eq!(refused.len(), 1, "with no `owner` at all: {refused:#?}");
    let stated = notes("    owner anyone\n    migrate owner session dropped\n");
    assert_eq!(
        diff(&session, &stated),
        (vec![change(Some(OwnerKind::Session), None)], Vec::new())
    );
    // The control: the same owner released and declared changes nothing.
    assert_eq!(diff(&user, &user), (Vec::new(), Vec::new()));
    assert_eq!(diff(&anyone, &none), (Vec::new(), Vec::new()));
    let (_, refused) = diff(&user, &session);
    assert_eq!(
        refused,
        [
            "`Notes` was released `owner user` and is `owner session`: each row's recorded owner \
          would be read as another kind of reader"
        ]
    );
    let (_, refused) = diff(&session, &user);
    assert_eq!(refused.len(), 1, "{refused:#?}");
}

/// **The owners given up are stated of the kind released** (the review of
/// 2026-11-07). `migrate owner user dropped` gives up no session's owners,
/// and states no change of a table released without owners; beside an
/// owner the table still records, it is malformed (PW0361), told once; and
/// `owner anyone` records none to give up. A field named `owner` keeps
/// statements of its own beside it: the two change different things.
#[test]
fn the_owners_given_up_are_stated_of_the_kind_released() {
    let session = notes("    owner session\n");
    let anyone = notes("    owner anyone\n");
    let stale = |kind: &str| {
        format!(
            "`migrate owner {kind} dropped` states no change of `Notes` from the schema released"
        )
    };
    let (steps, refused) = diff(
        &session,
        &notes("    owner anyone\n    migrate owner user dropped\n"),
    );
    assert!(steps.is_empty(), "{steps:#?}");
    assert_eq!(refused, [LOST.to_string(), stale("user")]);
    let (steps, refused) = diff(
        &anyone,
        &notes("    owner anyone\n    migrate owner session dropped\n"),
    );
    assert!(steps.is_empty(), "{steps:#?}");
    assert_eq!(refused, [stale("session")]);
    // Malformed, alone, released or not.
    let still = notes("    owner session\n    migrate owner session dropped\n");
    let told = |found: Vec<(String, pw_core::diagnostics::Diagnostic)>| -> Vec<(String, String)> {
        found
            .into_iter()
            .map(|(_, d)| (d.code.to_string(), d.message))
            .collect()
    };
    let malformed = (
        "PW0361".to_string(),
        "`migrate owner session dropped` gives up the owners `Notes` still records: it says \
         `owner session`"
            .to_string(),
    );
    assert_eq!(
        told(diagnostics(&[("Notes.pw", &still)])),
        std::slice::from_ref(&malformed)
    );
    let against = check_sources_with(&program(&[("Notes.pw", &still)]), Some(&released(&session)))
        .into_iter()
        .filter(|(n, _)| n == "Notes.pw")
        .flat_map(|(n, ds)| ds.into_iter().map(move |d| (n.clone(), d)))
        .collect();
    assert_eq!(told(against), [malformed]);
    let nothing = notes("    owner anyone\n    migrate owner anyone dropped\n");
    let found = told(diagnostics(&[("Notes.pw", &nothing)]));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .1
            .ends_with("gives up no owner: `owner anyone` records none"),
        "{found:#?}"
    );
    // A field named `owner`, dropped as the owners are given up.
    let with_field = |owner: &str| {
        notes(owner).replace(
            "type Note = Note { shelf: Shelf, id: NoteId, text: String }",
            "type Note = Note { shelf: Shelf, id: NoteId, text: String, owner: String }",
        )
    };
    let after =
        notes("    owner anyone\n    migrate owner dropped\n    migrate owner session dropped\n");
    assert_eq!(
        diff(&with_field("    owner session\n"), &after),
        (
            vec![
                Step::DropField {
                    table: "Notes".to_string(),
                    field: "owner".to_string(),
                },
                Step::ChangeOwner {
                    table: "Notes".to_string(),
                    from: Some(OwnerKind::Session),
                    to: None,
                },
            ],
            Vec::new()
        )
    );
}
