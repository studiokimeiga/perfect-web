//! Real receipt storage faults and recovery across a forgotten document.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const COMMAND: &str = "feed.app.post";

fn memory() -> Served {
    let (dir, out) = built_feed_with(|app| app.to_string());
    let s = Served {
        server: Server::from_build_with(
            out.clone(),
            out,
            Some(Arc::new(crate::feed::FeedData::new())),
        )
        .expect("explicit compiled memory feed"),
        _dir: dir,
    };
    s.identity.signed_in_for_test("owner", "bob", "Bob");
    s.identity.signed_in_for_test("other", "carol", "Carol");
    s
}

fn post(s: &Server, document: Option<u64>, id: &str, text: &str) -> String {
    let body = serde_json::json!([text]).to_string();
    let page = document
        .map(|n| format!("pw-document: {n}\r\n"))
        .unwrap_or_default();
    super::sign_in::exchanged(
        s,
        &format!(
            "POST /command/{COMMAND} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\npw-interaction: {id}\r\n{page}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn home(s: &Server) -> u64 {
    let page = get_as(s, "owner", "/");
    page.starts_with("HTTP/1.1 200")
        .then_some(())
        .expect("actual served page");
    manifest_of(page.split_once("\r\n\r\n").expect("body").1)["cursor"]
        .as_u64()
        .expect("actual document")
}

fn posts(s: &Server, text: &str) -> usize {
    let timeline = s
        .query(
            "feed.app.Timeline",
            "other",
            &[Val::String("other".into()), Val::S64(100)],
        )
        .expect("actual compiled timeline");
    val_to_json(&timeline)
        .as_array()
        .expect("timeline list")
        .iter()
        .filter(|row| row["text"] == text)
        .count()
}

fn record(s: &Server, key: &str) -> (String, String) {
    let mut found = None;
    s.materializer
        .command::<String>(|tx| {
            found = Some(
                tx.query_row(
                    "SELECT sent, answer FROM interactions WHERE key = ?1",
                    rusqlite::params![key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| "receipt fixture read failed".to_string())?,
            );
            Ok(Vec::new())
        })
        .expect("actual materializer receipt observation");
    found.expect("physical committed receipt")
}

fn set_answer(s: &Server, key: &str, answer: &str) {
    s.materializer
        .command::<String>(|tx| {
            let changed = tx
                .execute(
                    "UPDATE interactions SET answer = ?1 WHERE key = ?2",
                    rusqlite::params![answer, key],
                )
                .map_err(|_| "receipt fixture write failed".to_string())?;
            if changed != 1 {
                return Err("receipt fixture did not change one existing row".to_string());
            }
            Ok(Vec::new())
        })
        .expect("only the answer of the actual committed receipt changed");
}

#[test]
fn malformed_persisted_receipts_never_invent_rollback_or_reexecute() {
    let s = memory();
    let cases = [
        ("invalid-json", "{"),
        ("missing-committed", "{}"),
        ("non-boolean", r#"{"committed":"true"}"#),
        ("invalid-why", r#"{"committed":true,"why":7}"#),
        ("invalid-refusal", r#"{"committed":true,"refused":[]}"#),
        (
            "committed-refusal",
            r#"{"committed":true,"refused":"SignedIn"}"#,
        ),
        (
            "committed-error",
            r#"{"committed":true,"why":"PRIVATE_CORRUPT_REASON"}"#,
        ),
        (
            "durable-negative",
            r#"{"committed":false,"why":null,"refused":null}"#,
        ),
    ];
    for (id, malformed) in cases {
        println!("receipt fault case: {id}");
        let text = format!("ACTUAL_RECEIPT_{id}");
        let document = home(&s);
        let first = post(&s, Some(document), id, &text);
        (first.starts_with("HTTP/1.1 202") && first.contains("\"committed\":true"))
            .then_some(())
            .expect("the actual compiled write committed");
        let payload = [serde_json::json!(text)];
        let (key, _, sent) = s.receipt_keys("owner", COMMAND, id, &payload);
        let healthy = record(&s, &key);
        (healthy.0 == sent)
            .then_some(())
            .expect("the real receipt has this reader and arguments");
        (posts(&s, &text) == 1)
            .then_some(())
            .expect("the business row really committed");
        let neighbor = post(&s, Some(document), id, &text);
        assert!(
            neighbor.contains("\"committed\":true"),
            "healthy same-reader replay succeeds"
        );
        set_answer(&s, &key, malformed);
        let stored = record(&s, &key);
        (stored.0 == healthy.0 && stored.1 == malformed)
            .then_some(())
            .expect("only the answer is corrupted; the exact envelope and row remain");
        let replay = post(&s, Some(document), id, &text);
        // Restore and check retained state before the confidentiality/outcome oracle.
        set_answer(&s, &key, &healthy.1);
        assert_eq!(
            posts(&s, &text),
            1,
            "receipt recovery never runs the write twice"
        );
        assert!(
            record(&s, &key) == healthy,
            "the physical receipt is retained"
        );
        assert!(
            replay.contains("\"outcome\":\"unknown\"")
                && !replay.contains("\"committed\":false")
                && !replay.contains("\"result\""),
            "malformed committed receipt must refuse as unknown, never invent rollback"
        );
        let restored = post(&s, Some(document), id, &text);
        assert!(
            restored.contains("\"committed\":true"),
            "restored valid receipt still replays"
        );
        // Force the stale-document durable path, independently of the healthy
        // process cache and its bounded sent-argument bookkeeping.
        s.interactions
            .lock()
            .expect("sent bookkeeping")
            .get_mut("owner")
            .expect("actual sent row")
            .clear();
        s.commands
            .forget_command(&s.receipt_keys("owner", COMMAND, id, &payload).1);
        set_answer(&s, &key, malformed);
        s.revoke_document(&("owner".into(), document));
        let stale = post(&s, Some(document), id, &text);
        set_answer(&s, &key, &healthy.1);
        assert_eq!(
            posts(&s, &text),
            1,
            "stale malformed replay never repeats business writes"
        );
        assert!(
            stale.contains("\"outcome\":\"unknown\"")
                && !stale.contains("\"committed\":false")
                && !stale.contains("\"result\""),
            "stale-document malformed receipt must also refuse as unknown"
        );
        let stale_neighbor = post(&s, Some(document), id, &text);
        assert!(
            stale_neighbor.contains("\"committed\":true"),
            "stale same-reader valid durable receipt remains a positive neighbor"
        );
    }
}

#[test]
fn materializer_receipt_read_error_never_reenters_command_evaluation() {
    let s = memory();
    let id = "decode-read-error";
    let text = "READ_ERROR_MUST_NOT_EVALUATE_AGAIN";
    let first = post(&s, None, id, text);
    first
        .contains("\"committed\":true")
        .then_some(())
        .expect("real compiled command committed");
    let payload = [serde_json::json!(text)];
    let (key, cache, _) = s.receipt_keys("owner", COMMAND, id, &payload);
    let healthy = record(&s, &key);
    s.commands
        .forget_command(&cache)
        .then_some(())
        .expect("only the finished RAM answer is evicted");
    s.materializer
        .command::<String>(|tx| {
            tx.execute(
                "UPDATE interactions SET answer = ?1 WHERE key = ?2",
                rusqlite::params![vec![0xff_u8], &key],
            )
            .map_err(|_| "fault write failed".to_string())?;
            let storage: String = tx
                .query_row(
                    "SELECT typeof(answer) FROM interactions WHERE key = ?1",
                    rusqlite::params![&key],
                    |row| row.get(0),
                )
                .map_err(|_| "fault row absent".to_string())?;
            let decoded = tx.query_row(
                "SELECT answer FROM interactions WHERE key = ?1",
                rusqlite::params![&key],
                |row| row.get::<_, String>(0),
            );
            if storage != "blob" || decoded.is_ok() {
                return Err("the actual receipt decode fault was not armed".to_string());
            }
            Ok(Vec::new())
        })
        .expect("a physically present receipt has a real read-only decode failure");
    let authority = Arc::new(AtomicUsize::new(0));
    let evaluated = Arc::new(AtomicUsize::new(0));
    let (entered, completed) = (authority.clone(), evaluated.clone());
    let (restore_key, restore_answer) = (key.clone(), healthy.1.clone());
    *s.before_command_authority.lock().expect("authority hook") = Some(Box::new(move |server| {
        // Any later action sees healthy storage. A subsequent write/setup fault
        // therefore cannot stand in for the initial SELECT decode failure.
        set_answer(server, &restore_key, &restore_answer);
        entered.fetch_add(1, Ordering::SeqCst);
    }));
    *s.before_commit.lock().expect("evaluation hook") = Some(Box::new(move |_| {
        completed.fetch_add(1, Ordering::SeqCst);
    }));
    let refused = post(&s, None, id, text);
    s.before_command_authority
        .lock()
        .expect("authority hook")
        .take();
    s.before_commit.lock().expect("evaluation hook").take();
    set_answer(&s, &key, &healthy.1);
    let restored = post(&s, None, id, text);
    assert!(
        restored.contains("\"committed\":true"),
        "restored authoritative receipt replays"
    );
    assert_eq!(
        posts(&s, text),
        1,
        "the original business row remains the only commit"
    );
    assert!(
        record(&s, &key) == healthy,
        "the original receipt survives the fault"
    );
    assert_eq!(
        authority.load(Ordering::SeqCst),
        0,
        "an unreadable committed receipt was treated as a miss and entered command authority"
    );
    assert_eq!(
        evaluated.load(Ordering::SeqCst),
        0,
        "an unreadable committed receipt must not run the actual compiled command again"
    );
    assert!(
        refused.contains("\"outcome\":\"unknown\"") && !refused.contains("\"committed\":false"),
        "read failure refuses without claiming rollback"
    );
}

#[cfg(feature = "session-postgres-tests")]
struct PgReceipt {
    url: String,
    schema: String,
}
#[cfg(feature = "session-postgres-tests")]
impl PgReceipt {
    fn new() -> Self {
        let url = std::env::var("PW_FEED_DATABASE_URL")
            .expect("selected PG receipt test needs its actual DB");
        let schema = format!(
            "pw_session_receipt_{}",
            identity::hex(&identity::random_bytes::<8>())
        );
        postgres::Client::connect(&url, postgres::NoTls)
            .expect("PG connection")
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .expect("fresh owned schema");
        let owned = Self { url, schema };
        if let Ok(path) = std::env::var("PW_SESSION_SCHEMA_RECEIPT") {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("schema receipt")
                .write_all(format!("{}\n", owned.schema).as_bytes())
                .expect("schema receipt append");
        }
        owned
    }
    fn sql(&self) -> postgres::Client {
        let mut c = postgres::Client::connect(&self.url, postgres::NoTls).expect("PG observation");
        c.batch_execute(&format!("SET search_path TO {}", self.schema))
            .expect("owned schema path");
        c
    }
    fn count(&self, table: &str) -> i64 {
        // All callers below use one of these fixed owned fixture tables.
        if !matches!(table, "posts" | "interactions") {
            panic!("unknown fixture table");
        }
        self.sql()
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .expect("physical PG row observation")
            .get(0)
    }
}
#[cfg(feature = "session-postgres-tests")]
impl Drop for PgReceipt {
    fn drop(&mut self) {
        let cleaned = (|| -> Result<(), String> {
            let mut c = postgres::Client::connect(&self.url, postgres::NoTls)
                .map_err(|_| "cleanup connection")?;
            c.batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .map_err(|_| "cleanup DROP")?;
            let absent: bool = c
                .query_one(
                    "SELECT NOT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname=$1)",
                    &[&self.schema],
                )
                .map_err(|_| "cleanup absence query")?
                .get(0);
            if !absent {
                return Err("owned receipt schema remains".into());
            }
            Ok(())
        })();
        if cleaned.is_err() {
            eprintln!(
                "SESSION_SCHEMA_CLEANUP_ERROR {}: receipt cleanup failed",
                self.schema
            );
        }
    }
}

#[cfg(feature = "session-postgres-tests")]
#[test]
fn a_stale_same_reader_recovers_a_visible_receipt_after_actual_lost_acknowledgement() {
    let db = PgReceipt::new();
    let (dir, out) = built_feed_with(|app| app.to_string());
    let mut s = Served {
        server: Server::from_build_with(
            out.clone(),
            out,
            Some(Arc::new(
                crate::feed_pg::FeedPg::open(&db.url, Some(&db.schema)).expect("real PG feed"),
            )),
        )
        .expect("compiled PG host"),
        _dir: dir,
    };
    s.identity.use_provider(
        Arc::new(super::sign_in::TestProvider::default()),
        super::sign_in::REDIRECT,
    );
    s.identity.signed_in_for_test("owner", "bob", "Bob");
    let document = home(&s);
    let (original, crossed) = super::scheduled_probe::client_probe(&mut s, true);
    let id = "restore-visible";
    let text = "ACTUAL_APPLIED_THEN_VISIBLE_RECEIPT";
    let before_posts = db.count("posts");
    let before_receipts = db.count("interactions");
    let first = post(&s, Some(document), id, text);
    (crossed.load(Ordering::SeqCst) == 1
        && db.count("posts") == before_posts + 1
        && db.count("interactions") == before_receipts + 1)
        .then_some(())
        .expect("actual adapter COMMIT and durable rows crossed before lost acknowledgement");
    first
        .contains("\"outcome\":\"unknown\"")
        .then_some(())
        .expect("the hidden receipt leaves an unknown outcome");
    let payload = [serde_json::json!(text)];
    let (_, cache, _) = s.receipt_keys("owner", COMMAND, id, &payload);
    matches!(s.commands.command_result(&cache), Some(Err(_)))
        .then_some(())
        .expect("the real lost acknowledgement retained an unknown reservation");
    s.data = original;
    s.revoke_document(&("owner".into(), document));
    let neighbor = post(&s, None, id, text);
    assert!(
        neighbor.contains("\"committed\":true"),
        "current-document-free same-reader receipt is visible"
    );
    matches!(s.commands.command_result(&cache), Some(Err(_)))
        .then_some(())
        .expect("the stale retry still faces the actual unknown reservation after the visible-receipt neighbor");
    let recovered = post(&s, Some(document), id, text);
    assert_eq!(
        db.count("posts"),
        before_posts + 1,
        "recovery never reexecutes the write"
    );
    assert_eq!(
        db.count("interactions"),
        before_receipts + 1,
        "the authoritative receipt is retained"
    );
    assert_eq!(
        crossed.load(Ordering::SeqCst),
        1,
        "only the original actual adapter commit occurred"
    );
    assert!(
        recovered.starts_with("HTTP/1.1 202") && recovered.contains("\"committed\":true"),
        "a stale same-reader retry must recover a now-visible authoritative receipt before an old unknown reservation"
    );
}

#[test]
fn a_stale_same_reader_recalls_a_known_refusal_without_a_durable_receipt() {
    let s = memory();
    let text = "KNOWN_REFUSAL_OTHER_POST";
    let other = super::sign_in::command(
        &s,
        COMMAND,
        "other",
        "other-post",
        &serde_json::json!([text]).to_string(),
    );
    other
        .contains("\"committed\":true")
        .then_some(())
        .expect("another reader's real post committed");
    let timeline = s
        .query(
            "feed.app.Timeline",
            "other",
            &[Val::String("other".into()), Val::S64(100)],
        )
        .expect("actual compiled timeline");
    let json = val_to_json(&timeline);
    let post_id = json
        .as_array()
        .expect("timeline list")
        .iter()
        .find(|row| row["text"] == text)
        .and_then(|row| row["id"].as_str())
        .expect("the real post's id");
    let document = home(&s);
    let id = "cached-refusal";
    let component = "feed.app.delete";
    let args = [serde_json::json!(post_id)];
    let body = serde_json::Value::Array(args.to_vec()).to_string();
    let request = || {
        format!(
            "POST /command/{component} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\npw-interaction: {id}\r\npw-document: {document}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    };
    let first = super::sign_in::exchanged(&s, &request());
    (first.starts_with("HTTP/1.1 403") && first.contains("\"refused\":\"OwnsPost\""))
        .then_some(())
        .expect("the actual predicate produced a known refusal");
    let (key, cache, _) = s.receipt_keys("owner", component, id, &args);
    let mut durable = None;
    s.materializer
        .command::<String>(|tx| {
            durable = Some(
                tx.query_row(
                    "SELECT count(*) FROM interactions WHERE key = ?1",
                    rusqlite::params![&key],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|_| "refusal receipt observation failed".to_string())?,
            );
            Ok(Vec::new())
        })
        .expect("actual durable absence observed");
    (durable == Some(0) && matches!(s.commands.command_result(&cache), Some(Ok(_))))
        .then_some(())
        .expect("the known refusal exists only in completed process cache");
    s.revoke_document(&("owner".into(), document));
    let replay = super::sign_in::exchanged(&s, &request());
    assert_eq!(
        posts(&s, text),
        1,
        "the refused delete never takes the other reader's post"
    );
    assert!(
        replay.starts_with("HTTP/1.1 403")
            && replay.contains("\"refused\":\"OwnsPost\"")
            && replay.contains("\"committed\":false")
            && !replay.contains("\"outcome\":\"unknown\""),
        "a genuine durable miss must preserve the same-reader completed refusal cache neighbor"
    );
}
