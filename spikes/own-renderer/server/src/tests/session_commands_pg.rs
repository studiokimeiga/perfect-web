//! Actual PostgreSQL session authority; feature selection requires a database.
use super::*;
use postgres::{Client, NoTls};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Kind {
    Feed,
    Store,
}

struct Schema {
    url: String,
    name: String,
    application: String,
}
impl Schema {
    fn new(kind: Kind) -> Self {
        let variable = match kind {
            Kind::Feed => "PW_FEED_DATABASE_URL",
            Kind::Store => "PW_STORE_DATABASE_URL",
        };
        let base =
            std::env::var(variable).expect("selected session PG gate needs its actual database");
        let name = format!(
            "pw_session_{}_{}",
            match kind {
                Kind::Feed => "feed",
                Kind::Store => "store",
            },
            identity::hex(&identity::random_bytes::<8>())
        );
        let application = format!("session_test_{name}");
        let url = format!(
            "{base}{}application_name={application}",
            if base.contains('?') { "&" } else { "?" }
        );
        let mut c = Client::connect(&url, NoTls).expect("PG fixture connection");
        c.batch_execute(&format!("CREATE SCHEMA {name}"))
            .expect("owned fresh schema CREATE without IF");
        let owned = Self {
            url,
            name,
            application,
        };
        if let Ok(path) = std::env::var("PW_SESSION_SCHEMA_RECEIPT") {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("schema receipt")
                .write_all(format!("{}\n", owned.name).as_bytes())
                .expect("schema receipt append");
        }
        owned
    }
    fn sql(&self) -> Client {
        let mut c = Client::connect(&self.url, NoTls).expect("PG fixture query connection");
        c.batch_execute(&format!("SET search_path TO {}", self.name))
            .expect("owned schema path");
        c
    }
    fn number(&self, sql: &str) -> i64 {
        self.sql()
            .query_one(sql, &[])
            .expect("PG observation")
            .get(0)
    }
    fn written(&self, kind: Kind) -> i64 {
        self.number(match kind {
            Kind::Feed => "SELECT count(*) FROM posts WHERE text='PG_SESSION_FENCE'",
            Kind::Store => "SELECT coalesce(sum(quantity),0)::bigint FROM cart_lines",
        })
    }
    fn waiting(&self) -> bool {
        self.sql().query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')", &[&self.application]).expect("actual lock observation").get(0)
    }
}
impl Drop for Schema {
    fn drop(&mut self) {
        let cleaned = (|| -> Result<(), String> {
            let mut c = Client::connect(&self.url, NoTls).map_err(|e| e.to_string())?;
            c.batch_execute(&format!("DROP SCHEMA {} CASCADE", self.name))
                .map_err(|e| e.to_string())?;
            let absent: bool = c
                .query_one(
                    "SELECT NOT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname=$1)",
                    &[&self.name],
                )
                .map_err(|e| e.to_string())?
                .get(0);
            if !absent {
                return Err("owned schema remains".into());
            }
            Ok(())
        })();
        if let Err(why) = cleaned {
            eprintln!("SESSION_SCHEMA_CLEANUP_ERROR {}: {why}", self.name);
        }
    }
}

