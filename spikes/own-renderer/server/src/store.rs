//! **The store's data** (ADR-0218): what the food-delivery app's data layer
//! holds, which a deployment supplies to the host. Until ADR-0218 it was the
//! host's own state, and no other program could be served.
//!
//! Track `store-pg`: **one set of operations over two stores of rows.** The
//! operations a query reads through and a command writes through are built
//! once, here, over [`Rows`]: the in-memory layer's ([`StoreData`]) and
//! PostgreSQL's (`store_pg.rs`). Each layer supplies its rows and its
//! transaction; what an operation answers, and how a fault meets it, is
//! this file's, so the two layers cannot answer differently.
//!
//! **All of the store's state is the layer's**: the stores and their menus,
//! the stock, the carts, the orders, the notice, the kitchen's preparation
//! time, the curated recommendations and each session's estimate. A route
//! that changes any of it stages one of the host's own operations
//! (`store:host/…`) through [`DataLayer::begin`] and commits it with its
//! declared events, as a command does. No program is linked to those
//! operations: the host strips them from a component's operations, and a
//! program that names one is refused before it is served.
//!
//! **The fault hooks stay test controls** ([`Faults`]): delays, one-shot
//! failures, the recommender's gate and the slow category. Both layers apply
//! them, as a database that is slow or down would meet either.
//!
//! [`DataLayer::begin`]: crate::data::DataLayer::begin

use super::*;
use crate::data::Ops;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// **The prefix of the host's own operations** (track `store-pg`): what a
/// route that changes the store's data stages, and no program may import.
pub(crate) const HOST_OPS: &str = "store:host/";

/// **What the store's data layer is, to the host** (track `store-pg`): a
/// data layer, and the test controls its operations meet.
pub(crate) trait StoreLayer: crate::data::DataLayer {
    /// The fault hooks a test sets, which every operation of the layer meets.
    fn faults(&self) -> &Faults;

    /// **A line's quantity written around the program** (ADR-0179), as a
    /// database's row can be: a test's, of what the store must refuse to
    /// show. A database that holds the invariant itself refuses it.
    #[cfg(test)]
    fn line_around(&self, owner: &str, at: usize, quantity: i64) -> Result<(), String>;
}

/// **The layer the server's tests serve the store on** (track `store-pg`):
/// in memory, or on PostgreSQL where `PW_STORE_TEST_LAYER=postgres` and
/// `PW_STORE_DATABASE_URL` say, in a schema of its own that is dropped with
/// it. So the whole of the server's suite runs on either layer.
#[cfg(test)]
pub(crate) fn layer_for_tests() -> Arc<dyn StoreLayer> {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() == Ok("postgres") {
        let url = std::env::var("PW_STORE_DATABASE_URL")
            .expect("PW_STORE_TEST_LAYER=postgres needs PW_STORE_DATABASE_URL");
        return Arc::new(
            crate::store_pg::StorePg::for_test(&url).expect("the store on PostgreSQL"),
        );
    }
    Arc::new(StoreData::new())
}

/// **The test controls every operation of the store meets** (charter §15.5,
/// ADR-0174, ADR-0177, ADR-0148, ADR-0223, E14's T07 and T10). Not the
/// store's data: what a database or a source that is slow, held or down does
/// to it.
pub(crate) struct Faults {
    /// **A test's: the next command's data layer answers `cart-expired`**,
    /// the declared refusal, once (ADR-0174).
    pub(crate) fail_next: AtomicBool,
    /// **How long the store's data layer takes to answer for a store**
    /// (charter §15.5's store delay, ADR-0174), in milliseconds: one delay
    /// for every reader, as the store is one value for all of them. A test
    /// sets it through `/bench/store`.
    pub(crate) store_delay_ms: Arc<AtomicU64>,
    /// **Whether the store's next read fails at its origin** (ADR-0177),
    /// once, for every reader. A test sets it through `/bench/store?fail=next`.
    pub(crate) store_fails_next: Arc<AtomicBool>,
    /// **What each session's cart meets at the database** (charter §15.5,
    /// ADR-0174): how long a read of it takes, and whether its next write
    /// and its next read fail. A test sets them through `/bench/cart` and
    /// `/bench/fail`.
    pub(crate) cart_faults: Arc<Mutex<BTreeMap<String, CartFaults>>>,
    /// **How the recommender answers** (ADR-0148, ADR-0223): its delay, how
    /// it fails, and the gate a held read waits at. What it recommends is
    /// the layer's data.
    pub(crate) recommender: Mutex<Recommender>,
    /// **How each session's estimator answers** (E14, T10): its delay and
    /// how it fails; the default for a session no test has. What it
    /// estimates is the layer's data.
    pub(crate) estimators: Mutex<BTreeMap<String, Estimator>>,
    /// **The menu's slow category** (E14, T07): which, and how slow.
    pub(crate) categories: Mutex<Categories>,
    /// How many reads of a category saw they were stopped, and ended early
    /// (E14, T07).
    pub(crate) category_stopped: Arc<AtomicU64>,
    /// **The kitchen, held at a gate** (E14, T02; ADR-0223): a read of its
    /// prep time waits there until a test lets it go. A test that needs to
    /// know how many pages are asking the kitchen at once holds it, and hears
    /// them arrive, rather than timing eight pages against a delay.
    pub(crate) kitchen_gate: Mutex<Option<Arc<Gate>>>,
    /// **How many delays were taken**, a cart's or the store's (ADR-0174):
    /// a test hears from this that a delay was waited by what it names and
    /// by nothing else, where the time a read took was the machine's too.
    pub(crate) delays_taken: Arc<AtomicU64>,
}

impl Faults {
    pub(crate) fn new() -> Faults {
        Faults {
            fail_next: AtomicBool::new(false),
            store_delay_ms: Arc::new(AtomicU64::new(0)),
            store_fails_next: Arc::new(AtomicBool::new(false)),
            cart_faults: Arc::new(Mutex::new(BTreeMap::new())),
            recommender: Mutex::new(Recommender::default()),
            estimators: Mutex::new(BTreeMap::new()),
            categories: Mutex::new(Categories::default()),
            category_stopped: Arc::new(AtomicU64::new(0)),
            kitchen_gate: Mutex::new(None),
            delays_taken: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// **A store, as the data layer holds it** (ADR-0125, ADR-0192).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoreRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// **What it serves, in its own words** ("Café, Bakery"; ADR-0308,
    /// amended): what the home page, its page and a search show and match.
    pub(crate) cuisine: String,
    pub(crate) opens_minute: i64,
    pub(crate) closes_minute: i64,
    /// **Where it is and how far it delivers** (track `store-accounts`,
    /// milestone 2): what `places::delivers_to` reads.
    pub(crate) zone: crate::places::Zone,
}

/// **A reader's saved address, as the data layer holds it** (track
/// `store-accounts`, milestone 2): its id, the label the reader gave it, the
/// place it names (`places::PLACES`), and whether it is the one chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddressRow {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) place: String,
    pub(crate) chosen: bool,
}

/// **The most addresses one reader keeps** (track `store-accounts`).
pub(crate) const MOST_ADDRESSES: usize = 10;

/// **Where each store is, and how far it delivers** (track `store-accounts`):
/// store 47 at Union Square, four kilometres around it; store 48 at the Ferry
/// Building, two. `migrations/store/0004_addresses.sql` is the same rows.
pub(crate) fn zone_of(store: &str) -> crate::places::Zone {
    let at = |place: &str, radius_m: i64| {
        let p = crate::places::place(place).expect("a store's place is in the table");
        crate::places::Zone {
            lat_e6: p.lat_e6,
            lon_e6: p.lon_e6,
            radius_m,
        }
    };
    match store {
        "48" => at("ferry-building", 2_000),
        _ => at("union-square", 4_000),
    }
}

/// **One item of a store's menu, as the data layer holds it** (ADR-0166,
/// ADR-0169, ADR-0178, ADR-0181, E14's T07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// In cents (ADR-0169).
    pub(crate) price: i64,
    /// The section it is listed under (ADR-0181), its id and name.
    pub(crate) category: String,
    pub(crate) category_name: String,
    /// How it is served, `hot` or `cold`: what T07's categories read.
    pub(crate) served: String,
    /// Whether it can be ordered now (ADR-0178).
    pub(crate) available: bool,
}

impl Item {
    /// **An item a store's menu gains** (E7-P), or one it was first seeded
    /// with: described, priced and listed as the store's catalogue says of
    /// its id, and available.
    pub(crate) fn new(store: &str, id: &str, name: &str) -> Item {
        let (category, category_name) = item_category(store, id);
        Item {
            id: id.to_string(),
            name: name.to_string(),
            description: item_description(id).to_string(),
            price: item_price(id),
            category: category.to_string(),
            category_name: category_name.to_string(),
            served: category_of(id).to_string(),
            available: true,
        }
    }
}

/// **A session's order** (ADR-0193): the cart's lines it was placed with,
/// and its status's case. TRACK SEAM (reviews): and its id, PostgreSQL's
/// `orders.id`, or in memory the count of orders made when it was, so that a
/// rating is of this order and not of its owner's next (ADR-0341).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Order {
    pub(crate) id: i64,
    pub(crate) status: String,
    pub(crate) lines: Lines,
}

/// **A session's estimate** (E14, T10; ADR-0180): the least minutes and,
/// where a test said, the most.
pub(crate) type Estimate = (i64, Option<i64>);

/// **The store's rows, as an operation reads and writes them** (track
/// `store-pg`): the one thing each layer supplies. A read is of what the
/// layer's transaction sees; a write is staged in it, and kept only if it
/// commits.
pub(crate) trait Rows {
    /// Every store, in the order the home page lists them (ADR-0192).
    fn stores(&mut self) -> Result<Vec<StoreRow>, String>;
    /// A store's menu, in its order; empty where the store has none.
    fn menu(&mut self, store: &str) -> Result<Vec<Item>, String>;
    /// A store's menu, replaced whole.
    fn set_menu(&mut self, store: &str, items: &[Item]) -> Result<(), String>;
    /// Whether every store's item `item` can be ordered.
    fn stock(&mut self, item: &str, available: bool) -> Result<(), String>;
    /// **What a new line records of `item`** (ADR-0172): its name and price
    /// in the last store that lists it, and whether any store sells it now.
    fn catalog_item(&mut self, item: &str) -> Result<Option<(String, i64, bool)>, String>;
    /// **An owner's cart** (track `store-accounts`): the owner is opaque, a
    /// user's id where the program reads a cart by its reader's handle
    /// (`store:data/user-carts`), or a session's where it reads one by the
    /// session (`store:data/carts`, the benchmark's copy of the store).
    fn cart(&mut self, owner: &str) -> Result<Lines, String>;
    fn set_cart(&mut self, owner: &str, lines: &[Line]) -> Result<(), String>;
    /// **An owner's saved addresses** (track `store-accounts`), in the order
    /// they were saved.
    fn addresses(&mut self, owner: &str) -> Result<Vec<AddressRow>, String>;
    fn set_addresses(&mut self, owner: &str, rows: &[AddressRow]) -> Result<(), String>;
    /// An owner's order, its latest.
    fn order(&mut self, owner: &str) -> Result<Option<Order>, String>;
    /// An owner's order, placed with `lines`.
    fn place(&mut self, owner: &str, lines: &[Line]) -> Result<(), String>;
    /// An owner's order moved along, the lines it was placed with kept;
    /// one set with none placed has none. `None` removes it.
    fn set_status(&mut self, owner: &str, status: Option<&str>) -> Result<(), String>;
    /// NEXT 6c: the payment an owner's order awaits (`payments.rs`), none
    /// where it awaits none; `None` sets none.
    fn awaiting(&mut self, owner: &str) -> Result<Option<crate::payments::Awaiting>, String>;
    fn set_awaiting(
        &mut self,
        owner: &str,
        awaiting: Option<&crate::payments::Awaiting>,
    ) -> Result<(), String>;
    fn notice(&mut self, store: &str) -> Result<Option<String>, String>;
    fn set_notice(&mut self, store: &str, text: &str) -> Result<(), String>;
    fn prep(&mut self, store: &str) -> Result<Option<i64>, String>;
    fn set_prep(&mut self, store: &str, minutes: i64) -> Result<(), String>;
    /// What the recommender suggests for every store, where it was curated.
    fn curated(&mut self) -> Result<Option<Vec<(String, String)>>, String>;
    fn curate(&mut self, items: Option<&[(String, String)]>) -> Result<(), String>;
    fn estimate(&mut self, session: &str) -> Result<Option<Estimate>, String>;
    fn set_estimate(&mut self, session: &str, estimate: Estimate) -> Result<(), String>;
    // TRACK SEAM (store-search): what a search's terms match, each layer's (`search.rs`).
    fn search(&mut self, terms: &[String]) -> Result<Vec<crate::search::Matched>, String> {
        crate::search::scan(self, terms)
    }
    // TRACK SEAM (store-hours): a store's opening hours (`hours.rs`), none
    // where the layer holds no such store.
    /// The hours as read are one value with its order (`hours::held`):
    /// weekly periods by weekday and opening, special dates by date, the
    /// last of each listed twice kept. Every layer hands them so, and the
    /// program relies on it, sorting nothing itself.
    fn hours(&mut self, store: &str) -> Result<Option<crate::hours::Hours>, String>;
    fn set_hours(&mut self, store: &str, hours: &crate::hours::Hours) -> Result<(), String>;

