//! **Accounts and sign-in, served** (track `identity`, ADR-0258): the
//! relying party's flow over HTTP against a provider of the test's own, the
//! session it rotates and forgets, `requires SignedIn` and `OwnsPost(post)`
//! held on the feed, a command from another origin refused, and two users'
//! pages sharing nothing private.
//!
//! The provider here is the test's: it plays the deployment's OpenID
//! provider, so these tests hold Pleris's half of the flow and nothing of
//! the development provider's (`accounts.rs`, tested there).
//!
//! Track `store-accounts` signs the store's readers in through the same
//! provider and flow (`tests/store_accounts.rs`): what it needs of them is
//! `pub(super)`.

use super::*;
use crate::identity::{Claims, Principal, Provider, parse_form, s256};
use std::collections::HashMap;

/// **A deployment's provider, as a test plays it**: it authenticates whom
/// the test says, issues a code bound to the request's challenge and nonce,
/// and exchanges it once for that verifier, as RFC 7636 §4.6 asks.
#[derive(Default)]
pub(super) struct TestProvider {
    /// By code: the subject, their name, the challenge and the nonce.
    codes: Mutex<HashMap<String, Issued>>,
    /// What the next token says, where a test makes it wrong.
    forge: Mutex<Option<fn(&mut Claims)>>,
}

/// What a code was issued for: subject, name, challenge, nonce.
type Issued = (String, String, String, Option<String>);

/// A change that makes a token wrong, and what its refusal says.
type Forgery = (fn(&mut Claims), &'static str);

const ISSUER: &str = "https://id.example.test";
const CLIENT: &str = "feed-client";
pub(super) const REDIRECT: &str = "http://127.0.0.1/sign-in/callback";

impl TestProvider {
    /// **The browser at the provider, signed in as `sub`**: the
    /// authentication request in `location` checked as a provider checks
    /// it, and the redirect back's query, with a code and the request's
    /// state.
    pub(super) fn authenticate(&self, location: &str, sub: &str, name: &str) -> String {
        let (endpoint, query) = location.split_once('?').expect("a query");
        assert_eq!(endpoint, "https://id.example.test/authorize");
        let q = parse_form(query);
        assert_eq!(q["response_type"], "code");
        assert!(q["scope"].split(' ').any(|s| s == "openid"), "{q:?}");
        assert_eq!(q["client_id"], CLIENT);
        assert_eq!(q["redirect_uri"], REDIRECT);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"].len(), 43, "{q:?}");
        let code = format!("code-{sub}-{}", q["state"].len());
        self.codes.lock().expect("codes").insert(
            code.clone(),
            (
                sub.to_string(),
                name.to_string(),
                q["code_challenge"].clone(),
                q.get("nonce").cloned(),
            ),
        );
        format!("code={code}&state={}", crate::identity::encode(&q["state"]))
    }
}

impl Provider for TestProvider {
    fn issuer(&self) -> String {
        ISSUER.to_string()
    }

    fn client_id(&self) -> String {
        CLIENT.to_string()
    }

    fn authorization_endpoint(&self) -> String {
        format!("{ISSUER}/authorize")
    }

    fn exchange(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<Claims, String> {
        let (sub, name, challenge, nonce) = self
            .codes
            .lock()
            .expect("codes")
            .remove(code)
            .ok_or("invalid_grant: unknown code")?;
        if redirect_uri != REDIRECT {
            return Err("invalid_grant: redirect_uri".to_string());
        }
        if s256(verifier) != challenge {
            return Err("invalid_grant: code_verifier".to_string());
        }
        let mut claims = Claims {
            iss: ISSUER.to_string(),
            sub: sub.clone(),
            aud: vec![CLIENT.to_string()],
            exp: u64::MAX / 2,
            nonce,
            preferred_username: Some(sub),
            name: Some(name),
        };
        if let Some(forge) = *self.forge.lock().expect("forge") {
            forge(&mut claims);
        }
        Ok(claims)
    }
}

/// One HTTP request to `s`, written as given, and the whole answer.
pub(super) fn exchanged(s: &Server, request: &str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client.write_all(request.as_bytes()).expect("request");
        let mut answer = String::new();
        let _ = client.read_to_string(&mut answer);
        answer
    })
}

/// A page's text, its parts' markers taken out.
pub(super) fn text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.find("<!--pw:") {
        out.push_str(&rest[..at]);
        rest = rest[at..].split_once("-->").map_or("", |(_, r)| r);
    }
    out.push_str(rest);
    out
}

/// A page served to `session`, as text.
fn page(s: &Server, session: &str, name: &str, params: &Params) -> String {
    let (html, _, _, _) = s
        .serve_document_settled(session, name, params, &[])
        .expect("served");
    text(&html)
}

/// The home page served to `session`, as text.
fn home(s: &Server, session: &str) -> String {
    page(s, session, "feed.app.Home~ui", &Params::new())
}

/// The thread page's parameters for the post `id`.
fn thread_of(id: &str) -> Params {
    Params::from([("id".to_string(), id.to_string())])
}

/// A GET with these cookies.
pub(super) fn get(s: &Server, path: &str, cookies: &str) -> String {
    exchanged(
        s,
        &format!("GET {path} HTTP/1.1\r\nHost: t\r\nCookie: {cookies}\r\n\r\n"),
    )
}

