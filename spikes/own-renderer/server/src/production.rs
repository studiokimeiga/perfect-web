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
pub(crate) mod connections;
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

/// **How long a request's whole head and body may take to arrive** at a
/// production host, from its connection's start, however its bytes are
/// spaced (`connections::Arriving`): a connection that has not sent its
/// request by then is closed, so that it does not hold its thread for good.
/// Thirty seconds, as Node's `headersTimeout` gives a head sixty and its
/// `requestTimeout` a request three hundred: a feed's 5 MB image arrives
/// within it at 1.4 Mbit/s, and a slower deployment states a longer one.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// **How long an answer's whole head and body may take to be written**, and
/// any one write on a connection (`connections::Sending`), as Go's
/// `Server.WriteTimeout` bounds an answer: a client that reads nothing, or a
/// byte at a time, holds its thread no longer. Thirty seconds: the largest
/// answer, the renderer's WebAssembly, is 1 MB.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// **How many connections a production host holds open at once**, each a
/// thread: one more is answered 503 at once (`connections::turn_away`). A
/// page holds two while it is open (its document, then its subscription),
/// so 1,024 is some 500 readers at once; each thread's stack is 2 MiB of
/// address space and what it touches of it, and the single-VM recipe's unit
/// holds the process to `TasksMax=` and `MemoryMax=` above it.
const MAX_CONNECTIONS: usize = 1024;

/// **What `PW_MAX_CONNECTIONS` may say**: from two (a page's document and
/// its subscription at once; one would hold every page without its
/// updates) to 65,536 (196,864 descriptors, past what any limit here
/// grants).
const CONNECTIONS: (u64, u64) = (2, 65_536);

/// **Whole seconds from `low` to `high`**, the setting `name` states, or
/// `default` where it states none; a refusal where it states another.
fn seconds(
    env: Env,
    name: &str,
    range: (u64, u64),
    default: Duration,
    refused: &mut Vec<String>,
) -> Duration {
    Duration::from_secs(whole(
        env,
        name,
        range,
        default.as_secs(),
        "whole seconds",
        refused,
    ))
}

