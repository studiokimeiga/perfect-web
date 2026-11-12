//! **Whose a table's rows are** (track `row-ownership`, ADR-0384's H1): an
//! owned table's clauses, the reads and writes it gives, each by the
//! reader's handle in the place of the field it leads with, the seen row a
//! reader it shares with is answered, `tables.json`'s owners, and each
//! refusal beside its valid neighbour: PW0368 to PW0370, and the ways a
//! program could reach another's rows that the checker refuses (a plain id,
//! PW0605; a forged handle, PW5037; a shared cache, PW5001; a read of every
//! row, PW0368; a binding by hand, PW0362).
//!
//! The fixture is `examples/owned/`. Recorded by `just e14-row-ownership`.

use pw_core::check::{Unit, check_sources};
use pw_core::diagnostics::Diagnostic;

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

fn fixture(name: &str) -> String {
    std::fs::read_to_string(root().join("examples/owned").join(name)).expect("fixture")
}

/// A domain result held by an assertion, so a test fails at an `assert!`
/// (the mutation producer's native assertion rule), never at an `expect`.
fn held<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    assert!(result.is_ok(), "{what}: {:?}", result.as_ref().err());
    match result {
        Ok(value) => value,
        Err(_) => unreachable!(),
    }
}

/// An optional domain value held by an assertion.
fn found_one<T>(value: Option<T>, what: &str) -> T {
    assert!(value.is_some(), "{what}");
    match value {
        Some(value) => value,
        None => unreachable!(),
    }
}

/// The platform, the fixture's table and source, and `files`: the fixture's
/// `app.pw` only where `files` names it, since a probe of a changed table
/// is the table's, not the app's.
fn program(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = platform();
    for f in ["Notes.pw", "NoteData.pw"] {
        if !files.iter().any(|(n, _)| *n == f) {
            out.push((f.to_string(), fixture(f)));
        }
    }
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.to_string())));
    out
}

/// The diagnostics of the fixture's files and `files`.
fn diagnostics(files: &[(&str, &str)]) -> Vec<Diagnostic> {
    check_sources(&program(files))
        .into_iter()
        .filter(|(n, _)| !n.starts_with("packages/"))
        .flat_map(|(_, d)| d)
        .collect()
}

fn found(files: &[(&str, &str)]) -> Vec<(String, String)> {
    diagnostics(files)
        .into_iter()
        .map(|d| (d.code.to_string(), d.message))
        .collect()
}

fn assert_refused(files: &[(&str, &str)], code: &str, says: &str) {
    let got = found(files);
    assert!(
        got.iter().any(|(c, m)| c == code && m.contains(says)),
        "expected {code} saying {says:?}, got {got:#?}"
    );
}

fn assert_clean(files: &[(&str, &str)]) {
    let got = found(files);
    assert!(got.is_empty(), "expected no diagnostics, got {got:#?}");
}

