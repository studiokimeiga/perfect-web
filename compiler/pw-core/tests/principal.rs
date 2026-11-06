//! **The reader's user, and an event that names a user** (track
//! `notifications`, ADR-0270).
//!
//! `context.current_user()` is the host's operation `pw:host/principal#read`,
//! a handle, `User<UserId>`. A user query keyed by one serves its reader
//! alone. ADR-0091 related a listener's argument to its event's value by
//! type; this amends it in one place: in `invalidates_on` alone, a parameter
//! of the platform's `User<T>` binds an event's value of type `T`, so an
//! event naming a user drops the entries at that user in each of their
//! sessions. Nowhere else does an id become a handle, or a handle an id:
//! each test states one case, with its controls.

use pw_core::check::check_sources;

fn files(dir: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "pw"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let src = std::fs::read_to_string(&p).expect("read");
            (p.display().to_string(), src)
        })
        .collect()
}

/// What `app.pw` reports, with the platform: each code and message.
fn reported(src: &str) -> Vec<String> {
    let mut program = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        program.extend(files(d));
    }
    program.push(("app.pw".to_string(), src.to_string()));
    check_sources(&program)
        .into_iter()
        .filter(|(n, _)| n == "app.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// A user's count, keyed by their handle, listening with `listens`, and a
/// command that writes it with `clauses` and `body`; `extra` declares more.
fn program(listens: &str, clauses: &str, body: &str, extra: &str) -> String {
    format!(
        "module m\n\n\
         import context.{{ current_user }}\n\
         import capability.{{ User, UserId, Session, SessionId }}\n\n\
         opaque type PostId = String\n\
         opaque type InteractionId = String\n\n\
         type Count = Count {{ n: Int }}\n\n\
         event Noted(user: UserId)\n\
         event Seen(post: PostId)\n\
         event Visited(session: SessionId)\n\n\
         fn count_of(reader: User<UserId>) -> Int !{{ database.read<Count> }}\n    \
             host \"m:data/counts#of\"\n\n\
         fn bump(reader: User<UserId>) -> Int !{{ database.write<Count> }}\n    \
             host \"m:data/counts#bump\"\n\n\
         {extra}\n\
         user query Counted(reader: User<UserId>) -> Int\n    \
             freshness      0.seconds\n    \
             consistency    read_your_writes\n    \
             cache          private\n    \
             key            reader\n    \
             invalidates_on {listens}\n    \
             concurrency    one_per_key\n    \
             on_key_change  cancel\n    \
             timeout        2.seconds\n\
         {{\n    count_of(reader)\n}}\n\n\
         command noted() -> Int\n    requires Anyone\n    \
             idempotent_by InteractionId\n    \
             {clauses}\n\
         {{\n    {body}\n}}\n"
    )
}

fn clean(src: &str) {
    let found = reported(src);
    assert!(found.is_empty(), "{src}\n{found:#?}");
}

fn refused(src: &str, says: &str) {
    let found = reported(src);
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW0605") && d.contains(says)),
        "{src}\n{found:#?}"
    );
}

/// **A listener's handle binds the event's id** (amending ADR-0091): the
/// user's entries are dropped by an event naming them.
#[test]
fn a_listeners_handle_binds_the_events_id() {
    clean(&program(
        "Noted(reader)",
        "emits Noted(UserId(\"u-1\"))",
        "bump(current_user())",
        "",
    ));
}

/// **Controls: only the platform's `User`, only over the event's own type,
/// and only in a listener.**
#[test]
fn nothing_else_turns_an_id_into_a_handle_or_a_handle_into_an_id() {
    // A handle over the user's id, against an event carrying a post.
    refused(
        &program(
            "Seen(reader)",
            "emits Noted(UserId(\"u-1\"))",
            "bump(current_user())",
            "",
        ),
        "argument 1 of `m.Seen` is declared `m.PostId`",
    );
    // Another qualifier: a session's handle against an event's session id.
    let session = program(
        "Noted(reader)",
        "emits Noted(UserId(\"u-1\"))",
        "bump(current_user())",
        "user query Visits(s: Session<SessionId>) -> Int\n    \
             cache          private\n    \
             key            s\n    \
             invalidates_on Visited(s)\n\
         {\n    0\n}\n",
    );
    refused(
        &session,
        "argument 1 of `m.Visited` is declared `capability.SessionId`",
    );
    // An event emitted with a handle: `emits` is no listener.
    refused(
        &program(
            "Noted(reader)",
            "emits Noted(current_user())",
            "bump(current_user())",
            "",
        ),
        "argument 1 of `m.Noted` is declared `capability.UserId`",
    );
    // A call given a handle where it takes an id.
    refused(
        &program(
            "Noted(reader)",
            "emits Noted(UserId(\"u-1\"))",
            "named(current_user())",
            "fn named(user: UserId) -> Int !{} {\n    1\n}\n",
        ),
        "argument 1 of `m.named` is declared `capability.UserId`",
    );
    // A call given an id where it takes a handle.
    refused(
        &program(
            "Noted(reader)",
            "emits Noted(UserId(\"u-1\"))",
            "bump(UserId(\"u-1\"))",
            "",
        ),
        "argument 1 of `m.bump` is declared `capability.User<capability.UserId>`",
    );
    // An `invalidates` key given an id where the entry is keyed by a handle.
    refused(
        &program(
            "Noted(reader)",
            "invalidates Counted(UserId(\"u-1\"))",
            "bump(current_user())",
            "",
        ),
        "argument 1 of `m.Counted` is declared `capability.User<capability.UserId>`",
    );
}

/// **The platform declares the reader's user as the host's operation**, as
/// it declares the session.
#[test]
fn the_readers_user_is_the_hosts_operation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let context = std::fs::read_to_string(root.join("packages/pw-platform-web/context.pw"))
        .expect("context.pw");
    let declared = context
        .split("fn current_user()")
        .nth(1)
        .expect("current_user is declared");
    let head: String = declared.lines().take(2).collect::<Vec<_>>().join("\n");
    assert!(
        head.contains("-> User<UserId>") && head.contains("host \"pw:host/principal#read\""),
        "{head}"
    );
}