/// A POST of a command, from the page's own origin as a browser says it.
pub(super) fn command(
    s: &Server,
    id: &str,
    session: &str,
    interaction: &str,
    body: &str,
) -> String {
    exchanged(
        s,
        &format!(
            "POST /command/{id} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\npw-interaction: {interaction}\r\n\
             content-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// The value of the cookie `name` an answer sets, and the whole line.
pub(super) fn set_cookie(answer: &str, name: &str) -> Option<(String, String)> {
    answer.lines().find_map(|l| {
        let rest = l.strip_prefix("set-cookie: ")?;
        let value = rest.strip_prefix(&format!("{name}="))?;
        Some((value.split(';').next()?.to_string(), l.to_string()))
    })
}

pub(super) fn location(answer: &str) -> String {
    answer
        .lines()
        .find_map(|l| l.strip_prefix("location: "))
        .unwrap_or_else(|| panic!("no location: {answer}"))
        .to_string()
}

/// **The feed, its sessions signed in through `provider`**.
fn feed_with(provider: Arc<TestProvider>) -> Served {
    let s = served_feed();
    s.identity.use_provider(provider, REDIRECT);
    s
}

/// **A browser signed in as `sub` through the flow**, from the session it
/// came with: the session it is given.
pub(super) fn signed_in(s: &Server, provider: &TestProvider, came_with: &str, sub: &str) -> String {
    let started = get(s, "/sign-in", &format!("pw-session={came_with}"));
    let (state, _) = set_cookie(&started, "pw-sign-in").expect("the sign-in's cookie");
    let back = provider.authenticate(&location(&started), sub, &format!("Name of {sub}"));
    let answer = get(
        s,
        &format!("/sign-in/callback?{back}"),
        &format!("pw-session={came_with}; pw-sign-in={state}"),
    );
    assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
    set_cookie(&answer, "pw-session").expect("a session").0
}

/// **A sign-in rotates the session, and the one the browser came with is no
/// one** (OpenID Connect Core §3.1.2.1, §3.1.3.7; OWASP's session fixation
/// defence). The request sent to the provider carries `openid`, the client,
/// its redirect URI, a state, a nonce and an S256 challenge; the state is
/// also in an HttpOnly cookie on `/sign-in`; and the callback gives a new
/// session, signed in, and removes the state's cookie.
#[test]
fn a_sign_in_rotates_the_session_and_the_old_one_is_no_one() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let came_with = "s-fixed-by-someone-else";
    let host = BTreeMap::new();
    assert_eq!(
        s.identity.requires("SignedIn", &[], came_with, &host),
        Ok(false)
    );

    let started = get(&s, "/sign-in", &format!("pw-session={came_with}"));
    assert!(started.starts_with("HTTP/1.1 303"), "{started}");
    let (state, line) = set_cookie(&started, "pw-sign-in").expect("the state's cookie");
    assert!(
        line.contains("HttpOnly") && line.contains("SameSite=Lax"),
        "{line}"
    );
    assert!(line.contains("Path=/sign-in"), "{line}");
    let to = location(&started);
    assert!(to.contains(&format!("state={state}")), "{to}");
    assert!(to.contains("nonce="), "{to}");
    let back = provider.authenticate(&to, "ada", "Ada Lovelace");
    let answer = get(
        &s,
        &format!("/sign-in/callback?{back}"),
        &format!("pw-session={came_with}; pw-sign-in={state}"),
    );
    assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
    assert_eq!(location(&answer), "/");
    let (rotated, line) = set_cookie(&answer, "pw-session").expect("a new session");
    assert!(
        line.contains("HttpOnly") && line.contains("SameSite=Lax"),
        "{line}"
    );
    assert_ne!(rotated, came_with);
    assert_eq!(rotated.len(), 34, "128 random bits: {rotated}");
    let (_, removed) = set_cookie(&answer, "pw-sign-in").expect("the state's cookie removed");
    assert!(removed.contains("Max-Age=0"), "{removed}");

    assert_eq!(
        s.identity.requires("SignedIn", &[], &rotated, &host),
        Ok(true)
    );
    assert_eq!(
        s.identity.requires("SignedIn", &[], came_with, &host),
        Ok(false)
    );
    let principal = s.identity.principals().of(&rotated).expect("a principal");
    assert_eq!(
        (
            principal.user.as_str(),
            principal.name.as_str(),
            principal.issuer.as_str()
        ),
        ("ada", "Ada Lovelace", ISSUER)
    );
    // The callback is single use: the same answer again completes nothing.
    let again = get(
        &s,
        &format!("/sign-in/callback?{back}"),
        &format!("pw-session={came_with}; pw-sign-in={state}"),
    );
    assert!(again.starts_with("HTTP/1.1 400"), "{again}");
    assert!(set_cookie(&again, "pw-session").is_none(), "{again}");
    // Signing in again from a signed-in session forgets that one too.
    let newer = signed_in(&s, &provider, &rotated, "grace");
    assert_eq!(
        s.identity.requires("SignedIn", &[], &rotated, &host),
        Ok(false)
    );
    assert_eq!(
        s.identity.requires("SignedIn", &[], &newer, &host),
        Ok(true)
    );
}

/// **A callback this browser did not start is refused** (login CSRF): an
/// attacker's own code and state, sent to someone else's browser, which
/// holds no state cookie or another one, signs no one in.
#[test]
fn a_callback_another_browser_started_is_refused() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    // The attacker's sign-in, to the provider and back, in the attacker's
    // browser, stopped before its callback.
    let started = get(&s, "/sign-in", "pw-session=s-attacker");
    let back = provider.authenticate(&location(&started), "mallory", "Mallory");
    for cookies in [
        "pw-session=s-victim",
        "pw-session=s-victim; pw-sign-in=other",
    ] {
        let answer = get(&s, &format!("/sign-in/callback?{back}"), cookies);
        assert!(answer.starts_with("HTTP/1.1 400"), "{answer}");
        assert!(answer.contains("not started by this browser"), "{answer}");
        assert!(set_cookie(&answer, "pw-session").is_none(), "{answer}");
    }
    assert_eq!(s.identity.principals().signed_in(), 0);
}

/// **An ID token is held to what was sent** (Core §3.1.3.7): its issuer
/// exactly, this client in its audience, not expired, and this sign-in's
/// nonce. Each forged, the sign-in is refused and no session opened.
#[test]
fn a_token_not_this_sign_ins_is_refused() {
    let forgeries: [Forgery; 4] = [
        (|c| c.iss = "https://evil.example.test".into(), "issuer"),
        (
            |c| c.aud = vec!["another-client".into()],
            "not for this client",
        ),
        (|c| c.exp = 1, "expired"),
        (|c| c.nonce = Some("replayed".into()), "nonce"),
    ];
    for (forge, why) in forgeries {
        let provider = Arc::new(TestProvider::default());
        *provider.forge.lock().expect("forge") = Some(forge);
        let s = feed_with(provider.clone());
        let started = get(&s, "/sign-in", "pw-session=s-a");
        let (state, _) = set_cookie(&started, "pw-sign-in").expect("state");
        let back = provider.authenticate(&location(&started), "ada", "Ada");
        let answer = get(
            &s,
            &format!("/sign-in/callback?{back}"),
            &format!("pw-session=s-a; pw-sign-in={state}"),
        );
        assert!(answer.starts_with("HTTP/1.1 400"), "{why}: {answer}");
        assert!(answer.contains(why), "{why}: {answer}");
        assert_eq!(s.identity.principals().signed_in(), 0, "{why}");
    }
}

/// **Signing out forgets the principal and rotates the session**: the old
/// id, replayed, is no one, and the browser is given a new one.
#[test]
fn signing_out_forgets_the_principal() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let session = signed_in(&s, &provider, "s-a", "ada");
    let host = BTreeMap::new();
    assert_eq!(
        s.identity.requires("SignedIn", &[], &session, &host),
        Ok(true)
    );
    let answer = exchanged(
        &s,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\ncontent-length: 0\r\n\r\n"
        ),
    );
    assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
    let (fresh, _) = set_cookie(&answer, "pw-session").expect("a new session");
    assert_ne!(fresh, session);
    assert_eq!(
        s.identity.requires("SignedIn", &[], &session, &host),
        Ok(false)
    );
    assert_eq!(
        s.identity.requires("SignedIn", &[], &fresh, &host),
        Ok(false)
    );
}

