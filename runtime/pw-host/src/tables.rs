//! **A program's tables, as the host reads `tables.json`** (ADR-0338 D2;
//! ADR-0385, the reads; ADR-0386, the writes and the other reads).
//!
//! The compiler writes `tables.json` beside `contracts.json`: each table a
//! program declares, its fields and their stored types, its key, order,
//! indexes and bound, and the functions it gives, each bound to one host
//! primitive at an import of its own (`pw:tables/lists#get`). This crate does
//! not link the compiler (ADR-0018, ADR-0020), so what the host decides on is
//! mirrored here by field name, as [`crate::ComponentContract`] is;
//! `pw-table-memory`'s tests hold the mirror to what the compiler writes.
//!
//! What the host decides with it:
//! - **a manifest is whole before anything reads it** ([`Manifest::from_json`]):
//!   each table's interface is its name's; its key, order and indexes name
//!   its fields; each function's import is at its own table's interface under
//!   the name its primitive gives it (`get`, `of_list`, `by_slug`, `all`,
//!   `insert`, `delete_of_list`, …), one function an import; each reads or
//!   writes by what its primitive reads by (the key, a proper prefix of it,
//!   one of the table's indexes, or nothing); its effect is its primitive's;
//!   `all` only of a bounded table; and its rows come in the order D1 ruled
//!   (`sorted_by`). Its fields are private, so no other manifest exists;
//! - **the gate** ([`Manifest::admit_reads`] for a query, [`Manifest::admit_command`]
//!   for a command): the `pw:tables/…` imports an admitted component may
//!   link, each a function this manifest declares, the host's to supply,
//!   carrying exactly its table's capability (`database.read<Lists>`,
//!   `database.write<Lists>`), granted; a write only into a command. Any
//!   other table import refuses the whole component;
//! - **the only way a table import links** ([`gate_links`]): the engine asks
//!   the gate for every `pw:tables/…` import a grant would link, and links
//!   none without a manifest, so an import's own claim of a capability
//!   (`database.read<Other>` on `pw:tables/likes#get`) never decides;
//! - **each function's description** ([`Read`], [`Write`]): what it is given,
//!   its row's fields in their declared order, its key and order, which the
//!   engine holds the import's type and each of its answers to.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{ComponentContract, Granted, ImportKind};

/// **`tables.json`, as far as the host reads it**, and only as
/// [`Manifest::from_json`] checked it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    tables: Vec<Table>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Table {
    name: String,
    interface: String,
    fields: Vec<Field>,
    key: Vec<String>,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    indexes: Vec<Index>,
    /// The most rows the table holds, where it is read whole.
    #[serde(default)]
    bounded: Option<u64>,
    /// **Whose its rows are** (track `row-ownership`, ADR-0384 H1): the
    /// field holding each row's owner's id, the fields holding the ids of
    /// the others who read it, and the fields its owner alone reads.
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    shares: Vec<String>,
    #[serde(default)]
    private: Vec<String>,
    /// Each `refers <field> to <Table>`: a foreign key its database holds
    /// across every row of both tables, whoever owns them.
    #[serde(default)]
    references: Vec<Reference>,
    functions: Vec<Function>,
}

/// `refers owner to Users`, as tables.json states it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Reference {
    field: String,
    table: String,
}

/// An index a table declares: `index by_done on done`, `unique by_slug on
/// slug`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Index {
    name: String,
    fields: Vec<String>,
    #[serde(default)]
    unique: bool,
}

/// A table's field, or a record's: its name as Pleris writes it, and what a
/// database stores its value as.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Field {
    pub name: String,
    pub stored: Stored,
}

/// **What a field's value is stored as** (`pw_core::tables::Stored`, mirrored
/// by name): an `Int` and a `String` with the bounds an opaque type states.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stored {
    Int {
        #[serde(default)]
        at_least: Option<i64>,
        #[serde(default)]
        at_most: Option<i64>,
    },
    String {
        #[serde(default)]
        min_length: Option<i64>,
        #[serde(default)]
        max_length: Option<i64>,
    },
    Bool,
    Float,
    Unit,
    Option(Box<Stored>),
    List(Box<Stored>),
    Set(Box<Stored>),
    Map(Box<Stored>, Box<Stored>),
    Record(Vec<Field>),
    Cases(Vec<Case>),
    Recursive(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Case {
    pub name: String,
    #[serde(default)]
    pub carries: Vec<Stored>,
}

/// **The primitive a table's function is bound to** (ADR-0338, 1.3;
/// `pw_core::tables::Primitive`, mirrored by name): one adapter function each,
/// serving every table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Primitive {
    Get,
    GetMany,
    Prefix,
    PrefixMany,
    Index,
    IndexMany,
    Count,
    CountMany,
    All,
    Insert,
    Put,
    Delete,
    DeletePrefix,
}

