//! **A production host** (track `deploy`, ADR-XXXX): what
//! `PW_DEPLOYMENT=production` asks of this server before it listens, and what
//! it changes in what the server answers. `main.rs` reaches it only at lines
//! marked `TRACK SEAM (deploy)`.
//!
//! The development server serves every reference program with test controls
//! beside it, keys its addresses with constants this repository publishes,
//! and reads its assets from disk for each request. A production deployment
//! of the same binary:
//! - **refuses at start** what may not run there, every reason said at once
//!   ([`Settings::read`], [`program_refusals`]): an origin that is not https;
//!   no identity key of its own; a test's setting; a program whose data would
//!   live in this process's memory alone; and, where they are configured, the
//!   development identity models and payment provider (`identity.rs`,
//!   `payments.rs`, which refused them before this track);
//! - **answers no test control** ([`is_test_control`]): `/bench/…`, E7-P's
//!   structural commands, `/menu-entry`, and a menu rewritten by
//!   `/StorePage.html?items=`;
//! - **keys its addresses with its own secret** ([`HostKeys`]), where the
//!   development server keys them with `pw_resource::DEVELOPMENT_KEY` and
//!   `own-renderer-spike-key`: "a deployment mode that forbids it is E8's
//!   to add" (`pw-resource`'s `DevelopmentIdentityKey`);
//! - **reads its secrets from its environment or its secrets file**
//!   ([`secrets`]), and never says one;
//! - **serves its build's assets from memory** ([`assets`]): compressed,
//!   validated by their digest, and kept for good at an address that changes
//!   with them;
//! - **says whether it lives and whether it is ready** (`/_pw/health`,
//!   `/_pw/ready`), and its metrics only to who holds their token;
//! - **sends `Strict-Transport-Security`** with every answer, its origin
//!   being https;
//! - **logs one JSON object a line** ([`log`]);
//! - **stops when told** ([`shutdown`]).
//!
//! TLS is its proxy's or its platform's, not the host's: the ADR says why.

pub(crate) mod assets;
pub(crate) mod log;
pub(crate) mod secrets;
pub(crate) mod shutdown;