/// **A control is shown where its command's predicate holds for the reader**
/// (ADR-0307): the feed's pages ask `SignedIn` of their reader, and the
/// deployment answers it as it answers `requires`. Signed out, a post is
/// shown without its Like and the composer is hidden; signed in, both are
/// there; and the document carries the answer to the browser, which renders
/// a speculated region with it. Until ADR-0307 the composer's guard was the
/// program's own `me.signed_in`, and Like was shown to every reader.
#[test]
fn a_control_is_shown_where_its_commands_predicate_holds() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let session = signed_in(&s, &provider, "s-a", "ada");
    let posted = command(&s, "feed.app.post", &session, "i-1", "[\"Shown to both\"]");
    assert!(posted.starts_with("HTTP/1.1 2"), "{posted}");
    let served = |session: &str| {
        let answer = get(&s, "/", &format!("pw-session={session}"));
        answer
            .split_once("\r\n\r\n")
            .map_or(String::new(), |(_, b)| b.to_string())
    };
    let (signed, out) = (served(&session), served("s-out"));
    for html in [&signed, &out] {
        assert!(html.contains("Shown to both"), "{html}");
    }
    assert!(signed.contains(">Like</button>"), "{signed}");
    assert!(!out.contains(">Like</button>"), "{out}");
    // The composer, by its textarea's draft: hidden from the reader the
    // deployment does not hold signed in.
    let composer = |html: &str| {
        let at = html
            .find("bind:value")
            .or_else(|| html.find("<textarea"))
            .unwrap_or(0);
        html[..at]
            .rfind("<form")
            .map(|f| html[f..at].to_string())
            .unwrap_or_default()
    };
    assert!(composer(&out).contains(" hidden"), "{}", composer(&out));
    assert!(
        !composer(&signed).contains(" hidden"),
        "{}",
        composer(&signed)
    );
    // The answer the document was rendered with, which the browser renders a
    // speculated region with.
    let manifest = |html: &str| -> serde_json::Value {
        let start = html
            .find("id=\"pw-parts\"")
            .and_then(|i| html[i..].find('>').map(|j| i + j + 1));
        let start = start.expect("a parts manifest");
        let end = start + html[start..].find("</script>").expect("its end");
        serde_json::from_str(&html[start..end]).expect("the manifest is JSON")
    };
    assert_eq!(
        manifest(&signed)["holds"],
        serde_json::json!({ "SignedIn~holds": true })
    );
    assert_eq!(
        manifest(&out)["holds"],
        serde_json::json!({ "SignedIn~holds": false })
    );
}

/// **What a page asked of its reader is the document's for its life**
/// (ADR-0307): its values, read again after its reader signed out in another
/// tab, answer as they did when it was first read, as the data it shows was
/// read for that reader; a document served since is answered anew. Until
/// 2026-10-11 each read answered again, as `requires` would then, and a page
/// that changed after a sign-out elsewhere lost the controls of the regions
/// that changed and kept the rest (the identity spec's two stale-tab tests,
/// failing in turn in Firefox).
#[test]
fn a_documents_answers_are_its_own_for_its_life() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let session = signed_in(&s, &provider, "s-a", "ada");
    let html = get(&s, "/", &format!("pw-session={session}"));
    assert!(html.contains(">Like</button>"), "{html}");
    let doc = latest(&s.pending.lock().expect("pending"), &session);
    let asked = |document: u64| {
        s.bindings(&session, document).expect("read")[&asked_name("SignedIn")].clone()
    };
    assert_eq!(asked(doc.1), Val::Bool(true));
    let answer = exchanged(
        &s,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\ncontent-length: 0\r\n\r\n"
        ),
    );
    assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
    // Read again, as a change it shows reads it: as it was first answered.
    assert_eq!(asked(doc.1), Val::Bool(true));
    // A document served since is answered anew.
    let (fresh, _) = set_cookie(&answer, "pw-session").expect("a new session");
    let out = get(&s, "/", &format!("pw-session={fresh}"));
    assert!(!out.contains(">Like</button>"), "{out}");
}

/// The host reads an answer by the name the compiler gives it (ADR-0307).
#[test]
fn an_answer_is_read_by_the_compilers_name_for_it() {
    assert_eq!(
        asked_name("SignedIn"),
        pw_core::template_ir::asked_name("SignedIn")
    );
}

/// **A signed-out reader reads, and cannot post** (ADR-0115): the home page
/// shows the timeline and, where the form's guard is, "Sign in to post"
/// with a way to; a post sent anyway is refused by `requires SignedIn`
/// before the command runs, answered 403 with the predicate, and nothing is
/// written.
#[test]
fn a_signed_out_reader_reads_and_cannot_post() {
    let s = feed_with(Arc::new(TestProvider::default()));
    let html = home(&s, "s-reader");
    assert!(html.contains("Hello, feed."), "{html}");
    assert!(html.contains("Sign in to post"), "{html}");
    assert!(html.contains("action=\"/sign-in\""), "{html}");
    assert!(!html.contains("Signed in as"), "{html}");
    let answer = command(&s, "feed.app.post", "s-reader", "i-1", "[\"Let me in\"]");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert!(answer.contains("\"refused\":\"SignedIn\""), "{answer}");
    assert!(answer.contains("\"committed\":false"), "{answer}");
    let html = home(&s, "s-other");
    assert!(!html.contains("Let me in"), "{html}");
    // A like and a reply are held to it as well.
    let liked = command(&s, "feed.app.like", "s-reader", "i-2", "[\"p1\"]");
    assert!(liked.contains("\"refused\":\"SignedIn\""), "{liked}");
    let replied = command(&s, "feed.app.reply", "s-reader", "i-3", "[\"p1\",\"Hi\"]");
    assert!(replied.contains("\"refused\":\"SignedIn\""), "{replied}");
}

/// A response's body, as JSON.
fn body_of(answer: &str) -> serde_json::Value {
    let body = answer.split("\r\n\r\n").nth(1).unwrap_or_default();
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {answer}"))
}

/// **A refusal says the program's words for its predicate** (ADR-0302): the
/// feed declares `predicate SignedIn  says "…"`, and a post refused by it is
/// answered with those words, the predicate's name, and nothing the
/// predicate read.
#[test]
fn a_refusal_says_the_programs_words_for_its_predicate() {
    let s = feed_with(Arc::new(TestProvider::default()));
    let answer = command(&s, "feed.app.post", "s-reader", "i-1", "[\"Let me in\"]");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(
        body_of(&answer),
        serde_json::json!({
            "committed": false,
            "refused": "SignedIn",
            "says": "Sign in to post, reply, like or follow.",
        })
    );
}

