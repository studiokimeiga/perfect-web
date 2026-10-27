//! ADR-0389 browser invocation and receipt authority, using real compiled commands.
use super::*;

fn fixture() -> Served {
    let s = served_feed();
    assert!(
        s.identity.principals().kept().is_none(),
        "this witness selects memory explicitly"
    );
    s.identity.signed_in_for_test("owner", "bob", "Bob");
    s.identity.signed_in_for_test("other", "carol", "Carol");
    s
}

fn post(s: &Server, session: &str, document: Option<u64>, id: &str, text: &str) -> String {
    let body = serde_json::json!([text]).to_string();
    let page = document
        .map(|n| format!("pw-document: {n}\r\n"))
        .unwrap_or_default();
    super::sign_in::exchanged(
        s,
        &format!(
            "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\nSec-Fetch-Site: same-origin\r\npw-interaction: {id}\r\n{page}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn home(s: &Server, session: &str) -> u64 {
    let page = get_as(s, session, "/");
    assert!(page.starts_with("HTTP/1.1 200"), "valid page fixture");
    manifest_of(page.split_once("\r\n\r\n").expect("HTTP body").1)["cursor"]
        .as_u64()
        .expect("document")
}

#[test]
fn another_cookie_cannot_recall_a_page_owners_committed_receipt() {
    let s = fixture();
    let document = home(&s, "owner");
    let first = post(
        &s,
        "owner",
        Some(document),
        "private-receipt",
        "receipt fixture",
    );
    assert!(
        first.starts_with("HTTP/1.1 202") && first.contains("\"committed\":true"),
        "first commits"
    );
    let neighbor = post(
        &s,
        "owner",
        Some(document),
        "private-receipt",
        "receipt fixture",
    );
    assert_eq!(
        first, neighbor,
        "the same authorized reader still recalls its receipt"
    );
    let stolen = post(
        &s,
        "other",
        Some(document),
        "private-receipt",
        "receipt fixture",
    );
    assert!(
        stolen.starts_with("HTTP/1.1 403")
            && !stolen.contains("\"committed\":true")
            && !stolen.contains("\"result\""),
        "a document number cannot grant another cookie its owners receipt: {stolen}"
    );
}

#[test]
fn a_logged_out_cookie_cannot_recall_an_old_page_receipt() {
    let s = fixture();
    let document = home(&s, "owner");
    let first = post(
        &s,
        "owner",
        Some(document),
        "before-logout",
        "before logout",
    );
    assert!(first.contains("\"committed\":true"), "first commits");
    let out = super::sign_in::exchanged(
        &s,
        "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 303"), "logout completes");
    let replay = post(
        &s,
        "owner",
        Some(document),
        "before-logout",
        "before logout",
    );
    assert!(
        replay.starts_with("HTTP/1.1 403")
            && !replay.contains("\"committed\":true")
            && !replay.contains("\"result\""),
        "a revoked cookie cannot deliver its old receipt: {replay}"
    );
}

#[test]
fn a_reopened_cookie_cannot_borrow_or_reexecute_an_old_generations_receipt() {
    let s = fixture();
    let args = [serde_json::json!("old actor operation")];
    let first = s
        .command_answer("feed.app.post", "owner", &args, Some("same-id"))
        .expect("first ran");
    assert!(first.committed, "first commits");
    s.identity
        .signed_in_for_test("owner", "carol", "Replacement actor");
    let replay = s.command_answer("feed.app.post", "owner", &args, Some("same-id"));
    assert!(
        replay.is_err() || replay.is_ok_and(|a| !a.committed),
        "new generation must neither return old success nor silently run it anew"
    );
    assert_eq!(
        post_count(&s, "old actor operation"),
        1,
        "old generation retry cannot duplicate a durable write"
    );
    let fresh = s
        .command_answer("feed.app.post", "owner", &args, Some("fresh-id"))
        .expect("new id admitted");
    assert!(
        fresh.committed,
        "the new actor has a legitimate new-command neighbor"
    );
}

fn post_count(s: &Server, text: &str) -> usize {
    let value = s
        .query(
            "feed.app.Timeline",
            "other",
            &[Val::String("other".into()), Val::S64(100)],
        )
        .expect("actual timeline query");
    val_to_json(&value)
        .as_array()
        .expect("timeline list")
        .iter()
        .filter(|row| row["text"] == text)
        .count()
}

fn logout(s: &Server, session: &str) -> String {
    super::sign_in::exchanged(
        s,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n"
        ),
    )
}

