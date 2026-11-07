//! **A session's, a user's or an organization's handle is the platform's to
//! make** (ADR-0263). A handle reads the data of the one it names, so a
//! program that could make one could read anyone's, and until ADR-0263 one
//! could: `Session("…")` checked clean, and so did a command whose session
//! the browser supplies. Each way one was made is refused where it is
//! written: constructed (PW5037), answered by an operation that is not the
//! platform's (PW5038), and supplied by the browser (PW5039). An id is the
//! program's to make, and grants nothing. Each test states one case, with
//! its control.

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
        "module app\n\nimport capability.{{ Session, SessionId, User, UserId, \
         Organization, OrganizationId, Secret, Payments }}\nimport context.{{ \
         current_session, current_user }}\n\ntype Cart = Cart {{ items: Int }}\n\n{body}"
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

#[test]
fn a_program_constructs_no_handle() {
    for (made, at) in [
        ("Session<SessionId>", "Session(\"someone-elses\")"),
        ("User<UserId>", "User(\"someone-else\")"),
        ("Organization<OrganizationId>", "Organization(\"another\")"),
    ] {
        let got = reported(&format!("fn forged() -> {made} !{{}} {{\n    {at}\n}}\n"));
        assert_eq!(got, vec![("PW5037".to_string(), at.to_string())], "{made}");
    }
    // Inside another value, and in a page's view, as anywhere.
    let got = codes("fn forged() -> Option<User<UserId>> !{} {\n    Some(User(\"x\"))\n}\n");
    assert_eq!(got, ["PW5037"]);
    // The controls: an id names someone, and is the program's to make; a
    // secret made from a value restricts it, and grants nothing; and the
    // reader's own, from the platform.
    assert!(codes("fn named() -> UserId !{} {\n    UserId(\"someone\")\n}\n").is_empty());
    assert!(
        codes("fn kept(token: String) -> Secret<Payments> !{} {\n    Secret(token)\n}\n")
            .is_empty()
    );
    assert!(
        codes("fn me() -> User<UserId> !{ session.read } {\n    current_user()\n}\n").is_empty()
    );
}

#[test]
fn a_data_layer_answers_ids_never_handles() {
    for answer in [
        "User<UserId>",
        "Option<User<UserId>>",
        "List<Session<SessionId>>",
        "Who",
    ] {
        let got = codes(&format!(
            "type Who = Who {{ user: User<UserId>, name: String }}\n\n\
             fn who(s: Session<SessionId>) -> {answer} !{{ database.read<Cart> }}\n    \
             host \"app:data/users#who\"\n"
        ));
        assert_eq!(got, ["PW5038"], "{answer}");
    }
    // The controls: a data layer's id, given the reader's session; and the
    // platform's own operation, which answers the reader's handle.
    assert!(
        codes(
            "fn author(s: Session<SessionId>) -> UserId !{ database.read<Cart> }\n    \
             host \"app:data/users#of-session\"\n"
        )
        .is_empty()
    );
    assert!(
        codes(
            "fn reader() -> User<UserId> !{ session.read }\n    host \"pw:host/principal#read\"\n"
        )
        .is_empty()
    );
}

#[test]
fn nothing_the_browser_supplies_holds_a_handle() {
    let cart = "fn cart_of(s: Session<SessionId>) -> Cart !{ database.read<Cart> }\n    \
                host \"app:data/carts#of\"\n\n";
    // A command's parameter: the browser writes it.
    let got = reported(&format!(
        "{cart}command peek(s: Session<SessionId>) -> Cart {{\n    cart_of(s)\n}}\n"
    ));
    assert_eq!(
        got,
        vec![("PW5039".to_string(), "s: Session<SessionId>".to_string())]
    );
    assert_eq!(
        codes(&format!(
            "{cart}command peek(u: Option<User<UserId>>) -> Cart {{\n    \
             cart_of(current_session())\n}}\n"
        )),
        ["PW5039"]
    );
    // A page's parameter: its address carries it.
    assert_eq!(
        codes(
            "page Theirs(org: Organization<OrganizationId>) {\n    view {\n        \
             <main><p>Theirs</p></main>\n    }\n}\n"
        ),
        ["PW5039"]
    );
    // A signal, a module's or a view's: the browser's state.
    assert_eq!(codes("signal who: User<UserId>\n"), ["PW5039"]);
    assert_eq!(
        codes(
            "view Mine() !{ session.read } {\n    signal mine: Option<Session<SessionId>> = None\n\n    \
             <p>Mine</p>\n}\n"
        ),
        ["PW5039"]
    );
    // The controls: a command given an id, the reader's own read inside it,
    // and a query keyed by the reader's session, which a page calls with
    // `current_session()`.
    assert!(
        codes(&format!(
            "{cart}command mine(user: UserId) -> Cart {{\n    cart_of(current_session())\n}}\n"
        ))
        .is_empty()
    );
    assert!(
        codes(&format!(
            "{cart}session query Mine(session: Session<SessionId>) -> Cart\n    \
             freshness     0.seconds\n    consistency   read_your_writes\n    \
             cache         private\n    key           session\n{{\n    cart_of(session)\n}}\n"
        ))
        .is_empty()
    );
}

