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

/// A table only the platform's commands write needs none: no reader runs
/// them (PW6401). Beside a reader's command, only the reader's is named.
#[test]
fn a_table_only_the_platform_writes_needs_no_owner() {
    let notes = notes("");
    let platform = "opaque type InteractionId = String\n\n\
                    command sweep(shelf: Shelf) -> Result<Int, String>\n    invoked_by    Payments\n    \
                    idempotent_by InteractionId\n{\n    Notes.delete_of_shelf(shelf)\n    Ok(1)\n}\n";
    let alone = app(platform);
    assert_eq!(
        who_may_change(&[("Notes.pw", &notes), ("app.pw", &alone)]),
        [],
        "the platform's command alone"
    );
    let beside = app(&format!("{platform}\n{FORGET}"));
    let found = who_may_change(&[("Notes.pw", &notes), ("app.pw", &beside)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    let named: Vec<&str> = found[0].3.iter().map(|n| n.2.as_str()).collect();
    assert_eq!(
        named,
        ["`forget` writes it, and runs for any reader's request"]
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

/// A table released without owners comes to record them: a step, each row
/// released owned by no reader. One released with them gives them up: a
/// step too. One whose rows recorded a user comes to record a session (or
/// the other way): each owner recorded would be read as another kind of
/// reader, refused.
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
    assert_eq!(
        diff(&session, &anyone),
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