impl Primitive {
    /// Does it write its table?
    pub fn writes(self) -> bool {
        matches!(
            self,
            Primitive::Insert | Primitive::Put | Primitive::Delete | Primitive::DeletePrefix
        )
    }

    /// Is it a set form, given one list of what its keyed read is given?
    pub fn many(self) -> bool {
        matches!(
            self,
            Primitive::GetMany
                | Primitive::PrefixMany
                | Primitive::IndexMany
                | Primitive::CountMany
        )
    }

    /// Its name, as `tables.json` writes it.
    pub fn name(self) -> &'static str {
        match self {
            Primitive::Get => "get",
            Primitive::GetMany => "get-many",
            Primitive::Prefix => "prefix",
            Primitive::PrefixMany => "prefix-many",
            Primitive::Index => "index",
            Primitive::IndexMany => "index-many",
            Primitive::Count => "count",
            Primitive::CountMany => "count-many",
            Primitive::All => "all",
            Primitive::Insert => "insert",
            Primitive::Put => "put",
            Primitive::Delete => "delete",
            Primitive::DeletePrefix => "delete-prefix",
        }
    }

    pub fn counts(self) -> bool {
        matches!(self, Primitive::Count | Primitive::CountMany)
    }

    /// What it reads or writes by, in a refusal's words.
    pub fn by_words(self) -> &'static str {
        match self {
            Primitive::Get | Primitive::GetMany | Primitive::Delete => "key",
            Primitive::Prefix | Primitive::PrefixMany | Primitive::DeletePrefix => "prefix",
            Primitive::Index | Primitive::IndexMany => "index",
            Primitive::All | Primitive::Insert | Primitive::Put => "row",
            Primitive::Count | Primitive::CountMany => "selector",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Function {
    name: String,
    import: String,
    primitive: Primitive,
    effect: String,
    by: Vec<String>,
    #[serde(default)]
    sorted_by: Vec<String>,
    /// An owned table's function's: the field whose place its handle takes.
    #[serde(default)]
    reader: Option<String>,
}

/// **Who a call reads and writes as** (track `row-ownership`, ADR-0384 H1):
/// the user the host answers `pw:host/principal#read` with for the call,
/// from the session's principal (ADR-0270), or no one, for a call no reader
/// makes (a command the platform invokes, NEXT 6c). Whoever calls the engine
/// says which, and an owned table's function is served only to a call made
/// as someone, for them: a handle a component passes is held to it, never
/// believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The session's principal's user, or its own guest's (ADR-0263).
    User(String),
    /// A checked sign-in lifecycle: the old guest may only be read/deleted
    /// through its owner key; the new account has its ordinary authority.
    /// The engine admits this caller only for an Identity command export.
    IdentityTransition {
        previous_guest: String,
        current: String,
    },
    /// No reader: every owned table's function refuses.
    Nobody,
}

/// **The handle an owned table's function is given** (track
/// `row-ownership`, ADR-0384 H1): the field whose place it takes, the first
/// it reads or deletes by (an `insert`'s and a `put`'s is given beside the
/// row, its owner's), and what a read by it is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle {
    /// The owner's field, or one the table shares with.
    pub field: String,
    /// **The seen row**: the columns a reader the table shares with is
    /// answered, the row's without its private fields; none where the read
    /// answers the row.
    pub seen: Option<Vec<Column>>,
}

/// **One field as a function's values carry it**: its Pleris name (the
/// adapter's rows), its WIT name (the component's records), and its stored
/// type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub wit: String,
    pub stored: Stored,
}