/// **A whole number from `low` to `high`**, the setting `name` states, or
/// `default` where it states none; a refusal, saying it is `what`, where it
/// states another.
fn whole(
    env: Env,
    name: &str,
    (low, high): (u64, u64),
    default: u64,
    what: &str,
    refused: &mut Vec<String>,
) -> u64 {
    match env(name) {
        None => default,
        Some(text) => match text.parse::<u64>() {
            Ok(n) if (low..=high).contains(&n) => n,
            _ => {
                refused.push(format!("{name} is `{text}`: {what}, from {low} to {high}"));
                default
            }
        },
    }
}

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
    /// The keys its addresses are derived with: a production deployment's
    /// own, always ([`keys_for`]).
    pub(crate) keys: Keys,
    pub(crate) grace: Duration,
    /// How long a request's whole head and body may take to arrive
    /// (`PW_READ_TIMEOUT`).
    pub(crate) read_timeout: Duration,
    /// How long an answer's whole head and body may take to be written
    /// (`PW_WRITE_TIMEOUT`).
    pub(crate) write_timeout: Duration,
    /// How many connections it holds open at once (`PW_MAX_CONNECTIONS`).
    pub(crate) max_connections: usize,
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
        let grace = seconds(env, "PW_SHUTDOWN_GRACE", (1, 600), GRACE, &mut refused);
        let read_timeout = seconds(
            env,
            "PW_READ_TIMEOUT",
            (1, 3600),
            READ_TIMEOUT,
            &mut refused,
        );
        let write_timeout = seconds(
            env,
            "PW_WRITE_TIMEOUT",
            (1, 3600),
            WRITE_TIMEOUT,
            &mut refused,
        );
        let max_connections = whole(
            env,
            "PW_MAX_CONNECTIONS",
            CONNECTIONS,
            MAX_CONNECTIONS as u64,
            "a whole number of connections",
            &mut refused,
        ) as usize;
        let keys = keys_for(production, &secrets);
        if production {
            refused.extend(production_refusals(env, &secrets, &keys));
        } else if let Err(why) = &keys {
            refused.push(why.clone());
        }
        match (refused.is_empty(), keys) {
            (true, Ok(keys)) => Ok(Settings {
                production,
                bind,
                secrets,
                keys,
                grace,
                read_timeout,
                write_timeout,
                max_connections,
            }),
            _ => Err(refused),
        }
    }

    /// **The deployment's keys, taken to be installed** ([`install_keys`]):
    /// once, by `main`, which reads them nowhere else.
    pub(crate) fn take_keys(&mut self) -> Keys {
        std::mem::replace(&mut self.keys, Keys::Development)
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
/// beside what `identity.rs` and `payments.rs` refuse of their providers:
/// an origin that is not https, its keys' refusal (`keys`), a test's
/// setting, and a database on another machine reached without TLS.
fn production_refusals(env: Env, secrets: &Secrets, keys: &Result<Keys, String>) -> Vec<String> {
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
    if let Err(why) = keys {
        refused.push(why.clone());
    }
    for (name, value, why) in TEST_SETTINGS {
        if env(name).as_deref() == Some(*value) {
            refused.push(format!(
                "{name}={value} {why}: a production deployment refuses it"
            ));
        }
    }
    for name in ["PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL"] {
        if let Some(url) = secrets.get(name)
            && let Err(why) = database_reached_safely(url)
        {
            refused.push(format!("{name}: {why}"));
        }
    }
    refused
}

/// **A database a production host may reach** (the review's finding 5h):
/// one on this machine, by its Unix socket or its loopback, or one that
/// requires TLS (`sslmode=require`; this driver reads no stronger mode).
/// Elsewhere its password and every row would cross a network in the
/// clear. The address is never quoted: it may hold the password.
///
/// The host's driver connects without TLS today (`postgres::NoTls`), so a
/// database that requires it is refused when the host connects: a database
/// on another machine waits for the host's TLS connector (the ADR's Next).
pub(crate) fn database_reached_safely(url: &Secret) -> Result<(), String> {
    let config: postgres::Config = url
        .expose()
        .parse()
        .map_err(|_| "its address is no PostgreSQL connection string".to_string())?;
    if config.get_ssl_mode() == postgres::config::SslMode::Require {
        return Ok(());
    }
    let local_host = |host: &postgres::config::Host| match host {
        postgres::config::Host::Tcp(name) => {
            name == "localhost"
                || name
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }
        #[cfg(unix)]
        postgres::config::Host::Unix(_) => true,
    };
    let local = config.get_hosts().iter().all(local_host)
        && config.get_hostaddrs().iter().all(IpAddr::is_loopback);
    if local {
        Ok(())
    } else {
        Err(
            "it names a database on another machine without requiring TLS: a production host \
             reaches one on this machine (its Unix socket or its loopback), or one that requires \
             TLS (sslmode=require)"
                .to_string(),
        )
    }
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
    // kiokun's files are the deployment's, and its search index's cache is
    // written where the deployment says: the repository's sample and the
    // build's `target/` are a development's, at the build machine's paths.
    if layer == Layer::Kiokun {
        for (name, what) in [
            ("KIOKUN_DATA", "its dictionary's files"),
            ("PW_CACHE_DIR", "where its search index is cached"),
        ] {
            if env(name).is_none() {
                refused.push(format!(
                    "this program is kiokun's: a production deployment names {what} ({name})"
                ));
            }
        }
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

/// **Keys this repository publishes**, which no deployment may key its
/// addresses with (the review's finding 5d): its tests' keys until the
/// review, each refused by name whatever the rules below say of it. Tests
/// now make a key of their own each run (`/dev/urandom`), and a test of
/// this module holds that every 64-digit key a source of the server, the
/// deploy recipe or this track's ADR names is refused.
const PUBLISHED: &[&str] = &[
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
];

/// **The fewest distinct hexadecimal digits a key may hold**: 64 digits of
/// `openssl rand -hex 32` hold fewer than ten with a probability under
/// 10⁻¹¹ (at most C(16, 9)·(9/16)⁶⁴), and a key typed by hand (`aaaa…`,
/// `0f0f…`, `deadbeef…`) holds fewer.
const DISTINCT_DIGITS: usize = 10;

/// **Why a key's digits are not a secret's**, where they are not: fewer
/// than [`DISTINCT_DIGITS`] distinct digits, a run of digits repeated (its
/// first half its second, or any shorter period), or a key this repository
/// publishes. Read in lowercase, as a key's bytes are.
fn weak(text: &str) -> Option<&'static str> {
    let text = text.to_ascii_lowercase();
    if PUBLISHED.contains(&text.as_str()) {
        return Some("this repository publishes it (a test's key)");
    }
    let distinct = text
        .bytes()
        .collect::<std::collections::BTreeSet<u8>>()
        .len();
    if distinct < DISTINCT_DIGITS {
        return Some(
            "it holds fewer than ten distinct hexadecimal digits, which no random key does",
        );
    }
    let bytes = text.as_bytes();
    if (1..=bytes.len() / 2).any(|period| bytes[period..] == bytes[..bytes.len() - period]) {
        return Some("it repeats a run of its digits, which no random key does");
    }
    None
}

/// **The deployment's identity keys**, each derived from its one secret for
/// one use: the address of a resource entry, and a document's instance
/// tokens. No `Debug`: they are the secret's.
pub(crate) struct HostKeys {
    entries: String,
    instances: String,
}

impl HostKeys {
    /// **The keys `secret` makes**, or why it makes none: 64 hexadecimal
    /// digits or more (32 bytes from `openssl rand -hex 32`), never the
    /// development key, and none whose digits are no secret's ([`weak`]).
    /// The key is never quoted.
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
        if let Some(why) = weak(text) {
            return Err(format!(
                "PW_IDENTITY_KEY is no secret's: {why}; make one with `openssl rand -hex 32`"
            ));
        }
        let bytes: Vec<u8> = (0..text.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
            .collect();
        let hex = |key: [u8; 32]| key.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Ok(HostKeys {
            entries: hex(blake3::derive_key(ENTRIES_CONTEXT, &bytes)),
            instances: hex(blake3::derive_key(INSTANCES_CONTEXT, &bytes)),
        })
    }
}

/// **The keys a process's addresses are derived with**: a deployment's
/// own, from its secret, or the development server's, which this
/// repository publishes.
pub(crate) enum Keys {
    Development,
    Deployment(HostKeys),
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Keys::Development => "Keys::Development",
            Keys::Deployment(_) => "Keys::Deployment(..)",
        })
    }
}

/// **The keys a deployment's addresses are derived with** (the review's
/// finding 2): those its secret makes, where it states one (refused where
/// it makes none); where it states none, the development server's in
/// development, and in production a refusal, never the development keys.
pub(crate) fn keys_for(production: bool, secrets: &Secrets) -> Result<Keys, String> {
    match secrets.get("PW_IDENTITY_KEY") {
        Some(secret) => HostKeys::from_secret(secret).map(Keys::Deployment),
        None if production => Err(
            "a production deployment keys its addresses with a secret of its own: \
             PW_IDENTITY_KEY, 64 hexadecimal digits or more (`openssl rand -hex 32`), in its \
             environment or its secrets file"
                .to_string(),
        ),
        None => Ok(Keys::Development),
    }
}

