//! Actual staging/preview scope before the generic invocation bridge opens.
use super::*;
trait RequireClaim {
    fn require_claim(self, why: &str) -> Claimed;
}
impl RequireClaim for Option<Claimed> {
    fn require_claim(self, why: &str) -> Claimed {
        assert!(
            self.is_some(),
            "a declaration-scoped claim must be available: {why}"
        );
        match self {
            Some(claimed) => claimed,
            None => unreachable!(),
        }
    }
}

fn two() -> Uploads {
    let post = tests::declared();
    let mut avatar = post.clone();
    avatar.name = "Avatar".into();
    avatar.route = "/uploads/avatar".into();
    avatar.serves = "/avatars".into();
    Uploads::with(
        vec![post, avatar],
        Box::new(MemoryBlobs::default()),
        Box::new(MemoryBlobs::default()),
        LEASE,
    )
}
fn shared(u: &Uploads) -> &Shared {
    u.shared.as_deref().unwrap()
}
fn post(u: &Uploads, route: &str, session: &str, bytes: &[u8]) -> Answer {
    let (ty, body) = tests::form(bytes, "image.png", "image/png");
    let headers = tests::headers_for(&ty, body.len());
    shared(u).answer(
        "POST",
        route,
        &headers,
        session,
        false,
        &mut std::io::Cursor::new(body),
    )
}
fn preview(u: &Uploads, route: &str, id: &str, session: &str) -> Answer {
    shared(u).answer(
        "GET",
        &format!("{route}/{id}"),
        "",
        session,
        false,
        &mut std::io::Cursor::new(Vec::<u8>::new()),
    )
}
#[test]
fn two_declarations_stage_and_replace_independently_for_one_session() {
    let u = two();
    let a = tests::png(6, 4);
    let b = tests::png(8, 5);
    let c = tests::png(7, 4);
    assert_eq!(post(&u, "/uploads/post-image", "reader", &a).status, 303);
    assert_eq!(post(&u, "/uploads/avatar", "reader", &b).status, 303);
    let first = preview(&u, "/uploads/post-image", "up-1", "reader");
    assert_eq!(first.status, 200);
    assert_eq!(first.body, a);
    let second = preview(&u, "/uploads/avatar", "up-2", "reader");
    assert_eq!(second.status, 200);
    assert_eq!(second.body, b);
    assert_eq!(
        preview(&u, "/uploads/avatar", "up-2", "foreign").status,
        404
    );
    assert_eq!(post(&u, "/uploads/post-image", "reader", &c).status, 303);
    assert_eq!(
        preview(&u, "/uploads/post-image", "up-1", "reader").status,
        404
    );
    assert_eq!(preview(&u, "/uploads/avatar", "up-2", "reader").body, b);
    assert_eq!(preview(&u, "/uploads/post-image", "up-3", "reader").body, c);
    assert_eq!(shared(&u).state.lock().unwrap().leases.len(), 2);
}
#[test]
fn private_preview_route_cannot_borrow_another_declarations_lease() {
    let u = two();
    let image = tests::png(6, 4);
    assert_eq!(post(&u, "/uploads/avatar", "reader", &image).status, 303);
    assert_eq!(preview(&u, "/uploads/avatar", "up-1", "reader").status, 200);
    assert_eq!(
        preview(&u, "/uploads/post-image", "up-1", "reader").status,
        404,
        "the other declaration's private route is no lease authority"
    );
}
#[test]
fn identical_bytes_hold_one_staged_blob_until_both_declarations_release_their_own_lease() {
    let u = two();
    let image = tests::png(6, 4);
    assert_eq!(
        post(&u, "/uploads/post-image", "reader", &image).status,
        303
    );
    assert_eq!(post(&u, "/uploads/avatar", "reader", &image).status, 303);
    assert_eq!(
        shared(&u)
            .state
            .lock()
            .unwrap()
            .holds
            .get(&BlobKey::of(&image)),
        Some(&2)
    );
    assert_eq!(shared(&u).staged.count().unwrap(), 1);
    let claim = shared(&u)
        .claim("reader")
        .require_claim("legacy wrapper claims its first declaration only");
    shared(&u).discarded(claim);
    assert_eq!(
        preview(&u, "/uploads/post-image", "up-1", "reader").status,
        404
    );
    assert_eq!(preview(&u, "/uploads/avatar", "up-2", "reader").body, image);
    assert_eq!(shared(&u).staged.count().unwrap(), 1);
}