    // TRACK SEAM (bounded-statements): rows read by set (NEXT 4b), each one
    // statement on PostgreSQL whatever the number of keys (`= ANY($1)`), and
    // a scan in memory, which these defaults are.
    /// The stores among `ids`, in the stores' order.
    fn stores_by_ids(&mut self, ids: &[String]) -> Result<Vec<StoreRow>, String> {
        Ok(self
            .stores()?
            .into_iter()
            .filter(|s| ids.contains(&s.id))
            .collect())
    }
    /// The store that lists each of `items`: the first, in the stores'
    /// order, whose menu has it.
    fn stores_of_items(&mut self, items: &[String]) -> Result<BTreeMap<String, String>, String> {
        scanned_stores_of_items(self, items)
    }
    /// The hours of each of `stores` the layer holds, in `stores`' order.
    fn hours_of(
        &mut self,
        stores: &[String],
    ) -> Result<Vec<(String, crate::hours::Hours)>, String> {
        hours_one_by_one(self, stores)
    }

    // TRACK SEAM (order-total): the checkout's rows (`checkout.rs`).
    /// The fees of a list of stores, in its order, a store without a
    /// schedule left out: one statement on PostgreSQL.
    fn fees(&mut self, stores: &[String]) -> Result<Vec<(String, crate::checkout::Fees)>, String>;
    fn set_fees(&mut self, store: &str, fees: &crate::checkout::Fees) -> Result<(), String>;
    /// A code, and whether `owner` used it.
    fn promo(
        &mut self,
        owner: &str,
        code: &str,
    ) -> Result<Option<(crate::checkout::Promo, bool)>, String>;
    fn set_promo(&mut self, promo: &crate::checkout::Promo) -> Result<(), String>;
    /// `owner`'s use of `code` recorded, or `false` where it was already.
    fn use_promo(&mut self, owner: &str, code: &str) -> Result<bool, String>;
    /// `owner`'s use of `code` released (NEXT 6c, step eight): an order taken
    /// back before it was paid used none.
    fn release_promo(&mut self, owner: &str, code: &str) -> Result<(), String>;
    fn chosen(&mut self, owner: &str) -> Result<crate::checkout::Chosen, String>;
    fn set_chosen(&mut self, owner: &str, chosen: &crate::checkout::Chosen) -> Result<(), String>;
    /// `owner`'s latest order, its charges recorded.
    fn charge(&mut self, owner: &str, charged: &crate::checkout::Charged) -> Result<(), String>;
    fn charged(&mut self, owner: &str) -> Result<Option<crate::checkout::Charged>, String>;

    // TRACK SEAM (reviews): a store's ratings (`reviews.rs`).
    /// Each of `stores`' ratings counted and their stars summed, in
    /// `stores`' order, a store with none left out: one statement on
    /// PostgreSQL whatever their number.
    fn ratings_of(&mut self, stores: &[String]) -> Result<Vec<(String, i64, i64)>, String>;
    /// A store's newest `most` ratings with a comment, newest first: each
    /// one's id, stars and comment.
    fn reviews(&mut self, store: &str, most: usize) -> Result<Vec<(i64, i64, String)>, String>;
    /// `owner`'s latest order as rating reads it, none before one.
    fn to_rate(&mut self, owner: &str) -> Result<Option<crate::reviews::ToRate>, String>;
    /// A rating written, or `false` where its order has one of its store.
    fn rate(&mut self, rating: &crate::reviews::Rating) -> Result<bool, String>;
    // TRACK SEAM (scheduled-orders): an owner's order's time
    // (`scheduled_orders.rs`), one for their latest order that was placed for
    // a time, written as it awaits its payment; read only with the order's
    // reference, so one kept past its order is another order's to no one.
    fn order_time(
        &mut self,
        owner: &str,
    ) -> Result<Option<crate::scheduled_orders::OrderTime>, String>;
    fn set_order_time(
        &mut self,
        owner: &str,
        time: &crate::scheduled_orders::OrderTime,
    ) -> Result<(), String>;
}

/// **Where an operation's rows are**, each layer's: it runs `f` on them, in
/// a query's own read or in the command's transaction.
pub(crate) type With = Arc<
    dyn Fn(&mut dyn FnMut(&mut dyn Rows) -> Result<Vec<Val>, String>) -> Result<Vec<Val>, String>
        + Send
        + Sync,
>;

/// **What a command's writes left to commit** (track `store-pg`): whether
/// it wrote, and the session's cart's total where it wrote the cart, which
/// the host's materializer records as the in-memory path always has.
#[derive(Default)]
pub(crate) struct Written {
    pub(crate) wrote: bool,
    pub(crate) cart_total: Option<i64>,
}

impl Written {
    /// **What the commit writes in the materializer's transaction**, by key,
    /// or none where nothing was written: the session's cart's total.
    pub(crate) fn rows(&self, session: &str) -> Option<Vec<(String, String)>> {
        if !self.wrote {
            return None;
        }
        Some(match self.cart_total {
            Some(total) => vec![(format!("cart:{session}"), total.to_string())],
            None => Vec::new(),
        })
    }
}

/// An order's status as the program reads it (NEXT 6c): its case, and an
/// awaiting one's provider's page.
const AWAITING: &str = "awaiting-payment";
/// An order taken back before it was paid (step eight).
const CANCELED: &str = "canceled";

pub(crate) fn order_status(r: &mut dyn Rows, owner: &str, status: String) -> Result<Val, String> {
    // TRACK SEAM (scheduled-orders): a scheduled order carries its time.
    if status == crate::scheduled_orders::SCHEDULED {
        let time = r
            .order_time(owner)?
            .ok_or_else(|| format!("{owner}'s order is scheduled and has no time"))?;
        return Ok(Val::Variant(
            status,
            Some(Box::new(crate::scheduled_orders::scheduled_val(&time))),
        ));
    }
    if status != AWAITING {
        return Ok(Val::Variant(status, None));
    }
    let page = r.awaiting(owner)?.map(|a| a.page).unwrap_or_default();
    Ok(Val::Variant(
        status,
        Some(Box::new(Val::Record(vec![(
            "url".into(),
            Val::String(page),
        )]))),
    ))
}

/// **An awaiting order taken back** (NEXT 6c, step eight): canceled, its
/// lines back in the reader's cart beside what the cart holds now (a line of
/// the same item gains its quantity), its code's use released, and the
/// payment it awaited forgotten. The checkout's choices are the reader's to
/// make again.
fn take_back(r: &mut dyn Rows, owner: &str) -> Result<(), String> {
    let Some(order) = r.order(owner)? else {
        return Ok(());
    };
    let mut cart = r.cart(owner)?;
    for line in order.lines {
        match cart.iter_mut().find(|held| held.item == line.item) {
            Some(held) => held.quantity += line.quantity,
            None => cart.push(line),
        }
    }
    r.set_cart(owner, &cart)?;
    if let Some(code) = r.charged(owner)?.and_then(|c| c.promo) {
        r.release_promo(owner, &code)?;
    }
    r.set_status(owner, Some(CANCELED))?;
    r.set_awaiting(owner, None)
}

pub(crate) fn ok(v: Val) -> Vec<Val> {
    vec![Val::Result(Ok(Some(Box::new(v))))]
}

pub(crate) fn declared(case: &str) -> Vec<Val> {
    vec![Val::Result(Err(Some(Box::new(Val::Variant(
        case.into(),
        None,
    )))))]
}

fn not_found() -> Vec<Val> {
    declared("not-found")
}

/// A host operation's answer: done, with a value where it has one.
fn done(v: Option<Val>) -> Vec<Val> {
    vec![Val::Result(Ok(v.map(Box::new)))]
}

/// A host operation refused, and why: nothing it staged commits.
fn refused(why: String) -> Vec<Val> {
    vec![Val::Result(Err(Some(Box::new(Val::String(why)))))]
}

/// A store as the program's `Store` is (ADR-0125, ADR-0166).
fn store_val(row: &StoreRow) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(row.id.clone())),
        ("name".into(), Val::String(row.name.clone())),
        // A program whose `Store` declares none is not given it (ADR-0166):
        // the benchmark's.
        ("description".into(), Val::String(row.description.clone())),
        // What it serves (ADR-0308, amended); a program whose `Store`
        // declares none is not given it (ADR-0166).
        ("cuisine".into(), Val::String(row.cuisine.clone())),
        (
            "hours".into(),
            Val::Record(vec![
                ("opens-minute".into(), Val::S64(row.opens_minute)),
                ("closes-minute".into(), Val::S64(row.closes_minute)),
            ]),
        ),
    ])
}

/// An item as the program's `MenuItem` is (ADR-0166): the benchmark's, which
/// declares fewer fields, is passed what it declares.
fn item_val(store: &str, item: &Item) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(item.id.clone())),
        // Its store, and its category (ADR-0181).
        ("store-id".into(), Val::String(store.to_string())),
        ("name".into(), Val::String(item.name.clone())),
        ("description".into(), Val::String(item.description.clone())),
        // Its price, in cents (ADR-0169).
        ("price".into(), Val::S64(item.price)),
        // Whether it can be ordered now (ADR-0178): what
        // `menus#is-available` answers.
        ("available".into(), Val::Bool(item.available)),
        (
            "category".into(),
            Val::Record(vec![
                ("id".into(), Val::String(item.category.clone())),
                ("name".into(), Val::String(item.category_name.clone())),
            ]),
        ),
    ])
}

/// `{ id, name }`, as a recommendation and a category's item are.
fn named(id: &str, name: &str) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(id.to_string())),
        ("name".into(), Val::String(name.to_string())),
    ])
}

/// **The menu, grouped by category** (ADR-0181): each category in the
/// order its first item is listed, with its items in theirs.
fn sections(store: &str, menu: &[Item]) -> Val {
    let mut grouped: Vec<(String, Val, Vec<Val>)> = Vec::new();
    for item in menu {
        let row = item_val(store, item);
        match grouped.iter_mut().find(|(c, ..)| *c == item.category) {
            Some((_, _, items)) => items.push(row),
            None => grouped.push((
                item.category.clone(),
                named(&item.category, &item.category_name),
                vec![row],
            )),
        }
    }
    Val::List(
        grouped
            .into_iter()
            .map(|(_, category, items)| {
                Val::Record(vec![
                    ("category".into(), category),
                    ("items".into(), Val::List(items)),
                ])
            })
            .collect(),
    )
}

/// The session an operation was passed, which must be the one the host gave
/// the component, or the component is acting for someone else.
fn own_session<'v>(op: &str, session: &str, args: &'v [Val]) -> Result<&'v [Val], String> {
    let Some(Val::String(s)) = args.first() else {
        return Err(format!("{op} received {args:?}"));
    };
    if s != session {
        return Err(format!("{op} was passed another session"));
    }
    Ok(&args[1..])
}

/// **The reader an operation by a user's handle was passed** (track
/// `store-accounts`): the user's id, a handle on the wire (ADR-0270), which
/// only the host makes (ADR-0263), so the program can pass no one else's.
/// It is the owner of the rows the operation reads and writes.
fn reader_of<'v>(op: &str, args: &'v [Val]) -> Result<(&'v str, &'v [Val]), String> {
    match args.first() {
        Some(Val::String(reader)) if !reader.is_empty() => Ok((reader, &args[1..])),
        _ => Err(format!("{op} received {args:?}")),
    }
}

