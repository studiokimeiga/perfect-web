//! **The guestbook outlives the host that served it** (track
//! `durable-tables`): the development server as a process of its own (the
//! binary Cargo builds, run as a deployment runs it), serving the guide's
//! guestbook from PostgreSQL (`PW_TABLES_DATABASE_URL`). A reader signs up
//! at the development identity provider and signs the guestbook, all over
//! HTTP; the process is killed (SIGKILL), started again, stopped (SIGTERM)
//! and started again. Each time: the entries are there, the reader is still
//! signed in, an interaction sent again is answered as it was and signs
//! nothing more, and the schema is not made again. A second process on the
//! same database, beside the first, reads what the first committed, knows
//! its reader, and answers its retry from the first's record.
//!
//! Without a database (`PW_TABLES_DATABASE_URL`, else `PW_STORE_DATABASE_URL`
//! or `PW_FEED_DATABASE_URL`) it passes doing nothing, as every PostgreSQL
//! test of the repository does; `just e14-durable-tables` and its mutation
//! script refuse to run without one. Ports: `PW_DURABLE_TABLES_PORT` and the
//! next where it is set (a worker's own range), the system's free ones
//! otherwise.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SERVER: &str = env!("CARGO_BIN_EXE_pw-dev-server");

fn database() -> Option<String> {
    [
        "PW_TABLES_DATABASE_URL",
        "PW_STORE_DATABASE_URL",
        "PW_FEED_DATABASE_URL",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

/// A result asserted to be what it should, then read: this test fails at
/// its assertions, never at an `expect`.
trait Asserted<T> {
    fn asserted(self, what: &str) -> T;
}

impl<T, E: std::fmt::Debug> Asserted<T> for Result<T, E> {
    fn asserted(self, what: &str) -> T {
        assert!(self.is_ok(), "{what}: {:?}", self.as_ref().err());
        match self {
            Ok(value) => value,
            Err(_) => unreachable!("asserted above"),
        }
    }
}

impl<T> Asserted<T> for Option<T> {
    fn asserted(self, what: &str) -> T {
        assert!(self.is_some(), "{what}");
        match self {
            Some(value) => value,
            None => unreachable!("asserted above"),
        }
    }
}

/// **The guestbook, as `pw build` builds it**: the platform's packages and
/// `examples/guestbook`, compiled, its build written where the test holds it.
fn built() -> (tempfile::TempDir, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = tempfile::TempDir::with_prefix("pw-durable-").asserted("a temporary directory");
    let mut units = Vec::new();
    for source in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/guestbook",
    ] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join(source))
            .asserted(source)
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for path in paths {
            let src = std::fs::read_to_string(&path).asserted("a source");
            units.push(pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: path.display().to_string(),
                src,
            });
        }
    }
    let build = pw_core::build::build(&units).asserted("the guestbook builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let out = dir.path().join("build");
    build.write(&out).asserted("its build is written");
    (dir, out)
}

/// **A schema of the test's own**, dropped when it is done.
struct Schema {
    url: String,
    name: String,
}

impl Schema {
    fn new(url: &str) -> Schema {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Schema {
            url: url.to_string(),
            name: format!("pw_dt_process_{}_{nanos}", std::process::id()),
        }
    }

    /// The first column of each row `query` answers, as text; an error as
    /// its one line, for the asserting test.
    fn texts(&self, query: &str) -> Vec<String> {
        let answered = postgres::Client::connect(&self.url, postgres::NoTls).and_then(|mut c| {
            c.batch_execute(&format!("SET search_path TO \"{}\"", self.name))?;
            c.query(query, &[])
        });
        match answered {
            Ok(rows) => rows
                .iter()
                .map(|r| {
                    r.try_get::<_, String>(0)
                        .unwrap_or_else(|e| format!("error: {e}"))
                })
                .collect(),
            Err(e) => vec![format!("error: {e}")],
        }
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", self.name));
        }
    }
}

/// `n` ports to serve on: the worker's own from `PW_DURABLE_TABLES_PORT`
/// where it is set, each free now; the system's free ones otherwise.
fn ports(n: usize) -> Vec<u16> {
    let free = |port: u16| TcpListener::bind(("127.0.0.1", port)).is_ok();
    match std::env::var("PW_DURABLE_TABLES_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        Some(base) => (base..base.saturating_add(100))
            .filter(|p| free(*p))
            .take(n)
            .collect(),
        None => (0..n)
            .filter_map(|_| {
                TcpListener::bind(("127.0.0.1", 0))
                    .and_then(|l| l.local_addr())
                    .ok()
                    .map(|a| a.port())
            })
            .collect(),
    }
}

/// **One host**: the development server serving the guestbook's build on
/// `port`, its tables in the test's schema, its log in the test's directory.
struct Host {
    child: Child,
    log: PathBuf,
}

