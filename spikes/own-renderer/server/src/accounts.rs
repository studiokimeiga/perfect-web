//! **A development identity provider** (track `identity`, ADR-0258): accounts
//! kept in memory, served by this server at `/dev-idp/authorize` as a
//! deployment's OpenID provider would be at its own origin. It implements
//! [`Provider`], the interface a deployment supplies, and it is **not
//! production**: [`DevProvider::start`] refuses anything but a development
//! deployment on a loopback origin, and every page it serves says so.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::identity::{
    CALLBACK, Claims, Deployment, Identity, Provider, Reply, encode, escape, hex, is_verifier,
    now_secs, parse_form, random_bytes, s256, same, token,
};

/// One account at the development provider: its password only as an
/// Argon2id PHC string, never the password.
#[derive(Debug, Clone)]
struct Account {
    sub: String,
    handle: String,
    name: String,
    hash: String,
}

/// A code the development provider issued, until it is exchanged once.
#[derive(Debug, Clone)]
struct Grant {
    sub: String,
    client_id: String,
    redirect_uri: String,
    challenge: String,
    nonce: Option<String>,
    issued: Instant,
}

/// How long a code may wait to be exchanged (RFC 6749 §4.1.2: "A maximum
/// authorization code lifetime of 10 minutes is RECOMMENDED"; shorter here).
const CODE_LIFETIME: Duration = Duration::from_secs(60);

/// **A development OpenID provider**: accounts kept in memory, passwords
/// hashed with Argon2id (RustCrypto's `argon2`, its default parameters, the
/// minimum the OWASP Password Storage Cheat Sheet gives: 19 MiB, two
/// iterations, one lane). Served by this server at `/dev-idp/authorize`, as a
/// deployment's provider would be at its own origin. **Not production**:
/// [`DevProvider::start`] refuses anything but a development deployment on
/// this machine.
pub struct DevProvider {
    issuer: String,
    client_id: String,
    redirect_uri: String,
    accounts: Mutex<HashMap<String, Account>>,
    codes: Mutex<HashMap<String, Grant>>,
}

/// The path the development provider authenticates at.
pub const DEV_AUTHORIZE: &str = "/dev-idp/authorize";

impl DevProvider {
    /// **The development provider, for `deployment`, or why not.** It
    /// refuses a deployment that is not development, and an origin that is
    /// not this machine's: its accounts are in memory, and its pages say it
    /// is not production.
    pub fn start(deployment: &Deployment) -> Result<DevProvider, String> {
        if !deployment.development {
            return Err(
                "the development identity provider refuses to start in a production deployment: \
                 a deployment supplies its own provider"
                    .to_string(),
            );
        }
        if !deployment.loopback() {
            return Err(format!(
                "the development identity provider refuses to start at `{}`: it serves only a \
                 loopback origin",
                deployment.origin
            ));
        }
        Ok(DevProvider {
            issuer: format!("{}/dev-idp", deployment.origin),
            client_id: "pleris-dev".to_string(),
            redirect_uri: format!("{}{CALLBACK}", deployment.origin),
            accounts: Mutex::new(HashMap::new()),
            codes: Mutex::new(HashMap::new()),
        })
    }