/// `Notes.pw` with `from` written `to`.
fn notes(from: &str, to: &str) -> String {
    let src = fixture("Notes.pw");
    assert!(src.contains(from), "{from:?} is not in Notes.pw");
    src.replacen(from, to, 1)
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

// --- the declaration --------------------------------------------------------

#[test]
fn the_fixture_checks_clean_and_reads_each_clause() {
    assert_clean(&[("app.pw", &fixture("app.pw"))]);
    let units = units(&[("Notes.pw".to_string(), fixture("Notes.pw"))]);
    let t = &pw_core::tables::declared(&units[0].hir)[0];
    assert_eq!(t.owner_field(), Some("author"));
    assert_eq!(t.sharers(), ["to"]);
    assert_eq!(t.private_fields(), ["draft"]);
    assert!(t.unread.is_empty(), "{:?}", t.unread);
}

#[test]
fn each_clause_is_read_as_its_grammar_says() {
    assert_refused(
        &[("Notes.pw", &notes("owned   by author", "owned   author"))],
        "PW0361",
        "`owned by <field>`",
    );
    assert_refused(
        &[("Notes.pw", &notes("shares  with to", "shares  to"))],
        "PW0361",
        "`shares with <fields>`",
    );
    assert_refused(
        &[("Notes.pw", &notes("private draft", "private draft, draft"))],
        "PW0361",
        "names `draft` twice",
    );
}

// --- what it gives ------------------------------------------------------------

/// Each generated declaration of `Notes`: its name and, for a function, its
/// signature as the checker resolves it, its effects and its import.
fn generated(src: &str) -> Vec<(String, String, Vec<String>, String)> {
    let mut files = platform();
    files.push(("Notes.pw".to_string(), src.to_string()));
    let units = units(&files);
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = pw_core::resolve::Workspace::build(&hirs);
    let sigs = pw_core::signatures::Signatures::build(&ws, &hirs);
    let unit = units.len() - 1;
    let hir = &units[unit].hir;
    hir.generated
        .keys()
        .map(|id| {
            let d = hir.decl(*id);
            let Some(sig) = sigs.by_def(pw_core::resolve::DefId { unit, decl: id.0 }) else {
                let fields = d
                    .fields
                    .iter()
                    .flatten()
                    .map(|f| f.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                return (d.name.clone(), fields, Vec::new(), String::new());
            };
            let shown = |t: Option<&pw_core::resolved::TypeResolution>| {
                t.map_or("?".to_string(), ToString::to_string)
            };
            (
                d.name.clone(),
                format!(
                    "({}) -> {}",
                    sig.params
                        .iter()
                        .map(|p| shown(p.as_ref()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    shown(sig.returns.as_ref())
                ),
                d.declared_effects
                    .iter()
                    .flatten()
                    .map(|e| e.written.clone())
                    .collect(),
                d.policy("host")
                    .map(|p| p.value.clone())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn an_owned_table_gives_each_read_and_write_by_its_readers_handle() {
    let got = generated(&fixture("Notes.pw"));
    let read = vec!["database.read<Notes>".to_string()];
    let write = vec!["database.write<Notes>".to_string()];
    let none = Vec::new();
    let handle = "capability.User<UserId>";
    let want: Vec<(String, String, &Vec<String>, &str)> = vec![
        (
            "NoteSeen".into(),
            "author, id, to, text, added".into(),
            &none,
            "",
        ),
        (
            "get".into(),
            format!("({handle}, NoteId) -> Option<Note>"),
            &read,
            "\"pw:tables/notes#get\"",
        ),
        (
            "get_many".into(),
            format!("({handle}, List<NoteId>) -> List<Note>"),
            &read,
            "\"pw:tables/notes#get-many\"",
        ),
        (
            "of_author".into(),
            format!("({handle}) -> List<Note>"),
            &read,
            "\"pw:tables/notes#of-author\"",
        ),
        (
            "by_to".into(),
            format!("({handle}) -> List<NoteSeen>"),
            &read,
            "\"pw:tables/notes#by-to\"",
        ),
        (
            "insert".into(),
            format!("({handle}, Note) -> Result<(), Note>"),
            &write,
            "\"pw:tables/notes#insert\"",
        ),
        (
            "put".into(),
            format!("({handle}, Note) -> ()"),
            &write,
            "\"pw:tables/notes#put\"",
        ),
        (
            "delete".into(),
            format!("({handle}, NoteId) -> ()"),
            &write,
            "\"pw:tables/notes#delete\"",
        ),
        (
            "delete_of_author".into(),
            format!("({handle}) -> ()"),
            &write,
            "\"pw:tables/notes#delete-of-author\"",
        ),
    ];
    let want: Vec<(String, String, Vec<String>, String)> = want
        .into_iter()
        .map(|(n, s, e, h)| (n, s, e.clone(), h.to_string()))
        .collect();
    assert_eq!(got, want, "{got:#?}");
    // No set form where nothing follows the reader, and no `all`, though a
    // bound would give an unowned table one.
    let bounded = generated(&notes(
        "    private draft\n",
        "    private draft\n    bounded 50\n",
    ));
    for missing in ["of_author_many", "by_to_many", "all"] {
        assert!(
            !bounded.iter().any(|(n, ..)| n == missing),
            "`{missing}` is given: {bounded:#?}"
        );
    }
}

#[test]
fn a_set_form_takes_the_handle_and_a_list_of_what_follows_it() {
    // A key of three fields: the prefix read by the owner and the next
    // field has a set form over that field, the handle beside its list; an
    // index a sharer reads by two fields, a set form over the second.
    let src = notes(
        "type Note = Note { author: UserId, id: NoteId, to: UserId, text: Text, draft: Text, added: Int }",
        "type Note = Note { author: UserId, shelf: Int, id: NoteId, to: UserId, text: Text, draft: Text, added: Int }",
    )
    .replacen("key     author, id", "key     author, shelf, id", 1)
    .replacen("index   by_to on to", "index   by_to on to, shelf", 1);
    let got = generated(&src);
    let sig = |name: &str| {
        found_one(
            got.iter()
                .find(|(n, ..)| n == name)
                .map(|(_, s, ..)| s.clone()),
            &format!("`{name}` is not given: {got:#?}"),
        )
    };
    let handle = "capability.User<UserId>";
    assert_eq!(
        sig("get_many"),
        format!("({handle}, List<NotesShelfId>) -> List<Note>")
    );
    assert_eq!(
        sig("of_author_shelf_many"),
        format!("({handle}, List<Int>) -> List<Note>")
    );
    assert_eq!(
        sig("by_to_many"),
        format!("({handle}, List<Int>) -> List<NoteSeen>")
    );
    assert_eq!(
        sig("delete_of_author_shelf"),
        format!("({handle}, Int) -> ()")
    );
    // The tuple the key's set form holds is what follows the reader.
    assert_eq!(
        got.iter()
            .find(|(n, ..)| n == "NotesShelfId")
            .map(|(_, f, ..)| f.as_str()),
        Some("shelf, id")
    );
}

#[test]
fn without_a_private_field_a_sharer_is_answered_the_row() {
    let got = generated(&notes("    private draft\n", ""));
    assert!(!got.iter().any(|(n, ..)| n == "NoteSeen"), "{got:#?}");
    assert!(
        got.iter()
            .any(|(n, s, ..)| n == "by_to" && s.ends_with("-> List<Note>")),
        "{got:#?}"
    );
}

#[test]
fn an_unowned_table_gives_what_it_gave() {
    // The same table with no ownership is D1's, plain ids and `all`.
    let src = notes(
        "    owned   by author\n    shares  with to\n    private draft\n",
        "    bounded 50\n",
    );
    let got = generated(&src);
    let sig = |name: &str| {
        found_one(
            got.iter()
                .find(|(n, ..)| n == name)
                .map(|(_, s, ..)| s.clone()),
            &format!("`{name}` is not given: {got:#?}"),
        )
    };
    assert_eq!(sig("get"), "(UserId, NoteId) -> Option<Note>");
    assert_eq!(sig("by_to"), "(UserId) -> List<Note>");
    assert_eq!(sig("insert"), "(Note) -> Result<(), Note>");
    assert_eq!(sig("all"), "() -> List<Note>");
}

#[test]
fn tables_json_states_whose_rows_and_each_functions_reader() {
    let files = program(&[("app.pw", &fixture("app.pw"))]);
    let build = held(pw_core::build::build(&units(&files)), "the fixture builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let json = &build.tables;
    let notes = json.tables.iter().find(|t| t.schema.name == "Notes");
    let notes = found_one(notes, "Notes");
    assert_eq!(notes.owner.as_deref(), Some("author"));
    assert_eq!(notes.shares, ["to"]);
    assert_eq!(notes.private, ["draft"]);
    let readers: Vec<(&str, Option<&str>)> = notes
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.reader.as_deref()))
        .collect();
    assert_eq!(
        readers,
        [
            ("get", Some("author")),
            ("get_many", Some("author")),
            ("of_author", Some("author")),
            ("by_to", Some("to")),
            ("insert", Some("author")),
            ("put", Some("author")),
            ("delete", Some("author")),
            ("delete_of_author", Some("author")),
        ]
    );
    // Who reads a row is no part of what is stored: the schema, and so its
    // hash and `schema.lock`, are a plain table's.
    let text = serde_json::to_string(&notes.schema).expect("schema");
    for word in ["owner", "shares", "private", "reader"] {
        assert!(!text.contains(word), "the schema holds `{word}`: {text}");
    }
    // Every component the fixture's queries and commands make builds, each
    // import a table's.
    for c in [
        "owned.app.Mine",
        "owned.app.One",
        "owned.app.ToMe",
        "owned.app.write",
        "owned.app.keep",
        "owned.app.remove",
        "owned.app.forget",
    ] {
        assert!(
            build.components.iter().any(|(id, b)| id == c
                && matches!(b, pw_core::backend::component::Built::Component { .. })),
            "{c} is not built"
        );
    }
    // A handle crosses as its user's id (ADR-0270), in the owner's place.
    assert!(
        build
            .wit
            .contains("of-author: func(arg0: capability-user-id) -> list<notes-note>;"),
        "{}",
        build.wit
    );
    assert!(
        build
            .wit
            .contains("by-to: func(arg0: capability-user-id) -> list<notes-note-seen>;"),
        "{}",
        build.wit
    );
}

// --- what no program can do -----------------------------------------------------

const PROBE: &str = "module probe\n\nimport capability.{ User, UserId }\nimport context.{ current_user }\nimport Notes.{ Note }\nimport Notes\n\n";

fn probe(body: &str) -> String {
    format!("{PROBE}{body}")
}

#[test]
fn no_program_reads_anothers_rows_by_naming_an_id() {
    // A plain id where the reader's handle goes: a type the read does not
    // take (PW0605).
    assert_refused(
        &[(
            "probe.pw",
            &probe(
                "query ById() -> List<Note>\n    consistency strong\n{\n    Notes.of_author(UserId(\"bob\"))\n}\n",
            ),
        )],
        "PW0605",
        "argument 1 of `Notes.of_author` is declared `capability.User<capability.UserId>`",
    );
    // A handle the program makes: the platform's alone (PW5037).
    assert_refused(
        &[(
            "probe.pw",
            &probe(
                "query Forged() -> List<Note>\n    consistency strong\n{\n    Notes.of_author(User(\"bob\"))\n}\n",
            ),
        )],
        "PW5037",
        "a user's handle is the platform's to make",
    );
    // A sharer's read by an id: the same.
    assert_refused(
        &[(
            "probe.pw",
            &probe(
                "query Sent() -> List<Notes.NoteSeen>\n    consistency strong\n{\n    Notes.by_to(UserId(\"bob\"))\n}\n",
            ),
        )],
        "PW0605",
        "argument 1 of `Notes.by_to`",
    );
    // A write of another's key: the handle in the owner's place.
    assert_refused(
        &[(
            "probe.pw",
            &probe(
                "command drop(id: Notes.NoteId) -> ()\n    requires    Anyone\n    transaction serializable\n{\n    Notes.delete(UserId(\"bob\"), id)\n}\n",
            ),
        )],
        "PW0605",
        "argument 1 of `Notes.delete`",
    );
    // The neighbour: the reader's own, by the platform's handle.
    assert_clean(&[(
        "probe.pw",
        &probe(
            "user query Own(reader: User<UserId>) -> List<Note>\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key reader\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{\n    Notes.of_author(reader)\n}\n",
        ),
    )]);
}

#[test]
fn an_owned_read_in_a_shared_cache_is_pw5001() {
    // ADR-0384's PW6902 is PW5001 for a table: the handle labels the read
    // its reader's (ADR-0129), so one defect is told once.
    let shared = probe(
        "query Shared() -> List<Note>\n    consistency strong\n    cache       shared\n{\n    Notes.of_author(current_user())\n}\n",
    );
    assert_refused(
        &[("probe.pw", &shared)],
        "PW5001",
        "`Shared` is User<UserId> and declares a shared cache",
    );
    assert!(
        !found(&[("probe.pw", &shared)])
            .iter()
            .any(|(c, _)| c.starts_with("PW69")),
        "told twice"
    );
    assert_clean(&[(
        "probe.pw",
        &shared.replace("cache       shared", "cache       private"),
    )]);
}

#[test]
fn pw0368_an_owned_table_is_never_read_whole() {
    let bounded = notes("    private draft\n", "    private draft\n    bounded 50\n");
    assert_refused(
        &[
            ("Notes.pw", &bounded),
            (
                "probe.pw",
                &probe(
                    "query Whole() -> List<Note>\n    consistency strong\n{\n    Notes.all()\n}\n",
                ),
            ),
        ],
        "PW0368",
        "`Notes.all()` reads every row of `Notes`, whose rows are each their owner's",
    );
}

#[test]
fn pw0368_an_owned_table_states_no_bound_across_its_owners() {
    // The review of the track, finding 2 (probe B): `bounded 3` on votes
    // owned by their voter counted every voter's, so eve's three refused
    // alice's first and told her how many there were.
    let bounded = notes("    private draft\n", "    private draft\n    bounded 50\n");
    assert_refused(
        &[("Notes.pw", &bounded)],
        "PW0368",
        "`bounded 50` counts every owner's rows of `Notes`: one owner's rows would refuse \
         another's write",
    );
    let got = found(&[("Notes.pw", &bounded)]);
    assert_eq!(got.len(), 1, "one defect, told once: {got:#?}");
    // The neighbours: the owned table with no bound, and a bound of a table
    // no one owns (D1's).
    assert_clean(&[("Notes.pw", &fixture("Notes.pw"))]);
    assert_clean(&[(
        "Notes.pw",
        &notes(
            "    owned   by author\n    shares  with to\n    private draft\n",
            "    bounded 50\n",
        ),
    )]);
}

// --- PW6924: a reference stays within one owner's rows ----------------------------

/// An owned table keyed by its owner alone: what a reference may name.
const PREMIUMS: &str = "module Premiums\n\nimport capability.{ UserId }\n\ntype Premium = Premium { user: UserId, tier: Int }\n\ntable Premiums\n    row     Premium\n    key     user\n    owned   by user\n";

/// A table no one owns, keyed by its id.
const TOPICS: &str = "module Topics\n\nopaque type TopicId = String\n\ntype Topic = Topic { id: TopicId, title: String }\n\ntable Topics\n    row     Topic\n    key     id\n";

/// Crushes, owned by `from`, with `clauses` after its key.
fn crushes(clauses: &str) -> String {
    format!(
        "module Crushes\n\nimport capability.{{ UserId }}\nimport Premiums\n\ntype Crush = Crush {{ from: UserId, target: UserId, added: Int }}\n\ntable Crushes\n    row     Crush\n    key     from, target\n{clauses}"
    )
}

/// The diagnostics PW6924 told, by message.
fn within_one_owner(files: &[(&str, &str)]) -> Vec<String> {
    found(files)
        .into_iter()
        .filter(|(c, _)| c == "PW6924")
        .map(|(_, m)| m)
        .collect()
}

#[test]
fn pw6924_a_reference_into_an_owned_table_is_made_from_its_owners_field() {
    // The review's finding 1 (probe A): eve's crush naming bob's premium
    // row committed only where bob had one, and bob could not delete his
    // while it named him.
    let refused = crushes("    owned   by from\n    refers  target to Premiums\n");
    let told = within_one_owner(&[("Premiums.pw", PREMIUMS), ("Crushes.pw", &refused)]);
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains(
            "`refers target to Premiums`: `Premiums`'s rows are each owned by their `user`, \
             and `target` is not `Crushes`'s owner, `from`"
        ),
        "{told:#?}"
    );
    // A table no one owns, referring into an owned one.
    let public = crushes("    refers  target to Premiums\n");
    let told = within_one_owner(&[("Premiums.pw", PREMIUMS), ("Crushes.pw", &public)]);
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains("and `Crushes`'s rows are no one's"),
        "{told:#?}"
    );
    // The neighbour: the reference from the owner's own field names the
    // owner's own row, and checks clean.
    let within = crushes("    owned   by from\n    refers  from to Premiums\n");
    assert_clean(&[("Premiums.pw", PREMIUMS), ("Crushes.pw", &within)]);
}

#[test]
fn pw6924_an_owned_table_refers_to_no_table_no_one_owns() {
    // Deleting a topic would be refused while a private note names it,
    // which tells whoever deletes it that the note exists.
    let jottings = |clauses: &str| {
        format!(
            "module Jottings\n\nimport capability.{{ UserId }}\nimport Topics.{{ TopicId }}\nimport Topics\n\nopaque type JottingId = String\n\ntype Jotting = Jotting {{ author: UserId, id: JottingId, topic: TopicId }}\n\ntable Jottings\n    row     Jotting\n    key     author, id\n{clauses}    refers  topic to Topics\n"
        )
    };
    let owned = jottings("    owned   by author\n");
    let told = within_one_owner(&[("Topics.pw", TOPICS), ("Jottings.pw", &owned)]);
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains(
            "`Jottings`'s rows are each owned by their `author`, and `Topics`'s are no one's: \
             deleting a row of `Topics` would be refused while an owner's private row names it"
        ),
        "{told:#?}"
    );
    // The neighbour: a table no one owns referring to one no one owns is
    // D1's reference, held by the database.
    assert_clean(&[("Topics.pw", TOPICS), ("Jottings.pw", &jottings(""))]);
}

#[test]
fn pw0362_a_binding_by_hand_reaches_no_owned_row() {
    assert_refused(
        &[(
            "probe.pw",
            &probe(
                "fn raw(author: UserId) -> List<Note> !{ database.read<Notes> }\n    host \"pw:tables/notes#of-author\"\n",
            ),
        )],
        "PW0362",
        "binds `pw:tables/notes#of-author` by hand",
    );
}

// --- PW0370: whose its rows are ------------------------------------------------

#[test]
fn pw0370_owned_by_names_a_field_holding_a_principals_id() {
    assert_refused(
        &[("Notes.pw", &notes("owned   by author", "owned   by writer"))],
        "PW0370",
        "`owned by writer`: `writer` is no field of `Note`",
    );
    assert_refused(
        &[(
            "Notes.pw",
            &notes("owned   by author", "owned   by text").replacen(
                "key     author, id",
                "key     text, id",
                1,
            ),
        )],
        "PW0370",
        "`text` is a `Text`, and a principal is named by its id",
    );
}

#[test]
fn pw0370_shares_with_names_others_fields_holding_principals_ids() {
    assert_refused(
        &[("Notes.pw", &notes("shares  with to", "shares  with author"))],
        "PW0370",
        "names `author`, its owner",
    );
    assert_refused(
        &[("Notes.pw", &notes("shares  with to", "shares  with text"))],
        "PW0370",
        "`shares with text`: `text` is a `Text`",
    );
    // A predicate deciding who reads is H3's `shares when`: said, not built.
    assert_refused(
        &[(
            "Notes.pw",
            &notes("shares  with to", "shares  with Matched"),
        )],
        "PW0370",
        "ADR-0384's `shares when`, track H3",
    );
    // Shares, and no one owns its rows.
    assert_refused(
        &[(
            "Notes.pw",
            &notes("    owned   by author\n", "").replacen("    private draft\n", "", 1),
        )],
        "PW0370",
        "shares its rows and no one owns them",
    );
}

// --- PW0368: every read by its reader -------------------------------------------

#[test]
fn pw0368_every_read_of_an_owned_table_leads_with_a_reader() {
    assert_refused(
        &[(
            "Notes.pw",
            &notes("key     author, id", "key     id, author"),
        )],
        "PW0368",
        "`key id, author` leads with `id`",
    );
    assert_refused(
        &[(
            "Notes.pw",
            &notes(
                "index   by_to on to",
                "index   by_to on to\n    index   by_added on added",
            ),
        )],
        "PW0368",
        "`index by_added on added` leads with `added`",
    );
    assert_refused(
        &[(
            "Notes.pw",
            &notes(
                "index   by_to on to",
                "index   by_to on to\n    unique  by_text on to, text",
            ),
        )],
        "PW0368",
        "`unique by_text on to, text` holds no `author`",
    );
    // The neighbours: an index by the owner, one by a sharer, and a unique
    // index per owner.
    assert_clean(&[(
        "Notes.pw",
        &notes(
            "index   by_to on to",
            "index   by_to on to\n    index   by_author_added on author, added\n    unique  by_to_author_text on to, author, text",
        ),
    )]);
}

// --- PW0369: what its owner alone reads -----------------------------------------

#[test]
fn pw0369_a_private_field_is_one_no_other_reader_reads_rows_by() {
    for (private, says) in [
        ("author", "names its owner"),
        ("to", "names a field it shares with"),
        ("id", "names a field of its key"),
        ("added", "names its order"),
        ("nothing", "`private nothing` names no field of `Note`"),
    ] {
        assert_refused(
            &[(
                "Notes.pw",
                &notes("private draft", &format!("private draft, {private}")),
            )],
            "PW0369",
            says,
        );
    }
    // In an index a sharer reads by.
    assert_refused(
        &[(
            "Notes.pw",
            &notes(
                "index   by_to on to",
                "index   by_to on to\n    index   by_to_draft on to, draft",
            ),
        )],
        "PW0369",
        "`private draft` is in `by_to_draft`",
    );
    // Private, and no one owns its rows.
    assert_refused(
        &[(
            "Notes.pw",
            &notes("    owned   by author\n    shares  with to\n", ""),
        )],
        "PW0369",
        "keeps fields private and no one owns its rows",
    );
    // The neighbours: the order kept private where no one else reads the
    // rows, and a private field in an index its owner reads by.
    assert_clean(&[(
        "Notes.pw",
        &notes("    shares  with to\n", "")
            .replacen("    index   by_to on to\n", "", 1)
            .replacen("private draft", "private draft, added", 1),
    )]);
    assert_clean(&[(
        "Notes.pw",
        &notes(
            "index   by_to on to",
            "index   by_to on to\n    index   by_author_draft on author, draft",
        ),
    )]);
}

#[test]
fn pw0369_the_seen_row_is_the_compilers_to_declare() {
    assert_refused(
        &[(
            "Notes.pw",
            &notes(
                "table Notes",
                "type NoteSeen = NoteSeen { id: NoteId }\n\ntable Notes",
            ),
        )],
        "PW0369",
        "`NoteSeen` is the record a reader `Notes` shares with is answered",
    );
}

#[test]
fn private_is_still_a_declarations_visibility_after_a_table() {
    // `private` heads a table's clause only where a field's name follows
    // it; before a declaration's word it is that declaration's visibility.
    let src = format!(
        "{}\nprivate fn helper() -> Int !{{}} {{\n    1\n}}\n",
        fixture("Notes.pw")
    );
    let parsed = pw_syntax::parse_tree(&src);
    assert!(parsed.ok(), "{:?}", parsed.errors);
    let hir = pw_core::lower::lower_file(&src, &parsed.green);
    assert!(
        hir.all_decls()
            .any(|(_, d)| d.name == "helper" && d.kind == pw_core::hir::DeclKind::Fn),
        "`private fn helper` is no function"
    );
    let t = &pw_core::tables::declared(&hir)[0];
    assert_eq!(t.private_fields(), ["draft"]);
    assert!(t.unread.is_empty(), "{:?}", t.unread);
}
