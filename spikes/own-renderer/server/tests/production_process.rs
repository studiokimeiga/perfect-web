//! **A production host, as a process** (track `deploy`): the server binary
//! started as a deployment starts it, with `PW_DEPLOYMENT=production`, and
//! asked from outside. What lives in the process is held here: its
//! refusals at start, said as JSON lines and never with a secret; the
//! `Strict-Transport-Security` line on every answer; the runtime started at
//! the build's version; its keys; its stop, which finishes an answer under
//! way, ends a start, a subscription and, asked twice, the process; and its
//! bounds (the review's finding 1): a request's deadline however its bytes
//! are spaced, an answer's to a client that reads nothing, its limit of
//! connections, a thread the system refuses, and its readiness while its
//! database stalls.
//!
//! The program is kiokun's (`examples/kiokun-site`), whose data are the
//! repository's sample files: a production host serves it with no
//! database. The store's, where a test needs a database, on the local
//! PostgreSQL. The request-level rules are `src/tests/production_host.rs`'s.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

const ORIGIN: &str = "https://preview.example";

/// **A key made for this run**, as `openssl rand -hex 32` makes one: no key
/// this repository holds is one a production host takes (the review's
/// finding 5d).
fn fresh_key() -> String {
    let mut bytes = [0u8; 32];
    let mut random = std::fs::File::open("/dev/urandom").expect("the random source");
    random.read_exact(&mut bytes).expect("random bytes");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key this run's hosts are started with.
fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(fresh_key)
}

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
    // The sample's rows, repeated past the 4 MiB under which an index is
    // kept in memory: so its index is built into the cache, as kiokun.com's
    // 56 MB one is.
    let sample = std::fs::read_to_string(root.join("examples/kiokun/data/search-sample.csv"))
        .expect("the search sample");
    let (head, rows) = sample.split_once('\n').expect("a header");
    let mut csv = format!("{head}\n");
    while csv.len() < 5 << 20 {
        csv.push_str(rows);
    }
    std::fs::write(data.join("output_search_index.csv"), csv).expect("the search index");
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
/// process's environment but its `PATH`), each line it writes read into the
/// channel: its standard error's, and its standard output's, which a
/// production host writes nothing to (every line JSON, on standard error)
/// and a development host says it listens on.
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
    let stdout = child.stdout.take().expect("its standard output");
    let out = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if out.send(format!("(standard output) {line}")).is_err() {
                break;
            }
        }
    });
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
        ("PW_IDENTITY_KEY", key().into()),
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
    child: Child,
    lines: &mpsc::Receiver<String>,
) -> (Option<i32>, Vec<serde_json::Value>) {
    said_until_exit_within(child, lines, Duration::from_secs(30))
}