/// **The session's cart operations, as its faults make them** (ADR-0174).
/// A read waits the session's delay. The next write, and the next read,
/// fail once each as a database that is down fails: the operation answers
/// no value, and the component that called it traps.
pub(crate) fn faulted(
    cart_faults: &Arc<Mutex<BTreeMap<String, CartFaults>>>,
    delays_taken: &Arc<AtomicU64>,
    session: &str,
    mut host: Ops,
) -> Ops {
    for (name, writes) in [
        ("current", false),
        ("add", true),
        ("decrease", true),
        ("remove", true),
        ("clear", true),
    ] {
        for key in [
            format!("store:data/carts#{name}"),
            format!("store:data/user-carts#{name}"),
        ] {
            let Some(op) = host.remove(&key) else {
                continue;
            };
            let faults = cart_faults.clone();
            let delays = delays_taken.clone();
            let session = session.to_string();
            host.insert(
                key,
                Arc::new(move |args: &[Val]| {
                    let delay = {
                        let mut all = faults.lock().expect("cart faults");
                        let mut mine = all.get_mut(&session);
                        let failing = mine.as_deref_mut().is_some_and(|m| {
                            std::mem::take(if writes {
                                &mut m.fail_write
                            } else {
                                &mut m.fail_read
                            })
                        });
                        if failing {
                            return Err(format!("carts#{name}: the database is unavailable"));
                        }
                        mine.map(|m| if writes { 0 } else { m.delay_ms })
                            .unwrap_or_default()
                    };
                    if delay > 0 {
                        delays.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(delay));
                    }
                    op(args)
                }),
            );
        }
    }
    host
}

/// **The store's reads, for one session** (ADR-0218): the cart, with the
/// faults a test set; the session's order and its delivery estimate; and
/// the catalogue. Each through `with`, the layer's rows. Nothing a query does
/// commits.
///
/// Its slow sources poll `stopped`, to end early a read nobody is waiting
/// for (ADR-0152).
pub(crate) fn reads(with: With, session: &str, faults: &Faults, stopped: Option<Stopped>) -> Ops {
    let mut ops = faulted(
        &faults.cart_faults,
        &faults.delays_taken,
        session,
        unfaulted(with.clone(), session, faults),
    );
    ops.extend(catalog(with, faults, stopped));
    ops
}

/// [`reads`], before the cart's faults meet them.
fn unfaulted(with: With, session: &str, faults: &Faults) -> Ops {
    let mut ops: Ops = BTreeMap::new();
    let this = session.to_string();
    let w = with.clone();
    ops.insert(
        "store:data/carts#current".to_string(),
        Arc::new(move |args: &[Val]| {
            let rest = own_session("carts#current", &this, args)?;
            if !rest.is_empty() {
                return Err(format!("carts#current received {args:?}"));
            }
            w(&mut |r| Ok(ok(cart_value(&r.cart(&this)?))))
        }),
    );
    // **The reader's cart, by their handle** (track `store-accounts`): the
    // user's, in every session of theirs.
    let w = with.clone();
    ops.insert(
        "store:data/user-carts#current".to_string(),
        Arc::new(move |args: &[Val]| {
            let (reader, rest) = reader_of("user-carts#current", args)?;
            if !rest.is_empty() {
                return Err(format!("user-carts#current received {args:?}"));
            }
            w(&mut |r| Ok(ok(cart_value(&r.cart(reader)?))))
        }),
    );
    // The order, as its status's case (E14, T04). A case the program's
    // type does not have is refused by the component's types, as any value
    // the host gives is. **And the lines it was placed with** (ADR-0193), a
    // cart's value, or none before one is placed: no program reads it yet,
    // and the store's tests read an order through it.
    //
    // By the session (`store:data/orders`, the benchmark's copy of the store)
    // and by the reader's handle (`store:data/user-orders`, track
    // `store-accounts`): a user's order is read by its user alone, as a
    // notification is (ADR-0274).
    for (interface, by_reader) in [("orders", false), ("user-orders", true)] {
        let (this, w) = (session.to_string(), with.clone());
        ops.insert(
            format!("store:data/{interface}#current"),
            Arc::new(move |args: &[Val]| {
                let op = format!("{interface}#current");
                let owner = if by_reader {
                    reader_of(&op, args)?.0
                } else {
                    own_session(&op, &this, args)?;
                    this.as_str()
                };
                w(&mut |r| {
                    let status = match r.order(owner)? {
                        Some(o) => Some(Box::new(order_status(r, owner, o.status)?)),
                        None => None,
                    };
                    Ok(ok(Val::Option(status)))
                })
            }),
        );
        let (this, w) = (session.to_string(), with.clone());
        ops.insert(
            format!("store:data/{interface}#placed"),
            Arc::new(move |args: &[Val]| {
                let op = format!("{interface}#placed");
                let owner = if by_reader {
                    reader_of(&op, args)?.0
                } else {
                    own_session(&op, &this, args)?;
                    this.as_str()
                };
                w(&mut |r| {
                    let cart = r.order(owner)?.map(|o| Box::new(cart_value(&o.lines)));
                    Ok(vec![Val::Option(cart)])
                })
            }),
        );
    }
    // **The payment the reader's order awaits** (NEXT 6c, step eight): the
    // order's reference and the payment's id, where an order of theirs
    // awaits one the provider has begun.
    let w = with.clone();
    ops.insert(
        "store:data/user-orders#awaited".to_string(),
        Arc::new(move |args: &[Val]| {
            let (owner, _) = reader_of("user-orders#awaited", args)?;
            w(&mut |r| {
                let awaits = r.order(owner)?.is_some_and(|o| o.status == AWAITING);
                let awaited = match r.awaiting(owner)? {
                    Some(a) if awaits && !a.payment.is_empty() => {
                        Some(Box::new(Val::Record(vec![
                            ("order".into(), Val::String(a.reference)),
                            ("payment".into(), Val::String(a.payment)),
                        ])))
                    }
                    _ => None,
                };
                Ok(vec![Val::Option(awaited)])
            })
        }),
    );
    // The session's delivery estimate (E14, T10), after the estimator's
    // delay: a slow source, which a streamed region does not wait for.
    let estimator = faults
        .estimators
        .lock()
        .expect("estimators")
        .get(session)
        .cloned()
        .unwrap_or_default();
    let (this, w) = (session.to_string(), with.clone());
    ops.insert(
        "store:data/estimates#current".to_string(),
        Arc::new(move |args: &[Val]| {
            own_session("estimates#current", &this, args)?;
            // Held until a test lets it go (ADR-0223), then slow.
            if let Some(gate) = &estimator.gate {
                gate.wait();
            }
            std::thread::sleep(std::time::Duration::from_millis(estimator.delay_ms));
            match estimator.fail.as_deref() {
                Some("declared") => return Ok(declared("location-unavailable")),
                Some(_) => return Err("the estimator is down".to_string()),
                None => {}
            }
            // A range, and when it was made (ADR-0180). And `minutes`, which
            // the benchmark's own store still reads (ADR-0156): a program is
            // passed the fields its type declares (ADR-0166).
            w(&mut |r| {
                let (minutes, max) = r.estimate(&this)?.unwrap_or(DEFAULT_ESTIMATE);
                Ok(ok(Val::Record(vec![
                    ("minutes".into(), Val::S64(minutes)),
                    ("min-minutes".into(), Val::S64(minutes)),
                    ("max-minutes".into(), Val::S64(max.unwrap_or(minutes + 10))),
                    ("generated-at".into(), Val::S64(wall_millis())),
                ])))
            })
        }),
    );
    // **A store's estimate to the reader's chosen address** (track
    // `store-accounts`, milestone 2): the session's estimator, as above, and
    // the courier's minutes from the store to the place on top; a store that
    // does not reach it answers `no-coverage`, by the one rule a command's
    // refusal reads (`places::delivers_to`). No address chosen, no travel.
    // The estimator's faults are the session's, a test's: the query is kept
    // for no time and privately, so each document's read is its session's.
    let estimator = faults
        .estimators
        .lock()
        .expect("estimators")
        .get(session)
        .cloned()
        .unwrap_or_default();
    let (this, w) = (session.to_string(), with.clone());
    ops.insert(
        "store:data/user-estimates#for-store".to_string(),
        Arc::new(move |args: &[Val]| {
            let [Val::String(store), Val::String(reader)] = args else {
                return Err(format!("user-estimates#for-store received {args:?}"));
            };
            // Held until a test lets it go (ADR-0223), then slow.
            if let Some(gate) = &estimator.gate {
                gate.wait();
            }
            std::thread::sleep(std::time::Duration::from_millis(estimator.delay_ms));
            match estimator.fail.as_deref() {
                Some("declared") => return Ok(declared("location-unavailable")),
                Some(_) => return Err("the estimator is down".to_string()),
                None => {}
            }
            w(&mut |r| {
                let travel = match coverage(r, store, reader)? {
                    None => 0,
                    Some(Reach {
                        delivers: false, ..
                    }) => return Ok(declared("no-coverage")),
                    Some(Reach { minutes, .. }) => minutes,
                };
                let (minutes, max) = r.estimate(&this)?.unwrap_or(DEFAULT_ESTIMATE);
                let max = max.unwrap_or(minutes + 10);
                Ok(ok(Val::Record(vec![
                    ("minutes".into(), Val::S64(minutes + travel)),
                    ("min-minutes".into(), Val::S64(minutes + travel)),
                    ("max-minutes".into(), Val::S64(max + travel)),
                    ("generated-at".into(), Val::S64(wall_millis())),
                ])))
            })
        }),
    );
    // ADR-0347 integration: estimates for all stores in one operation.
    // Read the address, store rows and estimate once. Folding the returned
    // values never issues another database read, whatever the key count.
    let estimator = faults
        .estimators
        .lock()
        .expect("estimators")
        .get(session)
        .cloned()
        .unwrap_or_default();
    let (this, w) = (session.to_string(), with.clone());
    ops.insert(
        "store:data/user-estimates#for-stores".to_string(),
        Arc::new(move |args: &[Val]| {
            let [Val::List(keys), Val::String(reader)] = args else {
                return Err(format!("user-estimates#for-stores received {args:?}"));
            };
            let stores: Vec<String> = keys
                .iter()
                .map(|key| match key {
                    Val::String(id) => Ok(id.clone()),
                    other => Err(format!("user-estimates#for-stores received key {other:?}")),
                })
                .collect::<Result<_, _>>()?;
            if stores.is_empty() {
                return Ok(vec![Val::List(Vec::new())]);
            }
            std::thread::sleep(std::time::Duration::from_millis(estimator.delay_ms));
            if estimator
                .fail
                .as_deref()
                .is_some_and(|why| why != "declared")
            {
                return Err("the estimator is down".to_string());
            }
            if estimator.fail.as_deref() == Some("declared") {
                let refused = stores
                    .iter()
                    .map(|store| {
                        Val::Record(vec![
                            ("store".into(), Val::String(store.clone())),
                            (
                                "estimate".into(),
                                declared("location-unavailable").remove(0),
                            ),
                        ])
                    })
                    .collect();
                return Ok(vec![Val::List(refused)]);
            }
            w(&mut |r| {
                let coverage = coverage_of(r, &stores, reader)?;
                let bounds = r.estimate(&this)?;
                let (minutes, max) = bounds.unwrap_or(DEFAULT_ESTIMATE);
                let upper = max.unwrap_or(minutes + 10);
                let generated_at = wall_millis();
                let values = stores
                    .iter()
                    .map(|store| {
                        let estimate = if coverage
                            .get(store)
                            .and_then(|c| c.as_ref())
                            .is_some_and(|c| !c.delivers)
                        {
                            declared("no-coverage").remove(0)
                        } else {
                            let travel = coverage
                                .get(store)
                                .and_then(|c| c.as_ref())
                                .map_or(0, |c| c.minutes);
                            ok(Val::Record(vec![
                                ("minutes".into(), Val::S64(minutes + travel)),
                                ("min-minutes".into(), Val::S64(minutes + travel)),
                                ("max-minutes".into(), Val::S64(upper + travel)),
                                ("generated-at".into(), Val::S64(generated_at)),
                            ]))
                            .remove(0)
                        };
                        Val::Record(vec![
                            ("store".into(), Val::String(store.clone())),
                            ("estimate".into(), estimate),
                        ])
                    })
                    .collect();
                Ok(vec![Val::List(values)])
            })
        }),
    );
    // **Whether a store reaches the reader's chosen address** (track
    // `store-accounts`): what the store's page says where the menu is.
    let w = with.clone();
    ops.insert(
        "store:data/coverage#for-store".to_string(),
        Arc::new(move |args: &[Val]| {
            let [Val::String(store), Val::String(reader)] = args else {
                return Err(format!("coverage#for-store received {args:?}"));
            };
            w(&mut |r| {
                let reach = coverage(r, store, reader)?;
                Ok(vec![Val::Record(vec![
                    ("chosen".into(), Val::Bool(reach.is_some())),
                    (
                        "delivers".into(),
                        Val::Bool(reach.as_ref().is_none_or(|r| r.delivers)),
                    ),
                    (
                        "to".into(),
                        Val::String(reach.as_ref().map(|r| r.label.clone()).unwrap_or_default()),
                    ),
                    (
                        "travel-minutes".into(),
                        Val::S64(reach.as_ref().map_or(0, |r| r.minutes)),
                    ),
                ])])
            })
        }),
    );
    // **The reader's saved addresses** (track `store-accounts`), by their
    // handle: the user's, or the session's guest's.
    let w = with.clone();
    ops.insert(
        "store:data/addresses#list".to_string(),
        Arc::new(move |args: &[Val]| {
            let (reader, _) = reader_of("addresses#list", args)?;
            w(&mut |r| Ok(vec![addresses_val(&r.addresses(reader)?)]))
        }),
    );
    // **The places an address may name** (track `store-accounts`): the
    // fixed table, `places::PLACES`.
    ops.insert(
        "store:data/places#list".to_string(),
        Arc::new(move |args: &[Val]| {
            if !args.is_empty() {
                return Err(format!("places#list received {args:?}"));
            }
            Ok(vec![Val::List(
                crate::places::PLACES
                    .iter()
                    .map(|p| named(p.id, p.name))
                    .collect(),
            )])
        }),
    );
    ops
}

