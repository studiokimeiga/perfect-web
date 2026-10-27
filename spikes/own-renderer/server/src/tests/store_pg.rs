//! **The store on PostgreSQL** (track `store-pg`): the store served by the
//! host, its data in a real database, held to what its `source` states.
//!
//! Each test runs against `PW_STORE_DATABASE_URL`, in a schema of its own
//! (`pw_store_test_…`) that is dropped with its layer, and passes doing
//! nothing without one: `just e14-store-postgres` is what runs them, and says
//! so when it has no database. The rest of the server's suite runs on this
//! layer too there (`PW_STORE_TEST_LAYER=postgres`); these are the tests
//! that need to see the database itself.

use super::*;
use crate::feed_pg::Isolation;
use crate::store_pg::StorePg;

/// The database the tests run against, where the environment names one.
fn database() -> Option<String> {
    match std::env::var("PW_STORE_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_STORE_DATABASE_URL is not set");
            None
        }
    }
}

/// **The store, built and served on PostgreSQL**, and its layer, to look at
/// what it committed. The server is dropped before the layer, which drops
/// its schema.
struct OnPostgres {
    served: Served,
    layer: Arc<StorePg>,
}

impl std::ops::Deref for OnPostgres {
    type Target = Server;
    fn deref(&self) -> &Server {
        &self.served
    }
}

impl OnPostgres {
    fn count(&self, query: &str) -> i64 {
        self.layer
            .sql()
            .query_one(query, &[])
            .unwrap_or_else(|e| panic!("{query}: {e}"))
            .get(0)
    }
}

/// The canonical store, its `app.pw` changed by `change` and `patches`
/// applied, served on the layer `open` opens, or why the host refused it.
fn served_with(
    change: fn(&str) -> String,
    patches: &[&str],
    open: impl FnOnce() -> Result<StorePg, String>,
) -> Result<OnPostgres, String> {
    let layer = Arc::new(open()?);
    let (dir, out) = built_from_patches_in("examples", change, patches);
    let server = Server::from_build_layers(out.clone(), out, None, Some(layer.clone()))?;
    Ok(OnPostgres {
        served: Served { server, _dir: dir },
        layer,
    })
}

/// The canonical store as it is, on PostgreSQL.
fn served(url: &str) -> OnPostgres {
    served_with(|app| app.to_string(), &[], || StorePg::for_test(url))
        .expect("served on PostgreSQL")
}

/// **Every row the outbox is given is seen**, with the transaction that
/// gave it: what a test compares with the transaction that wrote the state.
/// Installed in the test's schema only.
fn audit_the_outbox(s: &OnPostgres) {
    s.layer
        .sql()
        .batch_execute(
            "CREATE TABLE outbox_seen (name text NOT NULL, committed_in xid8 NOT NULL);
             CREATE FUNCTION outbox_seen() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 INSERT INTO outbox_seen VALUES (NEW.name, NEW.committed_in);
                 RETURN NULL;
             END $$;
             CREATE TRIGGER outbox_seen AFTER INSERT ON outbox
                 FOR EACH ROW EXECUTE FUNCTION outbox_seen();",
        )
        .expect("the outbox audited");
}

fn add(s: &Server, session: &str, item: &str, quantity: i64, interaction: &str) -> Answered {
    s.command_answered(ADD, session, &add_shown(item, quantity), Some(interaction))
        .expect("runs")
}

/// **What the database provides, measured** (ADR-0246): a command's
/// transaction is serializable though the connection's default is read
/// committed, a read is of the latest commit, and no change feed. And the
/// store's connections search its own schema alone.
#[test]
fn on_postgres_the_database_provides_what_the_store_states() {
    let Some(url) = database() else { return };
    let layer = StorePg::for_test(&url).expect("opened");
    let mut sql = layer.sql();
    let default: String = sql
        .query_one("SHOW default_transaction_isolation", &[])
        .expect("shown")
        .get(0);
    assert_eq!(
        default, "read committed",
        "the control: the server's default"
    );
    let provided = crate::data::DataLayer::provides(&layer).expect("measured");
    assert_eq!(provided.transactions, "serializable");
    assert_eq!(
        provided.reads,
        std::collections::BTreeSet::from(["strong".to_string()])
    );
    assert!(!provided.feed);
    let path: String = sql
        .query_one("SHOW search_path", &[])
        .expect("shown")
        .get(0);
    assert_eq!(path, layer.schema(), "its schema alone");
}