/// [`said_until_exit`], the host killed where it has not ended `within`.
fn said_until_exit_within(
    mut child: Child,
    lines: &mpsc::Receiver<String>,
    within: Duration,
) -> (Option<i32>, Vec<serde_json::Value>) {
    // A host that does not end is killed at the wait, and has no code.
    let status = wait(&mut child, within);
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
/// A host that never says it listens on its standard error (a development
/// host says it on its standard output) is refused here, at an assertion
/// naming what it did say.
fn listening(lines: &mpsc::Receiver<String>) -> (u16, Vec<serde_json::Value>) {
    let mut said = Vec::new();
    loop {
        let line = lines.recv_timeout(Duration::from_secs(120));
        assert!(
            line.is_ok(),
            "the host says it listens on its standard error; it said {said:?}"
        );
        let line = line.unwrap();
        let value = read_line(&line);
        let message = value["msg"]
            .as_str()
            .unwrap_or(line.trim_start_matches("(standard output) "))
            .to_string();
        let port = message
            .strip_prefix("pw dev server on ")
            .and_then(|p| p.parse().ok());
        said.push(value);
        if let Some(port) = port {
            return (port, said);
        }
    }
}

/// A line as JSON, or `{"not_json": <the line>}`. A panic the host said is
/// kept as `panicked (at) …`: a test's assertion that shows the host's lines
/// then names one place a test panicked, its own, as the mutation controls'
/// producer requires.
fn read_line(line: &str) -> serde_json::Value {
    let line = line.replace("panicked at ", "panicked (at) ");
    serde_json::from_str(&line).unwrap_or_else(|_| serde_json::json!({ "not_json": line }))
}

/// The lines among `said` that are not JSON.
fn not_json(said: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    said.iter()
        .filter(|l| l.get("not_json").is_some())
        .collect()
}

/// A request to the host, written whole, and its answer: head and body. A
/// host that answers nothing, or no longer listens, is `(no answer)`, whose
/// status is none, for a test's assertion to refuse.
fn ask(port: u16, request: &str) -> (String, Vec<u8>) {
    let Ok(mut client) = TcpStream::connect(("127.0.0.1", port)) else {
        return ("(no answer: not listening)".to_string(), Vec::new());
    };
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = client.write_all(request.as_bytes());
    let mut answer = Vec::new();
    let _ = client.read_to_end(&mut answer);
    match answer.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(split) => (
            String::from_utf8_lossy(&answer[..split]).into_owned(),
            answer[split + 4..].to_vec(),
        ),
        None => ("(no answer)".to_string(), answer),
    }
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

/// The status an answer's head says, or 0 where it says none.
fn status(head: &str) -> u16 {
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
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
    assert!(!all.contains(key()), "{all}");

    use std::os::unix::fs::PermissionsExt;
    let file = dir.path().join("secrets");
    std::fs::write(&file, format!("PW_IDENTITY_KEY={}\n", key())).expect("written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let env = production(&[
        ("PW_IDENTITY_KEY", ""),
        ("PW_SECRETS_FILE", file.to_str().expect("a path")),
    ]);
    let (child, lines) = start(dir.path(), &env);
    let (code, said) = said_until_exit(child, &lines);
    assert_eq!(code, Some(1), "{said:?}");
    let all = serde_json::to_string(&said).expect("JSON");
    assert!(all.contains("may be read or written by anyone"), "{all}");
    assert!(!all.contains(key()), "{all}");

    // Named by a link: not followed, and refused unread.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let link = dir.path().join("secrets-link");
    std::os::unix::fs::symlink(&file, &link).expect("a link");
    let env = production(&[
        ("PW_IDENTITY_KEY", ""),
        ("PW_SECRETS_FILE", link.to_str().expect("a path")),
    ]);
    let (child, lines) = start(dir.path(), &env);
    let (code, said) = said_until_exit(child, &lines);
    assert_eq!(code, Some(1), "{said:?}");
    let all = serde_json::to_string(&said).expect("JSON");
    assert!(all.contains("is a symbolic link"), "{all}");
    assert!(!all.contains(key()), "{all}");
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
    let cache = dir.path().join("cache").display().to_string();
    // Read from a secrets file only its owner may read, as systemd's
    // credentials are.
    use std::os::unix::fs::PermissionsExt;
    let file = dir.path().join("secrets");
    std::fs::write(
        &file,
        format!("# the deployment's\nPW_IDENTITY_KEY={}\n", key()),
    )
    .expect("written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).expect("mode");
    let env = production(&[
        ("PW_IDENTITY_KEY", ""),
        ("PW_SECRETS_FILE", file.to_str().expect("a path")),
        ("KIOKUN_DATA", &data),
        ("PW_CACHE_DIR", &cache),
    ]);
    let cache = env
        .iter()
        .find(|(n, _)| *n == "PW_CACHE_DIR")
        .map(|(_, v)| PathBuf::from(v))
        .expect("a cache directory");
    let (child, lines) = start(dir.path(), &env);
    let (port, said) = listening(&lines);
    assert!(not_json(&said).is_empty(), "{said:?}");
    assert!(said.iter().any(|l| l["event"] == "assets"), "{said:?}");
    // kiokun's search index, built into its cache before the host listened.
    let built = std::fs::read_dir(cache.join("kiokun-search"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|e| e.path().join("index.db").is_file())
        })
        .unwrap_or(false);
    assert!(
        built && said.iter().any(|l| l["event"] == "warm"),
        "the search index was not built before the host listened: {said:?}"
    );
    assert!(
        said.iter().any(|l| l["event"] == "secrets"
            && l["msg"]
                .as_str()
                .is_some_and(|m| m.contains("PW_IDENTITY_KEY"))),
        "{said:?}"
    );
    assert!(serde_json::to_string(&said).is_ok_and(|s| !s.contains(key())));

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
        .unwrap_or_default()
        .to_string();
    assert!(!version.is_empty(), "no build's version said: {said:?}");
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
    // No new connection from now on: one made is refused, which a proxy
    // retries after the restart, not queued for a host that will not
    // answer it.
    std::thread::sleep(Duration::from_millis(300));
    let refused = TcpStream::connect(("127.0.0.1", port));
    assert!(
        refused.is_err(),
        "a connection made after the stop was taken: {refused:?}"
    );
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
        .cloned()
        .unwrap_or_default();
    assert!(
        stopped["msg"]
            .as_str()
            .is_some_and(|m| m.contains("; 0 connection(s)")),
        "{stopped} {rest:?}"
    );
    // Each request it answered said once, with its status and its route's
    // pattern, never the path a reader asked by (the review's finding 5e);
    // a health check that passed is not said; what it does not serve has
    // no route.
    let requests: Vec<&serde_json::Value> =
        rest.iter().filter(|l| l["event"] == "request").collect();
    assert!(
        requests
            .iter()
            .any(|l| l["route"] == "/word/{word}" && l["status"] == 200),
        "{rest:?}"
    );
    assert!(
        requests
            .iter()
            .any(|l| l["route"].is_null() && l["status"] == 404),
        "{rest:?}"
    );
    let all = serde_json::to_string(&rest).expect("JSON");
    for asked in [
        "%E4%BA%BA",
        "人",
        "/Old.html",
        "/sources.json",
        "/bench/clock",
    ] {
        assert!(!all.contains(asked), "the log says `{asked}`: {all}");
    }
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
    let cache = dir.path().join("cache").display().to_string();
    // Where PW_BIND says: 127.0.0.2, a loopback address of its own, which
    // the stop's wake reaches too.
    let (child, lines) = start(
        dir.path(),
        &production(&[
            ("PW_SHUTDOWN_GRACE", "1"),
            ("KIOKUN_DATA", &data),
            ("PW_CACHE_DIR", &cache),
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
        .cloned()
        .unwrap_or_default();
    assert!(
        stopped["msg"]
            .as_str()
            .is_some_and(|m| m.contains("; 1 connection(s)")),
        "{stopped} {rest:?}"
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
    /// Dropped where the server answers; never a panic, which a failing
    /// test's unwinding would make the whole binary's abort.
    fn drop(&mut self) {
        if let Ok(mut client) = postgres::Client::connect(&self.admin, postgres::NoTls) {
            let _ = client.batch_execute(&format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.name
            ));
        }
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
    let other_key = fresh_key();
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

    let (head, first) = page_with(key());
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

    let again = page_with(key()).1;
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
    let cache = dir.path().join("cache").display().to_string();
    let (child, lines) = start(
        dir.path(),
        &production(&[
            ("PW_READ_TIMEOUT", "1"),
            ("KIOKUN_DATA", &data),
            ("PW_CACHE_DIR", &cache),
        ]),
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

// ---------------------------------------------------------------------------
// The review's finding 1: a production host's connections bounded
// ---------------------------------------------------------------------------

/// **kiokun, served as production serves it**, with `extra` settings and
/// `before` done to its build first: its directory, the host, its lines,
/// and its port.
fn kiokun_host(
    extra: &[(&str, &str)],
    before: impl FnOnce(&Path),
) -> (tempfile::TempDir, Child, mpsc::Receiver<String>, u16) {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let data = built_kiokun(dir.path());
    before(dir.path());
    let cache = dir.path().join("cache").display().to_string();
    let mut env = vec![
        ("KIOKUN_DATA", data.as_str()),
        ("PW_CACHE_DIR", cache.as_str()),
    ];
    env.extend_from_slice(extra);
    let (child, lines) = start(dir.path(), &production(&env));
    let (port, said) = listening(&lines);
    assert!(not_json(&said).is_empty(), "{said:?}");
    (dir, child, lines, port)
}

/// **What a connection is answered by `within`**: its bytes, and whether
/// it ended (closed or reset) by then rather than being held.
fn answered_within(client: &mut TcpStream, within: Duration) -> (Vec<u8>, bool) {
    let until = Instant::now() + within;
    let mut got = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return (got, false);
        }
        client.set_read_timeout(Some(left)).expect("a timeout");
        match client.read(&mut buffer) {
            Ok(0) => return (got, true),
            Ok(n) => got.extend_from_slice(&buffer[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return (got, false);
            }
            Err(_) => return (got, true),
        }
    }
}

/// The status line's code of an answer's bytes, or 0 where there is none.
fn status_of(answer: &[u8]) -> u16 {
    let text = String::from_utf8_lossy(answer);
    text.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// **A request whose head trickles in is answered 408 at its deadline**
/// (the review's finding 1a: each read waited the bound anew, and a byte
/// every 1.5 s held a connection past 12 s with `PW_READ_TIMEOUT=2`): a
/// byte every 300 ms, never the head's end, and the connection is answered
/// and closed within the deadline's two seconds, not held while bytes come.
#[test]
fn a_head_that_trickles_in_is_answered_at_the_requests_deadline() {
    let (_dir, child, lines, port) = kiokun_host(&[("PW_READ_TIMEOUT", "2")], |_| {});
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let opened = Instant::now();
    let mut writer = client.try_clone().expect("a writer");
    let trickle = std::thread::spawn(move || {
        let _ = writer.write_all(b"GET /_pw/health HTTP/1.1\r\n");
        for byte in b"Host: preview.example.example.example.example.example".iter() {
            std::thread::sleep(Duration::from_millis(300));
            if writer.write_all(&[*byte]).is_err() {
                break;
            }
        }
    });
    let (answer, ended) = answered_within(&mut client, Duration::from_secs(6));
    let took = opened.elapsed();
    drop(client);
    let _ = trickle.join();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert!(
        status_of(&answer) == 408 && ended && took < Duration::from_secs(4),
        "a trickling head was answered {:?} (ended: {ended}) after {took:?}",
        String::from_utf8_lossy(&answer)
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

/// **A body that trickles in is answered 408 at the request's deadline**,
/// the head and the body one request with one deadline (the review's
/// finding 1a): a hundred bytes declared, one sent every 300 ms.
#[test]
fn a_body_that_trickles_in_is_answered_at_the_requests_deadline() {
    let (_dir, child, lines, port) = kiokun_host(&[("PW_READ_TIMEOUT", "2")], |_| {});
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let opened = Instant::now();
    let mut writer = client.try_clone().expect("a writer");
    let trickle = std::thread::spawn(move || {
        let _ = writer.write_all(
            b"POST /word/%E4%BA%BA HTTP/1.1\r\nHost: preview.example\r\nContent-Length: 100\r\n\r\n",
        );
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(300));
            if writer.write_all(b"x").is_err() {
                break;
            }
        }
    });
    let (answer, ended) = answered_within(&mut client, Duration::from_secs(6));
    let took = opened.elapsed();
    drop(client);
    let _ = trickle.join();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert!(
        status_of(&answer) == 408 && ended && took < Duration::from_secs(4),
        "a trickling body was answered {:?} (ended: {ended}) after {took:?}",
        String::from_utf8_lossy(&answer)
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

/// **An answer is written by one deadline, whatever its client reads**
/// (the review's finding 1a: no write was bounded): ten megabytes, more
/// than the sockets between hold, asked by a client that reads nothing are
/// cut at `PW_WRITE_TIMEOUT=2` and the connection closed; asked by one that
/// reads four kilobytes a fifth of a second, which keeps every single write
/// moving, its connection is done at the deadline all the same, so a stop
/// four seconds on finds it closed; asked by one that reads, it is whole.
#[test]
fn an_answer_is_written_by_one_deadline_whatever_its_client_reads() {
    let filler = "a stylesheet's line, of no consequence\n".repeat(8 << 20 >> 5);
    let (_dir, child, lines, port) = kiokun_host(
        &[("PW_WRITE_TIMEOUT", "2"), ("PW_SHUTDOWN_GRACE", "1")],
        |dir| {
            std::fs::write(dir.join("filler.css"), &filler).expect("a large asset");
        },
    );
    let ask = "GET /filler.css HTTP/1.1\r\nHost: preview.example\r\n\r\n";
    // Its declared length and the body's, or none where no head came.
    let length_of = |answer: &[u8]| -> Option<(usize, usize)> {
        let split = answer.windows(4).position(|w| w == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&answer[..split]).into_owned();
        let declared = field(&head, "content-length")?.parse().ok()?;
        Some((declared, answer.len() - split - 4))
    };
    // A client that reads.
    let mut reader = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    reader.write_all(ask.as_bytes()).expect("asked");
    let (whole, _) = answered_within(&mut reader, Duration::from_secs(20));
    // A client that reads a little, often, from now until after the stop.
    let mut trickle = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    trickle.write_all(ask.as_bytes()).expect("asked");
    let trickling = std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let until = Instant::now() + Duration::from_secs(7);
        let _ = trickle.set_read_timeout(Some(Duration::from_millis(200)));
        while Instant::now() < until {
            if matches!(trickle.read(&mut buffer), Ok(0)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    // A client that reads nothing for four seconds.
    let mut slow = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    slow.write_all(ask.as_bytes()).expect("asked");
    std::thread::sleep(Duration::from_secs(4));
    signal(&child, "TERM");
    let (answer, ended) = answered_within(&mut slow, Duration::from_secs(20));
    let (code, rest) = said_until_exit(child, &lines);
    let _ = trickling.join();
    let cut = rest
        .iter()
        .find(|l| l["event"] == "stopped")
        .and_then(|l| l["msg"].as_str())
        .unwrap_or_default()
        .to_string();
    assert_eq!(length_of(&whole), Some((filler.len(), filler.len())));
    assert!(
        ended && length_of(&answer).is_some_and(|(declared, body)| body < declared),
        "an answer no one read was written past its deadline: {:?} (ended: {ended})",
        length_of(&answer)
    );
    assert!(
        cut.contains("; 0 connection(s)"),
        "an answer to a client that reads a little was still being written at the stop: {cut}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

/// How many threads the process `pid` has (Linux's `/proc`).
fn threads(pid: u32) -> Option<usize> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("Threads:"))
        .and_then(|n| n.trim().parse().ok())
}

/// **Connections past the limit are answered 503 by the accepting thread**
/// (the review's finding 1b: 2,000 connections held 2,036 threads): with
/// `PW_MAX_CONNECTIONS=4` and four held open, a fifth is answered at once,
/// `503` and `Retry-After`, and no thread is started for it; once one of
/// the four closes, a connection is answered again.
#[test]
fn connections_past_the_limit_are_answered_503_by_the_accepting_thread() {
    let (_dir, child, lines, port) = kiokun_host(
        &[("PW_MAX_CONNECTIONS", "4"), ("PW_READ_TIMEOUT", "30")],
        |_| {},
    );
    let mut held: Vec<TcpStream> = (0..4)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).expect("connect"))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let before = threads(child.id());
    let mut fifth = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let asked = Instant::now();
    let _ = fifth.write_all(b"GET /_pw/health HTTP/1.1\r\nHost: preview.example\r\n\r\n");
    let (busy, ended) = answered_within(&mut fifth, Duration::from_secs(5));
    let took = asked.elapsed();
    let after = threads(child.id());
    let busy = String::from_utf8_lossy(&busy).into_owned();
    // One of the four closes: its place is taken again.
    drop(held.pop());
    std::thread::sleep(Duration::from_millis(500));
    let (head, _) = get(port, "/_pw/health", "");
    drop(held);
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert!(
        busy.starts_with("HTTP/1.1 503") && ended && took < Duration::from_secs(2),
        "a fifth connection at a limit of four was answered {busy:?} after {took:?}"
    );
    assert_eq!(field(&busy, "retry-after"), Some("1"), "{busy}");
    assert_eq!(
        field(&busy, "strict-transport-security"),
        Some("max-age=31536000"),
        "{busy}"
    );
    if cfg!(target_os = "linux") {
        assert!(
            matches!((before, after), (Some(b), Some(a)) if a <= b),
            "a thread was started for a connection turned away: {before:?} then {after:?}"
        );
    }
    assert_eq!(status(&head), 200, "{head}");
    assert!(
        rest.iter().any(|l| l["event"] == "busy"),
        "the turning away is said: {rest:?}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

/// **A connection the system gives no thread is dropped, and the host
/// lives** (the review's finding 1c: `std::thread::spawn` panicked in the
/// accepting thread under `prlimit --nproc`, the host exited 101 and
/// refused everything after): the host's process limit set to one while a
/// connection comes, which it drops unanswered and says so once; the limit
/// restored, the next is answered. Not where the tests run as root, whom
/// the limit does not hold.
#[cfg(target_os = "linux")]
#[test]
fn a_connection_the_system_gives_no_thread_is_dropped_and_the_host_lives() {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root's threads are not held to RLIMIT_NPROC");
        return;
    }
    let (_dir, child, lines, port) = kiokun_host(&[], |_| {});
    let pid = child.id() as libc::pid_t;
    let mut was = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let one = |max: libc::rlim_t| libc::rlimit {
        rlim_cur: 1,
        rlim_max: max,
    };
    // SAFETY: a valid pid of this test's child and valid rlimit structs.
    let limited = unsafe { libc::prlimit(pid, libc::RLIMIT_NPROC, std::ptr::null(), &mut was) }
        == 0
        && unsafe {
            libc::prlimit(
                pid,
                libc::RLIMIT_NPROC,
                &one(was.rlim_max),
                std::ptr::null_mut(),
            )
        } == 0;
    assert!(limited, "{}", std::io::Error::last_os_error());
    let mut refused = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let _ = refused.write_all(b"GET /_pw/health HTTP/1.1\r\nHost: preview.example\r\n\r\n");
    let (dropped, ended) = answered_within(&mut refused, Duration::from_secs(5));
    // SAFETY: as above, the limit the host had.
    let restored = unsafe { libc::prlimit(pid, libc::RLIMIT_NPROC, &was, std::ptr::null_mut()) };
    let again = TcpStream::connect(("127.0.0.1", port)).map(|mut c| {
        let _ = c.write_all(b"GET /_pw/health HTTP/1.1\r\nHost: preview.example\r\n\r\n");
        answered_within(&mut c, Duration::from_secs(5)).0
    });
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert_eq!(restored, 0, "the limit was restored");
    assert!(
        dropped.is_empty() && ended,
        "a connection with no thread was answered {:?} (ended: {ended})",
        String::from_utf8_lossy(&dropped)
    );
    assert!(
        again
            .as_ref()
            .is_ok_and(|a| String::from_utf8_lossy(a).starts_with("HTTP/1.1 200")),
        "the host did not live: {again:?} {rest:?}"
    );
    assert!(
        rest.iter().any(|l| l["event"] == "spawn-failed"),
        "the dropped connection is said: {rest:?}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

// ---------------------------------------------------------------------------
// The review's finding 2: what a production store may not run without
// ---------------------------------------------------------------------------

/// **A production store with no database is refused at start, by its
/// reason** (the review's finding 2: a host that never refused it passed
/// every test): its data would live in the process's memory alone.
#[test]
fn a_production_store_with_no_database_is_refused() {
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    built_store(dir.path());
    let (child, lines) = start(dir.path(), &production(&[]));
    let (code, said) = said_until_exit_within(child, &lines, Duration::from_secs(20));
    let reasons: Vec<&str> = said
        .iter()
        .filter(|l| l["event"] == "refused")
        .filter_map(|l| l["msg"].as_str())
        .collect();
    assert_eq!(code, Some(1), "{said:?}");
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("PW_STORE_DATABASE_URL") && r.contains("process's memory")),
        "{reasons:?}"
    );
    assert!(not_json(&said).is_empty(), "{said:?}");
}

/// Each resource entry a document's manifest names (`"entry":"<id>"`).
fn entries(page: &str) -> std::collections::BTreeSet<String> {
    page.split("\"entry\":\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id.to_string()))
        .collect()
}

/// **A production store's resource entries are keyed by the deployment's
/// key, and are not the development key's** (the review's finding 2: a
/// host that keyed them with the development key passed every test): one
/// session's entries are the same after a restart under the same key, and
/// share nothing with another key's, or with a development server's, which
/// keys them with the key this repository publishes.
#[test]
fn a_production_stores_entries_are_its_keys_and_not_the_development_keys() {
    let Some(database) = Database::new("entries") else {
        return;
    };
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    built_store(dir.path());
    let session = "Cookie: pw-session=s-deploy-entries\r\n";
    let page_from = |env: &[(&'static str, String)]| -> String {
        let (child, lines) = start(dir.path(), env);
        let (port, _) = listening(&lines);
        let (head, page) = get(port, "/stores/47", session);
        signal(&child, "TERM");
        let _ = said_until_exit(child, &lines);
        assert_eq!(status(&head), 200, "{head}");
        String::from_utf8(page).expect("a page in text")
    };
    let production_with = |key: &str| {
        page_from(&production(&[
            ("PW_IDENTITY_KEY", key),
            ("PW_STORE_DATABASE_URL", &database.url()),
        ]))
    };
    let first = entries(&production_with(key()));
    let again = entries(&production_with(key()));
    let other = entries(&production_with(&fresh_key()));
    let development = entries(&page_from(&[
        ("PORT", "0".to_string()),
        ("PW_IDENTITY", "none".to_string()),
    ]));
    assert!(!first.is_empty(), "the store's page names no entry");
    assert!(
        !development.is_empty(),
        "the development page names no entry"
    );
    assert_eq!(first, again, "the same key, another entry");
    assert!(
        first.is_disjoint(&other),
        "another key made the same entries: {first:?} {other:?}"
    );
    assert!(
        first.is_disjoint(&development),
        "a production host's entries are the development key's: {first:?} {development:?}"
    );
}

// ---------------------------------------------------------------------------
// The review's finding 1e: readiness while the database stalls
// ---------------------------------------------------------------------------

/// **A TCP passage to a database that can be told to stall**: while
/// stalled it passes nothing either way and holds every connection open,
/// as a database that stopped answering does; it counts the connections it
/// took. With no database behind it, it stalls every connection from the
/// first.
struct Stall {
    port: u16,
    stalled: Arc<AtomicBool>,
    taken: Arc<AtomicUsize>,
}

impl Stall {
    fn to(behind: Option<std::net::SocketAddr>) -> Stall {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("an address").port();
        let stalled = Arc::new(AtomicBool::new(behind.is_none()));
        let taken = Arc::new(AtomicUsize::new(0));
        let (stall, count) = (stalled.clone(), taken.clone());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for client in listener.incoming().map_while(Result::ok) {
                count.fetch_add(1, Ordering::SeqCst);
                let Some(server) = behind.and_then(|at| TcpStream::connect(at).ok()) else {
                    held.push(client);
                    continue;
                };
                for (from, to) in [
                    (client.try_clone(), server.try_clone()),
                    (server.try_clone(), client.try_clone()),
                ] {
                    let (Ok(from), Ok(to)) = (from, to) else {
                        continue;
                    };
                    let stall = stall.clone();
                    std::thread::spawn(move || pass(from, to, &stall));
                }
                held.push(client);
                held.push(server);
            }
        });
        Stall {
            port,
            stalled,
            taken,
        }
    }

    fn stall(&self, stalled: bool) {
        self.stalled.store(stalled, Ordering::SeqCst);
    }
}

/// Bytes from `from` to `to`, held while `stalled`, until either closes.
fn pass(mut from: TcpStream, mut to: TcpStream, stalled: &AtomicBool) {
    let _ = from.set_read_timeout(Some(Duration::from_millis(50)));
    let mut buffer = [0u8; 16384];
    loop {
        if stalled.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        match from.read(&mut buffer) {
            Ok(0) => {
                let _ = to.shutdown(std::net::Shutdown::Write);
                return;
            }
            Ok(n) => {
                while stalled.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if to.write_all(&buffer[..n]).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

/// The local PostgreSQL's address, from the server `PW_STORE_DATABASE_URL`
/// names; and the URL of `database` reached through `port` instead.
fn through(admin: &str, database: &str, port: u16) -> (std::net::SocketAddr, String) {
    let config: postgres::Config = admin.parse().expect("a PostgreSQL URL");
    let host = match config.get_hosts().first() {
        Some(postgres::config::Host::Tcp(host)) => host.clone(),
        _ => "127.0.0.1".to_string(),
    };
    let at = std::net::ToSocketAddrs::to_socket_addrs(&(
        host.as_str(),
        config.get_ports().first().copied().unwrap_or(5432),
    ))
    .expect("an address")
    .find(|a| a.is_ipv4())
    .expect("an IPv4 address");
    let user = config.get_user().unwrap_or("postgres");
    let password = config
        .get_password()
        .map(|p| format!(":{}", String::from_utf8_lossy(p)))
        .unwrap_or_default();
    (
        at,
        format!("postgresql://{user}{password}@127.0.0.1:{port}/{database}"),
    )
}

/// **A production host's readiness answers within its bound while its
/// database stalls** (the review's finding 1e: `SELECT 1` waited without
/// end, under a lock every other check waited behind): four checks asked at
/// once each answer 503 within it, asking the database one connection's
/// worth; once the database answers again, so does the host.
#[test]
fn readiness_answers_within_its_bound_while_the_database_stalls() {
    let Some(database) = Database::new("ready") else {
        return;
    };
    let (behind, _) = through(&database.admin, &database.name, 0);
    let stall = Stall::to(Some(behind));
    let (_, url) = through(&database.admin, &database.name, stall.port);
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    built_store(dir.path());
    let (child, lines) = start(dir.path(), &production(&[("PW_STORE_DATABASE_URL", &url)]));
    let (port, _) = listening(&lines);
    let (head, _) = get(port, "/_pw/ready", "");
    let ready_before = status(&head);

    stall.stall(true);
    let taken = stall.taken.load(Ordering::SeqCst);
    let asked = Instant::now();
    let answers: Vec<(u16, Duration)> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
                let _ =
                    client.write_all(b"GET /_pw/ready HTTP/1.1\r\nHost: preview.example\r\n\r\n");
                let (answer, _) = answered_within(&mut client, Duration::from_secs(10));
                (status_of(&answer), asked.elapsed())
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().expect("a check"))
        .collect();
    // Asked again while the first check still waits on the database.
    let (head, _) = get(port, "/_pw/ready", "");
    let again = status(&head);
    let connections = stall.taken.load(Ordering::SeqCst) - taken;

    stall.stall(false);
    let until = Instant::now() + Duration::from_secs(15);
    let mut ready_after = 0;
    while Instant::now() < until {
        ready_after = status(&get(port, "/_pw/ready", "").0);
        if ready_after == 200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    assert_eq!(ready_before, 200, "{rest:?}");
    for (status, took) in &answers {
        assert!(
            *status == 503 && *took < Duration::from_secs(4),
            "a readiness check while the database stalled: {answers:?}"
        );
    }
    assert_eq!(again, 503, "{rest:?}");
    assert!(
        connections <= 2,
        "five checks while the database stalled opened {connections} connections to it"
    );
    assert_eq!(
        ready_after, 200,
        "not ready once the database answered: {rest:?}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

// ---------------------------------------------------------------------------
// The review's finding 5f: a stop, whenever it comes
// ---------------------------------------------------------------------------

/// **A stop that comes while the host starts ends the start** (the review's
/// finding 5f: a signal before the host listened was waited on, unheard,
/// for as long as its start took): a store whose database accepts and
/// never answers holds its start, and SIGTERM ends it at once, with 0, the
/// host never having listened.
#[test]
fn a_stop_while_the_host_starts_ends_the_start() {
    let stall = Stall::to(None);
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    built_store(dir.path());
    let url = format!("postgresql://pw:pw@127.0.0.1:{}/pleris", stall.port);
    let (child, lines) = start(dir.path(), &production(&[("PW_STORE_DATABASE_URL", &url)]));
    let until = Instant::now() + Duration::from_secs(60);
    while stall.taken.load(Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        stall.taken.load(Ordering::SeqCst) > 0,
        "the host never asked its database"
    );
    std::thread::sleep(Duration::from_millis(300));
    let stopped_at = Instant::now();
    signal(&child, "TERM");
    let (code, said) = said_until_exit_within(child, &lines, Duration::from_secs(10));
    let took = stopped_at.elapsed();
    assert_eq!(code, Some(0), "a start was not stopped: {said:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert!(
        said.iter().any(|l| l["event"] == "stopped"
            && l["msg"]
                .as_str()
                .is_some_and(|m| m.contains("before it listened"))),
        "{said:?}"
    );
    assert!(said.iter().all(|l| l["event"] != "listening"), "{said:?}");
}

/// **A second signal stops the host at once** (the review's finding 5f):
/// SIGTERM waits for a connection under way, up to a thirty-second grace;
/// another SIGTERM ends the process then, with 0, saying how many were cut.
#[test]
fn a_second_signal_stops_the_host_at_once() {
    let (_dir, child, lines, port) = kiokun_host(
        &[("PW_SHUTDOWN_GRACE", "30"), ("PW_READ_TIMEOUT", "60")],
        |_| {},
    );
    let mut held = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    held.write_all(b"GET /_pw/health HTTP/1.1\r\n")
        .expect("half a head");
    std::thread::sleep(Duration::from_millis(300));
    signal(&child, "TERM");
    std::thread::sleep(Duration::from_millis(500));
    let second_at = Instant::now();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit_within(child, &lines, Duration::from_secs(10));
    let took = second_at.elapsed();
    assert_eq!(code, Some(0), "{rest:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert!(
        rest.iter().any(|l| l["event"] == "stopped"
            && l["msg"]
                .as_str()
                .is_some_and(|m| m.contains("stopped at once; 1 connection(s)"))),
        "{rest:?}"
    );
    drop(held);
}

/// **A subscription ends at the stop** (the review's finding 5f: a
/// streaming subscription held the stop for its passes): a stream and a
/// long poll opened just before SIGTERM each end at once, and the process
/// with them, where their passes would have held it two seconds and one.
#[test]
fn a_subscription_ends_at_the_stop() {
    let (_dir, child, lines, port) = kiokun_host(&[("PW_SHUTDOWN_GRACE", "10")], |_| {});
    let subscribe = |query: &str| {
        let mut held = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        held.write_all(
            format!("GET /stream?{query} HTTP/1.1\r\nHost: preview.example\r\n\r\n").as_bytes(),
        )
        .expect("subscribed");
        held
    };
    let mut stream = subscribe("doc=1&since=0&mode=stream");
    std::thread::sleep(Duration::from_millis(100));
    let mut poll = subscribe("doc=2&since=0");
    std::thread::sleep(Duration::from_millis(50));
    let stopped_at = Instant::now();
    signal(&child, "TERM");
    let (_, stream_ended) = answered_within(&mut stream, Duration::from_secs(5));
    let (polled, poll_ended) = answered_within(&mut poll, Duration::from_secs(5));
    let (code, rest) = said_until_exit_within(child, &lines, Duration::from_secs(10));
    let took = stopped_at.elapsed();
    assert!(
        stream_ended && poll_ended,
        "a subscription was held: {rest:?}"
    );
    assert!(
        took < Duration::from_millis(600),
        "a subscription held the stop {took:?}: {rest:?}"
    );
    // The long poll answered with what it had: nothing, at its cursor.
    let polled = String::from_utf8_lossy(&polled).into_owned();
    assert!(
        polled.starts_with("HTTP/1.1 200") && polled.ends_with("{\"cursor\":0,\"frames\":[]}"),
        "{polled}"
    );
    assert_eq!(code, Some(0), "{rest:?}");
}

/// **A host has the descriptors its connections need, or does not start**
/// (the review's finding 1b): its soft limit raised to three descriptors a
/// connection where its hard limit allows; refused at start, by its reason,
/// where the hard limit is below them.
#[cfg(target_os = "linux")]
#[test]
fn a_host_has_the_descriptors_its_connections_need() {
    use std::os::unix::process::CommandExt;
    let dir = tempfile::TempDir::with_prefix("pw-production-").expect("a directory");
    let data = built_kiokun(dir.path());
    let cache = dir.path().join("cache").display().to_string();
    let env = production(&[("KIOKUN_DATA", &data), ("PW_CACHE_DIR", &cache)]);
    let start_limited = |soft: libc::rlim_t, hard: libc::rlim_t| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pw-dev-server"));
        command
            .arg(dir.path())
            .arg(dir.path().join("build"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for (name, value) in &env {
            command.env(name, value);
        }
        // SAFETY: setrlimit is async-signal-safe, and only lowers this
        // child's own limit before it runs the host.
        unsafe {
            command.pre_exec(move || {
                let limit = libc::rlimit {
                    rlim_cur: soft,
                    rlim_max: hard,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
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
    };
    // Room to raise: the soft limit 512, the hard 8,192.
    let (child, lines) = start_limited(512, 8192);
    let (_, said) = listening(&lines);
    let limits =
        std::fs::read_to_string(format!("/proc/{}/limits", child.id())).unwrap_or_default();
    signal(&child, "TERM");
    let (code, rest) = said_until_exit(child, &lines);
    let open_files = limits
        .lines()
        .find(|l| l.starts_with("Max open files"))
        .map(|l| l.split_whitespace().nth(3).unwrap_or("").to_string())
        .unwrap_or_default();
    assert_eq!(open_files, "3328", "{limits}\n{said:?}");
    assert_eq!(code, Some(0), "{rest:?}");
    // No room: the hard limit 1,024, under 3,328.
    let (child, lines) = start_limited(1024, 1024);
    let (code, said) = said_until_exit_within(child, &lines, Duration::from_secs(20));
    assert_eq!(code, Some(1), "{said:?}");
    assert!(
        said.iter().any(|l| l["event"] == "refused"
            && l["msg"]
                .as_str()
                .is_some_and(|m| m.contains("RLIMIT_NOFILE") && m.contains("3328"))),
        "{said:?}"
    );
}
