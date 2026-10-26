#[test]
fn privacy_refusals_leave_no_lease_staged_or_final_blob() {
    let d = dir();
    let u = uploads(d.path(), declared(), LEASE);
    for name in ["privacy-research-late-chrm.png", "privacy-research-duplicate-chrm.png", "privacy-research-idat-suffix.png", "privacy-late-chrm.png", "privacy-duplicate-chrm.png", "privacy-idat-suffix.png", "privacy-later-idat-suffix.png", "privacy-nonconsecutive-complete-idat.png", "privacy-decoded-suffix.png", "privacy-apng-suffix.png", "privacy-orphan-fdat.png"] {
        let sent = strip::tests::fixture(name);
        assert_eq!(post(&u, "privacy", &sent, name, "image/png").status, 422);
        let leases = shared(&u);
        assert!(leases.shown("privacy").is_none());
        let state = leases.state.lock().expect("state");
        assert_eq!(state.staged_bytes, 0);
        assert!(state.leases.is_empty());
        drop(state);
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(files(&d.path().join("blobs")), 0);
        let raw = format!("/images/{}.png", BlobKey::of(&sent).hex());
        assert_eq!(get(&u, &raw, "privacy", "").status, 404);
    }
}

#[test]
fn privacy_rewrite_is_the_preview_claim_storage_key_and_served_body() {
    for name in [ "privacy-duplicate-icc.png", "privacy-valid-icc.png", "privacy-clean.png", "privacy-split-idat.png", "privacy-apng-included.png", "privacy-apng-excluded.png", "privacy-apng-split.png"] {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let sent = strip::tests::fixture(name);
        let kept = strip::strip(&sent).required("rewritten image");
        assert_eq!(post(&u, "privacy", &sent, name, "image/png").status, 303);
        let leases = shared(&u);
        let shown = leases.shown("privacy").required("staged preview");
        let preview = get(&u, &shown.src, "privacy", "");
        assert_eq!(preview.status, 200);
        assert_eq!(preview.body, kept);
        assert_eq!(get(&u, &shown.src, "other", "").status, 404);
        assert_eq!(leases.state.lock().expect("state").staged_bytes, sent.len().max(kept.len()) as u64);
        let claimed = leases.claim("privacy").required("claimed preview");
        let key = BlobKey::of(&kept);
        assert_eq!(claimed.key, key);
        let stored = d.path().join("staged").join(&key.hex()[..2]).join(key.hex());
        assert_eq!(std::fs::read(stored).expect("staged file"), kept);
        leases.keep(&claimed).required("final storage succeeds");
        leases.committed(claimed);
        let expected = key.hex().to_string();
        u.named_by(Box::new(move |named| Ok(named == expected)));
        let served = get(&u, &leases.src(&key, Kind::Png), "any", "");
        assert_eq!(served.status, 200);
        assert_eq!(served.body, kept);
        assert_eq!(strip::strip(&served.body), Ok(kept.clone()));
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(files(&d.path().join("blobs")), 1);
        assert_eq!(leases.state.lock().expect("state").staged_bytes, 0);
        assert_eq!(get(&u, &shown.src, "privacy", "").status, 404);
        if name.ends_with("duplicate-icc.png") {
            assert!(strip::tests::profiles_in(&served.body).is_empty());
            assert_ne!(BlobKey::of(&sent), key);
            let original = d.path().join("blobs").join(&BlobKey::of(&sent).hex()[..2]).join(BlobKey::of(&sent).hex());
            assert!(!original.exists(), "raw profile-bearing input was never kept");
        }
    }
}