#[test]
fn memory_revocation_before_fence_refuses_the_captured_actor_and_all_writes() {
    let s = fixture();
    let (entered, reached) = std::sync::mpsc::channel();
    let (release, go) = std::sync::mpsc::channel();
    *s.before_command_authority.lock().expect("hook") = Some(Box::new(move |_| {
        entered.send(()).expect("observer");
        go.recv_timeout(std::time::Duration::from_secs(20))
            .expect("released");
    }));
    let result = std::thread::scope(|scope| {
        let pending = scope.spawn(|| {
            s.command_answer(
                "feed.app.post",
                "owner",
                &[serde_json::json!("REVOKED_BEFORE_FENCE")],
                Some("revoked-id"),
            )
        });
        reached
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("actual captured actor reached fence seam");
        let out = logout(&s, "owner");
        release.send(()).expect("released");
        let result = pending.join().expect("command returns normally");
        assert!(
            out.starts_with("HTTP/1.1 303"),
            "logout won before acquisition"
        );
        result
    });
    assert!(
        result.is_err() || result.is_ok_and(|a| !a.committed),
        "old actor cannot commit after revocation"
    );
    assert_eq!(
        post_count(&s, "REVOKED_BEFORE_FENCE"),
        0,
        "no business row published"
    );
    assert!(
        s.recalled_interaction("owner\u{1f}feed.app.post\u{1f}revoked-id")
            .expect("receipt lookup")
            .is_none(),
        "no receipt committed"
    );
}

#[test]
fn memory_expiry_during_evaluation_rolls_back_rows_events_and_receipt() {
    let s = fixture();
    let events_before = s.materializer.retained_events();
    let schedules_before = s.materializer.scheduled_runs().len();
    *s.before_commit.lock().expect("hook") = Some(Box::new(|server| {
        server
            .identity
            .principals()
            .expire_after(identity::SessionLifetimes {
                idle: std::time::Duration::ZERO,
                absolute: std::time::Duration::ZERO,
            });
    }));
    let result = s.command_answer(
        "feed.app.post",
        "owner",
        &[serde_json::json!("EXPIRED_DURING_EVALUATION")],
        Some("expired-id"),
    );
    assert!(
        result.is_err() || result.is_ok_and(|a| !a.committed),
        "expired authority refuses before publication"
    );
    assert_eq!(
        post_count(&s, "EXPIRED_DURING_EVALUATION"),
        0,
        "business rows rolled back"
    );
    assert!(
        s.recalled_interaction("owner\u{1f}feed.app.post\u{1f}expired-id")
            .expect("receipt lookup")
            .is_none(),
        "receipt rolled back too"
    );
    assert_eq!(
        s.materializer.retained_events(),
        events_before,
        "outbox rolled back"
    );
    assert_eq!(
        s.materializer.scheduled_runs().len(),
        schedules_before,
        "no schedules escaped rollback"
    );
}

#[test]
fn an_unbound_legacy_receipt_refuses_without_returning_or_reexecuting_it() {
    let s = fixture();
    let args = [serde_json::json!("LEGACY_COMMITTED_ONCE")];
    let first = s
        .command_answer("feed.app.post", "owner", &args, Some("legacy-id"))
        .expect("first actual operation");
    assert!(first.committed, "real business operation committed");
    let key = "owner\u{1f}feed.app.post\u{1f}legacy-id";
    let (_, answer) = s
        .recalled_interaction(key)
        .expect("lookup")
        .expect("real receipt");
    let old_sent = serde_json::Value::Array(args.to_vec()).to_string();
    let now = u64::try_from(s.now()).expect("clock");
    s.materializer
        .command(|tx| {
            Materializer::prune_interactions(tx, u64::MAX);
            Materializer::record_interaction(tx, key, &old_sent, &answer, now)?;
            Ok::<_, String>(Vec::new())
        })
        .expect("owned fixture models an old unbound receipt");
    let replay = s.command_answer("feed.app.post", "owner", &args, Some("legacy-id"));
    assert!(
        replay.is_err() || replay.is_ok_and(|a| !a.committed),
        "legacy answer must not be adopted into a current generation"
    );
    assert_eq!(
        post_count(&s, "LEGACY_COMMITTED_ONCE"),
        1,
        "upgrade refusal cannot silently duplicate the old operation"
    );
}

