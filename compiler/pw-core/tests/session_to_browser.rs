//! **A session's handle never reaches the browser** (ADR-0264). A session's
//! id is its cookie's value, and the cookie is `HttpOnly`, kept from the
//! page's scripts; a page that printed the handle gave it back to them, and
//! until ADR-0264 `<p>{mine.session}</p>` checked clean. What reaches the
//! browser is refused where it holds one (PW5040): a query's, a
//! subscription's or a command's answer, and what markup prints. A user's
//! id is a name, and may be printed. Each test states one case, with its
//! control.

use pw_core::check::check_sources;

fn platform() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|e| panic!("{dir}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            let src = std::fs::read_to_string(&p).expect("read");
            out.push((p.display().to_string(), src));
        }
    }
    out
}

/// What `app.pw` reports with the platform: each diagnostic's code, and the
/// text its primary span covers.
fn reported(body: &str) -> Vec<(String, String)> {
    let src = format!(
        "module app\n\nimport capability.{{ Session, SessionId, User, UserId }}\n\
         import context.{{ current_session, current_user }}\n\n\
         type Cart = Cart {{ items: Int }}\n\
         type Seen = Seen {{ session: Session<SessionId>, items: Int }}\n\n\
         fn cart_of(s: Session<SessionId>) -> Cart !{{ database.read<Cart> }}\n    \
         host \"app:data/carts#of\"\n\n{body}"
    );
    let mut program = platform();
    program.push(("app.pw".to_string(), src.clone()));
    check_sources(&program)
        .into_iter()
        .filter(|(n, _)| n == "app.pw")
        .flat_map(|(_, ds)| {
            ds.into_iter().map(|d| {
                let at = src[d.primary_span.start..d.primary_span.end].to_string();
                (d.code.to_string(), at)
            })
        })
        .collect()
}

fn codes(body: &str) -> Vec<String> {
    reported(body).into_iter().map(|(c, _)| c).collect()
}

/// A session query `name` answering `answer` from `body`.
fn query(name: &str, answer: &str, body: &str) -> String {
    format!(
        "session query {name}(session: Session<SessionId>) -> {answer}\n    \
         freshness     0.seconds\n    consistency   read_your_writes\n    \
         cache         private\n    key           session\n{{\n    {body}\n}}\n\n"
    )
}

#[test]
fn no_answer_holds_a_sessions_handle() {
    for (answer, body) in [
        ("Session<SessionId>", "session"),
        ("Seen", "Seen { session: session, items: 1 }"),
        ("Option<Session<SessionId>>", "Some(session)"),
    ] {
        let got = codes(&query("Mine", answer, body));
        assert_eq!(got, ["PW5040"], "{answer}");
    }
    assert_eq!(
        codes(
            "subscription Watched(session: Session<SessionId>) -> Session<SessionId>\n    \
             key           session\n{\n    session\n}\n"
        )
        .iter()
        .filter(|c| *c == "PW5040")
        .count(),
        1
    );
    assert_eq!(
        codes(
            "command which() -> Session<SessionId>\n    requires Anyone\n{\n    current_session()\n}\n"
        ),
        ["PW5040"]
    );
    // The controls: the session's data answered, and a user's handle,
    // which names a user and opens nothing.
    assert!(codes(&query("Mine", "Cart", "cart_of(session)")).is_empty());
    assert!(
        codes("command who() -> User<UserId> !{ session.read }\n    requires Anyone\n{\n    current_user()\n}\n")
            .is_empty()
    );
}

/// A page printing `markup`, reading the session's cart.
fn page(markup: &str) -> String {
    query("Mine", "Cart", "cart_of(session)")
        + &format!(
            "page Shown() {{\n    let mine = query Mine(current_session())\n    \
             let session = current_session()\n\n    view {{\n        <main>{markup}</main>\n    }}\n}}\n"
        )
}

#[test]
fn nothing_markup_prints_holds_a_sessions_handle() {
    for (markup, at) in [
        ("<p>{session}</p>", "session"),
        ("<p title={session}>Seen</p>", "session"),
        ("<a href=\"/seen/{session}\">Seen</a>", "session"),
    ] {
        let got = reported(&page(markup));
        assert_eq!(
            got,
            vec![("PW5040".to_string(), at.to_string())],
            "{markup}"
        );
    }
    // The controls: the session's data printed, and a user's id.
    assert!(codes(&page("<p>{mine.items}</p>")).is_empty());
    assert!(
        codes(
            "page Named() {\n    let me = current_user()\n\n    view {\n        \
             <main><p>{me}</p></main>\n    }\n}\n"
        )
        .is_empty()
    );
}

#[test]
fn a_views_prop_holding_a_sessions_handle_is_refused_where_it_is_given() {
    let view = "view Card(session: Session<SessionId>) !{} {\n    <p>A card</p>\n}\n\n";
    let got = reported(&format!(
        "{view}page Carded() {{\n    let session = current_session()\n\n    view {{\n        \
         <main><Card session={{session}} /></main>\n    }}\n}}\n"
    ));
    assert_eq!(got, vec![("PW5040".to_string(), "session".to_string())]);
}