    /// **An account made**: its handle (1 to 15 of `a-z`, `0-9`, `_`, as a
    /// feed's handles are), its name (1 to 50 characters), and a password of
    /// at least 8 characters (NIST SP 800-63B §3.1.1.2), kept only as its
    /// Argon2id hash. Its `sub` is random.
    pub fn sign_up(&self, handle: &str, name: &str, password: &str) -> Result<String, String> {
        let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
        if handle.is_empty()
            || handle.len() > 15
            || !handle
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("A handle is 1 to 15 letters, digits or underscores.".to_string());
        }
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err("A name is 1 to 50 characters.".to_string());
        }
        if password.chars().count() < 8 {
            return Err("A password is at least 8 characters.".to_string());
        }
        if self
            .accounts
            .lock()
            .expect("accounts")
            .contains_key(&handle)
        {
            return Err(format!("@{handle} is taken."));
        }
        let hash = hash_password(password)?;
        let sub = format!("acct-{}", hex(&random_bytes::<8>()));
        let mut accounts = self.accounts.lock().expect("accounts");
        // Checked again under the lock: hashing is slow, and another sign-up
        // may have taken the handle meanwhile.
        if accounts.contains_key(&handle) {
            return Err(format!("@{handle} is taken."));
        }
        accounts.insert(
            handle.clone(),
            Account {
                sub: sub.clone(),
                handle,
                name: name.to_string(),
                hash,
            },
        );
        Ok(sub)
    }

    /// **The account `handle` names, where `password` is its password.**
    /// One answer for an unknown handle and a wrong password, so neither
    /// says which handles exist.
    pub fn sign_in(&self, handle: &str, password: &str) -> Result<String, String> {
        let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
        let account = self
            .accounts
            .lock()
            .expect("accounts")
            .get(&handle)
            .cloned();
        let refused = "That handle and password do not match an account.".to_string();
        match account {
            Some(a) if verify_password(password, &a.hash) => Ok(a.sub),
            Some(_) => Err(refused),
            None => {
                // As long as a wrong password takes, so that the time taken
                // does not say whether the handle exists.
                let _ = verify_password(password, dummy_hash());
                Err(refused)
            }
        }
    }

    /// What the provider keeps for each account, for a test to read: its
    /// handle and its stored hash.
    #[cfg(test)]
    fn stored(&self) -> Vec<(String, String)> {
        self.accounts
            .lock()
            .expect("accounts")
            .values()
            .map(|a| (a.handle.clone(), a.hash.clone()))
            .collect()
    }

    fn account_by_sub(&self, sub: &str) -> Option<Account> {
        self.accounts
            .lock()
            .expect("accounts")
            .values()
            .find(|a| a.sub == sub)
            .cloned()
    }

    /// **A code for `sub`**, bound to the request it answers.
    fn issue(&self, sub: &str, request: &AuthRequest) -> String {
        let code = token();
        self.codes.lock().expect("codes").insert(
            code.clone(),
            Grant {
                sub: sub.to_string(),
                client_id: request.client_id.clone(),
                redirect_uri: request.redirect_uri.clone(),
                challenge: request.challenge.clone(),
                nonce: request.nonce.clone(),
                issued: Instant::now(),
            },
        );
        code
    }

    /// **The authentication request in `query`, checked** (Core
    /// §3.1.2.2): this client, its one registered redirect URI, `openid`
    /// in scope, the code flow, and an S256 challenge (RFC 7636 §4.4.1:
    /// this provider requires PKCE).
    fn request(&self, query: &HashMap<String, String>) -> Result<AuthRequest, String> {
        let get = |k: &str| query.get(k).cloned().unwrap_or_default();
        if get("client_id") != self.client_id {
            return Err("unknown client_id".to_string());
        }
        if get("redirect_uri") != self.redirect_uri {
            return Err("redirect_uri is not the client's registered one".to_string());
        }
        if get("response_type") != "code" {
            return Err("response_type must be code".to_string());
        }
        if !get("scope").split(' ').any(|s| s == "openid") {
            return Err("scope must contain openid".to_string());
        }
        if get("code_challenge_method") != "S256" || get("code_challenge").len() != 43 {
            return Err("an S256 code_challenge is required".to_string());
        }
        Ok(AuthRequest {
            client_id: get("client_id"),
            redirect_uri: get("redirect_uri"),
            state: get("state"),
            nonce: query.get("nonce").cloned(),
            challenge: get("code_challenge"),
            sign_up: get("prompt") == "create",
        })
    }

    /// **Its `/dev-idp/authorize`**: the form on `GET`, and on `POST` the
    /// account signed in or made, answered by a redirect to the client with
    /// a code. A request it cannot verify is answered here, never sent back
    /// to a redirect URI it did not check.
    pub(crate) fn answer(&self, method: &str, query: &str, body: &[u8]) -> Reply {
        let fields = match method {
            "GET" => parse_form(query),
            "POST" => parse_form(&String::from_utf8_lossy(body)),
            _ => return Reply::provider_page(405, "Method not allowed", "<p>Use GET or POST.</p>"),
        };
        let request = match self.request(&fields) {
            Ok(r) => r,
            Err(why) => {
                return Reply::provider_page(
                    400,
                    "Sign-in request refused",
                    &format!("<p role=\"alert\">{}</p>", escape(&why)),
                );
            }
        };
        if method == "GET" {
            return Reply::provider_page(200, request.title(), &request.form(None, ""));
        }
        let get = |k: &str| fields.get(k).cloned().unwrap_or_default();
        let outcome = if request.sign_up {
            self.sign_up(&get("handle"), &get("name"), &get("password"))
        } else {
            self.sign_in(&get("handle"), &get("password"))
        };
        match outcome {
            Ok(sub) => {
                let code = self.issue(&sub, &request);
                Reply::redirect(
                    &format!(
                        "{}?code={}&state={}",
                        request.redirect_uri,
                        encode(&code),
                        encode(&request.state)
                    ),
                    Vec::new(),
                )
            }
            Err(why) => Reply::provider_page(
                if request.sign_up { 409 } else { 401 },
                request.title(),
                &request.form(Some(&why), &get("handle")),
            ),
        }
    }
}