use std::io::Write;
use std::net::{IpAddr, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use secrets::{Secret, Secrets};

/// **The environment, as `main` reads it or a test states it**: a name's
/// value, none where it is unset or empty.
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The process's environment.
pub(crate) fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Where a production host says it lives.
pub(crate) const HEALTH: &str = "/_pw/health";
/// Where a production host says whether it is ready for requests.
pub(crate) const READY: &str = "/_pw/ready";

/// How long a stop waits for the connections open: under the ten seconds
/// Docker and Podman wait before they kill (`docker stop`'s default).
const GRACE: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// What the deployment states, before the server is built
// ---------------------------------------------------------------------------

/// **What the deployment states before the program is read**: whether it is
/// production, where it listens, its secrets, and how long a stop waits.
#[derive(Debug)]
pub(crate) struct Settings {
    pub(crate) production: bool,
    pub(crate) bind: IpAddr,
    pub(crate) secrets: Secrets,
    pub(crate) grace: Duration,
}

/// **Whether the deployment says it is production** (`PW_DEPLOYMENT`; any
/// other value is `identity::Deployment`'s to refuse).
pub(crate) fn is_production(env: Env) -> bool {
    env("PW_DEPLOYMENT").as_deref() == Some("production")
}

impl Settings {
    /// **What the deployment states, or every reason it may not start.**
    pub(crate) fn read(env: Env) -> Result<Settings, Vec<String>> {
        let mut refused = Vec::new();
        let production = is_production(env);
        let secrets = Secrets::read(env).unwrap_or_else(|why| {
            refused.push(why);
            Secrets::default()
        });
        let loopback = IpAddr::from([127, 0, 0, 1]);
        let bind = match env("PW_BIND") {
            None => loopback,
            Some(text) => match text.parse::<IpAddr>() {
                Ok(address) if production || address.is_loopback() => address,
                Ok(address) => {
                    refused.push(format!(
                        "PW_BIND is `{address}`: a development server listens on this machine's \
                         loopback alone (its guest model signs every session in)"
                    ));
                    loopback
                }
                Err(_) => {
                    refused.push(format!("PW_BIND is `{text}`: not an IP address"));
                    loopback
                }
            },
        };
        let grace = match env("PW_SHUTDOWN_GRACE") {
            None => GRACE,
            Some(text) => match text.parse::<u64>() {
                Ok(seconds @ 1..=600) => Duration::from_secs(seconds),
                _ => {
                    refused.push(format!(
                        "PW_SHUTDOWN_GRACE is `{text}`: whole seconds, from 1 to 600"
                    ));
                    GRACE
                }
            },
        };
        if production {
            refused.extend(production_refusals(env, &secrets));
        }
        if refused.is_empty() {
            Ok(Settings {
                production,
                bind,
                secrets,
                grace,
            })
        } else {
            Err(refused)
        }
    }
}

/// **A test's settings**, which a production deployment refuses: each the
/// name, the value, and why.
const TEST_SETTINGS: &[(&str, &str, &str)] = &[(
    "PW_FEED_TRANSACTIONS",
    "connection",
    "leaves a command's isolation to the database's default, a measurement's setting",
)];

/// **What a production deployment refuses before the program is read**,
/// beside what `identity.rs` and `payments.rs` refuse of their providers.
fn production_refusals(env: Env, secrets: &Secrets) -> Vec<String> {
    let mut refused = Vec::new();
    match env("PW_ORIGIN") {
        None => refused.push(
            "a production deployment states its public origin: PW_ORIGIN=https://…".to_string(),
        ),
        Some(origin) if !origin.starts_with("https://") => refused.push(format!(
            "PW_ORIGIN is `{origin}`: a production deployment is served over https, which its \
             proxy or its platform terminates (a browser keeps a Secure cookie and HSTS only there)"
        )),
        Some(_) => {}
    }
    match secrets.get("PW_IDENTITY_KEY") {
        None => refused.push(
            "a production deployment keys its addresses with a secret of its own: \
             PW_IDENTITY_KEY, 64 hexadecimal digits or more (`openssl rand -hex 32`), in its \
             environment or its secrets file"
                .to_string(),
        ),
        Some(key) => {
            if let Err(why) = HostKeys::from_secret(key) {
                refused.push(why);
            }
        }
    }
    for (name, value, why) in TEST_SETTINGS {
        if env(name).as_deref() == Some(*value) {
            refused.push(format!(
                "{name}={value} {why}: a production deployment refuses it"
            ));
        }
    }
    refused
}

// ---------------------------------------------------------------------------
// What the program needs of the deployment
// ---------------------------------------------------------------------------

/// **The data layer a program's contracts import** (ADR-0218): the feed's,
/// kiokun's files, or the store's, which any other program is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Feed,
    Kiokun,
    Store,
}

/// **What a production deployment refuses of the program it serves**: its
/// data in this process's memory, lost when it stops. The store's and the
/// feed's are on PostgreSQL; kiokun's are files nothing writes; an upload's
/// blobs are where the deployment keeps them, not inside its build, which a
/// production host reads only.
pub(crate) fn program_refusals(
    layer: Layer,
    secrets: &Secrets,
    uploads: bool,
    env: Env,
) -> Vec<String> {
    let mut refused = Vec::new();
    let database = match layer {
        Layer::Store => Some("PW_STORE_DATABASE_URL"),
        Layer::Feed => Some("PW_FEED_DATABASE_URL"),
        Layer::Kiokun => None,
    };
    if let Some(name) = database
        && secrets.get(name).is_none()
    {
        refused.push(format!(
            "this program's data is the {} layer's, which would live in this process's memory \
             alone: a production deployment names its database ({name})",
            match layer {
                Layer::Store => "store",
                _ => "feed",
            }
        ));
    }
    if uploads && env("PW_BLOB_DIR").is_none() {
        refused.push(
            "this program keeps uploads: a production deployment names where (PW_BLOB_DIR), \
             outside its build"
                .to_string(),
        );
    }
    refused
}

