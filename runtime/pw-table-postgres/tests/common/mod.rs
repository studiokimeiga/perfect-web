//! What the PostgreSQL adapter's tests share: the database a test may use (a
//! throwaway one: each test makes a schema of its own and drops it), the
//! programs they serve, as the compiler builds them, and the reference
//! adapter's answers to compare with.
//!
//! Without a database (`PW_TABLES_DATABASE_URL`, else `PW_STORE_DATABASE_URL`
//! or `PW_FEED_DATABASE_URL`, as `just e14-d3-postgres` and CI's database
//! shard set them) each test passes doing nothing, as every PostgreSQL test
//! of the repository does (ADR-0246); the recipe and the mutation script
//! refuse to run without one.
#![allow(dead_code)]

use crate::assert_result::*;

use std::sync::atomic::{AtomicUsize, Ordering};

use pw_core::check::Unit;
use pw_core::tables::{Schema, TablesJson};
use serde_json::{Value, json};

/// The throwaway database the tests may use, if one is named.
pub fn database() -> Option<String> {
    [
        "PW_TABLES_DATABASE_URL",
        "PW_STORE_DATABASE_URL",
        "PW_FEED_DATABASE_URL",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

static SCRATCHES: AtomicUsize = AtomicUsize::new(0);

/// **A schema of one test's own**, dropped with everything in it when the
/// test is done, whatever happened.
pub struct Scratch {
    pub url: String,
    pub schema: String,
}

impl Scratch {
    pub fn new(url: &str, what: &str) -> Scratch {
        let n = SCRATCHES.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Scratch {
            url: url.to_string(),
            schema: format!("pw_d3_{what}_{}_{n}_{nanos}", std::process::id()),
        }
    }

    /// Another schema's name for the same database (two programs' tables).
    pub fn schema(&self) -> Option<&str> {
        Some(&self.schema)
    }

    /// `statements`, run as one batch on a connection of its own, in this
    /// schema: what another writer of the database does.
    pub fn sql(&self, statements: &str) -> Result<(), String> {
        let mut c =
            postgres::Client::connect(&self.url, postgres::NoTls).map_err(|e| e.to_string())?;
        c.batch_execute(&format!(
            "SET search_path TO \"{}\"; {statements}",
            self.schema
        ))
        .map_err(|e| match e.as_db_error() {
            Some(db) => format!("{}: {}", db.code().code(), db.message()),
            None => e.to_string(),
        })
    }

    /// The first column of each row `query` answers, as text.
    pub fn texts(&self, query: &str) -> Vec<String> {
        let mut c = postgres::Client::connect(&self.url, postgres::NoTls).expect("a connection");
        c.batch_execute(&format!("SET search_path TO \"{}\"", self.schema))
            .expect("the schema");
        c.query(query, &[])
            .expect("the query")
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!(
                "DROP SCHEMA IF EXISTS \"{}\" CASCADE",
                self.schema
            ));
        }
    }
}

fn units(sources: &[(&str, &str)], fixtures: &[&str]) -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files: Vec<(String, String)> = Vec::new();
    for directory in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(directory))
            .expect("platform sources")
            .map(|entry| entry.expect("source entry").path())
            .filter(|path| path.extension().is_some_and(|e| e == "pw"))
            .collect();
        paths.sort();
        for path in paths {
            files.push((
                path.display().to_string(),
                std::fs::read_to_string(&path).expect("platform source"),
            ));
        }
    }
    for name in fixtures {
        let path = root.join("examples/tables").join(name);
        files.push((
            path.display().to_string(),
            std::fs::read_to_string(&path).expect("fixture source"),
        ));
    }
    for (path, src) in sources {
        files.push((path.to_string(), src.to_string()));
    }
    files
        .into_iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(&src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            let hir = pw_core::lower::lower_file(&src, &parsed.green);
            Unit { path, src, hir }
        })
        .collect()
}

/// `examples/tables`' fixtures, with the platform's sources, as units.
pub fn fixture_units(fixtures: &[&str]) -> Vec<Unit> {
    units(&[], fixtures)
}

/// **The program `sources` declare, as `pw build` builds it** against the
/// schema `released` (its `schema.lock`), or none: its build.
pub fn build(sources: &[(&str, &str)], released: Option<&Schema>) -> pw_core::build::Build {
    let build =
        pw_core::build::build_with(&units(sources, &[]), released).assert_ok("the program builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    build
}

/// tables.json of the program `sources` declare.
pub fn tables(sources: &[(&str, &str)]) -> TablesJson {
    build(sources, None).tables
}

/// The owners of things: a bounded table read whole.
pub const OWNERS: &str = "module Owners

opaque type OwnerId = String where String.length(value) >= 1 & String.length(value) <= 12

type Owner = Owner { id: OwnerId, name: String }

table Owners
    row     Owner
    key     id
    bounded 5
";

/// **A table of every kind of field the adapters store**: a key of two
/// fields, an order, a unique index, a plain one, a composite one, a
/// reference, and an `Int`'s, a `String`'s and an `Option`'s bounds; a
/// float, a unit, a list, a record, an option of a list.
pub const THINGS: &str = "module Things

import Owners.{ OwnerId }
import Owners

opaque type Slug = String where String.length(value) <= 8
opaque type Rank = Int where value >= 0 & value <= 1000
opaque type Positive = Int where value >= 1

type Point = Point { x: Float, y: Int }

type Thing = Thing {
    owner: OwnerId,
    n: Int,
    rank: Rank,
    slug: Slug,
    flag: Bool,
    score: Float,
    note: Option<String>,
    tags: List<String>,
    point: Point,
    nothing: Unit,
    maybe: Option<List<Int>>,
    count: Option<Positive>,
}

table Things
    row     Thing
    key     owner, n
    order   rank
    unique  by_slug on slug
    index   by_flag on flag
    index   by_owner_rank on owner, rank
    refers  owner to Owners
";

/// The source that holds both, in one database.
pub const THING_DATA: &str = "module ThingData

import Owners
import Things

source ThingData
    holds        Owners, Things
    transactions serializable
    reads        strong
    changes      none
    sets         one_call
";

/// The things' program, as sources.
pub fn things_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Owners.pw", OWNERS),
        ("Things.pw", THINGS),
        ("ThingData.pw", THING_DATA),
    ]
}

/// tables.json of the things' program.
pub fn things() -> TablesJson {
    tables(&things_sources())
}

pub const OWNER: &str = "pw:tables/owners#";
pub const THING: &str = "pw:tables/things#";

/// `Owners`'s or `Things`'s import of `function`.
pub fn owner(function: &str) -> String {
    format!("{OWNER}{function}")
}
pub fn thing(function: &str) -> String {
    format!("{THING}{function}")
}

pub fn owner_row(id: &str) -> Value {
    json!({"id": id, "name": format!("Owner {id}")})
}

/// A thing, every field given.
pub fn thing_row(owner: &str, n: i64, rank: i64, slug: &str, flag: bool) -> Value {
    json!({
        "owner": owner, "n": n, "rank": rank, "slug": slug, "flag": flag,
        "score": (n as f64) / 4.0, "note": null, "tags": [format!("t{n}")],
        "point": {"x": 0.5, "y": n}, "nothing": null, "maybe": null, "count": null,
    })
}

/// The things' tables, empty, on a schema of the test's own.
pub fn open_things(scratch: &Scratch) -> pw_table_postgres::PgTables {
    pw_table_postgres::PgTables::open(&scratch.url, scratch.schema(), things())
        .assert_ok("the adapter opens on a schema of its own")
}
