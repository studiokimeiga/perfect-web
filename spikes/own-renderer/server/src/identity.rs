//! **Who a request is, and what `requires` is told** (track `identity`,
//! `docs/PARALLEL.md`, ADR-0253, ADR-0258).
//!
//! The identity track owns this module: sign-up, sign-in and sign-out, a
//! session's principal, the session cookie, the check that a command comes
//! from the page's own origin, and the `requires` evaluator. `main.rs` reaches
//! it only at lines marked `TRACK SEAM (identity)`.
//!
//! **What Pleris owns, and what a deployment owns** (ADR-0258). Pleris is not
//! an identity provider (the charter). It owns the relying party's half of an
//! OpenID Connect authorization code flow with PKCE: the `state`, the
//! `nonce` and the code verifier, the callback and its checks, the session it
//! opens, the cookie that carries it, and what `requires` is told. A
//! deployment owns the provider, behind [`Provider`]: where a browser is sent
//! to sign in, and the exchange of a code for an ID token's verified claims.
//!
//! Two providers are here, and both are for development only:
//! - **[`Mode::Guest`]**, the model this server held before the track: every
//!   session is its own guest principal, so every session is `SignedIn`.
//!   The store and the feed's existing tests run on it.
//! - **The development provider** (`accounts.rs`), served by this server at
//!   `/dev-idp/…` as an external provider would be. It refuses to start
//!   outside a development deployment on a loopback origin.

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pw_host::engine::{HostFn, Val};

// ---------------------------------------------------------------------------
// The deployment
// ---------------------------------------------------------------------------

/// **What the deployment says it is**: development or not, and the origin
/// its pages are served from. A cookie is `Secure` unless that origin is a
/// loopback one, and a development provider runs only on one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub development: bool,
    /// `scheme://host[:port]`, no path: `http://127.0.0.1:3143`.
    pub origin: String,
}

impl Deployment {
    /// A development deployment on this machine, at `port`.
    #[cfg(test)]
    pub fn development(port: u16) -> Deployment {
        Deployment {
            development: true,
            origin: format!("http://127.0.0.1:{port}"),
        }
    }

    /// **The deployment the environment states.** `PW_DEPLOYMENT` is
    /// `development` (or unset: this binary is the development server) or
    /// `production`; `PW_ORIGIN` is the public origin, by default this
    /// machine's at `port`. Anything else is refused, not guessed.
    pub fn from_env(port: u16) -> Result<Deployment, String> {
        let development = match std::env::var("PW_DEPLOYMENT").as_deref() {
            Err(_) | Ok("") | Ok("development") => true,
            Ok("production") => false,
            Ok(other) => {
                return Err(format!(
                    "PW_DEPLOYMENT is `{other}`: it is `development` or `production`"
                ));
            }
        };
        let origin = match std::env::var("PW_ORIGIN") {
            Ok(o) if !o.is_empty() => Deployment::origin_checked(&o)?,
            _ => format!("http://127.0.0.1:{port}"),
        };
        Ok(Deployment {
            development,
            origin,
        })
    }

    /// **`PW_ORIGIN`, read as an origin**: `http` or `https`, a host and a
    /// port, a trailing `/` dropped. Since ADR-0311 also nothing after them,
    /// as the URL Standard's tuple origin has nothing after them: a path, a
    /// query, a fragment or credentials would be written into every absolute
    /// address made with it, a page's canonical address among them.
    pub fn origin_checked(origin: &str) -> Result<String, String> {
        let origin = origin.trim_end_matches('/').to_string();
        if !(origin.starts_with("http://") || origin.starts_with("https://")) {
            return Err(format!("PW_ORIGIN `{origin}` is not an http(s) origin"));
        }
        let authority = origin.split_once("://").map_or("", |(_, rest)| rest);
        if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
            return Err(format!(
                "PW_ORIGIN `{origin}` is not an origin: a scheme, a host and a port, and \
                 nothing after them"
            ));
        }
        Ok(origin)
    }

    /// The origin's host, without its scheme or port.
    fn host(&self) -> &str {
        let rest = self
            .origin
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or(&self.origin);
        let authority = rest.split('/').next().unwrap_or(rest);
        if let Some(v6) = authority.strip_prefix('[') {
            return v6.split(']').next().unwrap_or(v6);
        }
        authority.split(':').next().unwrap_or(authority)
    }

    /// **Is the origin this machine's?** `localhost`, a name under
    /// `.localhost`, `127.0.0.1` or `::1` (RFC 6761 §6.3 reserves
    /// `localhost` for the loopback).
    pub fn loopback(&self) -> bool {
        let host = self.host().to_ascii_lowercase();
        host == "localhost" || host.ends_with(".localhost") || host == "127.0.0.1" || host == "::1"
    }

    /// **Does a cookie need `Secure`?** Everywhere but a plain-HTTP origin on
    /// this machine, where a browser could not be sent one back.
    pub fn secure_cookies(&self) -> bool {
        self.origin.starts_with("https://") || !self.loopback()
    }
}

/// Whether this process's cookies are `Secure`, set once by `main` from the
/// deployment ([`configure_cookies`]). A test's server never sets it, and is
/// a loopback one.
static SECURE_COOKIES: OnceLock<bool> = OnceLock::new();

/// **Set this process's cookie attributes from its deployment.** Once.
pub fn configure_cookies(deployment: &Deployment) {
    let _ = SECURE_COOKIES.set(deployment.secure_cookies());
}

// ---------------------------------------------------------------------------
// Cookies and ids
// ---------------------------------------------------------------------------

/// The session cookie's name.
pub const SESSION_COOKIE: &str = "pw-session";
/// The cookie binding a sign-in in flight to the browser that started it.
const SIGN_IN_COOKIE: &str = "pw-sign-in";

/// **A `Set-Cookie` line**: `HttpOnly`, so no script reads it; `SameSite=Lax`,
/// so a cross-site request carries it only on a top-level navigation by a
/// safe method (which a provider's redirect back is); and `Secure` outside
/// this machine. `max_age: Some(0)` removes it.
pub fn cookie_line(
    name: &str,
    value: &str,
    path: &str,
    max_age: Option<u64>,
    secure: bool,
) -> String {
    let mut line = format!("set-cookie: {name}={value}; Path={path}; HttpOnly; SameSite=Lax");
    if let Some(age) = max_age {
        line.push_str(&format!("; Max-Age={age}"));
    }
    if secure {
        line.push_str("; Secure");
    }
    line.push_str("\r\n");
    line
}

/// **The cookie that names `session`**, as every response that issues one
/// sends it (`main.rs`'s seams).
pub fn session_cookie(session: &str) -> String {
    cookie_line(
        SESSION_COOKIE,
        session,
        "/",
        None,
        *SECURE_COOKIES.get().unwrap_or(&false),
    )
}

/// `n` bytes from the operating system's random source.
pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the operating system's random source");
    bytes
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// **A fresh session id**: 128 bits from the operating system's random
/// source (OWASP's Session Management Cheat Sheet asks at least 64 bits of
/// entropy). Until the identity track a fresh session was the process id
/// plus a counter, which the next visitor could guess.
pub fn new_session_id() -> String {
    format!("s-{}", hex(&random_bytes::<16>()))
}

/// **An unguessable token**: 256 bits, base64url without padding, 43
/// characters (RFC 7636 §4.1's code verifier is exactly this).
pub fn token() -> String {
    base64url(&random_bytes::<32>())
}

/// Base64url without padding (RFC 4648 §5), as RFC 7636 §3 defines it.
pub fn base64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// **PKCE's S256** (RFC 7636 §4.2):
/// `BASE64URL-ENCODE(SHA256(ASCII(code_verifier)))`.
pub fn s256(verifier: &str) -> String {
    use sha2::Digest;
    base64url(&sha2::Sha256::digest(verifier.as_bytes()))
}

/// Two secrets compared in time that does not depend on where they differ.
pub(crate) fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// RFC 7636 §4.1: 43 to 128 unreserved characters.
pub(crate) fn is_verifier(v: &str) -> bool {
    (43..=128).contains(&v.len())
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Principals
// ---------------------------------------------------------------------------

/// **Who a session is** (ADR-0258): the user a provider vouched for. Its
/// `user` is the program's `UserId`: the provider's `sub`, "locally unique
/// and never reassigned identifier within the Issuer" (OpenID Connect Core
/// §2), or a guest's.
#[derive(Clone, PartialEq, Eq)]
pub struct Principal {
    pub user: String,
    pub handle: String,
    pub name: String,
    /// The provider's issuer, or `guest`.
    pub issuer: String,
}

/// Authority failures contain no session, principal, generation or source
/// error payload. Only this bounded category may cross into diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorityError {
    NotBrowserActor,
    NotBrowserReader,
    NotBrowserAdmission,
    Superseded,
    MissingPlatformContext,
    MissingPlatformBinding,
    SourceUnavailable,
    Changed,
    ChangedBeforeCommit,
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotBrowserActor => "a platform principal is not a browser command actor",
            Self::NotBrowserReader => "a platform principal is not a browser reader",
            Self::NotBrowserAdmission => "a platform principal is not a browser admission",
            Self::Superseded => "session admission was superseded",
            Self::MissingPlatformContext => "platform invocation has no on-behalf principal",
            Self::MissingPlatformBinding => "platform invocation principal is no longer held",
            Self::SourceUnavailable => "session authority source is unavailable",
            Self::Changed => "session authority changed; command outcome may have committed",
            Self::ChangedBeforeCommit => "session authority changed before commit",
        })
    }
}

impl From<AuthorityError> for String {
    fn from(error: AuthorityError) -> Self {
        error.to_string()
    }
}

/// A server-side actor generation, never a browser value or a bearer token.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SessionIdentity {
    pub(crate) principal: Principal,
    pub(crate) epoch: [u8; 16],
}

/// The reader a document binds. Anonymous is explicit; absence of a binding
/// must never be silently interpreted as an anonymous reader after revocation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ReaderIdentity {
    Guest,
    Anonymous,
    Account(SessionIdentity),
}

impl ReaderIdentity {
    /// One private-partition field. Hex encodes the cookie bytes so a caller's
    /// delimiters cannot add a query-key field or alias another generation.
    pub(crate) fn partition(&self, session: &str) -> String {
        let reader = match self {
            Self::Guest => "g".to_string(),
            Self::Anonymous => "n".to_string(),
            Self::Account(identity) => format!("a:{}", hex(&identity.epoch)),
        };
        format!("v1:{}:{reader}", hex(session.as_bytes()))
    }
}