#[test]
fn logout_after_commit_refuses_private_delivery_without_claiming_rollback() {
    let s = fixture();
    *s.before_command_delivery.lock().expect("hook") = Some(Box::new(|server| {
        let out = logout(server, "owner");
        assert!(
            out.starts_with("HTTP/1.1 303"),
            "logout completes after the fixed response exists"
        );
    }));
    let response = post(
        &s,
        "owner",
        None,
        "committed-before-logout",
        "COMMITTED_BEFORE_LOGOUT",
    );
    assert_eq!(
        post_count(&s, "COMMITTED_BEFORE_LOGOUT"),
        1,
        "the transaction really committed before revocation"
    );
    assert!(
        s.recalled_interaction("owner\u{1f}feed.app.post\u{1f}committed-before-logout")
            .expect("lookup")
            .is_some(),
        "the authoritative receipt is preserved"
    );
    assert!(
        response.starts_with("HTTP/1.1 403")
            && response.contains("\"outcome\":\"unknown\"")
            && !response.contains("\"committed\":false")
            && !response.contains("\"result\"")
            && !response.contains("\"basis\""),
        "delivery refusal cannot expose private data or pretend this commit rolled back: {response}"
    );
}

#[test]
fn an_old_document_cannot_authorize_a_fresh_interaction_after_same_cookie_reopen() {
    let s = fixture();
    let document = home(&s, "owner");
    s.identity
        .signed_in_for_test("owner", "carol", "New account");
    let refused = post(
        &s,
        "owner",
        Some(document),
        "new-interaction",
        "STALE_PAGE_MUST_NOT_WRITE",
    );
    assert!(
        refused.starts_with("HTTP/1.1 403"),
        "fresh ids cannot escape document reader binding: {refused}"
    );
    assert_eq!(post_count(&s, "STALE_PAGE_MUST_NOT_WRITE"), 0);
    let current = home(&s, "owner");
    let allowed = post(
        &s,
        "owner",
        Some(current),
        "new-interaction",
        "STALE_PAGE_MUST_NOT_WRITE",
    );
    assert!(
        allowed.contains("\"committed\":true"),
        "a current document can make the new actor's fresh command"
    );
    assert_eq!(post_count(&s, "STALE_PAGE_MUST_NOT_WRITE"), 1);
}

#[test]
fn one_render_does_not_merge_principal_operations_between_actor_generations() {
    let s = fixture();
    let principals = s.identity.principals();
    let bob = principals.bound_reader("owner");
    let render = crate::render_merge::begin(&*s.data, s.queries.epoch());
    let first = principals
        .with_reader("owner", &bob, || {
            s.query("feed.app.Me", "owner", &[Val::String("owner".into())])
        })
        .expect("first actual compiled Me");
    s.identity
        .signed_in_for_test("owner", "carol", "New generation");
    let carol = principals.bound_reader("owner");
    let second = principals
        .with_reader("owner", &carol, || {
            s.query("feed.app.Me", "owner", &[Val::String("owner".into())])
        })
        .expect("second actual compiled Me");
    drop(render);
    assert_eq!(
        val_to_json(&first)["name"],
        "Bob",
        "first actor really read"
    );
    assert_eq!(
        val_to_json(&second)["name"],
        "New generation",
        "per-render operation deduplication must include the immutable reader"
    );
}