/// **A cart's change commits with its event, in one transaction**
/// (ADR-0208): the line's row and the outbox's are written by one
/// transaction, and what was delivered is kept for the other hosts, the
/// committing host's (ADR-0369).
#[test]
fn on_postgres_a_cart_change_commits_with_its_event_in_one_transaction() {
    let Some(url) = database() else { return };
    let s = served(&url);
    audit_the_outbox(&s);
    let answered = add(&s, "a", "espresso", 2, "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    let mut sql = s.layer.sql();
    let rows = sql
        .query(
            "SELECT s.name, s.committed_in = c.committed_in \
             FROM outbox_seen s, cart_lines c WHERE c.owner = 'u-a'",
            &[],
        )
        .expect("read");
    let seen: Vec<(String, bool)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    // Its event, and the entry it drops (ADR-0209), each written by the
    // line's own transaction.
    assert_eq!(
        seen,
        [
            // The reader's, session `a`'s guest's (track `store-accounts`).
            ("Events.UserCartChanged".to_string(), true),
            ("store.page.Cart".to_string(), true)
        ],
        "written by the line's own transaction"
    );
    // Delivered, and kept for the other hosts on the database, which read
    // them from their cursors (ADR-0369): until then they were consumed.
    let host = s.data.tail().expect("a shared database").host().to_string();
    assert_eq!(
        s.layer
            .sql()
            .query_one("SELECT count(*) FROM outbox WHERE host = $1", &[&host])
            .expect("counted")
            .get::<_, i64>(0),
        2,
        "delivered, and kept for the other hosts: the committing host's"
    );
}

/// **A route's change commits with its event, in one transaction** (track
/// `store-pg`): the kitchen moving an order along is a write the layer
/// stages, and its `OrderChanged` is the same transaction's.
#[test]
fn on_postgres_a_routes_change_commits_with_its_event_in_one_transaction() {
    let Some(url) = database() else { return };
    let s = served(&url);
    assert!(add(&s, "a", "espresso", 1, "i-1").committed);
    let placed = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], Some("i-2"))
        .expect("runs");
    assert!(placed.committed, "{:?}", placed.result);
    audit_the_outbox(&s);
    let answer = posted(&s, "/bench/order?status=preparing", "a", "", "");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let rows = s
        .layer
        .sql()
        .query(
            "SELECT s.name, s.committed_in = o.committed_in \
             FROM outbox_seen s, orders o WHERE o.owner = 'u-a'",
            &[],
        )
        .expect("read");
    let seen: Vec<(String, bool)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(
        seen,
        [("Events.UserOrderChanged".to_string(), true)],
        "the order's change and its event, one transaction"
    );
    assert_eq!(s.order_of("a").map(|o| o.0).as_deref(), Some("preparing"));
}

/// **A cart changed on PostgreSQL reaches the session's open page** (track
/// `store-pg`): the commit is recorded in the host's materializer, its rows
/// and its events, as the in-memory layer's is, and the session's entry
/// moves, so the page's count is patched.
#[test]
fn on_postgres_a_cart_change_reaches_the_sessions_open_page() {
    let Some(url) = database() else { return };
    let s = served(&url);
    let home = fetched_as(&s, "/", Some("a"))
        .into_iter()
        .map(|(_, c)| c)
        .collect::<String>();
    assert!(visible(&home).contains("Your cart: 0 items"), "{home}");
    let answered = add(&s, "a", "espresso", 2, "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(
        s.materializer.state("cart:a").as_deref(),
        Some("2"),
        "the commit's rows, recorded"
    );
    let count = s
        .template_of("store.page.HomePage~ui")
        .manifest()
        .into_iter()
        .find(|e| e.value == "cart.line_count")
        .expect("the home page's count")
        .id;
    assert!(
        patch_sets(&s, "a")
            .iter()
            .any(|set| set.patches.iter().any(|p| p.target.part == count
                && matches!(&p.operation, PatchOp::ReplaceText { text } if text == "2"))),
        "the home page's count was not patched"
    );
}