/// The keys this process's addresses are derived with, set once by `main`
/// from the deployment's settings ([`keys_for`]).
static KEYS: OnceLock<Keys> = OnceLock::new();

/// **This process keys its addresses with `keys` from now on**: once.
pub(crate) fn install_keys(keys: Keys) -> Result<(), String> {
    KEYS.set(keys)
        .map_err(|_| "the deployment's keys were installed twice".to_string())
}

/// **The keys installed**, or a test's server's, which installs none and is
/// a development server: `main` installs a production host's own before
/// anything is keyed, and a production deployment states them or is
/// refused ([`keys_for`]).
fn installed() -> &'static Keys {
    KEYS.get().unwrap_or(&Keys::Development)
}

/// **The key resource entries are addressed by** (`main.rs`'s
/// `IDENTITY`): the deployment's, where it states one; the development
/// key otherwise, which a production deployment never is.
pub(crate) struct HostKey;

impl pw_resource::IdentityKeyProvider for HostKey {
    fn identity_key(&self) -> pw_resource::DeploymentIdentityKey {
        entry_key(installed())
    }
}

/// The entries' key `keys` give.
pub(crate) fn entry_key(keys: &Keys) -> pw_resource::DeploymentIdentityKey {
    use pw_resource::IdentityKeyProvider;
    match keys {
        Keys::Deployment(keys) => {
            pw_resource::ExplicitIdentityKey(keys.entries.clone()).identity_key()
        }
        Keys::Development => pw_resource::DevelopmentIdentityKey.identity_key(),
    }
}

/// **The key a document's instance tokens are derived with**: the
/// deployment's, where it states one; the development key otherwise, which
/// a production deployment never is.
pub(crate) fn instance_key() -> &'static str {
    instance_key_of(installed())
}

/// The instance key `keys` give.
pub(crate) fn instance_key_of(keys: &Keys) -> &str {
    match keys {
        Keys::Deployment(keys) => keys.instances.as_str(),
        Keys::Development => DEVELOPMENT_INSTANCE_KEY,
    }
}

// ---------------------------------------------------------------------------
// What a production host answers itself
// ---------------------------------------------------------------------------

/// **The paths the platform's host answers itself**, each with every path
/// under it, segment by segment: its runtime's subscription, reads and
/// handler table, its build's modules and runtime, its commands, its health,
/// readiness and metrics, the relying party's sign-in, the payments
/// provider's word, and a development server's test controls and
/// measurement pages. The host answers each before any page of the
/// program's, so no program route may be one: `pw check` refuses it
/// (PW7001, `pw_core::reserved::RESERVED`, which a test holds this list to,
/// and every route `main.rs` answers itself to this list).
pub(crate) const RESERVED: &[&str] = &[
    "/_pw",
    "/bench",
    "/command",
    "/computed",
    "/handler",
    "/menu",
    "/menu-entry",
    "/metrics",
    "/page",
    "/payments",
    "/pw-handlers",
    "/pw-read",
    "/pw-render.wasm",
    "/pw-resume.wasm",
    "/pw-runtime.mjs",
    "/sign-in",
    "/sign-out",
    "/sign-up",
    "/speculation",
    "/StorePage.html",
    "/stream",
];

