//! **Notifications** (track `notifications`, ADR-0274): a like, a reply or a
//! follow writes a row for the user it involves, in its own transaction;
//! one's own act notifies no one; a deleted post's notifications go with it;
//! a user's unread count and list are theirs alone, by page, by `/pw-read`,
//! by cache and by session, and reach each of their sessions live. On the
//! in-memory layer and on PostgreSQL. Each test states one case, with its
//! control.

use super::*;

/// What `session`'s page `page` shows, as text.
fn shown(s: &Server, session: &str, page: &str) -> String {
    let (html, _, _, _) = s
        .serve_document_settled(session, page, &Params::new(), &[])
        .expect("served");
    visible(&html)
}

/// What a document was sent since it was served, as the text it writes.
fn told(s: &Server, doc: &Doc) -> String {
    sets_of(s, doc)
        .iter()
        .map(|set| visible(&written(set)))
        .collect::<Vec<_>>()
        .join("\n")
}

const HOME: &str = "feed.app.Home~ui";
const PAGE: &str = "feed.app.NotificationsPage~ui";

/// The accounts model: `alice` in two sessions, `bob` in one, and `nobody`,
/// a session no one signed in to.
fn people(s: &Server) {
    s.identity.signed_in_for_test("s-alice-1", "alice", "Alice");
    s.identity.signed_in_for_test("s-alice-2", "alice", "Alice");
    s.identity.signed_in_for_test("s-bob", "bob", "Bob");
}

fn run(s: &Server, command: &str, session: &str, args: &[&str], interaction: &str) -> Answered {
    let args: Vec<Val> = args.iter().map(|a| Val::String(a.to_string())).collect();
    let answered = s
        .command_answered(
            &format!("feed.app.{command}"),
            session,
            &args,
            Some(interaction),
        )
        .expect("runs");
    assert!(answered.committed, "{command}: {:?}", answered.result);
    answered
}

/// **A post of `session`'s**: its id.
fn posted(s: &Server, session: &str, text: &str, interaction: &str) -> String {
    run(s, "post", session, &[text], interaction);
    let (html, _, _, _) = s
        .serve_document_settled(session, HOME, &Params::new(), &[])
        .expect("served");
    let at = html.find(text).expect("the post is shown");
    let link = html[..at].rfind("href=\"/post/").expect("its link") + 12;
    html[link..link + html[link..].find('"').expect("its end")].to_string()
}

/// The count the home page shows its reader.
fn count_on_home(s: &Server, session: &str) -> String {
    let home = shown(s, session, HOME);
    let at = home.find(" unread").unwrap_or_else(|| panic!("{home}"));
    let before = home[..at].trim_end();
    let digits = before.len() - before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    before[before.len() - digits..].to_string()
}

/// **A like, a reply and a follow each tell the user they involve**: a row
/// each, written with what the act wrote, shown on their page, and counted
/// on their home page.
fn each_act_tells_its_user(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    assert_eq!(count_on_home(s, "s-alice-1"), "0");
    run(s, "like", "s-bob", &[&post], "i-2");
    run(s, "reply", "s-bob", &[&post, "Bob answers"], "i-3");
    run(s, "follow", "s-bob", &["alice"], "i-4");
    assert_eq!(count_on_home(s, "s-alice-1"), "3");
    let page = shown(s, "s-alice-2", PAGE);
    for said in [
        "liked your post",
        "replied to your post",
        "followed you",
        "Bob",
        "3 unread",
    ] {
        assert!(page.contains(said), "{said}: {page}");
    }
    // Following again writes nothing, and tells no one.
    run(s, "follow", "s-bob", &["alice"], "i-5");
    assert_eq!(count_on_home(s, "s-alice-2"), "3");
    // The control: bob was told nothing.
    assert_eq!(count_on_home(s, "s-bob"), "0");
}

#[test]
fn a_like_a_reply_and_a_follow_each_tell_the_user_they_involve() {
    each_act_tells_its_user(&served_feed());
}

