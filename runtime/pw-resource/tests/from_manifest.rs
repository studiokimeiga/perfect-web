//! The compiler and the runtime, connected.
//!
//! Until this existed, `hir::Policy` recorded what an author wrote and
//! `pw_resource::Manifest` was built by hand in tests, with nothing in between —
//! so "the runtime honours the declared policy" was an assertion about two
//! independent pieces of code that happened to agree.
//!
//! These tests take a real `.pw` declaration, generate its manifest with the
//! compiler, feed it to the runtime, and assert the runtime *behaves* the way
//! the source says. A change to either side that breaks the correspondence
//! fails here.

use pw_core::lower::lower_file;
use pw_core::manifest as m;
use pw_resource::*;
use pw_syntax::parse_tree;

/// Build the manifests a `.pw` source declares.
fn manifests(src: &str) -> m::Built {
    let p = parse_tree(src);
    assert!(p.ok(), "source must parse: {:?}", p.errors);
    m::build(&lower_file(src, &p.green))
}

/// The compiler's manifest, as the runtime's.
///
/// This conversion is the connection. It lives in the test rather than in
/// either crate on purpose: the compiler must not depend on one host's runtime,
/// and the runtime must not depend on the compiler. Both read the schema.
fn to_runtime(c: &m::Manifest) -> Manifest {
    let mut r = Manifest::new(&c.name);
    r.privacy = match c.privacy {
        m::Privacy::Public => Privacy::Public,
        // Session and private values are both per-user; neither may be served
        // from a cache shared between users.
        m::Privacy::Session | m::Privacy::Private => Privacy::Private,
    };
    if c.cache_partition == m::CachePartition::Private {
        r.privacy = Privacy::Private;
    }
    if let Some(f) = c.freshness {
        r.freshness = f;
    }
    if let Some(t) = c.timeout {
        r.timeout = t;
    }
    r.max_attempts = match &c.retry {
        m::Retry::None => 1,
        // `forever` is a declaration the checker rejects (PW0313). If one
        // reaches the runtime anyway, it must not actually loop forever —
        // a runtime that trusted an unchecked policy would hang.
        m::Retry::Forever => 1,
        m::Retry::Bounded { max, .. } => *max,
    };
    r
}

const STORE_QUERY: &str = "\
module store.queries

public query Store(id: StoreId) -> Result<Store, StoreError>
    cache       shared
    freshness   30.seconds
    consistency snapshot
    retry       bounded_exponential(max = 3, jitter = true)
    concurrency one_per_key
    timeout     2.seconds
{
    Stores.get(id)
}
";

#[test]
fn a_declared_freshness_window_governs_the_runtime() {
    let built = manifests(STORE_QUERY);
    assert!(built.unparsed.is_empty(), "{:?}", built.unparsed);
    let declared = &built.manifests[0];
    assert_eq!(
        declared.freshness,
        Some(30_000),
        "30.seconds, from the source"
    );

    let clock = Clock::new();
    let rt = Resources::new(clock.clone());
    let rm = to_runtime(declared);
    let key = Key::new(&declared.name, "blue-bottle");

    let mut calls = 0;
    let mut load = |_: u32| {
        calls += 1;
        Ok("Blue Bottle".to_string())
    };
    assert!(matches!(rt.fetch(&rm, &key, &mut load), Fetched::Fresh(_)));

    // One millisecond inside the declared window: still served from cache.
    clock.advance(29_999);
    assert!(matches!(
        rt.fetch(&rm, &key, &mut load),
        Fetched::FromCache(_)
    ));

    // One millisecond past it: refetched. The boundary is the number written
    // in the `.pw` file, not a default that happens to be close to it.
    clock.advance(2);
    assert!(matches!(rt.fetch(&rm, &key, &mut load), Fetched::Fresh(_)));
    assert_eq!(calls, 2);
}

#[test]
fn changing_the_source_changes_the_behaviour() {
    // The control that makes the test above mean something. If the runtime
    // ignored the manifest and used its own default, the first test would still
    // pass whenever the default matched — so edit the source and watch the
    // boundary move.
    let src = STORE_QUERY.replace("freshness   30.seconds", "freshness   5.seconds");
    let built = manifests(&src);
    let rm = to_runtime(&built.manifests[0]);
    assert_eq!(rm.freshness, 5_000);

    let clock = Clock::new();
    let rt = Resources::new(clock.clone());
    let key = Key::new("Store", "k");
    rt.fetch(&rm, &key, |_| Ok("v".to_string()));

    clock.advance(6_000);
    let after = rt.fetch(&rm, &key, |_| Ok("v2".to_string()));
    assert!(
        matches!(&after, Fetched::Fresh(v) if v == "v2"),
        "at 6s the 5s window must have expired: {after:?}"
    );
}

