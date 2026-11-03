//! **A query that `requires` a predicate is answered to its reader alone, and
//! read by a page's binding alone** (track `row-ownership`; the second
//! review's findings 1 and 2, PW6907 and PW6926): the review's server
//! probes, kept.
//!
//! - With no `cache` clause, a public `Club() requires SignedIn` was keyed by
//!   its arguments alone, and an anonymous reader asking while bob's read was
//!   in flight joined it (`pw-resource`'s one flight a key) and was answered
//!   the secret. Such a query is refused now; cached `private`, its reads are
//!   keyed by their readers, and no reader joins another's.
//! - `Lobby(post) { Club(post) }` compiled `Club`'s body into `Lobby`, whose
//!   contract required nothing, and `/lobby/p-nobodys` answered the secret.
//!   Such a program is refused now; a page binds `Club` itself, and the host
//!   asks `OwnsPost` for it.
use super::tables::Asserted;
use super::*;
use std::time::Duration;

const SECRET: &str = "CLUB_SECRET_FOR_MEMBERS";

/// The feed's program, its `app.pw` changed by `change`, as `pw build`
/// builds it, or why it does not.
fn feed_build(change: fn(&str) -> String) -> Result<pw_core::build::Build, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut units = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/feed",
    ] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            let mut src = std::fs::read_to_string(&p).expect("read");
            if d == "examples/feed" && p.ends_with("app.pw") {
                src = change(&src);
            }
            units.push(pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: p.display().to_string(),
                src,
            });
        }
    }
    pw_core::build::build(&units)
}

/// `Club`, which `SignedIn` guards, `cache` as given, and the page binding it.
fn club(app: &str, cache: &str) -> String {
    format!(
        "{app}\n\
public query Club() -> String\n    consistency    snapshot\n{cache}    concurrency    one_per_key\n    timeout        20.seconds\n    requires       SignedIn\n{{\n    \"CLUB_SECRET_FOR_MEMBERS\"\n}}\n\n\
page ClubPage() {{\n    route        \"/club\"\n    placement    origin\n    cache        private\n\n    let club = query Club()\n\n    view {{\n        <title>Club</title>\n        <main><p id=\"club\">{{club}}</p></main>\n    }}\n}}\n"
    )
}

fn club_unkeyed(app: &str) -> String {
    club(app, "")
}

fn club_private(app: &str) -> String {
    club(app, "    cache          private\n")
}

/// Hold the next read of `Club` in flight, after it ran, until released.
fn pause_next_club_read(
    s: &Server,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) {
    *s.in_flight.lock().expect("read hook") = Some(Box::new(move |server| {
        let club = server.queries.trace().iter().rev().find_map(|t| match t {
            pw_resource::Trace::RequestStarted { key, .. } => Some(key.resource.clone()),
            _ => None,
        });
        if club.as_deref().is_some_and(|r| r.ends_with("Club")) {
            entered.send(()).expect("entered");
            release
                .recv_timeout(Duration::from_secs(20))
                .expect("released");
        } else {
            pause_next_club_read(server, entered, release);
        }
    }));
}

#[test]
fn a_query_that_requires_and_is_keyed_without_its_reader_is_refused_at_build() {
    let refused = feed_build(club_unkeyed).err().unwrap_or_default();
    assert!(
        refused.contains("[PW6907]")
            && refused.contains("`Club` requires SignedIn and is read for every reader alike"),
        "{refused}"
    );
}

