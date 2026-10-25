//! **A program's tables, as the host reads `tables.json`** (ADR-0338 D2).
//!
//! The compiler writes `tables.json` beside `contracts.json`: each table a
//! program declares, its fields and their stored types, its key, and the
//! functions it gives, each bound to one host primitive at an import of its
//! own (`pw:tables/lists#get`). This crate does not link the compiler
//! (ADR-0018, ADR-0020), so what the host decides on is mirrored here by
//! field name, as [`crate::ComponentContract`] is; `pw-table-memory`'s tests
//! hold the mirror to what the compiler writes.
//!
//! What the host decides with it (the D2 read path, and the integrator's
//! rulings of 2026-10-25 on PRs #29 to #40):
//! - **a manifest is whole before anything reads it** ([`Manifest::from_json`]):
//!   each table's interface is its name's, each function's import is at its
//!   own table's interface under its own name, each import is one function's,
//!   each effect is its primitive's, and every field a function names is its
//!   table's. Its fields are private, so no other manifest exists;
//! - **the gate** ([`Manifest::admit_reads`]): the `pw:tables/…` imports an
//!   admitted component may link, each a `get` or `get-many` read carrying
//!   exactly its table's read capability (`database.read<Lists>`), and
//!   granted. Any other table import refuses the whole component;
//! - **the only way a table import links** ([`gate_links`]): the engine asks
//!   the gate for every `pw:tables/…` import a grant would link, and links
//!   none without a manifest, so an import's own claim of a capability
//!   (`database.read<Other>` on `pw:tables/likes#get`) never decides;
//! - **each read's description** ([`Read`]): its selector's fields, its row's
//!   fields in their declared order, its key and its order, which the engine
//!   holds the import's type and each of its answers to.

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
    functions: Vec<Function>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Primitive {
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
    fn writes(self) -> bool {
        matches!(
            self,
            Primitive::Insert | Primitive::Put | Primitive::Delete | Primitive::DeletePrefix
        )
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

/// **One field as a read's values carry it**: its Pleris name (the adapter's
/// rows), its WIT name (the component's records), and its stored type.
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

/// **One `get` or `get-many`, as tables.json declares it**: what the engine
/// holds its import's type, its arguments and each of its answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// `pw:tables/lists#get`.
    pub import: String,
    /// The table, by its Pleris name: `Lists`.
    pub table: String,
    /// `get-many`: one list of keys in, a list of rows out. Otherwise a `get`:
    /// the key's fields in, none or one row out.
    pub many: bool,
    /// The key's fields, in its order: what a selector holds.
    pub by: Vec<Column>,
    /// The row's fields, in their declared order.
    pub row: Vec<Column>,
    /// The table's key and order, and the function's `sorted_by`: the order
    /// a `get-many`'s rows come in ([`Read::canonical`]).
    pub key: Vec<String>,
    pub order: Option<String>,
    pub sorted_by: Vec<String>,
}

impl Read {
    /// **The order a `get-many` answers in** (D1's ruling): by its
    /// `sorted_by`, then the table's order, then its whole key, each field
    /// once.
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
            if !table.order.iter().all(known) {
                return Err(format!(
                    "tables.json: {name}'s order is not one of its fields"
                ));
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
                // A primitive the host serves is at its own name (PR #29's
                // rule): a get at `#get`, a get-many at `#get-many`.
                let served = match function.primitive {
                    Primitive::Get => Some("get"),
                    Primitive::GetMany => Some("get-many"),
                    _ => None,
                };
                if let Some(served) = served
                    && function.import != format!("{interface}#{served}")
                {
                    return Err(format!(
                        "tables.json: `{import}` is a {served}, imported elsewhere than `#{served}`"
                    ));
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
                if matches!(function.primitive, Primitive::Get | Primitive::GetMany)
                    && function.by != table.key
                {
                    return Err(format!(
                        "tables.json: `{import}` reads by other than {name}'s key"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The `get` or `get-many` tables.json declares at `import`, or none.
    fn read(&self, import: &str) -> Option<Read> {
        self.tables.iter().find_map(|table| {
            let function = table.functions.iter().find(|f| f.import == import)?;
            let many = match function.primitive {
                Primitive::Get => false,
                Primitive::GetMany => true,
                _ => return None,
            };
            let column = |name: &String| {
                table
                    .fields
                    .iter()
                    .find(|f| &f.name == name)
                    .map(Column::of)
            };
            Some(Read {
                import: function.import.clone(),
                table: table.name.clone(),
                many,
                by: function.by.iter().map(column).collect::<Option<_>>()?,
                row: table.fields.iter().map(Column::of).collect(),
                key: table.key.clone(),
                order: table.order.clone(),
                sorted_by: function.sorted_by.clone(),
            })
        })
    }

    /// **The gate** (PR #29's, moved here so that the engine holds every
    /// table import to it): the table reads an admitted component may link,
    /// by import, each as tables.json declares it. Every `pw:tables/…` import
    /// the contract declares must be a `get` or `get-many` this manifest
    /// declares, the host's to supply, carrying exactly its table's read
    /// capability, and granted; one that is not refuses the whole component,
    /// named, rather than being left out.
    pub fn admit_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<BTreeMap<String, Read>, String> {
        let linkable: BTreeSet<String> = crate::linkable(contract, granted).into_iter().collect();
        let mut reads = BTreeMap::new();
        for import in contract
            .imports
            .iter()
            .filter(|import| is_table_interface(&import.interface))
        {
            let key = import.key();
            let refused = |why: String| format!("read-only table host cannot bind `{key}`: {why}");
            let Some(read) = self.read(&key) else {
                return Err(refused(
                    "tables.json declares no get or get-many there".into(),
                ));
            };
            if import.kind != ImportKind::HostCapability {
                return Err(refused("it is a component's import, not the host's".into()));
            }
            let capability = format!("database.read<{}>", read.table);
            if import.capability != capability {
                return Err(refused(format!(
                    "its contract claims `{}`, and {}'s read is `{capability}`",
                    import.capability, read.table
                )));
            }
            if !linkable.contains(&key) {
                return Err(refused(format!("`{capability}` is not granted here")));
            }
            reads.insert(key, read);
        }
        Ok(reads)
    }
}

/// **The table reads a call links, and no other way to link one** (the
/// integrator's ruling 2 of 2026-10-25). `linkable` trusts each import's own
/// claim of its capability, which is right for a capability and wrong for a
/// table: an import may claim `database.read<Other>` on `pw:tables/likes#get`.
/// So where a grant would link any `pw:tables/…` import, the engine links it
/// only through the gate, with the program's manifest, and with none it links
/// nothing. Each read the gate admits is keyed by its import; a table import
/// the grant links is always one of them, since the gate refuses any it does
/// not admit.
pub fn gate_links(
    contract: &ComponentContract,
    granted: &Granted,
    tables: Option<&Manifest>,
) -> Result<BTreeMap<String, Read>, String> {
    let supplied = crate::linkable(contract, granted);
    let Some(first) = supplied.iter().find(|key| is_table_import(key)) else {
        return Ok(BTreeMap::new());
    };
    match tables {
        Some(tables) => tables.admit_reads(contract, granted),
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
