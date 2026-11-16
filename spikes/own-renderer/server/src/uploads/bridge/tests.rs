//! The bridge's imports: each declaration's (never the first's alone), a
//! handle held to the request's session, and each answer in its ABI's shape:
//! a found value, or the declared `NotFound`, or a refusal that traps.
use super::*;
use crate::uploads::{LEASE, MemoryBlobs, Uploads, tests as fixtures};
use std::sync::atomic::{AtomicUsize, Ordering};

fn manifest() -> Manifest {
    let declared = |name: &str, route: &str, serves: &str| {
        serde_json::json!({"name": name, "module": name, "source": "Data", "route": route,
            "serves": serves, "max_bytes": 4096, "types": ["png", "jpeg", "webp", "gif"],
            "max_width": 640, "max_height": 480})
    };
    let json = serde_json::json!([
        declared("PostImage", "/uploads/post-image", "/images"),
        declared("Avatar", "/uploads/avatar", "/avatars"),
    ]);
    Manifest::from_json(&json.to_string()).expect("two declarations")
}

fn leases() -> (Uploads, Leases) {
    let post = fixtures::declared();
    let mut avatar = post.clone();
    avatar.name = "Avatar".into();
    avatar.route = "/uploads/avatar".into();
    avatar.serves = "/avatars".into();
    let uploads = Uploads::with(
        vec![post, avatar],
        Box::new(MemoryBlobs::default()),
        Box::new(MemoryBlobs::default()),
        LEASE,
    );
    let leases = uploads.leases().expect("staged leases");
    (uploads, leases)
}

fn stage(leases: &Leases, cookie: &str, route: &str) {
    let bytes = fixtures::png(6, 4);
    let (ty, body) = fixtures::form(&bytes, "image.png", "image/png");
    let headers = fixtures::headers_for(&ty, body.len());
    let answer = leases.answer("POST", route, &headers, cookie, false, &mut std::io::Cursor::new(body));
    assert_eq!(answer.status, 303, "staged");
}