impl Provider for DevProvider {
    fn issuer(&self) -> String {
        self.issuer.clone()
    }

    fn client_id(&self) -> String {
        self.client_id.clone()
    }

    fn authorization_endpoint(&self) -> String {
        DEV_AUTHORIZE.to_string()
    }

    fn serve(&self, method: &str, route: &str, query: &str, body: &[u8]) -> Option<Reply> {
        (route == DEV_AUTHORIZE).then(|| self.answer(method, query, body))
    }

    /// **The token request** (RFC 7636 §4.6): the code once, within its
    /// lifetime, for the client and redirect URI it was issued to, and only
    /// with the verifier whose S256 is its challenge. Otherwise
    /// `invalid_grant`.
    fn exchange(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<Claims, String> {
        let grant = self
            .codes
            .lock()
            .expect("codes")
            .remove(code)
            .ok_or("invalid_grant: the code is unknown or was used")?;
        if grant.issued.elapsed() > CODE_LIFETIME {
            return Err("invalid_grant: the code expired".to_string());
        }
        if grant.client_id != self.client_id || grant.redirect_uri != redirect_uri {
            return Err("invalid_grant: the code was issued for another redirect_uri".to_string());
        }
        if !is_verifier(verifier) || !same(&s256(verifier), &grant.challenge) {
            return Err(
                "invalid_grant: the code_verifier does not match the challenge".to_string(),
            );
        }
        let account = self
            .account_by_sub(&grant.sub)
            .ok_or("invalid_grant: the account is gone")?;
        Ok(Claims {
            iss: self.issuer.clone(),
            sub: account.sub,
            aud: vec![self.client_id.clone()],
            exp: now_secs() + 300,
            nonce: grant.nonce,
            preferred_username: Some(account.handle),
            name: Some(account.name),
        })
    }
}

/// One checked authentication request.
#[derive(Debug, Clone)]
struct AuthRequest {
    client_id: String,
    redirect_uri: String,
    state: String,
    nonce: Option<String>,
    challenge: String,
    sign_up: bool,
}

impl AuthRequest {
    fn title(&self) -> &'static str {
        if self.sign_up { "Sign up" } else { "Sign in" }
    }