/// **Where the program declares no words, the deployment's are said**
/// (ADR-0302): the deployment that gives a predicate its meaning gives it
/// words too.
#[test]
fn a_refusal_the_program_gives_no_words_is_told_in_the_deployments() {
    // With no `SignedIn` declared, no page can ask it (ADR-0307): every
    // control is shown to each reader, `|refusable`, its refusal told.
    let s = served_feed_with(|app| {
        not_asked(
            &app.replace(
                "predicate SignedIn\n    says \"Sign in to post, reply, like or follow.\"\n",
                "",
            ),
            "SignedIn",
        )
        .replace("on:press=", "on:press|refusable=")
        .replace("on:submit|prevent=", "on:submit|prevent|refusable=")
    });
    s.identity
        .use_provider(Arc::new(TestProvider::default()), REDIRECT);
    let answer = command(&s, "feed.app.post", "s-reader", "i-1", "[\"Let me in\"]");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(body_of(&answer)["says"], "Sign in to do this.");
}

/// **A refused press sent again is refused again, as it was** (ADR-0302):
/// the refusal is kept with its interaction. Until ADR-0302 it lived only
/// on the thread that ran `requires`, and a resend, answered from what was
/// kept, was answered 202 as a command that did not commit, with nothing
/// to tell the reader.
#[test]
fn a_refused_press_sent_again_is_refused_again() {
    let s = feed_with(Arc::new(TestProvider::default()));
    let first = command(&s, "feed.app.post", "s-reader", "i-1", "[\"Let me in\"]");
    let again = command(&s, "feed.app.post", "s-reader", "i-1", "[\"Let me in\"]");
    assert!(again.starts_with("HTTP/1.1 403"), "{again}");
    assert_eq!(body_of(&first), body_of(&again));
}

/// **A page holds its announcer from the first byte** (ADR-0302): where a
/// failed press is said, once, empty, before the runtime that says it. A
/// live region added with its words is not reliably said (ADR-0182).
#[test]
fn a_page_holds_its_announcer_from_the_first_byte() {
    let s = feed_with(Arc::new(TestProvider::default()));
    let answer = get(&s, "/", "pw-session=s-reader");
    let html = answer.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    assert!(html.starts_with("<!doctype html>"), "{answer}");
    assert_eq!(html.matches(pw_render::ANNOUNCER).count(), 1, "{html}");
    let (before, _) = html
        .split_once("id=\"pw-parts\"")
        .expect("a parts manifest");
    assert!(before.contains(pw_render::ANNOUNCER), "{html}");
}

/// **A program is not served by a deployment that cannot evaluate a
/// predicate it requires or declares** (ADR-0302): each press of its
/// commands would be answered with nothing to tell.
#[test]
fn a_predicate_the_deployment_cannot_evaluate_is_refused_at_start() {
    for (change, unknown) in [
        (
            (|app: &str| {
                // `Verified` is no predicate the program declares, so no page
                // asks it: the composer that sends `post` is `|refusable`
                // (ADR-0307), and the host is what refuses it.
                app.replacen(
                    "    requires      SignedIn\n",
                    "    requires      SignedIn, Verified\n",
                    1,
                )
                .replace(
                    "<form hidden={!SignedIn} on:submit|prevent={() => match post_text(draft) {",
                    "<form hidden={!SignedIn} on:submit|prevent|refusable={() => match post_text(draft) {",
                )
            }) as fn(&str) -> String,
            "`Verified`",
        ),
        (
            |app: &str| {
                format!(
                    "{app}\npredicate Trusted\n    says \"Only a trusted reader can do this.\"\n"
                )
            },
            "`Trusted`",
        ),
    ] {
        let (_dir, out) = built_feed_with(change);
        match Server::from_build(out.clone(), out) {
            Ok(_) => panic!("served, though the deployment cannot evaluate {unknown}"),
            Err(why) => assert!(
                why.contains(&format!(
                    "cannot evaluate the predicate the program requires: {unknown}"
                )),
                "{why}"
            ),
        }
    }
}

/// **What a signed-in user writes is theirs** (ADR-0258): a post and a reply
/// name their author as their provider named them, to every reader, and the
/// reader's own page says who is signed in.
#[test]
fn a_post_and_a_reply_are_their_signed_in_authors() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let ada = signed_in(&s, &provider, "s-a", "ada");
    let answer = command(&s, "feed.app.post", &ada, "i-1", "[\"Ada's own post\"]");
    assert!(answer.contains("\"committed\":true"), "{answer}");
    let answer = command(
        &s,
        "feed.app.reply",
        &ada,
        "i-2",
        "[\"p1\",\"Ada's reply\"]",
    );
    assert!(answer.contains("\"committed\":true"), "{answer}");
    let mine = home(&s, &ada);
    assert!(mine.contains("Signed in as Name of ada"), "{mine}");
    assert!(!mine.contains("Sign in to post"), "{mine}");
    let theirs = home(&s, "s-reader");
    let row = theirs
        .split("<li")
        .find(|li| li.contains("Ada's own post"))
        .unwrap_or_else(|| panic!("the post: {theirs}"));
    assert!(row.contains("Name of ada"), "{row}");
    let thread = page(&s, "s-reader", "feed.app.PostPage~ui", &thread_of("p1"));
    let reply = thread
        .split("<article")
        .find(|a| a.contains("Ada's reply"))
        .unwrap_or_else(|| panic!("the reply: {thread}"));
    assert!(reply.contains("Name of ada"), "{reply}");
}

/// **Only its author deletes a post** (`requires SignedIn, OwnsPost(post)`,
/// ADR-0115): a predicate over the command's parameter, read in the
/// command's own transaction. Another user's delete is refused with the
/// predicate and the post stays; the author's deletes it; its Delete button
/// is on the author's row alone.
#[test]
fn only_its_author_deletes_a_post() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let ada = signed_in(&s, &provider, "s-a", "ada");
    let grace = signed_in(&s, &provider, "s-g", "grace");
    let answer = command(&s, "feed.app.post", &ada, "i-1", "[\"Mine to delete\"]");
    assert!(answer.contains("\"committed\":true"), "{answer}");
    let rows = |session: &str| home(&s, session);
    let row = |html: &str| {
        html.split("<li")
            .find(|li| li.contains("Mine to delete"))
            .map(str::to_string)
    };
    let mine = row(&rows(&ada)).expect("ada's row");
    let id = mine
        .split("href=\"/post/")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or_else(|| panic!("the post's id: {mine}"))
        .to_string();
    assert!(mine.contains(">Delete<"), "{mine}");
    assert!(
        !row(&rows(&grace))
            .expect("grace's row")
            .contains(">Delete<")
    );

    let refused = command(&s, "feed.app.delete", &grace, "i-2", &format!("[\"{id}\"]"));
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(refused.contains("\"refused\":\"OwnsPost\""), "{refused}");
    assert!(row(&rows(&grace)).is_some(), "the post stays");
    let signed_out = command(
        &s,
        "feed.app.delete",
        "s-nobody",
        "i-3",
        &format!("[\"{id}\"]"),
    );
    assert!(
        signed_out.contains("\"refused\":\"SignedIn\""),
        "{signed_out}"
    );

    let deleted = command(&s, "feed.app.delete", &ada, "i-4", &format!("[\"{id}\"]"));
    assert!(deleted.starts_with("HTTP/1.1 202"), "{deleted}");
    assert!(deleted.contains("\"committed\":true"), "{deleted}");
    assert!(row(&rows(&grace)).is_none(), "the post is gone");
    // Another's post, which the seed's `p1` is, ada's own seed account aside.
    let theirs = command(&s, "feed.app.delete", &ada, "i-5", "[\"p1\"]");
    assert!(theirs.contains("\"refused\":\"OwnsPost\""), "{theirs}");
}