impl Column {
    /// A field by its Pleris name, its WIT name the compiler's for it.
    pub fn new(name: &str, stored: Stored) -> Column {
        Column {
            name: name.to_string(),
            wit: wit_name(name),
            stored,
        }
    }

    pub fn of(field: &Field) -> Column {
        Column::new(&field.name, field.stored.clone())
    }
}

/// **One read, as tables.json declares it**: what the engine holds its
/// import's type, its arguments and each of its answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// `pw:tables/lists#get`.
    pub import: String,
    /// The table, by its Pleris name: `Lists`.
    pub table: String,
    /// `get`, `get-many`, `prefix`, `prefix-many`, `index`, `index-many` or
    /// `all`. A set form (`*-many`) is given one list of what its keyed read
    /// is given, and answers a list of rows.
    pub primitive: Primitive,
    /// What it reads by, in order: the key's fields, a prefix's, an index's;
    /// none for `all`.
    pub by: Vec<Column>,
    /// The row's fields, in their declared order.
    pub row: Vec<Column>,
    /// The table's key and order, and the function's `sorted_by`: the order
    /// its rows come in ([`Read::canonical`]).
    pub key: Vec<String>,
    pub order: Option<String>,
    pub sorted_by: Vec<String>,
    /// An `index` read of a unique index: none or one row, as a `get`.
    pub unique: bool,
    /// The table's bound: the most rows an `all` answers.
    pub bounded: Option<u64>,
    /// An owned table's read's handle (track `row-ownership`): none for an
    /// unowned table's.
    pub handle: Option<Handle>,
}

impl Read {
    /// Is it a set form, given one list?
    pub fn many(&self) -> bool {
        self.primitive.many()
    }

    /// **Does it answer none or one row** (an `option<row>`): a `get`, and a
    /// unique index's read. Every other read answers a list.
    pub fn one(&self) -> bool {
        self.primitive == Primitive::Get || (self.primitive == Primitive::Index && self.unique)
    }

    /// **The order a read's rows come in** (D1's ruling): by its `sorted_by`,
    /// then the table's order, then its whole key, each field once.
    pub fn canonical(&self) -> Vec<String> {
        if self.primitive.counts() {
            return self.sorted_by.clone();
        }
        let mut out: Vec<String> = Vec::new();
        for field in self
            .sorted_by
            .iter()
            .chain(self.order.iter())
            .chain(self.key.iter())
        {
            if !out.contains(field) {
                out.push(field.clone());
            }
        }
        out
    }
}

/// **One write, as tables.json declares it** (ADR-0386): `insert` and `put`
/// given a whole row, `delete` its key's fields, `delete_of_<fields>` a
/// prefix's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    /// `pw:tables/lists#insert`.
    pub import: String,
    pub table: String,
    /// `insert`, `put`, `delete` or `delete-prefix`.
    pub primitive: Primitive,
    /// What a delete is given: the key's fields, or a prefix's; none for
    /// `insert` and `put`, which are given a row.
    pub by: Vec<Column>,
    /// The row's fields, in their declared order.
    pub row: Vec<Column>,
    /// The table's key.
    pub key: Vec<String>,
    /// Each unique index's fields, in tables.json's order.
    pub unique: Vec<Vec<String>>,
    /// An owned table's write's handle, its owner's (track
    /// `row-ownership`): none for an unowned table's.
    pub handle: Option<Handle>,
}

impl Write {
    /// **Does it answer `result<_, row>`**, refused by the row that holds what
    /// it would write: an `insert` always (its key, or a unique value), and a
    /// `put` where its table has a unique index (D1's types).
    pub fn refusable(&self) -> bool {
        self.primitive == Primitive::Insert
            || (self.primitive == Primitive::Put && !self.unique.is_empty())
    }
}

/// **A table function the gate admits**: a read, or a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Read(Read),
    Write(Write),
}

impl Access {
    pub fn import(&self) -> &str {
        match self {
            Access::Read(read) => &read.import,
            Access::Write(write) => &write.import,
        }
    }

    pub fn table(&self) -> &str {
        match self {
            Access::Read(read) => &read.table,
            Access::Write(write) => &write.table,
        }
    }

    /// An owned table's function's handle (track `row-ownership`).
    pub fn handle(&self) -> Option<&Handle> {
        match self {
            Access::Read(read) => read.handle.as_ref(),
            Access::Write(write) => write.handle.as_ref(),
        }
    }
}

