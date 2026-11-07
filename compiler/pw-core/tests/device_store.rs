//! **A value the reader's device keeps** (track `device-store`, ADR-0366):
//! a module's signal with `storage device "name"`, `privacy Device` and
//! `conflict last_write_wins`, its versions and their migrations, its form,
//! the `Device` label at each read and the sinks that keep it off the
//! server (PW6801-PW6808); and what the browser computes for a value it
//! alone holds (the note's decision 7). Each rule is held by a case and its
//! control, the same program written well, which checks clean.

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

fn packages() -> Vec<(String, String)> {
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

/// What checking `src` says of it, each as `code message`.
fn found(src: &str) -> Vec<String> {
    let mut sources = packages();
    sources.push(("t.pw".to_string(), src.to_string()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// What each diagnostic checking `src` names as where its value is the
/// device's (charter §16.3's boundary), as `code: label`.
fn boundaries(src: &str) -> Vec<String> {
    let mut sources = packages();
    sources.push(("t.pw".to_string(), src.to_string()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| {
            ds.into_iter().flat_map(|d| {
                d.related
                    .into_iter()
                    .map(move |r| format!("{}: {}", d.code, r.label))
            })
        })
        .collect()
}

fn clean(src: &str) {
    assert_eq!(found(src), Vec::<String>::new(), "{src}");
}

/// `src` refused by `code`, its message holding `why`, and by nothing else.
fn refused(src: &str, code: &str, why: &str) {
    let found = found(src);
    assert!(
        found.iter().any(|f| f.starts_with(code) && f.contains(why)),
        "{code} `{why}` expected of\n{src}\ngot {found:#?}"
    );
    assert!(
        found.iter().all(|f| f.starts_with(code)),
        "only {code} expected of\n{src}\ngot {found:#?}"
    );
}

/// The program built, as `pw build` builds it.
fn built(src: &str) -> pw_core::build::Build {
    let mut units = Vec::new();
    let mut sources = packages();
    sources.push(("t.pw".to_string(), src.to_string()));
    for (path, src) in sources {
        units.push(Unit {
            hir: lower_file(&src, &parse_tree(&src).green),
            path,
            src,
        });
    }
    let result = pw_core::build::build(&units);
    assert!(result.is_ok(), "it builds: {:?}", result.as_ref().err());
    result.unwrap()
}

/// A module declaring `kept`, a page `P` holding `signals` and showing
/// `markup`, and `rest` beside it.
fn program(kept: &str, signals: &str, markup: &str, rest: &str) -> String {
    format!(
        "module t\n\nimport List\nimport String\nimport events.{{ KeyEvent }}\n\n\
         type Note = Note {{ id: Int, text: String }}\n\n{kept}\n\n{rest}\n\n\
         page P() {{\n    route \"/\"\n    cache private\n\n    {signals}\n\n    view {{\n        \
         <title>t</title>\n        <main>\n            {markup}\n        </main>\n    }}\n}}\n"
    )
}

const KEPT: &str = "signal notes: List<Note> = []\n    storage  device \"t-notes\"\n    privacy  Device\n    conflict last_write_wins";
const WORD: &str = "signal word: String = \"\"\n    storage  device \"t-word\"\n    privacy  Device\n    conflict last_write_wins";
const ADD: &str = "<button type=\"button\" on:press={() => notes = List.concat(notes, [Note { id: 1, text: \"a\" }])}>Add</button>";
const COUNT: &str = "<p id=\"count\">{List.length(notes)}</p>";

// --- the declaration --------------------------------------------------------

/// **The control**: a value the device keeps, read and written by a page,
/// checks clean; its page holds it, under its name and version, its form
/// the decoder's, and its element marked as the device's.
#[test]
fn a_value_the_device_keeps_checks_and_its_page_holds_it() {
    let src = program(
        KEPT,
        "signal draft: String = \"\"",
        &format!("{COUNT}\n            {ADD}"),
        "",
    );
    clean(&src);
    let b = built(&src);
    let plan = b
        .pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .and_then(|p| p.plan.as_ref().ok());
    assert!(
        plan.is_some(),
        "a valid device page retains its executable plan"
    );
    let plan = plan.expect("the executable plan was asserted");
    let held = serde_json::to_value(&plan.device).expect("JSON");
    assert_eq!(
        held,
        serde_json::json!([{
            "signal": "notes", "name": "t-notes", "version": 1, "conflict": "last_write_wins",
            "schemas": [{"t": "list", "of": {"t": "record", "fields": [
                ["id", {"t": "int"}], ["text", {"t": "string"}]]}}],
        }])
    );
    // Its first value is the default, held as a signal of the page.
    assert!(
        plan.signals
            .iter()
            .any(|s| s.name == "notes" && s.initial == serde_json::json!([])),
        "{:?}",
        plan.signals
    );
    // The element it decides is marked; the page's own is not.
    let template = serde_json::to_string(&b.templates).expect("JSON");
    assert!(
        template.contains("<p data-pw-device id=\\\"count\\\">"),
        "{template}"
    );
    assert!(!template.contains("<button data-pw-device"), "{template}");
}

#[test]
fn pw6805_where_it_rests_whose_it_is_and_what_two_tabs_do() {
    // A value and clauses, none of them where it rests.
    refused(
        &program(
            "signal notes: List<Note> = []\n    privacy  Device\n    conflict last_write_wins",
            "",
            COUNT,
            "",
        ),
        "PW6805",
        "says nowhere to keep it",
    );
    refused(
        &program(&KEPT.replace("device \"t-notes\"", "origin"), "", COUNT, ""),
        "PW6805",
        "kept at `origin`",
    );
    refused(
        &program(&KEPT.replace("device \"t-notes\"", "device"), "", COUNT, ""),
        "PW6805",
        "under no name",
    );
    refused(
        &program(&KEPT.replace("t-notes", "9 notes"), "", COUNT, ""),
        "PW6805",
        "a name is of letters, digits",
    );
    refused(
        &program(
            &format!("{KEPT}\n\n{}", WORD.replace("t-word", "t-notes")),
            "",
            COUNT,
            "",
        ),
        "PW6805",
        "which `notes` is kept under too",
    );
    refused(
        &program(&KEPT.replace("    privacy  Device\n", ""), "", COUNT, ""),
        "PW6805",
        "does not say whose it is",
    );
    refused(
        &program(
            &KEPT.replace(
                "conflict last_write_wins",
                "conflict merge_by_field(text = last_write_wins)",
            ),
            "",
            COUNT,
            "",
        ),
        "PW6805",
        "offers `last_write_wins` alone",
    );
    refused(
        &program(
            &KEPT.replace("\n    conflict last_write_wins", ""),
            "",
            COUNT,
            "",
        ),
        "PW6805",
        "what two tabs' writes of it do",
    );
}

#[test]
fn pw6805_privacy_other_than_the_devices_is_a_later_versions() {
    refused(
        &program(
            &KEPT.replace("privacy  Device", "privacy  Public"),
            "",
            COUNT,
            "",
        ),
        "PW6805",
        "keeps only the device's own",
    );
}

#[test]
fn pw6806_a_pure_step_for_each_earlier_version() {
    const V1: &str = "type NoteV1 = NoteV1 { text: String }";
    let v2 = format!("{KEPT}\n    version  2\n    migrate  from_1");
    let step = "fn from_1(old: List<NoteV1>) -> List<Note> !{} {\n    List.map(old, (n) => Note { id: 0, text: n.text })\n}";
    // The control: one step, from version 1 to the declared type.
    clean(&program(&v2, "", COUNT, &format!("{V1}\n\n{step}")));
    // Version 2 with no step.
    refused(
        &program(&format!("{KEPT}\n    version  2"), "", COUNT, V1),
        "PW6806",
        "names 0 migrations where it needs 1",
    );
    // A step that is no function.
    refused(
        &program(
            &v2,
            "",
            COUNT,
            &format!("{V1}\n\nfn other() -> Int !{{}} {{\n    1\n}}"),
        ),
        "PW6806",
        "`from_1` is no function here",
    );
    // A step that does not say it performs nothing.
    refused(
        &program(
            &v2,
            "",
            COUNT,
            &format!("{V1}\n\n{}", step.replace(" !{}", "")),
        ),
        "PW6806",
        "does not say it performs nothing",
    );
    // A step that makes another type than the declared one.
    refused(
        &program(
            &v2,
            "",
            COUNT,
            &format!("{V1}\n\nfn from_1(old: List<NoteV1>) -> List<NoteV1> !{{}} {{\n    old\n}}"),
        ),
        "PW6806",
        "makes a `List<NoteV1>`, and `notes` is a `List<Note>`",
    );
    // Two steps that do not chain.
    let v3 = format!("{KEPT}\n    version  3\n    migrate  from_1, from_2");
    refused(
        &program(
            &v3,
            "",
            COUNT,
            &format!(
                "{V1}\n\n{step}\n\nfn from_2(old: List<NoteV1>) -> List<Note> !{{}} {{\n    from_1(old)\n}}"
            ),
        ),
        "PW6806",
        "`from_1` makes a `List<Note>`, and `from_2`, the next step, takes a `List<NoteV1>`",
    );
}

#[test]
fn pw6807_a_form_the_device_can_hold() {
    refused(
        &program(
            "signal shown: Map<String, Int> = Map.empty()\n    storage  device \"t-map\"\n    privacy  Device\n    conflict last_write_wins",
            "",
            "<p>{Map.size(shown)}</p>",
            "import Map",
        ),
        "PW6807",
        "is not kept by the first version",
    );
    refused(
        &program(
            "signal kept: Bounded = Bounded(1)\n    storage  device \"t-bounded\"\n    privacy  Device\n    conflict last_write_wins",
            "",
            "<p>{kept.value}</p>",
            "opaque type Bounded = Int where value >= 1",
        ),
        "PW6807",
        "holds a `where`",
    );
    // The control: a record, a list, an option and a case.
    clean(&program(
        "signal kept: Kept = Kept { notes: [], picked: None, shape: Shape.Round }\n    storage  device \"t-kept\"\n    privacy  Device\n    conflict last_write_wins",
        "",
        "<p>{List.length(kept.notes)}</p>",
        "type Shape = | Round | Square(Int)\n\ntype Kept = Kept { notes: List<Note>, picked: Option<Int>, shape: Shape }",
    ));
}

#[test]
fn pw6808_the_device_provides_it() {
    refused(
        &program(KEPT, "provide notes = []", COUNT, ""),
        "PW6808",
        "is kept by the device, and this gives it a value of its own",
    );
}

// --- the label, and where it may not go --------------------------------------

const SEND: &str = "command send(text: String) -> Result<String, String>\n    requires Anyone\n    idempotent_by Press\n{\n    Ok(text)\n}\n\nopaque type Press = String";

fn first(notes: &str) -> String {
    format!(
        "fn first_text(notes: List<Note>) -> String !{{}} {{\n    match List.get(notes, 0) {{\n        Some(n) => n.text,\n        None => \"\",\n    }}\n}}\n\n{notes}"
    )
}

#[test]
fn pw6801_no_command_is_given_what_the_device_keeps() {
    let press = |arg: &str| {
        format!(
            "<button type=\"button\" on:press={{() => match send({arg}) {{\n                Ok(_) => draft = \"\",\n                Err(_) => draft = \"\",\n            }}}}>Send</button>"
        )
    };
    let rest = first(SEND);
    // Directly, and through a helper's body.
    let direct = program(WORD, "signal draft: String = \"\"", &press("word"), &rest);
    refused(
        &direct,
        "PW6801",
        "`word` is kept on this device, and `send`'s argument is sent to the server",
    );
    // Each refusal names where the value is the device's: its declaration.
    assert_eq!(
        boundaries(&direct),
        ["PW6801: `word` is kept on the device, `privacy Device`, here"],
        "{direct}"
    );
    refused(
        &program(
            KEPT,
            "signal draft: String = \"\"",
            &press("first_text(notes)"),
            &rest,
        ),
        "PW6801",
        "`notes` is kept on this device",
    );
    // Through a branch on it.
    refused(
        &program(
            KEPT,
            "signal draft: String = \"\"",
            &press("if List.length(notes) > 0 { \"some\" } else { \"none\" }"),
            &rest,
        ),
        "PW6801",
        "is kept on this device",
    );
    // Through a page's signal it was assigned to.
    let assigned = program(
        WORD,
        "signal draft: String = \"\"",
        &format!(
            "<input aria-label=\"d\" on:keydown={{(e: KeyEvent) => draft = word}} />\n            {}",
            press("draft")
        ),
        &rest,
    );
    refused(
        &assigned,
        "PW6801",
        "`draft` holds what this device keeps, and `send`'s argument is sent to the server",
    );
    // Its boundary: where the page's signal came to hold it.
    assert!(
        boundaries(&assigned)
            .iter()
            .all(|b| b.starts_with("PW6801: `draft` holds what the device keeps")),
        "{:?}",
        boundaries(&assigned)
    );
    // A call it decides.
    refused(
        &program(
            KEPT,
            "signal draft: String = \"\"",
            "<button type=\"button\" on:press={() => if List.length(notes) > 0 {\n                match send(\"x\") {\n                    Ok(_) => draft = \"\",\n                    Err(_) => draft = \"\",\n                }\n            }}>Send</button>",
            &rest,
        ),
        "PW6801",
        "whether `send` is sent is decided by `notes`",
    );
    // The control: what the page holds of its own.
    clean(&program(
        KEPT,
        "signal draft: String = \"\"",
        &format!("{COUNT}\n            {}", press("draft")),
        &rest,
    ));
}

#[test]
fn pw6802_no_query_is_keyed_by_what_the_device_keeps() {
    let search = "session query Search(session: Session<SessionId>, term: String) -> String\n    freshness      0.seconds\n    consistency    read_your_writes\n    cache          private\n    key            session, term\n    concurrency    one_per_key\n    on_key_change  cancel\n    timeout        2.seconds\n{\n    term\n}";
    // A page's query is keyed by the page's own signal (ADR-0152), which a
    // handler assigns: a value the device keeps reaches the key through it,
    // as R-109's does.
    let with = |assigned: &str| {
        program(
            WORD,
            "signal shown: String = \"\"\n    let found = query Search(current_session(), shown)",
            &format!(
                "<button type=\"button\" on:press={{() => shown = {assigned}}}>Search</button>\n            <p>{{found}}</p>"
            ),
            &format!(
                "import context.{{ current_session }}\nimport capability.{{ Session, SessionId }}\n\n{search}"
            ),
        )
    };
    refused(
        &with("word"),
        "PW6802",
        "`shown` holds what this device keeps, and a query's key is sent to the server",
    );
    // The control: the page's own value.
    clean(&with("\"cheese\""));
}

#[test]
fn pw6803_nothing_the_browser_sends_holds_it() {
    // A link to a route the program declares (PW5009 is a dead one's).
    let x = "page X(w: String) {\n    route \"/x/{w}\"\n    cache private\n\n    view {\n        <title>x</title>\n        <main><p>{w}</p></main>\n    }\n}";
    refused(
        &program(WORD, "", "<a href=\"/x/{word}\">x</a>", x),
        "PW6803",
        "`href` is an address, which the browser sends",
    );
    refused(
        &program(WORD, "", "<img alt=\"x\" src={word} />", ""),
        "PW6803",
        "`src` is an address",
    );
    refused(
        &program(
            WORD,
            "",
            // Sent to the page's own address (PW5041 is a form sent where
            // nothing answers).
            "<form>\n                <input aria-label=\"w\" name=\"w\" value={word} />\n            </form>",
            "",
        ),
        "PW6803",
        "is inside a `<form>`, which sends its controls' values",
    );
    // The control: shown as text, which the browser keeps.
    clean(&program(WORD, "", "<p>{word}</p>", ""));
}

#[test]
fn pw6804_the_device_keeps_its_own() {
    let who = "session query Who(session: Session<SessionId>) -> String\n    freshness      0.seconds\n    consistency    read_your_writes\n    cache          private\n    key            session\n    concurrency    one_per_key\n    on_key_change  cancel\n    timeout        2.seconds\n{\n    \"me\"\n}";
    // The handler reads the session's value it captures (PW5017 is one
    // captured and never read), and keeps on the device what it is given.
    let src = |handler: &str| {
        program(
            WORD,
            "signal draft: String = \"\"\n    let me = query Who(current_session())",
            &format!(
                "<button type=\"button\" on:press={{resumable(captures = {{ me }}) => {{\n                {handler}\n            }}}}>Keep</button>"
            ),
            &format!(
                "import context.{{ current_session }}\nimport capability.{{ Session, SessionId }}\n\n{who}"
            ),
        )
        // A session's page, as a handler capturing a session's value asks
        // (PW5018).
        .replace("page P()", "session page P()")
    };
    refused(
        &src("word = me"),
        "PW6804",
        "`word` is kept on this device, and this value is Session",
    );
    // The control: the session's value stays the page's; the device keeps
    // a value of its own.
    clean(&src("draft = me\n                word = \"public\""));
}

// --- what the browser computes (decision 7) ----------------------------------

/// **A value computed from several signals, a value the device keeps among
/// them, and a list computed inside a block the browser renders**: each
/// built, the browser's to compute, its block rendered again for each input.
#[test]
fn the_browser_computes_from_several_values_and_a_list_in_its_block() {
    let src = program(
        KEPT,
        "signal shown: String = \"all\"",
        "<p id=\"left\">{left(notes, shown)}</p>\n            {#if List.length(notes) > 0}\n                <ul>\n                    {#each visible(notes, shown) as n (n.id)}\n                        <li>{n.text}</li>\n                    {/each}\n                </ul>\n            {/if}",
        "fn left(notes: List<Note>, shown: String) -> Int !{} {\n    List.length(visible(notes, shown))\n}\n\nfn visible(notes: List<Note>, shown: String) -> List<Note> !{} {\n    if shown == \"all\" {\n        notes\n    } else {\n        []\n    }\n}",
    );
    clean(&src);
    let b = built(&src);
    assert!(b.refusals().is_empty(), "{:?}", b.refusals());
    let plan = b
        .pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .and_then(|p| p.plan.as_ref().ok());
    assert!(
        plan.is_some(),
        "a valid device page retains its executable plan"
    );
    let plan = plan.expect("the executable plan was asserted");
    let live = serde_json::to_value(&plan.live).expect("JSON");
    let entries = live.as_array().expect("a list");
    // The text from two values: each given, in order.
    assert!(
        entries
            .iter()
            .any(|l| l["kind"] == "text" && l["paths"] == serde_json::json!(["notes", "shown"])),
        "{live:#}"
    );
    // The list, for its block's render alone.
    assert!(
        entries
            .iter()
            .any(|l| l["kind"] == "value" && l["paths"] == serde_json::json!(["notes", "shown"])),
        "{live:#}"
    );
    // Its block is rendered again for each.
    assert!(
        entries.iter().any(|l| l["kind"] == "conditional"
            && l["reads"]
                .as_array()
                .is_some_and(|r| r.contains(&serde_json::json!("shown")))),
        "{live:#}"
    );
    // The module takes each value its function reads.
    let module = b
        .computed
        .iter()
        .find(|m| m.page == "t.P~ui")
        .and_then(|m| m.module.as_ref().ok())
        .expect("a module");
    assert!(module.contains("(j0, j1) =>"), "{module}");
}

/// **A value computed in a row of a list the browser renders is not built
/// yet** (ruling 0073-a): refused by name, as before.
#[test]
fn a_value_computed_in_a_browser_rows_is_refused_by_name() {
    let src = program(
        KEPT,
        "",
        "{#if List.length(notes) > 0}\n                <ul>\n                    {#each notes as n (n.id)}\n                        <li>{String.trim(n.text)}</li>\n                    {/each}\n                </ul>\n            {/if}",
        "",
    );
    clean(&src);
    // Its row's item typed from the list the device keeps, so it is this
    // refusal and not a type the build lacks.
    let why = refusals(&src);
    assert!(
        why.contains("computes a value in a row of `n`, a list no query gives"),
        "{why}"
    );
}

/// What a build of `src` refuses, as one line, or nothing.
fn refusals(src: &str) -> String {
    let mut units = Vec::new();
    let mut sources = packages();
    sources.push(("t.pw".to_string(), src.to_string()));
    for (path, src) in sources {
        units.push(Unit {
            hir: lower_file(&src, &parse_tree(&src).green),
            path,
            src,
        });
    }
    match pw_core::build::build(&units) {
        Err(why) => why,
        Ok(b) => b.refusals().join("; "),
    }
}

/// **A value computed from what the browser holds, inside a block a host
/// decides**, which the browser does not render again: refused by name,
/// each value it reads named; at the top of the page, its control, it
/// builds.
#[test]
fn a_value_computed_from_the_devices_inside_a_hosts_block_is_refused_by_name() {
    let rest = "public query Pinned() -> Bool !{} {\n    true\n}\n\nfn left(notes: List<Note>, shown: String) -> Int !{} {\n    if shown == \"all\" {\n        List.length(notes)\n    } else {\n        0\n    }\n}";
    let signals = "signal shown: String = \"all\"\n    let pinned = query Pinned()";
    let inside = program(
        KEPT,
        signals,
        "{#if pinned}<p id=\"left\">{left(notes, shown)}</p>{/if}",
        rest,
    );
    clean(&inside);
    let why = refusals(&inside);
    assert!(
        why.contains(
            "computes a value from the signals `notes` and `shown` inside a block a value \
             other than a signal decides, which the browser does not render again (ruling \
             0073-a)"
        ),
        "{why}"
    );
    let top = program(
        KEPT,
        signals,
        "<p id=\"left\">{left(notes, shown)}</p>",
        rest,
    );
    assert_eq!(refusals(&top), "", "{top}");
}

/// **Decision 7 lifts two refusals of ruling 0073-a for the browser's values
/// alone** (the integrator's ruling 3 on this track, 2026-10-20): a value
/// computed from several values, and a value computed inside a block a signal
/// decides, a block's computed `{#each}` list among them. Each still fires,
/// in the words it has always used, where a value a host holds is read: a
/// query's, beside the value the device keeps or alone. And each is built
/// from the browser's values alone, below. The ADR names each refusal, its
/// reason and these cases.
#[test]
fn a_value_a_host_holds_is_still_refused_where_decision_7_lifts_the_refusals() {
    let rest = "public query Likes() -> Int !{} {\n    3\n}\n\n\
        public query More() -> Int !{} {\n    4\n}\n\n\
        public query Pinned() -> List<Note> !{} {\n    []\n}\n\n\
        fn total(notes: List<Note>, likes: Int) -> Int !{} {\n    List.length(notes) + likes\n}\n\n\
        fn sum(likes: Int, more: Int) -> Int !{} {\n    likes + more\n}\n\n\
        fn plus_one(likes: Int) -> Int !{} {\n    likes + 1\n}\n\n\
        fn merged(notes: List<Note>, pinned: List<Note>) -> List<Note> !{} {\n    List.concat(notes, pinned)\n}\n\n\
        fn only(pinned: List<Note>) -> List<Note> !{} {\n    pinned\n}\n\n\
        fn left(notes: List<Note>, shown: String) -> Int !{} {\n    List.length(visible(notes, shown))\n}\n\n\
        fn visible(notes: List<Note>, shown: String) -> List<Note> !{} {\n    if shown == \"all\" {\n        notes\n    } else {\n        []\n    }\n}";
    // The queries' values are a host's: the page is rendered with them.
    let signals = "signal shown: String = \"all\"\n    let likes = query Likes()\n    let more = query More()\n    let pinned = query Pinned()";
    // A block the device's value decides, which the browser renders again.
    let decides = "{#if List.length(notes) > 0}";
    let several = |names: &str| {
        format!(
            "computes a value from {names}, and a host computes one from one value (ruling 0073-a)"
        )
    };
    let inside = "computes a value inside a block a signal decides, and the browser, which \
                  renders that block again, computes none yet (ruling 0073-a)";
    let cases: Vec<(String, String)> = vec![
        // From several values, a host's among them.
        (
            "<p id=\"total\">{total(notes, likes)}</p>".to_string(),
            several("`notes` and `likes`"),
        ),
        (
            "<p id=\"total\">{sum(likes, more)}</p>".to_string(),
            several("`likes` and `more`"),
        ),
        // Inside a block a signal decides, from a host's value.
        (
            [decides, "<p id=\"total\">{total(notes, likes)}</p>{/if}"].concat(),
            inside.to_string(),
        ),
        (
            [decides, "<p id=\"total\">{plus_one(likes)}</p>{/if}"].concat(),
            inside.to_string(),
        ),
        // And the list such a block reads.
        (
            [
                decides,
                "<ul>{#each merged(notes, pinned) as n (n.id)}<li>{n.text}</li>{/each}</ul>{/if}",
            ]
            .concat(),
            "`{#each}` reads its list by path, and `merged(notes, pinned)` is computed".to_string(),
        ),
        (
            [
                decides,
                "<ul>{#each only(pinned) as n (n.id)}<li>{n.text}</li>{/each}</ul>{/if}",
            ]
            .concat(),
            "`{#each}` reads its list by path, and `only(pinned)` is computed".to_string(),
        ),
    ];
    for (markup, why) in &cases {
        let src = program(KEPT, signals, markup, rest);
        clean(&src);
        let said = refusals(&src);
        assert!(said.contains(why.as_str()), "{markup}: {said}");
    }
    // The controls: each shape from the browser's values alone builds.
    let signals = "signal shown: String = \"all\"";
    for markup in [
        "<p id=\"left\">{left(notes, shown)}</p>".to_string(),
        [decides, "<p id=\"left\">{left(notes, shown)}</p>{/if}"].concat(),
        [
            decides,
            "<ul>{#each visible(notes, shown) as n (n.id)}<li>{n.text}</li>{/each}</ul>{/if}",
        ]
        .concat(),
    ] {
        let src = program(KEPT, signals, &markup, rest);
        clean(&src);
        assert_eq!(refusals(&src), "", "{markup}");
    }
}

// The generic solver and Device checker must agree on this same row.
#[test]
fn generic_reverse_keeps_a_device_rows_type_and_privacy() {
    let rows = "{#each List.reverse(notes) as n (n.id)}<li>{n.text}</li>{/each}";
    clean(&program(KEPT, "", rows, ""));
    let send = "{#each List.reverse(notes) as n (n.id)}<button type=\"button\" on:press={() => match send(n.text) { Ok(_) => draft = \"\", Err(_) => draft = \"\", }}>Send</button>{/each}";
    let private = program(KEPT, "signal draft: String = \"\"", send, SEND);
    refused(&private, "PW6801", "sent");
    let boundary = boundaries(&private);
    assert!(!boundary.is_empty(), "{private}");
    assert!(
        boundary.iter().all(|b| b.contains("device")),
        "{boundary:?}"
    );
    clean(&program(
        "",
        "signal notes: List<Note> = []\nsignal draft: String = \"\"",
        send,
        SEND,
    ));
}