    /// The provider's form, the request carried in hidden fields.
    fn form(&self, error: Option<&str>, handle: &str) -> String {
        let hidden = |k: &str, v: &str| {
            format!(
                "<input type=\"hidden\" name=\"{k}\" value=\"{}\">",
                escape(v)
            )
        };
        let mut fields = [
            hidden("client_id", &self.client_id),
            hidden("redirect_uri", &self.redirect_uri),
            hidden("response_type", "code"),
            hidden("scope", "openid profile"),
            hidden("state", &self.state),
            hidden("code_challenge", &self.challenge),
            hidden("code_challenge_method", "S256"),
        ]
        .join("");
        if let Some(nonce) = &self.nonce {
            fields.push_str(&hidden("nonce", nonce));
        }
        if self.sign_up {
            fields.push_str(&hidden("prompt", "create"));
        }
        let error = error
            .map(|e| format!("<p id=\"error\" role=\"alert\">{}</p>", escape(e)))
            .unwrap_or_default();
        let name = if self.sign_up {
            "<p><label>Name <input name=\"name\" required maxlength=\"50\" autocomplete=\"name\"></label></p>"
        } else {
            ""
        };
        let (password, other, other_text) = if self.sign_up {
            ("new-password", false, "Have an account? Sign in")
        } else {
            ("current-password", true, "No account? Sign up")
        };
        let mut other_query = format!(
            "client_id={}&redirect_uri={}&response_type=code&scope=openid%20profile&state={}\
             &code_challenge={}&code_challenge_method=S256",
            encode(&self.client_id),
            encode(&self.redirect_uri),
            encode(&self.state),
            encode(&self.challenge)
        );
        if let Some(nonce) = &self.nonce {
            other_query.push_str(&format!("&nonce={}", encode(nonce)));
        }
        if other {
            other_query.push_str("&prompt=create");
        }
        format!(
            "{error}<form method=\"post\" action=\"{DEV_AUTHORIZE}\">{fields}\
             <p><label>Handle <input name=\"handle\" required maxlength=\"16\" \
             autocomplete=\"username\" value=\"{}\"></label></p>{name}\
             <p><label>Password <input name=\"password\" type=\"password\" required \
             minlength=\"8\" autocomplete=\"{password}\"></label></p>\
             <p><button type=\"submit\">{}</button></p></form>\
             <p><a href=\"{DEV_AUTHORIZE}?{}\">{other_text}</a></p>",
            escape(handle),
            self.title(),
            escape(&other_query),
        )
    }
}

/// An Argon2id PHC string for `password`, salted at random.
fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::PasswordHasher;
    argon2::Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| format!("the password could not be hashed: {e}"))
}