#[test]
fn a_query_that_requires_is_keyed_by_its_reader_so_no_reader_joins_anothers_flight() {
    let s = served_feed_with(club_private);
    s.identity.signed_in_for_test("b", "bob", "bob");
    assert!(
        !get_as(&s, "anon", "/club").contains(SECRET),
        "the anonymous reader alone is refused"
    );
    assert!(get_as(&s, "b", "/club").contains(SECRET), "bob reads it");
    // Bob's read held in flight; the anonymous reader asks meanwhile.
    let (entered, started) = std::sync::mpsc::channel();
    let (release, cont) = std::sync::mpsc::channel::<()>();
    pause_next_club_read(&s, entered, cont);
    let (bob_doc, anon_doc, joined) = std::thread::scope(|scope| {
        let bobs = scope.spawn(|| get_as(&s, "b", "/club"));
        started
            .recv_timeout(Duration::from_secs(20))
            .expect("bob's read of Club held in flight");
        let from = s.queries.trace().len();
        let anons = scope.spawn(|| get_as(&s, "anon", "/club"));
        // Until the anonymous reader is answered, or joins bob's flight.
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut joined = false;
        while !anons.is_finished() && std::time::Instant::now() < deadline {
            joined = s.queries.trace()[from..].iter().any(|t| {
                matches!(t, pw_resource::Trace::RequestDeduplicated { key } if key.resource.ends_with("Club"))
            });
            if joined {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        release.send(()).expect("release");
        (
            bobs.join().expect("bob"),
            anons.join().expect("anon"),
            joined,
        )
    });
    assert!(bob_doc.contains(SECRET), "bob's own read");
    assert!(!joined, "the anonymous reader joined bob's read in flight");
    assert!(
        !anon_doc.contains(SECRET),
        "the anonymous reader was answered bob's read"
    );
}

fn owned_club(app: &str) -> String {
    format!(
        "{app}\n\
public query Club(post: PostId) -> String\n    consistency    snapshot\n    cache          private\n    requires       OwnsPost(post)\n{{\n    \"CLUB_SECRET_FOR_MEMBERS\"\n}}\n\n\
page ClubPage(post: PostId) {{\n    route        \"/club/{{post}}\"\n    placement    origin\n    cache        private\n\n    let club = query Club(post)\n\n    view {{\n        <title>Club</title>\n        <main><p id=\"club\">{{club}}</p></main>\n    }}\n}}\n"
    )
}

fn owned_lobby(app: &str) -> String {
    format!(
        "{}\n\
public query Lobby(post: PostId) -> String\n    consistency    snapshot\n    cache          private\n{{\n    Club(post)\n}}\n\n\
page LobbyPage(post: PostId) {{\n    route        \"/lobby/{{post}}\"\n    placement    origin\n    cache        private\n\n    let lobby = query Lobby(post)\n\n    view {{\n        <title>Lobby</title>\n        <main><p id=\"lobby\">{{lobby}}</p></main>\n    }}\n}}\n",
        owned_club(app)
    )
}

#[test]
fn a_query_calling_a_guarded_query_is_refused_and_a_page_binds_it() {
    let refused = feed_build(owned_lobby).err().unwrap_or_default();
    assert!(
        refused.contains("[PW6926]")
            && refused.contains("`Lobby` calls `Club`, which requires OwnsPost(post)"),
        "{refused}"
    );
    // The neighbour: the page binds `Club` itself, and the host asks
    // `OwnsPost` for its reader, who owns no such post.
    let s = served_feed_with(owned_club);
    let club = get_as(&s, "anon", "/club/p-nobodys");
    assert!(
        !club.contains(SECRET),
        "the guarded query answered a reader who owns no such post"
    );
}

fn served_owned() -> Served {
    let (dir, out) = tables::built(&["examples/owned"], &[("page.pw", OWNED_PAGE)]);
    Served {
        server: Server::from_build(out.clone(), out).asserted("the owned program is served"),
        _dir: dir,
    }
}

const OWNED_PAGE: &str = r#"module owned.page
import context.{ current_user }
import Notes.{ NoteId }
import owned.app.{ Shown }

session page NotePage(id: NoteId) {
    route "/owned/{id}"
    placement origin
    cache private
    let shown = query Shown(current_user(), id)
    view {
        <title>Owned</title>
        <main>
            {#match shown}
                {:Some(note)}<p id="note">{note.text}</p>
                {:None}<p id="none">No note</p>
            {/match}
        </main>
    }
}
"#;

fn write_owned(s: &Server, session: &str, text: &str) {
    let answer = s
        .command_answer(
            "owned.app.write",
            session,
            &[
                serde_json::json!("same-id"),
                serde_json::json!("bob"),
                serde_json::json!(text),
            ],
            None,
        )
        .asserted("write answered");
    assert!(answer.committed, "{:?} {:?}", answer.why, answer.result);
}

#[test]
fn served_owned_tables_hold_every_handle_to_the_request_reader() {
    let s = served_owned();
    s.identity.signed_in_for_test("a", "alice", "Alice");
    s.identity.signed_in_for_test("b", "bob", "Bob");
    write_owned(&s, "a", "ALICES_DRAFT");
    let args = |reader: &str| [Val::String(reader.into()), Val::String("same-id".into())];
    let own = s
        .answer("owned.app.One", "a", &args("alice"))
        .asserted("Alice reads her note");
    assert!(format!("{own:?}").contains("ALICES_DRAFT"), "{own:?}");
    assert_eq!(
        s.answer("owned.app.One", "b", &args("bob"))
            .asserted("Bob reads his key"),
        Val::Option(None)
    );
    let forged = s.answer("owned.app.One", "b", &args("alice"));
    assert!(
        forged.is_err(),
        "a forged handle read Alice's note: {forged:?}"
    );
    let shared = s
        .answer("owned.app.ToMe", "b", &[Val::String("bob".into())])
        .asserted("Bob reads his projection");
    assert!(format!("{shared:?}").contains("ALICES_DRAFT"), "{shared:?}");
    assert!(!format!("{shared:?}").contains("\"draft\""), "{shared:?}");
    let forged = s.command_answer("owned.app.keep", "b", &[serde_json::json!({
        "author": "alice", "id": "same-id", "to": "bob", "text": "DEFACED", "draft": "DEFACED", "added": 0
    })], None).asserted("forged write answered");
    assert!(!forged.committed, "{:?}", forged.result);
    assert_eq!(
        s.answer("owned.app.One", "a", &args("alice"))
            .asserted("Alice's original note"),
        own
    );
    write_owned(&s, "b", "BOBS_DRAFT");
    let bob = s
        .answer("owned.app.One", "b", &args("bob"))
        .asserted("Bob's independent same id");
    assert!(format!("{bob:?}").contains("BOBS_DRAFT"), "{bob:?}");
    assert_eq!(
        s.answer("owned.app.One", "a", &args("alice"))
            .asserted("Alice is unchanged"),
        own
    );
}

#[test]
fn served_program_predicates_guard_commands_and_page_queries() {
    let s = served_owned();
    s.identity.signed_in_for_test("a", "alice", "Alice");
    s.identity.signed_in_for_test("b", "bob", "Bob");
    write_owned(&s, "a", "ALICES_PRIVATE_NOTE");
    assert!(get_as(&s, "a", "/owned/same-id").contains("ALICES_PRIVATE_NOTE"));
    assert!(!get_as(&s, "b", "/owned/same-id").contains("ALICES_PRIVATE_NOTE"));
    let edit = |session: &str, text: &str| {
        s.command_answer(
            "owned.app.edit",
            session,
            &[
                serde_json::json!("same-id"),
                serde_json::json!("bob"),
                serde_json::json!(text),
            ],
            None,
        )
        .asserted("edit answered")
    };
    let refused = edit("b", "DEFACED");
    assert!(!refused.committed, "{:?}", refused.result);
    assert_eq!(
        refused.refused.as_deref(),
        Some("Wrote"),
        "why={:?}, result={:?}",
        refused.why,
        refused.result
    );
    let http = posted(
        &s,
        "/command/owned.app.edit",
        "b",
        "row-denial",
        r#"["same-id","bob","DEFACED"]"#,
    );
    assert!(http.starts_with("HTTP/1.1 403"), "{http}");
    let body: serde_json::Value = serde_json::from_str(
        http.split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or_default(),
    )
    .asserted("HTTP refusal JSON");
    assert_eq!(
        body,
        serde_json::json!({ "committed": false, "refused": "Wrote", "says": "You can only change a note you wrote." })
    );
    let edited = edit("a", "ALICES_EDITED_NOTE");
    assert!(edited.committed, "{:?} {:?}", edited.why, edited.result);
    let page = get_as(&s, "a", "/owned/same-id");
    assert!(page.contains("ALICES_EDITED_NOTE"), "{page}");
    assert!(!get_as(&s, "b", "/owned/same-id").contains("ALICES_EDITED_NOTE"));
}

/// A controlled writer commits immediately after the compiled predicate's
/// actual read. Only the query's selected snapshot can keep its body on
/// the state that authorized it, including outside a page render.
struct BetweenPredicateAndBody {
    inner: Arc<dyn data::DataLayer>,
    after: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
}

impl data::DataLayer for BetweenPredicateAndBody {
    fn reads(&self, session: &str, stopped: Option<Stopped>) -> data::Ops {
        self.inner.reads(session, stopped)
    }
    fn begin<'a>(&'a self, session: &str) -> Box<dyn data::Staged + 'a> {
        self.inner.begin(session)
    }
    fn tables(&self) -> Option<&pw_host::tables::Manifest> {
        self.inner.tables()
    }
    fn snapshot(&self) -> Option<data::Snapshot> {
        self.inner.snapshot()
    }
    fn grants(&self) -> Vec<&'static str> {
        self.inner.grants()
    }
    fn default_page(&self) -> Option<&'static str> {
        None
    }
    fn table_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
        snapshot: Option<&data::Snapshot>,
    ) -> Result<data::Ops, String> {
        let reads = self.inner.table_reads(contract, granted, snapshot)?;
        if !contract.exports.iter().any(|e| e.kind == "predicate") {
            return Ok(reads);
        }
        Ok(reads
            .into_iter()
            .map(|(name, read)| {
                let after = Arc::clone(&self.after);
                let wrapped: HostFn = Arc::new(move |args| {
                    let result = read(args)?;
                    let hook = after.lock().expect("predicate hook").take();
                    if let Some(hook) = hook {
                        hook();
                    }
                    Ok(result)
                });
                (name, wrapped)
            })
            .collect())
    }
}