#[test]
fn a_declared_retry_bound_governs_how_many_attempts_run() {
    let built = manifests(STORE_QUERY);
    let rm = to_runtime(&built.manifests[0]);
    assert_eq!(rm.max_attempts, 3, "max = 3, from the source");

    let rt = Resources::new(Clock::new());
    let mut attempts = 0;
    let result = rt.fetch(&rm, &Key::new("Store", "k"), |_| {
        attempts += 1;
        Err("upstream down".to_string())
    });
    assert!(matches!(result, Fetched::Failed(_)));
    assert_eq!(
        attempts, 3,
        "exactly what the declaration says, not a default"
    );
}

#[test]
fn a_session_query_is_never_in_public_cache_output() {
    // The privacy field is not decoration: it decides which cache the value
    // lands in, and the M4 gate is about what a public cache can produce.
    let src = "\
module cart.queries

session query Cart(s: SessionId) -> Cart
    cache     private
    freshness 10.seconds
{
    Carts.current(s)
}
";
    let built = manifests(src);
    let declared = &built.manifests[0];
    assert_eq!(declared.privacy, m::Privacy::Session);
    assert_eq!(declared.cache_partition, m::CachePartition::Private);

    let rt = Resources::new(Clock::new());
    let rm = to_runtime(declared);
    rt.fetch(&rm, &Key::new("Cart", "session-1"), |_| {
        Ok("2 items, $9.25".to_string())
    });

    assert!(
        rt.public_cache_contents().is_empty(),
        "a session query must not appear in public cache output: {:?}",
        rt.public_cache_contents()
    );

    // Control: a public query from the same runtime does appear, so the check
    // is not passing because nothing is ever cached.
    let public = manifests(STORE_QUERY);
    let pm = to_runtime(&public.manifests[0]);
    rt.fetch(&pm, &Key::new("Store", "k"), |_| Ok("Blue Bottle".into()));
    assert_eq!(rt.public_cache_contents().len(), 1);
}

#[test]
fn a_policy_the_schema_cannot_read_is_reported_not_defaulted() {
    // A manifest with a silently defaulted freshness is worse than none: the
    // runtime serves stale data and nothing says why.
    let src = STORE_QUERY.replace("freshness   30.seconds", "freshness   soon");
    let built = manifests(&src);
    assert_eq!(built.manifests.len(), 1, "the manifest is still produced");
    assert_eq!(
        built.manifests[0].freshness, None,
        "but the field is absent"
    );

    let bad = &built.unparsed;
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert_eq!(bad[0].policy, "freshness");
    assert_eq!(bad[0].value, "soon");
    assert_eq!(bad[0].reason, "not a duration");
}

#[test]
fn every_accepted_corpus_resource_produces_a_manifest_the_schema_understands() {
    // The coverage property. Any policy value the corpus uses that the schema
    // cannot read shows up here rather than as a field quietly set to None.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/accepted");
    let mut total = 0usize;
    let mut unparsed = Vec::new();

    for e in std::fs::read_dir(&dir).expect("accepted/") {
        let path = e.expect("entry").path();
        if path.extension().is_none_or(|x| x != "pw") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        let built = manifests(&src);
        total += built.manifests.len();
        for u in built.unparsed {
            unparsed.push(format!(
                "{}: {} declares `{} {}` — {}",
                path.file_name().unwrap().to_string_lossy(),
                u.declaration,
                u.policy,
                u.value,
                u.reason
            ));
        }
    }

    // The accepted corpus declares exactly 25 written resources: the 14
    // through C26 (including A-038's loop query), plus A-050's four table
    // queries and two commands, A-058's command and query (track
    // `row-ownership`, a predicate with a body required where it is
    // imported), and A-060's query and two commands (L2, ADR-0395: a public
    // table read by anyone and written by its owner). Generated functions
    // and predicates are not manifests. Pin every declaration so a missing
    // lowering cannot shrink the sample.
    assert_eq!(total, 25, "expected every accepted resource declaration");
    assert!(
        unparsed.is_empty(),
        "{} policy value(s) the schema does not understand:\n{}",
        unparsed.len(),
        unparsed.join("\n")
    );
}

#[test]
fn changing_the_declared_timeout_changes_late_result_acceptance() {
    // Same work, two source policies. A hand-built default cannot satisfy both.
    for (timeout, should_succeed) in [("2.seconds", false), ("5.seconds", true)] {
        let src = STORE_QUERY.replace("timeout     2.seconds", &format!("timeout     {timeout}"));
        let built = manifests(&src);
        assert!(built.unparsed.is_empty());
        let rm = to_runtime(&built.manifests[0]);
        let clock = Clock::new();
        let rt = Resources::new(clock.clone());
        let result = rt.fetch(&rm, &Key::new("Store", "deadline"), |_| {
            clock.advance(3_000);
            Ok("finished".into())
        });
        assert_eq!(
            result,
            if should_succeed {
                Fetched::Fresh("finished".into())
            } else {
                Fetched::TimedOut
            }
        );
        assert_eq!(
            rt.public_cache_contents().len(),
            usize::from(should_succeed)
        );
    }
}