/// **A command from another origin is refused before anything runs**
/// (charter §17.5, "CSRF for commands"): a cross-site `Sec-Fetch-Site`, or
/// without it an `Origin` that is not the request's host, is answered 403,
/// though the request carries a signed-in session's cookie.
#[test]
fn a_command_from_another_origin_is_refused() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let ada = signed_in(&s, &provider, "s-a", "ada");
    let body = "[\"Forged\"]";
    for from in [
        "Sec-Fetch-Site: cross-site\r\n",
        "Sec-Fetch-Site: same-site\r\n",
        "Origin: https://evil.example.test\r\n",
        "Origin: null\r\n",
    ] {
        let answer = exchanged(
            &s,
            &format!(
                "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session={ada}\r\n\
                 {from}pw-interaction: i-{}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\n\r\n{body}",
                from.len(),
                body.len()
            ),
        );
        assert!(answer.starts_with("HTTP/1.1 403"), "{from}: {answer}");
    }
    let signed_out = exchanged(
        &s,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={ada}\r\n\
             Sec-Fetch-Site: cross-site\r\ncontent-length: 0\r\n\r\n"
        ),
    );
    assert!(signed_out.starts_with("HTTP/1.1 403"), "{signed_out}");
    let html = home(&s, &ada);
    assert!(!html.contains("Forged"), "{html}");
    assert!(html.contains("Signed in as"), "still signed in: {html}");
    // From this origin, by `Origin` alone, it runs.
    let answer = exchanged(
        &s,
        &format!(
            "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session={ada}\r\n\
             Origin: http://t\r\npw-interaction: i-9\r\ncontent-type: application/json\r\n\
             content-length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(answer.contains("\"committed\":true"), "{answer}");
}

/// **A fresh session's cookie is HttpOnly and SameSite=Lax** on a page, and
/// its id is 128 random bits: not the process id and a counter, which the
/// next visitor could guess.
#[test]
fn a_fresh_sessions_cookie_is_http_only_and_unguessable() {
    let s = served_feed();
    let ids: Vec<String> = (0..2)
        .map(|_| {
            let answer = exchanged(&s, "GET / HTTP/1.1\r\nHost: t\r\n\r\n");
            let (id, line) = set_cookie(&answer, "pw-session").expect("a session");
            assert!(line.contains("HttpOnly"), "{line}");
            assert!(line.contains("SameSite=Lax"), "{line}");
            assert!(!line.contains("Secure"), "a loopback origin: {line}");
            assert!(id.starts_with("s-") && id.len() == 34, "{id}");
            id
        })
        .collect();
    assert_ne!(ids[0], ids[1]);
}

/// **Two users' pages share nothing private** (charter §15.6 tests 12 and
/// 13, ADR-0184): signed in as two users, each reads the home page and a
/// thread. Each page says who it is and not the other; and what
/// the query runtime keeps for every reader, and the materializer's public
/// fragments, name neither session, neither reader, nor anything of
/// `Me`'s, and are what they are when no one signed in.
#[test]
fn two_users_pages_never_share_private_parts() {
    let kept = |signed: bool| -> (Vec<String>, Vec<String>, Vec<String>) {
        let provider = Arc::new(TestProvider::default());
        let s = feed_with(provider.clone());
        let sessions: Vec<String> = if signed {
            vec![
                signed_in(&s, &provider, "s-a", "ada"),
                signed_in(&s, &provider, "s-g", "grace"),
            ]
        } else {
            vec!["s-a".to_string(), "s-g".to_string()]
        };
        let mut pages = Vec::new();
        for session in &sessions {
            pages.push(home(&s, session));
            page(&s, session, "feed.app.PostPage~ui", &thread_of("p1"));
        }
        if signed {
            assert!(
                pages[0].contains("Signed in as Name of ada"),
                "{}",
                pages[0]
            );
            assert!(!pages[0].contains("Name of grace"), "{}", pages[0]);
            assert!(
                pages[1].contains("Signed in as Name of grace"),
                "{}",
                pages[1]
            );
            assert!(!pages[1].contains("Name of ada"), "{}", pages[1]);
        }
        let mut values: Vec<String> = s
            .queries
            .public_cache_contents()
            .into_iter()
            .map(|(key, value)| format!("{key:?} = {value:?}"))
            .collect();
        values.sort();
        let mut fragments: Vec<String> = s
            .materializer
            .all_entries()
            .into_iter()
            .filter(|(key, _)| key.contains("partition=public"))
            .map(|(key, entry)| format!("{key} = {}", entry.body))
            .collect();
        fragments.sort();
        (values, fragments, sessions)
    };
    let (values, fragments, sessions) = kept(true);
    assert!(!values.is_empty(), "the query runtime keeps the thread");
    let (unsigned_values, unsigned_fragments, _) = kept(false);
    assert_eq!(
        (&values, &fragments),
        (&unsigned_values, &unsigned_fragments)
    );
    for kept in values.iter().chain(&fragments) {
        for private in sessions.iter().map(String::as_str).chain([
            "Name of ada",
            "Name of grace",
            "signed-in",
            "Signed in as",
        ]) {
            assert!(
                !kept.contains(private),
                "{private} is kept for every reader: {kept}"
            );
        }
    }
}

/// **The guest model's reader is its session's guest**, the development
/// model the server held before the track: signed in, named for its
/// session's digest and never the session (track `data-path`, the
/// integrator's ruling 1 of 2026-11-02), posting as it, and with no account
/// to sign out of.
#[test]
fn the_guest_models_reader_is_its_sessions_guest() {
    let s = served_feed();
    let html = home(&s, "s-guest");
    let guest = Principal::guest("s-guest").name;
    assert!(
        html.contains(&format!("Signed in as {guest}")),
        "{guest}: {html}"
    );
    assert!(!html.contains("s-guest"), "the session is no name: {html}");
    assert!(!html.contains("action=\"/sign-out\""), "{html}");
    assert!(!html.contains("Sign in to post"), "{html}");
    let answer = command(&s, "feed.app.post", "s-guest", "i-1", "[\"As a guest\"]");
    assert!(answer.contains("\"committed\":true"), "{answer}");
}