/// Is this interface the tables' namespace, in any spelling of it?
pub fn is_table_interface(interface: &str) -> bool {
    interface == "pw:tables"
        || interface.starts_with("pw:tables/")
        || interface.starts_with("pw:tables@")
}

/// Is this import key (`interface#function`) a table's?
pub fn is_table_import(key: &str) -> bool {
    is_table_interface(key.split('#').next().unwrap_or(key))
}

/// The fields in order, each once.
fn each_once(fields: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for field in fields {
        if !out.contains(field) {
            out.push(field.clone());
        }
    }
    out
}

impl Table {
    /// **The name a function bound to `primitive` by `by` has**, as D1
    /// generates it, or why no function of this table may be bound so: a
    /// read or a delete by the key, by a proper prefix of it, or by one of
    /// its indexes; `all` of a bounded table; `insert` and `put` by nothing.
    fn generated_name(&self, primitive: Primitive, by: &[String]) -> Result<String, String> {
        let name = &self.name;
        let prefix = !by.is_empty() && by.len() < self.key.len() && self.key[..by.len()] == *by;
        let of = || format!("of_{}", by.join("_"));
        match primitive {
            Primitive::Count | Primitive::CountMany => {
                let prefix =
                    !by.is_empty() && by.len() <= self.key.len() && self.key[..by.len()] == *by;
                let indexed = self.indexes.iter().any(|index| index.fields == by);
                if !prefix && !indexed {
                    return Err(format!(
                        "counts by no declared key, prefix or index of {name}"
                    ));
                }
                let suffix = if primitive.many() { "_many" } else { "" };
                Ok(format!("count_by_{}{suffix}", by.join("_")))
            }
            Primitive::Get | Primitive::GetMany | Primitive::Delete if by != self.key => {
                Err(format!("reads by other than {name}'s key"))
            }
            Primitive::Get => Ok("get".into()),
            Primitive::GetMany => Ok("get_many".into()),
            Primitive::Delete => Ok("delete".into()),
            Primitive::Prefix | Primitive::PrefixMany | Primitive::DeletePrefix if !prefix => Err(
                format!("reads by other than a proper prefix of {name}'s key"),
            ),
            Primitive::Prefix => Ok(of()),
            Primitive::PrefixMany => Ok(format!("{}_many", of())),
            Primitive::DeletePrefix => Ok(format!("delete_{}", of())),
            Primitive::Index | Primitive::IndexMany => {
                let index = self
                    .indexes
                    .iter()
                    .find(|index| index.fields == by)
                    .ok_or_else(|| format!("reads by other than one of {name}'s indexes"))?;
                Ok(match primitive {
                    Primitive::Index => index.name.clone(),
                    _ => format!("{}_many", index.name),
                })
            }
            Primitive::All | Primitive::Insert | Primitive::Put if !by.is_empty() => Err(format!(
                "is given a whole row or nothing, not {name}'s fields"
            )),
            Primitive::All if self.bounded.is_none() => Err(format!(
                "is an all of {name}, which states no bound (PW0364)"
            )),
            Primitive::All => Ok("all".into()),
            Primitive::Insert => Ok("insert".into()),
            Primitive::Put => Ok("put".into()),
        }
    }

    /// **The order a function's rows come in** (D1's `sorted_by`): none for
    /// what answers one row or none, and for a write; otherwise grouped by
    /// what a set form was given, then by the table's order, then by its key,
    /// each field once.
    fn sorted_by(&self, primitive: Primitive, by: &[String]) -> Vec<String> {
        if primitive == Primitive::Count {
            return Vec::new();
        }
        if primitive == Primitive::CountMany {
            return by.to_vec();
        }
        if primitive.writes() || primitive == Primitive::Get || self.unique_index(primitive, by) {
            return Vec::new();
        }
        let grouped: &[String] = if primitive.many() { by } else { &[] };
        each_once(
            &grouped
                .iter()
                .chain(self.order.iter())
                .chain(self.key.iter())
                .cloned()
                .collect::<Vec<_>>(),
        )
    }

