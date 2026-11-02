//! **A production host, as a process** (track `deploy`): the server binary
//! started as a deployment starts it, with `PW_DEPLOYMENT=production`, and
//! asked from outside. What lives in the process is held here: its
//! refusals at start, said as JSON lines and never with a secret; the
//! `Strict-Transport-Security` line on every answer; the runtime started at
//! the build's version; and its stop, which finishes an answer under way
//! and ends the process when told.
//!
//! The program is kiokun's (`examples/kiokun-site`), whose data are the
//! repository's sample files: a production host serves it with no
//! database. The request-level rules are `src/tests/production_host.rs`'s.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const ORIGIN: &str = "https://preview.example";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// **kiokun's program, built as `pw build` builds it**, into `dir/build`,
/// with the runtime beside it in `dir`, as `run.sh` lays a build out; and
/// its data laid out as kiokun-data's are, `output_dictionary` beside
/// `output_search_index.csv`, from the repository's sample. The data's
/// directory is returned, as `KIOKUN_DATA` names it.
fn built_kiokun(dir: &Path) -> String {
    let root = root();
    built(
        dir,
        &[
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/kiokun-site",
        ],
        &["examples/kiokun/Shards.pw", "examples/kiokun/dictionary.pw"],
    );
    // Copied, not linked: kiokun reads `KIOKUN_DATA/..`, which a link's
    // target's parent would answer (29 files, 288 KiB).
    let data = dir.join("kiokun");
    copy_tree(
        &root.join("examples/kiokun/data/han-1char-3"),
        &data.join("output_dictionary"),
    );
    std::fs::copy(
        root.join("examples/kiokun/data/search-sample.csv"),
        data.join("output_search_index.csv"),
    )
    .expect("the search index");
    data.join("output_dictionary").display().to_string()
}

/// **The store's program, built as `deploy/build-dist.sh` builds it**,
/// into `dir/build`, the runtime beside it.
fn built_store(dir: &Path) {
    built(
        dir,
        &[
            "packages/pw-std",
            "packages/pw-platform-web",
            "examples/lib",
            "examples/store",
        ],
        &["examples/domain.pw"],
    );
}

/// The `.pw` files of each of `directories`, in name order, and `files`,
/// built as `pw build` builds them into `dir/build`, with the runtime
/// beside them in `dir`, as `run.sh` lays a build out, and what a
/// production host never serves: a page as a file, and the build's
/// sources, which name the paths of the machine that built it.
fn built(dir: &Path, directories: &[&str], files: &[&str]) {
    let root = root();
    let mut paths: Vec<PathBuf> = Vec::new();
    for d in directories {
        let mut ps: Vec<PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        ps.sort();
        paths.extend(ps);
    }
    paths.extend(files.iter().map(|f| root.join(f)));
    let units: Vec<pw_core::check::Unit> = paths
        .into_iter()
        .map(|p| {
            let src = std::fs::read_to_string(&p).expect("read");
            pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: p.display().to_string(),
                src,
            }
        })
        .collect();
    let build = pw_core::build::build(&units).expect("the program builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    build
        .write(&dir.join("build"))
        .expect("the build is written");
    std::fs::copy(
        root.join("spikes/own-renderer/public/pw-runtime.mjs"),
        dir.join("pw-runtime.mjs"),
    )
    .expect("the runtime");
    // What a production host never serves: a page as a file, and the
    // build's sources, which name the paths of the machine that built it.
    std::fs::write(dir.join("sources.json"), "{\"files\":[]}").expect("sources");
    std::fs::write(dir.join("Old.html"), "<p>old</p>").expect("a page");
}

/// Every file under `from`, copied to the same place under `to`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a directory");
    for entry in std::fs::read_dir(from).expect("a directory") {
        let path = entry.expect("an entry").path();
        let into = to.join(path.file_name().expect("a name"));
        if path.is_dir() {
            copy_tree(&path, &into);
        } else {
            std::fs::copy(&path, &into).expect("a file");
        }
    }
}