// ---------------------------------------------------------------------------
// The deployment's identity keys
// ---------------------------------------------------------------------------

/// The key a development server derives its instance tokens with, which
/// this repository publishes.
pub(crate) const DEVELOPMENT_INSTANCE_KEY: &str = "own-renderer-spike-key";

/// BLAKE3 `derive_key` contexts, one for each use (its documentation:
/// "hardcoded, globally unique, and application-specific").
const ENTRIES_CONTEXT: &str = "pleris 2026-11-02 resource entry addresses";
const INSTANCES_CONTEXT: &str = "pleris 2026-11-02 document instance tokens";

/// **The deployment's identity keys**, each derived from its one secret for
/// one use: the address of a resource entry, and a document's instance
/// tokens. No `Debug`: they are the secret's.
pub(crate) struct HostKeys {
    entries: String,
    instances: String,
}

impl HostKeys {
    /// **The keys `secret` makes**, or why it makes none: 64 hexadecimal
    /// digits or more (32 bytes from `openssl rand -hex 32`), and never the
    /// development key.
    pub(crate) fn from_secret(secret: &Secret) -> Result<HostKeys, String> {
        let text = secret.expose().trim();
        let digits = text.len() >= 64
            && text.len().is_multiple_of(2)
            && text.bytes().all(|b| b.is_ascii_hexdigit());
        if !digits {
            return Err(
                "PW_IDENTITY_KEY is not 64 or more hexadecimal digits: make one with `openssl \
                 rand -hex 32`"
                    .to_string(),
            );
        }
        let bytes: Vec<u8> = (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hexadecimal"))
            .collect();
        let hex = |key: [u8; 32]| key.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Ok(HostKeys {
            entries: hex(blake3::derive_key(ENTRIES_CONTEXT, &bytes)),
            instances: hex(blake3::derive_key(INSTANCES_CONTEXT, &bytes)),
        })
    }
}

/// The deployment's keys, set once by `main` where it states them.
static KEYS: OnceLock<HostKeys> = OnceLock::new();

/// **This process keys its addresses with `keys` from now on.**
pub(crate) fn install_keys(keys: HostKeys) {
    let _ = KEYS.set(keys);
}

/// **The key resource entries are addressed by** (`main.rs`'s
/// `IDENTITY`): the deployment's, where it states one; the development
/// key otherwise.
pub(crate) struct HostKey;

impl pw_resource::IdentityKeyProvider for HostKey {
    fn identity_key(&self) -> pw_resource::DeploymentIdentityKey {
        entry_key(KEYS.get())
    }
}

/// The entries' key `keys` give, or the development key without them.
pub(crate) fn entry_key(keys: Option<&HostKeys>) -> pw_resource::DeploymentIdentityKey {
    use pw_resource::IdentityKeyProvider;
    match keys {
        Some(keys) => pw_resource::ExplicitIdentityKey(keys.entries.clone()).identity_key(),
        None => pw_resource::DevelopmentIdentityKey.identity_key(),
    }
}

/// **The key a document's instance tokens are derived with**: the
/// deployment's, where it states one; the development key otherwise.
pub(crate) fn instance_key() -> &'static str {
    instance_key_of(KEYS.get())
}

/// The instance key `keys` give, or the development key without them.
pub(crate) fn instance_key_of(keys: Option<&HostKeys>) -> &str {
    keys.map_or(DEVELOPMENT_INSTANCE_KEY, |k| k.instances.as_str())
}

// ---------------------------------------------------------------------------
// What a production host answers itself
// ---------------------------------------------------------------------------

/// E7-P's structural commands and the materializer's window: a test's
/// controls, each by its route.
const TEST_ROUTES: &[&str] = &[
    "/command/menu",
    "/command/menu_changed",
    "/command/menu_ghost",
    "/menu-entry",
];