#[test]
fn an_admitted_memory_command_holds_authority_until_publication() {
    let s = fixture();
    let (entered, reached) = std::sync::mpsc::channel();
    let (release, go) = std::sync::mpsc::channel();
    *s.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
        entered.send(()).expect("observer");
        go.recv_timeout(std::time::Duration::from_secs(20))
            .expect("release");
    }));
    let result = std::thread::scope(|scope| {
        let command = scope.spawn(|| {
            s.command_answer(
                "feed.app.post",
                "owner",
                &[serde_json::json!("MEMORY_ADMITTED_FIRST")],
                Some("admitted-first"),
            )
        });
        reached
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("actual evaluated command reached commit");
        let gate = s.identity.principals().command_gate("owner");
        let held = gate.try_write().is_err();
        let (closed, close_done) = std::sync::mpsc::channel();
        let server = &s;
        let closing = scope.spawn(move || {
            let out = logout(server, "owner");
            closed.send(out).expect("logout observer");
        });
        // A guard-removal control is released only after its winning logout.
        let early = if held {
            None
        } else {
            Some(
                close_done
                    .recv_timeout(std::time::Duration::from_secs(20))
                    .expect("unfenced logout completes"),
            )
        };
        release.send(()).expect("release command before assertions");
        let result = command.join().expect("command worker");
        closing.join().expect("logout worker");
        let out = early.unwrap_or_else(|| close_done.recv().expect("completed logout"));
        assert!(out.starts_with("HTTP/1.1 303"));
        result
    });
    let _ = result; // Delivery may legitimately lose the race after COMMIT.
    assert_eq!(
        post_count(&s, "MEMORY_ADMITTED_FIRST"),
        1,
        "authority acquired first stays held through memory publication"
    );
    assert!(
        s.recalled_interaction("owner\u{1f}feed.app.post\u{1f}admitted-first")
            .expect("lookup")
            .is_some(),
        "row and receipt commit together before logout"
    );
}

#[test]
fn automatic_delivery_polls_do_not_renew_client_idle_time() {
    let (s, document) = super::session_revocation::private_feed_for_commands();
    let principals = s.identity.principals();
    let actor = principals.of("b").expect("reader");
    principals.expire_after(identity::SessionLifetimes {
        idle: std::time::Duration::from_secs(50),
        absolute: std::time::Duration::from_secs(200),
    });
    principals.held_for_test(
        "b",
        actor,
        std::time::Duration::from_secs(100),
        std::time::Duration::from_secs(90),
    );
    let _poll = get_as(&s, "b", &format!("/stream?doc={document}&since={document}"));
    assert_eq!(
        principals.seen_for_test("b"),
        Some(std::time::Duration::from_secs(90)),
        "automatic transport maintenance cannot keep an idle user signed in forever"
    );
}

#[test]
fn an_initial_page_uses_its_admitted_reader_after_a_later_cache_installation() {
    let s = fixture();
    assert!(
        get_as(&s, "owner", "/").starts_with("HTTP/1.1 200"),
        "authorized neighboring page"
    );
    *s.after_page_admission.lock().expect("hook") = Some(Box::new(|server| {
        server
            .identity
            .signed_in_for_test("owner", "carol", "Later account");
    }));
    let page = get_as(&s, "owner", "/");
    assert!(
        page.starts_with("HTTP/1.1 403") || page.starts_with("HTTP/1.1 503"),
        "the old admission cannot borrow a later cached actor: {page}"
    );
    assert!(
        !page.contains("Later account"),
        "no unrelated actor's private page is delivered"
    );
}

#[test]
fn same_cookie_reopening_does_not_inherit_prior_interaction_acknowledgements_or_basis() {
    let s = fixture();
    let old = home(&s, "owner");
    assert!(
        s.command_answer(
            "feed.app.post",
            "owner",
            &[serde_json::json!("OLD_GENERATION_POST")],
            Some("old-generation-ack")
        )
        .expect("old command")
        .committed
    );
    let old_values = entry_values(&s, &("owner".into(), old));
    assert!(
        old_values
            .iter()
            .any(|(_, _, _, applied)| applied.contains(&"old-generation-ack".into())),
        "old generation's real frame acknowledges its press"
    );
    assert!(
        !s.committed_basis("owner")
            .as_array()
            .expect("basis")
            .is_empty(),
        "old generation has actual basis"
    );
    s.identity
        .signed_in_for_test("owner", "carol", "New generation");
    let current = home(&s, "owner");
    assert_eq!(
        s.committed_basis("owner"),
        serde_json::json!([]),
        "new reader cannot inherit an old generation's basis"
    );
    assert!(
        s.command_answer(
            "feed.app.post",
            "owner",
            &[serde_json::json!("NEW_GENERATION_POST")],
            Some("new-generation-ack")
        )
        .expect("new command")
        .committed
    );
    let values = entry_values(&s, &("owner".into(), current));
    assert!(
        values
            .iter()
            .any(|(_, _, _, applied)| applied.contains(&"new-generation-ack".into())),
        "valid new-generation acknowledgement"
    );
    assert!(
        values
            .iter()
            .all(|(_, _, _, applied)| !applied.contains(&"old-generation-ack".into())),
        "new values cannot acknowledge an old generation's interactions"
    );
}