/// **Whether `path` is the platform's**: one of [`RESERVED`], or under one.
pub(crate) fn reserved(path: &str) -> bool {
    RESERVED.iter().any(|r| {
        path.strip_prefix(r)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// **What a production host refuses of its build** (the review's finding
/// 5b): a page served, or an upload declared, under a path the platform
/// answers itself. `pw check` refuses such a program (PW7001); a build made
/// by other means could still name one, and a production host would then
/// answer the platform's route where the program meant its own.
pub(crate) fn build_refusals(server: &crate::Server) -> Vec<String> {
    let pages = server.plans.iter().filter_map(|(page, plan)| {
        let route = plan["route"].as_str()?;
        Some((page.clone(), crate::route_path(route).to_string()))
    });
    pages
        .chain(server.uploads.paths())
        .filter(|(_, path)| reserved(path))
        .map(|(name, path)| {
            format!(
                "the build serves `{name}` at `{path}`, a path the platform's host answers \
                 itself, which `pw check` refuses a program (PW7001)"
            )
        })
        .collect()
}

/// **The platform's own routes a production host serves**, beside its
/// assets and its commands: its health and readiness, its metrics (to
/// their token's bearer), what a page's runtime asks (its subscription, a
/// keyed read, the handler table), and the relying party's routes, which
/// say there is no sign-in here while there is none.
const PLATFORM: &[&str] = &[
    HEALTH,
    READY,
    "/metrics",
    "/stream",
    "/pw-read",
    "/pw-handlers",
    "/sign-in",
    "/sign-up",
    crate::identity::CALLBACK,
    "/sign-out",
];

/// **What a production host serves, by the pattern of the route that
/// answers it** (the review's finding 5b: a deny-list of test controls left
/// `GET /menu`, a development server's, answering in production): the
/// program's pages at their routes, its commands, its uploads and its
/// build's assets, and the platform's own ([`PLATFORM`]); none, and `404`,
/// for anything else a development server answers. The pattern is what the
/// request's log line says (`/word/{word}`, never the path).
pub(crate) fn served_as(
    server: &crate::Server,
    assets: &assets::Assets,
    method: &str,
    route: &str,
) -> Option<String> {
    if PLATFORM.contains(&route) || assets.holds(route) {
        return Some(route.to_string());
    }
    // A command the program declares and a reader may press, by its id.
    if let Some(id) = route.strip_prefix("/command/")
        && server.components.contains_key(id)
        && !server.invoked_by_platform(id)
    {
        return Some(route.to_string());
    }
    // The provider's word, where the deployment takes payments.
    if route == "/payments/webhook" && server.payments.get().is_some() {
        return Some(route.to_string());
    }
    if let Some(pattern) = server.uploads.pattern(method, route) {
        return Some(pattern);
    }
    // A page, at the route its program declares.
    server
        .plans
        .values()
        .filter_map(|plan| plan["route"].as_str())
        .find(|pattern| crate::route_params(crate::route_path(pattern), route).is_some())
        .map(str::to_string)
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
/// development). Every writer of a head calls it (`main.rs`'s, `identity`'s,
/// `uploads`'): a source test holds each to it.
pub(crate) fn head(status: u16) -> &'static str {
    log::answered(status);
    lines()
}

/// **The header lines every answer of this process carries**, for the one
/// answer no request's thread writes: the busy answer of a host at its
/// limit (`connections::turn_away`), which is said in no request's line.
pub(crate) fn lines() -> &'static str {
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
        .map(|url| Ping::new(url.clone()));
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

    /// **A request a production host answers itself, before any other**:
    /// anything it does not serve ([`served_as`]), refused as a route it
    /// does not have; its health and readiness; its metrics, refused without
    /// their token; and its build's assets. False where the request is the
    /// program's or the platform's to answer, its route's pattern said to the
    /// request's log line.
    pub(crate) fn answer(
        &self,
        server: &crate::Server,
        method: &str,
        path: &str,
        headers: &str,
        stream: &mut TcpStream,
    ) -> bool {
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let Some(pattern) = served_as(server, &self.assets, method, route) else {
            crate::respond_build(stream, 404, "text/plain; charset=utf-8", b"not found");
            return true;
        };
        log::routed(&pattern);
        if method != "GET" {
            return false;
        }
        match route {
            HEALTH => {
                answer_json(stream, 200, r#"{"status":"live"}"#);
                true
            }
            READY => {
                match self.ready(&shutdown::GATE) {
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

    /// **Whether the host is ready for requests**: not stopping (its
    /// `gate`, the process's), and its database answering. The reason it
    /// is not, said without the database's words, which an answer to anyone
    /// does not carry.
    fn ready(&self, gate: &shutdown::Gate) -> Result<(), &'static str> {
        if gate.draining() {
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

/// **How long a readiness check waits for its database** (the review's
/// finding 1e): its answer is "not ready" past it, whatever the database is
/// doing.
const READY_WITHIN: Duration = Duration::from_secs(2);

/// **The program's database, asked whether it answers** (the review's
/// finding 1e: `SELECT 1` waited without end, under a lock every other
/// check waited behind). A check is asked by a thread of its own, one at a
/// time: a check asked while one is under way waits for that one's answer,
/// and none waits longer than [`READY_WITHIN`], holding no lock while it
/// waits. A database that stalls holds one thread, the check's, until it
/// answers again; never one a request.
#[derive(Clone)]
struct Ping(std::sync::Arc<Asked>);

/// What the checks share: the database's address, the connection kept
/// between checks, and the check under way.
struct Asked {
    url: Secret,
    /// The connection the last check left, taken by the next: a lock held
    /// for the taking and the putting back, never across a query.
    idle: Mutex<Option<postgres::Client>>,
    checks: Mutex<Checks>,
    answered: std::sync::Condvar,
}

/// The checks asked: how many began, whether one is under way, and the
/// last answer, by the check that gave it.
#[derive(Default)]
struct Checks {
    began: u64,
    under_way: bool,
    last: Option<(u64, Result<(), String>)>,
}

impl Ping {
    fn new(url: Secret) -> Ping {
        Ping(std::sync::Arc::new(Asked {
            url,
            idle: Mutex::new(None),
            checks: Mutex::new(Checks::default()),
            answered: std::sync::Condvar::new(),
        }))
    }

    /// **Whether the database answers within [`READY_WITHIN`]**: the answer
    /// of the check under way, or of one begun now, whichever this joins;
    /// "not ready" where none comes in time.
    fn answers(&self) -> Result<(), String> {
        let until = std::time::Instant::now() + READY_WITHIN;
        let asked = &self.0;
        let mut checks = asked
            .checks
            .lock()
            .map_err(|_| "a check failed".to_string())?;
        if !checks.under_way {
            checks.began += 1;
            checks.under_way = true;
            let (check, shared) = (checks.began, std::sync::Arc::clone(asked));
            let started = std::thread::Builder::new()
                .name("pw-ready".into())
                .spawn(move || shared.check(check));
            if let Err(e) = started {
                checks.under_way = false;
                return Err(format!("no thread to ask it with: {e}"));
            }
        }
        // The answer of the check under way when this was asked, or of one
        // after it: never an answer older than this question.
        let wanted = checks.began;
        loop {
            if let Some((check, answer)) = &checks.last
                && *check >= wanted
            {
                return answer.clone();
            }
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(format!(
                    "it did not answer within {} s",
                    READY_WITHIN.as_secs()
                ));
            }
            checks = asked
                .answered
                .wait_timeout(checks, left)
                .map_err(|_| "a check failed".to_string())?
                .0;
        }
    }
}

impl Asked {
    /// **One check, in its own thread**: its answer kept for whoever waits.
    fn check(&self, check: u64) {
        let answer = self.ask();
        if let Ok(mut checks) = self.checks.lock() {
            checks.under_way = false;
            checks.last = Some((check, answer));
        }
        self.answered.notify_all();
    }

    /// An empty query on the connection the last check left, or on a new
    /// one, each bounded where the driver bounds it: a new connection's
    /// socket by [`READY_WITHIN`] and its unanswered bytes by ten seconds
    /// (`TCP_USER_TIMEOUT`), the query by `is_valid`'s timeout. Its startup
    /// is bounded by nothing the driver offers: a database that accepts and
    /// never answers holds this thread until it does, and the checks after
    /// wait on this one, each no longer than [`READY_WITHIN`]. A connection
    /// that failed is dropped.
    fn ask(&self) -> Result<(), String> {
        let kept = self.idle.lock().ok().and_then(|mut idle| idle.take());
        let mut client = match kept {
            Some(client) => client,
            None => {
                let mut config: postgres::Config = self
                    .url
                    .expose()
                    .parse()
                    .map_err(|_| "its address could not be read".to_string())?;
                config.connect_timeout(READY_WITHIN);
                config.tcp_user_timeout(Duration::from_secs(10));
                config
                    .connect(postgres::NoTls)
                    .map_err(crate::feed_pg::pg)?
            }
        };
        client.is_valid(READY_WITHIN).map_err(crate::feed_pg::pg)?;
        if let Ok(mut idle) = self.idle.lock() {
            *idle = Some(client);
        }
        Ok(())
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

    /// **A key made for this run**, as `openssl rand -hex 32` makes one: 32
    /// bytes of the operating system's random source, so no key a test
    /// starts with is one this repository holds (the review's finding 5d).
    pub(crate) fn fresh_key() -> String {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("the operating system's random source");
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

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
        let key = fresh_key();
        for (n, v) in [
            ("PW_ORIGIN", "https://preview.example"),
            ("PW_IDENTITY_KEY", key.as_str()),
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
        let refused = production(&[("PW_ORIGIN", "")]).err().unwrap_or_default();
        assert!(
            refused
                .iter()
                .any(|r| r.contains("states its public origin")),
            "{refused:?}"
        );
        let refused = production(&[("PW_ORIGIN", "http://preview.example")])
            .err()
            .unwrap_or_default();
        assert!(
            refused.iter().any(|r| r.contains("served over https")),
            "{refused:?}"
        );
        let refused = production(&[("PW_ORIGIN", ""), ("PW_IDENTITY_KEY", "")])
            .err()
            .unwrap_or_default();
        assert_eq!(refused.len(), 2, "{refused:?}");
    }

    /// **A production deployment with no key of its own, or a key too short
    /// or not hexadecimal, is refused**, and the key is never quoted.
    #[test]
    fn a_production_deployment_refuses_a_key_not_its_own() {
        let refused = production(&[("PW_IDENTITY_KEY", "")])
            .err()
            .unwrap_or_default();
        assert!(
            refused.iter().any(|r| r.contains("openssl rand -hex 32")),
            "{refused:?}"
        );
        let key = fresh_key();
        for key in [
            &key[..62],
            "pw-development-identity-key-not-for-deployment",
            &format!("{}zz", &key[..62]),
            &format!("{key}0"),
        ] {
            let refused = production(&[("PW_IDENTITY_KEY", key)])
                .err()
                .unwrap_or_default();
            assert!(
                refused.iter().any(|r| r.contains("not 64 or more")),
                "{key}: {refused:?}"
            );
            assert!(refused.iter().all(|r| !r.contains(key)), "{refused:?}");
        }
        let longer = format!("{key}{}", fresh_key());
        assert!(production(&[("PW_IDENTITY_KEY", &longer)]).is_ok());
        assert!(production(&[("PW_IDENTITY_KEY", &key.to_uppercase())]).is_ok());
    }

    /// **A key whose digits are no secret's is refused** (the review's
    /// finding 5d), in production and in development, and never quoted:
    /// fewer than ten distinct digits, a run of digits repeated, or a key
    /// this repository publishes; a key made by `openssl rand -hex 32` is
    /// taken.
    #[test]
    fn a_key_whose_digits_are_no_secrets_is_refused() {
        let fresh = fresh_key();
        let ab = "ab".repeat(32);
        // Nine distinct digits, in no repeated run.
        let nine: String = (0..64u32)
            .map(|i| char::from_digit((i * i + i / 7) % 9, 16).expect("a digit"))
            .collect();
        let run = "9f3c2b7a51e04d8c7e6b5a4f3c2d1e0f";
        let half = format!("{run}{run}");
        let thirds = format!("{}{}{}", &run[..22], &run[..22], &run[..20]);
        for (key, why) in [
            ("f".repeat(64), "fewer than ten distinct"),
            (ab.clone(), "fewer than ten distinct"),
            (nine.clone(), "fewer than ten distinct"),
            (half.clone(), "repeats a run"),
            (thirds.clone(), "repeats a run"),
            (PUBLISHED[0].to_string(), "publishes it"),
            (PUBLISHED[1].to_string(), "publishes it"),
            (PUBLISHED[1].to_uppercase(), "publishes it"),
        ] {
            for production_too in [true, false] {
                let mut pairs: Vec<(&'static str, String)> = vec![("PW_IDENTITY_KEY", key.clone())];
                if production_too {
                    pairs.push(("PW_DEPLOYMENT", "production".into()));
                    pairs.push(("PW_ORIGIN", "https://preview.example".into()));
                }
                let refused = Settings::read(&env_of(pairs)).err().unwrap_or_default();
                assert!(
                    refused
                        .iter()
                        .any(|r| r.contains("no secret's") && r.contains(why)),
                    "{key} ({production_too}): {refused:?}"
                );
                assert!(refused.iter().all(|r| !r.contains(&key)), "{refused:?}");
            }
        }
        assert_eq!(weak(&fresh), None, "{fresh}");
        assert!(production(&[("PW_IDENTITY_KEY", &fresh)]).is_ok());
        // Each published key is refused by name, whatever the rules say.
        for key in PUBLISHED {
            assert!(weak(key).is_some(), "{key}");
        }
    }

    /// **No key this repository's sources name is one a deployment may
    /// use** (the review's finding 5d): every run of 64 hexadecimal digits
    /// or more that the server's sources, its process tests, the deploy
    /// recipe and this track's ADR name beside a key's name is refused.
    /// A digest (an image's, a test vector's) is not named so.
    #[test]
    fn no_key_the_repository_names_is_taken() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut files: Vec<std::path::PathBuf> = vec![
            root.join("spikes/own-renderer/server/tests/production_process.rs"),
            root.join("docs/DECISIONS/ADR-XXXX-a-production-host.md"),
            root.join("deploy/smoke.sh"),
            root.join("deploy/vm/setup.sh"),
            root.join("deploy/Containerfile"),
        ];
        let mut dirs = vec![root.join("spikes/own-renderer/server/src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("a directory") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        let mut read = 0;
        for file in files {
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            read += 1;
            for line in text.lines() {
                if !line.contains("KEY") {
                    continue;
                }
                let mut run = String::new();
                for c in line.chars().chain(std::iter::once(' ')) {
                    if c.is_ascii_hexdigit() {
                        run.push(c);
                        continue;
                    }
                    if run.len() >= 64 {
                        assert!(
                            weak(&run).is_some(),
                            "{}: a key a deployment could use: {run}",
                            file.display()
                        );
                    }
                    run.clear();
                }
            }
        }
        assert!(read > 20, "the repository's sources were read: {read}");
    }

    /// **A test's setting is refused in production**, and kept in
    /// development.
    #[test]
    fn a_tests_setting_is_refused_in_production() {
        let refused = production(&[("PW_FEED_TRANSACTIONS", "connection")])
            .err()
            .unwrap_or_default();
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
        let refused = development("0.0.0.0").err().unwrap_or_default();
        assert!(
            refused
                .first()
                .is_some_and(|r| r.contains("loopback alone")),
            "{refused:?}"
        );
        assert!(development("localhost").is_err());
        assert_eq!(settings.read_timeout, READ_TIMEOUT);
        let read = production(&[("PW_READ_TIMEOUT", "5")]).expect("starts");
        assert_eq!(read.read_timeout, Duration::from_secs(5));
        for timeout in ["0", "3601", "half"] {
            assert!(
                production(&[("PW_READ_TIMEOUT", timeout)]).is_err(),
                "{timeout}"
            );
        }
        for grace in ["0", "601", "ten", "-1"] {
            assert!(
                production(&[("PW_SHUTDOWN_GRACE", grace)]).is_err(),
                "{grace}"
            );
        }
    }

    /// **How many connections a host holds, and how long an answer's writing
    /// may take** (the review's finding 1): stated defaults, each a setting,
    /// and an absurd one refused: no fewer connections than a page holds at
    /// once (two), no more than any descriptor limit here grants.
    #[test]
    fn how_many_connections_and_how_long_an_answer_may_take() {
        let settings = production(&[]).expect("starts");
        assert_eq!(settings.max_connections, MAX_CONNECTIONS);
        assert_eq!(settings.write_timeout, WRITE_TIMEOUT);
        let stated =
            production(&[("PW_MAX_CONNECTIONS", "64"), ("PW_WRITE_TIMEOUT", "5")]).expect("starts");
        assert_eq!(stated.max_connections, 64);
        assert_eq!(stated.write_timeout, Duration::from_secs(5));
        assert_eq!(
            production(&[("PW_MAX_CONNECTIONS", "2")])
                .expect("starts")
                .max_connections,
            2
        );
        for count in ["0", "1", "65537", "many", "-4"] {
            let refused = production(&[("PW_MAX_CONNECTIONS", count)])
                .err()
                .unwrap_or_default();
            assert!(
                refused
                    .iter()
                    .any(|r| r.contains("PW_MAX_CONNECTIONS") && r.contains("from 2 to 65536")),
                "{count}: {refused:?}"
            );
        }
        for timeout in ["0", "3601", "soon"] {
            assert!(
                production(&[("PW_WRITE_TIMEOUT", timeout)]).is_err(),
                "{timeout}"
            );
        }
    }

    /// **A database on another machine is reached only by TLS** (the
    /// review's finding 5h): one on this machine, by its socket or its
    /// loopback, is taken; one elsewhere without `sslmode=require` is
    /// refused, in production, by its own rule, and the address, which may
    /// hold a password, is never quoted.
    #[test]
    fn a_database_on_another_machine_is_reached_only_by_tls() {
        for url in [
            "postgresql://pleris@%2Fvar%2Frun%2Fpostgresql/pleris",
            "postgresql:///pleris",
            "postgresql://pw:s3cr3t@localhost:5432/pw",
            "postgresql://pw:s3cr3t@127.0.0.1/pw",
            "postgresql://pw:s3cr3t@[::1]:5432/pw",
            "host=/run/postgresql dbname=pleris",
            "postgresql://pw:s3cr3t@db.example.com/pw?sslmode=require",
            "postgresql://pw:s3cr3t@10.0.0.5/pw?sslmode=require",
        ] {
            let checked = database_reached_safely(&Secret::new(url));
            assert!(checked.is_ok(), "{url}: {checked:?}");
        }
        for url in [
            "postgresql://pw:s3cr3t@db.example.com/pw",
            "postgresql://pw:s3cr3t@10.0.0.5:5432/pw",
            "postgresql://pw:s3cr3t@db.example.com/pw?sslmode=prefer",
            "postgresql://pw:s3cr3t@localhost,db.example.com/pw",
            "postgresql://pw:s3cr3t@127.0.0.1/pw?hostaddr=10.0.0.5",
        ] {
            let refused = database_reached_safely(&Secret::new(url))
                .err()
                .unwrap_or_default();
            assert!(refused.contains("another machine"), "{url}: {refused}");
            assert!(!refused.contains("s3cr3t"), "{refused}");
        }
        // In production, by its name; a development host is not held to it.
        let remote = "postgresql://pw:s3cr3t@db.example.com/pw";
        let refused = production(&[("PW_STORE_DATABASE_URL", remote)])
            .err()
            .unwrap_or_default();
        assert!(
            refused
                .iter()
                .any(|r| r.starts_with("PW_STORE_DATABASE_URL:") && r.contains("another machine")),
            "{refused:?}"
        );
        assert!(refused.iter().all(|r| !r.contains("s3cr3t")), "{refused:?}");
        let refused = production(&[("PW_FEED_DATABASE_URL", remote)])
            .err()
            .unwrap_or_default();
        assert!(
            refused
                .iter()
                .any(|r| r.starts_with("PW_FEED_DATABASE_URL:")),
            "{refused:?}"
        );
        let development = env_of(vec![("PW_STORE_DATABASE_URL", remote.into())]);
        assert!(Settings::read(&development).is_ok());
    }

    /// **A program whose data would live in memory is refused in
    /// production**: the store's and the feed's without their database, and
    /// uploads without their directory, and kiokun without its files and
    /// its cache.
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
        // kiokun's: its files and its cache, not the repository's sample and
        // the build machine's `target/`.
        let refused = program_refusals(Layer::Kiokun, &none, false, &no_env);
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert!(refused[0].contains("KIOKUN_DATA") && refused[1].contains("PW_CACHE_DIR"));
        let kiokun = env_of(vec![
            ("KIOKUN_DATA", "/srv/kiokun/output_dictionary".into()),
            ("PW_CACHE_DIR", "/var/cache/pleris".into()),
        ]);
        assert!(program_refusals(Layer::Kiokun, &none, false, &kiokun).is_empty());
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
        let key = fresh_key();
        let deployed = |secret: &str| {
            Keys::Deployment(HostKeys::from_secret(&Secret::new(secret)).expect("keys"))
        };
        let (keys, again, other) = (deployed(&key), deployed(&key), deployed(&fresh_key()));
        let Keys::Deployment(own) = &keys else {
            panic!("a deployment's keys")
        };
        assert_ne!(own.entries, own.instances);
        assert_eq!(instance_key_of(&keys), own.instances);
        assert_eq!(instance_key_of(&keys), instance_key_of(&again));
        assert_ne!(instance_key_of(&keys), instance_key_of(&other));
        assert_eq!(
            instance_key_of(&Keys::Development),
            DEVELOPMENT_INSTANCE_KEY
        );
        assert_eq!(own.instances.len(), 64);

        struct Given<'k>(&'k Keys);
        impl pw_resource::IdentityKeyProvider for Given<'_> {
            fn identity_key(&self) -> pw_resource::DeploymentIdentityKey {
                entry_key(self.0)
            }
        }
        let identity = EntryIdentity::new("Resources.Cart", &["s-1"], Partition::Public);
        let address = |keys: &Keys| ResourceEntryId::derive(&identity, &Given(keys));
        assert_ne!(address(&keys), address(&Keys::Development));
        assert_ne!(address(&keys), address(&other));
        assert_eq!(address(&keys), address(&again));
        assert_eq!(
            address(&Keys::Development),
            ResourceEntryId::derive(&identity, &pw_resource::DevelopmentIdentityKey)
        );
        // Not the secret itself, used as a key.
        assert_ne!(
            address(&keys),
            ResourceEntryId::derive(&identity, &pw_resource::ExplicitIdentityKey(key.clone()))
        );
        assert_eq!(format!("{keys:?}"), "Keys::Deployment(..)");
    }

    /// **A production deployment's keys are its own, or it does not start**
    /// (the review's finding 2): with no key it is refused, never given the
    /// development keys; a development one without a key is given them, and
    /// one with a key its own.
    #[test]
    fn production_never_falls_back_to_the_development_keys() {
        let none = Secrets::default();
        let refused = keys_for(true, &none).err().unwrap_or_default();
        assert!(refused.contains("PW_IDENTITY_KEY"), "{refused}");
        assert!(matches!(keys_for(false, &none), Ok(Keys::Development)));
        let key = fresh_key();
        let stated = Secrets::of(&[("PW_IDENTITY_KEY", key.as_str())]);
        assert!(matches!(keys_for(true, &stated), Ok(Keys::Deployment(_))));
        assert!(matches!(keys_for(false, &stated), Ok(Keys::Deployment(_))));
        // The settings carry them: a production deployment's are its own.
        let mut settings = production(&[]).expect("starts");
        assert!(matches!(settings.take_keys(), Keys::Deployment(_)));
    }

    /// **A domain keyed by the deployment never says its key** (the
    /// review's finding 5g): the instance key a production host derives
    /// shows in no eight characters of a domain's `Debug`.
    #[test]
    fn a_domain_keyed_by_the_deployment_never_says_its_key() {
        let keys =
            Keys::Deployment(HostKeys::from_secret(&Secret::new(fresh_key())).expect("keys"));
        let key = instance_key_of(&keys);
        let domain = pw_document::IdentityDomain::document(
            "store.page.StorePage~ui",
            pw_document::Partition::Session { id: "s-1".into() },
            "B1",
        )
        .keyed(key);
        let shown = format!("{domain:?}");
        for at in 0..=key.len() - 8 {
            assert!(!shown.contains(&key[at..at + 8]), "{shown}");
        }
        assert!(shown.contains("store.page.StorePage~ui"), "{shown}");
    }

    /// **The platform's paths, each with what is under it**, segment by
    /// segment: `/bench/clock` is a test control's, `/benchmark` a
    /// program's; and the platform's own routes a production host serves
    /// are each among them.
    #[test]
    fn the_platforms_paths_and_what_is_under_them() {
        for path in [
            "/bench",
            "/bench/clock",
            "/_pw/health",
            "/command/menu",
            "/command/store.page.add_to_cart",
            "/menu",
            "/menu-entry",
            "/StorePage.html",
            "/stream",
            "/sign-in/callback",
            "/pw-runtime.mjs",
        ] {
            assert!(reserved(path), "{path}");
        }
        for path in [
            "/",
            "/benchmark",
            "/stores/47",
            "/menus/espresso",
            "/streams",
            "/storepage.html",
            "/word/stream",
        ] {
            assert!(!reserved(path), "{path}");
        }
        for route in PLATFORM {
            assert!(reserved(route), "{route}");
        }
        let mut sorted = RESERVED.to_vec();
        sorted.sort_by_key(|r| r.to_ascii_lowercase());
        assert_eq!(sorted, RESERVED, "kept in order");
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

    /// **A host that is stopping is not ready**; one with no database to
    /// ask is, until then; one whose database does not answer is not, and
    /// says so without the database's words.
    #[test]
    fn a_host_that_is_stopping_or_whose_database_does_not_answer_is_not_ready() {
        let host = Production::for_test(assets::Assets::of(vec![]), None);
        let gate = shutdown::Gate::new();
        assert_eq!(host.ready(&gate), Ok(()));
        gate.drain();
        assert_eq!(host.ready(&gate), Err("the host is stopping"));
        let unreachable = Production {
            assets: assets::Assets::of(vec![]),
            metrics: None,
            database: Some(Ping::new(Secret::new(
                "postgresql://pw:s3cr3t@127.0.0.1:1/pw?connect_timeout=2",
            ))),
        };
        assert_eq!(
            unreachable.ready(&shutdown::Gate::new()),
            Err("its database does not answer")
        );
    }

    /// **A database that accepts and never answers is not ready within the
    /// check's bound** (the review's finding 1e: `SELECT 1` waited without
    /// end, under a lock): each of several checks asked at once answers
    /// within it, and they ask the database once between them.
    #[test]
    fn a_database_that_never_answers_is_not_ready_within_the_bound() {
        let stalled = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = stalled.local_addr().expect("address").port();
        let (taken, accepted) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Each connection held open, unanswered, until the test ends.
            let mut held = Vec::new();
            for connection in stalled.incoming().map_while(Result::ok) {
                held.push(connection);
                let _ = taken.send(());
            }
        });
        let ping = Ping::new(Secret::new(format!(
            "postgresql://pw:s3cr3t@127.0.0.1:{port}/pw"
        )));
        let started = std::time::Instant::now();
        // Each check in a thread of its own, its answer awaited no longer
        // than the bound and some: a check that waits without end fails
        // here, at the assertion, and is left.
        let (told, answered) = std::sync::mpsc::channel();
        for _ in 0..4 {
            let (ping, told) = (ping.clone(), told.clone());
            std::thread::spawn(move || {
                let _ = told.send(ping.answers());
            });
        }
        let answers: Vec<Result<(), String>> = (0..4)
            .filter_map(|_| answered.recv_timeout(READY_WITHIN * 3).ok())
            .collect();
        let took = started.elapsed();
        assert!(
            answers.len() == 4 && took < READY_WITHIN + Duration::from_secs(2),
            "{} of four checks answered, after {took:?}",
            answers.len()
        );
        for answer in &answers {
            assert!(
                answer
                    .as_ref()
                    .is_err_and(|why| why.contains("did not answer within")),
                "{answers:?}"
            );
        }
        let mut connections = 0;
        while accepted.recv_timeout(Duration::from_millis(200)).is_ok() {
            connections += 1;
        }
        assert_eq!(
            connections, 1,
            "four checks asked the database {connections} times"
        );
        // Asked again while the first check still waits: the same check's
        // answer, within the bound, and no other connection.
        let again = std::time::Instant::now();
        let (told, answered) = std::sync::mpsc::channel();
        let asked = ping.clone();
        std::thread::spawn(move || {
            let _ = told.send(asked.answers());
        });
        let answer = answered.recv_timeout(READY_WITHIN * 3);
        assert!(
            matches!(answer, Ok(Err(_))) && again.elapsed() < READY_WITHIN + Duration::from_secs(1),
            "{answer:?} after {:?}",
            again.elapsed()
        );
        assert!(accepted.recv_timeout(Duration::from_millis(200)).is_err());
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