    /// Is it a unique index's one-row read?
    fn unique_index(&self, primitive: Primitive, by: &[String]) -> bool {
        primitive == Primitive::Index
            && self
                .indexes
                .iter()
                .any(|index| index.unique && index.fields == by)
    }

    fn column(&self, name: &str) -> Option<Column> {
        self.fields.iter().find(|f| f.name == name).map(Column::of)
    }

    /// **Whose its rows are, held whole** (track `row-ownership`, ADR-0384
    /// H1; the compiler's PW0367 to PW0369, held again where the host reads
    /// the manifest): an owner and each sharer a field holding a principal's
    /// id, a string; the key led by the owner; every index by the owner or a
    /// sharer, a unique one holding the owner; a private field no reader but
    /// the owner selects or orders rows by; each function given its handle
    /// in the place of the field it leads with, a read's the owner's or a
    /// sharer's, a write's the owner's; and no `all`. A table no one owns
    /// states none of it.
    fn owned_whole(&self) -> Result<(), String> {
        let name = &self.name;
        let refused = |why: String| Err(format!("tables.json: {name} {why}"));
        let Some(owner) = &self.owner else {
            if !self.shares.is_empty() || !self.private.is_empty() {
                return refused("shares or keeps fields private, and no one owns it".into());
            }
            if let Some(f) = self.functions.iter().find(|f| f.reader.is_some()) {
                return refused(format!(
                    "gives `{}` a handle, and no one owns its rows",
                    f.name
                ));
            }
            return Ok(());
        };
        let principal = |field: &String| {
            self.fields
                .iter()
                .any(|f| f.name == *field && matches!(f.stored, Stored::String { .. }))
        };
        if !principal(owner) {
            return refused(format!(
                "is owned by `{owner}`, which is no field holding a principal's id"
            ));
        }
        for (at, sharer) in self.shares.iter().enumerate() {
            if sharer == owner || !principal(sharer) || self.shares[..at].contains(sharer) {
                return refused(format!(
                    "shares with `{sharer}`, which is no other field holding a principal's id"
                ));
            }
        }
        if self.key.first() != Some(owner) {
            return refused(format!(
                "is owned by `{owner}`, and its key leads with another"
            ));
        }
        let reader =
            |field: Option<&String>| field.is_some_and(|f| f == owner || self.shares.contains(f));
        for index in &self.indexes {
            if !reader(index.fields.first()) || (index.unique && !index.fields.contains(owner)) {
                return refused(format!(
                    "is owned, and its index `{}` is read by no reader of its rows alone",
                    index.name
                ));
            }
        }
        for (at, private) in self.private.iter().enumerate() {
            let seen_by_others = !self.shares.is_empty()
                && (self.order.as_ref() == Some(private)
                    || self.indexes.iter().any(|index| {
                        index.fields.contains(private)
                            && index
                                .fields
                                .first()
                                .is_some_and(|f| self.shares.contains(f))
                    }));
            if !self.fields.iter().any(|f| f.name == *private)
                || reader(Some(private))
                || self.key.contains(private)
                || self.private[..at].contains(private)
                || seen_by_others
            {
                return refused(format!(
                    "keeps `{private}` private, which another reader would select or order its \
                     rows by"
                ));
            }
        }
        for function in &self.functions {
            if function.primitive == Primitive::CountMany && function.by.len() <= 1 {
                return refused(format!(
                    "`{}` has no selector after its reader",
                    function.name
                ));
            }
            let given = match function.primitive {
                Primitive::All => {
                    return refused("is owned, and is read whole by `all`".into());
                }
                Primitive::Insert | Primitive::Put => Some(owner),
                Primitive::Delete | Primitive::DeletePrefix => {
                    function.by.first().filter(|f| *f == owner)
                }
                _ => function.by.first().filter(|f| reader(Some(f))),
            };
            if given.is_none() || function.reader.as_ref() != given {
                return refused(format!(
                    "is owned, and `{}` is not given its reader's handle in the place of the \
                     field it leads with",
                    function.name
                ));
            }
        }
        // A bound counts every owner's rows (the review of track
        // `row-ownership`, finding 2; the compiler's PW0368): one owner's
        // rows would refuse another's write, and tell them how many.
        if let Some(bound) = self.bounded {
            return refused(format!(
                "is owned, and its bound of {bound} rows counts every owner's"
            ));
        }
        Ok(())
    }