fn pair(kind: Kind, test: impl FnOnce(&mut Server, &Server, &Schema)) {
    let db = Schema::new(kind);
    let (_dir, out) = match kind {
        Kind::Feed => built_feed_with(|app| app.to_string()),
        Kind::Store => built_from_patches_in("examples", |app| app.to_string(), &[]),
    };
    let host = || match kind {
        Kind::Feed => Server::from_build_with(
            out.clone(),
            out.clone(),
            Some(Arc::new(
                crate::feed_pg::FeedPg::open(&db.url, Some(&db.name)).expect("PG feed"),
            )),
        )
        .expect("compiled feed host"),
        Kind::Store => Server::from_build_layers(
            out.clone(),
            out.clone(),
            None,
            Some(Arc::new(
                crate::store_pg::StorePg::open_with(
                    &db.url,
                    &db.name,
                    crate::feed_pg::Isolation::Serializable,
                    crate::feed_pg::RenderSeats::default(),
                )
                .expect("PG store"),
            )),
        )
        .expect("compiled store host"),
    };
    let mut a = host();
    let b = host();
    let provider = Arc::new(super::sign_in::TestProvider::default());
    a.identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    b.identity.use_provider(provider, super::sign_in::REDIRECT);
    a.identity.signed_in_for_test("owner", "bob", "Bob");
    b.identity
        .principals()
        .admit("owner")
        .expect("other host actual admission");
    test(&mut a, &b, &db);
}
fn component(kind: Kind) -> &'static str {
    match kind {
        Kind::Feed => "feed.app.post",
        Kind::Store => ADD,
    }
}
fn args(kind: Kind) -> Vec<serde_json::Value> {
    match kind {
        Kind::Feed => vec![serde_json::json!("PG_SESSION_FENCE")],
        Kind::Store => vec![shown("espresso"), serde_json::json!(1)],
    }
}
fn logout(s: &Server) -> String {
    super::sign_in::exchanged(
        s,
        "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n",
    )
}
fn revoke_before(kind: Kind) {
    pair(kind, |a, b, db| {
        let (entered, reached) = std::sync::mpsc::channel();
        let (release, go) = std::sync::mpsc::channel();
        *a.before_command_authority.lock().expect("hook") = Some(Box::new(move |_| {
            entered.send(()).expect("observer");
            go.recv_timeout(Duration::from_secs(20)).expect("release");
        }));
        let payload = args(kind);
        let result = std::thread::scope(|scope| {
            let command = scope
                .spawn(|| a.command_answer(component(kind), "owner", &payload, Some("pg-before")));
            reached
                .recv_timeout(Duration::from_secs(20))
                .expect("real captured actor before authority acquisition");
            let out = logout(b);
            release.send(()).expect("release");
            let result = command.join().expect("command worker");
            assert!(
                out.starts_with("HTTP/1.1 303"),
                "other-host logout completed before fence"
            );
            result
        });
        assert!(
            result.is_err() || result.is_ok_and(|a| !a.committed),
            "revoked captured actor must refuse"
        );
        assert_eq!(
            db.written(kind),
            0,
            "same transaction must publish no business row"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM interactions"),
            0,
            "receipt rollback"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM outbox"),
            0,
            "event/invalidation rollback"
        );
    });
}
fn expire_during(kind: Kind) {
    pair(kind, |a, _, db| {
        a.identity
            .principals()
            .expire_after(identity::SessionLifetimes {
                idle: Duration::from_secs(2),
                absolute: Duration::from_secs(2),
            });
        let mut observer = db.sql();
        let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = expired.clone();
        *a.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                let crossed: bool = observer.query_one(
                    "SELECT clock_timestamp() > opened + interval '2 seconds' FROM sessions WHERE hash=$1",
                    &[&identity::kept_hash("owner")]).expect("actual expiry-clock observation").get(0);
                if crossed {
                    observed.store(true, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
                (Instant::now() < until)
                    .then_some(())
                    .expect("database clock observer timed out before the fixture deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
        }));
        let result = a.command_answer(component(kind), "owner", &args(kind), Some("pg-expired"));
        expired
            .load(std::sync::atomic::Ordering::SeqCst)
            .then_some(())
            .expect("fixture reached the commit seam and observed its original deadline");
        assert!(
            result.is_err() || result.is_ok_and(|a| !a.committed),
            "final DB-clock authority refuses expired work"
        );
        assert_eq!(db.written(kind), 0, "business writes rollback");
        assert_eq!(
            db.number("SELECT count(*) FROM interactions"),
            0,
            "receipt rollback"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM outbox"),
            0,
            "outbox rollback"
        );
    });
}
fn admitted_first(kind: Kind) {
    pair(kind, |a, b, db| {
        let (entered, reached) = std::sync::mpsc::channel();
        let (release, go) = std::sync::mpsc::channel();
        *a.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
            entered.send(()).expect("observer");
            go.recv_timeout(Duration::from_secs(20)).expect("release");
        }));
        let payload = args(kind);
        let blocked = std::thread::scope(|scope| {
            let command = scope
                .spawn(|| a.command_answer(component(kind), "owner", &payload, Some("pg-won")));
            reached
                .recv_timeout(Duration::from_secs(20))
                .expect("real staged writes under the authority fence");
            let closing = scope.spawn(|| logout(b));
            let until = Instant::now() + Duration::from_secs(3);
            let observed = loop {
                if closing.is_finished() {
                    break Some(false);
                }
                if db.waiting() {
                    break Some(true);
                }
                if Instant::now() >= until {
                    break None;
                }
                std::thread::yield_now();
            };
            release
                .send(())
                .expect("release command before any fixture assertion");
            let _answer = command.join().expect("command worker returns");
            let out = closing.join().expect("logout worker returns");
            assert!(
                out.starts_with("HTTP/1.1 303"),
                "logout eventually completes"
            );
            observed.expect("fixture must observe an actual PG wait or completed logout")
        });
        assert!(
            blocked,
            "revocation that loses acquisition must wait for the actual business transaction"
        );
        assert_eq!(
            db.written(kind),
            1,
            "admitted command commits exactly once before winning logout"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM interactions"),
            1,
            "receipt commits with the row"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM sessions"),
            0,
            "revocation completes after business commit"
        );
    });
}
fn receipt_restart(kind: Kind) {
    pair(kind, |a, b, db| {
        let payload = args(kind);
        let first = a
            .command_answer(component(kind), "owner", &payload, Some("pg-receipt"))
            .expect("first command");
        assert!(first.committed, "valid current actor commits");
        let retry = b
            .command_answer(component(kind), "owner", &payload, Some("pg-receipt"))
            .expect("other host recalls");
        assert!(
            retry.committed,
            "current same-generation other host recalls authoritative receipt"
        );
        assert_eq!(
            db.written(kind),
            1,
            "one business operation across both hosts"
        );
        a.identity.signed_in_for_test("owner", "carol", "New actor");
        let changed = b.command_answer(component(kind), "owner", &payload, Some("pg-receipt"));
        assert!(
            changed.is_err() || changed.is_ok_and(|a| !a.committed),
            "another generation cannot adopt the old receipt"
        );
        assert_eq!(
            db.written(kind),
            1,
            "new generation cannot silently rerun a committed operation"
        );
    });
}
fn source_and_repeat(kind: Kind) {
    pair(kind, |a, b, db| {
        let actor = a
            .identity
            .principals()
            .command_actor("owner")
            .expect("actor");
        {
            let mut foreign = b.data.begin("owner");
            assert!(
                foreign.authorize(&actor).is_err(),
                "foreign local authority object cannot bind this data adapter"
            );
        }
        {
            let mut own = a.data.begin("owner");
            own.authorize(&actor).expect("own source admitted");
            assert!(
                own.authorize(&actor).is_err(),
                "session-level lock must never be acquired twice"
            );
            assert!(
                own.commit(&[], &[], None, &[], None).is_err(),
                "repeated authorization poisons publication"
            );
        }
        assert_eq!(db.written(kind), 0, "no source/refusal writes");
        let live = a
            .command_answer(component(kind), "owner", &args(kind), Some("after-cleanup"))
            .expect("locks were released and pool remains usable");
        assert!(
            live.committed,
            "valid neighbor still commits after refusal cleanup"
        );
    });
}

