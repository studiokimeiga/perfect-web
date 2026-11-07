//! **A shared cache holds no one reader's value, whatever its declaration
//! says** (ADR-0128).
//!
//! Charter §7.8's first must-fail example is `SharedCache<Cart@Session>`.
//! Until 2026-10-02 it passed in two shapes:
//! - a cart query declared `public`, or with no visibility at all, given the
//!   session as a parameter and keyed by it. A declaration's own parameters
//!   were read by nothing, and a label a parameter states was a contract that
//!   kept it out of the result (ADR-0085). This was T12's plausible wrong fix;
//! - any value keyed by its reader: PW5004 accepted a session's or a user's
//!   value in a shared cache whose key named them.
//!
//! Each test states the defect in each shape, with controls: the same
//! declaration `cache private`, and a tenant's value keyed by its tenant, the
//! one partition a shared cache may carry by its key.

use pw_core::check::check_sources;

fn sources(src: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/lib",
    ] {
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
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain.pw"),
    ));
    out.push(("t.pw".to_string(), src.to_string()));
    out
}

fn reported(src: &str) -> Vec<String> {
    check_sources(&sources(src))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn codes(src: &str) -> Vec<String> {
    reported(src)
        .into_iter()
        .map(|d| d.split(' ').next().unwrap_or_default().to_string())
        .collect()
}

const IMPORTS: &str = "module t\n\nimport Carts\nimport Menus\nimport context\n\
     import capability.{ Session, SessionId, Organization, OrganizationId }\n\
     import domain.{ Cart, CartError, StoreId, MenuItem, StoreError }\n\n";

/// A query `head` (its keyword and name), cached in `cache`, keyed by `key`.
fn query(head: &str, params: &str, ty: &str, cache: &str, key: &str, body: &str) -> String {
    format!(
        "{head}({params}) -> {ty}\n    freshness 0.seconds\n    cache     {cache}\n    \
         key       {key}\n{{\n    {body}\n}}\n\n"
    )
}

const CART: &str = "Result<Cart, CartError>";
const MENU: &str = "Result<List<MenuItem>, StoreError>";

#[test]
fn a_query_given_the_session_holds_the_sessions_value_whatever_its_keyword() {
    for head in ["public query Cart", "query Cart", "session query Cart"] {
        let src = format!(
            "{IMPORTS}{}",
            query(
                head,
                "session: Session<SessionId>",
                CART,
                "shared",
                "session",
                "Carts.current(session)",
            )
        );
        let found = reported(&src);
        assert!(
            found
                .iter()
                .any(|d| d.starts_with("PW5001") && d.contains("Session<SessionId>")),
            "`{head}` holds a session's cart in a shared cache: {found:?}"
        );
    }

    // Control: the same query, cached privately.
    let src = format!(
        "{IMPORTS}{}",
        query(
            "public query Cart",
            "session: Session<SessionId>",
            CART,
            "private",
            "session",
            "Carts.current(session)",
        )
    );
    assert_eq!(
        codes(&src),
        Vec::<String>::new(),
        "a private cache is its place"
    );
}

#[test]
fn a_key_naming_the_reader_does_not_make_their_value_shareable() {
    // The user's value, keyed by the user.
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Recent",
            "store: StoreId, limit: Int",
            MENU,
            "shared",
            "store, limit, user",
            "Menus.for_user(store, limit, context.current_user())",
        )
    );
    let found = reported(&src);
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5001") && d.contains("User<UserId>")),
        "{found:?}"
    );

    // The session's, keyed by the session, through a helper.
    let src = format!(
        "{IMPORTS}fn mine() -> {CART} !{{ session.read, database.read<Carts> }} {{\n    \
         Carts.current(context.current_session())\n}}\n\n{}",
        query("public query Cart", "", CART, "shared", "session", "mine()")
    );
    let found = reported(&src);
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5001") && d.contains("Session<SessionId>")),
        "{found:?}"
    );
}

#[test]
fn a_tenants_value_may_be_shared_keyed_by_its_tenant() {
    let body = "Menus.for_store_in_org(store, context.current_organization())";

    // Control: keyed by the organization, by name.
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Menu",
            "store: StoreId",
            MENU,
            "shared",
            "store, organization",
            body
        )
    );
    assert_eq!(codes(&src), Vec::<String>::new());

    // Control: keyed by a parameter that is the tenant.
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Menu",
            "store: StoreId, org: Organization<OrganizationId>",
            MENU,
            "shared",
            "store, org",
            "Menus.for_store_in_org(store, org)",
        )
    );
    assert_eq!(codes(&src), Vec::<String>::new());

    // The defect: the tenant left out of the key is PW5004's, not PW5001's.
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Menu",
            "store: StoreId",
            MENU,
            "shared",
            "store",
            body
        )
    );
    assert_eq!(codes(&src), ["PW5004"]);
}