/// **One's own act notifies no one.**
fn ones_own_act_is_quiet(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    run(s, "like", "s-alice-1", &[&post], "i-2");
    run(s, "reply", "s-alice-2", &[&post, "Alice again"], "i-3");
    assert_eq!(count_on_home(s, "s-alice-1"), "0");
    let page = shown(s, "s-alice-1", PAGE);
    assert!(page.contains("Nothing yet"), "{page}");
    // The control: another's like does.
    run(s, "like", "s-bob", &[&post], "i-4");
    assert_eq!(count_on_home(s, "s-alice-1"), "1");
}

#[test]
fn ones_own_act_notifies_no_one() {
    ones_own_act_is_quiet(&served_feed());
}

/// **A deleted post's notifications go with it**, and its replies'; a
/// follow's stays.
fn deleted_with_its_post(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    let kept = posted(s, "s-alice-1", "Alice keeps this", "i-2");
    run(s, "like", "s-bob", &[&post], "i-3");
    run(s, "reply", "s-bob", &[&post, "Bob answers"], "i-4");
    run(s, "like", "s-bob", &[&kept], "i-5");
    run(s, "follow", "s-bob", &["alice"], "i-6");
    assert_eq!(count_on_home(s, "s-alice-1"), "4");
    run(s, "delete", "s-alice-1", &[&post], "i-7");
    assert_eq!(count_on_home(s, "s-alice-1"), "2");
    let page = shown(s, "s-alice-1", PAGE);
    assert!(!page.contains("replied to your post"), "{page}");
    assert!(page.contains("liked your post"), "{page}");
    assert!(page.contains("followed you"), "{page}");
}

#[test]
fn a_deleted_posts_notifications_go_with_it() {
    deleted_with_its_post(&served_feed());
}

/// **What a like tells reaches each of its user's open pages**, and no
/// other user's page is sent it.
fn reaches_each_session(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    for session in ["s-alice-1", "s-alice-2", "s-bob"] {
        assert!(shown(s, session, HOME).contains("Notifications: 0 unread"));
    }
    let docs: Vec<Doc> = {
        let pending = s.pending.lock().expect("pending");
        ["s-alice-1", "s-alice-2", "s-bob"]
            .iter()
            .map(|session| latest(&pending, session))
            .collect()
    };
    run(s, "like", "s-bob", &[&post], "i-2");
    s.tell_waiting();
    // The count, set where it is in the block that shows it (ADR-0320).
    for doc in &docs[..2] {
        let counts = texts_told(s, doc, HOME, "waiting");
        assert_eq!(counts, ["1"], "{doc:?}: {}", told(s, doc));
    }
    // The control: bob's own page is told nothing of alice's.
    let bobs = texts_told(s, &docs[2], HOME, "waiting");
    assert!(bobs.is_empty(), "{bobs:?}: {}", told(s, &docs[2]));
}

/// **An entry at a user, as the host keys it** (`entry_key`): a private
/// query's entries are each a session's, keyed by its session, then by the
/// user's handle, which is their id on the wire, and the rest.
fn at_user(query: &str, session: &str, user: &str, rest: &str) -> pw_resource::Key {
    pw_resource::Key::new(
        &format!("feed.app.{query}"),
        &format!("session={session}\u{1f}\"{user}\"{rest}"),
    )
}

/// Keep each of `keys`, as `every_entry::keep` does, each under its own
/// query's manifest.
fn keep(s: &Server, keys: &[pw_resource::Key]) {
    for key in keys {
        let manifest = pw_resource::Manifest::new(&key.resource).freshness(60_000);
        s.queries
            .fetch(&manifest, key, |_| Ok(Arc::new(Val::String("kept".into()))));
    }
}