/// **Whether a store reaches a reader's chosen address** (track
/// `store-accounts`): none where no address is chosen; else whether
/// `places::delivers_to`, the courier's minutes, and the address's label. A
/// store the layer does not hold reaches nowhere.
pub(crate) struct Reach {
    pub(crate) delivers: bool,
    pub(crate) minutes: i64,
    pub(crate) label: String,
}

pub(crate) fn coverage(
    r: &mut dyn Rows,
    store: &str,
    owner: &str,
) -> Result<Option<Reach>, String> {
    let Some(chosen) = r.addresses(owner)?.into_iter().find(|a| a.chosen) else {
        return Ok(None);
    };
    let place = crate::places::place(&chosen.place)
        .ok_or_else(|| format!("address `{}` names no place `{}`", chosen.id, chosen.place))?;
    // The one store's row (NEXT 4b), not every store's.
    let zone = r
        .stores_by_ids(&[store.to_string()])?
        .into_iter()
        .next()
        .map(|s| s.zone);
    Ok(Some(match zone {
        Some(zone) => Reach {
            delivers: crate::places::delivers_to(&zone, place),
            minutes: crate::places::travel_minutes(&zone, place),
            label: chosen.label,
        },
        None => Reach {
            delivers: false,
            minutes: 0,
            label: chosen.label,
        },
    }))
}

/// **The store that lists each of `items`, by scanning every store's
/// menu**: memory's way, free; on PostgreSQL a statement per store, which
/// its own `stores_of_items` is not (NEXT 4b).
pub(crate) fn scanned_stores_of_items<R: Rows + ?Sized>(
    r: &mut R,
    items: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for s in r.stores()? {
        for i in r.menu(&s.id)? {
            if items.contains(&i.id) {
                out.entry(i.id).or_insert_with(|| s.id.clone());
            }
        }
    }
    Ok(out)
}

/// **The hours of each of `stores`, a store at a time**: memory's way; on
/// PostgreSQL three statements a store, which its own `hours_of` is not.
pub(crate) fn hours_one_by_one<R: Rows + ?Sized>(
    r: &mut R,
    stores: &[String],
) -> Result<Vec<(String, crate::hours::Hours)>, String> {
    let mut out = Vec::new();
    for s in stores {
        if let Some(h) = r.hours(s)? {
            out.push((s.clone(), h));
        }
    }
    Ok(out)
}

/// **Whether each of `stores` reaches the owner's chosen address**, by set
/// (NEXT 4b): the owner's addresses once and the stores' rows at once,
/// whatever their number. None for a store where no address is chosen.
pub(crate) fn coverage_of(
    r: &mut dyn Rows,
    stores: &[String],
    owner: &str,
) -> Result<BTreeMap<String, Option<Reach>>, String> {
    let Some(chosen) = r.addresses(owner)?.into_iter().find(|a| a.chosen) else {
        return Ok(stores.iter().map(|s| (s.clone(), None)).collect());
    };
    let place = crate::places::place(&chosen.place)
        .ok_or_else(|| format!("address `{}` names no place `{}`", chosen.id, chosen.place))?;
    let zones: BTreeMap<String, crate::places::Zone> = r
        .stores_by_ids(stores)?
        .into_iter()
        .map(|s| (s.id, s.zone))
        .collect();
    Ok(stores
        .iter()
        .map(|s| {
            let reach = match zones.get(s) {
                Some(zone) => Reach {
                    delivers: crate::places::delivers_to(zone, place),
                    minutes: crate::places::travel_minutes(zone, place),
                    label: chosen.label.clone(),
                },
                None => Reach {
                    delivers: false,
                    minutes: 0,
                    label: chosen.label.clone(),
                },
            };
            (s.clone(), Some(reach))
        })
        .collect())
}

/// **Whether every line of a cart is delivered to the owner's chosen
/// address** (track `store-accounts`): a line's item from the store that
/// lists it, which reaches the address. True where no address is chosen. By
/// set (NEXT 4b): the lines' stores in one read, their reach in two.
fn cart_reaches(r: &mut dyn Rows, lines: &[Line], owner: &str) -> Result<bool, String> {
    if !r.addresses(owner)?.iter().any(|a| a.chosen) {
        return Ok(true);
    }
    let items: Vec<String> = lines.iter().map(|l| l.item.clone()).collect();
    let of_item = r.stores_of_items(&items)?;
    let stores: Vec<String> = of_item.values().cloned().collect();
    let reach = coverage_of(r, &stores, owner)?;
    Ok(lines.iter().all(|line| {
        of_item
            .get(&line.item)
            .and_then(|s| reach.get(s))
            .is_some_and(|c| c.as_ref().is_some_and(|c| c.delivers))
    }))
}

/// A reader's addresses as the program's `List<Address>` is.
fn addresses_val(rows: &[AddressRow]) -> Val {
    Val::List(
        rows.iter()
            .map(|a| {
                Val::Record(vec![
                    ("id".into(), Val::String(a.id.clone())),
                    ("label".into(), Val::String(a.label.clone())),
                    (
                        "place".into(),
                        Val::String(
                            crate::places::place(&a.place)
                                .map(|p| p.name.to_string())
                                .unwrap_or_default(),
                        ),
                    ),
                    ("chosen".into(), Val::Bool(a.chosen)),
                ])
            })
            .collect(),
    )
}

/// What a session no test gave an estimate is estimated (ADR-0180).
pub(crate) const DEFAULT_ESTIMATE: Estimate = (25, None);

/// **The deployment's catalogue** (ADR-0125): `store:data/stores#get` and
/// `store:data/menus#sections`, what the `Store` and `Menu` queries read,
/// and `menus#for-store`, the benchmark's own store's menu (ADR-0156,
/// ADR-0181); the notice, the kitchen, the menu by category and the
/// recommender. Any store the layer does not hold is answered `not-found`.
fn catalog(with: With, faults: &Faults, stopped: Option<Stopped>) -> Ops {
    let mut ops: Ops = BTreeMap::new();
    ops.extend(crate::search::ops(with.clone())); // TRACK SEAM (store-search)
    ops.extend(crate::hours::ops(with.clone())); // TRACK SEAM (store-hours)
    ops.extend(crate::checkout::ops(with.clone())); // TRACK SEAM (order-total)
    ops.extend(crate::reviews::ops(with.clone())); // TRACK SEAM (reviews)
    // Charter §15.5's store delay (ADR-0174), and its origin failing once
    // (ADR-0177), as a test set them.
    let store_delay = faults.store_delay_ms.clone();
    let store_delays = faults.delays_taken.clone();
    let store_fails = faults.store_fails_next.clone();
    let w = with.clone();
    ops.insert(
        "store:data/stores#get".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => {
                // The one store's row (NEXT 4b), not every store's.
                let found = w(&mut |r| {
                    Ok(r.stores_by_ids(std::slice::from_ref(id))?
                        .iter()
                        .map(store_val)
                        .collect())
                })?;
                let Some(store) = found.into_iter().next() else {
                    return Ok(not_found());
                };
                if store_fails.swap(false, Ordering::SeqCst) {
                    return Err("stores#get: the store's origin is unavailable".to_string());
                }
                let delay = store_delay.load(Ordering::SeqCst);
                if delay > 0 {
                    store_delays.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                }
                Ok(ok(store))
            }
            other => Err(format!("stores#get received {other:?}")),
        }),
    );
    // Every store this server holds, in its order (ADR-0192): what the home
    // page lists.
    let w = with.clone();
    ops.insert(
        "store:data/stores#list".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [] => w(&mut |r| Ok(vec![Val::List(r.stores()?.iter().map(store_val).collect())])),
            other => Err(format!("stores#list received {other:?}")),
        }),
    );
    // A store's menu, `None` where the layer holds no such store.
    fn menu_of(r: &mut dyn Rows, id: &str) -> Result<Option<Vec<Item>>, String> {
        if r.stores_by_ids(&[id.to_string()])?.is_empty() {
            return Ok(None);
        }
        r.menu(id).map(Some)
    }
    let w = with.clone();
    ops.insert(
        "store:data/menus#for-store".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => w(&mut |r| {
                Ok(match menu_of(r, id)? {
                    Some(menu) => ok(Val::List(menu.iter().map(|i| item_val(id, i)).collect())),
                    None => not_found(),
                })
            }),
            other => Err(format!("menus#for-store received {other:?}")),
        }),
    );
    let w = with.clone();
    ops.insert(
        "store:data/menus#sections".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => w(&mut |r| {
                Ok(match menu_of(r, id)? {
                    Some(menu) => ok(sections(id, &menu)),
                    None => not_found(),
                })
            }),
            other => Err(format!("menus#sections received {other:?}")),
        }),
    );
    // What the recommender answers (ADR-0148), after its delay: a slow
    // source, which a streamed region does not wait for. What it suggests is
    // what a test curated, or each store's own menu after its first item, two
    // of them (ADR-0165), so it changes when the menu does.
    let recommender = faults.recommender.lock().expect("recommender").clone();
    let w = with.clone();
    ops.insert(
        "store:data/recommendations#for-store".to_string(),
        Arc::new(move |args: &[Val]| {
            let [Val::String(store)] = args else {
                return Err(format!("recommendations#for-store received {args:?}"));
            };
            // Held until a test lets it go (ADR-0223), then slow.
            if let Some(gate) = &recommender.gate {
                gate.wait();
            }
            std::thread::sleep(std::time::Duration::from_millis(recommender.delay_ms));
            let items = w(&mut |r| {
                let items = match r.curated()? {
                    Some(items) => items,
                    None => match menu_of(r, store)? {
                        Some(menu) => recommended_from(
                            &menu
                                .iter()
                                .map(|i| (i.id.clone(), i.name.clone()))
                                .collect::<Vec<_>>(),
                        ),
                        None => Vec::new(),
                    },
                };
                Ok(items.iter().map(|(id, name)| named(id, name)).collect())
            })?;
            match recommender.fail.as_deref() {
                Some("host") => Err("the recommender is down".to_string()),
                Some(_) => Ok(declared("none-available")),
                None => Ok(ok(Val::List(items))),
            }
        }),
    );
    // What the store has posted (E14, T09). Every call is counted, as every
    // data-layer call is: what `/bench/calls` reports.
    let w = with.clone();
    ops.insert(
        "store:data/notices#current".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => w(&mut |r| {
                Ok(match r.notice(id)? {
                    Some(text) => ok(Val::String(text)),
                    None => not_found(),
                })
            }),
            other => Err(format!("notices#current received {other:?}")),
        }),
    );
    // How long the kitchen takes now (E14, T02), after [`KITCHEN_MS`]: a
    // slow source. The minutes are the ones it was asked with.
    let kitchen_gate = faults.kitchen_gate.lock().expect("kitchen").clone();
    let w = with.clone();
    ops.insert(
        "store:data/kitchen#prep-minutes".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => {
                let minutes = w(&mut |r| Ok(r.prep(id)?.map(Val::S64).into_iter().collect()))?;
                let Some(minutes) = minutes.into_iter().next() else {
                    return Ok(not_found());
                };
                // Held until a test lets it go (ADR-0223), then slow.
                if let Some(gate) = &kitchen_gate {
                    gate.wait();
                }
                std::thread::sleep(std::time::Duration::from_millis(KITCHEN_MS));
                Ok(ok(minutes))
            }
            other => Err(format!("kitchen#prep-minutes received {other:?}")),
        }),
    );
    // The menu's items served hot or cold (E14, T07), the slow category after
    // its delay. A read that is stopped meanwhile ends early, and says so:
    // what `/bench/calls` counts as stopped.
    let categories = faults.categories.lock().expect("categories").clone();
    let ended_early = faults.category_stopped.clone();
    ops.insert(
        "store:data/menus#in-category".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id), Val::String(category)] => {
                let Some(menu) = with(&mut |r| {
                    Ok(menu_of(r, id)?
                        .map(|menu| {
                            Val::List(
                                menu.iter()
                                    .filter(|i| category == "all" || i.served == *category)
                                    .map(|i| named(&i.id, &i.name))
                                    .collect(),
                            )
                        })
                        .into_iter()
                        .collect())
                })?
                .into_iter()
                .next() else {
                    return Ok(not_found());
                };
                if categories.slow.as_deref() == Some(category.as_str()) {
                    // Held until a test lets it go, or until nobody is waiting
                    // for it: stopped part way, not read to its end.
                    if let Some(gate) = &categories.gate {
                        let nobody = || stopped.as_ref().is_some_and(|s| s());
                        if gate.wait_unless(&nobody) {
                            ended_early.fetch_add(1, Ordering::SeqCst);
                            return Err("stopped: nobody is waiting for it".to_string());
                        }
                    }
                    let until = std::time::Instant::now()
                        + std::time::Duration::from_millis(categories.delay_ms);
                    while std::time::Instant::now() < until {
                        if stopped.as_ref().is_some_and(|s| s()) {
                            ended_early.fetch_add(1, Ordering::SeqCst);
                            return Err("stopped: nobody is waiting for it".to_string());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                Ok(ok(menu))
            }
            other => Err(format!("menus#in-category received {other:?}")),
        }),
    );
    ops
}