/// **Whether a request is a test's control**, which a production host
/// answers `404`, as a route it does not have: anything under `/bench/`, E7-P's
/// structural commands and the materializer's window, and the store's
/// page asked to rewrite its menu (`?items=`, E7's large-menu case).
pub(crate) fn is_test_control(route: &str, query: &str) -> bool {
    route.starts_with("/bench/")
        || route == "/bench"
        || TEST_ROUTES.contains(&route)
        || (route == "/StorePage.html" && query.split('&').any(|p| p.starts_with("items=")))
}

/// **The header lines every answer of a production host carries**:
/// `Strict-Transport-Security`, its origin being https (RFC 6797; a year,
/// as OWASP's HTTP Headers Cheat Sheet gives, and not `includeSubDomains`,
/// which would bind the origin's other hosts, which this host does not
/// know). Set once, by `main`.
static HEADERS: OnceLock<String> = OnceLock::new();

/// The `Strict-Transport-Security` line a production host sends.
pub(crate) const HSTS: &str = "strict-transport-security: max-age=31536000\r\n";

/// **This process's answers carry `lines` from now on.**
pub(crate) fn install_headers(lines: &str) {
    let _ = HEADERS.set(lines.to_string());
}

/// **A response head is written with `status`**: the status kept for the
/// request's log line, and the header lines every answer carries (none in
/// development).
pub(crate) fn head(status: u16) -> &'static str {
    log::answered(status);
    HEADERS.get().map_or("", String::as_str)
}

/// The text that starts a production document's runtime, and its script:
/// set once, by `main`, from the build's version.
static RUNTIME: OnceLock<(String, String)> = OnceLock::new();

/// **The text that starts the runtime at the build's version**, and the
/// script element holding it.
pub(crate) fn bootstrap(version: &str) -> (String, String) {
    let source = format!("import(\"/pw-runtime.mjs?v={version}\")");
    let script = format!("<script>{source}</script>");
    (source, script)
}

/// **This process's documents start the runtime at `version` from now on.**
pub(crate) fn install_runtime(version: &str) {
    let _ = RUNTIME.set(bootstrap(version));
}

/// The text a production document starts its runtime with, where one is
/// installed.
pub(crate) fn runtime_source() -> Option<&'static str> {
    RUNTIME.get().map(|(source, _)| source.as_str())
}

/// The script a production document starts its runtime with, where one is
/// installed.
pub(crate) fn runtime_script() -> Option<&'static str> {
    RUNTIME.get().map(|(_, script)| script.as_str())
}

/// **A production host's own**: its build's assets, the token its metrics
/// are read with, and the database its readiness asks.
pub(crate) struct Production {
    pub(crate) assets: assets::Assets,
    metrics: Option<Secret>,
    database: Option<Ping>,
}

impl Production {
    /// **What a production host serves of `server`**: the build's assets as
    /// `server` would answer them (its root's files a page loads, each
    /// handler, speculation and computed module), read now; a build missing
    /// one did not finish, and is refused.
    pub(crate) fn of(
        server: &crate::Server,
        secrets: &Secrets,
        layer: Layer,
    ) -> Result<Production, String> {
        let assets = assets::Assets::read(asset_files(server)?)?;
        let database = match layer {
            Layer::Store => secrets.get("PW_STORE_DATABASE_URL"),
            Layer::Feed => secrets.get("PW_FEED_DATABASE_URL"),
            Layer::Kiokun => None,
        }
        .map(|url| Ping {
            url: url.clone(),
            client: Mutex::new(None),
        });
        Ok(Production {
            assets,
            metrics: secrets.get("PW_METRICS_TOKEN").cloned(),
            database,
        })
    }

    /// A test's: `assets`, no token, no database.
    #[cfg(test)]
    pub(crate) fn for_test(assets: assets::Assets, metrics: Option<&str>) -> Production {
        Production {
            assets,
            metrics: metrics.map(Secret::new),
            database: None,
        }
    }