/// **A commit the database refuses keeps neither its state nor its event**
/// (ADR-0208, ADR-0246): a deferred constraint, checked at the commit after
/// the line and its event were written, refuses it. Nothing is in the cart
/// or the outbox, the materializer records nothing, the open page is told
/// nothing, and the next add commits.
#[test]
fn on_postgres_a_refused_commit_commits_neither_its_state_nor_its_event() {
    let Some(url) = database() else { return };
    let s = served(&url);
    audit_the_outbox(&s);
    s.layer
        .sql()
        .batch_execute(
            "CREATE FUNCTION refuse_four() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.quantity >= 4 THEN
                     RAISE EXCEPTION 'four is refused at the commit';
                 END IF;
                 RETURN NULL;
             END $$;
             CREATE CONSTRAINT TRIGGER refuse_four AFTER INSERT OR UPDATE ON cart_lines
                 DEFERRABLE INITIALLY DEFERRED
                 FOR EACH ROW EXECUTE FUNCTION refuse_four();",
        )
        .expect("the constraint installed");
    fetched_as(&s, "/", Some("a"));
    let told = patch_sets(&s, "a").len();
    let refused = s
        .command_answered(ADD, "a", &add_shown("espresso", 4), Some("i-1"))
        .expect_err("refused at its commit");
    assert!(
        refused.contains("four is refused at the commit"),
        "{refused}"
    );
    assert!(s.cart_lines("a").is_empty(), "no line");
    assert_eq!(s.count("SELECT count(*) FROM cart_lines"), 0);
    assert_eq!(s.count("SELECT count(*) FROM outbox"), 0, "no event");
    assert_eq!(
        s.count("SELECT count(*) FROM outbox_seen"),
        0,
        "none written"
    );
    assert_eq!(s.materializer.state("cart:a"), None, "nothing recorded");
    assert_eq!(s.materializer.retained_events(), 0);
    assert_eq!(patch_sets(&s, "a").len(), told, "no page told");
    let answered = add(&s, "a", "espresso", 1, "i-2");
    assert!(answered.committed, "{:?}", answered.result);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
}

/// **Eight sessions adding five times each commit all forty** (track
/// `store-pg`): one serializable transaction each, one command of the host
/// at a time, and none lost.
#[test]
fn on_postgres_eight_sessions_adding_five_times_each_commit_all_forty() {
    let Some(url) = database() else { return };
    let s = served(&url);
    std::thread::scope(|scope| {
        for session in 0..8 {
            let s = &s;
            scope.spawn(move || {
                for press in 0..5 {
                    let answered = add(
                        s,
                        &format!("s-{session}"),
                        "espresso",
                        1,
                        &format!("i-{session}-{press}"),
                    );
                    assert!(answered.committed, "{:?}", answered.result);
                }
            });
        }
    });
    for session in 0..8 {
        assert_eq!(s.cart_value(&format!("s-{session}")), 5, "s-{session}");
    }
    assert_eq!(
        s.count("SELECT sum(quantity)::bigint FROM cart_lines"),
        40,
        "all forty committed"
    );
    // Each add's two rows kept for the other hosts (ADR-0369), each
    // commit's consecutive: its ids taken under the outbox's lock.
    assert_eq!(s.count("SELECT count(*) FROM outbox"), 80);
    assert_eq!(
        s.count(
            "SELECT count(*) FROM (SELECT max(id) - min(id) + 1 AS span, count(*) AS n \
             FROM outbox GROUP BY committed_in) c WHERE span <> n"
        ),
        0,
        "each commit's rows consecutive"
    );
}

/// **A retried interaction runs its command once** (ADR-0121): the second
/// request is given the first's answer, and the database holds one add and
/// saw one event.
#[test]
fn on_postgres_a_retried_interaction_runs_its_command_once() {
    let Some(url) = database() else { return };
    let s = served(&url);
    audit_the_outbox(&s);
    let request = [shown("espresso"), serde_json::json!(1)];
    let first = s
        .command_answer(ADD, "a", &request, Some("press-1"))
        .expect("a well-formed request");
    let again = s
        .command_answer(ADD, "a", &request, Some("press-1"))
        .expect("a well-formed request");
    assert!(first.committed);
    assert_eq!(again, first, "the first answer");
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(
        s.count("SELECT count(DISTINCT committed_in) FROM outbox_seen"),
        1,
        "one transaction gave the outbox anything"
    );
    assert_eq!(
        s.count("SELECT count(*) FROM outbox_seen WHERE name = 'Events.UserCartChanged'"),
        1,
        "one event"
    );
}

/// **Hosts over one database, its layer kept between them** (ADR-0325):
/// each built as a host is when it starts, with nothing of the others in its
/// memory.
fn hosts(url: &str) -> (Arc<StorePg>, impl Fn() -> Server, tempfile::TempDir) {
    hosts_with(url, |app| app.to_string())
}

/// [`hosts`], the store's `app.pw` changed by `change`.
fn hosts_with(
    url: &str,
    change: fn(&str) -> String,
) -> (Arc<StorePg>, impl Fn() -> Server, tempfile::TempDir) {
    let layer = Arc::new(StorePg::for_test(url).expect("the layer"));
    let (dir, out) = built_from_patches_in("examples", change, &[]);
    let kept = layer.clone();
    let host = move || {
        Server::from_build_layers(out.clone(), out.clone(), None, Some(kept.clone()))
            .expect("served on PostgreSQL")
    };
    (layer, host, dir)
}

