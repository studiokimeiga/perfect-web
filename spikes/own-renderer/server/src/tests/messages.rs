//! **Direct messages** (track `messages`, ADR-0279): a message is a row of
//! its own, written in its command's transaction; a conversation is read
//! from each side, private to its reader; who may message whom is X's rule,
//! held by `requires MayMessage(to)`; reading is a command; and a third user,
//! signed in and not, sees none of it, by page, by `/pw-read`, by cache, by
//! session and by stream. On the in-memory layer and on PostgreSQL. Each
//! test states one case, with its control.

use super::*;

/// What `session`'s page `page` shows, as text.
fn shown(s: &Server, session: &str, page: &str, params: &Params) -> String {
    visible(&html_of(s, session, page, params))
}

/// The page `page` served to `session`, as HTML.
fn html_of(s: &Server, session: &str, page: &str, params: &Params) -> String {
    let (html, _, _, _) = s
        .serve_document_settled(session, page, params, &[])
        .expect("served");
    html
}

/// What a document was sent since it was served, as the text it writes.
fn told(s: &Server, doc: &Doc) -> String {
    sets_of(s, doc)
        .iter()
        .map(|set| visible(&written(set)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every frame a document was sent since it was served, whatever it
/// writes: a row inserted into a list as much as a text replaced.
fn frames(s: &Server, doc: &Doc) -> String {
    format!("{:?}", sets_of(s, doc))
}

const HOME: &str = "feed.app.Home~ui";
const LIST: &str = "feed.app.MessagesPage~ui";
const TALK: &str = "feed.app.ConversationPage~ui";

/// The conversation page's parameters, with `with`.
fn with(user: &str) -> Params {
    Params::from([("with".to_string(), user.to_string())])
}

/// The accounts model: `alice` in two sessions, `bob` in one, `carol`, a
/// third user, in one, and `nobody`, a session no one signed in to.
fn people(s: &Server) {
    s.identity.signed_in_for_test("s-alice-1", "alice", "Alice");
    s.identity.signed_in_for_test("s-alice-2", "alice", "Alice");
    s.identity.signed_in_for_test("s-bob", "bob", "Bob");
    s.identity.signed_in_for_test("s-carol", "carol", "Carol");
    // Each is someone the feed knows once they post, by the name their
    // provider gave.
    for (session, said) in [
        ("s-alice-1", "Alice is here"),
        ("s-bob", "Bob is here"),
        ("s-carol", "Carol is here"),
    ] {
        run(s, "post", session, &[said], &format!("i-{session}"));
    }
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

/// One HTTP request to `s`, written as given, and the whole answer.
fn exchanged(s: &Server, request: &str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client.write_all(request.as_bytes()).expect("request");
        let mut answer = String::new();
        let _ = client.read_to_string(&mut answer);
        answer
    })
}

/// **`send`, as a browser posts it**: the whole answer.
fn send_over_http(s: &Server, session: &str, to: &str, text: &str, interaction: &str) -> String {
    let body = serde_json::json!([to, text]).to_string();
    exchanged(
        s,
        &format!(
            "POST /command/feed.app.send HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\npw-interaction: {interaction}\r\n\
             content-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// The count of unread messages the home page shows its reader.
fn count_on_home(s: &Server, session: &str) -> String {
    let home = shown(s, session, HOME, &Params::new());
    let at = home.find("Messages: ").unwrap_or_else(|| panic!("{home}")) + "Messages: ".len();
    home[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// Whether `session`'s conversation page with `user` hides its composer.
fn composer_hidden(s: &Server, session: &str, user: &str) -> bool {
    let html = html_of(s, session, TALK, &with(user));
    let at = html
        .find("id=\"composer\"")
        .unwrap_or_else(|| panic!("{html}"));
    let tag = &html[at..at + html[at..].find('>').expect("its end")];
    tag.split_whitespace()
        .any(|a| a == "hidden" || a.starts_with("hidden="))
}

/// **A message is shown on both sides, and counted unread for its
/// recipient alone**: a row of its own, in the list of each, on the
/// conversation page of each, and sending marks the sender's side read.
fn shown_on_both_sides(s: &Server) {
    people(s);
    // Bob follows Alice, so Alice may message him.
    run(s, "follow", "s-bob", &["alice"], "i-1");
    run(s, "send", "s-alice-1", &["bob", "Hello, Bob."], "i-2");
    assert_eq!(count_on_home(s, "s-bob"), "1");
    assert_eq!(count_on_home(s, "s-alice-2"), "0");
    let bobs = shown(s, "s-bob", LIST, &Params::new());
    for said in ["Alice", "Hello, Bob.", "1 unread", "1 unread message"] {
        assert!(bobs.contains(said), "{said}: {bobs}");
    }
    let talk = shown(s, "s-bob", TALK, &with("alice"));
    assert!(
        talk.contains("Hello, Bob.") && talk.contains("New"),
        "{talk}"
    );
    let alices = shown(s, "s-alice-2", TALK, &with("bob"));
    assert!(
        alices.contains("Hello, Bob.") && !alices.contains("New"),
        "{alices}"
    );
    let list = shown(s, "s-alice-2", LIST, &Params::new());
    assert!(
        list.contains("Bob") && list.contains("0 unread messages"),
        "{list}"
    );
    // Bob answers, which he may: Alice has messaged him. His side is read.
    run(s, "send", "s-bob", &["alice", "Hello, Alice."], "i-3");
    assert_eq!(count_on_home(s, "s-bob"), "0");
    assert_eq!(count_on_home(s, "s-alice-1"), "1");
    let talk = shown(s, "s-bob", TALK, &with("alice"));
    assert!(
        talk.find("Hello, Bob.") < talk.find("Hello, Alice."),
        "oldest first: {talk}"
    );
    // The control: a message is no post, and notifies no one; the follow's
    // notification is alice's one.
    assert!(shown(s, "s-bob", HOME, &Params::new()).contains("Notifications: 0 unread"));
    assert!(shown(s, "s-alice-1", HOME, &Params::new()).contains("Notifications: 1 unread"));
    assert!(!shown(s, "s-nobody", HOME, &Params::new()).contains("Hello, Bob."));
}

#[test]
fn a_message_is_shown_on_both_sides_and_counted_for_its_recipient() {
    shown_on_both_sides(&served_feed());
}

/// **Who may message whom is X's rule**: the recipient follows the sender,
/// or has messaged them before; no one messages themselves; a guest sends
/// nothing. A refusal is 403, `"refused":"MayMessage"`, and writes nothing;
/// the page shows the composer only where its reader may send, and says
/// why where not.
fn who_may_message_whom(s: &Server) {
    people(s);
    // A stranger may not.
    let refused = send_over_http(s, "s-alice-1", "bob", "Hello?", "i-1");
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(refused.contains("\"refused\":\"MayMessage\""), "{refused}");
    assert!(composer_hidden(s, "s-alice-1", "bob"));
    let page = shown(s, "s-alice-1", TALK, &with("bob"));
    assert!(page.contains("once they follow you"), "{page}");
    // Following someone is no leave to message them.
    run(s, "follow", "s-alice-1", &["bob"], "i-2");
    let refused = send_over_http(s, "s-alice-1", "bob", "Hello?", "i-3");
    assert!(refused.contains("\"refused\":\"MayMessage\""), "{refused}");
    // Being followed is.
    run(s, "follow", "s-bob", &["alice"], "i-4");
    assert!(!composer_hidden(s, "s-alice-1", "bob"));
    let sent = send_over_http(s, "s-alice-1", "bob", "Hello, Bob.", "i-5");
    assert!(sent.starts_with("HTTP/1.1 20"), "{sent}");
    assert!(sent.contains("\"committed\":true"), "{sent}");
    // Carol, whom no one follows, may answer whoever messaged her.
    run(s, "follow", "s-carol", &["alice"], "i-6");
    run(s, "send", "s-alice-2", &["carol", "Hello, Carol."], "i-7");
    run(s, "unfollow", "s-carol", &["alice"], "i-8");
    run(s, "send", "s-carol", &["alice", "Hello, Alice."], "i-9");
    // And no one messages themselves, though she follows herself in no way.
    let selfish = send_over_http(s, "s-alice-1", "alice", "Me?", "i-10");
    assert!(selfish.contains("\"refused\":\"MayMessage\""), "{selfish}");
    let page = shown(s, "s-alice-1", TALK, &with("alice"));
    assert!(page.contains("You cannot message yourself."), "{page}");
    assert!(composer_hidden(s, "s-alice-1", "alice"));
    // A guest sends nothing.
    let guest = send_over_http(s, "s-nobody", "alice", "Hi", "i-11");
    assert!(guest.contains("\"refused\":\"SignedIn\""), "{guest}");
    assert!(shown(s, "s-nobody", TALK, &with("alice")).contains("Sign in to send messages."));
    // What was refused wrote nothing: bob was sent one message, alice one.
    assert_eq!(count_on_home(s, "s-bob"), "1");
    assert_eq!(count_on_home(s, "s-alice-1"), "1");
    let bobs = shown(s, "s-bob", TALK, &with("alice"));
    assert!(!bobs.contains("Hello?"), "{bobs}");
}

#[test]
fn who_may_message_whom_is_xs_rule() {
    who_may_message_whom(&served_feed());
}

/// **A message's text is 1 to 10,000 code points**, held where it arrives,
/// as a post's is.
#[test]
fn a_messages_text_is_held_to_its_length_where_it_arrives() {
    let s = served_feed();
    people(&s);
    run(&s, "follow", "s-bob", &["alice"], "i-1");
    for (text, said) in [
        (String::new(), "0 code points long"),
        ("x".repeat(10_001), "10001 code points long"),
    ] {
        let refused = s
            .command_json(
                "feed.app.send",
                "s-alice-1",
                &[serde_json::json!("bob"), serde_json::json!(text)],
                Some("i-no"),
            )
            .expect_err("refused before it runs");
        assert!(
            refused.contains(said) && refused.contains("feed.app.MessageText"),
            "{refused}"
        );
    }
    assert_eq!(count_on_home(&s, "s-bob"), "0", "nothing was sent");
    s.command_json(
        "feed.app.send",
        "s-alice-1",
        &[
            serde_json::json!("bob"),
            serde_json::json!("\u{1F600}".repeat(10_000)),
        ],
        Some("i-yes"),
    )
    .expect("well-formed")
    .expect("committed");
    assert_eq!(count_on_home(&s, "s-bob"), "1");
}

/// **Reading a conversation reaches each of the reader's sessions**: the
/// conversation, the list and the count read again; the other's, and a
/// third user's, are theirs.
fn read_everywhere(s: &Server) {
    people(s);
    run(s, "follow", "s-alice-1", &["bob"], "i-1");
    run(s, "follow", "s-bob", &["alice"], "i-2");
    run(s, "follow", "s-carol", &["alice"], "i-3");
    run(s, "send", "s-bob", &["alice", "One"], "i-4");
    run(s, "send", "s-bob", &["alice", "Two"], "i-5");
    run(s, "send", "s-alice-1", &["bob", "Three"], "i-6");
    // Alice's answer read her side: two of Bob's, then hers.
    assert_eq!(count_on_home(s, "s-alice-2"), "0");
    run(s, "send", "s-bob", &["alice", "Four"], "i-7");
    run(s, "send", "s-alice-2", &["carol", "For Carol"], "i-8");
    assert_eq!(count_on_home(s, "s-alice-2"), "1");
    assert_eq!(count_on_home(s, "s-carol"), "1");
    let docs: Vec<Doc> = {
        let pending = s.pending.lock().expect("pending");
        ["s-alice-2", "s-carol"]
            .iter()
            .map(|session| latest(&pending, session))
            .collect()
    };
    run(s, "read_conversation", "s-alice-1", &["bob"], "i-9");
    s.tell_waiting();
    // The count, set where it is in the block that shows it (ADR-0320).
    let other = texts_told(s, &docs[0], HOME, "unread_dms");
    assert_eq!(other, ["0"], "{}", told(s, &docs[0]));
    let talk = shown(s, "s-alice-2", TALK, &with("bob"));
    assert!(talk.contains("Four") && !talk.contains("New"), "{talk}");
    // The control: Carol's are hers, unread.
    let carols = texts_told(s, &docs[1], HOME, "unread_dms");
    assert!(
        !carols.contains(&"0".to_string()),
        "{carols:?}: {}",
        told(s, &docs[1])
    );
    assert_eq!(count_on_home(s, "s-carol"), "1");
}

#[test]
fn reading_a_conversation_reaches_each_of_the_readers_sessions() {
    read_everywhere(&served_feed());
}

/// **A message reaches each of both users' sessions, live, and no third
/// user's**: their open pages are sent it; a third user's open
/// conversation and list are sent no frame holding its text.
fn reaches_both_and_no_third(s: &Server) {
    people(s);
    run(s, "follow", "s-bob", &["alice"], "i-1");
    run(s, "follow", "s-carol", &["alice"], "i-2");
    let open = [
        ("s-alice-1", TALK, with("bob")),
        ("s-alice-2", HOME, Params::new()),
        ("s-bob", LIST, Params::new()),
        ("s-bob", TALK, with("alice")),
        ("s-carol", TALK, with("alice")),
        ("s-carol", LIST, Params::new()),
        ("s-nobody", TALK, with("alice")),
    ];
    let mut docs = Vec::new();
    for (session, page, params) in &open {
        shown(s, session, page, params);
        docs.push(latest(&s.pending.lock().expect("pending"), session));
    }
    run(s, "send", "s-alice-2", &["bob", "Only for Bob"], "i-3");
    s.tell_waiting();
    let alices = frames(s, &docs[0]);
    assert!(
        alices.contains("Only for Bob"),
        "alice's other session: {alices}"
    );
    let home = frames(s, &docs[1]);
    assert!(!home.contains("Only for Bob"), "{home}");
    let bobs = frames(s, &docs[2]);
    assert!(bobs.contains("Only for Bob"), "bob's list: {bobs}");
    let talk = frames(s, &docs[3]);
    assert!(talk.contains("Only for Bob"), "bob's conversation: {talk}");
    // The control: neither carol's pages nor a guest's are sent a frame
    // holding the text.
    for doc in &docs[4..] {
        let sent = frames(s, doc);
        assert!(!sent.contains("Only for Bob"), "{doc:?}: {sent}");
    }
}

#[test]
fn a_message_reaches_both_users_sessions_and_no_third_users() {
    reaches_both_and_no_third(&served_feed());
}

/// **Whether the reader may send is read again when the other follows
/// them**: the open conversation is told, and its composer shown.
fn opened_by_a_follow(s: &Server) {
    people(s);
    assert!(composer_hidden(s, "s-alice-1", "bob"));
    let doc = latest(&s.pending.lock().expect("pending"), "s-alice-1");
    run(s, "follow", "s-bob", &["alice"], "i-1");
    s.tell_waiting();
    let sent = frames(s, &doc);
    assert!(
        sent.contains("RemoveAttribute { name: \"hidden\" }"),
        "alice's open conversation was told its composer opens: {sent}"
    );
    assert!(!composer_hidden(s, "s-alice-1", "bob"));
    // And closed again by an unfollow.
    run(s, "unfollow", "s-bob", &["alice"], "i-2");
    assert!(composer_hidden(s, "s-alice-1", "bob"));
}

#[test]
fn a_follow_opens_the_composer_of_the_open_conversation() {
    opened_by_a_follow(&served_feed());
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

/// Keep each of `keys`, each under its own query's manifest.
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

/// **A message drops the entries of both its users, in each of their
/// sessions, and no third user's** (ADR-0091 as amended, ADR-0256): the
/// conversation between them from each side, and each one's list and
/// count; not a conversation either has with someone else.
fn drops_at_both(s: &Server) {
    people(s);
    run(s, "follow", "s-bob", &["alice"], "i-1");
    let keys = [
        at_user(
            "Conversation",
            "s-alice-1",
            "alice",
            "\u{1f}\"bob\"\u{1f}50",
        ),
        at_user("Conversations", "s-alice-2", "alice", ""),
        at_user("UnreadMessages", "s-alice-1", "alice", ""),
        at_user("Conversation", "s-bob", "bob", "\u{1f}\"alice\"\u{1f}50"),
        at_user("UnreadMessages", "s-bob", "bob", ""),
        // Kept: another conversation of alice's, and carol's.
        at_user(
            "Conversation",
            "s-alice-1",
            "alice",
            "\u{1f}\"carol\"\u{1f}50",
        ),
        at_user(
            "Conversation",
            "s-carol",
            "carol",
            "\u{1f}\"alice\"\u{1f}50",
        ),
        at_user("Conversations", "s-carol", "carol", ""),
        at_user("UnreadMessages", "s-carol", "carol", ""),
    ];
    keep(s, &keys);
    assert_eq!(kept(s, &keys), keys, "the control: each is kept");
    run(s, "send", "s-alice-2", &["bob", "Hello"], "i-2");
    assert_eq!(
        kept(s, &keys),
        keys[5..],
        "alice's and bob's were dropped in each of their sessions, and carol's kept"
    );
}

#[test]
fn a_message_drops_both_users_entries_alone() {
    drops_at_both(&served_feed());
}

/// **A third user, signed in and not, sees none of it**: by page, by
/// `/pw-read`, by the cache every reader shares, and by session.
fn none_of_it_elsewhere(s: &Server) {
    people(s);
    run(s, "follow", "s-bob", &["alice"], "i-1");
    run(s, "send", "s-alice-1", &["bob", "Between us two"], "i-2");
    run(s, "send", "s-bob", &["alice", "Just so"], "i-3");
    for session in ["s-carol", "s-nobody"] {
        // By page: her list, and her conversations with each.
        let list = shown(s, session, LIST, &Params::new());
        assert!(list.contains("No messages yet"), "{session}: {list}");
        assert!(list.contains("0 unread messages"), "{session}: {list}");
        for other in ["alice", "bob"] {
            let page = shown(s, session, TALK, &with(other));
            for private in ["Between us two", "Just so"] {
                assert!(!page.contains(private), "{session}: {page}");
            }
            assert!(page.contains("No messages yet."), "{session}: {page}");
        }
        if session == "s-carol" {
            assert_eq!(count_on_home(s, session), "0");
        }
        // By `/pw-read`: her open conversation with bob read again, for a
        // key the browser asks, is her own.
        shown(s, session, TALK, &with("bob"));
        let doc = latest(&s.pending.lock().expect("pending"), session);
        let key = BTreeMap::from([("earlier".to_string(), serde_json::json!(100))]);
        let read = s
            .read_keyed(session, "talk", 1, doc.1, &key, &|| false)
            .expect("read");
        assert!(matches!(read, KeyOutcome::Applied), "{session}");
        let sent = frames(s, &doc);
        assert!(!sent.contains("Between us two"), "{session}: {sent}");
        // And alice's open conversation with bob, asked for by its number
        // from this session, is no page of this session's.
        assert!(shown(s, "s-alice-1", TALK, &with("bob")).contains("Between us two"));
        let alices = latest(&s.pending.lock().expect("pending"), "s-alice-1");
        let theirs = s.read_keyed(session, "talk", 2, alices.1, &key, &|| false);
        assert!(
            matches!(theirs, Ok(KeyOutcome::Superseded) | Err(_)),
            "{session}: {:?}",
            theirs.map(|_| ())
        );
        let sent = frames(s, &doc);
        assert!(!sent.contains("Between us two"), "{session}: {sent}");
    }
    // By cache: nothing of anyone's messages is kept for every reader.
    let public = format!("{:?}", s.queries.public_cache_contents());
    for private in [
        "feed.app.Conversation",
        "feed.app.Conversations",
        "feed.app.UnreadMessages",
    ] {
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
        if ![
            "feed.app.Conversation",
            "feed.app.Conversations",
            "feed.app.UnreadMessages",
        ]
        .contains(&key.resource.as_str())
        {
            continue;
        }
        let mut parts = key.key.split('\u{1f}');
        let partition = parts.next().expect("private partition field");
        // ADR-0389: a private field names the cookie AND its actor epoch.
        // Match exactly one fixture reader; do not mistake the versioned
        // partition for a raw cookie or accept a shared/unknown partition.
        let sessions = ["s-alice-1", "s-alice-2", "s-bob", "s-carol", "s-nobody"];
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
fn a_third_user_signed_in_and_not_sees_none_of_it() {
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
fn on_postgres_a_message_is_shown_on_both_sides_and_counted_for_its_recipient() {
    let Some(s) = on_postgres("dm_both") else {
        return;
    };
    shown_on_both_sides(&s);
    assert_eq!(s.db.count("SELECT count(*) FROM messages"), 2);
    // Each read mark written in the transaction of the message that set it:
    // sending marks the sender's side read.
    assert_eq!(
        s.db.count(
            "SELECT count(*) FROM message_reads r \
             WHERE NOT EXISTS (SELECT 1 FROM messages m \
                               WHERE m.committed_in = r.committed_in AND m.sender = r.reader)"
        ),
        0
    );
    // A message writes no notification: the follow's is the one.
    assert_eq!(s.db.count("SELECT count(*) FROM notifications"), 1);
}

#[test]
fn on_postgres_who_may_message_whom_is_xs_rule() {
    let Some(s) = on_postgres("dm_may") else {
        return;
    };
    who_may_message_whom(&s);
    assert_eq!(
        s.db.count("SELECT count(*) FROM messages WHERE sender = recipient"),
        0
    );
    assert_eq!(s.db.count("SELECT count(*) FROM messages"), 3);
}

#[test]
fn on_postgres_reading_a_conversation_reaches_each_of_the_readers_sessions() {
    let Some(s) = on_postgres("dm_read") else {
        return;
    };
    read_everywhere(&s);
}

#[test]
fn on_postgres_a_message_reaches_both_users_sessions_and_no_third_users() {
    let Some(s) = on_postgres("dm_reach") else {
        return;
    };
    reaches_both_and_no_third(&s);
}

#[test]
fn on_postgres_a_follow_opens_the_composer_of_the_open_conversation() {
    let Some(s) = on_postgres("dm_follow") else {
        return;
    };
    opened_by_a_follow(&s);
}

#[test]
fn on_postgres_a_message_drops_both_users_entries_alone() {
    let Some(s) = on_postgres("dm_drops") else {
        return;
    };
    drops_at_both(&s);
}

#[test]
fn on_postgres_a_third_user_signed_in_and_not_sees_none_of_it() {
    let Some(s) = on_postgres("dm_private") else {
        return;
    };
    none_of_it_elsewhere(&s);
}