/// **A principal is the deployment's, not the request's**: a session no
/// sign-in opened is no one in the accounts model, whatever it calls itself,
/// and a guest only in the guest model.
#[test]
fn a_session_is_no_one_until_a_provider_vouches_for_it() {
    let s = feed_with(Arc::new(TestProvider::default()));
    assert_eq!(s.identity.principals().of("u-ada"), None);
    assert_eq!(s.identity.principals().of("s-anything"), None);
    s.identity.use_guests();
    assert_eq!(
        s.identity.principals().of("s-anything"),
        Some(Principal::guest("s-anything"))
    );
}

/// **Nothing the feed keeps holds a session's cookie** (track `data-path`,
/// the integrator's ruling of 2026-11-02): a reader signed in through the
/// flow reads the home page, whose timeline is a read keyed by
/// `current_session()`, posts, and follows, whose invalidations name
/// `current_session()`. Nothing the host keeps or logs (the commands'
/// records, the cached reads' keys, the state, the outbox, the routes'
/// records) holds the session or the one the browser came with, as written
/// or in hex; the timeline's read is keyed by the program's handle on the
/// session, which until this ruling was the cookie itself.
#[test]
fn nothing_the_feed_keeps_holds_a_session_cookie() {
    let provider = Arc::new(TestProvider::default());
    let s = feed_with(provider.clone());
    let came_with = crate::identity::new_session_id();
    let ada = signed_in(&s, &provider, &came_with, "ada");
    // The messages print no answer, which may carry the cookie.
    let posted = command(&s, "feed.app.post", &ada, "i-post", "[\"Kept in memory\"]");
    assert!(posted.contains("\"committed\":true"), "the post commits");
    let followed = command(&s, "feed.app.follow", &ada, "i-follow", "[\"u-grace\"]");
    assert!(
        followed.contains("\"committed\":true"),
        "the follow commits"
    );
    let page = get(&s, "/", &format!("pw-session={ada}"));
    assert!(
        page.contains("Kept in memory"),
        "the home page shows the post"
    );
    let kept = super::guestbook::kept_by_host(&s, &ada);
    assert!(
        kept.contains(&crate::identity::session_handle(&ada)),
        "the timeline's read, keyed by the program's handle on the session"
    );
    super::guestbook::holds_none_of(&kept, &[&ada, &came_with]);
    // Nor does the page show it: its body (its headers are the browser's).
    let shown = page.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    super::guestbook::holds_none_of(shown, &[&ada, &came_with]);
}

/// **Every feed operation a session is passed to answers for its request's
/// session alone** (track `data-path`): for session `a`'s request, each read
/// and each write of the feed's layer that is passed a session refuses
/// another session's handle, and the cookie itself, which no program holds
/// (correction 8), and answers the request's own handle. `posts#delete` was
/// passed the session and read none of it until the re-review of
/// 2026-11-02. Each write in a command of its own, dropped: nothing stays.
pub(super) fn each_operation_answers_its_requests_session_alone(s: &Server) {
    let text = |v: &str| Val::String(v.into());
    let mine = crate::identity::session_handle("a");
    let theirs = crate::identity::session_handle("b");
    let reads: [(&str, Vec<Val>); 6] = [
        ("feed:data/posts#timeline", vec![Val::S64(5)]),
        ("feed:data/posts#following", vec![Val::S64(5)]),
        ("feed:data/users#relation", vec![text("u-grace")]),
        ("feed:data/users#of-session", vec![]),
        ("feed:data/users#viewer", vec![]),
        ("feed:data/uploads#attached", vec![]),
    ];
    let writes: [(&str, Vec<Val>); 10] = [
        ("feed:data/posts#publish", vec![text("Never posted")]),
        (
            "feed:data/posts#reply",
            vec![text("p1"), text("Never replied")],
        ),
        (
            "feed:data/posts#publish-image",
            vec![text("Never posted"), text("words"), text("no-upload")],
        ),
        ("feed:data/users#follow", vec![text("u-grace")]),
        ("feed:data/users#unfollow", vec![text("u-grace")]),
        ("feed:data/posts#delete", vec![text("p1")]),
        (
            "feed:data/messages#send",
            vec![text("u-grace"), text("Never sent")],
        ),
        ("feed:data/posts#like", vec![text("p1")]),
        ("feed:data/uploads#claim", vec![]),
        ("feed:data/uploads#discard", vec![text("no-upload")]),
    ];
    let passed = |session: &str, rest: &[Val]| -> Vec<Val> {
        std::iter::once(text(session))
            .chain(rest.iter().cloned())
            .collect()
    };
    let refused = |answer: &Result<Vec<Val>, String>| matches!(answer, Err(why) if why.contains("was passed another session"));
    let check = |op: &str, f: &HostFn, rest: &[Val]| {
        for (whose, session) in [
            ("another session's handle", theirs.as_str()),
            ("the cookie", "a"),
        ] {
            assert!(
                refused(&f(&passed(session, rest))),
                "`{op}` refuses {whose}"
            );
        }
        assert!(
            !refused(&f(&passed(&mine, rest))),
            "`{op}` answers its request's own handle"
        );
    };
    let ops = s.data.reads("a", None);
    for (op, rest) in &reads {
        let f = ops.get(*op);
        assert!(f.is_some(), "the feed reads `{op}`");
        if let Some(f) = f {
            check(op, f, rest);
        }
    }
    for (op, rest) in &writes {
        // A command of its own each: a write refused, or one answered and
        // dropped, leaves the next one's transaction whole.
        for session in [theirs.as_str(), "a", mine.as_str()] {
            let staged = s.data.begin("a");
            let ops = staged.ops();
            let f = ops.get(*op);
            assert!(f.is_some(), "the feed writes `{op}`");
            if let Some(f) = f {
                let answer = f(&passed(session, rest));
                if session == mine {
                    assert!(!refused(&answer), "`{op}` answers its request's own handle");
                } else {
                    assert!(
                        refused(&answer),
                        "`{op}` refuses a session not its request's"
                    );
                }
            }
        }
    }
}

/// [`each_operation_answers_its_requests_session_alone`], on the feed in
/// memory.
#[test]
fn each_feed_operation_answers_its_requests_session_alone() {
    each_operation_answers_its_requests_session_alone(&served_feed());
}

/// **On PostgreSQL** (ADR-0246): the same principals, the author's row
/// written with their post, and a delete held to `OwnsPost` in the command's
/// transaction. Skipped without `PW_FEED_DATABASE_URL`.
mod on_postgres {
    use super::*;
    use crate::feed_pg::FeedPg;

    /// A schema of the test's own, dropped when it is done.
    struct Schema {
        url: String,
        name: String,
    }

    impl Schema {
        fn count(&self, query: &str) -> i64 {
            let mut c = postgres::Client::connect(&self.url, postgres::NoTls).expect("connected");
            c.batch_execute(&format!("SET search_path TO {}", self.name))
                .expect("the test's schema");
            c.query_one(query, &[]).expect(query).get(0)
        }

        fn one(&self, query: &str) -> Vec<String> {
            let mut c = postgres::Client::connect(&self.url, postgres::NoTls).expect("connected");
            c.batch_execute(&format!("SET search_path TO {}", self.name))
                .expect("the test's schema");
            let row = c.query_one(query, &[]).expect(query);
            (0..row.len()).map(|i| row.get(i)).collect()
        }