// Diagnostics must not turn admission state into another private delivery
// channel. These values retain full equality and authority semantics, while
// Debug exposes only their kind, never cookie, principal or generation bytes.
impl std::fmt::Debug for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Principal").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for SessionIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionIdentity").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ReaderIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Guest => "Guest",
            Self::Anonymous => "Anonymous",
            Self::Account(_) => "Account([redacted])",
        })
    }
}

impl std::fmt::Debug for CommandActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandActor")
            .field("reader", &self.reader)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held").finish_non_exhaustive()
    }
}

impl Principal {
    /// **A session's own guest** (the guest model): named for its session,
    /// as the feed named one before accounts (ADR-0220).
    pub fn guest(session: &str) -> Principal {
        Principal {
            user: format!("u-{session}"),
            handle: format!("@{session}"),
            name: format!("Guest {session}"),
            issuer: "guest".to_string(),
        }
    }

    pub fn is_guest(&self) -> bool {
        self.issuer == "guest"
    }
}

type AdmissionClock = Mutex<Arc<()>>;

#[derive(Default)]
struct PrincipalsInner {
    /// The accounts model: a session is signed in only where a sign-in put
    /// it in `sessions`. Otherwise every session is its own guest.
    accounts: AtomicBool,
    sessions: RwLock<HashMap<String, Held>>,
    // Weak per-session tickets: unrelated admissions cannot suppress a
    // revocation. A live old reader retains its opaque token, preventing ABA.
    revisions: Mutex<HashMap<String, Weak<AdmissionClock>>>,
    // Separate authority gates; acquisition order is gate, sessions, ticket.
    gates: Mutex<HashMap<String, Weak<RwLock<()>>>>,
    /// TRACK SEAM (second-host): **where sessions are kept beyond this
    /// host's memory** (ADR-0369), the database the hosts share, where the
    /// data layer has one. `sessions` is then each request's copy of it.
    kept: OnceLock<Arc<dyn Kept>>,
    /// TRACK SEAM (second-host): **how long a session lives**, idle and in
    /// all (the integrator's ruling 2 of 2026-10-20 on this track), as the
    /// deployment states them.
    lifetimes: RwLock<SessionLifetimes>,
}

/// **A session's principal as this host holds it**, and when it was opened
/// and last used here, by which a session in this host's memory alone
/// expires (ADR-0369). Where sessions are kept beyond the host, the
/// database's clock decides, and this is each request's copy.
struct Held {
    principal: Principal,
    epoch: [u8; 16],
    opened: std::time::Instant,
    /// Milliseconds after `opened` it was last used.
    seen: std::sync::atomic::AtomicU64,
}

