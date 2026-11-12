//! Actual staging + invocation outcome ledger. Receipt values here are trusted
//! test inputs; actual adapter/lifetime inspection remains an integration gate.
use super::*;
trait Must<T> {
    fn must(self, why: &str) -> T;
}
impl<T> Must<T> for Option<T> {
    fn must(self, why: &str) -> T {
        assert!(self.is_some(), "{why}");
        match self {
            Some(v) => v,
            None => unreachable!(),
        }
    }
}
impl<T, E> Must<T> for Result<T, E> {
    fn must(self, why: &str) -> T {
        assert!(self.is_ok(), "{why}");
        match self {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }
}
fn setup() -> (Uploads, Leases) {
    let post = super::super::tests::declared();
    let mut avatar = post.clone();
    avatar.name = "Avatar".into();
    avatar.route = "/uploads/avatar".into();
    avatar.serves = "/avatars".into();
    let u = Uploads::with(
        vec![post, avatar],
        Box::new(MemoryBlobs::default()),
        Box::new(MemoryBlobs::default()),
        LEASE,
    );
    let leases = u.leases().must("actual staged leases");
    (u, leases)
}
fn post(leases: &Shared, cookie: &str, declaration: &str) -> Vec<u8> {
    let bytes = super::super::tests::png(6, 4);
    let (ty, body) = super::super::tests::form(&bytes, "image.png", "image/png");
    let headers = super::super::tests::headers_for(&ty, body.len());
    let route = if declaration == "Avatar" {
        "/uploads/avatar"
    } else {
        "/uploads/post-image"
    };
    let answer = leases.answer(
        "POST",
        route,
        &headers,
        cookie,
        false,
        &mut std::io::Cursor::new(body),
    );
    assert_eq!(answer.status, 303, "actual upload stages");
    bytes
}
fn invocation(leases: &Leases, cookie: &str) -> Invocation {
    Invocation::new(Arc::clone(leases), cookie, ReaderIdentity::Guest)
}
fn handle(cookie: &str) -> String {
    crate::identity::session_handle(cookie)
}
fn claim(inv: &mut Invocation, declaration: &str, cookie: &str) -> String {
    inv.claim(declaration, &handle(cookie))
        .must("claim callback succeeds")
        .must("matching actual lease is available")
}
fn discard(inv: &mut Invocation, declaration: &str, cookie: &str, token: &str) {
    assert!(
        inv.discard(declaration, &handle(cookie), token)
            .must("discard callback")
            .is_some(),
        "exact held claim closes"
    );
}
fn answer(inv: &Invocation) -> String {
    inv.receipt_answer(r#"{"committed":true,"result":null,"why":null,"refused":null}"#)
        .must("platform answer is sealed")
}
fn begin(inv: &mut Invocation, answer: &str) -> Result<(), String> {
    inv.begin_commit(Some(&crate::data::Recorded {
        key: "logical-interaction",
        sent: "same reader and arguments",
        answer,
        at_ms: 1000,
        forget_before_ms: 0,
    }))
}
fn receipt(answer: String) -> Receipt {
    Receipt {
        key: "logical-interaction".into(),
        sent: "same reader and arguments".into(),
        answer,
        at_ms: 1000,
    }
}
#[test]
fn actual_claim_tokens_are_bound_to_request_session_declaration_invocation_and_single_disposition()
{
    let (_u, leases) = setup();
    let bytes = post(&leases, "reader", "PostImage");
    post(&leases, "reader", "Avatar");
    let mut inv = invocation(&leases, "reader");
    assert!(inv.claim("PostImage", &handle("foreign")).is_err());
    let a = claim(&mut inv, "PostImage", "reader");
    let b = claim(&mut inv, "Avatar", "reader");
    assert_ne!(a, b);
    assert!(
        inv.claim("PostImage", &handle("reader"))
            .must("same invocation lookup")
            .is_none()
    );
    assert!(
        inv.discard("Avatar", &handle("reader"), &a)
            .must("wrong declaration token")
            .is_none()
    );
    assert!(inv.discard("PostImage", &handle("foreign"), &a).is_err());
    let mut other = invocation(&leases, "reader");
    assert!(
        other
            .discard("PostImage", &handle("reader"), &a)
            .must("other invocation cannot consume token")
            .is_none()
    );
    let image = inv
        .publish(
            "PostImage",
            &handle("reader"),
            &a,
            |decl, incoming, canonical| {
                assert_eq!(decl, "PostImage");
                assert_eq!(&*canonical, &bytes);
                let mut selected = incoming.reference;
                selected.generation = "b".repeat(32);
                Ok(selected)
            },
        )
        .must("trusted row proposal")
        .must("exact claim publishes");
    assert_eq!(image.generation, "b".repeat(32));
    let called = std::cell::Cell::new(false);
    assert!(
        inv.publish("PostImage", &handle("reader"), &a, |_, _, _| {
            called.set(true);
            Err("spent claim must not reach proposer".into())
        })
        .must("spent publication lookup")
        .is_none()
    );
    assert!(
        !called.get(),
        "spent claim cannot propose another publication"
    );
    let shown = inv
        .discard("Avatar", &handle("reader"), &b)
        .must("Avatar discard")
        .must("exact second claim");
    assert!(shown.src.starts_with("/uploads/avatar/"));
    let payload = inv.payloads().next().must("selected canonical payload");
    assert_eq!(payload.descriptor.reference, image);
    assert_eq!(&*payload.bytes, &bytes);
    let sealed = answer(&inv);
    begin(&mut inv, &sealed).must("actual record and all claims closed");
    inv.applied()
        .must("known applied consumes both discarded and published leases");
    assert!(leases.shown("reader").is_none());
    assert_eq!(leases.staged.count().must("staged count"), 0);
}
#[test]
fn known_body_rollback_and_open_drop_return_the_exact_actual_lease() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    {
        let mut inv = invocation(&leases, "reader");
        let token = claim(&mut inv, "Avatar", "reader");
        discard(&mut inv, "Avatar", "reader", &token);
    }
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "Avatar", "reader");
    discard(&mut inv, "Avatar", "reader", &token);
    let sealed = answer(&inv);
    begin(&mut inv, &sealed).must("mark before possible COMMIT");
    inv.refused().must("explicit known rollback returns lease");
    assert!(leases.pending_attempts(1).must("pending").is_empty());
    let mut retry = invocation(&leases, "reader");
    assert!(
        retry
            .claim("Avatar", &handle("reader"))
            .must("valid retry")
            .is_some()
    );
}
#[test]
fn an_open_or_unreceipted_claim_cannot_enter_the_commit_phase() {
    let (_u, leases) = setup();
    post(&leases, "reader", "PostImage");
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "PostImage", "reader");
    let sealed = answer(&inv);
    assert!(
        begin(&mut inv, &sealed).is_err(),
        "every actual claim needs a disposition"
    );
    discard(&mut inv, "PostImage", "reader", &token);
    assert!(
        inv.begin_commit(None).is_err(),
        "idempotency metadata is no actual receipt"
    );
    assert!(
        begin(&mut inv, r#"{"committed":true}"#).is_err(),
        "the raw answer must identify this attempt"
    );
    assert!(leases.pending_attempts(10).must("pending").is_empty());
    begin(&mut inv, &sealed).must("sealed actual receipt is a valid neighbor");
    inv.refused().must("known noncommit");
}
#[test]
fn uncertain_drop_and_absent_or_different_receipts_never_return_a_claim_or_expire_it() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    let sealed = {
        let mut inv = invocation(&leases, "reader");
        let token = claim(&mut inv, "Avatar", "reader");
        discard(&mut inv, "Avatar", "reader", &token);
        let sealed = answer(&inv);
        begin(&mut inv, &sealed).must("marked before unanswered COMMIT");
        sealed
    };
    let tickets = leases
        .pending_attempts(1)
        .must("explicit pending invocation");
    assert_eq!(tickets.len(), 1);
    let ticket = &tickets[0];
    assert_eq!(ticket.key(), "logical-interaction");
    assert!(
        !leases
            .reconcile_attempt(ticket, None)
            .must("absence remains indeterminate")
    );
    let different =
        receipt(r#"{"committed":true,"result":null,"upload_attempt":"different"}"#.into());
    assert!(
        !leases
            .reconcile_attempt(ticket, Some(&different))
            .must("different attempt remains indeterminate")
    );
    {
        let mut state = leases.state.lock().must("staging");
        state
            .leases
            .get_mut(&lease_key("reader", "Avatar"))
            .must("held lease")
            .made = Instant::now()
            .checked_sub(LEASE + Duration::from_secs(1))
            .must("earlier time");
        leases.sweep(&mut state);
    }
    let (ty, body) = super::super::tests::form(
        &super::super::tests::png(8, 5),
        "replacement.png",
        "image/png",
    );
    let headers = super::super::tests::headers_for(&ty, body.len());
    assert_eq!(
        leases
            .answer(
                "POST",
                "/uploads/avatar",
                &headers,
                "reader",
                false,
                &mut std::io::Cursor::new(body)
            )
            .status,
        409,
        "unknown claim still blocks replacement after expiry/sweep"
    );
    assert!(
        leases
            .reconcile_attempt(ticket, Some(&receipt(sealed)))
            .must("matching actual attempt is positive applied proof")
    );
    assert!(
        leases
            .pending_attempts(1)
            .must("completed pending invocation")
            .is_empty()
    );
    assert_eq!(leases.staged.count().must("settled staging"), 0);
    assert!(
        leases.reconcile_attempt(ticket, None).is_err(),
        "a spent pending ticket cannot resolve twice"
    );
}
#[test]
fn receipt_from_another_interaction_key_cannot_consume_a_pending_upload_claim() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "Avatar", "reader");
    discard(&mut inv, "Avatar", "reader", &token);
    let sealed = answer(&inv);
    begin(&mut inv, &sealed).must("actual pending claim");
    drop(inv);
    let ticket = leases
        .pending_attempts(1)
        .must("pending inventory")
        .into_iter()
        .next()
        .must("pending ticket");
    let mut unrelated = receipt(sealed.clone());
    unrelated.key = "another-interaction".into();
    assert!(
        !leases
            .reconcile_attempt(&ticket, Some(&unrelated))
            .must("unrelated raw record must not settle this attempt"),
        "matching nonce, body and timestamp from a different key cannot consume the lease"
    );
    assert_eq!(
        leases.pending_attempts(1).must("still held").len(),
        1,
        "unrelated record leaves the actual pending claim intact"
    );
    assert!(
        leases
            .reconcile_attempt(&ticket, Some(&receipt(sealed)))
            .must("exact original key is the valid neighboring proof")
    );
    assert!(leases.pending_attempts(1).must("settled").is_empty());
}
#[test]
fn raw_attempt_metadata_survives_storage_but_decoding_and_reencoding_cannot_settle_it() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "Avatar", "reader");
    discard(&mut inv, "Avatar", "reader", &token);
    let sealed = answer(&inv);
    let decoded = crate::Answered::from_browser_receipt(&sealed)
        .must("ordinary browser receipt remains compatible");
    assert!(decoded.committed);
    assert_eq!(decoded.result, Some(serde_json::Value::Null));
    let roundtrip = decoded.kept();
    assert!(!roundtrip.contains(ATTEMPT_FIELD));
    begin(&mut inv, &sealed).must("raw receipt frozen");
    drop(inv);
    let ticket = leases
        .pending_attempts(1)
        .must("pending")
        .into_iter()
        .next()
        .must("raw attempt pending");
    assert!(
        !leases
            .reconcile_attempt(&ticket, Some(&receipt(roundtrip)))
            .must("decoded answer lacks attempt provenance")
    );
    assert!(
        leases
            .reconcile_attempt(&ticket, Some(&receipt(sealed)))
            .must("raw actual receipt preserves provenance")
    );
}
#[test]
fn equal_clock_equal_logical_answers_from_a_different_attempt_do_not_settle_the_old_lease() {
    let (_ua, a) = setup();
    let (_ub, b) = setup();
    post(&a, "reader", "Avatar");
    post(&b, "reader", "Avatar");
    let mut first = invocation(&a, "reader");
    let token = claim(&mut first, "Avatar", "reader");
    discard(&mut first, "Avatar", "reader", &token);
    let old_answer = answer(&first);
    begin(&mut first, &old_answer).must("old attempt uncertain");
    drop(first);
    let mut second = invocation(&b, "reader");
    let token = claim(&mut second, "Avatar", "reader");
    discard(&mut second, "Avatar", "reader", &token);
    let new_answer = answer(&second);
    assert_ne!(old_answer, new_answer);
    begin(&mut second, &new_answer).must("same key/sent/clock new attempt");
    second.applied().must("other attempt actually applied");
    let ticket = a
        .pending_attempts(1)
        .must("old local pending")
        .into_iter()
        .next()
        .must("old uncertain attempt");
    assert!(
        !a.reconcile_attempt(&ticket, Some(&receipt(new_answer)))
            .must("logical interaction proof cannot consume a different local lease")
    );
    assert_eq!(a.pending_attempts(1).must("still unresolved").len(), 1);
    assert_eq!(a.staged.count().must("old private bytes held"), 1);
}
#[test]
fn opaque_pending_ticket_cannot_cross_independent_lease_instances_even_with_equal_attempt_text() {
    let (_ua, a) = setup();
    let (_ub, b) = setup();
    post(&a, "reader", "Avatar");
    post(&b, "reader", "Avatar");
    let mut first = invocation(&a, "reader");
    let mut second = invocation(&b, "reader");
    second.attempt = first.attempt.clone();
    for inv in [&mut first, &mut second] {
        let token = claim(inv, "Avatar", "reader");
        discard(inv, "Avatar", "reader", &token);
        let sealed = answer(inv);
        begin(inv, &sealed).must("separate instance pending");
    }
    let ticket = a
        .pending_attempts(1)
        .must("first ticket")
        .into_iter()
        .next()
        .must("first pending ticket");
    assert!(
        b.reconcile_attempt(&ticket, None).is_err(),
        "private invocation identity is not transferable by copied nonce text"
    );
    first.refused().must("own known rollback");
    second.refused().must("other own known rollback");
}
#[test]
fn oversized_pending_proof_refuses_before_commit_and_returns_its_claim() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "Avatar", "reader");
    discard(&mut inv, "Avatar", "reader", &token);
    let sealed = answer(&inv);
    let huge = "x".repeat(16 * 1024 * 1024);
    assert!(
        inv.begin_commit(Some(&crate::data::Recorded {
            key: "key",
            sent: &huge,
            answer: &sealed,
            at_ms: 1000,
            forget_before_ms: 0
        }))
        .is_err(),
        "proof text budget refuses before an uncertain phase"
    );
    assert!(
        leases
            .pending_attempts(1)
            .must("no pending insertion")
            .is_empty()
    );
    inv.refused().must("known refusal returns lease");
    let mut retry = invocation(&leases, "reader");
    assert!(
        retry
            .claim("Avatar", &handle("reader"))
            .must("returned lease")
            .is_some()
    );
}
#[test]
fn bounded_pending_count_refuses_new_uncertainty_without_releasing_old_claims() {
    let (_u, leases) = setup();
    for n in 0..1024 {
        let cookie = format!("reader-{n}");
        post(&leases, &cookie, "Avatar");
        let mut inv = invocation(&leases, &cookie);
        let token = claim(&mut inv, "Avatar", &cookie);
        discard(&mut inv, "Avatar", &cookie, &token);
        let sealed = answer(&inv);
        begin(&mut inv, &sealed).must("pending budget valid neighbor");
    }
    assert_eq!(
        leases
            .pending_attempts(usize::MAX)
            .must("bounded inventory")
            .len(),
        1024
    );
    let cookie = "new-reader";
    post(&leases, cookie, "Avatar");
    let mut inv = invocation(&leases, cookie);
    let token = claim(&mut inv, "Avatar", cookie);
    discard(&mut inv, "Avatar", cookie, &token);
    let sealed = answer(&inv);
    assert!(
        begin(&mut inv, &sealed).is_err(),
        "new uncertain metadata cannot exceed the pending count budget"
    );
    inv.refused()
        .must("known budget refusal returns only its own claim");
    let returned = leases
        .claim_for(cookie, "Avatar", &ReaderIdentity::Guest)
        .must("new invocation lease returned");
    leases.unclaim(returned);
    assert!(
        leases
            .claim_for("reader-0", "Avatar", &ReaderIdentity::Guest)
            .is_none(),
        "old unresolved claims remain held"
    );
}