/// Whether `password` is the one `phc` was made from, by the parameters
/// `phc` records.
fn verify_password(password: &str, phc: &str) -> bool {
    use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
    match PasswordHash::new(phc) {
        Ok(parsed) => argon2::Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A hash of no one's password, verified against for an unknown handle.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash_password(&token()).expect("a hash"))
}

impl Identity {
    /// **Sign-in through the development provider**, or why it would not
    /// start: the accounts model, its provider this server's `/dev-idp`.
    pub fn use_dev_accounts(&self, deployment: &Deployment) -> Result<(), String> {
        let dev = std::sync::Arc::new(DevProvider::start(deployment)?);
        let redirect = dev.redirect_uri.clone();
        self.use_provider(dev, &redirect);
        Ok(())
    }

    /// [`Identity::use_dev_accounts`], and the provider, for a test that
    /// makes accounts at it.
    #[cfg(test)]
    pub fn dev_accounts(
        &self,
        deployment: &Deployment,
    ) -> Result<std::sync::Arc<DevProvider>, String> {
        let dev = std::sync::Arc::new(DevProvider::start(deployment)?);
        let redirect = dev.redirect_uri.clone();
        self.use_provider(dev.clone(), &redirect);
        Ok(dev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev() -> DevProvider {
        DevProvider::start(&Deployment::development(3143)).expect("started")
    }

    /// The query of an authentication request to the development provider,
    /// its challenge made from `verifier`.
    fn request(dev: &DevProvider, verifier: &str, sign_up: bool) -> String {
        let mut q = format!(
            "client_id=pleris-dev&redirect_uri={}&response_type=code&scope=openid%20profile\
             &state=st&nonce=no&code_challenge={}&code_challenge_method=S256",
            encode(&dev.redirect_uri),
            s256(verifier)
        );
        if sign_up {
            q.push_str("&prompt=create");
        }
        q
    }

    fn text(reply: &Reply) -> String {
        String::from_utf8(reply.bytes("s-1", false, crate::identity::Cookies::default()))
            .expect("utf-8")
    }

    /// The code a redirect back to the client carries.
    fn code_in(reply: &Reply) -> String {
        let answer = text(reply);
        assert!(answer.starts_with("HTTP/1.1 303"), "{answer}");
        let location = answer
            .lines()
            .find_map(|l| l.strip_prefix("location: "))
            .expect("a location");
        assert!(
            location.starts_with("http://127.0.0.1:3143/sign-in/callback?"),
            "{location}"
        );
        assert!(location.ends_with("&state=st"), "{location}");
        parse_form(location.split_once('?').expect("a query").1)["code"].clone()
    }

    /// **The development provider is not production** (ADR-0258): it refuses
    /// a production deployment and an origin that is not this machine's, and
    /// every page it serves says so first.
    #[test]
    fn the_development_provider_refuses_to_start_outside_development() {
        let production = Deployment {
            development: false,
            origin: "http://127.0.0.1:3143".to_string(),
        };
        let why = DevProvider::start(&production).err().unwrap_or_default();
        assert!(why.contains("production"), "{why}");
        let public = Deployment {
            development: true,
            origin: "https://feed.example.com".to_string(),
        };
        let why = DevProvider::start(&public).err().unwrap_or_default();
        assert!(why.contains("loopback"), "{why}");
        let identity = Identity::default();
        assert!(
            identity
                .configure(&production, Some("dev-accounts"))
                .is_err()
        );
        assert!(identity.configure(&public, Some("dev-accounts")).is_err());
        assert!(
            identity.describe().starts_with("guest"),
            "{}",
            identity.describe()
        );
        assert!(
            identity
                .configure(&Deployment::development(3143), Some("dev-accounts"))
                .is_ok()
        );
        assert!(
            identity.describe().starts_with("accounts"),
            "{}",
            identity.describe()
        );
        let dev = dev();
        let page = text(&dev.answer("GET", &request(&dev, &token(), false), b""));
        assert!(page.contains("not for production"), "{page}");
    }

    /// **A request the provider cannot verify is answered at the provider**
    /// (Core §3.1.2.2, RFC 7636 §4.4.1): another client, another redirect
    /// URI, no `openid`, or no S256 challenge, and never a redirect to a URI
    /// it did not check.
    #[test]
    fn an_unverified_request_is_refused_at_the_provider() {
        let dev = dev();
        let good = request(&dev, &token(), false);
        for (bad, why) in [
            (
                good.replace("client_id=pleris-dev", "client_id=other"),
                "client_id",
            ),
            (good.replace("127.0.0.1", "evil.test"), "redirect_uri"),
            (
                good.replace("scope=openid%20profile", "scope=profile"),
                "openid",
            ),
            (
                good.replace("code_challenge_method=S256", "code_challenge_method=plain"),
                "S256",
            ),
            (
                good.replace("response_type=code", "response_type=token"),
                "response_type",
            ),
        ] {
            let answer = text(&dev.answer("GET", &bad, b""));
            assert!(answer.starts_with("HTTP/1.1 400"), "{why}: {answer}");
            assert!(answer.contains(why), "{why}: {answer}");
            assert!(!answer.contains("location:"), "{why}: {answer}");
        }
    }

    /// **A code is exchanged once, by the verifier of its challenge, for the
    /// redirect URI it was issued to** (RFC 7636 §4.6), for an ID token's
    /// claims naming the account, this client and the request's nonce.
    #[test]
    fn a_code_is_exchanged_once_by_its_verifier() {
        let dev = dev();
        let verifier = token();
        let form = format!(
            "{}&handle=ada&name=Ada+Lovelace&password=correct+horse",
            request(&dev, &verifier, true)
        );
        let code = code_in(&dev.answer("POST", "", form.as_bytes()));
        let redirect = dev.redirect_uri.clone();
        assert!(
            dev.exchange(&code, &token(), &redirect).is_err(),
            "another verifier"
        );
        // A code tried with the wrong verifier is spent.
        assert!(
            dev.exchange(&code, &verifier, &redirect).is_err(),
            "used once"
        );

        let form = format!(
            "{}&handle=ada&password=correct+horse",
            request(&dev, &verifier, false)
        );
        let code = code_in(&dev.answer("POST", "", form.as_bytes()));
        assert!(
            dev.exchange(&code, &verifier, "http://127.0.0.1:3143/elsewhere")
                .is_err()
        );
        let code = code_in(&dev.answer("POST", "", form.as_bytes()));
        let claims = dev
            .exchange(&code, &verifier, &redirect)
            .expect("exchanged");
        assert_eq!(claims.iss, "http://127.0.0.1:3143/dev-idp");
        assert_eq!(claims.aud, ["pleris-dev"]);
        assert_eq!(claims.nonce.as_deref(), Some("no"));
        assert_eq!(claims.preferred_username.as_deref(), Some("ada"));
        assert_eq!(claims.name.as_deref(), Some("Ada Lovelace"));
        assert!(claims.sub.starts_with("acct-"), "{}", claims.sub);
        assert!(
            dev.exchange(&code, &verifier, &redirect).is_err(),
            "used once"
        );
    }

    /// **A password is kept only as its Argon2id hash** (OWASP's Password
    /// Storage Cheat Sheet): the PHC string of Argon2id v19 at 19 MiB, two
    /// iterations, one lane, salted, so two accounts with one password keep
    /// two hashes, and neither keeps the password.
    #[test]
    fn a_password_is_kept_only_as_its_argon2id_hash() {
        let dev = dev();
        dev.sign_up("ada", "Ada", "correct horse battery")
            .expect("ada");
        dev.sign_up("grace", "Grace", "correct horse battery")
            .expect("grace");
        let stored = dev.stored();
        assert_eq!(stored.len(), 2);
        for (handle, hash) in &stored {
            assert!(
                hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
                "{handle}: {hash}"
            );
            assert!(!hash.contains("correct horse"), "{handle}: {hash}");
        }
        assert_ne!(stored[0].1, stored[1].1, "salted");
    }

    /// **Signing in takes the account's password**, and an unknown handle is
    /// answered as a wrong password is, so neither says which handles exist.
    #[test]
    fn signing_in_takes_the_password_and_says_nothing_of_who_exists() {
        let dev = dev();
        let sub = dev.sign_up("@Ada", "Ada", "correct horse").expect("made");
        assert_eq!(dev.sign_in("ada", "correct horse"), Ok(sub));
        let wrong = dev.sign_in("ada", "wrong horse").expect_err("refused");
        let unknown = dev.sign_in("nobody", "correct horse").expect_err("refused");
        assert_eq!(wrong, unknown);
        // Over the form, a wrong password is the form again, 401, with no
        // code and the reason read out.
        let form = format!(
            "{}&handle=ada&password=wrong",
            request(&dev, &token(), false)
        );
        let answer = text(&dev.answer("POST", "", form.as_bytes()));
        assert!(answer.starts_with("HTTP/1.1 401"), "{answer}");
        assert!(answer.contains("role=\"alert\""), "{answer}");
        assert!(!answer.contains("code="), "{answer}");
    }

    /// **An account's handle, name and password are checked when it is
    /// made**, and a handle is one account's.
    #[test]
    fn an_account_is_checked_when_it_is_made() {
        let dev = dev();
        assert!(dev.sign_up("", "Ada", "correct horse").is_err());
        assert!(dev.sign_up("ada lovelace", "Ada", "correct horse").is_err());
        assert!(
            dev.sign_up("a_very_long_handle_x", "Ada", "correct horse")
                .is_err()
        );
        assert!(dev.sign_up("ada", "", "correct horse").is_err());
        assert!(dev.sign_up("ada", "Ada", "short").is_err());
        assert!(dev.sign_up("ada", "Ada", "correct horse").is_ok());
        let taken = dev
            .sign_up("ADA", "Another", "correct horse")
            .expect_err("taken");
        assert!(taken.contains("taken"), "{taken}");
        // What a form echoes back is escaped.
        let form = format!(
            "{}&handle=%3Cscript%3E&name=x&password=correct+horse",
            request(&dev, &token(), true)
        );
        let answer = text(&dev.answer("POST", "", form.as_bytes()));
        assert!(answer.starts_with("HTTP/1.1 409"), "{answer}");
        assert!(!answer.contains("<script>"), "{answer}");
        assert!(answer.contains("&lt;script&gt;"), "{answer}");
    }
}