fn unknown_commit(kind: Kind) {
    pair(kind, |a, b, db| {
        let payload = args(kind);
        // Warm an actual document, so uncertain acknowledgement also proves view convergence.
        let page = match kind {
            Kind::Feed => "/",
            Kind::Store => "/stores/47",
        };
        let warm = get_as(a, "owner", page);
        assert!(warm.starts_with("HTTP/1.1 200"), "warm private document");
        let (original, crossed) = super::scheduled_probe::client_probe(a, true);
        let response = super::sign_in::command(
            a,
            component(kind),
            "owner",
            "lost-ack",
            &serde_json::Value::Array(payload.clone()).to_string(),
        );
        assert_eq!(
            crossed.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "real adapter COMMIT crossed before acknowledgement was lost"
        );
        assert_eq!(
            db.written(kind),
            1,
            "the authoritative business row really committed"
        );
        assert_eq!(
            db.number("SELECT count(*) FROM interactions"),
            1,
            "real durable receipt exists despite its hidden read"
        );
        assert!(
            response.contains("\"outcome\":\"unknown\"")
                && !response.contains("\"committed\":false")
                && !response.contains("\"result\""),
            "an inconclusive receipt read cannot become a cached rollback answer: {response}"
        );
        let original_document = manifest_of(warm.split_once("\r\n\r\n").expect("body").1)["cursor"]
            .as_u64()
            .expect("warm document");
        let updated_values = entry_values(a, &("owner".into(), original_document));
        let converged = updated_values.iter().any(|(_, value, _, _)| match kind {
            Kind::Feed => value.to_string().contains("PG_SESSION_FENCE"),
            Kind::Store => value["lines"].as_array().is_some_and(|lines| {
                lines
                    .iter()
                    .any(|line| line["quantity"] == 1 && line["item_id"] == "espresso")
            }),
        });
        assert!(
            converged,
            "the original warm document must receive the actual committed value before any new GET or source restoration: {updated_values:?}"
        );
        // Model bounded-bookkeeping eviction through the exact production
        // forget API: an unknown reservation is retained after its sent row goes.
        let (_, cache_key, _) = a.receipt_keys("owner", component(kind), "lost-ack", &payload);
        a.interactions
            .lock()
            .expect("interactions")
            .get_mut("owner")
            .expect("sent row")
            .clear();
        a.commands.forget_command(&cache_key);
        assert!(
            matches!(a.commands.command_result(&cache_key), Some(Err(_))),
            "an unknown reservation survives bounded interaction bookkeeping eviction"
        );
        let document = manifest_of(warm.split_once("\r\n\r\n").expect("body").1)["cursor"]
            .as_u64()
            .expect("document");
        a.revoke_document(&("owner".into(), document));
        let stale = super::sign_in::exchanged(
            a,
            &format!(
                "POST /command/{} HTTP/1.1\r\nHost: t\r\nCookie: pw-session=owner\r\nSec-Fetch-Site: same-origin\r\npw-interaction: lost-ack\r\npw-document: {document}\r\nContent-Length: {}\r\n\r\n{}",
                component(kind),
                serde_json::Value::Array(payload.clone()).to_string().len(),
                serde_json::Value::Array(payload.clone())
            ),
        );
        assert!(
            stale.contains("\"outcome\":\"unknown\"") && !stale.contains("\"committed\":false"),
            "evicted bookkeeping cannot turn an unknown commit into rollback: {stale}"
        );
        let retry = super::sign_in::command(
            a,
            component(kind),
            "owner",
            "lost-ack",
            &serde_json::Value::Array(payload.clone()).to_string(),
        );
        assert!(
            retry.contains("\"outcome\":\"unknown\""),
            "unknown reservation stays unknown"
        );
        assert_eq!(
            crossed.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "unknown operation is not re-executed"
        );
        assert_eq!(db.written(kind), 1);
        // A different host can read the authoritative receipt and confirm it.
        let recalled = b
            .command_answer(component(kind), "owner", &payload, Some("lost-ack"))
            .expect("authoritative recovery");
        assert!(
            recalled.committed,
            "a real visible receipt resolves uncertainty"
        );
        a.data = original;
        let restored = a
            .command_answer(component(kind), "owner", &payload, Some("lost-ack"))
            .expect("same-host visible receipt recovery");
        assert!(restored.committed);
        assert_eq!(
            db.written(kind),
            1,
            "recovery never repeats the business write"
        );
        let current = get_as(a, "owner", page);
        match kind {
            Kind::Feed => assert!(
                current.contains("PG_SESSION_FENCE"),
                "warm same-host document converges to actual commit"
            ),
            Kind::Store => assert_eq!(
                a.cart_value("owner"),
                1,
                "same-host materialized cart refreshes after uncertain commit"
            ),
        }
    });
}