    /// **The handle a function of this table is given** (track
    /// `row-ownership`), as [`Table::owned_whole`] held it: none where no
    /// one owns the table. A read by a sharer of a table with private fields
    /// is answered the seen row.
    fn handle(&self, function: &Function) -> Option<Handle> {
        let field = function.reader.clone()?;
        let sharer = self.shares.contains(&field) && !function.primitive.writes();
        let seen = (sharer && !self.private.is_empty()).then(|| {
            self.fields
                .iter()
                .filter(|f| !self.private.contains(&f.name))
                .map(Column::of)
                .collect()
        });
        Some(Handle { field, seen })
    }
}

impl Manifest {
    /// **`tables.json`, read and held whole** (see the module): a manifest
    /// that is not is refused here, not read past.
    pub fn from_json(text: &str) -> Result<Manifest, String> {
        let manifest: Manifest =
            serde_json::from_str(text).map_err(|error| format!("tables.json: {error}"))?;
        manifest.whole()?;
        Ok(manifest)
    }

    fn whole(&self) -> Result<(), String> {
        let mut imports = BTreeSet::new();
        for table in &self.tables {
            let name = &table.name;
            let interface = format!("pw:tables/{}", wit_name(name));
            if table.interface != interface {
                return Err(format!(
                    "tables.json: {name}'s interface is `{}`, not `{interface}`",
                    table.interface
                ));
            }
            let fields: BTreeSet<&str> = table.fields.iter().map(|f| f.name.as_str()).collect();
            if fields.len() != table.fields.len() {
                return Err(format!("tables.json: {name} repeats a field"));
            }
            let known = |field: &String| fields.contains(field.as_str());
            if table.key.is_empty() || !table.key.iter().all(known) {
                return Err(format!("tables.json: {name}'s key is not its fields'"));
            }
            if each_once(&table.key).len() != table.key.len() {
                return Err(format!("tables.json: {name}'s key names a field twice"));
            }
            if !table.order.iter().all(known) {
                return Err(format!(
                    "tables.json: {name}'s order is not one of its fields"
                ));
            }
            // Each index named as D1 names one, of its fields, each once,
            // and no two indexes on the same fields: a unique and a plain
            // index on one field would answer one row by D1's order and a
            // list by D1's types (ADR-0386's finding).
            let mut seen: Vec<&Index> = Vec::new();
            for index in &table.indexes {
                let reads = format!("{}_many", index.name);
                if !index.name.starts_with("by_")
                    || index.fields.is_empty()
                    || !index.fields.iter().all(known)
                    || each_once(&index.fields).len() != index.fields.len()
                    || seen.iter().any(|other| {
                        other.fields == index.fields
                            || other.name == index.name
                            || other.name == reads
                            || format!("{}_many", other.name) == index.name
                    })
                {
                    return Err(format!(
                        "tables.json: {name}'s index `{}` is not one index of its fields",
                        index.name
                    ));
                }
                seen.push(index);
            }
            if table.bounded == Some(0) {
                return Err(format!("tables.json: {name} is bounded to no row"));
            }
            for function in &table.functions {
                let import = format!("{interface}#{}", wit_name(&function.name));
                if function.import != import {
                    return Err(format!(
                        "tables.json: {name}.{} is imported at `{}`, outside `{import}`",
                        function.name, function.import
                    ));
                }
                if !imports.insert(function.import.as_str()) {
                    return Err(format!("tables.json: `{import}` is two functions'"));
                }
                let effect = if function.primitive.writes() {
                    "write"
                } else {
                    "read"
                };
                if function.effect != effect {
                    return Err(format!(
                        "tables.json: `{import}` is a {effect}, not a {}",
                        function.effect
                    ));
                }
                if !function.by.iter().chain(&function.sorted_by).all(known) {
                    return Err(format!(
                        "tables.json: `{import}` names a field {name} lacks"
                    ));
                }
                // What a primitive reads by, and so its name (PR #29's rule,
                // a get at `#get`, for every primitive).
                let generated = table
                    .generated_name(function.primitive, &function.by)
                    .map_err(|why| format!("tables.json: `{import}` {why}"))?;
                if function.name != generated {
                    let primitive = function.primitive.name();
                    let article = if primitive.starts_with(['a', 'e', 'i', 'o', 'u']) {
                        "an"
                    } else {
                        "a"
                    };
                    return Err(format!(
                        "tables.json: `{import}` is {article} {primitive}, imported elsewhere than `#{}`",
                        wit_name(&generated)
                    ));
                }
                let sorted = table.sorted_by(function.primitive, &function.by);
                if function.sorted_by != sorted {
                    return Err(format!(
                        "tables.json: `{import}` answers by ({}), where D1's order is ({})",
                        function.sorted_by.join(", "),
                        sorted.join(", ")
                    ));
                }
            }
        }
        // Whose each table's rows are (track `row-ownership`).
        for table in &self.tables {
            table.owned_whole()?;
        }
        self.references_within_one_owner()
    }

