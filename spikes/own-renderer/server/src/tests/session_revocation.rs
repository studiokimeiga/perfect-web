//! ADR0389: current session authority governs each private delivery.
use super::*;
use std::time::Duration;

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

fn logged_out_document(mode: &str) {
    let (s, document) = private_feed();
    let route = format!("/stream?doc={document}&since={document}{mode}");
    send_private(&s, "CURRENT_READER_PRIVATE_NEIGHBOR", "neighbor");
    assert!(
        get_as(&s, "b", &route).contains("CURRENT_READER_PRIVATE_NEIGHBOR"),
        "a current reader receives its actual private update on this transport"
    );
    send_private(
        &s,
        "QUEUED_BEFORE_LOGOUT_MUST_NOT_DELIVER",
        "private-before-logout",
    );
    let queued = {
        let pending = s.pending.lock().expect("pending");
        serde_json::to_string(&pending[&("b".to_string(), document)].frames)
            .expect("the actual queued frames serialize")
    };
    assert!(
        queued.contains("QUEUED_BEFORE_LOGOUT_MUST_NOT_DELIVER"),
        "the sensitive batch actually exists before revocation"
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
    let response = get_as(&s, "b", &route);
    assert!(
        !response.contains("QUEUED_BEFORE_LOGOUT_MUST_NOT_DELIVER"),
        "this transport cannot deliver its own queued private batch after logout: {response}"
    );
}

#[test]
fn a_logged_out_document_poll_receives_no_queued_private_frames() {
    logged_out_document("");
}

#[test]
fn a_logged_out_document_ndjson_receives_no_queued_private_frames() {
    logged_out_document("&mode=stream");
}

fn queued_then(s: &Server, document: u64, marker: &str) {
    let poll = format!("/stream?doc={document}&since={document}");
    let mut leaks = Vec::new();
    for path in [poll.clone(), format!("{poll}&mode=stream")] {
        let response = get_as(s, "b", &path);
        if response.contains(marker) {
            leaks.push(path);
        }
    }
    assert!(
        leaks.is_empty(),
        "revoked authority delivered a private batch over {leaks:?}"
    );
}

#[test]
fn idle_expiry_refuses_the_previously_queued_private_batch() {
    let (s, document) = private_feed();
    send_private(&s, "PRIVATE_IDLE_EXPIRED", "idle-private");
    let principals = s.identity.principals();
    let actor = principals.of("b").expect("live fixture principal");
    principals.expire_after(identity::SessionLifetimes {
        idle: Duration::from_secs(1),
        absolute: Duration::from_secs(10),
    });
    principals.held_for_test("b", actor, Duration::from_secs(2), Duration::ZERO);
    queued_then(&s, document, "PRIVATE_IDLE_EXPIRED");
}

#[test]
fn absolute_expiry_refuses_the_previously_queued_private_batch() {
    let (s, document) = private_feed();
    send_private(&s, "PRIVATE_ABSOLUTE_EXPIRED", "absolute-private");
    let principals = s.identity.principals();
    let actor = principals.of("b").expect("live fixture principal");
    principals.expire_after(identity::SessionLifetimes {
        idle: Duration::from_secs(10),
        absolute: Duration::from_secs(1),
    });
    principals.held_for_test("b", actor, Duration::from_secs(2), Duration::from_secs(2));
    queued_then(&s, document, "PRIVATE_ABSOLUTE_EXPIRED");
}

#[test]
fn reopening_the_same_cookie_and_actor_cannot_rebind_its_old_document() {
    let (s, document) = private_feed();
    send_private(&s, "PRIVATE_OLD_GENERATION", "old-generation-private");
    let before = s.identity.principals().bound_reader("b");
    s.identity.signed_in_for_test("b", "bob", "bob");
    assert_ne!(
        before,
        s.identity.principals().bound_reader("b"),
        "every open rotates the epoch"
    );
    queued_then(&s, document, "PRIVATE_OLD_GENERATION");
    let current = get_as(&s, "b", "/messages/alice");
    assert!(
        current.contains("PRIVATE_OLD_GENERATION"),
        "the same actor may read a fresh authorized document"
    );
}

#[test]
fn another_session_of_the_same_user_keeps_its_own_private_delivery() {
    let (s, _) = private_feed();
    s.identity.signed_in_for_test("b2", "bob", "bob");
    let page = get_as(&s, "b2", "/messages/alice");
    let manifest = manifest_of(page.split_once("\r\n\r\n").expect("HTTP body").1);
    let document = manifest["cursor"].as_u64().expect("served cursor");
    assert!(logout(&s, "b").starts_with("HTTP/1.1 303"));
    send_private(
        &s,
        "SECOND_SESSION_STILL_AUTHORIZED",
        "other-session-private",
    );
    let response = get_as(
        &s,
        "b2",
        &format!("/stream?doc={document}&since={document}"),
    );
    assert!(
        response.contains("SECOND_SESSION_STILL_AUTHORIZED"),
        "revocation is scoped to one session"
    );
}

#[test]
fn an_explicit_anonymous_document_still_receives_its_public_feed() {
    let (s, _) = private_feed();
    let page = get_as(&s, "anonymous", "/");
    assert!(page.starts_with("HTTP/1.1 200"), "anonymous home is served");
    let manifest = manifest_of(page.split_once("\r\n\r\n").expect("HTTP body").1);
    let document = manifest["cursor"].as_u64().expect("served cursor");
    let posted = s
        .command_answered(
            "feed.app.post",
            "a",
            &[Val::String("PUBLIC_ANONYMOUS_NEIGHBOR".into())],
            Some("public-neighbor"),
        )
        .expect("public post ran");
    assert!(posted.committed);
    s.tell_waiting();
    let response = get_as(
        &s,
        "anonymous",
        &format!("/stream?doc={document}&since={document}"),
    );
    assert!(
        response.contains("PUBLIC_ANONYMOUS_NEIGHBOR"),
        "anonymous is an explicit public reader"
    );
}

#[test]
fn internal_reads_and_inspections_do_not_renew_client_activity() {
    let (s, _) = private_feed();
    let principals = s.identity.principals();
    let actor = principals.of("b").expect("live fixture principal");
    principals.held_for_test("b", actor, Duration::from_secs(2), Duration::ZERO);
    let before = principals.seen_for_test("b").expect("held session");
    let binding = principals.bound_reader("b");
    for _ in 0..8 {
        assert!(principals.of("b").is_some());
        assert_eq!(principals.inspect_reader("b").expect("inspection"), binding);
    }
    assert_eq!(
        principals.seen_for_test("b"),
        Some(before),
        "background use is not client activity"
    );
    principals.refresh("b");
    assert!(
        principals.seen_for_test("b").expect("held session") > before,
        "an actual admitted client request records activity"
    );
    assert_eq!(
        principals.bound_reader("b"),
        binding,
        "activity retains the same actor generation"
    );
}

#[test]
fn a_private_initial_document_cannot_finish_after_its_session_is_revoked() {
    let (s, _) = private_feed();
    let marker = "PRIVATE_INITIAL_DOCUMENT_AFTER_LOGOUT";
    send_private(&s, marker, "private-before-render");
    assert!(
        get_as(&s, "b", "/messages/alice").contains(marker),
        "the current principal has real private source data"
    );
    s.queries.expire("feed.app.Conversation");
    let (entered, read_started) = std::sync::mpsc::channel();
    let (release, continue_read) = std::sync::mpsc::channel();
    *s.in_flight.lock().expect("read hook") = Some(Box::new(move |_| {
        entered.send(()).expect("read barrier receiver");
        continue_read
            .recv_timeout(Duration::from_secs(20))
            .expect("read barrier released");
    }));
    let response = std::thread::scope(|scope| {
        let request = scope.spawn(|| get_as(&s, "b", "/messages/alice"));
        read_started
            .recv_timeout(Duration::from_secs(20))
            .expect("actual private fetch reached its post-read barrier");
        let signed_out = logout(&s, "b");
        release.send(()).expect("release old read");
        assert!(
            signed_out.starts_with("HTTP/1.1 303"),
            "logout completes before old bytes are handed over"
        );
        request.join().expect("HTTP read completed normally")
    });
    assert!(
        !response.contains(marker),
        "a revoked in-flight render cannot hand over its private HTML: {response}"
    );
}

#[test]
fn a_new_actor_generation_cannot_reuse_the_old_private_session_query_cache() {
    // Session queries are compiled with zero staleness (PW0102); this is a
    // valid neighbor, not proof that cross-generation in-flight joins are safe.
    let (s, _) = private_feed();
    let before = get_as(&s, "b", "/");
    assert!(
        before.contains("Signed in as <!--pw:") && before.contains("bob"),
        "the old actor's private Me value is rendered and cached"
    );
    s.identity
        .signed_in_for_test("b", "carol", "CAROL_CURRENT_ACTOR");
    let after = get_as(&s, "b", "/");
    assert!(
        after.contains("CAROL_CURRENT_ACTOR"),
        "a new generation cannot reuse another actor's cached private Me value: {after}"
    );
}

fn pause_next_viewer_read(
    s: &Server,
    before: u64,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) {
    *s.in_flight.lock().expect("read hook") = Some(Box::new(move |server| {
        let now = server
            .calls
            .lock()
            .expect("calls")
            .get("feed:data/users#viewer")
            .copied()
            .unwrap_or(0);
        if now > before {
            entered.send(()).expect("viewer barrier receiver");
            release
                .recv_timeout(Duration::from_secs(20))
                .expect("viewer barrier released");
        } else {
            pause_next_viewer_read(server, before, entered, release);
        }
    }));
}

#[test]
fn a_new_actor_cannot_join_the_old_generations_private_read_in_flight() {
    let (s, _) = private_feed();
    let warm = get_as(&s, "b", "/");
    assert!(warm.contains("bob"), "the actual old reader has a profile");
    let before = s
        .calls
        .lock()
        .expect("calls")
        .get("feed:data/users#viewer")
        .copied()
        .unwrap_or(0);
    let (entered, read_started) = std::sync::mpsc::channel();
    let (release, continue_read) = std::sync::mpsc::channel();
    pause_next_viewer_read(&s, before, entered, continue_read);
    let (after, observed_join) = std::thread::scope(|scope| {
        let old = scope.spawn(|| get_as(&s, "b", "/"));
        read_started
            .recv_timeout(Duration::from_secs(20))
            .expect("actual old Me value read");
        let old_key = s
            .queries
            .trace()
            .iter()
            .rev()
            .find_map(|trace| match trace {
                pw_resource::Trace::RequestStarted { key, .. } if key.resource == "feed.app.Me" => {
                    Some(key.clone())
                }
                _ => None,
            })
            .expect("actual old private Me flight");
        s.identity
            .signed_in_for_test("b", "carol", "CAROL_CURRENT_ACTOR");
        let trace_start = s.queries.trace().len();
        let (finished, completion) = std::sync::mpsc::channel();
        let current_server = &s;
        let current = scope.spawn(move || {
            let response = get_as(current_server, "b", "/");
            finished.send(()).expect("new request observer");
            response
        });
        let until = std::time::Instant::now() + Duration::from_secs(1);
        let mut observed = false;
        let mut joined = false;
        while std::time::Instant::now() < until {
            joined = s.queries.trace()[trace_start..].iter().any(|trace|
                matches!(trace, pw_resource::Trace::RequestDeduplicated { key } if key == &old_key));
            if joined || completion.try_recv().is_ok() {
                observed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // Release every owned blocked reader before any domain/setup assert.
        release.send(()).expect("release old private flight");
        let current = current
            .join()
            .expect("current HTTP worker completed normally");
        old.join().expect("old HTTP worker completed normally");
        assert!(
            observed,
            "setup must observe a real join or completed independent read before its budget"
        );
        (current, joined)
    });
    assert!(
        !observed_join && after.contains("CAROL_CURRENT_ACTOR"),
        "a current actor cannot borrow the old actor's private in-flight value (joined={observed_join}): {after}"
    );
}

#[test]
fn generation_partitions_keep_wildcard_invalidation_and_session_cleanup_exact() {
    let policy = serde_json::json!({"cache":"private","privacy":"session","key":[0,1]});
    let args = [Val::String("shared-item".into()), Val::S64(7)];
    let who = "cookie:with#delimiters\u{1f}and-fields";
    let mut keys = Vec::new();
    for epoch in [1u8, 2] {
        let reader = identity::ReaderIdentity::Account(identity::SessionIdentity {
            principal: identity::Principal::guest("synthetic-actor"),
            epoch: [epoch; 16],
        });
        let key = entry_key(&reader.partition(who), &policy, &args).expect("complete key");
        assert!(
            names_entry(&policy, &[Some(args[0].clone()), None], &key),
            "one private field keeps wildcard invalidation across every epoch"
        );
        assert!(!names_entry(
            &policy,
            &[Some(Val::String("other-item".into())), None],
            &key
        ));
        assert!(
            private_entry_belongs_to(&key, who),
            "the cookie owns all its epochs"
        );
        assert!(
            !private_entry_belongs_to(&key, "another-cookie"),
            "other sessions are preserved"
        );
        let impersonated_legacy_cookie = key
            .split('\u{1f}')
            .next()
            .expect("partition")
            .strip_prefix("session=")
            .expect("private field");
        assert!(
            !private_entry_belongs_to(&key, impersonated_legacy_cookie),
            "a raw cookie cannot impersonate a structured generation field"
        );
        keys.push(key);
    }
    assert_ne!(
        keys[0], keys[1],
        "new generations cannot join the same private flight"
    );
}

#[test]
fn an_anonymous_render_never_borrows_a_transient_authenticated_actor() {
    let (s, _) = private_feed();
    let before = s
        .calls
        .lock()
        .expect("calls")
        .get("feed:data/users#viewer")
        .copied()
        .unwrap_or(0);
    let stage = Arc::new(std::sync::atomic::AtomicU8::new(0));
    let observed = stage.clone();
    *s.while_reading.lock().expect("read observer") = Some(Arc::new(move |server| {
        use std::sync::atomic::Ordering;
        if observed
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            server
                .identity
                .signed_in_for_test("anonymous-cycle", "bob", "TRANSIENT_PRIVATE_ACTOR");
        } else if observed.load(Ordering::SeqCst) == 1 {
            let now = server
                .calls
                .lock()
                .expect("calls")
                .get("feed:data/users#viewer")
                .copied()
                .unwrap_or(0);
            if now > before {
                let signed_out = logout(server, "anonymous-cycle");
                assert!(
                    signed_out.starts_with("HTTP/1.1 303"),
                    "the intermediate actor actually logs out"
                );
                observed.store(2, Ordering::SeqCst);
            }
        }
    }));
    let response = get_as(&s, "anonymous-cycle", "/");
    *s.while_reading.lock().expect("read observer") = None;
    assert_eq!(
        stage.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the real render must cross anonymous/authenticated/anonymous source states"
    );
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "the valid anonymous page must still be served after the account cycle"
    );
    let manifest = manifest_of(response.split_once("\r\n\r\n").expect("HTTP body").1);
    assert_eq!(
        manifest["holds"]["SignedIn~holds"], false,
        "the successful response is still an anonymous document"
    );
    assert!(
        !response.contains("TRANSIENT_PRIVATE_ACTOR"),
        "an anonymous-labeled response cannot borrow an intermediate actor's private value: {response}"
    );
}

fn streamed_viewer_as(s: &Server, reader: &identity::ReaderIdentity) -> Value {
    let principals = s.identity.principals();
    let binding = s.plan_of("feed.app.Home~ui")["bindings"]
        .as_array()
        .expect("bindings")
        .iter()
        .find(|b| b["resource"] == "feed.app.Me")
        .expect("compiled Me binding")
        .clone();
    let result = principals.with_reader("b", reader, || {
        std::thread::scope(|scope| {
            let mut settling = Settling::start(
                scope,
                s,
                "b",
                vec![StreamRun {
                    part: 1,
                    stream: binding,
                    args: vec![Val::String("b".into())],
                    streamed: false,
                }],
            );
            let mut settled = settling.for_document();
            assert_eq!(
                settled.len(),
                1,
                "the real worker completes its compiled query"
            );
            let (_, outcome) = settled.pop().expect("settled worker");
            match outcome {
                Settled::Ready(value) => value,
                Settled::Failed(why) => panic!("worker query failed, no actor evidence: {why:?}"),
            }
        })
    });
    let key = s
        .queries
        .trace()
        .iter()
        .rev()
        .find_map(|trace| match trace {
            pw_resource::Trace::RequestStarted { key, .. } if key.resource == "feed.app.Me" => {
                Some(key.clone())
            }
            _ => None,
        })
        .expect("actual private worker flight");
    assert!(
        key.key
            .starts_with(&format!("session={}\u{1f}", reader.partition("b"))),
        "worker query partition must follow the same frozen actor as its host calls: {key:?}"
    );
    result
}

#[test]
fn a_stream_worker_keeps_its_render_actor_across_a_concurrent_account_change() {
    let (s, _) = private_feed();
    let principals = s.identity.principals();
    let bob = principals.bound_reader("b");
    s.identity
        .signed_in_for_test("b", "carol", "REPLACEMENT_ACTOR");
    let value = streamed_viewer_as(&s, &bob);
    let Value::Record(fields) = &value else {
        panic!("viewer is a record")
    };
    assert_eq!(
        fields.get("name"),
        Some(&Value::Text("bob".into())),
        "a child worker must execute as the render's original actor"
    );
    assert!(
        !s.delivery_allowed(&("b".into(), 0), Some(&bob)),
        "frozen computation does not authorize delivery after account rotation"
    );
    assert_eq!(
        principals.of("b").expect("replacement live").name,
        "REPLACEMENT_ACTOR"
    );
}

#[test]
fn an_anonymous_stream_worker_cannot_borrow_the_live_account() {
    let (s, _) = private_feed();
    let value = streamed_viewer_as(&s, &identity::ReaderIdentity::Anonymous);
    let Value::Record(fields) = &value else {
        panic!("viewer is a record")
    };
    assert_eq!(
        fields.get("signed_in"),
        Some(&Value::Bool(false)),
        "a worker's explicitly anonymous actor cannot become the cache's current account"
    );
    assert!(!s.delivery_allowed(&("b".into(), 0), Some(&identity::ReaderIdentity::Anonymous)));
}

pub(super) fn private_feed_for_commands() -> (Served, u64) {
    private_feed()
}