    /// **A request a production host answers itself, before any other**: a
    /// test's control, refused as a route it does not have; its health and
    /// readiness; its metrics, refused without their token; and its build's
    /// assets. False where the request is the program's.
    pub(crate) fn answer(
        &self,
        method: &str,
        path: &str,
        headers: &str,
        stream: &mut TcpStream,
    ) -> bool {
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        if is_test_control(route, query) {
            crate::respond_build(stream, 404, "text/plain; charset=utf-8", b"not found");
            return true;
        }
        if method != "GET" {
            return false;
        }
        match route {
            HEALTH => {
                answer_json(stream, 200, r#"{"status":"live"}"#);
                true
            }
            READY => {
                match self.ready() {
                    Ok(()) => answer_json(stream, 200, r#"{"status":"ready"}"#),
                    Err(why) => answer_json(
                        stream,
                        503,
                        &serde_json::json!({ "status": "not ready", "why": why }).to_string(),
                    ),
                }
                true
            }
            "/metrics" if !self.may_read_metrics(headers) => {
                crate::respond_build(stream, 404, "text/plain; charset=utf-8", b"not found");
                true
            }
            _ => match self.assets.answer(route, query, headers) {
                Some(answer) => {
                    crate::write_response(
                        stream,
                        answer.status,
                        answer.mime,
                        &answer.headers,
                        answer.body,
                    );
                    true
                }
                None => false,
            },
        }
    }

    /// **Whether the host is ready for requests**: not stopping, and its
    /// database answering. The reason it is not, said without the
    /// database's words, which an answer to anyone does not carry.
    fn ready(&self) -> Result<(), &'static str> {
        if shutdown::GATE.draining() {
            return Err("the host is stopping");
        }
        if let Some(ping) = &self.database
            && let Err(why) = ping.answers()
        {
            log::warn("not-ready", &format!("the database does not answer: {why}"));
            return Err("its database does not answer");
        }
        Ok(())
    }

    /// **Whether the request may read `/metrics`**: it carries the
    /// deployment's token, compared in constant time; without a token, no
    /// one may.
    fn may_read_metrics(&self, headers: &str) -> bool {
        let Some(token) = &self.metrics else {
            return false;
        };
        let Some(given) = crate::identity::header(headers, "authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        same(given.trim().as_bytes(), token.expose().as_bytes())
    }
}

/// Two byte strings compared in time that depends on their lengths alone.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// A JSON answer no cache keeps.
fn answer_json(stream: &mut TcpStream, status: u16, body: &str) {
    crate::write_response(
        stream,
        status,
        "application/json; charset=utf-8",
        "cache-control: no-store\r\n",
        body.as_bytes(),
    );
}

/// **The files a production host serves of `server`'s build**, each by the
/// route `server` answers it at: its root's files a page loads (never a
/// page or `sources.json`), each handler's module, each speculation's and
/// each computed module.
fn asset_files(server: &crate::Server) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let mut files = Vec::new();
    let root =
        std::fs::read_dir(&server.dist).map_err(|e| format!("{}: {e}", server.dist.display()))?;
    for entry in root {
        let path = entry
            .map_err(|e| format!("{}: {e}", server.dist.display()))?
            .path();
        let loads = path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| assets::SERVED.contains(&e));
        if let (true, Some(name)) = (loads, path.file_name().and_then(|n| n.to_str())) {
            files.push((format!("/{name}"), path.clone()));
        }
    }
    for identity in server.handler_identities() {
        files.push((
            format!("/handler/{identity}.mjs"),
            server.handler_module(&identity),
        ));
    }
    for speculation in server.speculations.values() {
        if let Some(module) = speculation["module"].as_str() {
            files.push((
                format!("/speculation/{module}"),
                server.artifacts.join("speculations").join(module),
            ));
        }
    }
    for plan in server.plans.values() {
        if let Some(route) = crate::computed_module(plan) {
            let file = route.trim_start_matches("/computed/").to_string();
            files.push((route, server.artifacts.join("computed").join(file)));
        }
    }
    Ok(files)
}

/// **The program's database, asked whether it answers**: one connection,
/// opened when first asked and again after it failed, given two seconds to
/// connect.
struct Ping {
    url: Secret,
    client: Mutex<Option<postgres::Client>>,
}