    /// **Each reference stays within one owner's rows** (track
    /// `row-ownership`, the review's finding 1; the compiler's PW6924, held
    /// again where the host reads the manifest): its field is its table's,
    /// it names a table of this manifest, and a reference into an owned
    /// table is made by an owned table from its own owner field, naming the
    /// owner's own row; a table no one owns refers to no owned table, and an
    /// owned table to no table no one owns. A database checks a reference
    /// across every row, whoever owns it (PostgreSQL's foreign keys are
    /// checked past its row security), so any other reference would answer
    /// one owner's write with another's rows.
    fn references_within_one_owner(&self) -> Result<(), String> {
        for table in &self.tables {
            let name = &table.name;
            for reference in &table.references {
                let (field, to) = (&reference.field, &reference.table);
                if !table.fields.iter().any(|f| f.name == *field) {
                    return Err(format!(
                        "tables.json: {name} refers by `{field}`, which is not its field"
                    ));
                }
                let Some(target) = self.tables.iter().find(|t| t.name == *to) else {
                    return Err(format!(
                        "tables.json: {name} refers `{field}` to {to}, which it does not declare"
                    ));
                };
                let within = match (&table.owner, &target.owner) {
                    (None, None) => true,
                    (Some(owner), Some(_)) => field == owner,
                    _ => false,
                };
                if !within {
                    return Err(format!(
                        "tables.json: {name} refers `{field}` to {to}, across owners' rows: a \
                         reference into an owned table is made from its referring table's \
                         owner, and an owned table refers to no table no one owns"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The function tables.json declares at `import`, as the engine holds it,
    /// or none.
    fn access(&self, import: &str) -> Option<Access> {
        self.tables.iter().find_map(|table| {
            let function = table.functions.iter().find(|f| f.import == import)?;
            let by = function
                .by
                .iter()
                .map(|name| table.column(name))
                .collect::<Option<Vec<_>>>()?;
            let row: Vec<Column> = table.fields.iter().map(Column::of).collect();
            if function.primitive.writes() {
                return Some(Access::Write(Write {
                    import: function.import.clone(),
                    table: table.name.clone(),
                    primitive: function.primitive,
                    by,
                    row,
                    key: table.key.clone(),
                    unique: table
                        .indexes
                        .iter()
                        .filter(|index| index.unique)
                        .map(|index| index.fields.clone())
                        .collect(),
                    handle: table.handle(function),
                }));
            }
            Some(Access::Read(Read {
                import: function.import.clone(),
                table: table.name.clone(),
                primitive: function.primitive,
                by,
                row,
                key: table.key.clone(),
                order: table.order.clone(),
                sorted_by: function.sorted_by.clone(),
                unique: table.unique_index(function.primitive, &function.by),
                bounded: table.bounded,
                handle: table.handle(function),
            }))
        })
    }

    /// **Each table it declares, by name** (track `data-path`): what a host
    /// serving it grants a node, each table's read and write.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tables.iter().map(|table| table.name.as_str())
    }

    /// **The gate for a query** (PR #29's, moved here so that the engine
    /// holds every table import to it): the table reads an admitted
    /// component may link, by import, each as tables.json declares it. Every
    /// `pw:tables/…` import the contract declares must be a read this
    /// manifest declares, the host's to supply, carrying exactly its table's
    /// read capability, and granted; one that is not, a write among them,
    /// refuses the whole component, named, rather than being left out.
    pub fn admit_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<BTreeMap<String, Read>, String> {
        self.admit(contract, granted, false)?
            .into_iter()
            .map(|(key, access)| match access {
                Access::Read(read) => Ok((key, read)),
                Access::Write(write) => Err(format!(
                    "the table gate cannot bind `{}` into a query: it writes",
                    write.import
                )),
            })
            .collect()
    }

    /// **The gate for a command** (ADR-0386): its reads, as
    /// [`Manifest::admit_reads`] admits them, and its writes, each carrying
    /// exactly its table's write capability (`database.write<Lists>`), and
    /// granted.
    pub fn admit_command(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<BTreeMap<String, Access>, String> {
        self.admit(contract, granted, true)
    }

    fn admit(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
        command: bool,
    ) -> Result<BTreeMap<String, Access>, String> {
        let linkable: BTreeSet<String> = crate::linkable(contract, granted).into_iter().collect();
        let mut admitted = BTreeMap::new();
        for import in contract
            .imports
            .iter()
            .filter(|import| is_table_interface(&import.interface))
        {
            let key = import.key();
            let refused = |why: String| format!("the table gate cannot bind `{key}`: {why}");
            let Some(access) = self.access(&key) else {
                return Err(refused("tables.json declares no function there".into()));
            };
            if import.kind != ImportKind::HostCapability {
                return Err(refused("it is a component's import, not the host's".into()));
            }
            let (operation, write) = match &access {
                Access::Read(_) => ("read", false),
                Access::Write(_) => ("write", true),
            };
            if write && !command {
                return Err(refused(format!(
                    "it writes {}, and a table's write links only into a command",
                    access.table()
                )));
            }
            let capability = format!("database.{operation}<{}>", access.table());
            if import.capability != capability {
                return Err(refused(format!(
                    "its contract claims `{}`, and {}'s {operation} is `{capability}`",
                    import.capability,
                    access.table()
                )));
            }
            if !linkable.contains(&key) {
                return Err(refused(format!("`{capability}` is not granted here")));
            }
            admitted.insert(key, access);
        }
        Ok(admitted)
    }
}

/// **The table functions a call links, and no other way to link one** (the
/// integrator's ruling 2 of 2026-10-25; ADR-0386 for a command's writes).
/// `linkable` trusts each import's own claim of its capability, which is
/// right for a capability and wrong for a table: an import may claim
/// `database.read<Other>` on `pw:tables/likes#get`. So where a grant would
/// link any `pw:tables/…` import, the engine links it only through the gate,
/// with the program's manifest, and with none it links nothing. A `command`
/// call's gate admits its writes; any other call's refuses one. Each function
/// the gate admits is keyed by its import; a table import the grant links is
/// always one of them, since the gate refuses any it does not admit.
pub fn gate_links(
    contract: &ComponentContract,
    granted: &Granted,
    tables: Option<&Manifest>,
    command: bool,
) -> Result<BTreeMap<String, Access>, String> {
    let supplied = crate::linkable(contract, granted);
    let Some(first) = supplied.iter().find(|key| is_table_import(key)) else {
        return Ok(BTreeMap::new());
    };
    match tables {
        Some(tables) => tables.admit(contract, granted, command),
        None => Err(format!(
            "`{first}` is a table's import, which links only through its program's tables.json \
             (the table gate), and this call was given none"
        )),
    }
}

/// **A WIT identifier from a Pleris name**, as the compiler writes one
/// (`pw_core::wit::ident`, which `pw-table-memory`'s tests hold this equal
/// to): `sort_order` and `sortOrder` are `sort-order`, `Lists` is `lists`.
pub fn wit_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        match ch {
            '.' | '_' | ' ' | '/' | ':' => {
                if !out.ends_with('-') && !out.is_empty() {
                    out.push('-');
                }
                prev_lower = false;
            }
            c if c.is_ascii_uppercase() => {
                if prev_lower && !out.ends_with('-') {
                    out.push('-');
                }
                out.push(c.to_ascii_lowercase());
                prev_lower = false;
            }
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
            }
            _ => {}
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "x".to_string()
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        format!("x-{trimmed}")
    } else {
        trimmed
    }
}