#[test]
fn pending_phase_rejects_late_operations_and_only_an_exact_raw_receipt_can_resolve_it() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    post(&leases, "reader", "PostImage");
    let mut inv = invocation(&leases, "reader");
    let token = claim(&mut inv, "Avatar", "reader");
    discard(&mut inv, "Avatar", "reader", &token);
    let sealed = answer(&inv);
    begin(&mut inv, &sealed).must("pending before commit");
    assert!(
        inv.claim("PostImage", &handle("reader")).is_err(),
        "late callbacks cannot acquire another lease after entering commit"
    );
    assert!(inv.discard("Avatar", &handle("reader"), &token).is_err());
    assert!(inv.receipt_answer("{}").is_err());
    let ticket = leases
        .pending_attempts(1)
        .must("pending")
        .into_iter()
        .next()
        .must("pending ticket");
    let mut wrong = receipt(sealed.clone());
    wrong.sent = "another reader or body".into();
    assert!(
        !leases
            .reconcile_attempt(&ticket, Some(&wrong))
            .must("wrong canonical sent body is indeterminate")
    );
    let mut wrong = receipt(sealed);
    wrong.at_ms += 1;
    assert!(
        !leases
            .reconcile_attempt(&ticket, Some(&wrong))
            .must("wrong stored receipt instant is indeterminate")
    );
    inv.refused()
        .must("explicit known rollback still returns held claim");
}
#[test]
fn publication_checks_foreign_session_token_and_the_row_adapters_returned_canonical_descriptor() {
    let (_u, leases) = setup();
    post(&leases, "reader", "Avatar");
    post(&leases, "reader", "PostImage");
    let mut inv = invocation(&leases, "reader");
    let a = claim(&mut inv, "Avatar", "reader");
    let b = claim(&mut inv, "PostImage", "reader");
    assert!(
        inv.publish("Avatar", &handle("foreign"), &a, |_, d, _| Ok(d.reference))
            .is_err(),
        "foreign session handle is refused before publication"
    );
    assert!(
        inv.publish("Avatar", &handle("reader"), &b, |_, d, _| Ok(d.reference))
            .must("other declaration token lookup")
            .is_none(),
        "another declaration's token cannot publish this lease"
    );
    assert!(
        inv.publish("Avatar", &handle("reader"), &a, |_, mut d, _| {
            d.reference.key = "f".repeat(64);
            Ok(d.reference)
        })
        .is_err(),
        "row adapter cannot silently substitute canonical metadata"
    );
    discard(&mut inv, "Avatar", "reader", &a);
    discard(&mut inv, "PostImage", "reader", &b);
    let sealed = answer(&inv);
    begin(&mut inv, &sealed).must("refused proposals left claims closable");
    inv.refused().must("known rollback");
}