#[test]
fn a_browser_order_helper_cannot_commit_as_a_revoked_admitted_reader() {
    let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
    s.identity.use_provider(
        Arc::new(super::sign_in::TestProvider::default()),
        super::sign_in::REDIRECT,
    );
    s.identity.signed_in_for_test("owner", "bob", "Bob");
    let old = s.identity.principals().bound_reader("owner");
    let request = |status| {
        super::sign_in::exchanged(
            &s,
            &format!(
                "POST /bench/order?status={status} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n"
            ),
        )
    };
    assert!(
        request("preparing").starts_with("HTTP/1.1 200"),
        "valid current-reader helper neighbor"
    );
    let before = s.order_of("owner").expect("actual order");
    *s.before_command_authority.lock().expect("hook") = Some(Box::new(|server| {
        let out = logout(server, "owner");
        assert!(
            out.starts_with("HTTP/1.1 303"),
            "logout completes before helper fence"
        );
    }));
    let response = request("delivered");
    assert!(
        !response.starts_with("HTTP/1.1 200"),
        "revoked helper is refused: {response}"
    );
    let after = s
        .identity
        .principals()
        .with_reader("owner", &old, || s.order_of("owner"))
        .expect("physical predecessor order still exists");
    assert_eq!(
        after, before,
        "revoked helper cannot mutate predecessor's order"
    );
}

#[test]
fn invalid_interaction_metadata_is_a_known_refusal_before_any_execution() {
    let s = fixture();
    let payload = serde_json::json!(["INVALID_INTERACTION_NEVER_WRITES"]).to_string();
    let too_long = "x".repeat(129);
    for id in [None, Some(""), Some("bad id"), Some(too_long.as_str())] {
        let header = id
            .map(|id| format!("pw-interaction: {id}\r\n"))
            .unwrap_or_default();
        let response = super::sign_in::exchanged(
            &s,
            &format!(
                "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\n{header}Content-Length: {}\r\n\r\n{payload}",
                payload.len()
            ),
        );
        assert!(
            response.starts_with("HTTP/1.1 400"),
            "invalid protocol is refused: {response}"
        );
        let body: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").expect("body").1).expect("JSON");
        assert_eq!(body["committed"], false, "no evaluation could start");
        assert!(
            body.get("outcome").is_none()
                && body.get("result").is_none()
                && body.get("basis").is_none(),
            "known protocol refusal is not a possibly committed result"
        );
        assert_eq!(
            post_count(&s, "INVALID_INTERACTION_NEVER_WRITES"),
            0,
            "no business writes"
        );
    }
    let allowed = post(
        &s,
        "owner",
        None,
        "valid-after-protocol-refusals",
        "INVALID_INTERACTION_NEVER_WRITES",
    );
    assert!(
        allowed.contains("\"committed\":true"),
        "valid current-reader neighbor executes"
    );
    assert_eq!(
        post_count(&s, "INVALID_INTERACTION_NEVER_WRITES"),
        1,
        "only the legitimate command committed"
    );
    assert!(
        s.recalled_interaction("owner\u{1f}feed.app.post\u{1f}valid-after-protocol-refusals")
            .expect("lookup")
            .is_some(),
        "legitimate receipt exists"
    );
}

#[test]
fn command_traces_disclose_only_bounded_outcome_metadata() {
    let mut answered = Answered {
        committed: false,
        result: Some(serde_json::json!("private-command-result-do-not-log")),
        why: Some("private-backend-reason-do-not-log\nforged log entry".into()),
        refused: None,
    };
    let record = command_trace(&answered).expect("a refusal has trace metadata");
    assert!(
        record == "command outcome committed=false",
        "the trace includes only an operation and outcome category"
    );
    answered.committed = true;
    assert!(
        command_trace(&answered).expect("acknowledgement metadata")
            == "command outcome committed=true",
        "a trace retains the actual commit flag"
    );
    answered.why = None;
    assert!(
        command_trace(&answered).is_none(),
        "ordinary successful commands add no trace"
    );
}