/// **An interaction's answer outlives its host** (ADR-0325): committed on
/// one host, the same interaction sent to a host started after it over the
/// same database is answered as the first was, and the cart holds the one
/// line; sent there with other arguments, it is refused. Until ADR-0325 the
/// record was the first host's memory, and the second ran it again: two.
#[test]
fn on_postgres_an_interactions_answer_outlives_its_host() {
    let Some(url) = database() else { return };
    let (_layer, host, _dir) = hosts(&url);
    let request = [shown("espresso"), serde_json::json!(1)];
    let first = {
        let s = host();
        s.command_answer(ADD, "a", &request, Some("press-1"))
            .expect("a well-formed request")
    };
    assert!(first.committed, "{first:?}");
    let s = host();
    let again = s
        .command_answer(ADD, "a", &request, Some("press-1"))
        .expect("a well-formed request");
    assert_eq!(again, first, "the first host's answer");
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    let other = [shown("cortado"), serde_json::json!(1)];
    let refused = s
        .command_answer(ADD, "a", &other, Some("press-1"))
        .expect_err("another request under the same interaction");
    assert!(
        refused.contains("first sent with other arguments"),
        "{refused}"
    );
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
}

/// **A record answers a day** (ADR-0325): on a host started after the
/// first, its clock a day and a minute past the record, the same interaction
/// is a new request. It runs, and its commit forgets the old record in its
/// own transaction, so the new one takes its key.
#[test]
fn on_postgres_an_interactions_record_answers_a_day() {
    let Some(url) = database() else { return };
    let (_layer, host, _dir) = hosts(&url);
    let request = [shown("espresso"), serde_json::json!(1)];
    {
        let s = host();
        let first = s
            .command_answer(ADD, "a", &request, Some("press-1"))
            .expect("a well-formed request");
        assert!(first.committed, "{first:?}");
    }
    let s = host();
    posted(&s, "/bench/clock?advance=86460000", "clock", "c-1", "");
    let again = s
        .command_answer(ADD, "a", &request, Some("press-1"))
        .expect("a well-formed request");
    assert!(again.committed, "{again:?}");
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 2)]);
}

/// **Two hosts, one interaction** (ADR-0325): while the second runs it, the
/// first commits the same interaction, and the second's commit meets the
/// first's record by its key. It rolls back, writes nothing, and answers as
/// the first did. The first is played by its record alone, committed from
/// beside: the first's cart writes would wait on the second's.
#[test]
fn on_postgres_a_second_hosts_commit_answers_what_the_first_committed() {
    let Some(url) = database() else { return };
    let (layer, host, _dir) = hosts(&url);
    let s = host();
    let request = [shown("espresso"), serde_json::json!(1)];
    let key = format!("a\u{1f}{ADD}\u{1f}press-1");
    let (_, _, sent) = s.receipt_keys("a", ADD, "press-1", &request);
    let first = r#"{"committed":true,"why":null,"refused":null,"result":{"first":true}}"#;
    {
        let (layer, key, sent) = (layer.clone(), key.clone(), sent.clone());
        *s.before_commit.lock().expect("hook") = Some(Box::new(move |_: &Server| {
            layer
                .sql()
                .execute(
                    "INSERT INTO interactions (key, sent, answer, at_ms) VALUES ($1, $2, $3, $4)",
                    &[&key, &sent, &first, &i64::MAX],
                )
                .expect("the first host's record");
        }));
    }
    let answered = s
        .command_answer(ADD, "a", &request, Some("press-1"))
        .expect("a well-formed request");
    assert_eq!(
        answered,
        Answered::from_kept(first),
        "the first host's answer"
    );
    assert_eq!(
        s.cart_lines("a"),
        Vec::<(String, i64)>::new(),
        "nothing written"
    );
    let held = layer
        .sql()
        .query_one("SELECT count(*) FROM interactions WHERE key = $1", &[&key])
        .expect("counted")
        .get::<_, i64>(0);
    assert_eq!(held, 1, "one record, the first's");
}