impl Host {
    fn start(build: &Path, schema: &Schema, port: u16, log: &Path) -> Host {
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .asserted("the host's log");
        let err = out.try_clone().asserted("the host's log, twice");
        let child = Command::new(SERVER)
            .arg(build)
            .arg(build)
            .env("PORT", port.to_string())
            .env("PW_TABLES_DATABASE_URL", &schema.url)
            .env("PW_TABLES_SCHEMA", &schema.name)
            .env("PW_IDENTITY", "dev-accounts")
            // The feed's and the store's layers, which this program does
            // not read, in memory: nothing of theirs made in the database.
            .env("PW_FEED_DATABASE_URL", "")
            .env("PW_STORE_DATABASE_URL", "")
            .env_remove("PW_DEPLOYMENT")
            .env_remove("PW_ORIGIN")
            .env_remove("KIOKUN_DATA")
            .env_remove("KIOKUN_APP")
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .asserted("the development server starts");
        let mut host = Host {
            child,
            log: log.to_path_buf(),
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut ready = false;
        while Instant::now() < deadline {
            if !matches!(host.child.try_wait(), Ok(None)) {
                break;
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ready, "the host answers on {port}:\n{}", host.said());
        host
    }

    /// What the host said, each line marked as its own, so that its words
    /// are never read as this test's.
    fn said(&self) -> String {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|l| format!("  host| {}", l.replace("panicked at", "stopped at")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Ended at once, as a crash or a host's machine ends it: SIGKILL.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Stopped as a deployment stops it: SIGTERM, and its end waited for.
    fn stop(mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// **An HTTP exchange's answer**: its status, its headers, its body.
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// **A browser of one reader**: its cookies, kept from each answer.
#[derive(Default, Clone)]
struct Browser {
    cookies: Vec<(String, String)>,
}

impl Browser {
    fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// One request to `port`, its cookies sent and the answer's kept.
    fn exchange(
        &mut self,
        port: u16,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Answer {
        let mut stream =
            TcpStream::connect(("127.0.0.1", port)).asserted("a connection to the host");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
        let mut request = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
        if !self.cookies.is_empty() {
            let jar: Vec<String> = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            request.push_str(&format!("Cookie: {}\r\n", jar.join("; ")));
        }
        for (k, v) in headers {
            request.push_str(&format!("{k}: {v}\r\n"));
        }
        request.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        stream
            .write_all(request.as_bytes())
            .asserted("the request sent");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let mut lines = head.lines();
        let status = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        for (k, v) in &headers {
            if k.eq_ignore_ascii_case("set-cookie")
                && let Some((name, rest)) = v.split_once('=')
            {
                let value = rest.split(';').next().unwrap_or_default().to_string();
                self.cookies.retain(|(n, _)| n != name);
                if !value.is_empty() {
                    self.cookies.push((name.to_string(), value));
                }
            }
        }
        // A chunked body, as the server streams a page: its chunks joined.
        let body = if headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"))
        {
            dechunked(body)
        } else {
            body.to_string()
        };
        Answer {
            status,
            headers,
            body,
        }
    }

    /// **Signed up at the development provider, from the guestbook, and
    /// back**: its sign-in started, the provider's form posted, the
    /// callback followed; the session the sign-in opened kept.
    fn sign_up(&mut self, port: u16, handle: &str, name: &str) {
        let start = self.exchange(port, "GET", "/sign-up", &[], "");
        assert_eq!(start.status, 303, "the sign-up starts: {}", start.body);
        let location = start.header("location").asserted("sent to the provider");
        let (_, query) = location
            .split_once('?')
            .asserted("an authorization request");
        let form = format!(
            "{query}&handle={handle}&name={}&password=correct%20horse%20battery",
            name.replace(' ', "%20")
        );
        let provider = self.exchange(
            port,
            "POST",
            "/dev-idp/authorize",
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Sec-Fetch-Site", "same-origin"),
            ],
            &form,
        );
        assert_eq!(
            provider.status, 303,
            "the account is made: {}",
            provider.body
        );
        let back = provider.header("location").asserted("sent back");
        let callback = back
            .split_once("/sign-in/callback")
            .map(|(_, rest)| format!("/sign-in/callback{rest}"))
            .asserted("the callback");
        let signed = self.exchange(port, "GET", &callback, &[], "");
        assert_eq!(signed.status, 303, "signed in: {}", signed.body);
        assert!(self.cookie("pw-session").is_some(), "a session");
    }

    /// The guestbook's page, as this reader is served it and reads it.
    fn page(&mut self, port: u16) -> String {
        let page = self.exchange(port, "GET", "/", &[], "");
        assert_eq!(page.status, 200, "{}", page.body);
        plain(&page.body)
    }

    /// `text` signed, interaction `interaction`: what the host answered.
    fn sign(&mut self, port: u16, text: &str, interaction: &str) -> serde_json::Value {
        let answer = self.exchange(
            port,
            "POST",
            "/command/guestbook.book.sign",
            &[
                ("Content-Type", "application/json"),
                ("Sec-Fetch-Site", "same-origin"),
                ("pw-interaction", interaction),
            ],
            &serde_json::json!([text]).to_string(),
        );
        assert!(
            (200..300).contains(&answer.status),
            "signed: {} {}",
            answer.status,
            answer.body
        );
        serde_json::from_str(&answer.body).asserted("a JSON answer")
    }
}

/// A chunked body's chunks, joined.
fn dechunked(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].trim_start_matches("\r\n");
    }
    out
}

/// A page as a reader reads it: the renderer's part markers
/// (`<!--pw:s9-->`) taken out.
fn plain(page: &str) -> String {
    let mut plain = String::new();
    let mut rest = page;
    while let Some((before, after)) = rest.split_once("<!--") {
        plain.push_str(before);
        rest = after.split_once("-->").map_or("", |(_, after)| after);
    }
    plain.push_str(rest);
    plain
}

/// How many entries the page shows that say `text`.
fn shows(page: &str, text: &str) -> usize {
    page.matches(&format!("<p>{text}</p>")).count()
}

#[test]
fn the_guestbook_outlives_its_host_killed_or_stopped() {
    let Some(url) = database() else { return };
    let (dir, build) = built();
    let schema = Schema::new(&url);
    let ports = ports(2);
    assert_eq!(ports.len(), 2, "two free ports");
    let (a, b) = (ports[0], ports[1]);
    let log = dir.path().join("host-a.log");
    let host = Host::start(&build, &schema, a, &log);
    assert!(
        host.said().contains(&format!(
            "pw dev server tables: on PostgreSQL, schema {}",
            schema.name
        )),
        "the host keeps the tables in PostgreSQL:\n{}",
        host.said()
    );
    let mut ada = Browser::default();
    ada.sign_up(a, "ada", "Ada Lovelace");
    let first = ada.sign(a, "Hello, from before", "i-1");
    assert_eq!(first["committed"], true, "{first}");
    let second = ada.sign(a, "And again", "i-2");
    assert_eq!(second["committed"], true, "{second}");
    let page = ada.page(a);
    assert!(page.contains("Signed in as Ada Lovelace"), "{page}");
    assert_eq!(shows(&page, "Hello, from before"), 1, "{page}");
    assert_eq!(shows(&page, "And again"), 1, "{page}");
    // Killed: no warning, no shutdown.
    host.kill();
    let host = Host::start(&build, &schema, a, &log);
    let page = ada.page(a);
    assert!(
        page.contains("Signed in as Ada Lovelace"),
        "the reader is still signed in: {page}\n{}",
        host.said()
    );
    assert_eq!(shows(&page, "Hello, from before"), 1, "{page}");
    assert_eq!(shows(&page, "And again"), 1, "{page}");
    // The second signing sent again, as a client whose answer was lost
    // sends it: answered as it was, from its record, signing nothing more.
    let retried = ada.sign(a, "And again", "i-2");
    assert_eq!(retried["committed"], second["committed"], "{retried}");
    assert_eq!(retried["result"], second["result"], "{retried}");
    assert_eq!(
        schema.texts("SELECT count(*)::text FROM \"Entries\""),
        ["2"]
    );
    // Stopped, as a deployment stops it, and started again.
    host.stop();
    let host = Host::start(&build, &schema, a, &log);
    let page = ada.page(a);
    assert!(page.contains("Signed in as Ada Lovelace"), "{page}");
    assert_eq!(shows(&page, "And again"), 1, "{page}");
    let third = ada.sign(a, "After two restarts", "i-3");
    assert_eq!(third["committed"], true, "{third}");
    // A second host on the same database, beside the first: it reads what
    // the first committed, knows the reader, and answers the first's
    // interaction from its record.
    let other = Host::start(&build, &schema, b, &dir.path().join("host-b.log"));
    let page = ada.page(b);
    assert!(page.contains("Signed in as Ada Lovelace"), "{page}");
    assert_eq!(shows(&page, "Hello, from before"), 1, "{page}");
    assert_eq!(shows(&page, "After two restarts"), 1, "{page}");
    let elsewhere = ada.sign(b, "After two restarts", "i-3");
    assert_eq!(elsewhere["committed"], third["committed"], "{elsewhere}");
    assert_eq!(elsewhere["result"], third["result"], "{elsewhere}");
    assert_eq!(
        schema.texts("SELECT count(*)::text FROM \"Entries\""),
        ["3"]
    );
    // The schema was made once, and held to at each start.
    assert_eq!(schema.texts("SELECT hash FROM pw_schema").len(), 1);
    assert_eq!(
        schema.texts("SELECT version FROM pw_platform ORDER BY version"),
        [
            "0001_platform",
            "0002_runs_and_sessions",
            "0003_shared_outbox",
            "0004_outbox_removals_marked"
        ]
    );
    other.stop();
    host.stop();
}