impl Held {
    fn new(principal: Principal, epoch: [u8; 16]) -> Held {
        Held {
            principal,
            epoch,
            opened: std::time::Instant::now(),
            seen: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Past its lifetime, or idle past its idle time, now.
    fn expired(&self, lifetimes: &SessionLifetimes) -> bool {
        let age = self.opened.elapsed();
        let seen = Duration::from_millis(self.seen.load(Ordering::SeqCst));
        age > lifetimes.absolute || age.saturating_sub(seen) > lifetimes.idle
    }

    /// Used now.
    fn touch(&self) {
        let age = u64::try_from(self.opened.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.seen.store(age, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for PrincipalsInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrincipalsInner")
            .field("accounts", &self.accounts)
            .field("sessions", &"[redacted]")
            .field("kept", &self.kept.get().is_some())
            .field("lifetimes", &self.lifetimes)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// TRACK SEAM (second-host): how long a session lives
// ---------------------------------------------------------------------------

/// **How long a session lives** (the integrator's ruling 2 of 2026-10-20 on
/// track second-host; OWASP's Session Management Cheat Sheet asks both): an
/// idle timeout, past which a session unused is signed out, and an absolute
/// lifetime, past which it is signed out however it is used. A deployment's
/// settings (`PW_SESSION_IDLE`, `PW_SESSION_LIFETIME`, whole seconds),
/// [`SessionLifetimes::default`]'s where it says nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionLifetimes {
    pub(crate) idle: Duration,
    pub(crate) absolute: Duration,
}

/// A day.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

impl Default for SessionLifetimes {
    /// Fourteen days idle and sixty in all, as the integrator ruled.
    fn default() -> SessionLifetimes {
        SessionLifetimes {
            idle: DAY * 14,
            absolute: DAY * 60,
        }
    }
}

impl SessionLifetimes {
    /// **The lifetimes as the deployment states them**: `idle`, the value
    /// of `PW_SESSION_IDLE`, and `absolute`, of `PW_SESSION_LIFETIME`, each
    /// whole seconds, one at least, and its default where unset or empty.
    /// Anything else is refused, by name, so a host does not start on a
    /// setting it cannot read (as `PW_RENDER_SEATS` is).
    pub(crate) fn stated(
        idle: Option<&str>,
        absolute: Option<&str>,
    ) -> Result<SessionLifetimes, String> {
        let mut stated = SessionLifetimes::default();
        for (name, value, into) in [
            ("PW_SESSION_IDLE", idle, &mut stated.idle),
            ("PW_SESSION_LIFETIME", absolute, &mut stated.absolute),
        ] {
            if let Some(v) = value.filter(|v| !v.is_empty()) {
                *into = match v.parse::<u64>() {
                    Ok(n) if n >= 1 => Duration::from_secs(n),
                    _ => {
                        return Err(format!(
                            "{name}={v} is not a count of seconds, one at least"
                        ));
                    }
                };
            }
        }
        Ok(stated)
    }
}

// ---------------------------------------------------------------------------
// TRACK SEAM (second-host): sessions kept where the hosts share them
// ---------------------------------------------------------------------------

/// **A secret as it is kept**: its SHA-256, never the secret (ADR-0369). A
/// session's id (128 bits from the operating system) and a sign-in's state
/// (256) are past any guess, so a fast hash is enough: a copy of what is
/// kept opens no session and completes no sign-in. A password, which a
/// person chose, is the slow hash's (ADR-0258's Argon2id).
pub(crate) fn kept_hash(secret: &str) -> Vec<u8> {
    use sha2::Digest;
    sha2::Sha256::digest(secret.as_bytes()).to_vec()
}

/// **Where a deployment keeps its sessions and its sign-ins under way**
/// beyond one host's memory (ADR-0369, track `second-host`): the database
/// its hosts share, so that a host restarted, or another one, knows the
/// reader a sign-in made, and a sign-in begun on one host completes on
/// another. Each by its secret's [`kept_hash`].
pub(crate) trait Kept: Send + Sync {
    /// The principal of the session whose id hashes to `hash`, or none: none
    /// past `lifetimes` (the database's clock), and such a session signed
    /// out where it is kept. Its use recorded.
    fn session(
        &self,
        hash: &[u8],
        lifetimes: &SessionLifetimes,
    ) -> Result<Option<SessionIdentity>, String>;
    /// Current authority only: no idle touch, deletion or lifetime reset.
    fn inspect(
        &self,
        hash: &[u8],
        lifetimes: &SessionLifetimes,
    ) -> Result<Option<SessionIdentity>, String>;
    /// A session opened for `principal`, in a fresh generation.
    fn open(&self, hash: &[u8], principal: &Principal, epoch: &[u8; 16]) -> Result<(), String>;
    /// A session closed: no one from now on.
    fn close(&self, hash: &[u8]) -> Result<(), String>;
    /// A sign-in begun: its nonce and its PKCE verifier, which the token
    /// request needs as they are.
    fn begin_sign_in(&self, hash: &[u8], nonce: &str, verifier: &str) -> Result<(), String>;
    /// A sign-in's nonce and verifier, and how long ago it began, taken: it
    /// completes once.
    fn take_sign_in(&self, hash: &[u8]) -> Result<Option<(String, String, Duration)>, String>;
    /// The statements it ran, which a test counts.
    #[cfg(test)]
    fn ran(&self) -> u64 {
        0
    }
}

/// **A session open on a reader's behalf**, closed when it goes.
pub(crate) struct OnBehalf {
    principals: Principals,
    session: String,
}

/// **What the name of every session the platform opens on a reader's behalf
/// begins with** (NEXT 6c, step seven). Its name is no secret (a payment's
/// settling session is named by the payment, so that its interaction's
/// record keys it, ADR-0325), so no browser's session may carry it: a cookie
/// naming one is a new session of its own (`main.rs`'s `session_of`).
pub(crate) const ON_BEHALF: &str = "on-behalf:";

impl OnBehalf {
    /// The session's name: [`ON_BEHALF`], then what it was opened for.
    pub(crate) fn session(&self) -> &str {
        &self.session
    }
}

impl Drop for OnBehalf {
    fn drop(&mut self) {
        self.principals.close(&self.session);
    }
}

/// **Each session's principal**, shared by the identity and the data layer
/// that maps a session to its user (`DataLayer::identified_by`).
#[derive(Debug, Clone, Default)]
pub struct Principals(Arc<PrincipalsInner>);

// This synchronous host already scopes render snapshots and clock reads to a
// thread. Reader bindings follow that lifetime too. Child workers must receive
// their binding explicitly; a thread-local value is never inherited by spawn.
struct InvocationReader {
    principals: Principals,
    session: String,
    reader: ReaderIdentity,
}

thread_local! {
    static INVOCATION_READERS: std::cell::RefCell<Vec<InvocationReader>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

/// Fixed authority for one browser invocation. Platform invocations never
/// manufacture this from an on-behalf cookie.
#[derive(Clone)]
pub(crate) struct CommandActor {
    pub(crate) reader: ReaderIdentity,
    session: String,
    principals: Principals,
}

impl CommandActor {
    pub(crate) fn hash(&self) -> Vec<u8> {
        kept_hash(&self.session)
    }
    pub(crate) fn lifetimes(&self) -> SessionLifetimes {
        self.principals.lifetimes()
    }
    pub(crate) fn is_local(&self) -> bool {
        matches!(self.reader, ReaderIdentity::Guest) || self.principals.kept().is_none()
    }
    pub(crate) fn belongs_to(&self, source: &Arc<dyn Kept>) -> bool {
        self.principals
            .kept()
            .is_some_and(|own| Arc::ptr_eq(&own, source))
    }
    pub(crate) fn ensure_current(&self) -> Result<(), AuthorityError> {
        match self.principals.inspect_reader(&self.session) {
            Ok(current) if current == self.reader => Ok(()),
            Ok(_) => Err(AuthorityError::Changed),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn ensure_local_current(&self) -> Result<(), AuthorityError> {
        if self.is_local() && self.principals.cached_reader(&self.session) == self.reader {
            Ok(())
        } else {
            Err(AuthorityError::ChangedBeforeCommit)
        }
    }
}

impl Principals {
    pub(crate) fn command_gate(&self, session: &str) -> Arc<RwLock<()>> {
        let mut gates = self.0.gates.lock().expect("session authority gates");
        gates.retain(|_, weak| weak.strong_count() != 0);
        if let Some(gate) = gates.get(session).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(RwLock::new(()));
        gates.insert(session.to_string(), Arc::downgrade(&gate));
        gate
    }

    pub(crate) fn platform_reader(&self, session: &str) -> Result<ReaderIdentity, AuthorityError> {
        if !session.starts_with(ON_BEHALF) {
            return Err(AuthorityError::MissingPlatformContext);
        }
        if let Some(reader) = self.invocation_reader(session) {
            return Ok(reader);
        }
        self.0
            .sessions
            .read()
            .expect("principals")
            .get(session)
            .map(|held| {
                ReaderIdentity::Account(SessionIdentity {
                    principal: held.principal.clone(),
                    epoch: held.epoch,
                })
            })
            .ok_or(AuthorityError::MissingPlatformBinding)
    }

    pub(crate) fn command_actor(&self, session: &str) -> Result<CommandActor, AuthorityError> {
        if session.starts_with(ON_BEHALF) {
            return Err(AuthorityError::NotBrowserActor);
        }
        let capture = |reader| CommandActor {
            reader,
            session: session.into(),
            principals: self.clone(),
        };
        match self.invocation_reader(session) {
            Some(reader) => Ok(capture(reader)),
            None => self.inspect_reader(session).map(capture),
        }
    }

    /// Receipt identity includes the entire immutable actor, not only a cookie
    /// or epoch that an unsupported external row edit might accidentally reuse.
    pub(crate) fn receipt_reader(&self, session: &str) -> serde_json::Value {
        let reader = self.bound_reader(session);
        let principal = match &reader {
            ReaderIdentity::Account(identity) => serde_json::json!({
                "user":identity.principal.user, "handle":identity.principal.handle,
                "name":identity.principal.name, "issuer":identity.principal.issuer,
            }),
            _ => serde_json::Value::Null,
        };
        serde_json::json!({"partition":reader.partition(session), "principal":principal})
    }

    /// Execute synchronous reads as this fixed actor, including explicit
    /// Anonymous. This is not authority: delivery still inspects the source.
    /// No cache write, logout, or nested invocation changes this binding.
    pub(crate) fn with_reader<T>(
        &self,
        session: &str,
        reader: &ReaderIdentity,
        run: impl FnOnce() -> T,
    ) -> T {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                INVOCATION_READERS.with(|readers| {
                    readers.borrow_mut().pop();
                });
            }
        }
        INVOCATION_READERS.with(|readers| {
            readers.borrow_mut().push(InvocationReader {
                principals: self.clone(),
                session: session.to_string(),
                reader: reader.clone(),
            });
        });
        let _restore = Restore;
        run()
    }

    fn invocation_reader(&self, session: &str) -> Option<ReaderIdentity> {
        INVOCATION_READERS.with(|readers| {
            readers
                .borrow()
                .iter()
                .rev()
                .find(|bound| Arc::ptr_eq(&bound.principals.0, &self.0) && bound.session == session)
                .map(|bound| bound.reader.clone())
        })
    }

    fn revision(&self, session: &str) -> Arc<AdmissionClock> {
        let mut revisions = self.0.revisions.lock().expect("admission revisions");
        revisions.retain(|_, clock| clock.strong_count() != 0);
        if let Some(clock) = revisions.get(session).and_then(Weak::upgrade) {
            return clock;
        }
        let clock = Arc::new(Mutex::new(Arc::new(())));
        revisions.insert(session.to_string(), Arc::downgrade(&clock));
        clock
    }

    // Called while the sessions write lock is held. No network operation
    // holds this ticket's lock, and no ticket lock waits for a cache lock.
    fn advance_revision(&self, session: &str) {
        *self.revision(session).lock().expect("admission revision") = Arc::new(());
    }

    /// **The principal `session` is**, or none. A session no sign-in opened
    /// is its own guest in the guest model, and no one in the accounts one.
    pub fn of(&self, session: &str) -> Option<Principal> {
        if session.is_empty() {
            return None;
        }
        if let Some(reader) = self.invocation_reader(session) {
            return match reader {
                ReaderIdentity::Guest => Some(Principal::guest(session)),
                ReaderIdentity::Anonymous => None,
                ReaderIdentity::Account(identity) => Some(identity.principal),
            };
        }
        let lifetimes = self.lifetimes();
        let expired = match self.0.sessions.read().expect("principals").get(session) {
            Some(held) if !held.expired(&lifetimes) => {
                return Some(held.principal.clone());
            }
            Some(held) => Some(held.epoch),
            None => None,
        };
        // TRACK SEAM (second-host): **a session past its lifetimes is signed
        // out** (ADR-0369), here as where it is kept: it is no one, or its
        // own guest in the guest model, from now on.
        if let Some(epoch) = expired {
            let gate = self.command_gate(session);
            let _authority = gate.write().expect("session authority gate");
            let mut sessions = self.0.sessions.write().expect("principals");
            if sessions
                .get(session)
                .is_some_and(|held| held.epoch == epoch && held.expired(&lifetimes))
            {
                sessions.remove(session);
                self.advance_revision(session);
            }
        }
        (!self.0.accounts.load(Ordering::SeqCst)).then(|| Principal::guest(session))
    }

    /// The user `session` acts as: its principal's, or its guest's, who can
    /// write nothing where `requires SignedIn` is held.
    pub fn user_of(&self, session: &str) -> String {
        self.of(session)
            .map(|p| p.user)
            .unwrap_or_else(|| format!("u-{session}"))
    }

    fn open(&self, session: &str, principal: Principal) {
        self.open_identity(
            session,
            SessionIdentity {
                principal,
                epoch: random_bytes(),
            },
        );
    }

    fn open_identity(&self, session: &str, identity: SessionIdentity) {
        let gate = self.command_gate(session);
        let _authority = gate.write().expect("session authority gate");
        let mut sessions = self.0.sessions.write().expect("principals");
        Self::install(&mut sessions, session, identity);
        self.advance_revision(session);
    }

    fn install(sessions: &mut HashMap<String, Held>, session: &str, identity: SessionIdentity) {
        if let Some(held) = sessions.get_mut(session)
            && held.epoch == identity.epoch
        {
            held.principal = identity.principal;
            // An actual client admission, never a background inspection.
            held.touch();
        } else {
            sessions.insert(
                session.to_string(),
                Held::new(identity.principal, identity.epoch),
            );
        }
    }

    /// The reader used to begin a render, from its admitted local identity.
    /// It is a binding, not current authority; delivery must inspect again.
    pub(crate) fn bound_reader(&self, session: &str) -> ReaderIdentity {
        self.invocation_reader(session)
            .unwrap_or_else(|| self.cached_reader(session))
    }

    // Live local authority, deliberately bypassing any invocation binding.
    fn cached_reader(&self, session: &str) -> ReaderIdentity {
        if !self.0.accounts.load(Ordering::SeqCst) {
            return ReaderIdentity::Guest;
        }
        let lifetimes = self.lifetimes();
        self.0
            .sessions
            .read()
            .expect("principals")
            .get(session)
            .filter(|held| !held.expired(&lifetimes))
            .map(|held| {
                ReaderIdentity::Account(SessionIdentity {
                    principal: held.principal.clone(),
                    epoch: held.epoch,
                })
            })
            .unwrap_or(ReaderIdentity::Anonymous)
    }

    /// Read-only current authority. Neither long-held subscriptions nor
    /// internal rederivation renew client activity or the absolute deadline.
    pub(crate) fn inspect_reader(&self, session: &str) -> Result<ReaderIdentity, AuthorityError> {
        if session.starts_with(ON_BEHALF) {
            return Err(AuthorityError::NotBrowserReader);
        }
        if !self.0.accounts.load(Ordering::SeqCst) {
            return Ok(ReaderIdentity::Guest);
        }
        if let Some(kept) = self.0.kept.get() {
            return kept
                .inspect(&kept_hash(session), &self.lifetimes())
                .map_err(|_| AuthorityError::SourceUnavailable)
                .map(|identity| {
                    identity
                        .map(ReaderIdentity::Account)
                        .unwrap_or(ReaderIdentity::Anonymous)
                });
        }
        Ok(self.cached_reader(session))
    }

    /// TRACK SEAM (second-host): **how long its sessions live**, as the
    /// deployment states them (ADR-0369), set where the host starts.
    pub(crate) fn expire_after(&self, lifetimes: SessionLifetimes) {
        *self.0.lifetimes.write().expect("lifetimes") = lifetimes;
    }

    /// How long its sessions live.
    pub(crate) fn lifetimes(&self) -> SessionLifetimes {
        *self.0.lifetimes.read().expect("lifetimes")
    }

    fn close(&self, session: &str) {
        let gate = self.command_gate(session);
        let _authority = gate.write().expect("session authority gate");
        let mut sessions = self.0.sessions.write().expect("principals");
        sessions.remove(session);
        self.advance_revision(session);
    }

    /// TRACK SEAM (second-host): **sessions kept in `kept`** (ADR-0369),
    /// handed once, when the server is built on a layer that has one.
    pub(crate) fn keep_in(&self, kept: Arc<dyn Kept>) {
        let _ = self.0.kept.set(kept);
    }

    /// Where sessions are kept beyond this host's memory, where they are.
    pub(crate) fn kept(&self) -> Option<Arc<dyn Kept>> {
        self.0.kept.get().cloned()
    }

    /// TRACK SEAM (second-host): **`session` as it is kept, read once for a
    /// request** (ADR-0369): signed in on another host, or by this one
    /// before it restarted, it is signed in here; signed out anywhere, or
    /// past its lifetimes on the database's clock, it is no one here. One
    /// statement, in the accounts model where sessions are kept; nothing
    /// otherwise. A read that fails signs the request out: it is no one
    /// rather than someone it may no longer be.
    #[cfg(test)]
    pub fn refresh(&self, session: &str) {
        if let Err(why) = self.admit(session) {
            eprintln!("pw dev server: session admission refused: {why}");
        }
    }

    /// An explicit per-request result: stale/error results never borrow a
    /// newer cache entry. Network reads hold neither gate nor cache locks.
    pub(crate) fn admit(&self, session: &str) -> Result<ReaderIdentity, AuthorityError> {
        if session.starts_with(ON_BEHALF) {
            return Err(AuthorityError::NotBrowserAdmission);
        }
        let Some(kept) = self.0.kept.get() else {
            let gate = self.command_gate(session);
            let _authority = gate.write().expect("session authority gate");
            let lifetimes = self.lifetimes();
            let mut sessions = self.0.sessions.write().expect("principals");
            if sessions
                .get(session)
                .is_some_and(|held| held.expired(&lifetimes))
            {
                sessions.remove(session);
                self.advance_revision(session);
            } else if let Some(held) = sessions.get(session) {
                held.touch();
            }
            let reader = if !self.0.accounts.load(Ordering::SeqCst) {
                ReaderIdentity::Guest
            } else {
                sessions
                    .get(session)
                    .map(|held| {
                        ReaderIdentity::Account(SessionIdentity {
                            principal: held.principal.clone(),
                            epoch: held.epoch,
                        })
                    })
                    .unwrap_or(ReaderIdentity::Anonymous)
            };
            return Ok(reader);
        };
        if !self.0.accounts.load(Ordering::SeqCst) {
            return Ok(ReaderIdentity::Guest);
        }
        if session.is_empty() {
            return Ok(ReaderIdentity::Anonymous);
        }
        let revision = self.revision(session);
        let before = revision.lock().expect("admission revision").clone();
        let read = kept.session(&kept_hash(session), &self.lifetimes());
        let gate = self.command_gate(session);
        let _authority = gate.write().expect("session authority gate");
        let mut sessions = self.0.sessions.write().expect("principals");
        if !Arc::ptr_eq(&revision.lock().expect("admission revision"), &before) {
            return Err(AuthorityError::Superseded);
        }
        let result = match read {
            Ok(Some(identity)) => {
                Self::install(&mut sessions, session, identity.clone());
                Ok(ReaderIdentity::Account(identity))
            }
            Ok(None) => {
                sessions.remove(session);
                Ok(ReaderIdentity::Anonymous)
            }
            Err(_) => {
                sessions.remove(session);
                Err(AuthorityError::SourceUnavailable)
            }
        };
        self.advance_revision(session);
        result
    }

    /// **`session` signed in as `principal`**: kept first where sessions are
    /// kept, so that no host knows a sign-in the others cannot.
    fn sign_in(&self, session: &str, principal: Principal) -> Result<(), String> {
        let epoch = random_bytes();
        if let Some(kept) = self.0.kept.get() {
            kept.open(&kept_hash(session), &principal, &epoch)?;
        }
        self.open_identity(session, SessionIdentity { principal, epoch });
        Ok(())
    }

    /// **`session` signed out**: no one wherever it is kept, then here.
    fn sign_out(&self, session: &str) -> Result<(), String> {
        if let Some(kept) = self.0.kept.get()
            && !session.is_empty()
        {
            kept.close(&kept_hash(session))?;
        }
        self.close(session);
        Ok(())
    }

    /// **A session the platform opens on a reader's behalf** (NEXT 6c, step
    /// seven), named [`ON_BEHALF`] and then `name`, its principal issued by
    /// the `invoker`: a payment settled as the reader who began it, or a run
    /// as the user whose command scheduled it (the scheduled-commands
    /// track), its `current_user()` theirs. Never a browser's: no cookie is
    /// let name it, and it is closed when the guard goes, whatever the
    /// command answered.
    pub(crate) fn on_behalf(&self, invoker: &str, name: &str, user: &str) -> OnBehalf {
        let session = format!("{ON_BEHALF}{name}");
        self.open(
            &session,
            Principal {
                user: user.to_string(),
                handle: String::new(),
                name: String::new(),
                issuer: invoker.to_string(),
            },
        );
        OnBehalf {
            principals: self.clone(),
            session,
        }
    }

    fn set_accounts(&self, on: bool) {
        self.0.accounts.store(on, Ordering::SeqCst);
    }

    /// How many sessions are signed in.
    #[cfg(test)]
    pub fn signed_in(&self) -> usize {
        self.0.sessions.read().expect("principals").len()
    }

    /// TRACK SEAM (second-host): `session` held as `principal`, opened
    /// `opened_ago` and last used `seen_after` after it was opened, as a
    /// test ages a session in this host's memory alone.
    #[cfg(test)]
    pub(crate) fn held_for_test(
        &self,
        session: &str,
        principal: Principal,
        opened_ago: Duration,
        seen_after: Duration,
    ) {
        let gate = self.command_gate(session);
        let _authority = gate.write().expect("session authority gate");
        let epoch = self
            .0
            .sessions
            .read()
            .expect("principals")
            .get(session)
            .map(|held| held.epoch)
            .unwrap_or_else(random_bytes);
        let held = Held {
            principal,
            epoch,
            opened: std::time::Instant::now()
                .checked_sub(opened_ago)
                .expect("the host has run that long"),
            seen: std::sync::atomic::AtomicU64::new(
                u64::try_from(seen_after.as_millis()).unwrap_or(u64::MAX),
            ),
        };
        self.0
            .sessions
            .write()
            .expect("principals")
            .insert(session.to_string(), held);
    }

    /// When `session` was last used, after it was opened, as this host
    /// holds it; none where it holds it not.
    #[cfg(test)]
    pub(crate) fn seen_for_test(&self, session: &str) -> Option<Duration> {
        self.0
            .sessions
            .read()
            .expect("principals")
            .get(session)
            .map(|h| Duration::from_millis(h.seen.load(Ordering::SeqCst)))
    }
}

// ---------------------------------------------------------------------------
// The provider interface a deployment supplies
// ---------------------------------------------------------------------------

/// **An ID token's claims, as the provider adapter verified them**
/// (OpenID Connect Core §2). The adapter checks the token's signature
/// against the provider's keys; Pleris checks the protocol's own values
/// against what it sent ([`Identity`]'s callback, Core §3.1.3.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: Vec<String>,
    /// Seconds since the epoch.
    pub exp: u64,
    pub nonce: Option<String>,
    pub preferred_username: Option<String>,
    pub name: Option<String>,
}

/// **An OpenID provider, as a deployment supplies it** (ADR-0258): the
/// authorization code flow with PKCE (OpenID Connect Core §3.1, RFC 7636).
pub trait Provider: Send + Sync {
    /// The issuer identifier, which an ID token's `iss` must equal exactly.
    fn issuer(&self) -> String;
    /// This relying party's client id at the provider.
    fn client_id(&self) -> String;
    /// Where a browser is sent to authenticate: an absolute URL, or a path
    /// on this origin.
    fn authorization_endpoint(&self) -> String;
    /// **The token request** (Core §3.1.3.1, RFC 7636 §4.5): a code, the
    /// verifier its challenge was made from, and the redirect URI it was
    /// issued for, exchanged for the ID token's verified claims.
    fn exchange(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<Claims, String>;

    /// **A request to the provider itself**, where this server serves it
    /// (the development provider's pages). A deployment's provider is at its
    /// own origin, and serves nothing here.
    fn serve(&self, _method: &str, _route: &str, _query: &str, _body: &[u8]) -> Option<Reply> {
        None
    }
}

/// `PW_IDENTITY`'s name for the development provider.
pub const DEV_ACCOUNTS: &str = "dev-accounts";

/// **The authentication request a browser is sent with** (Core
/// §3.1.2.1, RFC 7636 §4.3): `scope openid`, `response_type code`, the
/// client and its redirect URI, `state`, `nonce`, and the S256 challenge.
/// `prompt=create` asks for an account to be made (OpenID Connect Prompt
/// Create 1.0).
pub fn authorization_url(
    provider: &dyn Provider,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    challenge: &str,
    sign_up: bool,
) -> String {
    let mut url = format!(
        "{}?response_type=code&scope=openid%20profile&client_id={}&redirect_uri={}\
         &state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        provider.authorization_endpoint(),
        encode(&provider.client_id()),
        encode(redirect_uri),
        encode(state),
        encode(nonce),
        encode(challenge),
    );
    if sign_up {
        url.push_str("&prompt=create");
    }
    url
}

// ---------------------------------------------------------------------------
// The identity: the relying party, and `requires`
// ---------------------------------------------------------------------------

/// Where a provider sends a browser back.
pub const CALLBACK: &str = "/sign-in/callback";

/// **A guest, signed in** (track `store-accounts`, ADR-0318; docs/PARALLEL.md,
/// Q2): the session the browser came with, which no sign-in had opened, the
/// session the sign-in opened in its place, and its principal. What a
/// deployment is told once the principal is opened and before the sign-in is
/// answered, so that what the guest held (a cart) is the user's before the
/// browser asks for a page. The identity knows nothing of what it is.
pub struct SignedIn<'a> {
    pub guest: &'a str,
    pub session: &'a str,
    pub principal: &'a Principal,
}
/// How long a sign-in may be in flight.
const SIGN_IN_LIFETIME: Duration = Duration::from_secs(600);

/// **Which principals a deployment has.**
pub enum Mode {
    /// Every session is its own guest principal: the development model this
    /// server held before the track (ADR-0115), for the store and the
    /// feed's existing tests.
    Guest,
    /// Sessions are signed in through a provider.
    Accounts(Accounts),
}

/// The accounts model: a provider, this client's redirect URI, and each
/// sign-in in flight.
pub struct Accounts {
    provider: Arc<dyn Provider>,
    redirect_uri: String,
    /// By `state`: the nonce and the code verifier, and when it started.
    pending: Mutex<HashMap<String, Pending>>,
}

struct Pending {
    nonce: String,
    verifier: String,
    started: Instant,
}

/// What the identity track keeps for one server.
pub struct Identity {
    principals: Principals,
    mode: RwLock<Mode>,
}

impl Default for Identity {
    fn default() -> Identity {
        Identity {
            principals: Principals::default(),
            mode: RwLock::new(Mode::Guest),
        }
    }
}

impl Identity {
    /// **Where a sign-in sends the browser, where that is not this origin**
    /// (`csp::Deployment::forms`): a deployment's provider, at its own; none
    /// in the guest model, or for the development provider, which this
    /// server serves at a path of its own.
    pub(crate) fn sign_in_origin(&self) -> Option<String> {
        let mode = self.mode.read().expect("mode");
        let Mode::Accounts(accounts) = &*mode else {
            return None;
        };
        crate::csp::origin_of(&accounts.provider.authorization_endpoint())
    }

    /// Each session's principal, which the data layer maps a session to
    /// its user by.
    pub fn principals(&self) -> Principals {
        self.principals.clone()
    }

    /// **The deployment's identity, as its configuration states it**:
    /// `PW_IDENTITY` is `guest` (or unset) for the guest model, or
    /// `dev-accounts` for the development provider. Neither runs outside a
    /// development deployment on this machine: a production deployment
    /// supplies a [`Provider`] of its own.
    pub fn configure(&self, deployment: &Deployment, choice: Option<&str>) -> Result<(), String> {
        match choice.unwrap_or("guest") {
            "" | "guest" => {
                if !deployment.development || !deployment.loopback() {
                    return Err(format!(
                        "the guest identity model signs every session in, and runs only in a \
                         development deployment on this machine, not at `{}`; a deployment \
                         supplies its own provider",
                        deployment.origin
                    ));
                }
                self.use_guests();
                Ok(())
            }
            DEV_ACCOUNTS => self.use_dev_accounts(deployment),
            other => Err(format!(
                "PW_IDENTITY is `{other}`: this server knows `guest` and `{DEV_ACCOUNTS}`"
            )),
        }
    }

    /// **Which model runs**, as the server's start says it.
    pub fn describe(&self) -> String {
        match &*self.mode.read().expect("mode") {
            Mode::Guest => "guest (development: every session is its own guest)".to_string(),
            Mode::Accounts(a) => format!("accounts, signed in through {}", a.provider.issuer()),
        }
    }

    /// The guest model.
    pub fn use_guests(&self) {
        *self.mode.write().expect("mode") = Mode::Guest;
        self.principals.set_accounts(false);
    }

    /// **Sign-in through `provider`**, sent back to `redirect_uri`: the
    /// accounts model. Every session signed in before is signed out.
    pub fn use_provider(&self, provider: Arc<dyn Provider>, redirect_uri: &str) {
        *self.mode.write().expect("mode") = Mode::Accounts(Accounts {
            provider,
            redirect_uri: redirect_uri.to_string(),
            pending: Mutex::new(HashMap::new()),
        });
        self.principals.set_accounts(true);
    }

    /// **`session` signed in as `user`, without the flow** (track
    /// `notifications`): the accounts model, a test's principal, so a test
    /// holds one user in two sessions and another user beside them. The
    /// flow itself is `tests/sign_in.rs`'s to hold. Kept where the hosts
    /// keep sessions, as a sign-in's is (ADR-0369): a request reads its
    /// session from there, and would find no one.
    #[cfg(test)]
    pub fn signed_in_for_test(&self, session: &str, user: &str, name: &str) {
        self.principals.set_accounts(true);
        self.principals
            .sign_in(
                session,
                Principal {
                    user: user.to_string(),
                    handle: format!("@{user}"),
                    name: name.to_string(),
                    issuer: "https://id.example.test".to_string(),
                },
            )
            .expect("signed in where sessions are kept");
    }

    /// **A request the identity track answers**, before any other route.
    /// `true` when it answered on `stream`; `false` leaves the request to
    /// the routes after it. `on_sign_in` is told of a guest that signed in
    /// ([`SignedIn`]), before the sign-in is answered.
    ///
    /// First, for every route, **a request that changes something is
    /// refused unless it comes from this origin** (charter §17.5's "CSRF for
    /// commands"): Fetch Metadata's `Sec-Fetch-Site`, and `Origin` where a
    /// browser sends no `Sec-Fetch-Site`, as Go 1.25's
    /// `http.CrossOriginProtection` decides it.
    #[allow(clippy::too_many_arguments)]
    pub fn answer(
        &self,
        method: &str,
        route: &str,
        headers: &str,
        session: &str,
        fresh: bool,
        body: &[u8],
        stream: &mut TcpStream,
        query: &str,
        on_sign_in: &dyn Fn(&SignedIn),
    ) -> bool {
        if let Err(why) = same_origin(method, headers) {
            Reply::text(403, &why).write(stream, session, fresh);
            return true;
        }
        let reply = match (method, route) {
            ("POST", "/sign-out") => Some(self.sign_out(session)),
            ("GET", "/sign-in") | ("GET", "/sign-up") => self.start(headers, route == "/sign-up"),
            ("GET", CALLBACK) => self.callback(headers, session, query, on_sign_in),
            _ => self.served(method, route, query, body),
        };
        match reply {
            Some(reply) => {
                reply.write(stream, session, fresh);
                true
            }
            None => false,
        }
    }

    /// A request to a provider this server also serves (the development
    /// one), answered by it.
    fn served(&self, method: &str, route: &str, query: &str, body: &[u8]) -> Option<Reply> {
        let provider = match &*self.mode.read().expect("mode") {
            Mode::Accounts(a) => a.provider.clone(),
            Mode::Guest => return None,
        };
        provider.serve(method, route, query, body)
    }

    /// **A sign-in started** (Core §3.1.2.1): a fresh `state`, `nonce` and
    /// code verifier, kept here by the state; the state also in a cookie of
    /// this browser's, so that the callback is this browser's sign-in and
    /// not one an attacker started and sent it to (login CSRF); and the
    /// browser sent to the provider. None in the guest model.
    fn start(&self, _headers: &str, sign_up: bool) -> Option<Reply> {
        let mode = self.mode.read().expect("mode");
        let Mode::Accounts(accounts) = &*mode else {
            return None;
        };
        let (state, nonce, verifier) = (token(), token(), token());
        let challenge = s256(&verifier);
        // TRACK SEAM (second-host): kept where the hosts share it, so that
        // the callback completes it on whichever host it reaches.
        if let Some(kept) = self.principals.kept() {
            if let Err(why) = kept.begin_sign_in(&kept_hash(&state), &nonce, &verifier) {
                return Some(Reply::page(
                    503,
                    "Sign-in not started",
                    &format!(
                        "<p role=\"alert\">Sign-in could not start: {}.</p>",
                        escape(&why)
                    ),
                ));
            }
        } else {
            let mut pending = accounts.pending.lock().expect("pending");
            pending.retain(|_, p| p.started.elapsed() < SIGN_IN_LIFETIME);
            pending.insert(
                state.clone(),
                Pending {
                    nonce: nonce.clone(),
                    verifier,
                    started: Instant::now(),
                },
            );
        }
        let url = authorization_url(
            &*accounts.provider,
            &accounts.redirect_uri,
            &state,
            &nonce,
            &challenge,
            sign_up,
        );
        Some(Reply::redirect(
            &url,
            vec![cookie_line(
                SIGN_IN_COOKIE,
                &state,
                "/sign-in",
                Some(SIGN_IN_LIFETIME.as_secs()),
                *SECURE_COOKIES.get().unwrap_or(&false),
            )],
        ))
    }

    /// **The provider's answer** (Core §3.1.2.5, §3.1.3.7): the state this
    /// browser started, once; the code exchanged with its verifier; the ID
    /// token's issuer, audience, expiry and nonce checked; and then a NEW
    /// session for the principal, the one the browser came with forgotten
    /// (OWASP: "renew the session ID after any privilege level change", the
    /// defence against session fixation).
    fn callback(
        &self,
        headers: &str,
        session: &str,
        query: &str,
        on_sign_in: &dyn Fn(&SignedIn),
    ) -> Option<Reply> {
        let mode = self.mode.read().expect("mode");
        let Mode::Accounts(accounts) = &*mode else {
            return None;
        };
        let fields = parse_form(query);
        let refused = |why: &str| {
            Reply::page(
                400,
                "Sign-in not completed",
                &format!(
                    "<p role=\"alert\">Sign-in was not completed: {}.</p>\
                     <p><a href=\"/sign-in\">Try again</a></p>",
                    escape(why)
                ),
            )
        };
        if let Some(error) = fields.get("error") {
            return Some(refused(&format!("the provider answered `{error}`")));
        }
        let state = fields.get("state").cloned().unwrap_or_default();
        let bound = cookie(headers, SIGN_IN_COOKIE).unwrap_or_default();
        if state.is_empty() || !same(&state, &bound) {
            return Some(refused("it was not started by this browser"));
        }
        // TRACK SEAM (second-host): taken from where the hosts keep it,
        // once, whichever host began it.
        let taken = match self.principals.kept() {
            Some(kept) => match kept.take_sign_in(&kept_hash(&state)) {
                Ok(taken) => taken,
                Err(why) => return Some(refused(&why)),
            },
            None => accounts
                .pending
                .lock()
                .expect("pending")
                .remove(&state)
                .map(|p| (p.nonce, p.verifier, p.started.elapsed())),
        };
        let Some((nonce, verifier, age)) = taken else {
            return Some(refused("it was already completed or is unknown"));
        };
        let pending = Pending {
            nonce,
            verifier,
            started: Instant::now(),
        };
        if age > SIGN_IN_LIFETIME {
            return Some(refused("it took too long"));
        }
        let code = fields.get("code").cloned().unwrap_or_default();
        let claims =
            match accounts
                .provider
                .exchange(&code, &pending.verifier, &accounts.redirect_uri)
            {
                Ok(c) => c,
                Err(why) => return Some(refused(&why)),
            };
        if let Err(why) = check_claims(&claims, &*accounts.provider, &pending.nonce) {
            return Some(refused(&why));
        }
        let principal = Principal {
            handle: claims
                .preferred_username
                .clone()
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| format!("@{}", claims.sub)),
            name: claims.name.clone().unwrap_or_else(|| claims.sub.clone()),
            user: claims.sub,
            issuer: claims.iss,
        };
        let rotated = new_session_id();
        // A guest's, where no sign-in opened the session it came with: a
        // signed-in session that signs in again is signed out, and what it
        // held is its user's, never the next one's.
        let was_guest = self.principals.of(session).is_none_or(|p| p.is_guest());
        // TRACK SEAM (second-host): kept where the hosts share them, before
        // the browser is answered (ADR-0369).
        if let Err(why) = self
            .principals
            .sign_out(session)
            .and_then(|()| self.principals.sign_in(&rotated, principal.clone()))
        {
            return Some(refused(&format!("the session was not kept: {why}")));
        }
        // TRACK SEAM (store-accounts): the deployment told, before the
        // browser is answered.
        if was_guest && !session.is_empty() {
            on_sign_in(&SignedIn {
                guest: session,
                session: &rotated,
                principal: &principal,
            });
        }
        let secure = *SECURE_COOKIES.get().unwrap_or(&false);
        Some(Reply::redirect(
            "/",
            vec![
                cookie_line(SESSION_COOKIE, &rotated, "/", None, secure),
                cookie_line(SIGN_IN_COOKIE, "", "/sign-in", Some(0), secure),
            ],
        ))
    }

    /// **Signed out**: the session's principal forgotten, so its id, if
    /// replayed, is no one; and a new session, so the browser is not left
    /// holding the old one.
    fn sign_out(&self, session: &str) -> Reply {
        // TRACK SEAM (second-host): no one on any host (ADR-0369); a
        // sign-out not kept is said, and the browser keeps its session.
        if let Err(why) = self.principals.sign_out(session) {
            return Reply::page(
                503,
                "Sign-out not completed",
                &format!(
                    "<p role=\"alert\">Sign-out was not completed: {}.</p>",
                    escape(&why)
                ),
            );
        }
        let rotated = new_session_id();
        Reply::redirect(
            "/",
            vec![cookie_line(
                SESSION_COOKIE,
                &rotated,
                "/",
                None,
                *SECURE_COOKIES.get().unwrap_or(&false),
            )],
        )
    }

    /// **What `requires predicate` is told for `session`** (ADR-0115):
    /// `Ok(true)` where the session's principal holds it, `Ok(false)` where
    /// it does not, and an error for a predicate this deployment does not
    /// know, so that an unknown predicate cannot run.
    ///
    /// - `SignedIn`: the session has a principal.
    /// - `OwnsPost(post)`: the session's principal wrote `post`, read through
    ///   the command's own operations (`feed:data/posts#thread`), so within
    ///   the command's transaction: the post it deletes is the post it was
    ///   shown to own.
    /// - TRACK SEAM (messages): `MayMessage(to)`: the session's principal may
    ///   message `to` (`messages.rs`), read the same way.
    pub fn requires(
        &self,
        predicate: &str,
        bound: &[&Val],
        session: &str,
        host: &std::collections::BTreeMap<String, HostFn>,
    ) -> Result<bool, String> {
        let held = self.holds(predicate, bound, session, host);
        if held == Ok(false) {
            REFUSED.with(|r| {
                r.borrow_mut().get_or_insert_with(|| predicate.to_string());
            });
        }
        held
    }

    /// **What a page asks of its reader** (ADR-0307): whether `predicate`,
    /// one with no parameters, holds for `session`'s principal, answered as
    /// `requires` answers it, and told to no refusal: the page is rendered
    /// with the answer, before any press.
    pub fn answers(&self, predicate: &str, session: &str) -> Result<bool, String> {
        self.holds(predicate, &[], session, &std::collections::BTreeMap::new())
    }

    fn holds(
        &self,
        predicate: &str,
        bound: &[&Val],
        session: &str,
        host: &std::collections::BTreeMap<String, HostFn>,
    ) -> Result<bool, String> {
        match predicate {
            "SignedIn" => Ok(self.principals.of(session).is_some()),
            "OwnsPost" => {
                let Some(principal) = self.principals.of(session) else {
                    return Ok(false);
                };
                let [Val::String(post)] = bound else {
                    return Err(format!(
                        "OwnsPost takes one post id, and was given {bound:?}"
                    ));
                };
                let thread = host
                    .get("feed:data/posts#thread")
                    .ok_or("OwnsPost reads a post's author through feed:data/posts#thread")?;
                match thread(&[Val::String(post.clone())])?.as_slice() {
                    [Val::Result(Ok(Some(found)))] => Ok(author_of(found) == Some(&principal.user)),
                    [Val::Result(Err(_))] => Ok(false),
                    other => Err(format!("posts#thread answered {other:?}")),
                }
            }
            // TRACK SEAM (messages): `MayMessage(to)`, the session's principal
            // may message `to`, read through the command's own operations, so
            // within its transaction, as `OwnsPost(post)` is.
            "MayMessage" => crate::messages::may_message(self.principals.of(session), bound, host),
            // TRACK SEAM (reviews): `MayRate(store)`, the session's
            // principal's latest order delivered, from `store` and not yet
            // rated by them, read through the command's own operations, so
            // within its transaction, as `OwnsPost(post)` is.
            "MayRate" => crate::reviews::may_rate(self.principals.of(session), bound, host),
            other => Err(format!(
                "the development deployment has no authorization predicate `{other}`"
            )),
        }
    }
}

/// **What a reader is told when a predicate refuses** (ADR-0302), in this
/// deployment's words: a program may declare its own (`predicate SignedIn
/// says "…"`), told in their place. `None` for a predicate this deployment
/// cannot evaluate, which the host refuses to serve a program requiring.
/// Each names the need, never what the predicate read: `OwnsPost` does not
/// say whose the post is.
pub fn says(predicate: &str) -> Option<&'static str> {
    Some(match predicate {
        "SignedIn" => "Sign in to do this.",
        "OwnsPost" => "Only its author can do this.",
        // TRACK SEAM (messages).
        "MayMessage" => "You can't send this person a message.",
        // TRACK SEAM (reviews).
        "MayRate" => "You can rate each store of your latest order once, after it's delivered.",
        _ => return None,
    })
}

thread_local! {
    /// **The predicate `requires` refused**, on the thread that ran the
    /// command: the command's route answers it as its own case (403), and
    /// takes it before the command runs so that none is left from another.
    static REFUSED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// **The predicate the last command on this thread was refused by**, taken.
pub fn take_refusal() -> Option<String> {
    REFUSED.with(|r| r.borrow_mut().take())
}

/// A post record's `author.id`.
fn author_of(post: &Val) -> Option<&String> {
    let Val::Record(fields) = post else {
        return None;
    };
    let (_, Val::Record(author)) = fields.iter().find(|(k, _)| k == "author")? else {
        return None;
    };
    match author.iter().find(|(k, _)| k == "id")? {
        (_, Val::String(id)) => Some(id),
        _ => None,
    }
}

/// **An ID token's claims, checked against what was sent** (Core
/// §3.1.3.7): the issuer exactly, this client in the audience, not expired,
/// and the nonce this sign-in sent.
pub fn check_claims(claims: &Claims, provider: &dyn Provider, nonce: &str) -> Result<(), String> {
    if claims.iss != provider.issuer() {
        return Err(format!("the token's issuer is `{}`", claims.iss));
    }
    if !claims.aud.iter().any(|a| *a == provider.client_id()) {
        return Err("the token is not for this client".to_string());
    }
    if claims.exp <= now_secs() {
        return Err("the token expired".to_string());
    }
    match &claims.nonce {
        Some(n) if same(n, nonce) => {}
        _ => return Err("the token's nonce is not this sign-in's".to_string()),
    }
    if claims.sub.is_empty() {
        return Err("the token names no subject".to_string());
    }
    Ok(())
}

/// **Does an unsafe request come from this origin?** (OWASP's CSRF
/// Prevention Cheat Sheet, "Fetch Metadata headers", with its "mandatory"
/// fallback to `Origin`; the decision Go 1.25's `CrossOriginProtection`
/// makes.) GET, HEAD and OPTIONS change nothing and pass. Otherwise:
/// `Sec-Fetch-Site` must be `same-origin` or `none` where a browser sends it;
/// where it sends none, `Origin`'s host must be the request's `Host`; and a
/// request with neither came from no browser, which carries no one's cookie
/// unasked, and passes.
pub fn same_origin(method: &str, headers: &str) -> Result<(), String> {
    if matches!(method, "GET" | "HEAD" | "OPTIONS") {
        return Ok(());
    }
    if let Some(site) = header(headers, "sec-fetch-site") {
        return match site.to_ascii_lowercase().as_str() {
            "same-origin" | "none" => Ok(()),
            other => Err(format!(
                "a {other} request is refused: it changes something"
            )),
        };
    }
    if let Some(origin) = header(headers, "origin") {
        let host = header(headers, "host").unwrap_or_default();
        let from = origin.split_once("://").map(|(_, h)| h).unwrap_or("");
        if from.is_empty() || !from.eq_ignore_ascii_case(host) {
            return Err(format!(
                "a request from `{origin}` to `{host}` is refused: it changes something"
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP, as the track's routes need it
// ---------------------------------------------------------------------------

/// A header's value, by its name in any case.
pub fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A cookie's value, by its name.
pub fn cookie(headers: &str, name: &str) -> Option<String> {
    headers
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("cookie").then_some(v)
        })
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
}

/// `application/x-www-form-urlencoded`, or a query string, as pairs.
pub fn parse_form(text: &str) -> HashMap<String, String> {
    text.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (digit(bytes[i + 1]), digit(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encoding of everything but RFC 3986's unreserved characters.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Text safe in HTML's text and in a quoted attribute.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// **An answer of the track's**: a page, a redirect or text, and the
/// cookies it sets. Every one is kept by no cache (ADR-0184).
pub struct Reply {
    status: u16,
    location: Option<String>,
    cookies: Vec<String>,
    mime: &'static str,
    body: String,
}

impl Reply {
    /// A page of this server's own: a sign-in that was not completed.
    pub(crate) fn page(status: u16, title: &str, main: &str) -> Reply {
        Reply::html(status, title, "", main)
    }

    /// **A page of the development provider's**, which says first that it
    /// is not production.
    pub(crate) fn provider_page(status: u16, title: &str, main: &str) -> Reply {
        Reply::html(
            status,
            title,
            "<p id=\"not-production\" role=\"note\">Development identity provider: not \
             for production. It runs only in a development deployment on this machine, and \
             keeps its accounts in memory.</p>",
            main,
        )
    }

    fn html(status: u16, title: &str, banner: &str, main: &str) -> Reply {
        Reply {
            status,
            location: None,
            cookies: Vec::new(),
            mime: "text/html; charset=utf-8",
            body: format!(
                "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
                 <title>{0}</title></head><body>{banner}\
                 <main><h1>{0}</h1>{main}</main></body></html>",
                escape(title)
            ),
        }
    }

    pub(crate) fn redirect(location: &str, cookies: Vec<String>) -> Reply {
        Reply {
            status: 303,
            location: Some(location.to_string()),
            cookies,
            mime: "text/plain; charset=utf-8",
            body: String::new(),
        }
    }

    fn text(status: u16, body: &str) -> Reply {
        Reply {
            status,
            location: None,
            cookies: Vec::new(),
            mime: "text/plain; charset=utf-8",
            body: body.to_string(),
        }
    }

    /// The response as bytes. A fresh session's cookie goes with it unless
    /// the reply sets the session itself.
    pub(crate) fn bytes(&self, session: &str, fresh: bool) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            303 => "See Other",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            405 => "Method Not Allowed",
            409 => "Conflict",
            _ => "",
        };
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\ncontent-type: {}\r\ncontent-length: {}\r\n{}{}",
            self.status,
            self.mime,
            self.body.len(),
            super::NOSNIFF,
            super::PRIVATE
        );
        if let Some(location) = &self.location {
            head.push_str(&format!("location: {location}\r\n"));
        }
        // An identity's page runs nothing (`csp::BARE`).
        if self.mime.starts_with("text/html") {
            head.push_str(&format!(
                "content-security-policy: {}\r\n",
                crate::csp::BARE
            ));
        }
        let sets_session = self
            .cookies
            .iter()
            .any(|c| c.starts_with(&format!("set-cookie: {SESSION_COOKIE}=")));
        if fresh && !sets_session {
            head.push_str(&session_cookie(session));
        }
        for c in &self.cookies {
            head.push_str(c);
        }
        head.push_str("connection: close\r\n\r\n");
        let mut out = head.into_bytes();
        out.extend_from_slice(self.body.as_bytes());
        out
    }

    fn write(&self, stream: &mut TcpStream, session: &str, fresh: bool) {
        let _ = stream.write_all(&self.bytes(session, fresh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PausedRevocation {
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
        fails: bool,
    }

    impl Kept for PausedRevocation {
        fn session(
            &self,
            _hash: &[u8],
            _life: &SessionLifetimes,
        ) -> Result<Option<SessionIdentity>, String> {
            self.entered.wait();
            self.release.wait();
            if self.fails {
                Err("authority read unavailable".into())
            } else {
                Ok(None)
            }
        }
        fn inspect(
            &self,
            _hash: &[u8],
            _life: &SessionLifetimes,
        ) -> Result<Option<SessionIdentity>, String> {
            Ok(None)
        }
        fn open(&self, _hash: &[u8], _p: &Principal, _epoch: &[u8; 16]) -> Result<(), String> {
            Ok(())
        }
        fn close(&self, _hash: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn begin_sign_in(&self, _hash: &[u8], _nonce: &str, _verifier: &str) -> Result<(), String> {
            Ok(())
        }
        fn take_sign_in(&self, _hash: &[u8]) -> Result<Option<(String, String, Duration)>, String> {
            Ok(None)
        }
    }

    fn an_unrelated_admission_cannot_cancel_this_revocation(fails: bool) {
        let principals = Principals::default();
        principals.set_accounts(true);
        principals.open("target", Principal::guest("target"));
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        principals.keep_in(Arc::new(PausedRevocation {
            entered: entered.clone(),
            release: release.clone(),
            fails,
        }));
        std::thread::scope(|scope| {
            let request = scope.spawn(|| principals.refresh("target"));
            entered.wait();
            principals.open("unrelated", Principal::guest("unrelated"));
            release.wait();
            request
                .join()
                .expect("authority request completed normally");
        });
        assert!(
            principals.of("target").is_none(),
            "an unrelated session cannot suppress authoritative revocation/refusal"
        );
        assert!(
            principals.of("unrelated").is_some(),
            "the other session stays live"
        );
    }

    #[test]
    fn an_unrelated_admission_does_not_suppress_a_revoked_session() {
        an_unrelated_admission_cannot_cancel_this_revocation(false);
    }

    #[test]
    fn an_unrelated_admission_does_not_suppress_an_authority_read_failure() {
        an_unrelated_admission_cannot_cancel_this_revocation(true);
    }

    use std::collections::BTreeMap;

    /// The development model the track started from, as `Server::run` held
    /// it inline before ADR-0253, is the guest model: a session is signed in,
    /// no session is not, and a predicate the deployment does not know is an
    /// error.
    #[test]
    fn requires_starts_as_the_development_model() {
        let identity = Identity::default();
        let host = BTreeMap::new();
        assert_eq!(identity.requires("SignedIn", &[], "s-1", &host), Ok(true));
        assert_eq!(identity.requires("SignedIn", &[], "", &host), Ok(false));
        assert!(identity.requires("Admin", &[], "s-1", &host).is_err());
    }

    /// **PKCE's S256 is RFC 7636's** (Appendix B): its worked example's
    /// verifier gives its challenge.
    #[test]
    fn s256_is_rfc_7636s_worked_example() {
        assert_eq!(
            s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let verifier = token();
        assert!(is_verifier(&verifier), "{verifier}");
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, token());
    }

    /// **`PW_ORIGIN` is an origin** (ADR-0311): a scheme, a host and a
    /// port, and nothing after them, which every absolute address the host
    /// makes is written with.
    #[test]
    fn an_origin_is_a_scheme_a_host_and_a_port() {
        for (given, read) in [
            ("https://kiokun.com", "https://kiokun.com"),
            ("https://kiokun.com/", "https://kiokun.com"),
            ("http://127.0.0.1:3143", "http://127.0.0.1:3143"),
            ("http://[::1]:3000", "http://[::1]:3000"),
        ] {
            assert_eq!(
                Deployment::origin_checked(given).as_deref(),
                Ok(read),
                "{given}"
            );
        }
        for refused in [
            "kiokun.com",
            "ftp://kiokun.com",
            "https://",
            "https://kiokun.com/app",
            "https://kiokun.com?a=1",
            "https://kiokun.com#top",
            "https://user:pass@kiokun.com",
        ] {
            assert!(Deployment::origin_checked(refused).is_err(), "{refused}");
        }
    }

    /// **A session's cookie**: HttpOnly and SameSite=Lax always, Secure
    /// where it is asked; a deployment asks it everywhere but plain HTTP on
    /// this machine.
    #[test]
    fn a_cookie_is_http_only_and_secure_outside_this_machine() {
        let line = cookie_line("pw-session", "s-1", "/", None, false);
        assert_eq!(
            line,
            "set-cookie: pw-session=s-1; Path=/; HttpOnly; SameSite=Lax\r\n"
        );
        let line = cookie_line("pw-session", "", "/", Some(0), true);
        assert!(line.ends_with("; Max-Age=0; Secure\r\n"), "{line}");
        let at = |origin: &str| Deployment {
            development: true,
            origin: origin.to_string(),
        };
        for local in [
            "http://127.0.0.1:3143",
            "http://localhost:8080",
            "http://feed.localhost",
            "http://[::1]:3000",
        ] {
            assert!(at(local).loopback(), "{local}");
            assert!(!at(local).secure_cookies(), "{local}");
        }
        assert!(at("https://localhost").secure_cookies());
        for public in [
            "https://feed.example.com",
            "http://192.168.1.5:3143",
            "http://localhost.example.com",
        ] {
            assert!(!at(public).loopback(), "{public}");
            assert!(at(public).secure_cookies(), "{public}");
        }
    }

    /// **A development identity runs only in a development deployment on
    /// this machine**: the guest model is refused in production and at a
    /// public origin, and an identity the server does not know is refused,
    /// not guessed.
    #[test]
    fn the_guest_model_runs_only_in_development_on_this_machine() {
        let identity = Identity::default();
        let production = Deployment {
            development: false,
            origin: "http://127.0.0.1:3143".to_string(),
        };
        assert!(identity.configure(&production, None).is_err());
        let public = Deployment {
            development: true,
            origin: "https://feed.example.com".to_string(),
        };
        assert!(identity.configure(&public, Some("guest")).is_err());
        assert!(
            identity
                .configure(&Deployment::development(3143), Some("ldap"))
                .is_err()
        );
        assert!(
            identity
                .configure(&Deployment::development(3143), None)
                .is_ok()
        );
        assert!(identity.describe().starts_with("guest"));
    }

    /// **A request that changes something comes from this origin**, as Go
    /// 1.25's `CrossOriginProtection` decides: Sec-Fetch-Site where a
    /// browser sends it, `Origin` against `Host` where it does not, and a
    /// request with neither is no browser's.
    #[test]
    fn an_unsafe_request_from_another_origin_is_refused() {
        let ok = |method: &str, headers: &str| same_origin(method, headers).is_ok();
        assert!(ok("GET", "Sec-Fetch-Site: cross-site\r\n"));
        assert!(ok("POST", "Sec-Fetch-Site: same-origin\r\nHost: t\r\n"));
        assert!(ok("POST", "Sec-Fetch-Site: none\r\n"));
        assert!(!ok("POST", "Sec-Fetch-Site: cross-site\r\n"));
        assert!(!ok("POST", "Sec-Fetch-Site: same-site\r\n"));
        // Sec-Fetch-Site decides where it is sent, whatever Origin says.
        assert!(!ok(
            "POST",
            "Sec-Fetch-Site: cross-site\r\nOrigin: http://t\r\nHost: t\r\n"
        ));
        assert!(ok("POST", "Origin: http://t:3143\r\nHost: t:3143\r\n"));
        assert!(!ok("POST", "Origin: http://evil.test\r\nHost: t:3143\r\n"));
        assert!(!ok(
            "POST",
            "Origin: http://t:3143.evil.test\r\nHost: t:3143\r\n"
        ));
        assert!(!ok("POST", "Origin: null\r\nHost: t\r\n"));
        assert!(!ok("DELETE", "origin: http://evil.test\r\nhost: t\r\n"));
        assert!(ok("POST", "Host: t\r\n"));
    }

    /// **An ID token is held to the sign-in that asked for it** (OpenID
    /// Connect Core §3.1.3.7).
    #[test]
    fn a_token_is_held_to_its_issuer_audience_expiry_and_nonce() {
        struct P;
        impl Provider for P {
            fn issuer(&self) -> String {
                "https://id.test".into()
            }
            fn client_id(&self) -> String {
                "c".into()
            }
            fn authorization_endpoint(&self) -> String {
                "https://id.test/authorize".into()
            }
            fn exchange(&self, _: &str, _: &str, _: &str) -> Result<Claims, String> {
                Err("unused".into())
            }
        }
        let good = Claims {
            iss: "https://id.test".into(),
            sub: "u1".into(),
            aud: vec!["other".into(), "c".into()],
            exp: now_secs() + 60,
            nonce: Some("n".into()),
            preferred_username: None,
            name: None,
        };
        assert_eq!(check_claims(&good, &P, "n"), Ok(()));
        let wrong: [fn(&mut Claims); 6] = [
            |c| c.iss = "https://id.test/".into(),
            |c| c.aud = vec!["other".into()],
            |c| c.exp = now_secs() - 1,
            |c| c.nonce = None,
            |c| c.nonce = Some("m".into()),
            |c| c.sub = String::new(),
        ];
        for (n, change) in wrong.iter().enumerate() {
            let mut claims = good.clone();
            change(&mut claims);
            assert!(check_claims(&claims, &P, "n").is_err(), "change {n}");
        }
    }

    /// **A fresh session's id is 128 random bits**, and a form's fields are
    /// read as a browser encodes them.
    #[test]
    fn ids_are_random_and_forms_are_decoded() {
        let (a, b) = (new_session_id(), new_session_id());
        assert_ne!(a, b);
        assert!(a.starts_with("s-") && a.len() == 34, "{a}");
        assert!(a[2..].bytes().all(|c| c.is_ascii_hexdigit()), "{a}");
        let form = parse_form("handle=ada&name=Ada+L%C3%B6velace&bad=%zz&cut=%4");
        assert_eq!(form["handle"], "ada");
        assert_eq!(form["name"], "Ada Lövelace");
        assert_eq!(form["bad"], "%zz");
        assert_eq!(form["cut"], "%4");
        assert_eq!(encode("a b/c~"), "a%20b%2Fc~");
        assert_eq!(
            escape("<a href=\"x\">'&"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;"
        );
    }

    /// **The refusal a command's route answers** is the first predicate
    /// `requires` refused on this thread, taken once.
    #[test]
    fn a_refusal_is_taken_once() {
        let identity = Identity::default();
        identity.principals.set_accounts(true);
        let host = BTreeMap::new();
        let _ = take_refusal();
        assert_eq!(identity.requires("SignedIn", &[], "s-1", &host), Ok(false));
        assert_eq!(identity.requires("OwnsPost", &[], "s-1", &host), Ok(false));
        assert_eq!(take_refusal().as_deref(), Some("SignedIn"));
        assert_eq!(take_refusal(), None);
    }
}

#[cfg(test)]
mod invocation_tests {
    use super::*;

    #[test]
    fn a_read_keeps_its_actor_while_live_inspection_observes_revocation() {
        let p = Principals::default();
        p.set_accounts(true);
        p.open("reader", Principal::guest("alice"));
        let bound = p.bound_reader("reader");
        p.with_reader("reader", &bound, || {
            p.close("reader");
            assert_eq!(p.of("reader"), Some(Principal::guest("alice")));
            assert_eq!(p.bound_reader("reader"), bound);
            assert_eq!(
                p.inspect_reader("reader").expect("local inspection"),
                ReaderIdentity::Anonymous
            );
        });
        assert!(p.of("reader").is_none());
        assert_eq!(p.bound_reader("reader"), ReaderIdentity::Anonymous);
    }

    #[test]
    fn anonymous_and_nested_bindings_are_scoped_to_their_store_and_cookie() {
        let p = Principals::default();
        let other = Principals::default();
        for store in [&p, &other] {
            store.set_accounts(true);
            store.open("reader", Principal::guest("alice"));
            store.open("other-cookie", Principal::guest("bob"));
        }
        let alice = p.bound_reader("reader");
        p.with_reader("reader", &ReaderIdentity::Anonymous, || {
            assert!(p.of("reader").is_none());
            assert_eq!(p.user_of("other-cookie"), "u-bob");
            assert_eq!(other.user_of("reader"), "u-alice");
            p.with_reader("reader", &alice, || {
                assert_eq!(p.user_of("reader"), "u-alice")
            });
            assert!(
                p.of("reader").is_none(),
                "the explicit anonymous outer binding is restored"
            );
        });
        assert_eq!(p.user_of("reader"), "u-alice");
    }

    #[test]
    fn authority_source_failures_are_bounded_without_borrowing_cached_identity() {
        struct Source {
            unavailable: AtomicBool,
            identity: SessionIdentity,
        }
        impl Kept for Source {
            fn session(
                &self,
                hash: &[u8],
                life: &SessionLifetimes,
            ) -> Result<Option<SessionIdentity>, String> {
                self.inspect(hash, life)
            }
            fn inspect(
                &self,
                _: &[u8],
                _: &SessionLifetimes,
            ) -> Result<Option<SessionIdentity>, String> {
                if self.unavailable.load(Ordering::SeqCst) {
                    Err("private-backend-fixture-do-not-log".into())
                } else {
                    Ok(Some(self.identity.clone()))
                }
            }
            fn open(&self, _: &[u8], _: &Principal, _: &[u8; 16]) -> Result<(), String> {
                Ok(())
            }
            fn close(&self, _: &[u8]) -> Result<(), String> {
                Ok(())
            }
            fn begin_sign_in(&self, _: &[u8], _: &str, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn take_sign_in(&self, _: &[u8]) -> Result<Option<(String, String, Duration)>, String> {
                Ok(None)
            }
        }
        let p = Principals::default();
        p.set_accounts(true);
        p.open("reader", Principal::guest("fixture"));
        let bound = p.bound_reader("reader");
        let actor = p.command_actor("reader").expect("admitted neighbor");
        actor.ensure_current().expect("current neighbor");
        let ReaderIdentity::Account(identity) = bound.clone() else {
            panic!("account fixture")
        };
        let backend = Arc::new(Source {
            unavailable: AtomicBool::new(true),
            identity,
        });
        p.keep_in(backend.clone());
        let failures = [
            p.inspect_reader("reader")
                .expect_err("source failure refuses inspection")
                .to_string(),
            p.command_actor("reader")
                .expect_err("source failure refuses admission")
                .to_string(),
            actor
                .ensure_current()
                .expect_err("source failure refuses existing actor")
                .to_string(),
        ];
        for failure in failures {
            assert!(
                failure == "session authority source is unavailable",
                "authority diagnostics must expose only a bounded failure category"
            );
        }
        assert!(
            p.of("reader").is_some(),
            "a cached principal is present but grants no authority"
        );
        assert!(
            actor.ensure_local_current().is_err(),
            "persistent authority cannot use a local fence"
        );
        let admission = p
            .admit("reader")
            .expect_err("failed admission refuses cached identity");
        assert!(
            admission.to_string() == "session authority source is unavailable",
            "admission diagnostics are bounded too"
        );
        assert!(
            p.of("reader").is_none(),
            "failed admission removes the cached principal"
        );
        backend.unavailable.store(false, Ordering::SeqCst);
        actor
            .ensure_current()
            .expect("same-generation source recovery");
        assert!(
            p.admit("reader")
                .expect("same-generation admission recovery")
                == bound,
            "recovery installs only the successful source generation"
        );
        assert!(
            p.inspect_reader("reader").expect("recovered source") == bound,
            "successful authority retains the exact reader generation"
        );
    }

    #[test]
    fn debug_output_never_discloses_session_authority_or_principal_fields() {
        let session = "debug-fixture-cookie-do-not-log";
        let principal = Principal {
            user: "debug-fixture-user-do-not-log".into(),
            handle: "debug-fixture-handle-do-not-log".into(),
            name: "debug-fixture-name-do-not-log".into(),
            issuer: "debug-fixture-issuer-do-not-log".into(),
        };
        let p = Principals::default();
        p.set_accounts(true);
        p.open(session, principal.clone());
        let other_session = "debug-other-cookie-do-not-log";
        let other_principal = Principal::guest("debug-other-principal-do-not-log");
        p.open(other_session, other_principal.clone());
        let other_reader = p.bound_reader(other_session);
        let reader = p.bound_reader(session);
        let actor = p.command_actor(session).expect("current fixture authority");
        let held = p.0.sessions.read().expect("fixture sessions");
        let identity = SessionIdentity {
            principal: principal.clone(),
            epoch: held.get(session).expect("opened fixture").epoch,
        };
        let epoch_debug = format!("{:?}", identity.epoch);
        assert!(
            reader != other_reader,
            "distinct opaque readers retain different identities"
        );
        assert!(
            reader.partition(session) != other_reader.partition(other_session),
            "redacted Debug never becomes a partition key"
        );
        assert!(
            format!("{reader:?}") == format!("{other_reader:?}"),
            "private identities share only their non-identifying debug kind"
        );
        let snapshots = [
            ("principal", format!("{principal:?} {principal:#?}")),
            ("generation", format!("{identity:?} {identity:#?}")),
            ("reader", format!("{reader:?} {reader:#?}")),
            ("actor", format!("{actor:?} {actor:#?}")),
            ("store", format!("{p:?} {p:#?}")),
            (
                "held",
                format!("{:?}", held.get(session).expect("opened fixture")),
            ),
        ];
        drop(held);
        let secrets = [
            session,
            &principal.user,
            &principal.handle,
            &principal.name,
            &principal.issuer,
            &epoch_debug,
            other_session,
            &other_principal.user,
            &other_principal.handle,
            &other_principal.name,
        ];
        let leaking: Vec<_> = snapshots
            .iter()
            .filter_map(|(kind, snapshot)| {
                secrets
                    .iter()
                    .any(|secret| snapshot.contains(secret))
                    .then_some(*kind)
            })
            .collect();
        assert!(
            leaking.is_empty(),
            "identity Debug disclosed private fields in: {leaking:?}"
        );
        actor
            .ensure_current()
            .expect("debugging retains current authority");
        assert!(
            p.of(session) == Some(principal),
            "debugging retains principal data"
        );
        assert!(
            p.inspect_reader(session).expect("inspection") == reader,
            "debugging retains the reader generation"
        );
    }

    #[test]
    fn a_panicking_read_restores_the_previous_invocation_binding() {
        let p = Principals::default();
        p.set_accounts(true);
        p.open("reader", Principal::guest("alice"));
        let bound = p.bound_reader("reader");
        p.with_reader("reader", &ReaderIdentity::Anonymous, || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                p.with_reader("reader", &bound, || panic!("fixture read unwinds"));
            }));
            assert!(result.is_err(), "the read actually unwinds");
            assert!(
                p.of("reader").is_none(),
                "unwinding restores explicit anonymous context"
            );
        });
        assert_eq!(p.user_of("reader"), "u-alice");
    }
}