/// **An order keeps its lines on PostgreSQL** (ADR-0193), and every event
/// a commit staged is consumed once delivered: after orders are placed and
/// moved along, the materializer's outbox holds none, and the database's
/// only what this host committed and read back, kept for the other hosts
/// until it is trimmed (ADR-0369; named `..._the_outboxes_hold_nothing`
/// until the database's outbox became every host's log).
#[test]
fn on_postgres_an_order_keeps_its_lines_and_the_outboxes_hold_only_its_hosts_rows() {
    let Some(url) = database() else { return };
    let s = served(&url);
    for (i, session) in ["a", "b"].iter().enumerate() {
        assert!(add(&s, session, "espresso", 2, &format!("i-{i}-1")).committed);
        assert!(add(&s, session, "cortado", 1, &format!("i-{i}-2")).committed);
        let placed = s
            .command_answered(
                PLACE,
                session,
                &[s.shown_total(session)],
                Some(&format!("i-{i}-3")),
            )
            .expect("runs");
        assert!(placed.committed, "{:?}", placed.result);
        let answer = posted(&s, "/bench/order?status=delivered", session, "", "");
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    }
    let rows = s
        .layer
        .sql()
        .query(
            "SELECT o.owner, l.item, l.quantity FROM order_lines l \
             JOIN orders o ON o.id = l.order_id ORDER BY o.owner, l.position",
            &[],
        )
        .expect("read");
    let lines: Vec<(String, String, i64)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    let want = |session: &str| {
        [
            (session.to_string(), "espresso".to_string(), 2),
            (session.to_string(), "cortado".to_string(), 1),
        ]
    };
    // Each the reader's, its session's guest's (track `store-accounts`).
    assert_eq!(lines, [want("u-a"), want("u-b")].concat());
    assert_eq!(
        s.count("SELECT count(*) FROM cart_lines"),
        0,
        "carts emptied"
    );
    // The database's outbox holds only what this host committed and read
    // back, kept for the other hosts until it is trimmed (ADR-0369); the
    // materializer's holds nothing.
    let host = s.data.tail().expect("a shared database").host().to_string();
    assert_eq!(
        s.layer
            .sql()
            .query_one("SELECT count(*) FROM outbox WHERE host <> $1", &[&host])
            .expect("counted")
            .get::<_, i64>(0),
        0,
        "nothing but this host's"
    );
    assert_eq!(s.materializer.retained_events(), 0);
}

/// **The two layers answer alike** (track `store-pg`): every read the
/// store's queries make, of the first data and after the same writes, from
/// the in-memory layer and from PostgreSQL. `migrations/store/0002_seed.sql`
/// cannot drift from `store.rs`'s seed unseen.
#[test]
fn on_postgres_the_two_layers_answer_alike() {
    let Some(url) = database() else { return };
    let on_pg = served(&url);
    // In memory whatever layer the rest of the suite is on; its pages
    // numbered as the PostgreSQL host's are, from that host's instance
    // (track `second-host`), so that the two hosts' pages compare to the
    // byte: a page's number is its host's, not what the layer answers.
    let in_memory = {
        let (dir, out) = built_from_patches_in("examples", |app| app.to_string(), &[]);
        let layer: Arc<dyn crate::store::StoreLayer> = Arc::new(crate::store::StoreData::new());
        let mut server = Server::from_build_layers(out.clone(), out, None, Some(layer))
            .expect("served in memory");
        server.numbered_from(on_pg.first_page);
        Served { server, _dir: dir }
    };
    let servers: [&Server; 2] = [&in_memory, &on_pg];
    for s in servers {
        // The slow sources' delays are test controls, not data.
        s.store
            .faults()
            .recommender
            .lock()
            .expect("recommender")
            .delay_ms = 0;
        s.store
            .faults()
            .estimators
            .lock()
            .expect("estimators")
            .insert(
                "a".to_string(),
                Estimator {
                    delay_ms: 0,
                    fail: None,
                    ..Estimator::default()
                },
            );
    }
    let s = |v: &str| Val::String(v.to_string());
    let reads: Vec<(&str, Vec<Val>)> = vec![
        ("store:data/stores#list", vec![]),
        ("store:data/stores#get", vec![s("47")]),
        ("store:data/stores#get", vec![s("48")]),
        ("store:data/stores#get", vec![s("49")]),
        ("store:data/menus#for-store", vec![s("47")]),
        ("store:data/menus#for-store", vec![s("48")]),
        ("store:data/menus#for-store", vec![s("49")]),
        ("store:data/menus#sections", vec![s("47")]),
        ("store:data/menus#sections", vec![s("48")]),
        ("store:data/menus#in-category", vec![s("47"), s("hot")]),
        ("store:data/menus#in-category", vec![s("47"), s("cold")]),
        ("store:data/menus#in-category", vec![s("47"), s("all")]),
        ("store:data/notices#current", vec![s("47")]),
        ("store:data/notices#current", vec![s("48")]),
        ("store:data/kitchen#prep-minutes", vec![s("47")]),
        ("store:data/kitchen#prep-minutes", vec![s("48")]),
        ("store:data/recommendations#for-store", vec![s("47")]),
        ("store:data/recommendations#for-store", vec![s("48")]),
        ("store:data/carts#current", vec![s("a")]),
        ("store:data/orders#current", vec![s("a")]),
        ("store:data/orders#placed", vec![s("a")]),
        ("store:data/estimates#current", vec![s("a")]),
    ];
    // Each read's answer, its estimate's `generated-at` (a clock reading)
    // left out.
    let answers = |server: &Server| -> Vec<String> {
        let ops = server.data.reads("a", None);
        reads
            .iter()
            .map(|(op, args)| {
                let answer = ops.get(*op).unwrap_or_else(|| panic!("no {op}"))(args)
                    .unwrap_or_else(|e| panic!("{op}: {e}"));
                let text = format!("{answer:?}");
                match text.find("\"generated-at\"") {
                    Some(at) => text[..at].to_string(),
                    None => text,
                }
            })
            .collect()
    };
    let first: Vec<Vec<String>> = servers.iter().map(|s| answers(s)).collect();
    assert_eq!(first[0], first[1], "the first data");
    // The same writes, through each layer: commands, and the routes'.
    for server in servers {
        assert!(add(server, "a", "espresso", 2, "i-1").committed);
        assert!(add(server, "a", "cold-brew", 1, "i-2").committed);
        server
            .broadcast_menu(MenuOp::Insert {
                id: "flat-white".into(),
                name: "Flat White".into(),
                at: Some("cortado".into()),
                before: false,
            })
            .expect("inserted");
        server
            .broadcast_menu(MenuOp::Rename {
                id: "espresso".into(),
                name: "Espresso Doppio".into(),
            })
            .expect("renamed");
        server
            .broadcast_menu(MenuOp::Stock {
                id: "cortado".into(),
                available: false,
            })
            .expect("sold out");
        server.stock_untold("scone", false);
        server.set_estimate("a", 15, Some(45)).expect("estimated");
        server
            .curate(Some(&[("drip".to_string(), "Drip Coffee".to_string())]))
            .expect("curated");
        server.untold(
            "",
            "store:host/notices#post",
            &[s("47"), s("Closing early")],
        );
        server.untold("", "store:host/kitchen#set-prep", &[s("47"), Val::S64(20)]);
        let placed = server
            .command_answered(PLACE, "a", &[server.shown_total("a")], Some("i-3"))
            .expect("runs");
        assert!(placed.committed, "{:?}", placed.result);
        server.order_set_untold("a", "preparing");
        assert!(add(server, "a", "drip", 3, "i-4").committed);
    }
    let after: Vec<Vec<String>> = servers.iter().map(|s| answers(s)).collect();
    assert_eq!(after[0], after[1], "after the same writes");
    assert_ne!(
        first[0], after[0],
        "the control: the writes changed what is read"
    );
    // And the same pages, to the byte: their headers and documents.
    let pages = |server: &Server| -> Vec<String> {
        ["/", "/stores/47", "/stores/48", "/cart", "/order"]
            .iter()
            .map(|path| {
                fetched_as(server, path, Some("b"))
                    .into_iter()
                    .map(|(_, c)| c)
                    .collect::<String>()
            })
            .collect()
    };
    for server in servers {
        assert!(add(server, "b", "matcha", 1, "j-1").committed);
    }
    let (memory_pages, pg_pages) = (pages(&in_memory), pages(&on_pg));
    for (m, p) in memory_pages.iter().zip(&pg_pages) {
        assert!(m.starts_with("HTTP/1.1 200"), "{m}");
        assert_eq!(m, p, "to the byte");
    }
}