/// **A command's operations** (ADR-0218): every read, in the command's own
/// transaction; the cart's writes and the order a session places, which the
/// program's commands call; and the host's own writes (`store:host/…`),
/// which routes that change data call and no program is linked to. Each
/// write stages in `with`'s rows, and `written` says what is left to
/// commit. `fail` makes the cart's operations answer `cart-expired` (a
/// test's).
pub(crate) fn writes(
    with: With,
    session: &str,
    faults: &Faults,
    fail: bool,
    written: Arc<Mutex<Written>>,
) -> Ops {
    let mut ops = unfaulted(with.clone(), session, faults);
    ops.extend(catalog(with.clone(), faults, None));
    // **The deployment's data layer: `store:data/carts`, whole.** Every
    // operation the interface declares, whichever command is running; the
    // host links only those the component imports and was granted.
    type Change = fn(&mut dyn Rows, &mut Lines, &[Val]) -> Result<(), String>;
    //
    // Each by the session (`store:data/carts`, the benchmark's copy of the
    // store) and by the reader's handle (`store:data/user-carts`, track
    // `store-accounts`): the same change, to the rows of the owner each
    // names.
    let mut cart = |name: &'static str, change: Change| {
        for (interface, by_reader) in [("carts", false), ("user-carts", true)] {
            let (this, w, written) = (session.to_string(), with.clone(), written.clone());
            ops.insert(
                format!("store:data/{interface}#{name}"),
                Arc::new(move |args: &[Val]| {
                    let op = format!("{interface}#{name}");
                    let (owner, rest) = if by_reader {
                        reader_of(&op, args)?
                    } else {
                        (this.as_str(), own_session(&op, &this, args)?)
                    };
                    if fail {
                        return Ok(declared("cart-expired"));
                    }
                    w(&mut |r| {
                        let mut lines = r.cart(owner)?;
                        change(r, &mut lines, rest)?;
                        r.set_cart(owner, &lines)?;
                        let mut written = written.lock().expect("written");
                        written.wrote = true;
                        written.cart_total = Some(lines.iter().map(|l| l.quantity).sum());
                        Ok(ok(cart_value(&lines)))
                    })
                }),
            );
        }
    };
    cart("add", |r, lines, args| {
        let [Val::String(item), Val::S64(quantity)] = args else {
            return Err(format!("carts#add received {args:?}"));
        };
        match lines.iter_mut().find(|l| l.item == *item) {
            Some(line) => line.quantity += quantity,
            None => {
                // The line records its item as it is now (ADR-0172).
                let (name, price, _) = r
                    .catalog_item(item)?
                    .ok_or_else(|| format!("carts#add: no store has an item `{item}`"))?;
                lines.push(Line {
                    item: item.clone(),
                    quantity: *quantity,
                    name,
                    price,
                });
            }
        }
        Ok(())
    });
    // One fewer; a line that holds one goes (ADR-0172). A line that is not
    // there is not: another page took it away first.
    cart("decrease", |_, lines, args| {
        let [Val::String(item)] = args else {
            return Err(format!("carts#decrease received {args:?}"));
        };
        if let Some(at) = lines.iter().position(|l| l.item == *item) {
            if lines[at].quantity > 1 {
                lines[at].quantity -= 1;
            } else {
                lines.remove(at);
            }
        }
        Ok(())
    });
    // The line, gone (ADR-0172).
    cart("remove", |_, lines, args| {
        let [Val::String(item)] = args else {
            return Err(format!("carts#remove received {args:?}"));
        };
        lines.retain(|l| l.item != *item);
        Ok(())
    });
    cart("clear", |_, lines, args| {
        if !args.is_empty() {
            return Err(format!("carts#clear received {args:?}"));
        }
        lines.clear();
        Ok(())
    });
    // **The store's data layer for orders, as a command sees it**
    // (ADR-0193): `orders#place` places the session's cart as its order.
    // The order is the cart's lines and the status `placed`, and the cart
    // is emptied, in the command's own transaction. An empty cart places
    // nothing, and says so. Until 2026-10-08 the order kept its status
    // alone, and its lines went with the emptied cart.
    //
    // By the session, and by the reader's handle (track `store-accounts`):
    // a user's order is placed from the user's cart.
    for (interface, by_reader) in [("orders", false), ("user-orders", true)] {
        let (this, w, placed) = (session.to_string(), with.clone(), written.clone());
        ops.insert(
            format!("store:data/{interface}#place"),
            Arc::new(move |args: &[Val]| {
                let op = format!("{interface}#place");
                let (owner, rest) = if by_reader {
                    reader_of(&op, args)?
                } else {
                    (this.as_str(), own_session(&op, &this, args)?)
                };
                // TRACK SEAM (order-total): by the reader's handle, with the
                // order as the program charged it, and the code it used.
                let charged = if by_reader {
                    Some(crate::checkout::place_args(rest)?)
                } else if rest.is_empty() {
                    None
                } else {
                    return Err(format!("{op} received {args:?}"));
                };
                w(&mut |r| {
                    let lines = r.cart(owner)?;
                    if lines.is_empty() {
                        return Ok(declared("nothing-to-order"));
                    }
                    // **An order the chosen address is out of reach of is
                    // refused** (track `store-accounts`): a cart can outlive
                    // an address's change, so the order is held to it too.
                    if by_reader && !cart_reaches(r, &lines, owner)? {
                        return Ok(declared("no-coverage"));
                    }
                    match &charged {
                        Some(charged) => {
                            if !crate::checkout::place_charged(r, owner, &lines, charged)? {
                                return Ok(crate::checkout::already_used());
                            }
                        }
                        None => r.place(owner, &lines)?,
                    }
                    r.set_cart(owner, &[])?;
                    let mut written = placed.lock().expect("written");
                    written.wrote = true;
                    written.cart_total = Some(0);
                    Ok(ok(Val::Variant("placed".into(), None)))
                })
            }),
        );
    }
    // **An order awaiting its payment** (NEXT 6c, step seven): placed as
    // `user-orders#place` places one, its status `awaiting-payment`, with a
    // reference of its own, which the payment carries. Refused while an
    // order of the reader's awaits one.
    let (w, placed) = (with.clone(), written.clone());
    ops.insert(
        "store:data/user-orders#await-payment".to_string(),
        Arc::new(move |args: &[Val]| {
            let op = "user-orders#await-payment";
            let (owner, rest) = reader_of(op, args)?;
            // TRACK SEAM (scheduled-orders): the time it is for, last, none
            // for as soon as possible.
            let Some((time, rest)) = rest.split_last() else {
                return Err(format!("{op} received {args:?}"));
            };
            let charged = crate::checkout::place_args(rest)?;
            w(&mut |r| {
                let current = r.order(owner)?.map(|o| o.status);
                if current.as_deref() == Some(AWAITING) {
                    return Ok(declared("awaiting-payment"));
                }
                // TRACK SEAM (scheduled-orders): nor beside one scheduled,
                // which is sent to its stores later.
                if current.as_deref() == Some(crate::scheduled_orders::SCHEDULED) {
                    return Ok(declared("scheduled"));
                }
                let lines = r.cart(owner)?;
                if lines.is_empty() {
                    return Ok(declared("nothing-to-order"));
                }
                if !cart_reaches(r, &lines, owner)? {
                    return Ok(declared("no-coverage"));
                }
                if !crate::checkout::place_charged(r, owner, &lines, &charged)? {
                    return Ok(crate::checkout::already_used());
                }
                r.set_status(owner, Some(AWAITING))?;
                r.set_cart(owner, &[])?;
                let reference = format!("o-{}", &crate::identity::token()[..16]);
                r.set_awaiting(
                    owner,
                    Some(&crate::payments::Awaiting {
                        reference: reference.clone(),
                        ..Default::default()
                    }),
                )?;
                // TRACK SEAM (scheduled-orders): for a time, with it.
                if let Some(t) = crate::scheduled_orders::time_from(time, &reference)? {
                    r.set_order_time(owner, &t)?;
                }
                let mut written = placed.lock().expect("written");
                written.wrote = true;
                written.cart_total = Some(0);
                Ok(ok(Val::String(reference)))
            })
        }),
    );
    // **The payment the reader's awaiting order awaits**, and the
    // provider's page for it. Another order's reference changes nothing.
    let (w, wrote) = (with.clone(), written.clone());
    ops.insert(
        "store:data/user-orders#paying".to_string(),
        Arc::new(move |args: &[Val]| {
            let op = "user-orders#paying";
            let (owner, rest) = reader_of(op, args)?;
            let [Val::String(reference), Val::Record(payment)] = rest else {
                return Err(format!("{op} received {args:?}"));
            };
            let field = |name: &str| match payment.iter().find(|(n, _)| n == name) {
                Some((_, Val::String(v))) => Ok(v.clone()),
                other => Err(format!("{op}'s payment has no `{name}`: {other:?}")),
            };
            let (id, page) = (field("id")?, field("page")?);
            w(&mut |r| {
                if let Some(a) = r.awaiting(owner)?
                    && a.reference == *reference
                {
                    r.set_awaiting(
                        owner,
                        Some(&crate::payments::Awaiting {
                            reference: reference.clone(),
                            payment: id.clone(),
                            page: page.clone(),
                        }),
                    )?;
                    wrote.lock().expect("written").wrote = true;
                }
                let status = r.order(owner)?.map(|o| o.status).unwrap_or_default();
                Ok(ok(order_status(r, owner, status)?))
            })
        }),
    );
    // **The reader's order's payment, settled as the provider says it**:
    // placed where it succeeded and the order awaits that payment; the
    // order as it is otherwise, a payment heard twice or for an order the
    // reader's is no longer changing nothing.
    let (w, wrote) = (with.clone(), written.clone());
    ops.insert(
        "store:data/user-orders#settle".to_string(),
        Arc::new(move |args: &[Val]| {
            let op = "user-orders#settle";
            let (owner, rest) = reader_of(op, args)?;
            let [Val::String(reference), Val::Variant(status, None)] = rest else {
                return Err(format!("{op} received {args:?}"));
            };
            w(&mut |r| {
                let awaits = r.order(owner)?.is_some_and(|o| o.status == AWAITING)
                    && r.awaiting(owner)?
                        .is_some_and(|a| a.reference == *reference);
                if awaits && status == "succeeded" {
                    // TRACK SEAM (scheduled-orders): for a time, scheduled,
                    // its stores told nothing yet; placed otherwise.
                    let timed = r
                        .order_time(owner)?
                        .is_some_and(|t| t.reference == *reference);
                    let settled = if timed {
                        crate::scheduled_orders::SCHEDULED
                    } else {
                        "placed"
                    };
                    r.set_status(owner, Some(settled))?;
                    r.set_awaiting(owner, None)?;
                    wrote.lock().expect("written").wrote = true;
                }
                // Step eight: taken back where it was canceled, by its reader
                // or by the provider.
                if awaits && status == "canceled" {
                    take_back(r, owner)?;
                    wrote.lock().expect("written").wrote = true;
                }
                match r.order(owner)? {
                    Some(o) => Ok(ok(order_status(r, owner, o.status)?)),
                    None => Ok(declared("nothing-to-order")),
                }
            })
        }),
    );
    // **A reader's addresses, written** (track `store-accounts`, milestone
    // 2): add one (a label and a place from the table, chosen as it is
    // saved), rename one, remove one (the chosen one leaves none chosen), or
    // choose one. Each answers the reader's addresses, or why not.
    type AddressChange =
        fn(&mut Vec<AddressRow>, &[Val]) -> Result<Result<(), &'static str>, String>;
    let mut address = |name: &'static str, change: AddressChange| {
        let (w, written) = (with.clone(), written.clone());
        ops.insert(
            format!("store:data/addresses#{name}"),
            Arc::new(move |args: &[Val]| {
                let op = format!("addresses#{name}");
                let (reader, rest) = reader_of(&op, args)?;
                w(&mut |r| {
                    let mut rows = r.addresses(reader)?;
                    if let Err(why) = change(&mut rows, rest)? {
                        return Ok(declared(why));
                    }
                    r.set_addresses(reader, &rows)?;
                    written.lock().expect("written").wrote = true;
                    Ok(ok(addresses_val(&rows)))
                })
            }),
        );
    };
    address("add", |rows, args| {
        let [Val::String(label), Val::String(place)] = args else {
            return Err(format!("addresses#add received {args:?}"));
        };
        if crate::places::place(place).is_none() {
            return Ok(Err("unknown-place"));
        }
        if rows.len() >= MOST_ADDRESSES {
            return Ok(Err("too-many"));
        }
        let next = rows
            .iter()
            .filter_map(|a| a.id.strip_prefix("addr-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        for a in rows.iter_mut() {
            a.chosen = false;
        }
        rows.push(AddressRow {
            id: format!("addr-{next}"),
            label: label.clone(),
            place: place.clone(),
            chosen: true,
        });
        Ok(Ok(()))
    });
    address("rename", |rows, args| {
        let [Val::String(id), Val::String(label)] = args else {
            return Err(format!("addresses#rename received {args:?}"));
        };
        let Some(a) = rows.iter_mut().find(|a| a.id == *id) else {
            return Ok(Err("no-such-address"));
        };
        a.label = label.clone();
        Ok(Ok(()))
    });
    address("remove", |rows, args| {
        let [Val::String(id)] = args else {
            return Err(format!("addresses#remove received {args:?}"));
        };
        let before = rows.len();
        rows.retain(|a| a.id != *id);
        if rows.len() == before {
            return Ok(Err("no-such-address"));
        }
        Ok(Ok(()))
    });
    address("choose", |rows, args| {
        let [Val::String(id)] = args else {
            return Err(format!("addresses#choose received {args:?}"));
        };
        if !rows.iter().any(|a| a.id == *id) {
            return Ok(Err("no-such-address"));
        }
        for a in rows.iter_mut() {
            a.chosen = a.id == *id;
        }
        Ok(Ok(()))
    });
    // **Whether a store reaches the reader's chosen address, in a command**
    // (track `store-accounts`): what `add_to_cart` reads in its own
    // transaction, by the rule the estimate reads. True where none is chosen.
    let w = with.clone();
    ops.insert(
        "store:data/coverage#reaches".to_string(),
        Arc::new(move |args: &[Val]| {
            let [Val::String(store), Val::String(reader)] = args else {
                return Err(format!("coverage#reaches received {args:?}"));
            };
            w(&mut |r| {
                Ok(vec![Val::Bool(
                    coverage(r, store, reader)?.is_none_or(|c| c.delivers),
                )])
            })
        }),
    );
    // Whether an item can be ordered now (charter §15.4), read inside the
    // command that adds it. One no store's menu has is not (ADR-0172): a page
    // sends the item it showed, and a request can name any.
    let w = with.clone();
    ops.insert(
        "store:data/menus#is-available".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(item)] => w(&mut |r| {
                Ok(vec![Val::Bool(
                    r.catalog_item(item)?.is_some_and(|(_, _, can)| can),
                )])
            }),
            other => Err(format!("menus#is-available received {other:?}")),
        }),
    );
    ops = faulted(&faults.cart_faults, &faults.delays_taken, session, ops);
    // TRACK SEAM (store-hours): a store's hours, the host's own write.
    ops.extend(crate::hours::host_writes(with.clone(), written.clone()));
    // TRACK SEAM (order-total): the checkout's writes, and the host's.
    ops.extend(crate::checkout::writes(with.clone(), written.clone()));
    ops.extend(crate::checkout::host_writes(with.clone(), written.clone()));
    // TRACK SEAM (reviews): a rating given.
    ops.extend(crate::reviews::writes(with.clone(), written.clone()));
    // TRACK SEAM (scheduled-orders): the time chosen, and an order sent.
    ops.extend(crate::scheduled_orders::writes(
        with.clone(),
        written.clone(),
    ));
    ops.extend(host_writes(with, session, written));
    ops
}