#[test]
fn explicit_claims_block_only_their_own_declarations_replacement_and_release() {
    let u = two();
    let a = tests::png(6, 4);
    let b = tests::png(8, 5);
    let replacement = tests::png(7, 4);
    assert_eq!(post(&u, "/uploads/post-image", "reader", &a).status, 303);
    assert_eq!(post(&u, "/uploads/avatar", "reader", &b).status, 303);
    let reader = crate::identity::ReaderIdentity::Guest;
    let first = shared(&u)
        .claim_for("reader", "PostImage", &reader)
        .require_claim("own first declaration");
    assert_eq!(first.declaration, "PostImage");
    assert_eq!(first.reader, reader);
    assert_eq!(
        post(&u, "/uploads/avatar", "reader", &replacement).status,
        303,
        "another declaration is independent of this hold"
    );
    let second = shared(&u)
        .claim_for("reader", "Avatar", &reader)
        .require_claim("own second declaration");
    assert_eq!(second.id, "up-3");
    assert_eq!(second.declaration, "Avatar");
    assert!(
        shared(&u)
            .claim_for("reader", "PostImage", &reader)
            .is_none()
    );
    assert!(shared(&u).claim_for("reader", "Avatar", &reader).is_none());
    assert_eq!(
        post(&u, "/uploads/post-image", "reader", &replacement).status,
        409
    );
    assert_eq!(post(&u, "/uploads/avatar", "reader", &b).status, 409);
    shared(&u).unclaim(second);
    assert_eq!(
        post(&u, "/uploads/avatar", "reader", &b).status,
        303,
        "unclaim returns exactly Avatar"
    );
    assert_eq!(
        post(&u, "/uploads/post-image", "reader", &replacement).status,
        409,
        "PostImage remains held"
    );
    shared(&u).discarded(first);
    assert_eq!(
        preview(&u, "/uploads/post-image", "up-1", "reader").status,
        404
    );
    assert_eq!(preview(&u, "/uploads/avatar", "up-4", "reader").body, b);
    let second = shared(&u)
        .claim_for("reader", "Avatar", &reader)
        .require_claim("Avatar can be consumed independently");
    shared(&u).discarded(second);
    assert_eq!(preview(&u, "/uploads/avatar", "up-4", "reader").status, 404);
    assert_eq!(shared(&u).staged.count().unwrap(), 0);
}
#[test]
fn explicit_attachment_and_claim_lookups_use_declaration_and_captured_reader_without_recapture() {
    let u = two();
    assert_eq!(
        post(&u, "/uploads/post-image", "reader", &tests::png(6, 4)).status,
        303
    );
    assert_eq!(
        post(&u, "/uploads/avatar", "reader", &tests::png(8, 5)).status,
        303
    );
    let reader = crate::identity::ReaderIdentity::Guest;
    let changed = crate::identity::ReaderIdentity::Anonymous;
    let shown = shared(&u)
        .shown_for("reader", "Avatar", &reader)
        .require_claim("Avatar attachment");
    assert_eq!(shown.src, "/uploads/avatar/up-2");
    assert_eq!((shown.width, shown.height), (8, 5));
    assert!(shared(&u).shown_for("reader", "Avatar", &changed).is_none());
    assert!(shared(&u).shown_for("foreign", "Avatar", &reader).is_none());
    assert!(shared(&u).shown_for("reader", "Missing", &reader).is_none());
    assert!(shared(&u).claim_for("reader", "Missing", &reader).is_none());
    assert!(shared(&u).claim_for("reader", "Avatar", &changed).is_none());
    assert!(shared(&u).claim_for("foreign", "Avatar", &reader).is_none());
    let claimed = shared(&u)
        .claim_for("reader", "Avatar", &reader)
        .require_claim("captured matching reader remains available");
    assert_eq!(claimed.reader, reader);
    shared(&u).unclaim(claimed);
}
#[test]
fn claim_debug_never_prints_the_cookie_private_lease_id_or_private_image_key() {
    let u = two();
    let cookie = "cookie-marker-must-stay-private";
    let image = tests::png(6, 4);
    assert_eq!(post(&u, "/uploads/avatar", cookie, &image).status, 303);
    let claimed = shared(&u)
        .claim_for(cookie, "Avatar", &crate::identity::ReaderIdentity::Guest)
        .require_claim("own attachment");
    let debug = format!("{claimed:?} {claimed:#?}");
    assert!(
        !debug.contains(cookie),
        "claim diagnostics must redact the session"
    );
    assert!(
        !debug.contains(&claimed.id),
        "claim diagnostics must redact the private lease token"
    );
    assert!(
        !debug.contains(claimed.key.hex()),
        "claim diagnostics must redact the private image hash"
    );
    assert!(debug.contains("Avatar"));
    shared(&u).unclaim(claimed);
}
fn expired() -> Uploads {
    let u = Uploads::with(
        vec![tests::declared()],
        Box::new(MemoryBlobs::default()),
        Box::new(MemoryBlobs::default()),
        Duration::ZERO,
    );
    assert_eq!(
        post(&u, "/uploads/post-image", "reader", &tests::png(6, 4)).status,
        303
    );
    u
}
#[test]
fn expired_unclaimed_attachment_metadata_is_hidden_before_periodic_sweep() {
    let u = expired();
    assert!(
        shared(&u)
            .shown_for(
                "reader",
                "PostImage",
                &crate::identity::ReaderIdentity::Guest
            )
            .is_none(),
        "elapsed lease is no private attachment authority"
    );
}
#[test]
fn expired_unclaimed_lease_cannot_be_acquired_before_periodic_sweep() {
    let u = expired();
    assert!(
        shared(&u)
            .claim_for(
                "reader",
                "PostImage",
                &crate::identity::ReaderIdentity::Guest
            )
            .is_none(),
        "elapsed unclaimed lease cannot become a held claim"
    );
}
#[test]
fn expired_unclaimed_private_preview_is_404_before_periodic_sweep() {
    let u = expired();
    assert_eq!(
        preview(&u, "/uploads/post-image", "up-1", "reader").status,
        404,
        "expired bytes remain private even before physical sweep"
    );
}
#[test]
fn claimed_lease_survives_expiry_and_sweep_until_an_explicit_outcome() {
    let u = two();
    let image = tests::png(6, 4);
    assert_eq!(post(&u, "/uploads/avatar", "reader", &image).status, 303);
    let claimed = shared(&u)
        .claim_for("reader", "Avatar", &crate::identity::ReaderIdentity::Guest)
        .require_claim("held before expiry");
    {
        let mut state = shared(&u).state.lock().unwrap();
        state
            .leases
            .get_mut(&lease_key("reader", "Avatar"))
            .unwrap()
            .made = Instant::now()
            .checked_sub(LEASE + Duration::from_secs(1))
            .unwrap();
        shared(&u).sweep(&mut state);
    }
    assert_eq!(preview(&u, "/uploads/avatar", "up-1", "reader").body, image);
    assert!(
        shared(&u)
            .shown_for("reader", "Avatar", &crate::identity::ReaderIdentity::Guest)
            .is_some()
    );
    assert_eq!(post(&u, "/uploads/avatar", "reader", &image).status, 409);
    shared(&u).unclaim(claimed);
    assert!(
        shared(&u)
            .shown_for("reader", "Avatar", &crate::identity::ReaderIdentity::Guest)
            .is_none(),
        "returned but expired lease is no longer shown"
    );
    {
        let mut state = shared(&u).state.lock().unwrap();
        shared(&u).sweep(&mut state);
        assert!(state.leases.is_empty());
    }
    assert_eq!(shared(&u).staged.count().unwrap(), 0);
}
