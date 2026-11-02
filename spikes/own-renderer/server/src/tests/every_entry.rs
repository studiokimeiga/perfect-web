//! **An entry written `_` is every entry at the rest, in every session's
//! partition** (ADR-0256, the first half of ADR-0195's ruling 10).
//!
//! A command's `invalidates Timeline(current_session(), _)` hands the host
//! the session alone, and the host drops the session's entries at every
//! limit, and no other session's; `Timeline(_, 20)` drops every session's
//! at 20. And an entry is dropped in every session's partition: a private
//! query's entry is one session's copy. Until ADR-0256 a like dropped the
//! liker's copy of a private thread alone, and another reader's open thread
//! kept its count. Each test states one case, with its control.

use super::*;

/// **The feed, and two commands liking a post**: one dropping the
/// session's timelines at every limit, and one every session's at 20. A
/// command that writes nothing commits nothing, its invalidations among it,
/// and one that writes a post reaches every reader of one (PW5106). Track
/// `notifications`: `add_like` writes a notification too, so each drops
/// every reader's notifications as PW5106 asks.
pub(super) fn forgetting(app: &str) -> String {
    format!(
        "{app}\n\
         command forget(post: PostId) -> Result<Post, FeedError>\n    \
         requires      SignedIn\n    \
         invalidates   Timeline(current_session(), _), \
         FollowingTimeline(current_session(), _), Notifications(_, _), Unread(_)\n{{\n    \
         add_like(current_session(), post)\n}}\n\n\
         command forget_twenty(post: PostId) -> Result<Post, FeedError>\n    \
         requires      SignedIn\n    \
         invalidates   Timeline(_, 20), FollowingTimeline(_, 20), Notifications(_, _), \
         Unread(_)\n{{\n    \
         add_like(current_session(), post)\n}}\n"
    )
}

/// **A session's timeline at `limit`, kept**, as the host keys it
/// (`entry_key`): a session query's entries are each the session's, keyed
/// by its session, as the program holds it (its handle,
/// `identity::session_handle`, track `data-path`), and its limit. A session
/// query declares no window, so a page reads it again each time; the test
/// keeps the entries itself, to see which a command drops.
pub(super) fn timeline_at(session: &str, limit: i64) -> pw_resource::Key {
    let handle = crate::identity::session_handle(session);
    pw_resource::Key::new(
        "feed.app.Timeline",
        &format!("session={session}\u{1f}\"{handle}\"\u{1f}{limit}"),
    )
}

/// Keep each of `keys`, read as `kept`.
pub(super) fn keep(s: &Server, keys: &[pw_resource::Key]) {
    let manifest = pw_resource::Manifest::new("feed.app.Timeline").freshness(60_000);
    for key in keys {
        s.queries
            .fetch(&manifest, key, |_| Ok(Arc::new(Val::String("kept".into()))));
    }
}

/// Which of `keys` are still kept.
pub(super) fn kept(s: &Server, keys: &[pw_resource::Key]) -> Vec<pw_resource::Key> {
    let manifest = pw_resource::Manifest::new("feed.app.Timeline").freshness(60_000);
    keys.iter()
        .filter(|key| {
            matches!(
                s.queries.fetch(&manifest, key, |_| Ok(Arc::new(Val::String(
                    "again".into()
                )))),
                pw_resource::Fetched::FromCache(_)
            )
        })
        .cloned()
        .collect()
}

#[test]
fn an_entry_written_with_a_wildcard_is_every_entry_at_the_rest() {
    let s = served_feed_with(forgetting);
    let keys = [
        timeline_at("a", 20),
        timeline_at("a", 40),
        timeline_at("b", 20),
    ];
    keep(&s, &keys);
    assert_eq!(kept(&s, &keys), keys, "the control: each is kept");
    let answered = s
        .command_answered(
            "feed.app.forget",
            "a",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(
        kept(&s, &keys),
        [timeline_at("b", 20)],
        "a's timeline at every limit was dropped, and b's kept"
    );
}

#[test]
fn a_wildcard_may_stand_first() {
    // `Timeline(_, 20)`: every session's timeline at 20, the first
    // position left to every value and the second given.
    let s = served_feed_with(forgetting);
    let keys = [
        timeline_at("a", 20),
        timeline_at("a", 40),
        timeline_at("b", 20),
    ];
    keep(&s, &keys);
    let answered = s
        .command_answered(
            "feed.app.forget_twenty",
            "a",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(
        kept(&s, &keys),
        [timeline_at("a", 40)],
        "each session's timeline at 20 was dropped"
    );
}

/// **The feed, its thread cached per reader**: a private query keyed by
/// something other than the session. Anchored on the thread's own
/// declaration: until 2026-10-07 this made the first shared cache keyed by
/// `id` private, which the follows timeline made the profile's (ADR-0257),
/// and the thread stayed shared.
fn private_threads(app: &str) -> String {
    let shared = "public query Thread(id: PostId) -> Result<Post, FeedError>\n    \
                  freshness      5.seconds\n    consistency    snapshot\n    \
                  cache          shared\n";
    assert_eq!(app.matches(shared).count(), 1, "the thread's cache, once");
    app.replace(
        shared,
        &shared.replace("cache          shared", "cache          private"),
    )
}

#[test]
fn a_private_entry_is_dropped_in_every_sessions_partition() {
    let s = served_feed_with(private_threads);
    let thread = Params::from([("id".to_string(), "p1".to_string())]);
    for session in ["a", "b"] {
        let (html, _, _, _) = s
            .serve_document_settled(session, "feed.app.PostPage~ui", &thread, &[])
            .expect("served");
        assert!(html.contains("2 likes"), "{html}");
    }
    let theirs = latest(&s.pending.lock().expect("pending"), "b");
    let answered = s
        .command_answered(
            "feed.app.like",
            "a",
            &[Val::String("p1".into())],
            Some("i-1"),
        )
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    s.tell_waiting();
    let set = format!(
        "{:?}",
        sets_of(&s, &theirs)
            .pop()
            .expect("b's open thread is told of the like")
    );
    assert!(set.contains("3 likes"), "{set}");
}

#[test]
fn an_entry_is_named_by_the_values_given_whatever_the_rest() {
    let shared = serde_json::json!({ "cache": "shared", "privacy": "public", "key": [0, 1] });
    let private = serde_json::json!({ "cache": "private", "privacy": "private", "key": [0, 1] });
    let given = |v: &str| Some(Val::String(v.into()));
    // Given each value, one entry.
    assert!(names_entry(
        &shared,
        &[given("x"), Some(Val::S64(20))],
        "\"x\"\u{1f}20"
    ));
    assert!(!names_entry(
        &shared,
        &[given("x"), Some(Val::S64(20))],
        "\"x\"\u{1f}40"
    ));
    // Given the first, every entry at it.
    assert!(names_entry(&shared, &[given("x"), None], "\"x\"\u{1f}40"));
    assert!(!names_entry(&shared, &[given("y"), None], "\"x\"\u{1f}40"));
    // In any session's partition, and a private entry is always one's.
    assert!(names_entry(
        &private,
        &[given("x"), None],
        "session=b\u{1f}\"x\"\u{1f}40"
    ));
    assert!(!names_entry(&private, &[given("x"), None], "\"x\"\u{1f}40"));
}