/// **The store's schema is its own** (track `store-pg`): in one database
/// with the feed's, each keeps its own migration table, outbox and lock,
/// and a write of one reaches nothing of the other's.
#[test]
fn on_postgres_the_stores_schema_is_its_own_beside_the_feeds() {
    let Some(url) = database() else { return };
    /// The feed's schema, created here and dropped here.
    struct FeedSchema(String, String);
    impl Drop for FeedSchema {
        fn drop(&mut self) {
            if let Ok(mut c) = postgres::Client::connect(&self.0, postgres::NoTls) {
                let _ = c.batch_execute(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", self.1));
            }
        }
    }
    let feed_schema = FeedSchema(
        url.clone(),
        format!("pw_store_test_{}_beside_feed", std::process::id()),
    );
    let feed = crate::feed_pg::FeedPg::open_with(
        &url,
        Some(&feed_schema.1),
        Isolation::Serializable,
        crate::feed_pg::RenderSeats::default(),
    )
    .expect("the feed opened");
    let s = served(&url);
    let tables = |schema: &str| -> Vec<String> {
        s.layer
            .sql()
            .query(
                "SELECT table_name::text FROM information_schema.tables \
                 WHERE table_schema = $1 ORDER BY 1",
                &[&schema],
            )
            .expect("read")
            .iter()
            .map(|r| r.get(0))
            .collect()
    };
    let (store_tables, feed_tables) = (tables(s.layer.schema()), tables(&feed_schema.1));
    assert!(
        store_tables.contains(&"pw_store_migrations".to_string()),
        "{store_tables:?}"
    );
    assert!(
        !store_tables.contains(&"pw_migrations".to_string()),
        "{store_tables:?}"
    );
    assert!(
        feed_tables.contains(&"pw_migrations".to_string()),
        "{feed_tables:?}"
    );
    assert!(
        !feed_tables.contains(&"pw_store_migrations".to_string()),
        "{feed_tables:?}"
    );
    assert!(
        store_tables.contains(&"outbox".to_string()) && feed_tables.contains(&"outbox".to_string())
    );
    // A store's write is the store's: the feed's tables are as they were.
    let feed_posts = |c: &mut postgres::Client| -> i64 {
        c.query_one(
            &format!("SELECT count(*) FROM \"{}\".posts", feed_schema.1),
            &[],
        )
        .expect("read")
        .get(0)
    };
    let before = feed_posts(&mut s.layer.sql());
    assert!(add(&s, "a", "espresso", 1, "i-1").committed);
    assert_eq!(feed_posts(&mut s.layer.sql()), before);
    assert!(s.count("SELECT count(*) FROM cart_lines") == 1);
    // Its connections cannot name the feed's tables unqualified.
    let unqualified = s.layer.sql().query_one("SELECT count(*) FROM posts", &[]);
    assert!(
        unqualified.is_err(),
        "the feed's `posts` resolved from the store's schema"
    );
    drop(feed);
}

/// **A source stating a feed of its changes is refused** (ADR-0246's
/// negative control): the layer delivers none, so the host does not serve,
/// and says which clause.
#[test]
fn on_postgres_a_stated_change_feed_the_layer_cannot_deliver_is_refused() {
    let Some(url) = database() else { return };
    const FEED: &str = "--- a/lib/StoreData.pw\n+++ b/lib/StoreData.pw\n@@ -37,4 +37,4 @@\n     transactions serializable\n     reads        strong\n-    changes      none\n+    changes      feed\n     sets         one_call\n";
    let Err(why) = served_with(|app| app.to_string(), &[FEED], || StorePg::for_test(&url)) else {
        panic!("a change feed the layer cannot deliver was served");
    };
    assert!(why.contains("`StoreData` states `changes feed`"), "{why}");
}

/// **Serializable, stated against a connection that opens read committed,
/// is refused**, unless the layer sets it on each transaction, or the
/// connection's default gives it (ADR-0246). What the host compares is
/// measured, not assumed.
#[test]
fn on_postgres_serializable_against_a_read_committed_default_is_refused() {
    let Some(url) = database() else { return };
    let as_is = |app: &str| app.to_string();
    let Err(why) = served_with(as_is, &[], || {
        StorePg::for_test_with(&url, Isolation::ConnectionDefault)
    }) else {
        panic!("serializable was stated, and read committed served");
    };
    assert!(
        why.contains(
            "`StoreData` states `transactions serializable`, and the database's are read_committed"
        ),
        "{why}"
    );
    // The controls: the same connection's default, made serializable, gives
    // it; and the layer setting it on each transaction gives it.
    let separator = if url.contains('?') { '&' } else { '?' };
    let serializable =
        format!("{url}{separator}options=-c%20default_transaction_isolation%3Dserializable");
    served_with(as_is, &[], || {
        StorePg::for_test_with(&serializable, Isolation::ConnectionDefault)
    })
    .expect("a serializable default gives serializable");
    served_with(as_is, &[], || StorePg::for_test(&url)).expect("serializable on each transaction");
}

// --- the scheduled-commands track ---------------------------------------

/// The store with the track's test commands (`store_scheduled::SCHEDULING`),
/// served by hosts over one database.
fn scheduling_hosts(url: &str) -> (Arc<StorePg>, impl Fn() -> Server, tempfile::TempDir) {
    hosts_with(url, super::store_scheduled::with_scheduling)
}

/// How many runs the database keeps.
fn runs_kept(layer: &StorePg) -> i64 {
    layer
        .sql()
        .query_one("SELECT count(*) FROM schedules", &[])
        .expect("counted")
        .get(0)
}

/// **A run is written by its command's own transaction** (the
/// scheduled-commands track): the run's row and the command's interaction
/// record, inserted by one transaction. The control: a command that answers
/// an error writes neither.
#[test]
fn on_postgres_a_run_is_written_by_its_commands_transaction() {
    let Some(url) = database() else { return };
    let (layer, host, _dir) = scheduling_hosts(&url);
    let s = host();
    let press = |refuse: bool, interaction: &str| {
        s.command_answer(
            "store.page.add_in_five",
            "a",
            &[serde_json::json!("espresso"), serde_json::json!(refuse)],
            Some(interaction),
        )
        .expect("a well-formed request")
    };
    assert!(!press(true, "i-1").committed);
    assert_eq!(runs_kept(&layer), 0, "the control");
    assert!(press(false, "i-2").committed);
    let same: bool = layer
        .sql()
        .query_one(
            "SELECT s.xmin::text = i.xmin::text FROM schedules s, interactions i \
             WHERE i.key LIKE '%add_in_five%'",
            &[],
        )
        .expect("one run, one record")
        .get(0);
    assert!(same, "written by one transaction");
}

/// **A run outlives the host that scheduled it**: a host started after it
/// over the same database runs it once its time has passed, on behalf of
/// the scheduler, and removes it.
#[test]
fn on_postgres_a_run_outlives_the_host_that_scheduled_it() {
    let Some(url) = database() else { return };
    let (layer, host, _dir) = scheduling_hosts(&url);
    {
        let s = host();
        assert!(super::store_scheduled::in_five(&s, "espresso", false, "i-1").committed);
    }
    let s = host();
    assert!(s.cart_lines("a").is_empty());
    posted(&s, "/bench/clock?advance=300000", "clock", "c-1", "");
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(runs_kept(&layer), 0);
}

/// Legacy ambiguous work is quarantined after upgrade (ADR0388). An old
/// action may already have committed; receipt expiry must not make it run again.
#[test]
fn on_postgres_a_run_committed_and_not_removed_commits_once() {
    let Some(url) = database() else { return };
    let (layer, host, _dir) = scheduling_hosts(&url);
    let s = host();
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "i-1").committed);
    let row = layer
        .sql()
        .query_one(
            "SELECT id, key, command, args, user_id, due_ms FROM schedules",
            &[],
        )
        .expect("one run");
    let (id, key, command, args, user, due): (i64, String, String, String, String, i64) = (
        row.get(0),
        row.get(1),
        row.get(2),
        row.get(3),
        row.get(4),
        row.get(5),
    );
    posted(&s, "/bench/clock?advance=300000", "clock", "c-1", "");
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    // An ambiguous pre-protocol row whose action may already have committed.
    layer
        .sql()
        .execute(
            "INSERT INTO schedules (id, key, command, args, user_id, due_ms, protocol) \
             VALUES ($1, $2, $3, $4, $5, $6, 0)",
            &[&id, &key, &command, &args, &user, &due],
        )
        .expect("kept");
    let next = host();
    posted(&next, "/bench/clock?advance=300000", "clock", "c-1", "");
    assert_eq!(next.run_due().0, 1, "run again");
    assert_eq!(
        next.cart_lines("a"),
        [("espresso".to_string(), 1)],
        "and committed once"
    );
    assert_eq!(
        runs_kept(&layer),
        1,
        "legacy ambiguity is retained for reconciliation"
    );
    assert_eq!(layer.sql().query_one("SELECT count(*) FROM schedules WHERE protocol=0 AND failed LIKE '%reconciliation%'", &[]).expect("quarantine").get::<_,i64>(0),1);
    next.query_clock.0.advance(25 * 60 * 60 * 1000);
    assert_eq!(next.run_due().0, 0);
    assert_eq!(host().cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(
        layer
            .sql()
            .query_one("SELECT count(*) FROM outbox WHERE kind='event'", &[])
            .expect("event audit")
            .get::<_, i64>(0),
        1
    );
}