/// **The host's own writes** (track `store-pg`): what a route that changes
/// the store's data stages, in the command's transaction, before the host
/// commits it with the route's declared events. Each answers `Ok`, with a
/// value where it has one, or `Err` with why, and then nothing it staged
/// commits.
fn host_writes(with: With, session: &str, written: Arc<Mutex<Written>>) -> Ops {
    type Write = fn(&mut dyn Rows, &str, &[Val]) -> Result<Result<Option<Val>, String>, String>;
    let mut ops: Ops = BTreeMap::new();
    let mut write = |name: &'static str, f: Write| {
        let (this, w, written) = (session.to_string(), with.clone(), written.clone());
        ops.insert(
            format!("{HOST_OPS}{name}"),
            Arc::new(move |args: &[Val]| {
                w(&mut |r| match f(r, &this, args)? {
                    Ok(answer) => {
                        // A write that changed nothing (`menus#replace` to
                        // the size it is) answers `false`, and commits none.
                        if answer != Some(Val::Bool(false)) {
                            written.lock().expect("written").wrote = true;
                        }
                        Ok(done(answer))
                    }
                    Err(why) => Ok(refused(why)),
                })
            }),
        );
    };
    // **A change to a store's menu** (E7-P): `MenuOp`'s, applied as it
    // always was, to the menu as the layer holds it. An item that stays
    // keeps what it was; one inserted is the catalogue's. A refusal changes
    // nothing.
    write("menus#change", |r, _, args| {
        let [Val::String(store), op @ ..] = args else {
            return Err(format!("menus#change received {args:?}"));
        };
        let op = MenuOp::from_vals(op)?;
        let menu = r.menu(store)?;
        let mut pairs: Vec<(String, String)> = menu
            .iter()
            .map(|i| (i.id.clone(), i.name.clone()))
            .collect();
        if let Err(why) = op.apply(&mut pairs) {
            return Ok(Err(why));
        }
        let items: Vec<Item> = pairs
            .iter()
            .map(|(id, name)| match menu.iter().find(|i| i.id == *id) {
                Some(item) => Item {
                    name: name.clone(),
                    ..item.clone()
                },
                None => Item::new(store, id, name),
            })
            .collect();
        r.set_menu(store, &items)?;
        // Whether it can be ordered (ADR-0178), in every store that sells it.
        if let MenuOp::Stock { id, available } = &op {
            r.stock(id, *available)?;
        }
        Ok(Ok(None))
    });
    // An item sold out at the source, or back, untold (charter §15.5's
    // forced stale item).
    write("menus#stock", |r, _, args| match args {
        [Val::String(item), Val::Bool(available)] => {
            r.stock(item, *available)?;
            Ok(Ok(None))
        }
        other => Err(format!("menus#stock received {other:?}")),
    });
    // **A store's menu, `n` items long** (E7 gate item 10's large menu):
    // `item-0` on. One that long already changes nothing.
    write("menus#replace", |r, _, args| match args {
        [Val::String(store), Val::S64(n)] => {
            if r.menu(store)?.len() as i64 == *n {
                return Ok(Ok(Some(Val::Bool(false))));
            }
            let items: Vec<Item> = (0..*n)
                .map(|i| Item::new(store, &format!("item-{i}"), &format!("Item {i}")))
                .collect();
            r.set_menu(store, &items)?;
            Ok(Ok(Some(Val::Bool(true))))
        }
        other => Err(format!("menus#replace received {other:?}")),
    });
    // **The kitchen moves an owner's order along** (E14, T04; ADR-0193),
    // or removes it: the owner the program reads its order by, the user's id
    // or the session (track `store-accounts`), as the host's route names it.
    write("orders#set-status", |r, _, args| match args {
        [Val::String(owner), Val::Option(status)] if !owner.is_empty() => {
            let status = match status.as_deref() {
                Some(Val::String(s)) => Some(s.as_str()),
                None => None,
                Some(other) => return Err(format!("orders#set-status received {other:?}")),
            };
            r.set_status(owner, status)?;
            Ok(Ok(None))
        }
        other => Err(format!("orders#set-status received {other:?}")),
    });
    // **A guest's cart joins its user's at sign-in** (track `store-accounts`,
    // ADR-0318; docs/PARALLEL.md, Q2): each of the guest's lines added to the
    // user's, the quantities of one item summed, a sum past what a line can
    // hold (a bigint) refused rather than wrapped; the guest's cart emptied.
    // `false` where the guest has nothing to join, and nothing commits: run
    // again, a join finds nothing.
    write("carts#join", |r, _, args| match args {
        [Val::String(guest), Val::String(user)] if !guest.is_empty() && !user.is_empty() => {
            if guest == user {
                return Ok(Ok(Some(Val::Bool(false))));
            }
            let joining = r.cart(guest)?;
            if joining.is_empty() {
                return Ok(Ok(Some(Val::Bool(false))));
            }
            let mut lines = r.cart(user)?;
            if let Err(why) = joined(&mut lines, &joining) {
                return Ok(Err(why));
            }
            r.set_cart(user, &lines)?;
            r.set_cart(guest, &[])?;
            Ok(Ok(Some(Val::Bool(true))))
        }
        other => Err(format!("carts#join received {other:?}")),
    });
    // The store's staff post a notice (E14, T09).
    write("notices#post", |r, _, args| match args {
        [Val::String(store), Val::String(text)] => {
            r.set_notice(store, text)?;
            Ok(Ok(None))
        }
        other => Err(format!("notices#post received {other:?}")),
    });
    // The kitchen's prep time changes (E14, T02).
    write("kitchen#set-prep", |r, _, args| match args {
        [Val::String(store), Val::S64(minutes)] => {
            r.set_prep(store, *minutes)?;
            Ok(Ok(None))
        }
        other => Err(format!("kitchen#set-prep received {other:?}")),
    });
    // What the recommender suggests (ADR-0148), curated, or drawn from each
    // store's menu again.
    write("recommendations#curate", |r, _, args| match args {
        [Val::Option(items)] => {
            let items = match items.as_deref() {
                None => None,
                Some(Val::List(items)) => Some(
                    items
                        .iter()
                        .map(|i| match (val_at(i, &["id"]), val_at(i, &["name"])) {
                            (Some(Val::String(id)), Some(Val::String(name))) => {
                                Ok((id.clone(), name.clone()))
                            }
                            _ => Err(format!("recommendations#curate received {i:?}")),
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                ),
                Some(other) => return Err(format!("recommendations#curate received {other:?}")),
            };
            r.curate(items.as_deref())?;
            Ok(Ok(None))
        }
        other => Err(format!("recommendations#curate received {other:?}")),
    });
    // What the session's estimator estimates (E14, T10; ADR-0180).
    write("estimates#set", |r, this, args| {
        match own_session("estimates#set", this, args)? {
            [Val::S64(minutes), Val::Option(max)] => {
                let max = match max.as_deref() {
                    Some(Val::S64(m)) => Some(*m),
                    None => None,
                    Some(other) => return Err(format!("estimates#set received {other:?}")),
                };
                r.set_estimate(this, (*minutes, max))?;
                Ok(Ok(None))
            }
            other => Err(format!("estimates#set received {other:?}")),
        }
    });
    ops
}

/// **A guest's lines added to a user's** (track `store-accounts`, ADR-0318):
/// in the guest's order, each item's quantities summed on the user's line
/// that holds it, or the guest's line appended as it recorded the item
/// (ADR-0172). A sum is a `PositiveInt` (ADR-0179), at least one, and a
/// line holds at most what a bigint does: past it the join is refused, and
/// nothing moves.
pub(crate) fn joined(lines: &mut Lines, joining: &[Line]) -> Result<(), String> {
    let mut out = lines.clone();
    for line in joining {
        match out.iter_mut().find(|l| l.item == line.item) {
            Some(held) => {
                held.quantity = held.quantity.checked_add(line.quantity).ok_or_else(|| {
                    format!(
                        "the cart cannot hold {} and {} more of `{}`",
                        held.quantity, line.quantity, line.item
                    )
                })?;
            }
            None => out.push(line.clone()),
        }
    }
    *lines = out;
    Ok(())
}

impl MenuOp {
    /// **The change as a host operation is passed it** (track `store-pg`).
    pub(crate) fn to_vals(&self) -> Vec<Val> {
        let s = |v: &str| Val::String(v.to_string());
        let opt = |v: &Option<String>| Val::Option(v.as_deref().map(|v| Box::new(s(v))));
        match self {
            MenuOp::Insert {
                id,
                name,
                at,
                before,
            } => vec![s("insert"), s(id), s(name), opt(at), Val::Bool(*before)],
            MenuOp::Remove { id } => vec![s("remove"), s(id)],
            MenuOp::Move { id, after } => vec![s("move"), s(id), opt(after)],
            MenuOp::Rename { id, name } => vec![s("rename"), s(id), s(name)],
            MenuOp::Stock { id, available } => vec![s("stock"), s(id), Val::Bool(*available)],
        }
    }

    /// [`MenuOp::to_vals`], read back.
    fn from_vals(vals: &[Val]) -> Result<MenuOp, String> {
        let opt = |v: &Val| match v {
            Val::Option(None) => Ok(None),
            Val::Option(Some(v)) => match v.as_ref() {
                Val::String(s) => Ok(Some(s.clone())),
                other => Err(format!("{other:?}")),
            },
            other => Err(format!("{other:?}")),
        };
        let bad = || format!("menus#change received {vals:?}");
        Ok(match vals {
            [
                Val::String(k),
                Val::String(id),
                Val::String(name),
                at,
                Val::Bool(before),
            ] if k == "insert" => MenuOp::Insert {
                id: id.clone(),
                name: name.clone(),
                at: opt(at).map_err(|_| bad())?,
                before: *before,
            },
            [Val::String(k), Val::String(id)] if k == "remove" => MenuOp::Remove { id: id.clone() },
            [Val::String(k), Val::String(id), after] if k == "move" => MenuOp::Move {
                id: id.clone(),
                after: opt(after).map_err(|_| bad())?,
            },
            [Val::String(k), Val::String(id), Val::String(name)] if k == "rename" => {
                MenuOp::Rename {
                    id: id.clone(),
                    name: name.clone(),
                }
            }
            [Val::String(k), Val::String(id), Val::Bool(available)] if k == "stock" => {
                MenuOp::Stock {
                    id: id.clone(),
                    available: *available,
                }
            }
            _ => return Err(bad()),
        })
    }
}

/// **What a node grants the store's data layer** (ADR-0218), beyond the
/// platform's session and outbox. The host's own writes (`store:host/…`)
/// are granted to no program.
pub(crate) fn grants() -> &'static [&'static str] {
    &[
        "database.read<Carts>",
        "database.read<Menus>",
        // A session's order (E14, T04).
        "database.read<Orders>",
        // And the order a session places (ADR-0193).
        "database.write<Orders>",
        "database.read<Stores>",
        "database.write<Carts>",
        // The recommender (ADR-0148), a source reached over the network.
        "network.fetch",
        // A session's delivery estimate (E14, T10).
        "database.read<Estimates>",
        // A reader's addresses (track `store-accounts`).
        "database.read<Addresses>",
        "database.write<Addresses>",
        // The store's notice board (E14, T09).
        "database.read<Notices>",
        // The kitchen's prep time (E14, T02).
        "database.read<Kitchen>",
        // The menu by category (E14, T07).
        "database.read<Categories>",
        // TRACK SEAM (reviews): a store's ratings, read and given.
        "database.read<Ratings>",
        "database.write<Ratings>",
    ]
}

