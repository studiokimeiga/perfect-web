//! ADR0389: current session authority governs each private delivery.
use super::*;

fn logout(s: &Server, session: &str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener setup");
    let at = listener.local_addr().expect("listener address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (socket, _) = listener.accept().expect("accepted setup connection");
            handle(s, socket);
        });
        let mut client = TcpStream::connect(at).expect("connected setup client");
        write!(client, "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n").expect("logout sent");
        let mut response = String::new();
        client.read_to_string(&mut response).expect("logout read");
        response
    })
}

fn private_feed() -> (Served, u64) {
    let s = served_feed();
    if s.identity.principals().kept().is_some() {
        panic!("this selected gate requires explicit memory session authority");
    }
    for (session, user) in [("a", "alice"), ("b", "bob")] {
        s.identity.signed_in_for_test(session, user, user);
        let posted = s
            .command_answered(
                "feed.app.post",
                session,
                &[Val::String(format!("{user} fixture"))],
                Some("profile-fixture"),
            )
            .expect("post fixture ran");
        assert!(posted.committed, "fixture post commits");
    }
    let followed = s
        .command_answered(
            "feed.app.follow",
            "b",
            &[Val::String("alice".into())],
            Some("follow-fixture"),
        )
        .expect("follow ran");
    assert!(followed.committed, "fixture follow commits");
    let page = get_as(&s, "b", "/messages/alice");
    assert!(
        page.starts_with("HTTP/1.1 200"),
        "private page served: {page}"
    );
    let manifest = manifest_of(page.split_once("\r\n\r\n").expect("HTTP body").1);
    let document = manifest["cursor"].as_u64().expect("served document cursor");
    (s, document)
}

fn send_private(s: &Server, marker: &str, interaction: &str) {
    let sent = s
        .command_answered(
            "feed.app.send",
            "a",
            &[Val::String("bob".into()), Val::String(marker.into())],
            Some(interaction),
        )
        .expect("actual compiled private send ran");
    assert!(sent.committed, "private send commits");
    s.tell_waiting();
}

#[test]
fn a_logged_out_document_receives_no_queued_private_frames() {
    let (s, document) = private_feed();
    let poll = format!("/stream?doc={document}&since={document}");
    send_private(&s, "CURRENT_READER_PRIVATE_NEIGHBOR", "neighbor");
    assert!(
        get_as(&s, "b", &poll).contains("CURRENT_READER_PRIVATE_NEIGHBOR"),
        "a current reader receives its actual private update"
    );
    send_private(
        &s,
        "QUEUED_BEFORE_LOGOUT_MUST_NOT_DELIVER",
        "private-before-logout",
    );
    let signed_out = logout(&s, "b");
    assert!(
        signed_out.starts_with("HTTP/1.1 303"),
        "logout completed: {signed_out}"
    );
    assert!(
        s.identity.principals().of("b").is_none(),
        "old cookie is no principal"
    );
    for path in [poll.clone(), format!("{poll}&mode=stream")] {
        let response = get_as(&s, "b", &path);
        assert!(
            !response.contains("QUEUED_BEFORE_LOGOUT_MUST_NOT_DELIVER"),
            "an old document cannot deliver its private queue after logout: {response}"
        );
    }
}