/// Which of `keys` are still kept.
fn kept(s: &Server, keys: &[pw_resource::Key]) -> Vec<pw_resource::Key> {
    keys.iter()
        .filter(|key| {
            let manifest = pw_resource::Manifest::new(&key.resource).freshness(60_000);
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

/// **An event naming a user drops the entries at that user in each of their
/// sessions, and no other user's** (ADR-0091 as amended, ADR-0256). A
/// private query declares no window, so a page reads it again each time;
/// the test keeps the entries itself, to see which a commit drops.
fn drops_at_the_user_named(s: &Server) {
    people(s);
    // Alice is someone the feed knows once she posts; no page is read, so
    // nothing of hers is kept but what the test keeps.
    run(s, "post", "s-alice-1", &["Alice writes"], "i-1");
    let keys = [
        at_user("Unread", "s-alice-1", "alice", ""),
        at_user("Unread", "s-alice-2", "alice", ""),
        at_user("Notifications", "s-alice-2", "alice", "\u{1f}20"),
        at_user("Unread", "s-bob", "bob", ""),
        at_user("Notifications", "s-bob", "bob", "\u{1f}20"),
    ];
    keep(s, &keys);
    assert_eq!(kept(s, &keys), keys, "the control: each is kept");
    // `Followed(follower, followee)`: the listeners' `Followed(_, reader)`.
    run(s, "follow", "s-bob", &["alice"], "i-2");
    assert_eq!(
        kept(s, &keys),
        keys[3..],
        "alice's were dropped in each of her sessions, and bob's kept"
    );
    // `Notified(user)`, by a like of alice's post: the like's `Notified`.
    keep(s, &keys);
    let post = shown(s, "s-nobody", HOME);
    assert!(post.contains("Alice writes"), "{post}");
    let (html, _, _, _) = s
        .serve_document_settled("s-nobody", HOME, &Params::new(), &[])
        .expect("served");
    let at = html.find("Alice writes").expect("the post");
    let link = html[..at].rfind("href=\"/post/").expect("its link") + 12;
    let id = &html[link..link + html[link..].find('"').expect("its end")];
    run(s, "like", "s-bob", &[id], "i-3");
    assert_eq!(
        kept(s, &keys),
        keys[3..],
        "a like drops alice's in each of her sessions, and keeps bob's"
    );
}

#[test]
fn an_event_naming_a_user_drops_that_users_entries_alone() {
    drops_at_the_user_named(&served_feed());
}

#[test]
fn a_like_reaches_each_of_its_users_open_pages() {
    reaches_each_session(&served_feed());
}

/// **Reading them reaches each of the reader's sessions**: the count and
/// the list read again; another user's are theirs.
fn read_everywhere(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    let bobs = posted(s, "s-bob", "Bob writes", "i-2");
    run(s, "like", "s-bob", &[&post], "i-3");
    run(s, "like", "s-alice-1", &[&bobs], "i-4");
    assert!(shown(s, "s-alice-2", HOME).contains("Notifications: 1 unread"));
    assert!(shown(s, "s-bob", HOME).contains("Notifications: 1 unread"));
    let docs: Vec<Doc> = {
        let pending = s.pending.lock().expect("pending");
        ["s-alice-2", "s-bob"]
            .iter()
            .map(|session| latest(&pending, session))
            .collect()
    };
    run(s, "mark_read", "s-alice-1", &[], "i-5");
    s.tell_waiting();
    let other = texts_told(s, &docs[0], HOME, "waiting");
    assert_eq!(other, ["0"], "{}", told(s, &docs[0]));
    assert_eq!(count_on_home(s, "s-alice-2"), "0");
    let page = shown(s, "s-alice-2", PAGE);
    assert!(
        page.contains("liked your post") && !page.contains("New"),
        "{page}"
    );
    // The control: bob's are his, unread.
    let bobs = texts_told(s, &docs[1], HOME, "waiting");
    assert!(
        !bobs.contains(&"0".to_string()),
        "{bobs:?}: {}",
        told(s, &docs[1])
    );
    assert_eq!(count_on_home(s, "s-bob"), "1");
}

#[test]
fn reading_them_reaches_each_of_the_readers_sessions() {
    read_everywhere(&served_feed());
}

/// **Telling by principal** (track `store-accounts`, ADR-0318): reading
/// one's notifications drops the reader's entries alone, which are keyed by
/// the reader's handle, so another user's open page is sent no frame at all,
/// where ADR-0270's superset sent it a version with nothing in it: another
/// user's activity, to every reader. The reader's other session is told.
fn read_tells_no_one_else(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    run(s, "like", "s-bob", &[&post], "i-2");
    for session in ["s-alice-2", "s-bob"] {
        shown(s, session, HOME);
    }
    // What the post and the like reached, told first.
    s.tell_waiting();
    let docs: Vec<Doc> = {
        let pending = s.pending.lock().expect("pending");
        ["s-alice-2", "s-bob"]
            .iter()
            .map(|session| latest(&pending, session))
            .collect()
    };
    let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
    let before: Vec<usize> = docs.iter().map(frames).collect();
    run(s, "mark_read", "s-alice-1", &[], "i-3");
    s.tell_waiting();
    assert!(
        frames(&docs[0]) > before[0],
        "alice's other session is told"
    );
    assert_eq!(frames(&docs[1]), before[1], "bob's page is sent no frame");
}

#[test]
fn reading_them_tells_no_other_users_page() {
    read_tells_no_one_else(&served_feed());
}

#[test]
fn on_postgres_reading_them_tells_no_other_users_page() {
    let Some(s) = on_postgres("notify_principal") else {
        return;
    };
    read_tells_no_one_else(&s);
}

/// **Another user's session sees none of it, signed in and not**: by page,
/// by `/pw-read`, by the cache every reader shares, and by session.
fn none_of_it_elsewhere(s: &Server) {
    people(s);
    let post = posted(s, "s-alice-1", "Alice writes", "i-1");
    run(s, "like", "s-bob", &[&post], "i-2");
    run(s, "follow", "s-bob", &["alice"], "i-3");
    assert_eq!(count_on_home(s, "s-alice-1"), "2");
    for session in ["s-bob", "s-nobody"] {
        // By page.
        let page = shown(s, session, PAGE);
        assert!(page.contains("Nothing yet"), "{session}: {page}");
        assert!(page.contains("0 unread"), "{session}: {page}");
        assert!(!page.contains("liked your post"), "{session}: {page}");
        // By `/pw-read`: the page's list read again, for a key the browser
        // asks, is the session's own.
        let doc = latest(&s.pending.lock().expect("pending"), session);
        let key = BTreeMap::from([("listed".to_string(), serde_json::json!(40))]);
        let read = s
            .read_keyed(session, "notes", 1, doc.1, &key, &|| false)
            .expect("read");
        assert!(matches!(read, KeyOutcome::Applied), "{session}");
        let sent = told(s, &doc);
        assert!(!sent.contains("liked your post"), "{session}: {sent}");
        assert!(!sent.contains("followed you"), "{session}: {sent}");
        // And alice's open page, asked for by its number from this session,
        // is no page of this session's: nothing is read for it.
        assert!(shown(s, "s-alice-1", PAGE).contains("liked your post"));
        let alices = latest(&s.pending.lock().expect("pending"), "s-alice-1");
        let theirs = s.read_keyed(session, "notes", 2, alices.1, &key, &|| false);
        assert!(
            matches!(theirs, Ok(KeyOutcome::Superseded) | Err(_)),
            "{session}: {:?}",
            theirs.map(|_| ())
        );
        let sent = told(s, &doc);
        assert!(!sent.contains("liked your post"), "{session}: {sent}");
    }
    // By cache: nothing of anyone's notifications is kept for every reader.
    let public = format!("{:?}", s.queries.public_cache_contents());
    for private in ["feed.app.Notifications", "feed.app.Unread"] {
        assert!(!public.contains(private), "{public}");
    }
    // By session: every entry read is in a session's partition, keyed by
    // that session's own user.
    let principals = s.identity.principals();
    let mut read = 0;
    for t in s.queries.trace() {
        let (pw_resource::Trace::RequestSucceeded { key }
        | pw_resource::Trace::ServedFromCache { key }) = t
        else {
            continue;
        };
        if !["feed.app.Notifications", "feed.app.Unread"].contains(&key.resource.as_str()) {
            continue;
        }
        let mut parts = key.key.split('\u{1f}');
        let partition = parts.next().expect("private partition field");
        // ADR-0389: a private field names the cookie AND its actor epoch.
        // Match exactly one fixture reader; do not mistake the versioned
        // partition for a raw cookie or accept a shared/unknown partition.
        let sessions = ["s-alice-1", "s-alice-2", "s-bob", "s-nobody"];
        let owners: Vec<_> = sessions
            .iter()
            .filter(|session| {
                partition
                    == format!(
                        "session={}",
                        principals.bound_reader(session).partition(session)
                    )
            })
            .collect();
        assert_eq!(
            owners.len(),
            1,
            "{key:?} must belong to exactly one fixture reader"
        );
        let session = *owners[0];
        let user = parts.next().expect("keyed by its user");
        assert_eq!(
            user,
            serde_json::json!(principals.user_of(session)).to_string(),
            "{key:?}"
        );
        read += 1;
    }
    assert!(read > 0, "the entries were read");
}

#[test]
fn another_users_session_sees_none_of_it() {
    none_of_it_elsewhere(&served_feed());
}

/// The feed on PostgreSQL, where the environment names a database.
fn on_postgres(test: &str) -> Option<super::feed_pg::OnPostgres> {
    let url = match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
            return None;
        }
    };
    Some(super::feed_pg::served_on(&url, test))
}

#[test]
fn on_postgres_a_like_a_reply_and_a_follow_each_tell_the_user_they_involve() {
    let Some(s) = on_postgres("notify_acts") else {
        return;
    };
    each_act_tells_its_user(&s);
    // Each row written in the transaction of the act it tells of.
    assert_eq!(
        s.db.count(
            "SELECT count(*) FROM notifications n WHERE NOT EXISTS ( \
                 SELECT 1 FROM likes l WHERE l.committed_in = n.committed_in \
                 UNION ALL SELECT 1 FROM posts p WHERE p.committed_in = n.committed_in \
                 UNION ALL SELECT 1 FROM follows f WHERE f.committed_in = n.committed_in)"
        ),
        0
    );
    assert_eq!(s.db.count("SELECT count(*) FROM notifications"), 3);
}

#[test]
fn on_postgres_ones_own_act_notifies_no_one() {
    let Some(s) = on_postgres("notify_own") else {
        return;
    };
    ones_own_act_is_quiet(&s);
    assert_eq!(
        s.db.count("SELECT count(*) FROM notifications WHERE recipient = actor"),
        0
    );
}

#[test]
fn on_postgres_a_deleted_posts_notifications_go_with_it() {
    let Some(s) = on_postgres("notify_deleted") else {
        return;
    };
    deleted_with_its_post(&s);
    assert_eq!(
        s.db.count(
            "SELECT count(*) FROM notifications n \
             WHERE n.post IS NOT NULL AND NOT EXISTS (SELECT 1 FROM posts p WHERE p.id = n.post)"
        ),
        0
    );
}

#[test]
fn on_postgres_a_like_reaches_each_of_its_users_open_pages() {
    let Some(s) = on_postgres("notify_reach") else {
        return;
    };
    reaches_each_session(&s);
}

#[test]
fn on_postgres_reading_them_reaches_each_of_the_readers_sessions() {
    let Some(s) = on_postgres("notify_read") else {
        return;
    };
    read_everywhere(&s);
}

#[test]
fn on_postgres_an_event_naming_a_user_drops_that_users_entries_alone() {
    let Some(s) = on_postgres("notify_drops") else {
        return;
    };
    drops_at_the_user_named(&s);
}

#[test]
fn on_postgres_another_users_session_sees_none_of_it() {
    let Some(s) = on_postgres("notify_private") else {
        return;
    };
    none_of_it_elsewhere(&s);
}