/// **The store's data, as it is first** (ADR-0125, ADR-0162, ADR-0192): two
/// stores, 47, whose menu E7-P changes, and 48; the notice the store has
/// posted; how long its kitchen takes; nothing curated, no cart, no order
/// and no estimate. `migrations/store/0002_seed.sql` is the same rows, and
/// a test holds the two layers to answer alike.
#[derive(Debug, Clone)]
pub(crate) struct State {
    stores: Vec<StoreRow>,
    menus: BTreeMap<String, Vec<Item>>,
    orders: BTreeMap<String, Order>,
    notices: BTreeMap<String, String>,
    prep: BTreeMap<String, i64>,
    curated: Option<Vec<(String, String)>>,
    estimates: BTreeMap<String, Estimate>,
    /// Each owner's saved addresses (track `store-accounts`).
    addresses: BTreeMap<String, Vec<AddressRow>>,
    /// TRACK SEAM (store-hours): each store's opening hours (`hours.rs`).
    hours: BTreeMap<String, crate::hours::Hours>,
    /// TRACK SEAM (order-total): each store's fees, the codes, each code's
    /// readers, each reader's choices, and each owner's order as charged.
    fees: BTreeMap<String, crate::checkout::Fees>,
    promos: BTreeMap<String, crate::checkout::Promo>,
    promo_uses: std::collections::BTreeSet<(String, String)>,
    choices: BTreeMap<String, crate::checkout::Chosen>,
    charged: BTreeMap<String, crate::checkout::Charged>,
    /// NEXT 6c: the payment each owner's order awaits.
    awaiting: BTreeMap<String, crate::payments::Awaiting>,
    /// TRACK SEAM (reviews): every rating given, oldest first, and how many
    /// orders were made, the last one's id.
    ratings: Vec<crate::reviews::Rating>,
    orders_made: i64,
    /// TRACK SEAM (scheduled-orders): each owner's order's time.
    times: BTreeMap<String, crate::scheduled_orders::OrderTime>,
}

impl State {
    fn seed() -> State {
        let store = |id: &str| StoreRow {
            id: id.to_string(),
            name: store_named(id).unwrap_or_default().to_string(),
            description: store_description(id).to_string(),
            cuisine: crate::search::cuisine_of(id).to_string(),
            opens_minute: 7 * 60,
            closes_minute: 19 * 60,
            zone: zone_of(id),
        };
        let menu = |id: &str, items: Vec<(String, String)>| {
            (
                id.to_string(),
                items
                    .iter()
                    .map(|(item, name)| Item::new(id, item, name))
                    .collect(),
            )
        };
        State {
            stores: vec![store(STORE_ID), store(SECOND_STORE.0)],
            menus: BTreeMap::from([
                menu(STORE_ID, default_menu()),
                menu(SECOND_STORE.0, second_menu()),
            ]),
            orders: BTreeMap::new(),
            notices: BTreeMap::from([(STORE_ID.to_string(), "Open until 7 pm".to_string())]),
            prep: BTreeMap::from([(STORE_ID.to_string(), 12)]),
            curated: None,
            estimates: BTreeMap::new(),
            addresses: BTreeMap::new(),
            // TRACK SEAM (store-hours): each store's, as it is first.
            hours: [STORE_ID, SECOND_STORE.0]
                .map(|id| (id.to_string(), crate::hours::seed()))
                .into(),
            // TRACK SEAM (order-total): as `0013_order_total.sql` seeds them.
            fees: [STORE_ID, SECOND_STORE.0]
                .map(|id| (id.to_string(), crate::checkout::seed_fees()))
                .into(),
            promos: crate::checkout::seed_promos()
                .into_iter()
                .map(|p| (p.code.clone(), p))
                .collect(),
            promo_uses: Default::default(),
            choices: BTreeMap::new(),
            charged: BTreeMap::new(),
            awaiting: BTreeMap::new(),
            // TRACK SEAM (reviews): none rated, and no order made.
            ratings: Vec::new(),
            orders_made: 0,
            times: BTreeMap::new(),
        }
    }

    /// TRACK SEAM (reviews): **the next order's id**, one more than the
    /// last made, as PostgreSQL's serial gives one.
    fn next_order(&mut self) -> i64 {
        self.orders_made += 1;
        self.orders_made
    }
}

/// **The in-memory layer's rows** (track `store-pg`): the carts, and the
/// rest of the store, as a read saw them or a command stages them. A
/// command's are committed by [`Staging`]'s `publish`. Track
/// `store-accounts`: every owner's cart, as last committed, and the carts the
/// command wrote, so that one command may write two owners' (a guest's cart
/// joining its user's).
struct MemRows {
    carts: Arc<BTreeMap<String, Lines>>,
    staged: BTreeMap<String, Lines>,
    state: Arc<State>,
}

impl MemRows {
    fn state(&mut self) -> &mut State {
        Arc::make_mut(&mut self.state)
    }
}