impl Ping {
    fn answers(&self) -> Result<(), String> {
        let mut held = self.client.lock().expect("the readiness connection");
        if held.is_none() {
            let mut config: postgres::Config = self
                .url
                .expose()
                .parse()
                .map_err(|_| "its address could not be read".to_string())?;
            config.connect_timeout(Duration::from_secs(2));
            *held = Some(
                config
                    .connect(postgres::NoTls)
                    .map_err(crate::feed_pg::pg)?,
            );
        }
        let client = held.as_mut().expect("connected");
        match client.simple_query("SELECT 1") {
            Ok(_) => Ok(()),
            Err(e) => {
                *held = None;
                Err(crate::feed_pg::pg(e))
            }
        }
    }
}

/// **The refusals of a deployment, said**, and the process ended.
pub(crate) fn refuse(reasons: &[String]) -> ! {
    for reason in reasons {
        log::error("refused", &format!("pw dev server: {reason}"));
    }
    let _ = std::io::stderr().flush();
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn env_of(pairs: Vec<(&'static str, String)>) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        }
    }

    /// A production deployment's settings, each pair as given; the origin
    /// and the key where `with` does not set them.
    fn production(with: &[(&'static str, &str)]) -> Result<Settings, Vec<String>> {
        let mut pairs: Vec<(&'static str, String)> = vec![("PW_DEPLOYMENT", "production".into())];
        for (n, v) in with {
            pairs.push((n, v.to_string()));
        }
        for (n, v) in [
            ("PW_ORIGIN", "https://preview.example"),
            ("PW_IDENTITY_KEY", KEY),
        ] {
            if !with.iter().any(|(w, _)| *w == n) {
                pairs.push((n, v.to_string()));
            }
        }
        Settings::read(&env_of(pairs))
    }

    /// **A production deployment states an https origin and a key of its
    /// own**, and then starts; a development one needs neither.
    #[test]
    fn a_production_deployment_starts_with_its_origin_and_key() {
        let settings = production(&[]).expect("starts");
        assert!(settings.production);
        assert_eq!(settings.bind, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(settings.grace, GRACE);
        let development = Settings::read(&env_of(vec![])).expect("starts");
        assert!(!development.production);
    }

    /// **A production deployment with no origin, or one that is not https,
    /// is refused**, every reason said at once.
    #[test]
    fn a_production_deployment_refuses_an_origin_not_https() {
        let refused = production(&[("PW_ORIGIN", "")]).expect_err("refused");
        assert!(
            refused
                .iter()
                .any(|r| r.contains("states its public origin")),
            "{refused:?}"
        );
        let refused = production(&[("PW_ORIGIN", "http://preview.example")]).expect_err("refused");
        assert!(
            refused.iter().any(|r| r.contains("served over https")),
            "{refused:?}"
        );
        let refused =
            production(&[("PW_ORIGIN", ""), ("PW_IDENTITY_KEY", "")]).expect_err("refused");
        assert_eq!(refused.len(), 2, "{refused:?}");
    }

    /// **A production deployment with no key of its own, or a key too short
    /// or not hexadecimal, is refused**, and the key is never quoted.
    #[test]
    fn a_production_deployment_refuses_a_key_not_its_own() {
        let refused = production(&[("PW_IDENTITY_KEY", "")]).expect_err("refused");
        assert!(
            refused.iter().any(|r| r.contains("openssl rand -hex 32")),
            "{refused:?}"
        );
        for key in [
            &KEY[..62],
            "pw-development-identity-key-not-for-deployment",
            &format!("{}zz", &KEY[..62]),
            &format!("{KEY}0"),
        ] {
            let refused = production(&[("PW_IDENTITY_KEY", key)]).expect_err("refused");
            assert!(
                refused.iter().any(|r| r.contains("not 64 or more")),
                "{key}: {refused:?}"
            );
            assert!(refused.iter().all(|r| !r.contains(key)), "{refused:?}");
        }
        assert!(production(&[("PW_IDENTITY_KEY", &format!("{KEY}{KEY}"))]).is_ok());
        assert!(production(&[("PW_IDENTITY_KEY", &KEY.to_uppercase())]).is_ok());
    }

    /// **A test's setting is refused in production**, and kept in
    /// development.
    #[test]
    fn a_tests_setting_is_refused_in_production() {
        let refused = production(&[("PW_FEED_TRANSACTIONS", "connection")]).expect_err("refused");
        assert!(
            refused
                .iter()
                .any(|r| r.contains("PW_FEED_TRANSACTIONS=connection")),
            "{refused:?}"
        );
        let development = env_of(vec![("PW_FEED_TRANSACTIONS", "connection".into())]);
        assert!(Settings::read(&development).is_ok());
    }

    /// **A production host may listen on any address; a development one on
    /// its loopback alone**; a grace is whole seconds from one to 600.
    #[test]
    fn where_a_host_listens_and_how_long_it_stops() {
        let settings =
            production(&[("PW_BIND", "0.0.0.0"), ("PW_SHUTDOWN_GRACE", "30")]).expect("starts");
        assert_eq!(settings.bind, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(settings.grace, Duration::from_secs(30));
        assert!(production(&[("PW_BIND", "::")]).is_ok());
        let development = |bind: &str| Settings::read(&env_of(vec![("PW_BIND", bind.into())]));
        assert!(development("127.0.0.1").is_ok());
        assert!(development("::1").is_ok());
        let refused = development("0.0.0.0").expect_err("refused");
        assert!(refused[0].contains("loopback alone"), "{refused:?}");
        assert!(development("localhost").is_err());
        for grace in ["0", "601", "ten", "-1"] {
            assert!(
                production(&[("PW_SHUTDOWN_GRACE", grace)]).is_err(),
                "{grace}"
            );
        }
    }

    /// **A program whose data would live in memory is refused in
    /// production**: the store's and the feed's without their database, and
    /// uploads without their directory; kiokun's files need neither.
    #[test]
    fn a_program_whose_data_would_live_in_memory_is_refused() {
        let none = Secrets::default();
        let no_env = env_of(vec![]);
        let refused = program_refusals(Layer::Store, &none, false, &no_env);
        assert!(
            refused.len() == 1 && refused[0].contains("PW_STORE_DATABASE_URL"),
            "{refused:?}"
        );
        let refused = program_refusals(Layer::Feed, &none, true, &no_env);
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert!(refused[0].contains("PW_FEED_DATABASE_URL") && refused[1].contains("PW_BLOB_DIR"));
        assert!(program_refusals(Layer::Kiokun, &none, false, &no_env).is_empty());
        let store = Secrets::of(&[("PW_STORE_DATABASE_URL", "postgresql:///pleris")]);
        assert!(program_refusals(Layer::Store, &store, false, &no_env).is_empty());
        let blobs = env_of(vec![("PW_BLOB_DIR", "/var/lib/pleris/blobs".into())]);
        let feed = Secrets::of(&[("PW_FEED_DATABASE_URL", "postgresql:///pleris")]);
        assert!(program_refusals(Layer::Feed, &feed, true, &blobs).is_empty());
        // The store's database does not serve the feed.
        assert_eq!(
            program_refusals(Layer::Feed, &store, false, &no_env).len(),
            1
        );
    }

    /// **The deployment's keys are its own**: each derived for its use,
    /// unlike the development keys, the other use's and another secret's,
    /// and the same from the same secret.
    #[test]
    fn the_deployments_keys_are_its_own() {
        use pw_resource::{EntryIdentity, Partition, ResourceEntryId};
        let keys = HostKeys::from_secret(&Secret::new(KEY)).expect("keys");
        let again = HostKeys::from_secret(&Secret::new(KEY)).expect("keys");
        let other = HostKeys::from_secret(&Secret::new(KEY.replace('0', "1"))).expect("keys");
        assert_ne!(keys.entries, keys.instances);
        assert_eq!(instance_key_of(Some(&keys)), keys.instances);
        assert_eq!(instance_key_of(Some(&keys)), instance_key_of(Some(&again)));
        assert_ne!(instance_key_of(Some(&keys)), instance_key_of(Some(&other)));
        assert_eq!(instance_key_of(None), DEVELOPMENT_INSTANCE_KEY);
        assert_eq!(keys.instances.len(), 64);

        struct Given<'k>(Option<&'k HostKeys>);
        impl pw_resource::IdentityKeyProvider for Given<'_> {
            fn identity_key(&self) -> pw_resource::DeploymentIdentityKey {
                entry_key(self.0)
            }
        }
        let identity = EntryIdentity::new("Resources.Cart", &["s-1"], Partition::Public);
        let address = |keys: Option<&HostKeys>| ResourceEntryId::derive(&identity, &Given(keys));
        assert_ne!(address(Some(&keys)), address(None));
        assert_ne!(address(Some(&keys)), address(Some(&other)));
        assert_eq!(address(Some(&keys)), address(Some(&again)));
        assert_eq!(
            address(None),
            ResourceEntryId::derive(&identity, &pw_resource::DevelopmentIdentityKey)
        );
        // Not the secret itself, used as a key.
        assert_ne!(
            address(Some(&keys)),
            ResourceEntryId::derive(&identity, &pw_resource::ExplicitIdentityKey(KEY.into()))
        );
    }

    /// **A test's controls are refused, and nothing else**: every `/bench`
    /// route, E7-P's structural commands, the materializer's window, and a
    /// menu rewritten through the store's page; a page, a command, the
    /// store's page and the runtime's routes are the program's.
    #[test]
    fn a_tests_controls_are_refused_and_nothing_else() {
        for (route, query) in [
            ("/bench/clock", ""),
            ("/bench/order", "status=preparing"),
            ("/bench/fail", ""),
            ("/bench", ""),
            ("/command/menu", "op=remove&id=1"),
            ("/command/menu_changed", ""),
            ("/command/menu_ghost", ""),
            ("/menu-entry", ""),
            ("/StorePage.html", "items=5000"),
            ("/StorePage.html", "x=1&items=1"),
        ] {
            assert!(is_test_control(route, query), "{route}?{query}");
        }
        for (route, query) in [
            ("/", ""),
            ("/stores/47", ""),
            ("/StorePage.html", ""),
            ("/StorePage.html", "itemsx=1"),
            ("/command/store.page.add_to_cart", ""),
            ("/command/menu.x", ""),
            ("/benchmark", ""),
            ("/stream", "doc=1&since=0"),
            ("/pw-read", "binding=b"),
            ("/pw-handlers", ""),
            ("/metrics", ""),
        ] {
            assert!(!is_test_control(route, query), "{route}?{query}");
        }
    }

    /// **The runtime is started at the build's version, by the text the
    /// policy hashes**: the script element holds the source, and nothing
    /// else.
    #[test]
    fn the_runtime_is_started_at_the_builds_version() {
        let (source, script) = bootstrap("0123456789abcdef");
        assert_eq!(source, "import(\"/pw-runtime.mjs?v=0123456789abcdef\")");
        assert_eq!(script, format!("<script>{source}</script>"));
    }

    /// **Metrics are read only with the deployment's token**: none without
    /// one, and only the whole token, as a bearer's.
    #[test]
    fn metrics_are_read_only_with_the_token() {
        let assets = assets::Assets::of(vec![]);
        let open = Production::for_test(assets, None);
        assert!(!open.may_read_metrics("Authorization: Bearer x\r\n"));
        let held = Production::for_test(assets::Assets::of(vec![]), Some("t0k3n-t0k3n"));
        assert!(held.may_read_metrics("Authorization: Bearer t0k3n-t0k3n\r\n"));
        assert!(held.may_read_metrics("authorization: Bearer  t0k3n-t0k3n \r\n"));
        assert!(!held.may_read_metrics("Authorization: Bearer t0k3n-t0k3\r\n"));
        assert!(!held.may_read_metrics("Authorization: Bearer t0k3n-t0k3nX\r\n"));
        assert!(!held.may_read_metrics("Authorization: Basic t0k3n-t0k3n\r\n"));
        assert!(!held.may_read_metrics("Host: t\r\n"));
        assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab"));
    }
}