/// **The host, started** on `dist` with `env` (and nothing of this
/// process's environment but its `PATH`), its standard error read line by
/// line into the channel.
fn start(dist: &Path, env: &[(&str, String)]) -> (Child, mpsc::Receiver<String>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pw-dev-server"));
    command
        .arg(dist)
        .arg(dist.join("build"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("the host starts");
    let (tx, lines) = mpsc::channel();
    let stderr = child.stderr.take().expect("its standard error");
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    (child, lines)
}

/// A production deployment's environment, `extra` added or replacing:
/// kiokun's files the repository's sample, named as a deployment names its
/// own, and its search cache a directory no search here writes to.
fn production(extra: &[(&str, &str)]) -> Vec<(&'static str, String)> {
    let sample = root().join("examples/kiokun/data/han-1char-3");
    let cache = std::env::temp_dir().join(format!("pw-production-cache-{}", std::process::id()));
    let mut env: Vec<(&'static str, String)> = vec![
        ("PW_DEPLOYMENT", "production".into()),
        ("PW_ORIGIN", ORIGIN.into()),
        ("PW_IDENTITY_KEY", KEY.into()),
        ("PW_IDENTITY", "none".into()),
        ("PORT", "0".into()),
        ("PW_SHUTDOWN_GRACE", "5".into()),
        ("KIOKUN_DATA", sample.display().to_string()),
        ("PW_CACHE_DIR", cache.display().to_string()),
    ];
    for (name, value) in extra {
        env.retain(|(n, _)| n != name);
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        if !value.is_empty() {
            env.push((name, value.to_string()));
        }
    }
    env
}

/// The host's exit code (none where a signal ended it) and every line it
/// said until it ended, each read as JSON.
fn said_until_exit(
    mut child: Child,
    lines: &mpsc::Receiver<String>,
) -> (Option<i32>, Vec<serde_json::Value>) {
    // A host that does not end is killed at the wait, and has no code.
    let status = wait(&mut child, Duration::from_secs(30));
    let mut said = Vec::new();
    // Its standard error is closed when it ends, and the reader with it.
    while let Ok(line) = lines.recv_timeout(Duration::from_secs(5)) {
        said.push(read_line(&line));
    }
    (status.and_then(|s| s.code()), said)
}

/// The process's status, once it ends within `within`; killed, and none,
/// where it does not.
fn wait(child: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let until = Instant::now() + within;
    loop {
        if let Some(status) = child.try_wait().expect("its status") {
            return Some(status);
        }
        if Instant::now() > until {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// **The port the host said it listens on**, from its line, and each line
/// it said before, read as JSON; a line that is not JSON is kept as
/// `{"not_json": <the line>}`, which the tests refuse at their assertions.
fn listening(lines: &mpsc::Receiver<String>) -> (u16, Vec<serde_json::Value>) {
    let mut said = Vec::new();
    loop {
        let line = lines
            .recv_timeout(Duration::from_secs(120))
            .expect("the host says it listens");
        let value = read_line(&line);
        let message = value["msg"].as_str().unwrap_or(&line).to_string();
        let port = message
            .strip_prefix("pw dev server on ")
            .and_then(|p| p.parse().ok());
        said.push(value);
        if let Some(port) = port {
            return (port, said);
        }
    }
}

/// A line as JSON, or `{"not_json": <the line>}`.
fn read_line(line: &str) -> serde_json::Value {
    serde_json::from_str(line).unwrap_or_else(|_| serde_json::json!({ "not_json": line }))
}

/// The lines among `said` that are not JSON.
fn not_json(said: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    said.iter()
        .filter(|l| l.get("not_json").is_some())
        .collect()
}

/// A request to the host, written whole, and its answer: head and body.
fn ask(port: u16, request: &str) -> (String, Vec<u8>) {
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    client
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a timeout");
    client.write_all(request.as_bytes()).expect("request");
    let mut answer = Vec::new();
    let _ = client.read_to_end(&mut answer);
    let split = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a head");
    (
        String::from_utf8_lossy(&answer[..split]).into_owned(),
        answer[split + 4..].to_vec(),
    )
}

fn get(port: u16, path: &str, headers: &str) -> (String, Vec<u8>) {
    ask(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: preview.example\r\n{headers}\r\n"),
    )
}

fn field<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn status(head: &str) -> u16 {
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status")
}

fn signal(child: &Child, name: &str) {
    let sent = Command::new("kill")
        .arg(format!("-{name}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill runs");
    assert!(sent.success());
}

/// **A production deployment missing what production needs is refused at
/// start, every reason said at once**, as JSON lines, before its build is
/// read; a development one would have started.
#[test]
fn a_production_host_refuses_to_start_without_what_production_needs() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let env = production(&[
        ("PW_ORIGIN", "http://preview.example"),
        ("PW_IDENTITY_KEY", ""),
        ("PW_FEED_TRANSACTIONS", "connection"),
    ]);
    let (child, lines) = start(dir.path(), &env);
    let (code, said) = said_until_exit(child, &lines);
    assert_eq!(code, Some(1), "{said:?}");
    assert!(not_json(&said).is_empty(), "{said:?}");
    let reasons: Vec<&str> = said
        .iter()
        .filter(|l| l["level"] == "error" && l["event"] == "refused")
        .filter_map(|l| l["msg"].as_str())
        .collect();
    assert_eq!(reasons.len(), 3, "{said:?}");
    assert!(reasons[0].contains("served over https"), "{reasons:?}");
    assert!(reasons[1].contains("PW_IDENTITY_KEY"), "{reasons:?}");
    assert!(
        reasons[2].contains("PW_FEED_TRANSACTIONS=connection"),
        "{reasons:?}"
    );
    for line in &said {
        assert!(
            line["ts"].as_str().is_some_and(|t| t.ends_with('Z')),
            "{line}"
        );
    }
}

/// **A production host never says a secret**: a database it cannot reach
/// is refused with its name and without its address's password, and a
/// secrets file others may read is refused unread.
#[test]
fn a_production_host_never_says_a_secret() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let env = production(&[(
        "PW_STORE_DATABASE_URL",
        "postgresql://pleris:s3cr3t-pa55w0rd@127.0.0.1:1/pleris?connect_timeout=2",
    )]);
    let (child, lines) = start(dir.path(), &env);
    let (code, said) = said_until_exit(child, &lines);
    assert_eq!(code, Some(1), "{said:?}");
    let all = serde_json::to_string(&said).expect("JSON");
    assert!(all.contains("PW_STORE_DATABASE_URL"), "{all}");
    assert!(!all.contains("s3cr3t-pa55w0rd"), "{all}");
    assert!(!all.contains(KEY), "{all}");

    use std::os::unix::fs::PermissionsExt;
    let file = dir.path().join("secrets");
    std::fs::write(&file, format!("PW_IDENTITY_KEY={KEY}\n")).expect("written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let env = production(&[
        ("PW_IDENTITY_KEY", ""),
        ("PW_SECRETS_FILE", file.to_str().expect("a path")),
    ]);
    let (child, lines) = start(dir.path(), &env);
    let (code, said) = said_until_exit(child, &lines);
    assert_eq!(code, Some(1), "{said:?}");
    let all = serde_json::to_string(&said).expect("JSON");
    assert!(all.contains("may be read by anyone"), "{all}");
    assert!(!all.contains(KEY), "{all}");
}

/// **A production host serves, says so in JSON, and stops when told**: its
/// answers carry HSTS; its documents start the runtime at the build's
/// version, which it serves for good and compressed; a page or a file of
/// its build is not served as a file; and SIGTERM lets an answer under way
/// finish, takes no new connection, and ends the process with 0.
#[test]
fn a_production_host_serves_and_stops_when_told() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let data = built_kiokun(dir.path());
    // Read from a secrets file only its owner may read, as systemd's
    // credentials are.
    use std::os::unix::fs::PermissionsExt;
    let file = dir.path().join("secrets");
    std::fs::write(
        &file,
        format!("# the deployment's\nPW_IDENTITY_KEY={KEY}\n"),
    )
    .expect("written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).expect("mode");
    let env = production(&[
        ("PW_IDENTITY_KEY", ""),
        ("PW_SECRETS_FILE", file.to_str().expect("a path")),
        ("KIOKUN_DATA", &data),
    ]);
    let (child, lines) = start(dir.path(), &env);
    let (port, said) = listening(&lines);
    assert!(not_json(&said).is_empty(), "{said:?}");
    assert!(said.iter().any(|l| l["event"] == "assets"), "{said:?}");
    assert!(
        said.iter().any(|l| l["event"] == "secrets"
            && l["msg"]
                .as_str()
                .is_some_and(|m| m.contains("PW_IDENTITY_KEY"))),
        "{said:?}"
    );
    assert!(serde_json::to_string(&said).is_ok_and(|s| !s.contains(KEY)));

    // Health, with HSTS, as every answer.
    let (head, body) = get(port, "/_pw/health", "");
    assert_eq!(status(&head), 200, "{head}");
    assert_eq!(body, br#"{"status":"live"}"#);
    assert_eq!(
        field(&head, "strict-transport-security"),
        Some("max-age=31536000"),
        "{head}"
    );

    // A page: the program's, with its policy, HSTS and a Secure cookie.
    let (head, page) = get(port, "/word/%E4%BA%BA", "");
    assert_eq!(
        status(&head),
        200,
        "{head}\n{}",
        String::from_utf8_lossy(&page)
    );
    assert_eq!(
        field(&head, "strict-transport-security"),
        Some("max-age=31536000"),
        "{head}"
    );
    assert!(field(&head, "content-security-policy").is_some(), "{head}");
    let cookie = field(&head, "set-cookie").expect("a session");
    assert!(
        cookie.contains("Secure") && cookie.contains("HttpOnly"),
        "{cookie}"
    );
    let page = String::from_utf8_lossy(&page).into_owned();
    assert!(page.contains('人'), "{page}");

    // The runtime, at the build's version: kept for good, compressed.
    let version = said
        .iter()
        .find(|l| l["event"] == "assets")
        .and_then(|l| l["msg"].as_str())
        .and_then(|m| m.rsplit(' ').next())
        .expect("the build's version")
        .to_string();
    let (head, body) = get(
        port,
        &format!("/pw-runtime.mjs?v={version}"),
        "Accept-Encoding: gzip\r\n",
    );
    assert_eq!(status(&head), 200, "{head}");
    assert_eq!(
        field(&head, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(field(&head, "content-encoding"), Some("gzip"));
    let runtime = std::fs::read(dir.path().join("pw-runtime.mjs")).expect("the runtime");
    assert!(
        body.len() * 3 < runtime.len(),
        "{} of {}",
        body.len(),
        runtime.len()
    );
    // Not a page as a file, nor the build's sources, nor a test's control.
    for path in ["/Old.html", "/sources.json", "/bench/clock"] {
        let (head, _) = get(port, path, "");
        assert_eq!(status(&head), 404, "{path}: {head}");
    }

    // An answer under way: its head half written when the stop comes.
    let mut under_way = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    under_way
        .write_all(b"GET /_pw/health HTTP/1.1\r\nHost: preview.example\r\n")
        .expect("half a head");
    std::thread::sleep(Duration::from_millis(200));
    let stopped_at = Instant::now();
    signal(&child, "TERM");
    // Not ready from now on, and no new connection is served.
    std::thread::sleep(Duration::from_millis(300));
    under_way.write_all(b"\r\n").expect("the head's end");
    under_way
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    let mut answer = String::new();
    let _ = under_way.read_to_string(&mut answer);
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let (code, rest) = said_until_exit(child, &lines);
    assert_eq!(code, Some(0), "{rest:?}");
    assert!(not_json(&rest).is_empty(), "{rest:?}");
    assert!(
        stopped_at.elapsed() < Duration::from_secs(5),
        "{:?}",
        stopped_at.elapsed()
    );
    let events: Vec<&str> = rest.iter().filter_map(|l| l["event"].as_str()).collect();
    assert!(
        events.contains(&"stopping") && events.contains(&"stopped"),
        "{events:?}"
    );
    let stopped = rest
        .iter()
        .find(|l| l["event"] == "stopped")
        .expect("stopped");
    assert!(
        stopped["msg"]
            .as_str()
            .is_some_and(|m| m.contains("; 0 connection(s)")),
        "{stopped}"
    );
    // Each request it answered said once, with its status; a health check
    // that passed is not said.
    let requests: Vec<&serde_json::Value> =
        rest.iter().filter(|l| l["event"] == "request").collect();
    assert!(
        requests
            .iter()
            .any(|l| l["route"] == "/word/%E4%BA%BA" && l["status"] == 200),
        "{rest:?}"
    );
    assert!(
        requests.iter().all(|l| l["route"] != "/_pw/health"),
        "{rest:?}"
    );
}

/// **A stop finishes within its grace whatever stays open**: a connection
/// that never finishes its request is cut at the grace, and the process
/// ends with 0 and says how many it cut.
#[test]
fn a_stop_ends_at_its_grace_whatever_stays_open() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let data = built_kiokun(dir.path());
    // Where PW_BIND says: 127.0.0.2, a loopback address of its own, which
    // the stop's wake reaches too.
    let (child, lines) = start(
        dir.path(),
        &production(&[
            ("PW_SHUTDOWN_GRACE", "1"),
            ("KIOKUN_DATA", &data),
            ("PW_BIND", "127.0.0.2"),
        ]),
    );
    let (port, _) = listening(&lines);
    let held = TcpStream::connect(("127.0.0.2", port));
    assert!(held.is_ok(), "it listens where PW_BIND says: {held:?}");
    let mut held = held.expect("connected");
    held.write_all(b"GET /_pw/health HTTP/1.1\r\n")
        .expect("half a head");
    std::thread::sleep(Duration::from_millis(200));
    let stopped_at = Instant::now();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert_eq!(code, Some(0), "{rest:?}");
    let took = stopped_at.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_secs(10),
        "{took:?}"
    );
    let stopped = rest
        .iter()
        .find(|l| l["event"] == "stopped")
        .expect("stopped");
    assert!(
        stopped["msg"]
            .as_str()
            .is_some_and(|m| m.contains("; 1 connection(s)")),
        "{stopped}"
    );
    drop(held);
}

/// **A database of the test's own** on the local PostgreSQL
/// (`PW_STORE_DATABASE_URL` names the server), dropped when the test is done
/// with it: a production host migrates the store's schema into it.
struct Database {
    admin: String,
    name: String,
}

impl Database {
    /// The database, where the environment names a server; none otherwise,
    /// and the test says it skipped, as the server's other tests on
    /// PostgreSQL do.
    fn new(test: &str) -> Option<Database> {
        let admin = match std::env::var("PW_STORE_DATABASE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                eprintln!("skipped: PW_STORE_DATABASE_URL is not set");
                return None;
            }
        };
        let name = format!("pw_deploy_{test}_{}", std::process::id());
        let database = Database { admin, name };
        let mut client = database.client();
        // Two statements, each its own: a database is not dropped or made
        // inside a transaction block, which one query of two would be.
        client
            .batch_execute(&format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                database.name
            ))
            .expect("a stale database dropped");
        client
            .batch_execute(&format!("CREATE DATABASE {}", database.name))
            .expect("a database of the test's own");
        Some(database)
    }

    fn client(&self) -> postgres::Client {
        postgres::Client::connect(&self.admin, postgres::NoTls).expect("the local PostgreSQL")
    }

    /// The database's address: the server's, with this database's name.
    fn url(&self) -> String {
        let (server, _) = self.admin.rsplit_once('/').expect("a database's address");
        format!("{server}/{}", self.name)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = self.client().batch_execute(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ));
    }
}

/// Each instance token a document's ranges carry (`<!--pw:s16@<token>-->`).
fn tokens(page: &str) -> std::collections::BTreeSet<String> {
    page.split("<!--pw:")
        .skip(1)
        .filter_map(|rest| {
            let (marker, _) = rest.split_once("-->")?;
            let (_, token) = marker.split_once('@')?;
            Some(token.to_string())
        })
        .collect()
}

/// **A production store starts its runtime at the build's version, by the
/// text its policy hashes, and keys its addresses with the deployment's
/// key**: its document's bootstrap names `/pw-runtime.mjs?v=<version>`, the
/// policy allows that text's SHA-256 and no other script; and the instance
/// tokens of the menu, which every reader shares, are the same under the
/// same key after a restart and share nothing under another key, where the
/// development key would make them the same.
#[test]
fn a_production_store_starts_its_runtime_at_its_version_and_keys_its_addresses() {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let Some(database) = Database::new("keys") else {
        return;
    };
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    built_store(dir.path());
    let other_key = "f".repeat(64);
    let page_with = |key: &str| -> (String, String) {
        let env = production(&[
            ("PW_IDENTITY_KEY", key),
            ("PW_STORE_DATABASE_URL", &database.url()),
        ]);
        let (child, lines) = start(dir.path(), &env);
        let (port, _) = listening(&lines);
        let (head, page) = get(port, "/stores/47", "");
        signal(&child, "TERM");
        let (code, rest) = said_until_exit(child, &lines);
        assert_eq!(code, Some(0), "{rest:?}");
        assert_eq!(status(&head), 200, "{head}");
        (head, String::from_utf8(page).expect("a page in text"))
    };

    let (head, first) = page_with(KEY);
    assert!(
        first.contains("<script>import(\"/pw-runtime.mjs?v="),
        "the runtime is not started at a version: {first}"
    );
    let start_at = first
        .find("<script>import(\"/pw-runtime.mjs?v=")
        .expect("the runtime started at a version");
    let script = &first[start_at + "<script>".len()..];
    let source = &script[..script.find("</script>").expect("its end")];
    let version = source
        .trim_start_matches("import(\"/pw-runtime.mjs?v=")
        .trim_end_matches("\")");
    assert!(
        version.len() == 16 && version.bytes().all(|b| b.is_ascii_hexdigit()),
        "{source}"
    );
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(source.as_bytes()));
    let policy = field(&head, "content-security-policy").expect("a policy");
    let scripts = policy
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("script-src "))
        .expect("script-src");
    let hashes: Vec<&str> = scripts
        .split_whitespace()
        .filter(|s| s.starts_with("'sha256-"))
        .collect();
    assert_eq!(hashes, vec![format!("'sha256-{hash}'")], "{scripts}");

    let again = page_with(KEY).1;
    let other = page_with(&other_key).1;
    let (first, again, other) = (tokens(&first), tokens(&again), tokens(&other));
    assert!(
        !first.is_empty(),
        "the store's page carries no instance token"
    );
    assert!(
        first.intersection(&again).count() > 0,
        "the same key made no token twice: {first:?} {again:?}"
    );
    assert_eq!(
        first.intersection(&other).count(),
        0,
        "another key made the same tokens: {first:?} {other:?}"
    );
}

/// **A connection that sends nothing is closed at the read's bound**
/// (`PW_READ_TIMEOUT`, thirty seconds where it says nothing), so it does
/// not hold its thread for good.
#[test]
fn a_connection_that_sends_nothing_is_closed_at_the_reads_bound() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let data = built_kiokun(dir.path());
    let (child, lines) = start(
        dir.path(),
        &production(&[("PW_READ_TIMEOUT", "1"), ("KIOKUN_DATA", &data)]),
    );
    let (port, _) = listening(&lines);
    let mut idle = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    idle.set_read_timeout(Some(Duration::from_secs(8)))
        .expect("a timeout");
    let opened = Instant::now();
    let mut byte = [0u8; 1];
    let read = idle.read(&mut byte);
    let took = opened.elapsed();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert!(
        matches!(read, Ok(0)) && took < Duration::from_secs(4),
        "an idle connection was {read:?} after {took:?}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}