/// A publisher that answers what it is proposed, and counts the receipts
/// required of the commit.
fn publisher(receipts: &Arc<AtomicUsize>) -> UploadPublisher {
    let required = Arc::clone(receipts);
    UploadPublisher {
        publish: Arc::new(|_, descriptor, _| Ok(descriptor.reference)),
        require_receipt: Arc::new(move || {
            required.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    }
}

fn call(ops: &BTreeMap<String, HostFn>, key: &str, args: &[&str]) -> Result<Vec<Val>, String> {
    let args: Vec<Val> = args.iter().map(|a| Val::String((*a).into())).collect();
    ops.get(key).unwrap_or_else(|| panic!("no import {key}"))(&args)
}

fn handle(cookie: &str) -> String {
    crate::identity::session_handle(cookie)
}

fn token(answer: &[Val]) -> String {
    match answer {
        [Val::Result(Ok(Some(value)))] => match value.as_ref() {
            Val::String(token) => token.clone(),
            other => panic!("a claim answers its token: {other:?}"),
        },
        other => panic!("a claim answers its token: {other:?}"),
    }
}

fn is_not_found(answer: &[Val]) -> bool {
    matches!(answer, [Val::Result(Err(Some(case)))] if matches!(case.as_ref(), Val::Variant(name, None) if name == "not-found"))
}

#[test]
fn every_declarations_imports_are_bound_and_none_besides() {
    let (_u, leases) = leases();
    let inv = Arc::new(Mutex::new(Invocation::new(Arc::clone(&leases), "reader", ReaderIdentity::Guest)));
    let receipts = Arc::new(AtomicUsize::new(0));
    let keys: Vec<String> = command_ops(&manifest(), &inv, &publisher(&receipts)).into_keys().collect();
    let mut expected: Vec<String> = ["avatar", "post-image"]
        .iter()
        .flat_map(|i| OPERATIONS.iter().map(move |op| format!("pw:uploads/{i}#{op}")))
        .collect();
    expected.sort();
    assert_eq!(keys, expected);
    let queried: Vec<String> = query_ops(&manifest(), &leases, "reader", &ReaderIdentity::Guest).into_keys().collect();
    assert_eq!(queried, ["pw:uploads/avatar#attached", "pw:uploads/post-image#attached"]);
    assert_eq!(refusing_ops(&manifest()).len(), 8);
}

#[test]
fn a_claim_publish_and_discard_answer_in_their_abis_shape() {
    let (_u, leases) = leases();
    stage(&leases, "reader", "/uploads/post-image");
    stage(&leases, "reader", "/uploads/avatar");
    let inv = Arc::new(Mutex::new(Invocation::new(Arc::clone(&leases), "reader", ReaderIdentity::Guest)));
    let receipts = Arc::new(AtomicUsize::new(0));
    let ops = command_ops(&manifest(), &inv, &publisher(&receipts));
    let me = handle("reader");
    // Attached, before and after the claim: the session's own lease.
    let attached = call(&ops, "pw:uploads/post-image#attached", &[&me]).expect("attached");
    assert!(matches!(attached.as_slice(), [Val::List(shown)] if shown.len() == 1), "{attached:?}");
    let claimed = token(&call(&ops, "pw:uploads/post-image#claim", &[&me]).expect("claimed"));
    assert_eq!(receipts.load(Ordering::SeqCst), 1, "a claim requires the commit's record");
    // Claimed already: no second claim of the same declaration.
    assert!(is_not_found(&call(&ops, "pw:uploads/post-image#claim", &[&me]).expect("answered")));
    let published = call(&ops, "pw:uploads/post-image#publish", &[&me, &claimed]).expect("published");
    match published.as_slice() {
        [Val::Result(Ok(Some(value)))] => match value.as_ref() {
            Val::Record(fields) => {
                let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, ["key", "generation", "kind", "width", "height"]);
                assert!(matches!(&fields[2].1, Val::Variant(kind, None) if kind == "png"));
                assert!(matches!(fields[3].1, Val::S64(6)) && matches!(fields[4].1, Val::S64(4)));
            }
            other => panic!("a reference: {other:?}"),
        },
        other => panic!("a published reference: {other:?}"),
    }
    // Published: its token closes no second time.
    assert!(is_not_found(&call(&ops, "pw:uploads/post-image#publish", &[&me, &claimed]).expect("answered")));
    // The other declaration's lease is its own, and is discarded whole.
    let avatar = token(&call(&ops, "pw:uploads/avatar#claim", &[&me]).expect("avatar"));
    let discarded = call(&ops, "pw:uploads/avatar#discard", &[&me, &avatar]).expect("discarded");
    assert!(
        matches!(discarded.as_slice(), [Val::Result(Ok(Some(value)))] if matches!(value.as_ref(), Val::Record(f) if f.len() == 3)),
        "{discarded:?}"
    );
    assert_eq!(receipts.load(Ordering::SeqCst), 2);
}

#[test]
fn another_sessions_handle_is_refused_and_a_missing_lease_is_not_found() {
    let (_u, leases) = leases();
    stage(&leases, "reader", "/uploads/post-image");
    let inv = Arc::new(Mutex::new(Invocation::new(Arc::clone(&leases), "reader", ReaderIdentity::Guest)));
    let receipts = Arc::new(AtomicUsize::new(0));
    let ops = command_ops(&manifest(), &inv, &publisher(&receipts));
    let other = handle("someone-else");
    for key in ["pw:uploads/post-image#claim", "pw:uploads/post-image#attached"] {
        let refused = call(&ops, key, &[&other]);
        assert!(matches!(&refused, Err(why) if why.contains("another session")), "{key}: {refused:?}");
    }
    // No Avatar lease is staged: not found, and nothing required of the commit.
    assert!(is_not_found(&call(&ops, "pw:uploads/avatar#claim", &[&handle("reader")]).expect("answered")));
    assert_eq!(receipts.load(Ordering::SeqCst), 0);
    // A query's `attached` holds the handle too.
    let queried = query_ops(&manifest(), &leases, "reader", &ReaderIdentity::Guest);
    assert!(call(&queried, "pw:uploads/post-image#attached", &[&other]).is_err());
    let own = call(&queried, "pw:uploads/post-image#attached", &[&handle("reader")]).expect("own");
    assert!(matches!(own.as_slice(), [Val::List(shown)] if shown.len() == 1), "{own:?}");
    let none = call(&queried, "pw:uploads/avatar#attached", &[&handle("reader")]).expect("none");
    assert!(matches!(none.as_slice(), [Val::List(shown)] if shown.is_empty()), "{none:?}");
}

#[test]
fn a_command_no_reader_runs_claims_nothing() {
    let ops = refusing_ops(&manifest());
    for (key, op) in &ops {
        let refused = op(&[Val::String(handle("reader"))]);
        assert!(matches!(&refused, Err(why) if why.contains("no reader runs")), "{key}: {refused:?}");
    }
}