        /// **Every row of every table of the schema, as text** (track
        /// `data-path`): each row as PostgreSQL writes a row as text, its
        /// `bytea` in hex.
        fn every_row(&self) -> String {
            let mut c = postgres::Client::connect(&self.url, postgres::NoTls).expect("connected");
            let tables: Vec<String> = c
                .query(
                    "SELECT table_name::text FROM information_schema.tables \
                     WHERE table_schema = $1 AND table_type = 'BASE TABLE'",
                    &[&self.name],
                )
                .expect("the schema's tables")
                .iter()
                .map(|row| row.get(0))
                .collect();
            let mut text = String::new();
            for table in tables {
                let rows = c
                    .query(
                        &format!("SELECT t::text FROM \"{}\".\"{table}\" t", self.name),
                        &[],
                    )
                    .expect("a table's rows");
                for row in rows {
                    text.push_str(&format!("{table}: {}\n", row.get::<_, String>(0)));
                }
            }
            text
        }
    }

    impl Drop for Schema {
        fn drop(&mut self) {
            if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
                let _ = c.batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.name));
            }
        }
    }

    /// The feed on PostgreSQL, signed in through `provider`, or none
    /// without a database. The server is dropped before the schema.
    fn served(test: &str, provider: Arc<TestProvider>) -> Option<(Served, Schema)> {
        let url = match std::env::var("PW_FEED_DATABASE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
                return None;
            }
        };
        let schema = Schema {
            name: format!("pw_identity_{}_{test}", std::process::id()),
            url: url.clone(),
        };
        let layer = FeedPg::open(&url, Some(&schema.name)).expect("opened");
        let (dir, out) = built_feed_with(|app| app.to_string());
        let server =
            Server::from_build_with(out.clone(), out, Some(Arc::new(layer))).expect("served");
        server.identity.use_provider(provider, REDIRECT);
        Some((Served { server, _dir: dir }, schema))
    }

    /// **A signed-in author's post names them**, their row written in the
    /// command's transaction as their provider named them.
    #[test]
    fn a_post_on_postgres_is_its_signed_in_authors() {
        let provider = Arc::new(TestProvider::default());
        let Some((s, db)) = served("author", provider.clone()) else {
            return;
        };
        let ada = signed_in(&s, &provider, "s-a", "ada");
        let answer = command(&s, "feed.app.post", &ada, "i-1", "[\"On the database\"]");
        assert!(answer.contains("\"committed\":true"), "{answer}");
        let row = db.one(
            "SELECT p.author, u.handle, u.name FROM posts p JOIN users u ON u.id = p.author \
             WHERE p.text = 'On the database'",
        );
        assert_eq!(row, ["ada", "@ada", "Name of ada"]);
        let theirs = home(&s, "s-reader");
        let row = theirs
            .split("<li")
            .find(|li| li.contains("On the database"))
            .unwrap_or_else(|| panic!("the post: {theirs}"));
        assert!(row.contains("Name of ada"), "{row}");
        assert!(!row.contains(">Delete<"), "{row}");
        assert!(home(&s, &ada).contains(">Delete<"));
        drop(s);
    }

    /// **A delete on PostgreSQL is its author's**: another's is refused and
    /// changes nothing; the author's removes the post, its replies and their
    /// likes, in one transaction.
    #[test]
    fn a_delete_on_postgres_is_its_authors_alone() {
        let provider = Arc::new(TestProvider::default());
        let Some((s, db)) = served("delete", provider.clone()) else {
            return;
        };
        let ada = signed_in(&s, &provider, "s-a", "ada");
        let grace = signed_in(&s, &provider, "s-g", "grace");
        let posted = command(&s, "feed.app.post", &ada, "i-1", "[\"Going soon\"]");
        assert!(posted.contains("\"committed\":true"), "{posted}");
        let id = db
            .one("SELECT id FROM posts WHERE text = 'Going soon'")
            .remove(0);
        let reply = format!("[\"{id}\",\"A reply to it\"]");
        let replied = command(&s, "feed.app.reply", &grace, "i-2", &reply);
        assert!(replied.contains("\"committed\":true"), "{replied}");
        let this = format!("[\"{id}\"]");
        let liked = command(&s, "feed.app.like", &grace, "i-3", &this);
        assert!(liked.contains("\"committed\":true"), "{liked}");
        let before = db.count("SELECT count(*) FROM posts");
        let likes = db.count("SELECT count(*) FROM likes");

        let refused = command(&s, "feed.app.delete", &grace, "i-4", &this);
        assert!(refused.contains("\"refused\":\"OwnsPost\""), "{refused}");
        assert_eq!(db.count("SELECT count(*) FROM posts"), before);

        let deleted = command(&s, "feed.app.delete", &ada, "i-5", &this);
        assert!(deleted.contains("\"committed\":true"), "{deleted}");
        assert_eq!(db.count("SELECT count(*) FROM posts"), before - 2);
        assert_eq!(db.count("SELECT count(*) FROM likes"), likes - 1);
        let again = command(&s, "feed.app.delete", &ada, "i-6", &this);
        assert!(
            again.contains("\"refused\":\"OwnsPost\""),
            "a post not there is no one's: {again}"
        );
        drop(s);
    }

    /// [`super::each_operation_answers_its_requests_session_alone`], on the
    /// feed on PostgreSQL, each write in a transaction rolled back.
    #[test]
    fn each_feed_operation_answers_its_requests_session_alone_on_postgres() {
        let Some((s, _db)) = served("handles", Arc::new(TestProvider::default())) else {
            return;
        };
        super::each_operation_answers_its_requests_session_alone(&s);
        drop(s);
    }

    /// **Nothing the feed keeps on PostgreSQL holds a session's cookie, nor
    /// does its page show it** (track `data-path`, the integrator's ruling of
    /// 2026-11-02): a reader signed in through the flow posts and follows,
    /// and no row of any table of the schema (the commands' records, the
    /// outbox's events and invalidations, its session, its sign-in) holds
    /// the session or the one the browser came with, as written or in hex;
    /// nor does the home page. The record is there, keyed by the session's
    /// digest, and the invalidations, by the program's handle on it: until
    /// this ruling the outbox kept the cookie in `Relation`'s.
    #[test]
    fn nothing_kept_on_postgres_holds_a_session_cookie() {
        let provider = Arc::new(TestProvider::default());
        let Some((s, db)) = served("cookie", provider.clone()) else {
            return;
        };
        let came_with = "s-came-with-0123456789";
        let ada = signed_in(&s, &provider, came_with, "ada");
        let body = "[\"Kept on the database\"]";
        let answer = command(&s, "feed.app.post", &ada, "i-kept", body);
        assert!(answer.contains("\"committed\":true"), "the post commits");
        // A follow invalidates `FollowingTimeline(current_session(), _)` and
        // `Relation(current_session(), user)`, which the outbox keeps: their
        // session is the program's handle on it.
        let followed = command(&s, "feed.app.follow", &ada, "i-follow", "[\"u-grace\"]");
        assert!(
            followed.contains("\"committed\":true"),
            "the follow commits"
        );
        let page = get(&s, "/", &format!("pw-session={ada}"));
        let shown = page.split_once("\r\n\r\n").map_or("", |(_, body)| body);
        assert!(
            shown.contains("Kept on the database"),
            "the home page shows the post"
        );
        // The messages name what is missing, and print no row: a failing
        // run's log is kept, and a row might hold the very cookie.
        let rows = db.every_row();
        let handle = crate::identity::session_handle(&ada);
        assert!(
            rows.lines()
                .any(|row| row.starts_with("outbox: ") && row.contains(&handle)),
            "the follow's invalidations, by the session's handle"
        );
        let digest = crate::identity::session_field(crate::identity::INTERACTION_DOMAIN, &ada);
        assert!(
            rows.lines()
                .any(|row| row.starts_with("interactions: ") && row.contains(&digest)),
            "the command's record, keyed by the session's digest"
        );
        assert!(
            rows.lines().any(|row| row.starts_with("sessions: ")),
            "the session, kept"
        );
        super::super::guestbook::holds_none_of(&rows, &[&ada, came_with]);
        super::super::guestbook::holds_none_of(shown, &[&ada, came_with]);
        drop(s);
    }
}

