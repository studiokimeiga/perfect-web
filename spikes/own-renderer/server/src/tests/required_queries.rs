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
