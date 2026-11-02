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
    functions: Vec<Function>,
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
            Primitive::GetMany | Primitive::PrefixMany | Primitive::IndexMany
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
            Primitive::All => "all",
            Primitive::Insert => "insert",
            Primitive::Put => "put",
            Primitive::Delete => "delete",
            Primitive::DeletePrefix => "delete-prefix",
        }
    }

    /// What it reads or writes by, in a refusal's words.
    pub fn by_words(self) -> &'static str {
        match self {
            Primitive::Get | Primitive::GetMany | Primitive::Delete => "key",
            Primitive::Prefix | Primitive::PrefixMany | Primitive::DeletePrefix => "prefix",
            Primitive::Index | Primitive::IndexMany => "index",
            Primitive::All | Primitive::Insert | Primitive::Put => "row",
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
            }))
        })
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