/// What a program reports with the platform, its own imports written: each
/// diagnostic's code.
fn codes_of(src: &str) -> Vec<String> {
    let mut program = platform();
    program.push(("app.pw".to_string(), src.to_string()));
    check_sources(&program)
        .into_iter()
        .filter(|(n, _)| n == "app.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| d.code.to_string()))
        .collect()
}

/// **The reader's account is the reader's** (track `data-path`):
/// `context.account_of` hands a program the id its reader's handle names, an
/// id that grants nothing, and the name their provider gave; which account
/// it is says who reads, so the answer is labelled as the handle it was read
/// from, and a shared cache refuses it as it refuses the handle (PW5001). A
/// private cache, and a query of the reader, take it.
#[test]
fn the_readers_account_is_the_readers() {
    let program = |answer: &str, body: &str, cache: &str| {
        format!(
            "module app\n\nimport capability.{{ User, UserId }}\nimport context.{{ \
             current_user, account_of, Account }}\n\nquery Who() -> {answer}\n    \
             consistency strong\n    cache       {cache}\n{{\n    {body}\n}}\n"
        )
    };
    for (answer, body) in [
        ("Option<Account>", "account_of(current_user())"),
        (
            "Option<UserId>",
            "match account_of(current_user()) {\n        Some(account) => Some(account.id),\n        \
             None => None,\n    }",
        ),
    ] {
        assert_eq!(
            codes_of(&program(answer, body, "shared")),
            ["PW5001"],
            "{body}, in a shared cache"
        );
        // The control: the same read, cached per reader.
        assert!(
            codes_of(&program(answer, body, "private")).is_empty(),
            "{body}, in a private cache"
        );
    }
    // And a user's query keyed by the reader is given the reader's handle,
    // whose account's id it compares with a row's, as the guestbook's
    // `Latest` does.
    assert!(
        codes_of(
            "module app\n\nimport capability.{ User, UserId }\nimport context.{ account_of }\n\n\
             user query Mine(reader: User<UserId>, author: UserId) -> Bool\n    \
             freshness     0.seconds\n    consistency   read_your_writes\n    \
             cache         private\n    key           reader, author\n{\n    \
             match account_of(reader) {\n        Some(account) => account.id == author,\n        \
             None => false,\n    }\n}\n"
        )
        .is_empty()
    );
}

/// What a table's module reports with the platform: each diagnostic's code,
/// and the text its primary span covers.
fn table_reported(src: &str) -> Vec<(String, String)> {
    let mut program = platform();
    program.push(("Holds.pw".to_string(), src.to_string()));
    check_sources(&program)
        .into_iter()
        .filter(|(n, _)| n == "Holds.pw")
        .flat_map(|(_, ds)| {
            ds.into_iter().map(|d| {
                let at = src[d.primary_span.start..d.primary_span.end].to_string();
                (d.code.to_string(), at)
            })
        })
        .collect()
}

/// **A table's row holds no handle** (PW6901, track `data-path`, the
/// integrator's ruling 3 of 2026-11-02): a row is read by every reader of
/// its table, and the table keeps no label, so a handle stored in one was
/// served to every other session (the review stored `current_user()`, and a
/// shared query answered another session `u-` and a guest's session). A
/// handle of each kind, wherever the field's type holds it, is refused at
/// the row; an id, which grants nothing, is not, and neither is a handle in
/// a record no table stores.
#[test]
fn a_tables_row_holds_no_handle() {
    let table = |fields: &str, beside: &str| {
        format!(
            "module Holds\n\nimport capability.{{ Session, SessionId, User, UserId, \
             Organization, OrganizationId }}\n\n{beside}type Held = Held {{ id: String, \
             {fields} }}\n\ntable Holds\n    row Held\n    key id\n"
        )
    };
    let refused = |fields: &str, beside: &str| {
        let got = table_reported(&table(fields, beside));
        assert_eq!(
            got,
            vec![("PW6901".to_string(), "row Held".to_string())],
            "{fields}"
        );
    };
    refused("who: User<UserId>", "");
    refused("who: Session<SessionId>", "");
    refused("who: Organization<OrganizationId>", "");
    refused("who: Option<capability.User<UserId>>", "");
    refused("who: List<User<UserId>>", "");
    refused(
        "who: Inner",
        "type Inner = Inner { at: Int, h: User<UserId> }\n\n",
    );
    refused(
        "who: Holder",
        "type Holder =\n    | Nobody\n    | Someone(Session<SessionId>)\n\n",
    );
    // Two fields, two refusals: each named.
    let two = table_reported(&table("who: User<UserId>, at: Session<SessionId>", ""));
    assert_eq!(two.len(), 2, "{two:?}");
    assert!(two.iter().all(|(code, _)| code == "PW6901"), "{two:?}");
    // The controls: an id names someone, and grants nothing; a handle in a
    // record no table stores is no row's.
    for fields in ["who: UserId", "who: Option<UserId>", "who: List<UserId>"] {
        assert!(table_reported(&table(fields, "")).is_empty(), "{fields}");
    }
    assert!(
        table_reported(&table(
            "who: UserId",
            "type Note = Note { who: User<UserId> }\n\n"
        ))
        .is_empty()
    );
}

#[test]
fn a_local_callback_is_not_a_handle_constructor() {
    for name in ["Session", "User", "Organization"] {
        let body = format!(
            "fn applied({name}: fn(String) -> String) -> String !{{}} {{ {name}(\"text\") }}\n"
        );
        assert_eq!(reported(&body), Vec::<(String, String)>::new(), "{body}");
    }
}