/// A fragment shared at the edge, depending on `Summary(session)`.
fn strip(partition: &str) -> String {
    format!(
        "materialize Strip(session: Session<SessionId>) {{\n    placement      edge\n    \
         partition      {partition}\n    depends_on     Summary(session)\n    \
         invalidates_on CartChanged(session)\n    regenerate     on_invalidation\n    \
         stampede       single_flight\n    fallback       last_known_good\n}}\n"
    )
}

#[test]
fn a_shared_fragment_of_a_query_given_the_session_is_refused() {
    // No cache clause: neither the query's own cache nor a private one says
    // whose its value is. Only what it is given does.
    let summary = "public query Summary(session: Session<SessionId>) -> Result<Cart, CartError>\n    \
                   freshness 0.seconds\n{\n    Carts.current(session)\n}\n\n";
    let src = format!(
        "{IMPORTS}import Events.{{ CartChanged }}\n\n{summary}{}",
        strip("public")
    );
    assert_eq!(codes(&src), ["PW5101"], "{:?}", reported(&src));

    // Control: the same fragment, partitioned privately.
    let src = format!(
        "{IMPORTS}import Events.{{ CartChanged }}\n\n{summary}{}",
        strip("private")
    );
    assert_eq!(codes(&src), Vec::<String>::new(), "{:?}", reported(&src));
}

/// **The note on the parameter a value comes from is a whole sentence.** A
/// review of pleris.dev on 2026-11-06 found it stopping mid-way, as it had
/// since ADR-0128 wrote it: "`session` is Session<SessionId>, so what `Cart`
/// makes from it is".
#[test]
fn the_parameter_a_readers_value_comes_from_is_noted_in_a_whole_sentence() {
    let src = format!(
        "{IMPORTS}{}",
        query(
            "public query Cart",
            "session: Session<SessionId>",
            CART,
            "shared",
            "session",
            "Carts.current(session)",
        )
    );
    let notes: Vec<String> = check_sources(&sources(&src))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.code == "PW5001")
        .flat_map(|d| d.related.into_iter().map(|r| r.label))
        .collect();
    assert_eq!(
        notes,
        ["`session` is Session<SessionId>, so what `Cart` makes from it is too"]
    );
}

/// **What is said of the parameter is its own label.** A query given a
/// session and an organization holds both, and until 2026-11-06 the note and
/// the cause chain said its `session` was both. The control: the message
/// says all the query holds.
#[test]
fn the_parameter_is_noted_with_its_own_label_where_more_are_given() {
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Cart",
            "session: Session<SessionId>, org: Organization<OrganizationId>",
            CART,
            "shared",
            "session, org",
            "Carts.current(session)",
        )
    );
    let found: Vec<_> = check_sources(&sources(&src))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.code == "PW5001")
        .collect();
    assert_eq!(found.len(), 1, "{found:#?}");
    let d = &found[0];
    let notes: Vec<&str> = d.related.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        notes,
        ["`session` is Session<SessionId>, so what `Cart` makes from it is too"]
    );
    let why = d.explanation.as_deref().unwrap_or_default();
    assert!(
        why.contains(
            "its parameter `session`, which is labeled `Session<SessionId>`, and \
             `Session<SessionId>` does not flow into `Public`"
        ),
        "{why}"
    );
    assert_eq!(
        d.message,
        "`Cart` is Session<SessionId> ⊔ Organization<OrganizationId> and declares a shared cache"
    );
}

#[test]
fn a_readers_value_in_a_tenants_cache_is_one_defect() {
    // Given the session and reading the tenant, keyed by neither. The session
    // makes it unshareable whatever the key, so which partitions the key
    // lacks is not asked as well (ADR-0112: one defect, one diagnostic).
    let src = format!(
        "{IMPORTS}{}",
        query(
            "query Menu",
            "store: StoreId, session: Session<SessionId>",
            MENU,
            "shared",
            "store",
            "Menus.for_store_in_org(store, context.current_organization())",
        )
    );
    assert_eq!(codes(&src), ["PW5001"], "{:?}", reported(&src));
}