#[test]
fn anonymous_authority_open_waits_for_its_admitted_store_transaction() {
    pair(Kind::Store, |a, b, db| {
        let (entered, reached) = std::sync::mpsc::channel();
        let (release, go) = std::sync::mpsc::channel();
        *a.before_commit.lock().expect("hook") = Some(Box::new(move |_| {
            entered.send(()).expect("observer");
            go.recv_timeout(Duration::from_secs(20)).expect("release");
        }));
        let payload = args(Kind::Store);
        let blocked = std::thread::scope(|scope| {
            let command =
                scope.spawn(|| a.command_answer(ADD, "anonymous", &payload, Some("anon-first")));
            reached
                .recv_timeout(Duration::from_secs(20))
                .expect("real anonymous transaction reaches pre-commit seam");
            let opening = scope.spawn(|| {
                b.identity
                    .signed_in_for_test("anonymous", "carol", "New actor")
            });
            let until = Instant::now() + Duration::from_secs(3);
            let observed = loop {
                if opening.is_finished() {
                    break Some(false);
                }
                if db.waiting() {
                    break Some(true);
                }
                if Instant::now() >= until {
                    break None;
                }
                std::thread::yield_now();
            };
            release.send(()).expect("release before fixture assertion");
            let _answer = command.join().expect("command worker");
            opening.join().expect("sign-in worker");
            observed.expect("must observe lock wait or completed open")
        });
        assert!(
            blocked,
            "an absent session row still needs authority serialization"
        );
        assert_eq!(
            db.written(Kind::Store),
            1,
            "one original anonymous write commits before the new actor opens"
        );
    });
}