#[test]
fn a_served_query_keeps_one_snapshot_across_its_predicate_and_body() {
    let (_dir, out) = tables::built(&["examples/owned"], &[]);
    let contracts = ComponentContract::from_json(
        &std::fs::read_to_string(out.join("contracts.json")).asserted("contracts"),
    )
    .asserted("contracts decoded");
    let after = Arc::default();
    let layer = BetweenPredicateAndBody {
        inner: Arc::new(
            crate::tables::TablesLayer::from_build(&out, &contracts).asserted("memory layer"),
        ),
        after: Arc::clone(&after),
    };
    let s =
        Arc::new(Server::from_build_tables(out.clone(), out, Arc::new(layer)).asserted("served"));
    s.identity.signed_in_for_test("a", "alice", "Alice");
    write_owned(&s, "a", "SNAPSHOT_NOTE");
    let args = [Val::String("alice".into()), Val::String("same-id".into())];
    let expected = s
        .answer("owned.app.One", "a", &args)
        .asserted("original note");
    let writer = Arc::downgrade(&s);
    *after.lock().expect("hook") = Some(Box::new(move || {
        let s = writer.upgrade().expect("server alive");
        let removed = s
            .command_answer(
                "owned.app.remove",
                "a",
                &[serde_json::json!("same-id"), serde_json::json!("bob")],
                None,
            )
            .asserted("concurrent removal answered");
        assert!(removed.committed, "{removed:?}");
    }));
    assert_eq!(
        s.answer("owned.app.Shown", "a", &args)
            .asserted("one query state"),
        expected
    );
    assert!(
        after.lock().expect("hook").is_none(),
        "the predicate really read"
    );
    assert_eq!(
        s.answer("owned.app.One", "a", &args)
            .asserted("committed removal"),
        Val::Option(None)
    );
    let fresh = s.answer("owned.app.Shown", "a", &args);
    assert!(
        fresh
            .as_ref()
            .is_err_and(|why| why.contains("`Wrote` denied")),
        "{fresh:?}"
    );
}