impl Rows for MemRows {
    fn stores(&mut self) -> Result<Vec<StoreRow>, String> {
        Ok(self.state.stores.clone())
    }
    fn menu(&mut self, store: &str) -> Result<Vec<Item>, String> {
        Ok(self.state.menus.get(store).cloned().unwrap_or_default())
    }
    fn set_menu(&mut self, store: &str, items: &[Item]) -> Result<(), String> {
        self.state().menus.insert(store.to_string(), items.to_vec());
        Ok(())
    }
    fn stock(&mut self, item: &str, available: bool) -> Result<(), String> {
        for menu in self.state().menus.values_mut() {
            for i in menu.iter_mut().filter(|i| i.id == item) {
                i.available = available;
            }
        }
        Ok(())
    }
    fn catalog_item(&mut self, item: &str) -> Result<Option<(String, i64, bool)>, String> {
        let mut found = None;
        let mut can = false;
        for store in &self.state.stores {
            for i in self.state.menus.get(&store.id).into_iter().flatten() {
                if i.id == item {
                    found = Some((i.name.clone(), i.price));
                    can |= i.available;
                }
            }
        }
        Ok(found.map(|(name, price)| (name, price, can)))
    }
    fn cart(&mut self, owner: &str) -> Result<Lines, String> {
        Ok(self
            .staged
            .get(owner)
            .or_else(|| self.carts.get(owner))
            .cloned()
            .unwrap_or_default())
    }
    fn set_cart(&mut self, owner: &str, lines: &[Line]) -> Result<(), String> {
        self.staged.insert(owner.to_string(), lines.to_vec());
        Ok(())
    }
    fn order(&mut self, session: &str) -> Result<Option<Order>, String> {
        Ok(self.state.orders.get(session).cloned())
    }
    fn addresses(&mut self, owner: &str) -> Result<Vec<AddressRow>, String> {
        Ok(self.state.addresses.get(owner).cloned().unwrap_or_default())
    }
    fn set_addresses(&mut self, owner: &str, rows: &[AddressRow]) -> Result<(), String> {
        self.state()
            .addresses
            .insert(owner.to_string(), rows.to_vec());
        Ok(())
    }
    fn place(&mut self, session: &str, lines: &[Line]) -> Result<(), String> {
        let order = Order {
            // TRACK SEAM (reviews): a new order, a new id.
            id: self.state().next_order(),
            status: "placed".to_string(),
            lines: lines.to_vec(),
        };
        self.state().orders.insert(session.to_string(), order);
        Ok(())
    }
    fn set_status(&mut self, session: &str, status: Option<&str>) -> Result<(), String> {
        // TRACK SEAM (reviews): an order the kitchen sets with none placed is
        // made, with an id of its own.
        if status.is_some() && !self.state.orders.contains_key(session) {
            let id = self.state().next_order();
            self.state().orders.insert(
                session.to_string(),
                Order {
                    id,
                    status: String::new(),
                    lines: Lines::new(),
                },
            );
        }
        let orders = &mut self.state().orders;
        match status {
            Some(status) => {
                if let Some(order) = orders.get_mut(session) {
                    order.status = status.to_string();
                }
            }
            None => {
                orders.remove(session);
                // TRACK SEAM (order-total): its charges with it, as
                // PostgreSQL's go with the order's row.
                self.state().charged.remove(session);
                // NEXT 6c: and the payment it awaited.
                self.state().awaiting.remove(session);
            }
        }
        Ok(())
    }
    fn awaiting(&mut self, owner: &str) -> Result<Option<crate::payments::Awaiting>, String> {
        Ok(self.state.awaiting.get(owner).cloned())
    }
    fn set_awaiting(
        &mut self,
        owner: &str,
        awaiting: Option<&crate::payments::Awaiting>,
    ) -> Result<(), String> {
        match awaiting {
            Some(a) => {
                self.state().awaiting.insert(owner.to_string(), a.clone());
            }
            None => {
                self.state().awaiting.remove(owner);
            }
        }
        Ok(())
    }
    fn notice(&mut self, store: &str) -> Result<Option<String>, String> {
        Ok(self.state.notices.get(store).cloned())
    }
    fn set_notice(&mut self, store: &str, text: &str) -> Result<(), String> {
        self.state()
            .notices
            .insert(store.to_string(), text.to_string());
        Ok(())
    }
    fn prep(&mut self, store: &str) -> Result<Option<i64>, String> {
        Ok(self.state.prep.get(store).copied())
    }
    fn set_prep(&mut self, store: &str, minutes: i64) -> Result<(), String> {
        self.state().prep.insert(store.to_string(), minutes);
        Ok(())
    }
    fn curated(&mut self) -> Result<Option<Vec<(String, String)>>, String> {
        Ok(self.state.curated.clone())
    }
    fn curate(&mut self, items: Option<&[(String, String)]>) -> Result<(), String> {
        self.state().curated = items.map(<[_]>::to_vec);
        Ok(())
    }
    fn estimate(&mut self, session: &str) -> Result<Option<Estimate>, String> {
        Ok(self.state.estimates.get(session).copied())
    }
    fn set_estimate(&mut self, session: &str, estimate: Estimate) -> Result<(), String> {
        self.state().estimates.insert(session.to_string(), estimate);
        Ok(())
    }
    // TRACK SEAM (store-hours): a store's hours, in memory.
    fn hours(&mut self, store: &str) -> Result<Option<crate::hours::Hours>, String> {
        Ok(self.state.hours.get(store).cloned())
    }
    fn set_hours(&mut self, store: &str, hours: &crate::hours::Hours) -> Result<(), String> {
        let held = crate::hours::held(hours);
        self.state().hours.insert(store.to_string(), held);
        Ok(())
    }
    // TRACK SEAM (order-total): the checkout's rows, in memory.
    fn fees(&mut self, stores: &[String]) -> Result<Vec<(String, crate::checkout::Fees)>, String> {
        Ok(stores
            .iter()
            .filter_map(|s| self.state.fees.get(s).map(|f| (s.clone(), f.clone())))
            .collect())
    }
    fn set_fees(&mut self, store: &str, fees: &crate::checkout::Fees) -> Result<(), String> {
        self.state().fees.insert(store.to_string(), fees.clone());
        Ok(())
    }
    fn promo(
        &mut self,
        owner: &str,
        code: &str,
    ) -> Result<Option<(crate::checkout::Promo, bool)>, String> {
        let used = self
            .state
            .promo_uses
            .contains(&(code.to_string(), owner.to_string()));
        Ok(self.state.promos.get(code).map(|p| (p.clone(), used)))
    }
    fn set_promo(&mut self, promo: &crate::checkout::Promo) -> Result<(), String> {
        self.state()
            .promos
            .insert(promo.code.clone(), promo.clone());
        Ok(())
    }
    fn use_promo(&mut self, owner: &str, code: &str) -> Result<bool, String> {
        Ok(self
            .state()
            .promo_uses
            .insert((code.to_string(), owner.to_string())))
    }
    fn release_promo(&mut self, owner: &str, code: &str) -> Result<(), String> {
        self.state()
            .promo_uses
            .remove(&(code.to_string(), owner.to_string()));
        Ok(())
    }
    fn chosen(&mut self, owner: &str) -> Result<crate::checkout::Chosen, String> {
        Ok(self.state.choices.get(owner).cloned().unwrap_or_default())
    }
    fn set_chosen(&mut self, owner: &str, chosen: &crate::checkout::Chosen) -> Result<(), String> {
        self.state()
            .choices
            .insert(owner.to_string(), chosen.clone());
        Ok(())
    }
    fn charge(&mut self, owner: &str, charged: &crate::checkout::Charged) -> Result<(), String> {
        self.state()
            .charged
            .insert(owner.to_string(), charged.clone());
        Ok(())
    }
    fn charged(&mut self, owner: &str) -> Result<Option<crate::checkout::Charged>, String> {
        Ok(self.state.charged.get(owner).cloned())
    }
    // TRACK SEAM (reviews): a store's ratings, in memory.
    fn ratings_of(&mut self, stores: &[String]) -> Result<Vec<(String, i64, i64)>, String> {
        Ok(crate::reviews::mem_ratings_of(&self.state.ratings, stores))
    }
    fn reviews(&mut self, store: &str, most: usize) -> Result<Vec<(i64, i64, String)>, String> {
        Ok(crate::reviews::mem_reviews(
            &self.state.ratings,
            store,
            most,
        ))
    }
    fn to_rate(&mut self, owner: &str) -> Result<Option<crate::reviews::ToRate>, String> {
        Ok(crate::reviews::mem_to_rate(
            self.state.orders.get(owner),
            self.state.charged.get(owner),
            &self.state.stores,
            &self.state.ratings,
        ))
    }
    fn rate(&mut self, rating: &crate::reviews::Rating) -> Result<bool, String> {
        Ok(crate::reviews::mem_rate(&mut self.state().ratings, rating))
    }
    // TRACK SEAM (scheduled-orders): an order's time, in memory.
    fn order_time(
        &mut self,
        owner: &str,
    ) -> Result<Option<crate::scheduled_orders::OrderTime>, String> {
        Ok(self.state.times.get(owner).cloned())
    }
    fn set_order_time(
        &mut self,
        owner: &str,
        time: &crate::scheduled_orders::OrderTime,
    ) -> Result<(), String> {
        self.state().times.insert(owner.to_string(), time.clone());
        Ok(())
    }
}

/// `with` over in-memory rows.
fn with_mem(rows: Arc<Mutex<MemRows>>) -> With {
    Arc::new(move |f| f(&mut *rows.lock().expect("rows")))
}

/// **The store's data, in memory** (ADR-0218): the default layer, and the
/// one the browser suite runs on.
pub(crate) struct StoreData {
    faults: Faults,
    /// Each owner's cart lines (track `store-accounts`: a user's, or a
    /// session's for a program that keys its cart by the session), behind
    /// the materializer's command boundary. This is the deployment's DATA
    /// LAYER: `store:data/user-carts#add` is its operation, and the compiled
    /// command calls it through the host. Its lock is held from a command's
    /// first operation to its commit: one command at a time. Behind an
    /// `Arc`, so a read or a command takes every cart for the price of one.
    carts: Mutex<Arc<BTreeMap<String, Lines>>>,
    /// **The rest of the store** (track `store-pg`): its stores and menus,
    /// the orders, the notice, the kitchen, the recommender's curation and
    /// the estimates. Written by a command's commit alone.
    state: Mutex<Arc<State>>,
}

impl StoreData {
    /// **The page a document with none recorded is** (ADR-0218): the store's
    /// own, which its first documents and tests were served as.
    pub(crate) fn default_page() -> &'static str {
        "store.page.StorePage~ui"
    }

    pub(crate) fn new() -> StoreData {
        StoreData {
            faults: Faults::new(),
            carts: Mutex::new(Arc::new(BTreeMap::new())),
            state: Mutex::new(Arc::new(State::seed())),
        }
    }

    /// **A command's writes, staged until its transaction commits**
    /// (ADR-0218): the session's cart and the rest of the store, read and
    /// written under the carts' lock, which the staging holds until it is
    /// dropped. `fail` makes the data layer answer `cart-expired` (a test's).
    pub(crate) fn begin(&self, session: &str, fail: bool) -> Staging<'_> {
        let carts = self.carts.lock().expect("carts");
        let rows = Arc::new(Mutex::new(MemRows {
            carts: carts.clone(),
            staged: BTreeMap::new(),
            state: self.state.lock().expect("state").clone(),
        }));
        let written: Arc<Mutex<Written>> = Arc::default();
        let ops = writes(
            with_mem(rows.clone()),
            session,
            &self.faults,
            fail,
            written.clone(),
        );
        Staging {
            store: self,
            session: session.to_string(),
            carts,
            rows,
            written,
            ops,
        }
    }
}

/// **One command's writes, staged until its transaction commits** (ADR-0218).
pub(crate) struct Staging<'a> {
    store: &'a StoreData,
    session: String,
    /// Held from the call until the commit is published.
    carts: std::sync::MutexGuard<'a, Arc<BTreeMap<String, Lines>>>,
    rows: Arc<Mutex<MemRows>>,
    written: Arc<Mutex<Written>>,
    ops: Ops,
}

impl crate::data::Staged for Staging<'_> {
    fn projection_scope(&self) -> crate::data::ProjectionScope {
        crate::data::ProjectionScope::Session
    }
    fn ops(&self) -> Ops {
        self.ops.clone()
    }

    fn rows(&self) -> Option<Vec<(String, String)>> {
        self.written.lock().expect("written").rows(&self.session)
    }

    /// **After the commit, what was staged is the store's**: the cart, and
    /// the rest of the store it wrote, before what reads them is dropped and
    /// read again.
    fn publish(&mut self) {
        let written = self.written.lock().expect("written");
        if !written.wrote {
            return;
        }
        let rows = self.rows.lock().expect("rows");
        if !rows.staged.is_empty() {
            let carts = Arc::make_mut(&mut self.carts);
            for (owner, lines) in &rows.staged {
                carts.insert(owner.clone(), lines.clone());
            }
        }
        *self.store.state.lock().expect("state") = rows.state.clone();
    }
}

impl crate::data::DataLayer for StoreData {
    fn reads(&self, session: &str, stopped: Option<Stopped>) -> Ops {
        // What a query reads is the store as it is now: the session's cart,
        // and the rest, as their last commit left them.
        let rows = MemRows {
            carts: self.carts.lock().expect("carts").clone(),
            staged: BTreeMap::new(),
            state: self.state.lock().expect("state").clone(),
        };
        reads(
            with_mem(Arc::new(Mutex::new(rows))),
            session,
            &self.faults,
            stopped,
        )
    }

    // TRACK SEAM (render-merge): a render's snapshot, the store as its last
    // commit left it when the render began, each part behind its `Arc`, so
    // nothing is copied; and each of the render's queries' reads over it
    // (NEXT 4c).
    fn snapshot(&self) -> Option<crate::data::Snapshot> {
        let rows = MemRows {
            carts: self.carts.lock().expect("carts").clone(),
            staged: BTreeMap::new(),
            state: self.state.lock().expect("state").clone(),
        };
        Some(Box::new(Arc::new(Mutex::new(rows))))
    }

    fn reads_in(
        &self,
        snapshot: &crate::data::Snapshot,
        session: &str,
        stopped: Option<Stopped>,
    ) -> Ops {
        match snapshot.downcast_ref::<Arc<Mutex<MemRows>>>() {
            Some(rows) => reads(with_mem(rows.clone()), session, &self.faults, stopped),
            None => self.reads(session, stopped),
        }
    }

    fn begin<'a>(&'a self, session: &str) -> Box<dyn crate::data::Staged + 'a> {
        let fail = self.faults.fail_next.swap(false, Ordering::SeqCst);
        Box::new(StoreData::begin(self, session, fail))
    }

    fn grants(&self) -> Vec<&'static str> {
        grants().to_vec()
    }

    fn default_page(&self) -> Option<&'static str> {
        Some(StoreData::default_page())
    }

    fn session_entry(&self) -> bool {
        true
    }

    fn style(&self) -> &'static str {
        crate::STYLE
    }
}

impl StoreLayer for StoreData {
    fn faults(&self) -> &Faults {
        &self.faults
    }

    #[cfg(test)]
    fn line_around(&self, owner: &str, at: usize, quantity: i64) -> Result<(), String> {
        let mut carts = self.carts.lock().expect("carts");
        let line = Arc::make_mut(&mut carts)
            .get_mut(owner)
            .and_then(|lines| lines.get_mut(at))
            .ok_or("no such line")?;
        line.quantity = quantity;
        Ok(())
    }
}

#[cfg(test)]
mod joins {
    use super::*;

    fn line(item: &str, quantity: i64) -> Line {
        Line {
            item: item.to_string(),
            quantity,
            name: format!("Name of {item}"),
            price: 100,
        }
    }

    /// **A guest's lines added to a user's** (track `store-accounts`): one
    /// item's quantities summed on the user's line, a new item appended as
    /// the guest's line recorded it, in the guest's order.
    #[test]
    fn a_guests_lines_are_summed_into_the_users_or_appended() {
        let mut lines = vec![line("espresso", 1), line("latte", 2)];
        let guest = [line("cortado", 1), line("espresso", 2)];
        joined(&mut lines, &guest).expect("joined");
        assert_eq!(
            lines,
            [line("espresso", 3), line("latte", 2), line("cortado", 1)]
        );
    }

    /// **A sum past a bigint is refused, and nothing moves**: not wrapped,
    /// and not half joined.
    #[test]
    fn a_sum_past_a_bigint_is_refused_and_nothing_moves() {
        let mut lines = vec![line("latte", 1), line("espresso", i64::MAX)];
        let guest = [line("cortado", 1), line("espresso", 1)];
        let why = joined(&mut lines, &guest).expect_err("refused");
        assert!(why.contains("espresso"), "{why}");
        assert_eq!(lines, [line("latte", 1), line("espresso", i64::MAX)]);
    }
}