#[test]
fn a_false_advisory_unlock_is_not_release_proof() {
    let db = Schema::new(Kind::Feed);
    let mut c = db.sql();
    let hash = identity::kept_hash("lock-only-fixture");
    crate::hosts::command_session_lock(&mut c, &hash, || {}).expect("real advisory acquisition");
    crate::hosts::command_session_unlock(&mut c, &hash, || {}).expect("first real release");
    assert!(
        crate::hosts::command_session_unlock(&mut c, &hash, || {}).is_err(),
        "a successful RPC returning false cannot authorize pooling"
    );
}

macro_rules! cases { ($($name:ident,$kind:ident,$test:ident);* $(;)?) => { $( #[test] fn $name(){ $test(Kind::$kind); } )* }; }
cases! {
    feed_lost_ack_and_hidden_receipt_remain_unknown,Feed,unknown_commit;
    store_lost_ack_and_hidden_receipt_remain_unknown,Store,unknown_commit;
    feed_revoke_before_transaction_authority,Feed,revoke_before;
    store_revoke_before_transaction_authority,Store,revoke_before;
    feed_expiry_after_evaluation_rolls_back,Feed,expire_during;
    store_expiry_after_evaluation_rolls_back,Store,expire_during;
    feed_admitted_command_precedes_revocation,Feed,admitted_first;
    store_admitted_command_precedes_revocation,Store,admitted_first;
    feed_receipts_keep_generation_across_hosts,Feed,receipt_restart;
    store_receipts_keep_generation_across_hosts,Store,receipt_restart;
    feed_source_binding_and_repeated_acquisition_refuse,Feed,source_and_repeat;
    store_source_binding_and_repeated_acquisition_refuse,Store,source_and_repeat;
}