#[test]
fn a_missing_compiled_predicate_is_refused_before_the_program_is_served() {
    let (_dir, out) = tables::built(&["examples/owned"], &[]);
    std::fs::remove_file(out.join("components/owned.app.Wrote.wasm"))
        .asserted("remove only the predicate artifact");
    let refused = Server::from_build(out.clone(), out)
        .err()
        .unwrap_or_default();
    assert!(
        refused.contains("no compiled program predicate `owned.app.Wrote`"),
        "{refused}"
    );
}

#[test]
fn a_compiled_predicate_without_tables_keeps_its_explicit_denial() {
    let (_dir, out) = tables::built(
        &[],
        &[(
            "pure.pw",
            r#"module guarded.pure
predicate Allowed(pass: Bool)
    says "This call needs permission."
{ pass }
command checked(pass: Bool) -> Bool
    requires Allowed(pass)
{ pass }
"#,
        )],
    );
    let s = Server::from_build(out.clone(), out).asserted("pure predicates served");
    let yes = s
        .command_answer(
            "guarded.pure.checked",
            "a",
            &[serde_json::json!(true)],
            None,
        )
        .asserted("permitted call");
    assert!(yes.committed, "{yes:?}");
    assert_eq!(yes.result, Some(serde_json::Value::Null));
    let no = s
        .command_answer(
            "guarded.pure.checked",
            "a",
            &[serde_json::json!(false)],
            None,
        )
        .asserted("denied call");
    assert!(!no.committed, "{no:?}");
    assert_eq!(no.refused.as_deref(), Some("Allowed"));
}