/// **Through the development provider, end to end**: sign up at its form,
/// back through the callback signed in; post; sign out; sign in again with
/// the password, the same user, whose post is still theirs to delete.
#[test]
fn the_development_provider_signs_up_and_in_through_the_flow() {
    let s = served_feed();
    s.identity
        .dev_accounts(&crate::identity::Deployment {
            development: true,
            origin: "http://127.0.0.1".to_string(),
        })
        .expect("started");
    // Through the provider and back, from `came_with`, with the form's
    // fields: the session the callback gives.
    let through = |came_with: &str, start: &str, fields: &str| -> String {
        let started = get(&s, start, &format!("pw-session={came_with}"));
        let (state, _) = set_cookie(&started, "pw-sign-in").expect("state");
        let to = location(&started);
        assert!(to.starts_with("/dev-idp/authorize?"), "{to}");
        let form = get(&s, &to, "");
        assert!(form.starts_with("HTTP/1.1 200"), "{form}");
        assert!(form.contains("not for production"), "{form}");
        // A page that runs nothing (`csp::BARE`), its type its type.
        assert!(
            form.contains(&format!(
                "content-security-policy: {}\r\n",
                crate::csp::BARE
            )),
            "{form}"
        );
        assert!(
            form.contains("x-content-type-options: nosniff\r\n"),
            "{form}"
        );
        let body = format!("{}&{fields}", to.split_once('?').expect("query").1);
        let answer = exchanged(
            &s,
            &format!(
                "POST /dev-idp/authorize HTTP/1.1\r\nHost: t\r\nSec-Fetch-Site: same-origin\r\n\
                 content-type: application/x-www-form-urlencoded\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        let back = location(&answer);
        let path = back
            .strip_prefix("http://127.0.0.1")
            .expect("to this origin");
        assert!(path.starts_with("/sign-in/callback?"), "{back}");
        let answer = get(
            &s,
            path,
            &format!("pw-session={came_with}; pw-sign-in={state}"),
        );
        assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
        set_cookie(&answer, "pw-session").expect("signed in").0
    };
    let first = through(
        "s-a",
        "/sign-up",
        "handle=ada&name=Ada+Lovelace&password=correct+horse",
    );
    assert!(home(&s, &first).contains("Signed in as Ada Lovelace"));
    let posted = command(
        &s,
        "feed.app.post",
        &first,
        "i-1",
        "[\"Through the provider\"]",
    );
    assert!(posted.contains("\"committed\":true"), "{posted}");
    exchanged(
        &s,
        &format!(
            "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session={first}\r\n\
             Sec-Fetch-Site: same-origin\r\ncontent-length: 0\r\n\r\n"
        ),
    );
    assert!(home(&s, &first).contains("Sign in to post"));
    let second = through("s-b", "/sign-in", "handle=ada&password=correct+horse");
    assert_ne!(first, second);
    let mine = home(&s, &second);
    let row = mine
        .split("<li")
        .find(|li| li.contains("Through the provider"))
        .unwrap_or_else(|| panic!("the post: {mine}"));
    assert!(
        row.contains("Ada Lovelace") && row.contains(">Delete<"),
        "{row}"
    );
}

/// **The relying party answers the routes the compiler checks forms and
/// links against** (ADR-0265): each of `pw_core::routes::RELYING_PARTY`
/// with its method, and the other method at each, nothing of it. The
/// compiler's table and this host's routes are one list, held here.
#[test]
fn the_relying_party_answers_the_routes_the_compiler_knows() {
    let s = feed_with(Arc::new(TestProvider::default()));
    let sent = |method: &str, route: &str| {
        exchanged(
            &s,
            &format!(
                "{method} {route} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=s-routes\r\n\
                 Sec-Fetch-Site: same-origin\r\ncontent-length: 0\r\n\r\n"
            ),
        )
    };
    for (method, route) in pw_core::routes::RELYING_PARTY {
        let answer = sent(method, route);
        assert!(
            !answer.starts_with("HTTP/1.1 404"),
            "{method} {route}: {answer}"
        );
        let other = if *method == "GET" { "POST" } else { "GET" };
        let answer = sent(other, route);
        assert!(
            answer.starts_with("HTTP/1.1 404") || answer.starts_with("HTTP/1.1 405"),
            "{other} {route}: {answer}"
        );
    }
}

/// `app` with nothing asked of `predicate` (ADR-0307): each `{#if P}` block
/// its first branch alone, and each `hidden={!P}` gone.
fn not_asked(app: &str, predicate: &str) -> String {
    let open = format!("{{#if {predicate}}}");
    let mut out = app.replace(&format!(" hidden={{!{predicate}}}"), "");
    while let Some(at) = out.find(&open) {
        let body = at + open.len();
        // The block's own `{:else}` and `{/if}`, past any block inside it.
        let (mut depth, mut i, mut els) = (0usize, body, None);
        let end = loop {
            let rest = &out[i..];
            if rest.starts_with("{#") {
                depth += 1;
            } else if rest.starts_with("{/") {
                if depth == 0 {
                    break i;
                }
                depth -= 1;
            } else if rest.starts_with("{:else}") && depth == 0 && els.is_none() {
                els = Some(i);
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        };
        let first = &out[body..els.unwrap_or(end)];
        let close = end + out[end..].find('}').map_or(0, |c| c + 1);
        out = format!("{}{}{}", &out[..at], first, &out[close..]);
    }
    out
}