/// Two real hosts select one generation before either dispatches it. Their
/// action, receipt, event and generation consume arbitrate in actual transactions.
#[test]
fn on_postgres_two_hosts_commit_a_run_once() {
    let Some(url) = database() else { return };
    let (layer, host, _dir) = scheduling_hosts(&url);
    let a = host();
    let b = host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "two-real-hosts").committed);
    a.query_clock.0.advance(300000);
    b.query_clock.0.advance(300000);
    let ac = schedules::select(a.runs(), a.now(), 16).expect("A selects");
    let bc = schedules::select(b.runs(), b.now(), 16).expect("B selects");
    assert_eq!(ac.len(), 1);
    assert_eq!(bc.len(), 1);
    assert_eq!(ac[0].id, bc[0].id);
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            a.run_scheduled(&ac[0]);
        });
        let second = scope.spawn(|| {
            barrier.wait();
            b.run_scheduled(&bc[0]);
        });
        first.join().expect("A completes");
        second.join().expect("B completes");
    });
    assert_eq!(host().cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(runs_kept(&layer), 0);
    assert_eq!(
        layer
            .sql()
            .query_one(
                "SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'",
                &[]
            )
            .expect("receipt audit")
            .get::<_, i64>(0),
        1
    );
    assert_eq!(
        layer
            .sql()
            .query_one("SELECT count(*) FROM outbox WHERE kind='event'", &[])
            .expect("event audit")
            .get::<_, i64>(0),
        1
    );
}
