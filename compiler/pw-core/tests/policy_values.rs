//! **A policy's value is one its domain has** (ADR-0089).
//!
//! `crate::policy` says what each head's value is: a word from a closed set,
//! a duration, a world, a parameter, a type, an operator with a signature.
//! Nothing held a value to it until 2026-09-26, and each reader of a clause
//! decided alone what it meant. `cache Shared` was no shared cache to the
//! rule that keeps a session's data out of one, and no cache to the
//! manifest. `placement originn`, `retry nope(..)` and `key nope` checked.
//! And three readers matched a value loosely enough to be bypassed:
//! `freshness 05.seconds` began with `0`, a key item called `username`
//! contained `user`, and `transport_onlyish` began with `transport_only`.
//! Each test states one case, with a control.

use pw_core::check::check_sources;

fn program(src: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out.push(("t.pw".to_string(), src.to_string()));
    out
}

/// What `pw check` reports for `t.pw`, the declaration rules included
/// (ADR-0090).
fn reported(src: &str) -> Vec<String> {
    check_sources(&program(src))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// Exactly these, in any order.
fn says(src: &str, expected: &[&str]) {
    let found = reported(src);
    assert_eq!(found.len(), expected.len(), "{src}\n{found:#?}");
    for e in expected {
        assert!(
            found.iter().any(|f| f.contains(e)),
            "{e}\n{src}\n{found:#?}"
        );
    }
}

fn clean(src: &str) {
    says(src, &[]);
}

/// A public query whose policies are `policies`, one per line.
fn query(policies: &str) -> String {
    format!(
        "module t\n\npublic query Box(id: Int) -> Int !{{}}\n    freshness 30.seconds\n    {}\n{{\n    id\n}}\n",
        policies.replace('\n', "\n    ")
    )
}

/// A session query: its data belongs to one session.
fn session_query(policies: &str) -> String {
    format!(
        "module t\n\nsession query Mine(id: Int) -> Int !{{}}\n    {}\n{{\n    id\n}}\n",
        policies.replace('\n', "\n    ")
    )
}

/// A command, idempotent or not.
fn command(policies: &str) -> String {
    format!(
        "module t\n\nopaque type Tag = String\n\ncommand act(n: Int) -> Int !{{}}\n    requires Anyone\n    {}\n{{\n    n\n}}\n",
        policies.replace('\n', "\n    ")
    )
}

#[test]
fn a_word_is_one_its_domain_lists() {
    says(
        &query("cache Shared"),
        &["PW0335 `cache Shared`: `Shared` is not one of `shared` or `private`"],
    );
    says(
        &query("consistency snapshott"),
        &["PW0335 `consistency snapshott`: `snapshott` is not one of"],
    );
    clean(&query("cache shared\nconsistency snapshot"));
}

/// The rule that keeps a session's data out of a shared cache reads the
/// word the domain lists. A word it does not was read as no shared cache,
/// and the program checked.
#[test]
fn a_session_query_in_a_shared_cache_is_refused_however_it_is_spelled() {
    says(
        &session_query("freshness 0.seconds\ncache Shared"),
        &["PW0335 `cache Shared`: `Shared` is not one of"],
    );
    // One error, PW5001: the declaration rule that wrote PW0100 for the
    // same clause is retired (ADR-0239). Two detectors reported it, under two
    // numbers, until then.
    says(
        &session_query("freshness 0.seconds\ncache shared"),
        &["PW5001 `Mine` is Session<SessionId> and declares a shared cache"],
    );
}

#[test]
fn a_duration_is_a_count_and_a_unit() {
    says(
        &query("timeout 30.secondz"),
        &["PW0335 `timeout 30.secondz`: `30.secondz` is not a duration"],
    );
    says(&query("timeout thirty"), &["PW0335 `timeout thirty`"]);
    // `freshness` is the duration domain's one head since `timeout` became a
    // budget (ADR-0109), and nothing held a freshness that is no duration:
    // its control survived, found by ADR-0239's run of them.
    let fresh = |value: &str| {
        format!(
            "module t\n\npublic query Box(id: Int) -> Int !{{}}\n    freshness {value}\n{{\n    id\n}}\n"
        )
    };
    says(
        &fresh("30.secondz"),
        &["PW0335 `freshness 30.secondz`: `30.secondz` is not a duration"],
    );
    says(&fresh("thirty"), &["PW0335 `freshness thirty`"]);
    clean(&fresh("30.seconds"));
    clean(&query("timeout 1.hours"));
    clean(&query("timeout 500.milliseconds"));
}

/// A session's data is never served stale, and a duration is read once:
/// `05.seconds` began with `0`, so the rule read it as no staleness.
#[test]
fn a_session_read_is_never_stale_however_it_is_written() {
    says(
        &session_query("freshness 05.seconds"),
        &["PW0102 session query `Mine` declares a 05.seconds staleness window"],
    );
    clean(&session_query("freshness 0.seconds"));
}

#[test]
fn a_world_is_one_a_platform_has() {
    says(
        &query("placement originn"),
        &["PW0335 `placement originn`: `originn` is not a world"],
    );
    says(
        &query("placement browser, edgee"),
        &["PW0335 `placement browser, edgee`: `edgee` is not a world"],
    );
    clean(&query("placement origin"));
    clean(&query("placement browser, edge, origin"));
}

#[test]
fn a_key_names_a_parameter_or_a_partition() {
    says(
        &query("key nope"),
        &["PW0335 `key nope`: `nope` is not a parameter of `Box`"],
    );
    clean(&query("key id"));
    clean(&query("key id, user"));
}

/// A cache key separates a partition by naming it. A parameter whose name
/// contains the partition's, `username`, is not the partition: the key held
/// every user's entry in one place, and the rule found `user` in the text.
#[test]
fn a_key_separates_a_partition_by_naming_it() {
    // A tenant's value, since ADR-0128: a shared cache holds no one reader's
    // value whatever its key, so a key names a partition only for a tenant.
    // The case was the user's, with a key item called `username`.
    let src = |key: &str| {
        format!(
            "module t\n\nimport context\nimport capability.{{ Organization, OrganizationId }}\n\n\
             public query Recent(store: Int, organization_name: String) -> \
             Organization<OrganizationId> !{{ session.read }}\n    \
             freshness 30.seconds\n    cache shared\n    key {key}\n{{\n    \
             context.current_organization()\n}}\n"
        )
    };
    says(&src("store, organization_name"), &["PW5004"]);
    clean(&src("store, organization_name, organization"));
}

#[test]
fn an_operator_is_given_its_arguments() {
    let retry = |v: &str| command(&format!("idempotent_by Tag\nretry {v}"));
    says(
        &retry("nope(max = 3)"),
        &["PW0335 `retry nope(max = 3)`: `nope` is none of `retry`'s operators"],
    );
    // `fixed` left `retry`'s operators with ADR-0215 (ruling 0089-b).
    says(
        &retry("fixed(max = 3)"),
        &["PW0335 `retry fixed(max = 3)`: `fixed` is none of `retry`'s operators"],
    );
    says(
        &retry("transport_only(maxx = 3)"),
        &["PW0335 `retry transport_only(maxx = 3)`: `transport_only` takes no argument `maxx`"],
    );
    says(
        &retry("transport_only(max = \"two\")"),
        &["`transport_only`'s `max` is a count from 1, and this is `\"two\"`"],
    );
    says(
        &retry("transport_only(max = 0)"),
        &["`transport_only`'s `max` is a count from 1, and this is `0`"],
    );
    says(
        &retry("bounded_exponential(jitter = true)"),
        &["`bounded_exponential` is not given `max`"],
    );
    says(
        &retry("transport_only(3)"),
        &["`transport_only` takes its arguments by name, and `3` names none"],
    );
    says(
        &retry("transport_only(max = 3, max = 4)"),
        &["`transport_only` is given `max` twice"],
    );
    clean(&retry("transport_only(max = 3)"));
    clean(&retry("bounded_exponential(max = 3, jitter = true)"));
    clean(&retry("none"));
}

/// Which operator a value applies is the operator's identity, not its
/// spelling's prefix.
#[test]
fn only_transport_only_is_transport_only() {
    says(
        &command("retry transport_onlyish(max = 2)"),
        &[
            "PW0335 `retry transport_onlyish(max = 2)`: `transport_onlyish` is none",
            "PW0312",
        ],
    );
    clean(&command("idempotent_by Tag\nretry transport_only(max = 2)"));
}

/// **A command that is retried is idempotent, whatever its policy**
/// (ADR-0173). A request that failed in transit may have arrived, and its
/// sender cannot tell. Until 2026-10-04 `transport_only` was let through.
#[test]
fn a_retried_command_is_idempotent_whatever_it_retries_on() {
    for retry in [
        "retry transport_only(max = 2, jitter = true)",
        "retry bounded_exponential(max = 3, jitter = true)",
        "retry bounded_exponential(max = 3)",
    ] {
        says(&command(retry), &["PW0312"]);
        clean(&command(&format!("idempotent_by Tag\n{retry}")));
    }
    // `retry none` retries nothing.
    clean(&command("retry none"));
}

#[test]
fn a_type_a_policy_names_is_visible() {
    says(
        &command("idempotent_by Nope"),
        &["PW0335 `idempotent_by Nope`: `Nope` names no type visible here"],
    );
    clean(&command("idempotent_by Tag"));
    // The language's own types are types too.
    clean(&command("idempotent_by Int"));
}
