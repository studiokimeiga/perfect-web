//! **An app's data, declared** (track `tables`; ADR-0338's decision 1, D1).
//!
//! A program declares its data in Pleris, and one adapter per database
//! serves every program's declarations, so no app's data layer is Rust
//! (ADR-0338's ruling 1). This is the declaration's half:
//!
//! ```text
//! module Todos
//!
//! type Todo = Todo { id: TodoId, list: ListId, title: String, done: Bool, added: Int }
//!
//! table Todos
//!     row     Todo
//!     key     id
//!     order   added
//!     index   by_list on list
//! ```
//!
//! - **A table is declared in the module of its name**, and its functions
//!   are that module's declarations (`generate`), so `Todos.get(id)` is an
//!   ordinary call into the module and `database.read<Todos>` names the table
//!   as ADR-0207's resources are named (`database.read<Carts>` names the
//!   module `Carts`). Each function is bound by `host` to a primitive
//!   imported per table, `pw:tables/todos#get`, and its effect row reads or
//!   writes the table: `get`, `get_many`, a read `of_<fields>` for each proper
//!   prefix of the key and its set form, a read for each index and its set
//!   form, `all` where the table is `bounded`, and the writes `insert`, `put`,
//!   `delete` and `delete_of_<fields>`. They are the program's only way to
//!   its rows (PW0362).
//! - **Its checks**: the declaration's shape (PW0361), its rows reached only
//!   through its functions (PW0362), `all()` only of a bounded table (PW0364),
//!   a warning that no database holds a reference across two sources
//!   (PW0365), and a change from the schema released stated
//!   (PW0366, `held_to`).
//! - **What a host needs** (`TablesJson`): `pw build` writes `tables.json`,
//!   each table's fields and their types, key, order, indexes, references,
//!   source and functions, and `schema.lock`, the schema the build declares.

use std::collections::{BTreeMap, BTreeSet};

use crate::codes;
use crate::diagnostics::{Detector, Diagnostic, Related, Repair, Severity};
use crate::hir::{Decl, DeclId, DeclKind, DeclaredType, EffectRef, Hir, Param, Policy, Span};
use crate::resolve::{DefId, Namespace, Resolution, Workspace};
use crate::resolved::{Primitive as Prim, ResolvedType};
use crate::signatures::Signatures;

// --- what the compiler writes ------------------------------------------------

/// **A primitive a table's function is bound to** (ADR-0338, 1.3): the host
/// keeps one adapter function for each, and binds every table's import to it
/// by what `tables.json` says of the function.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Primitive {
    /// One row by its whole key, or none.
    Get,
    /// Each row whose key is one of a list's.
    GetMany,
    /// The rows of one prefix of the key.
    Prefix,
    /// The rows of each prefix of a list's.
    PrefixMany,
    /// The rows of one value of an index.
    Index,
    /// The rows of each value of a list's.
    IndexMany,
    /// The cardinality of a declared key/prefix/index selector.
    Count,
    /// One count for each distinct requested selector, including zero.
    CountMany,
    /// Every row, of a bounded table.
    All,
    /// A new row, refused where its key or a unique value is held.
    Insert,
    /// A row, whole, by its key: a new one or the one it replaces.
    Put,
    /// The row of a key, if there is one.
    Delete,
    /// Every row of a prefix of the key.
    DeletePrefix,
}

impl Primitive {
    /// Its name, as `tables.json` and the note (ADR-0338, 1.3) write it.
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

    /// Does it write the table?
    pub fn writes(self) -> bool {
        matches!(
            self,
            Primitive::Insert | Primitive::Put | Primitive::Delete | Primitive::DeletePrefix
        )
    }

    /// Is it a set form, given a list of what its keyed read is given?
    pub fn many(self) -> bool {
        matches!(
            self,
            Primitive::GetMany
                | Primitive::PrefixMany
                | Primitive::IndexMany
                | Primitive::CountMany
        )
    }
}

/// **A declaration the compiler wrote for a table** (`Hir::generated`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The table's declaration.
    pub table: DeclId,
    pub what: Wrote,
}

/// What it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrote {
    /// A read or a write, bound to `primitive`, by `by`: the key's fields, a
    /// prefix's, an index's; none for `all`, `insert` and `put`.
    Function {
        primitive: Primitive,
        by: Vec<String>,
    },
    /// The record a set form's list holds for a tuple of `fields`: a key, a
    /// prefix or an index of more than one field.
    Tuple { fields: Vec<String> },
    /// A set aggregate's selector and count, never a stored row.
    Counted { fields: Vec<String> },
    /// **The seen row** (track `row-ownership`, ADR-0384 H1): the record a
    /// reader the table shares with is answered, its row's `fields` without
    /// the private ones, so a value only its owner reads has no field to
    /// reach another reader through.
    Seen { fields: Vec<String> },
}

// --- the declaration, as written ---------------------------------------------

/// **A clause's value, and where it is written.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause<T> {
    pub value: T,
    pub span: Span,
}

/// `index by_list on list`, `unique by_slug on slug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexClause {
    pub name: String,
    pub fields: Vec<String>,
    pub unique: bool,
}

/// `refers owner to Users`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub field: String,
    pub table: String,
}

/// `migrate title renamed from name`, `migrate notes dropped`, `migrate rank
/// backfilled with 0`: a field's change from the schema released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub field: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// It was `from` in the schema released.
    Renamed { from: String },
    /// It was in the schema released, and its values are given up.
    Dropped,
    /// Each row released holds this value in it: a field added that is no
    /// `Option`, or a field whose type changed.
    Backfilled(Literal),
}

/// **A value a migration writes into every row**: a literal of the field's
/// stored type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Literal {
    Int(i64),
    Str(String),
    Bool(bool),
}

impl std::fmt::Display for Literal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Literal::Int(n) => write!(f, "{n}"),
            Literal::Str(s) => write!(f, "\"{s}\""),
            Literal::Bool(b) => write!(f, "{b}"),
        }
    }
}

/// **A table, as its declaration states it.** Each clause as written; one
/// whose value its grammar does not read is in `unread`, refused by `check`
/// (PW0361), and left out here.
#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub decl: DeclId,
    /// Where its name is written.
    pub span: Span,
    pub row: Option<Clause<String>>,
    pub key: Option<Clause<Vec<String>>>,
    pub order: Option<Clause<String>>,
    pub indexes: Vec<Clause<IndexClause>>,
    pub references: Vec<Clause<Reference>>,
    pub bounded: Option<Clause<u64>>,
    pub migrations: Vec<Clause<Migration>>,
    /// **Whose its rows are** (track `row-ownership`, ADR-0384 H1): `owned by
    /// owner`, the field holding its owner's id.
    pub owner: Option<Clause<String>>,
    /// **Who else reads them**: `shares with to`, the fields holding the id
    /// of each other principal who reads a row, as its seen row.
    pub shares: Option<Clause<Vec<String>>>,
    /// **What its owner alone reads**: `private sent, day`, the fields no
    /// other reader's seen row holds.
    pub private: Option<Clause<Vec<String>>>,
    /// Each clause whose value is not one its grammar reads, and why.
    pub unread: Vec<(Span, String)>,
}

impl Table {
    /// The owner's field, where the table is owned.
    pub fn owner_field(&self) -> Option<&str> {
        self.owner.as_ref().map(|o| o.value.as_str())
    }

    /// The fields whose principals read a row besides its owner.
    pub fn sharers(&self) -> &[String] {
        self.shares.as_ref().map_or(&[], |s| s.value.as_slice())
    }

    /// The fields its owner alone reads.
    pub fn private_fields(&self) -> &[String] {
        self.private.as_ref().map_or(&[], |p| p.value.as_slice())
    }
}

/// **The record a reader the table shares with is answered** (track
/// `row-ownership`): the row's name and `Seen`, `LikeSeen`.
pub fn seen_name(row: &str) -> String {
    format!("{row}Seen")
}

/// Is `s` a name: a letter or `_`, then letters, digits and `_`?
fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `owner, id`: names, by commas, each once.
fn names(value: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for n in value.split(',').map(str::trim) {
        if !is_name(n) {
            return Err(match n.is_empty() {
                true => "names a field, or several by commas".to_string(),
                false => format!("`{n}` is no field's name"),
            });
        }
        if out.iter().any(|o| o == n) {
            return Err(format!("names `{n}` twice"));
        }
        out.push(n.to_string());
    }
    Ok(out)
}

/// `by_list on list, done`.
fn index_clause(value: &str, unique: bool) -> Result<IndexClause, String> {
    let Some((name, fields)) = value.split_once(" on ") else {
        return Err("is written `<name> on <fields>`: `by_owner on owner`".to_string());
    };
    let name = name.trim();
    if !is_name(name) {
        return Err(format!("`{name}` is no index's name"));
    }
    Ok(IndexClause {
        name: name.to_string(),
        fields: names(fields)?,
        unique,
    })
}

/// `owner to Users`.
fn reference(value: &str) -> Result<Reference, String> {
    let Some((field, table)) = value.split_once(" to ") else {
        return Err("is written `<field> to <table>`: `owner to Users`".to_string());
    };
    let (field, table) = (field.trim(), table.trim());
    if !is_name(field) {
        return Err(format!("`{field}` is no field's name"));
    }
    if !is_name(table) {
        return Err(format!("`{table}` is no table's name"));
    }
    Ok(Reference {
        field: field.to_string(),
        table: table.to_string(),
    })
}

/// `0`, `-3`, `"none"`, `true`.
fn literal(value: &str) -> Option<Literal> {
    let v = value.trim();
    match v {
        "true" => return Some(Literal::Bool(true)),
        "false" => return Some(Literal::Bool(false)),
        _ => {}
    }
    if let Some(inner) = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        // A string with no escape and no quote in it: what a backfill needs,
        // and nothing a reader has to decode twice.
        return (!inner.contains(['"', '\\'])).then(|| Literal::Str(inner.to_string()));
    }
    let digits = v.strip_prefix('-').unwrap_or(v);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| v.parse::<i64>().ok().map(Literal::Int))
        .flatten()
}

/// `title renamed from name`, `notes dropped`, `rank backfilled with 0`.
fn migration(value: &str) -> Result<Migration, String> {
    const FORMS: &str = "is written `<field> renamed from <old>`, `<field> dropped` or `<field> \
                         backfilled with <literal>`";
    let value = value.trim();
    let (field, rest) = value.split_once(' ').unwrap_or((value, ""));
    if !is_name(field) {
        return Err(FORMS.to_string());
    }
    let rest = rest.trim();
    let change = if rest == "dropped" {
        Change::Dropped
    } else if let Some(from) = rest.strip_prefix("renamed from ") {
        let from = from.trim();
        if !is_name(from) {
            return Err(format!("`{from}` is no field's name"));
        }
        Change::Renamed {
            from: from.to_string(),
        }
    } else if let Some(with) = rest.strip_prefix("backfilled with ") {
        match literal(with) {
            Some(l) => Change::Backfilled(l),
            None => {
                return Err(format!(
                    "`{}` is no literal: a backfill is an integer, a string or `true` or `false`",
                    with.trim()
                ));
            }
        }
    } else {
        return Err(FORMS.to_string());
    };
    Ok(Migration {
        field: field.to_string(),
        change,
    })
}

/// `by owner`: the one field holding a row's owner's id.
fn owned_by(value: &str) -> Result<String, String> {
    match value.trim().strip_prefix("by ").map(str::trim) {
        Some(field) if is_name(field) => Ok(field.to_string()),
        _ => Err(
            "is written `owned by <field>`, one field holding the id of the principal \
                  whose row it is: `owned by owner`"
                .to_string(),
        ),
    }
}

/// `with to, cc`: the fields holding the ids of the principals who read a row
/// besides its owner.
fn shared_with(value: &str) -> Result<Vec<String>, String> {
    match value.trim().strip_prefix("with ") {
        Some(fields) => names(fields),
        None => Err(
            "is written `shares with <fields>`, each holding the id of a principal \
                     who reads the row: `shares with to`"
                .to_string(),
        ),
    }
}

/// **Every table `hir` declares**, as written.
pub fn declared(hir: &Hir) -> Vec<Table> {
    hir.all_decls()
        .filter(|(_, d)| d.kind == DeclKind::Table)
        .map(|(id, d)| read(id, d))
        .collect()
}

fn read(id: DeclId, d: &Decl) -> Table {
    let mut t = Table {
        name: d.name.clone(),
        decl: id,
        span: d.name_span.clone(),
        row: None,
        key: None,
        order: None,
        indexes: Vec::new(),
        references: Vec::new(),
        bounded: None,
        migrations: Vec::new(),
        owner: None,
        shares: None,
        private: None,
        unread: Vec::new(),
    };
    for p in &d.policies {
        let v = p.value.trim();
        let span = p.span.clone();
        let clause = |value| Clause {
            value,
            span: span.clone(),
        };
        let fault = match p.name.as_str() {
            "row" => match is_name(v) {
                true => {
                    t.row = Some(clause(v.to_string()));
                    None
                }
                false => Some("names the record type of its rows: `row Todo`".to_string()),
            },
            "key" => match names(v) {
                Ok(fields) => {
                    t.key = Some(Clause {
                        value: fields,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            "order" => match is_name(v) {
                true => {
                    t.order = Some(clause(v.to_string()));
                    None
                }
                false => Some("names one field of the row: `order added`".to_string()),
            },
            "index" | "unique" => match index_clause(v, p.name == "unique") {
                Ok(i) => {
                    t.indexes.push(Clause {
                        value: i,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            "refers" => match reference(v) {
                Ok(r) => {
                    t.references.push(Clause {
                        value: r,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            // A count, or PW0335's (`Domain::Count`).
            "bounded" => {
                if let Some(n) = crate::policy::count(v) {
                    t.bounded = Some(Clause {
                        value: n,
                        span: span.clone(),
                    });
                }
                None
            }
            "migrate" => match migration(v) {
                Ok(m) => {
                    t.migrations.push(Clause {
                        value: m,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            // Track `row-ownership` (ADR-0384 H1). A second of any of these
            // is ADR-0098's, as every clause written twice is.
            "owned" => match owned_by(v) {
                Ok(field) => {
                    t.owner.get_or_insert(clause(field));
                    None
                }
                Err(why) => Some(why),
            },
            "shares" => match shared_with(v) {
                Ok(fields) => {
                    t.shares.get_or_insert(Clause {
                        value: fields,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            "private" => match names(v) {
                Ok(fields) => {
                    t.private.get_or_insert(Clause {
                        value: fields,
                        span: span.clone(),
                    });
                    None
                }
                Err(why) => Some(why),
            },
            _ => None,
        };
        if let Some(why) = fault {
            t.unread.push((span, format!("`{} {v}` {why}", p.name)));
        }
    }
    t
}

// --- the functions it gives --------------------------------------------------

/// `added_at` -> `AddedAt`.
fn pascal(name: &str) -> String {
    name.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// **The record a set form's list holds for a tuple of fields**: the table's
/// name and each field's, `CartLinesOwnerItem`.
pub fn tuple_name(table: &str, fields: &[String]) -> String {
    format!(
        "{table}{}",
        fields.iter().map(|f| pascal(f)).collect::<String>()
    )
}

/// **The read of a prefix of the key**: `of_owner`, `of_owner_store`.
pub fn prefix_read(fields: &[String]) -> String {
    format!("of_{}", fields.join("_"))
}

/// What a function is given, answers and is bound to, before it is a
/// declaration.
#[derive(Debug, Clone)]
struct Planned {
    name: String,
    params: Vec<(String, DeclaredType)>,
    ret: DeclaredType,
    primitive: Primitive,
    by: Vec<String>,
    span: Span,
}

/// **Who reads a table's function** (track `row-ownership`, ADR-0384 H1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// An unowned table's: by plain values, as D1 gives it.
    Plain,
    /// An owned table's, by its owner's handle in the owner's place: the row.
    Owner,
    /// An owned table's, by the handle of a principal it shares with, in
    /// that field's place: the seen row where any field is private.
    Sharer,
    /// An owned table's that leads with no reader: not given (PW0368).
    Refused,
}

/// **Who reads by `by`** on `t`: an owned table's reads lead with its owner
/// or a field it shares with.
fn reading(t: &Table, by: &[String]) -> Reading {
    let Some(owner) = t.owner_field() else {
        return Reading::Plain;
    };
    match by.first() {
        Some(f) if f == owner => Reading::Owner,
        Some(f) if t.sharers().contains(f) => Reading::Sharer,
        _ => Reading::Refused,
    }
}

/// **The tuples a table's set forms hold**, each of more than one field: of
/// an unowned table, its key's prefixes and its indexes; of an owned table,
/// what follows the reader in each (track `row-ownership`), whose handle is
/// given beside the list rather than in it.
fn set_tuples(t: &Table) -> Vec<(Vec<String>, Span)> {
    let mut out: Vec<(Vec<String>, Span)> = Vec::new();
    let mut selectors: Vec<(Vec<String>, Span)> = Vec::new();
    if let Some(key) = &t.key {
        for end in 1..=key.value.len() {
            selectors.push((key.value[..end].to_vec(), key.span.clone()));
        }
    }
    for i in &t.indexes {
        selectors.push((i.value.fields.clone(), i.span.clone()));
    }
    for (by, span) in selectors {
        let held: Vec<String> = match reading(t, &by) {
            Reading::Plain => by,
            Reading::Owner | Reading::Sharer => by[1..].to_vec(),
            Reading::Refused => continue,
        };
        if held.len() > 1 && !out.iter().any(|(f, _)| *f == held) {
            out.push((held, span));
        }
    }
    out
}

/// **The functions and records a table gives, in the order it gives them**,
/// over its row's fields as written, and the seen row where it gives one.
/// What its clauses do not read, or name outside its row, gives nothing;
/// `check` refuses it.
///
/// **An owned table's** (track `row-ownership`, ADR-0384 H1): each read
/// leading with its owner takes the reader's handle (`capability.User`) in
/// the owner's place and answers the row; each leading with a field it
/// shares with takes the handle there and answers the seen row; a set form
/// takes the handle and a list of what follows it, and none is given where
/// nothing follows; no read is given that leads with neither, nor `all`; its
/// writes take its owner's handle. So no read or write of its rows names a
/// principal by a plain id, and a program makes no handle (ADR-0263).
fn planned(
    t: &Table,
    row: &str,
    fields: &[(String, DeclaredType)],
) -> (Vec<Planned>, Vec<(Decl, Wrote)>, Option<Decl>) {
    let ty = |f: &str| fields.iter().find(|(n, _)| n == f).map(|(_, t)| t.clone());
    let all_in_row = |fs: &[String]| fs.iter().all(|f| ty(f).is_some());
    let row_ty = DeclaredType::new(row, Vec::new());
    let list = |t: DeclaredType| DeclaredType::new("List", vec![t]);
    let option = |t: DeclaredType| DeclaredType::new("Option", vec![t]);
    let unit = || DeclaredType::new("()", Vec::new());
    let refused = || DeclaredType::new("Result", vec![unit(), row_ty.clone()]);
    let params = |fs: &[String]| -> Vec<(String, DeclaredType)> {
        fs.iter()
            .filter_map(|f| Some((f.clone(), ty(f)?)))
            .collect()
    };
    // The reader's handle in a field's place: the platform's `User` over the
    // field's own type, which `owned by` and `shares with` hold to be the
    // platform's `UserId` (PW0367).
    let handle = |f: &str| DeclaredType::new("capability.User", vec![ty(f).unwrap_or_else(unit)]);
    // The seen row, where a reader the table shares with is answered one.
    let private: Vec<String> = t
        .private_fields()
        .iter()
        .filter(|p| ty(p).is_some())
        .cloned()
        .collect();
    let seen = match (&t.row, &t.owner) {
        (Some(r), Some(_)) if !t.sharers().is_empty() && !private.is_empty() => {
            let kept: Vec<(String, DeclaredType)> = fields
                .iter()
                .filter(|(n, _)| !private.contains(n))
                .cloned()
                .collect();
            Some(record(&seen_name(row), &kept, &r.span))
        }
        _ => None,
    };
    let answer = |r: Reading| match (r, &seen) {
        (Reading::Sharer, Some(s)) => DeclaredType::new(s.name.clone(), Vec::new()),
        _ => row_ty.clone(),
    };
    // What a keyed function is given: its fields, the reader's handle in the
    // place of the first where it has a reader.
    let keyed = |r: Reading, by: &[String]| -> Vec<(String, DeclaredType)> {
        match r {
            Reading::Owner | Reading::Sharer => std::iter::once((by[0].clone(), handle(&by[0])))
                .chain(params(&by[1..]))
                .collect(),
            _ => params(by),
        }
    };
    let mut records: Vec<(Decl, Wrote)> = Vec::new();
    let mut tuple = |fs: &[String], span: &Span| -> DeclaredType {
        if let [one] = fs {
            return ty(one).unwrap_or_else(unit);
        }
        let name = tuple_name(&t.name, fs);
        if !records.iter().any(|(r, _)| r.name == name) {
            records.push((
                record(&name, &params(fs), span),
                Wrote::Tuple {
                    fields: fs.to_vec(),
                },
            ));
        }
        DeclaredType::new(name, Vec::new())
    };
    // What a set form is given: one list of what it reads by, or, with a
    // reader, the handle and a list of what follows it; none where nothing
    // follows the reader, whose one value the keyed read is given already.
    let mut many = |r: Reading, by: &[String], list_name: &str, span: &Span| match r {
        Reading::Plain => Some(vec![(list_name.to_string(), list(tuple(by, span)))]),
        Reading::Owner | Reading::Sharer if by.len() > 1 => Some(vec![
            (by[0].clone(), handle(&by[0])),
            (list_name.to_string(), list(tuple(&by[1..], span))),
        ]),
        _ => None,
    };
    let mut out: Vec<Planned> = Vec::new();
    let mut push = |p: Planned| {
        if !out.iter().any(|o| o.name == p.name) {
            out.push(p);
        }
    };
    let key = t
        .key
        .as_ref()
        .filter(|k| !k.value.is_empty() && all_in_row(&k.value));
    let prefixes = |k: &Clause<Vec<String>>| -> Vec<Vec<String>> {
        (1..k.value.len()).map(|j| k.value[..j].to_vec()).collect()
    };
    // The reads, by the key and its prefixes.
    if let Some(k) = key {
        let r = reading(t, &k.value);
        if r != Reading::Refused {
            push(Planned {
                name: "get".to_string(),
                params: keyed(r, &k.value),
                ret: option(answer(r)),
                primitive: Primitive::Get,
                by: k.value.clone(),
                span: k.span.clone(),
            });
            if let Some(params) = many(r, &k.value, "keys", &k.span) {
                push(Planned {
                    name: "get_many".to_string(),
                    params,
                    ret: list(answer(r)),
                    primitive: Primitive::GetMany,
                    by: k.value.clone(),
                    span: k.span.clone(),
                });
            }
            for prefix in prefixes(k) {
                let name = prefix_read(&prefix);
                push(Planned {
                    name: name.clone(),
                    params: keyed(r, &prefix),
                    ret: list(answer(r)),
                    primitive: Primitive::Prefix,
                    by: prefix.clone(),
                    span: k.span.clone(),
                });
                if let Some(params) = many(r, &prefix, "prefixes", &k.span) {
                    push(Planned {
                        name: format!("{name}_many"),
                        params,
                        ret: list(answer(r)),
                        primitive: Primitive::PrefixMany,
                        by: prefix,
                        span: k.span.clone(),
                    });
                }
            }
        }
    }
    // By each index: an index's name says it reads by an index, `by_`, so it
    // is none of the other functions' names.
    for i in &t.indexes {
        let index = &i.value;
        if !index.name.starts_with("by_") || index.fields.is_empty() || !all_in_row(&index.fields) {
            continue;
        }
        let r = reading(t, &index.fields);
        if r == Reading::Refused {
            continue;
        }
        push(Planned {
            name: index.name.clone(),
            params: keyed(r, &index.fields),
            ret: match index.unique {
                true => option(answer(r)),
                false => list(answer(r)),
            },
            primitive: Primitive::Index,
            by: index.fields.clone(),
            span: i.span.clone(),
        });
        if let Some(params) = many(r, &index.fields, "values", &i.span) {
            push(Planned {
                name: format!("{}_many", index.name),
                params,
                ret: list(answer(r)),
                primitive: Primitive::IndexMany,
                by: index.fields.clone(),
                span: i.span.clone(),
            });
        }
    }
    // Counts use only the selectors the declaration already gives. A set
    // count includes a zero for every absent distinct selector and carries
    // that selector beside its scalar count.
    let mut selectors: Vec<(Vec<String>, Span)> = Vec::new();
    if let Some(k) = key {
        selectors.extend((1..=k.value.len()).map(|n| (k.value[..n].to_vec(), k.span.clone())));
    }
    selectors.extend(
        t.indexes
            .iter()
            .filter(|i| !i.value.fields.is_empty() && all_in_row(&i.value.fields))
            .map(|i| (i.value.fields.clone(), i.span.clone())),
    );
    let mut counted_records: Vec<(Decl, Wrote)> = Vec::new();
    let mut counted = BTreeSet::new();
    for (by, span) in selectors {
        let r = reading(t, &by);
        if r == Reading::Refused || !counted.insert(by.clone()) {
            continue;
        }
        let name = format!("count_by_{}", by.join("_"));
        push(Planned {
            name: name.clone(),
            params: keyed(r, &by),
            ret: DeclaredType::new("Int", Vec::new()),
            primitive: Primitive::Count,
            by: by.clone(),
            span: span.clone(),
        });
        if let Some(given) = many(r, &by, "values", &span) {
            let key_ty = if let [one] = by.as_slice() {
                ty(one).unwrap_or_else(unit)
            } else {
                let name = tuple_name(&t.name, &by);
                counted_records.push((
                    record(&name, &params(&by), &span),
                    Wrote::Tuple { fields: by.clone() },
                ));
                DeclaredType::new(name, Vec::new())
            };
            let count_name = format!("{}Count", tuple_name(&t.name, &by));
            counted_records.push((
                record(
                    &count_name,
                    &[
                        ("key".into(), key_ty),
                        ("count".into(), DeclaredType::new("Int", Vec::new())),
                    ],
                    &span,
                ),
                Wrote::Counted { fields: by.clone() },
            ));
            push(Planned {
                name: format!("{name}_many"),
                params: given,
                ret: list(DeclaredType::new(count_name, Vec::new())),
                primitive: Primitive::CountMany,
                by,
                span,
            });
        }
    }
    for (record, what) in counted_records {
        if !records
            .iter()
            .any(|(r, existing)| r.name == record.name && existing == &what)
        {
            records.push((record, what));
        }
    }
    // Whole, only where bounded (PW0364), and never an owned table: `all`
    // would read every owner's rows (PW0368).
    if let Some(b) = &t.bounded
        && t.owner.is_none()
    {
        push(Planned {
            name: "all".to_string(),
            params: Vec::new(),
            ret: list(row_ty.clone()),
            primitive: Primitive::All,
            by: Vec::new(),
            span: b.span.clone(),
        });
    }
    // The writes: an owned table's by its owner's handle, beside the row
    // whose owner the host holds to be the handle's (ADR-0384's `WITH
    // CHECK`), or in the owner's place in a key that leads with it.
    let owner = t.owner_field().filter(|o| ty(o).is_some());
    if let Some(r) = &t.row {
        let unique = t
            .indexes
            .iter()
            .any(|i| i.value.unique && all_in_row(&i.value.fields));
        let given = |row: DeclaredType| -> Vec<(String, DeclaredType)> {
            match owner {
                Some(o) => vec![(o.to_string(), handle(o)), ("row".to_string(), row)],
                None => vec![("row".to_string(), row)],
            }
        };
        push(Planned {
            name: "insert".to_string(),
            params: given(row_ty.clone()),
            ret: refused(),
            primitive: Primitive::Insert,
            by: Vec::new(),
            span: r.span.clone(),
        });
        push(Planned {
            name: "put".to_string(),
            params: given(row_ty.clone()),
            ret: match unique {
                true => refused(),
                false => unit(),
            },
            primitive: Primitive::Put,
            by: Vec::new(),
            span: r.span.clone(),
        });
    }
    if let Some(k) = key {
        // An owned table's deletes are its owner's: by a key that leads
        // with the owner (PW0368), the handle in its place.
        let r = reading(t, &k.value);
        if matches!(r, Reading::Plain | Reading::Owner) {
            push(Planned {
                name: "delete".to_string(),
                params: keyed(r, &k.value),
                ret: unit(),
                primitive: Primitive::Delete,
                by: k.value.clone(),
                span: k.span.clone(),
            });
            for prefix in prefixes(k) {
                push(Planned {
                    name: format!("delete_{}", prefix_read(&prefix)),
                    params: keyed(r, &prefix),
                    ret: unit(),
                    primitive: Primitive::DeletePrefix,
                    by: prefix,
                    span: k.span.clone(),
                });
            }
        }
    }
    (out, records, seen)
}

/// A record declaration, as `type Name = Name { a: A, .. }` lowers.
fn record(name: &str, fields: &[(String, DeclaredType)], span: &Span) -> Decl {
    Decl {
        name: name.to_string(),
        name_span: span.clone(),
        kind: DeclKind::Type,
        params: Vec::new(),
        ret: None,
        variants: None,
        fields: Some(
            fields
                .iter()
                .map(|(n, t)| Param {
                    name: n.clone(),
                    ty: Some(t.clone()),
                    span: span.clone(),
                })
                .collect(),
        ),
        policies: Vec::new(),
        imports: Vec::new(),
        visibility: None,
        opaque_of: None,
        invariant: None,
        type_params: Vec::new(),
        declared_effects: None,
        body: None,
        terms: None,
        children: Vec::new(),
        mutable: false,
    }
}

/// **The import a table's function is bound to**: `pw:tables/<table>#<name>`,
/// each part a WIT identifier (ADR-0262).
pub fn import_of(table: &str, function: &str) -> String {
    format!("{}#{}", interface_of(table), crate::wit::ident(function))
}

/// The emitted interface identity, shared by metadata and collision checks.
pub fn interface_of(table: &str) -> String {
    format!("pw:tables/{}", crate::wit::ident(table))
}

/// A function bound to its import, as `fn name(..) -> R !{ e<T> } host ".."`
/// lowers.
fn function(table: &str, p: &Planned) -> Decl {
    let effect = match p.primitive.writes() {
        true => "database.write",
        false => "database.read",
    };
    Decl {
        name: p.name.clone(),
        name_span: p.span.clone(),
        kind: DeclKind::Fn,
        params: p
            .params
            .iter()
            .map(|(n, t)| Param {
                name: n.clone(),
                ty: Some(t.clone()),
                span: p.span.clone(),
            })
            .collect(),
        ret: Some(p.ret.clone()),
        variants: None,
        fields: None,
        policies: vec![Policy {
            name: "host".to_string(),
            value: format!("\"{}\"", import_of(table, &p.name)),
            roots: Vec::new(),
            keys: Vec::new(),
            span: p.span.clone(),
        }],
        imports: Vec::new(),
        visibility: None,
        opaque_of: None,
        invariant: None,
        type_params: Vec::new(),
        declared_effects: Some(vec![EffectRef {
            path: effect.to_string(),
            written: format!("{effect}<{table}>"),
            args: vec![table.to_string()],
            span: p.span.clone(),
        }]),
        body: None,
        terms: None,
        children: Vec::new(),
        mutable: false,
    }
}

/// **The fields of the record `row` declared at the top of `hir`'s module**,
/// as written, in order. `None` where no such record is declared there.
fn row_fields(hir: &Hir, row: &str) -> Option<Vec<(String, DeclaredType)>> {
    let (_, module, _) = hir.modules.iter().next()?;
    module.decls.iter().find_map(|id| {
        let d = hir.decl(*id);
        if d.kind != DeclKind::Type || d.name != row {
            return None;
        }
        d.fields.as_ref().map(|fs| {
            fs.iter()
                .filter_map(|f| Some((f.name.clone(), f.ty.clone()?)))
                .collect()
        })
    })
}

/// **Write each table's functions into its module** (ADR-0338, 1.2): the
/// lowering's last step, so every way a file is read sees them as the
/// module's own declarations. A table declared outside the module of its
/// name, or the second of one name, gives nothing, which `check` refuses
/// (PW0361).
pub fn generate(hir: &mut Hir) {
    let Some((index, module, _)) = hir.modules.iter().next() else {
        return;
    };
    let (index, module) = (index, module.name.clone());
    let Some(t) = declared(hir).into_iter().find(|t| t.name == module) else {
        return;
    };
    let Some(row) = t.row.as_ref().map(|r| r.value.clone()) else {
        return;
    };
    let Some(fields) = row_fields(hir, &row) else {
        return;
    };
    let (functions, records, seen) = planned(&t, &row, &fields);
    let mut wrote: Vec<(Decl, Span, Wrote)> = Vec::new();
    for (r, what) in records {
        let span = r.name_span.clone();
        wrote.push((r, span, what));
    }
    // A reader the table shares with is answered its seen row (track
    // `row-ownership`), declared beside the row as a tuple is.
    if let Some(s) = seen {
        let fields = s.fields.iter().flatten().map(|f| f.name.clone()).collect();
        let span = s.name_span.clone();
        wrote.push((s, span, Wrote::Seen { fields }));
    }
    for p in &functions {
        wrote.push((
            function(&t.name, p),
            p.span.clone(),
            Wrote::Function {
                primitive: p.primitive,
                by: p.by.clone(),
            },
        ));
    }
    for (decl, span, what) in wrote {
        let id = DeclId(hir.decls.alloc(decl, span));
        if let Some(m) = hir.modules.get_mut(index) {
            m.decls.push(id);
        }
        hir.generated.insert(
            id,
            Generated {
                table: t.decl,
                what,
            },
        );
    }
}

// --- the checks --------------------------------------------------------------

/// **Can every adapter key on a value of `ty`?** (ADR-0338, PW0361): an
/// `Int`, a `String`, a `Bool`, an opaque type over one, or a type of cases
/// none of which carries a value (stored as the case's name). `Err` says what
/// it is instead.
pub fn keyable(sigs: &Signatures, ty: &ResolvedType) -> Result<(), String> {
    keyable_at(sigs, ty, 0)
}

fn keyable_at(sigs: &Signatures, ty: &ResolvedType, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err(format!("`{ty}`, whose representation does not end"));
    }
    if let Some(p) = ty.as_primitive() {
        return match p {
            Prim::Int | Prim::Str | Prim::Bool => Ok(()),
            Prim::Float => Err(
                "a `Float`, whose equality is no key's: two readings of one number can differ"
                    .to_string(),
            ),
            Prim::Unit => Err("`()`, which tells no row from another".to_string()),
        };
    }
    if let Some(b) = ty.as_builtin() {
        return Err(format!(
            "`{ty}`, a{} `{}`, which no database keys a row by",
            if b.name().starts_with(['O']) { "n" } else { "" },
            b.name()
        ));
    }
    let Some(def) = ty.def_id() else {
        return Err(format!("`{ty}`, a type parameter"));
    };
    let Some(decl) = sigs.type_decl(def) else {
        return Err(format!(
            "`{ty}`, which no declaration gives a representation"
        ));
    };
    if let Some(rep) = &decl.representation {
        return match rep.resolved() {
            Some(r) => keyable_at(sigs, r, depth + 1).map_err(|why| format!("`{ty}`, over {why}")),
            None => Err(format!("`{ty}`, whose representation does not resolve")),
        };
    }
    if let Some(cases) = &decl.variants {
        return match cases.iter().find(|(_, payload)| !payload.is_empty()) {
            None => Ok(()),
            Some((case, _)) => Err(format!(
                "`{ty}`, whose case `{case}` carries a value, and no database keys a row by one"
            )),
        };
    }
    Err(format!("`{ty}`, a record, which no database keys a row by"))
}

/// **The module `name` and the table it declares**, where `unit` sees it.
fn table_module(ws: &Workspace, hirs: &[&Hir], unit: usize, name: &str) -> Option<(usize, Table)> {
    let m = ws.visible_module(unit, name)?;
    let hir = hirs.get(m.unit)?;
    let t = declared(hir).into_iter().find(|t| t.name == name)?;
    Some((m.unit, t))
}

/// **The source that holds the table `name`**, by the program's sources'
/// `holds`; `None` for the host's own database (ADR-0207).
pub fn source_of(hirs: &[&Hir], name: &str) -> Option<String> {
    hirs.iter().find_map(|hir| {
        hir.all_decls().find_map(|(_, d)| {
            (d.kind == DeclKind::Source
                && d.policy("holds")
                    .is_some_and(|p| p.value.split(',').map(str::trim).any(|h| h == name)))
            .then(|| d.name.clone())
        })
    })
}

fn source_named(source: &Option<String>) -> String {
    source
        .as_ref()
        .map_or_else(|| "the host's database".to_string(), |s| format!("`{s}`"))
}

/// Source identity is still a bare name at the existing host seam. Until
/// that seam carries qualified identities, refuse an ambiguous spelling
/// where a table or one of its references would use it (D1 interim guard).
fn unambiguous_sources(hirs: &[&Hir], unit: usize, ws: &Workspace, t: &Table) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    // Duplicate modules are already refused by resolution (PW0024). Another
    // copy of this unit's own module must not add a database to its diagnostics.
    // Distinct modules, including ones this unit does not import, still count.
    let alternate_own_module = |source_unit: usize| {
        source_unit != unit
            && ws
                .module_of(source_unit)
                .and_then(|m| ws.visible_module(unit, &m.name))
                .is_some_and(|m| m.unit == unit)
    };
    for (table, span) in std::iter::once((&t.name, &t.span))
        .chain(t.references.iter().map(|r| (&r.value.table, &r.span)))
    {
        let Some(name) = source_of(hirs, table) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let sources: Vec<_> = hirs
            .iter()
            .enumerate()
            .filter(|(source_unit, _)| !alternate_own_module(*source_unit))
            .flat_map(|(unit, hir)| {
                hir.all_decls()
                    .filter(|(_, d)| d.kind == DeclKind::Source && d.name == name)
                    .map(move |(id, _)| (unit, hir.decl_span(id)))
            })
            .collect();
        if sources.len() > 1 {
            out.push(malformed(
                format!("`{name}` names {} source declarations: `{table}`'s database cannot be identified by this bare name", sources.len()),
                span.clone(),
                sources.into_iter().map(|(unit, span)| Related::in_unit(unit, span,
                    format!("source `{}.{name}`", ws.module_of(unit).map_or("?", |m| m.name.as_str())))).collect(),
                "name each source distinctly; the host seam needs qualified source identity before duplicate names can be accepted".into(),
            ));
        }
    }
    out
}

/// Distinct Pleris names must stay distinct at the emitted ABI boundary.
/// Compare the identities the emitter uses, never a second spelling rule.
fn distinct_imports(hirs: &[&Hir], unit: usize, t: &Table) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let interface = interface_of(&t.name);
    for (other_unit, other) in every(hirs) {
        if other.name != t.name && interface_of(&other.name) == interface {
            out.push(malformed(
                format!(
                    "`{}` and `{}` emit the same interface `{interface}`",
                    t.name, other.name
                ),
                t.span.clone(),
                vec![Related::in_unit(
                    other_unit,
                    other.span,
                    "the other table is declared here",
                )],
                "rename one table and its module so their emitted WIT interface identities differ"
                    .into(),
            ));
        }
    }
    let hir = hirs[unit];
    let mut seen = BTreeMap::new();
    for (id, _, _) in functions_of(hir, t.decl) {
        let d = hir.decl(id);
        let import = import_of(&t.name, &d.name);
        if let Some((first, span)) =
            seen.insert(import.clone(), (d.name.clone(), d.name_span.clone()))
        {
            out.push(malformed(
                format!("`{first}` and `{}` emit the same import `{import}`", d.name),
                d.name_span.clone(),
                vec![Related::new(span, "the first function is generated here")],
                "rename an index so the generated functions' emitted WIT import identities differ"
                    .into(),
            ));
        }
    }
    out
}

/// One table refusal.
#[allow(clippy::too_many_arguments)]
fn refusal(
    code: codes::Code,
    reason: &'static str,
    severity: Severity,
    message: String,
    span: Span,
    related: Vec<Related>,
    explanation: &str,
    repair: String,
) -> Diagnostic {
    Diagnostic {
        code: code.id,
        invariant: code.invariant,
        reason,
        detector: Detector::DeclarationRule,
        severity,
        message,
        primary_span: span,
        related,
        explanation: Some(explanation.to_string()),
        repairs: vec![Repair {
            description: repair,
            replacement: None,
        }],
    }
}

const MALFORMED: &str = "A table is the program's statement of its data: its row is a record \
     type declared beside it, in the module of its name, whose functions are its reads and \
     writes; its key, order and indexes name the row's fields, each of a type every adapter keys \
     a row by (an `Int`, a `String`, a `Bool`, an opaque type over one, or cases that carry no \
     value); and a reference names a table keyed by the field's type.";

fn malformed(message: String, span: Span, related: Vec<Related>, repair: String) -> Diagnostic {
    refusal(
        codes::TABLE_MALFORMED,
        "table_malformed",
        Severity::Error,
        message,
        span,
        related,
        MALFORMED,
        repair,
    )
}

/// **PW6901: a row holds a handle** (track `data-path`, the integrator's
/// ruling 3 of 2026-11-02). A handle is its reader's alone (ADR-0263), and
/// its value is a user's id, or a session's, whose id is its cookie's:
/// stored, every reader of the table reads it, with no label to say whose it
/// is. Found storing `current_user()` in a row read by a shared query, which
/// served another session `u-` and a guest's session.
fn holds_handle(t: &Table, row: &str, field: &str, whose: &str, span: Span) -> Diagnostic {
    refusal(
        codes::ROW_HOLDS_HANDLE,
        "row_holds_handle",
        Severity::Error,
        format!(
            "`{row}`'s field `{field}` holds {whose} handle, and `{}` stores it where every \
             reader of the table reads it",
            t.name
        ),
        span,
        Vec::new(),
        "A handle reads the data of the one it names, and only its own reader holds it: the \
         platform makes it from the request it answers (`current_user()`). A table's rows are \
         read by every reader whose reads reach the table, and a row keeps no label: a handle \
         stored there would be handed to them all. A row names someone by an id, which grants \
         nothing: the reader's own, from `context.account_of(current_user())`.",
        format!(
            "store an id in `{field}`: a `UserId`, such as the reader's account's \
             (`account_of(current_user())`'s `id`)"
        ),
    )
}

/// **The tables `unit` declares, and the bindings it writes, held to the
/// declaration** (ADR-0338, decision 4): PW0361, PW0362, PW0365 and, the
/// data-path track's, PW6901.
pub fn check(hirs: &[&Hir], unit: usize, ws: &Workspace, sigs: &Signatures) -> Vec<Diagnostic> {
    let Some(hir) = hirs.get(unit) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let module = ws
        .module_of(unit)
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let tables = declared(hir);
    let module_span = hir.modules.iter().next().map(|(_, _, s)| s.clone());
    for (i, t) in tables.iter().enumerate() {
        let mut found = shape(hirs, unit, ws, sigs, &module, t, i > 0);
        found.extend(unambiguous_sources(hirs, unit, ws, t));
        found.extend(distinct_imports(hirs, unit, t));
        if found.is_empty() {
            found.extend(across_sources(hirs, unit, ws, t));
        }
        // Each told beside the table it is about (charter §16.3's boundary
        // span), or, for the table itself, its module.
        for d in &mut found {
            if d.related.is_empty() {
                d.related.push(match d.primary_span == t.span {
                    true => Related::new(
                        module_span.clone().unwrap_or_else(|| t.span.clone()),
                        format!("the module `{module}`"),
                    ),
                    false => Related::new(t.span.clone(), format!("the table `{}`", t.name)),
                });
            }
        }
        out.extend(found);
    }
    out.extend(bindings(hirs, unit, ws));
    out
}

/// PW0361: the declaration's shape.
fn shape(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    sigs: &Signatures,
    module: &str,
    t: &Table,
    second: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (span, why) in &t.unread {
        out.push(malformed(
            why.clone(),
            span.clone(),
            Vec::new(),
            "write the clause as its grammar reads it".to_string(),
        ));
    }
    if t.name != module || second {
        out.push(malformed(
            match second {
                true => format!(
                    "`{}` is a second table in `{module}`: a module declares one table, of its \
                     name",
                    t.name
                ),
                false => format!(
                    "`{}` is declared in `{module}`: a table is declared in the module of its \
                     name, whose functions are its reads and writes",
                    t.name
                ),
            },
            t.span.clone(),
            Vec::new(),
            format!(
                "declare it in a module of its own, `module {}`, which the program imports to \
                 read and write it",
                t.name
            ),
        ));
    }
    // The row: a record type declared here.
    let Some(row) = &t.row else {
        out.push(malformed(
            format!("`{}` states no `row`: the record type of its rows", t.name),
            t.span.clone(),
            Vec::new(),
            "write `row` and a record type declared in this module".to_string(),
        ));
        return out;
    };
    let fields: Vec<(String, Option<ResolvedType>)> =
        match ws.resolve_in(unit, Namespace::Type, &row.value) {
            Resolution::Local(def) => {
                match sigs.type_decl(def).and_then(|d| d.record.as_ref()) {
                    Some(fields) => fields
                        .iter()
                        .map(|(n, r)| (n.clone(), r.resolved().cloned()))
                        .collect(),
                    None => {
                        out.push(malformed(
                    format!(
                        "`row {}` names a type that is no record: a row is a record of fields",
                        row.value
                    ),
                    row.span.clone(),
                    Vec::new(),
                    format!("declare `type {0} = {0} {{ .. }}` beside the table", row.value),
                ));
                        return out;
                    }
                }
            }
            Resolution::Imported { from, .. } => {
                out.push(malformed(
                    format!(
                        "`row {}` names a record declared in `{from}`: a table's row is declared \
                     beside it, so its fields are its own",
                        row.value
                    ),
                    row.span.clone(),
                    Vec::new(),
                    format!("declare the row's record in `{module}`"),
                ));
                return out;
            }
            _ => {
                out.push(malformed(
                    format!(
                        "`row {}` names no record type declared in `{module}`",
                        row.value
                    ),
                    row.span.clone(),
                    Vec::new(),
                    format!(
                        "declare `type {0} = {0} {{ .. }}` beside the table",
                        row.value
                    ),
                ));
                return out;
            }
        };
    // PW6901 (track `data-path`): no field holds a handle, wherever its type
    // holds it: a row is read by every reader of the table, and the table's
    // boundary keeps no label (a handle's value is its user's id, or the
    // session's own).
    for (name, ty) in &fields {
        if let Some(whose) = ty
            .as_ref()
            .and_then(|ty| crate::check::handle_in(sigs, ty, &mut BTreeSet::new()))
        {
            out.push(holds_handle(t, &row.value, name, whose, row.span.clone()));
        }
    }
    let field = |f: &str| fields.iter().find(|(n, _)| n == f);
    // Each field a clause names is the row's, of a type an adapter keys.
    let fields_of = |clause: &str, names: &[String], span: &Span, out: &mut Vec<Diagnostic>| {
        for n in names {
            match field(n) {
                None => out.push(malformed(
                    format!(
                        "`{clause}` names `{n}`, which is no field of `{}`",
                        row.value
                    ),
                    span.clone(),
                    Vec::new(),
                    format!("name a field of `{}`", row.value),
                )),
                Some((_, Some(ty))) => {
                    if let Err(why) = keyable(sigs, ty) {
                        out.push(malformed(
                            format!("`{clause}` names `{n}`, of {why}"),
                            span.clone(),
                            Vec::new(),
                            "key it by a field of an `Int`, a `String`, a `Bool`, an opaque \
                             type over one, or cases that carry no value"
                                .to_string(),
                        ));
                    }
                }
                // A type that resolves to nothing is PW0026's.
                Some((_, None)) => {}
            }
        }
    };
    match &t.key {
        None => out.push(malformed(
            format!(
                "`{}` states no `key`: the fields that identify a row",
                t.name
            ),
            t.span.clone(),
            Vec::new(),
            "write `key` and the fields that identify a row, its partition first".to_string(),
        )),
        Some(k) => fields_of("key", &k.value, &k.span, &mut out),
    }
    if let Some(o) = &t.order {
        fields_of("order", std::slice::from_ref(&o.value), &o.span, &mut out);
    }
    let mut seen: BTreeMap<String, Span> = BTreeMap::new();
    let mut field_lists: BTreeMap<Vec<String>, Span> = BTreeMap::new();
    for i in &t.indexes {
        let index = &i.value;
        let clause = if index.unique { "unique" } else { "index" };
        if let Some(first) = field_lists.get(&index.fields) {
            out.push(malformed(
                format!(
                    "two indexes of `{}` declare the same ordered fields: {}",
                    t.name,
                    index.fields.join(", ")
                ),
                i.span.clone(),
                vec![Related::new(
                    first.clone(),
                    "the first index on these fields is declared here",
                )],
                "keep one index on this ordered field list, unique or not".into(),
            ));
        } else {
            field_lists.insert(index.fields.clone(), i.span.clone());
        }
        if !index.name.starts_with("by_") {
            out.push(malformed(
                format!(
                    "`{clause} {}` is named as no index read is: an index's read is named \
                     `by_` and what it reads by",
                    index.name
                ),
                i.span.clone(),
                Vec::new(),
                format!("name it `by_{}`", index.fields.join("_")),
            ));
        }
        for name in [index.name.clone(), format!("{}_many", index.name)] {
            if let Some(first) = seen.get(&name) {
                out.push(malformed(
                    format!("`{name}` is the read of two of `{}`'s indexes", t.name),
                    i.span.clone(),
                    vec![Related::new(first.clone(), "the first is declared here")],
                    "name each index once, and none the set form of another".to_string(),
                ));
            }
        }
        seen.insert(index.name.clone(), i.span.clone());
        seen.insert(format!("{}_many", index.name), i.span.clone());
        fields_of(clause, &index.fields, &i.span, &mut out);
    }
    // Pascal-casing is readable but not injective: a_b,c and a,b_c both
    // name ABC. Never reuse a generated record for a different tuple: those
    // of the set forms the table gives (an owned table's hold what follows
    // its reader, track `row-ownership`).
    let mut shapes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (fields, span) in &set_tuples(t) {
        let name = tuple_name(&t.name, fields);
        if shapes
            .get(&name)
            .is_some_and(|first| first.as_slice() != fields)
        {
            out.push(malformed(
                format!(
                    "`{name}` would hold two different field tuples in `{}`",
                    t.name
                ),
                span.clone(),
                Vec::new(),
                "rename a field so the generated key and index record names are distinct".into(),
            ));
        }
        shapes.insert(name, fields.to_vec());
    }
    // Count results introduce full reader-bearing selector tuples and the
    // Count suffix. Check all emitted record meanings, including collisions
    // with ordinary selector/seen records, by their actual WIT identities.
    let hir = hirs[unit];
    let mut generated_shapes: BTreeMap<String, (&Wrote, Span)> = BTreeMap::new();
    for (id, generated) in &hir.generated {
        if generated.table != t.decl
            || !matches!(
                generated.what,
                Wrote::Tuple { .. } | Wrote::Counted { .. } | Wrote::Seen { .. }
            )
        {
            continue;
        }
        let decl = hir.decl(*id);
        let identity = crate::wit::ident(&decl.name);
        if let Some((first, span)) = generated_shapes.get(&identity)
            && *first != &generated.what
        {
            out.push(malformed(
                format!("generated records of `{}` emit the same type `{identity}` for different shapes", t.name),
                decl.name_span.clone(),
                vec![Related::new(span.clone(), "the first record is generated here")],
                "rename a field so selector and count record names are distinct".into(),
            ));
        }
        generated_shapes.insert(identity, (&generated.what, decl.name_span.clone()));
    }
    // Whose its rows are (track `row-ownership`): PW0367 to PW0369.
    out.extend(ownership(
        hirs.get(unit).copied(),
        sigs,
        t,
        &row.value,
        &fields,
    ));
    for r in &t.references {
        let malformed = referring(
            hirs,
            unit,
            ws,
            sigs,
            &row.value,
            r,
            field(&r.value.field).map(|(_, ty)| ty.as_ref()),
        );
        // Whose rows it reaches (track `row-ownership`): PW6924, of a
        // reference whose shape is a table's.
        if malformed.is_empty() {
            out.extend(within_one_owner(hirs, unit, ws, t, r));
        }
        out.extend(malformed);
    }
    let mut stated: BTreeMap<&str, &Span> = BTreeMap::new();
    for m in &t.migrations {
        let mig = &m.value;
        if let Some(first) = stated.insert(&mig.field, &m.span) {
            out.push(malformed(
                format!("`migrate` states two changes of `{}`", mig.field),
                m.span.clone(),
                vec![Related::new(first.clone(), "the first is stated here")],
                "state one change of each field".to_string(),
            ));
        }
        let in_row = field(&mig.field);
        let fault = match &mig.change {
            Change::Dropped => in_row.is_some().then(|| {
                format!(
                    "`migrate {} dropped` names a field `{}` still has",
                    mig.field, row.value
                )
            }),
            Change::Renamed { from } => match (in_row, field(from)) {
                (None, _) => Some(format!(
                    "`migrate {} renamed from {from}` names `{}`, which is no field of `{}`",
                    mig.field, mig.field, row.value
                )),
                (_, Some(_)) => Some(format!(
                    "`migrate {} renamed from {from}`: `{}` still has `{from}`",
                    mig.field, row.value
                )),
                _ => None,
            },
            Change::Backfilled(value) => match in_row {
                None => Some(format!(
                    "`migrate {} backfilled with {value}` names no field of `{}`",
                    mig.field, row.value
                )),
                Some((_, Some(ty))) => fits(sigs, ty, value)
                    .err()
                    .map(|why| format!("`migrate {} backfilled with {value}`: {why}", mig.field)),
                Some((_, None)) => None,
            },
        };
        if let Some(message) = fault {
            out.push(malformed(
                message,
                m.span.clone(),
                Vec::new(),
                "state the change the row makes from the schema released".to_string(),
            ));
        }
    }
    out
}

/// **Is `ty` the id of a principal**, the platform's `capability.UserId`
/// (ADR-0270): what an owned table's owner and sharers are named by.
pub fn principal_id(sigs: &Signatures, ty: &ResolvedType) -> bool {
    matches!(
        sigs.stable_type(ty),
        Some(crate::resolved::StableTypeId::Declared { path, args })
            if path == "capability.UserId" && args.is_empty()
    )
}

const OWNED: &str = "An owned table's rows are its owner's (ADR-0384): `owned by` names the \
     field holding the id of the principal whose row it is, a `capability.UserId`, and `shares \
     with` the fields holding the ids of the others who read it. Each read of its rows takes the \
     reader's handle in the place of one of those fields, a `capability.User` only the platform \
     makes (ADR-0263), so no program reads another's rows by naming an id; each write takes its \
     owner's handle, and the host holds the row's owner to it.";

const READ_BY_ITS_READER: &str = "An owned table is read by its reader alone (ADR-0384): each \
     read takes the reader's handle in the place of the field it leads with, its owner or one it \
     shares with, so a read that leads with any other field, or reads it whole, would answer \
     other owners' rows. A unique value is held across the table, so a unique index holds the \
     owner: a write refused by another owner's row would answer it. A bound counts the rows of \
     every owner, so an owned table states none: a write refused by it would tell one owner how \
     many rows the others hold.";

const WITHIN_ONE_OWNER: &str = "A reference is a foreign key the database holds across every \
     row of both tables, whoever owns them (ADR-0338; PostgreSQL's foreign keys are checked \
     past its row security): a row is not written naming a row that is not there, and a row \
     named is not deleted. Between two owners' rows that check is a read of another's: a write \
     refused, or let through, tells its writer whether a row they may not read exists, and an \
     owner could not delete their own row while another's names it. So a reference into an \
     owned table is made from an owned table's own owner field, naming its owner's own row; a \
     table no one owns refers to no owned table; and an owned table refers to no table no one \
     owns, since deleting the row it names would be refused because of a private row, which \
     the refusal reveals, until a reference states what deleting the row it names does \
     (ADR-0384's H3 and H9).";

const UNSEEN: &str = "A private field is read by its owner alone (ADR-0384): a reader the table \
     shares with is answered the seen row, the record the compiler declares beside the row \
     without the private fields, so no page, frame, cache entry or answer of theirs can hold \
     one. A field every reader selects or orders rows by, the owner, a sharer, the key, the order \
     and an index a sharer reads by, tells its values to them, and is not private.";

/// **PW0367 to PW0369: whose a table's rows are** (track `row-ownership`,
/// ADR-0384 H1): `owned by` and `shares with` name fields holding a
/// principal's id; every read leads with a reader; a private field is one
/// no other reader selects, orders or is answered by.
fn ownership(
    hir: Option<&Hir>,
    sigs: &Signatures,
    t: &Table,
    row: &str,
    fields: &[(String, Option<ResolvedType>)],
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let field = |f: &str| fields.iter().find(|(n, _)| n == f);
    let owner_refusal = |message: String, span: &Span, repair: &str| {
        refusal(
            codes::OWNER_NAMES_A_PRINCIPAL,
            "owner_names_a_principal",
            Severity::Error,
            message,
            span.clone(),
            Vec::new(),
            OWNED,
            repair.to_string(),
        )
    };
    let read_refusal = |message: String, span: &Span, repair: String| {
        refusal(
            codes::OWNED_READ_BY_ITS_READER,
            "owned_read_by_its_reader",
            Severity::Error,
            message,
            span.clone(),
            Vec::new(),
            READ_BY_ITS_READER,
            repair,
        )
    };
    let unseen = |message: String, span: &Span, repair: &str| {
        refusal(
            codes::PRIVATE_FIELD_UNSEEN,
            "private_field_unseen",
            Severity::Error,
            message,
            span.clone(),
            Vec::new(),
            UNSEEN,
            repair.to_string(),
        )
    };
    // A field holding a principal's id, or why it is not one.
    let principal = |name: &str| -> Result<(), String> {
        match field(name) {
            None => Err(format!("`{name}` is no field of `{row}`")),
            Some((_, Some(ty))) if !principal_id(sigs, ty) => Err(format!(
                "`{name}` is a `{}`, and a principal is named by its id, `capability.UserId`",
                ty.display_name()
            )),
            // A type that resolves to nothing is PW0026's.
            Some(_) => Ok(()),
        }
    };
    let Some(owned) = &t.owner else {
        if let Some(s) = &t.shares {
            out.push(owner_refusal(
                format!(
                    "`{}` shares its rows and no one owns them: `shares with` is an owned \
                     table's",
                    t.name
                ),
                &s.span,
                "state whose its rows are, `owned by <field>`, or read it as the program's \
                 public data",
            ));
        }
        if let Some(p) = &t.private {
            out.push(unseen(
                format!(
                    "`{}` keeps fields private and no one owns its rows: `private` is an \
                     owned table's",
                    t.name
                ),
                &p.span,
                "state whose its rows are, `owned by <field>`",
            ));
        }
        return out;
    };
    let owner = owned.value.as_str();
    if let Err(why) = principal(owner) {
        out.push(owner_refusal(
            format!("`owned by {owner}`: {why}"),
            &owned.span,
            "name the field of the row holding its owner's `capability.UserId`",
        ));
    }
    if let Some(s) = &t.shares {
        for sharer in &s.value {
            if sharer == owner {
                out.push(owner_refusal(
                    format!(
                        "`shares with {sharer}` names `{owner}`, its owner, who reads its rows \
                         already"
                    ),
                    &s.span,
                    "name the fields holding the ids of the others who read a row",
                ));
                continue;
            }
            if let Err(why) = principal(sharer) {
                // A name a predicate would have: who reads by a predicate
                // is `shares when`, H3's (ADR-0384 decision 1), not built.
                let predicate =
                    sharer.starts_with(|c: char| c.is_ascii_uppercase()) && field(sharer).is_none();
                out.push(owner_refusal(
                    match predicate {
                        true => format!(
                            "`shares with {sharer}`: {why}; a predicate deciding who reads \
                             a row is not built (ADR-0384's `shares when`, track H3)"
                        ),
                        false => format!("`shares with {sharer}`: {why}"),
                    },
                    &s.span,
                    "name the fields of the row holding the ids of the others who read it",
                ));
            }
        }
    }
    let sharers = t.sharers();
    let is_reader = |f: &String| f == owner || sharers.contains(f);
    // Every read leads with a reader: the key with the owner, each index
    // with the owner or a sharer.
    if let Some(k) = &t.key
        && k.value.first().is_some_and(|f| f != owner)
    {
        out.push(read_refusal(
            format!(
                "`key {}` leads with `{}`: an owned table's key leads with its owner, `{owner}`, \
                 so a read by it, a prefix of it or its whole is by the owner's handle",
                k.value.join(", "),
                k.value[0]
            ),
            &k.span,
            format!(
                "lead the key with `{owner}`: `key {owner}{}`",
                k.value
                    .iter()
                    .filter(|f| *f != owner)
                    .map(|f| format!(", {f}"))
                    .collect::<String>()
            ),
        ));
    }
    for i in &t.indexes {
        let index = &i.value;
        let clause = if index.unique { "unique" } else { "index" };
        if index.fields.first().is_some_and(|f| !is_reader(f)) {
            out.push(read_refusal(
                format!(
                    "`{clause} {} on {}` leads with `{}`, and a read by it would answer every \
                     owner's rows: an owned table's index leads with its owner or a field it \
                     shares with",
                    index.name,
                    index.fields.join(", "),
                    index.fields[0]
                ),
                &i.span,
                format!(
                    "lead it with `{owner}`, read by its owner's handle, or with a field it \
                     shares with"
                ),
            ));
        }
        if index.unique && !index.fields.iter().any(|f| f == owner) {
            out.push(read_refusal(
                format!(
                    "`unique {} on {}` holds no `{owner}`: a value unique across owners refuses \
                     one owner's write with another's row",
                    index.name,
                    index.fields.join(", ")
                ),
                &i.span,
                format!(
                    "make it unique per owner, `unique {} on {owner}, ..`, or keep the values \
                     unique across owners in a table no one owns",
                    index.name
                ),
            ));
        }
    }
    // A bound counts every owner's rows (the review's finding 2): one
    // owner's rows would refuse another's write, and the refusal tell them
    // how many the others hold. A bound per owner is ADR-0384's H4.
    if let Some(b) = &t.bounded {
        out.push(read_refusal(
            format!(
                "`bounded {}` counts every owner's rows of `{}`: one owner's rows would refuse \
                 another's write, and the refusal tell them how many rows the others hold",
                b.value, t.name
            ),
            &b.span,
            "remove `bounded`: an owned table is read by its reader's handle and never whole, \
             and a bound per owner is not built (ADR-0384's H4)"
                .to_string(),
        ));
    }
    // A private field: one its owner alone reads.
    let Some(p) = &t.private else {
        return out;
    };
    for name in &p.value {
        let fault = if field(name).is_none() {
            Some(format!("`private {name}` names no field of `{row}`"))
        } else if name == owner {
            Some(format!(
                "`private {name}` names its owner, which every reader reads its rows by"
            ))
        } else if sharers.contains(name) {
            Some(format!(
                "`private {name}` names a field it shares with, which that reader reads its rows \
                 by"
            ))
        } else if t.key.as_ref().is_some_and(|k| k.value.contains(name)) {
            Some(format!(
                "`private {name}` names a field of its key, which tells one row from another to \
                 every reader"
            ))
        } else if !sharers.is_empty() && t.order.as_ref().is_some_and(|o| o.value == *name) {
            Some(format!(
                "`private {name}` names its order: a reader it shares with is answered rows in \
                 `{name}`'s order, which tells them how its values compare"
            ))
        } else {
            // In an index a reader it shares with reads its rows by.
            t.indexes
                .iter()
                .find(|i| {
                    i.value.fields.contains(name)
                        && i.value.fields.first().is_some_and(|f| sharers.contains(f))
                })
                .map(|i| {
                    format!(
                        "`private {name}` is in `{}`, which a reader it shares with reads its \
                         rows by",
                        i.value.name
                    )
                })
        };
        if let Some(message) = fault {
            out.push(unseen(
                message,
                &p.span,
                "keep private only what no reader but the owner selects, orders or is answered \
                 rows by",
            ));
        }
    }
    // The seen row's name is the compiler's to declare.
    let seen = seen_name(row);
    if !sharers.is_empty()
        && let Some(hir) = hir
        && let Some((id, _)) = hir
            .all_decls()
            .find(|(id, d)| d.name == seen && !hir.generated.contains_key(id))
    {
        out.push(Diagnostic {
            related: vec![Related::new(
                hir.decl_span(id),
                format!("`{seen}` is declared here"),
            )],
            ..unseen(
                format!(
                    "`{seen}` is the record a reader `{}` shares with is answered, and the \
                     program declares a `{seen}` of its own",
                    t.name
                ),
                &p.span,
                "rename the program's declaration: the seen row is the compiler's",
            )
        });
    }
    out
}

/// **Does `value` fit a field of `ty`**, as every row's? An integer for an
/// `Int` (within an opaque type's bounds), a string for a `String`, `true`
/// or `false` for a `Bool`.
pub fn fits(sigs: &Signatures, ty: &ResolvedType, value: &Literal) -> Result<(), String> {
    let mut ty = ty.clone();
    let mut bounds: Vec<crate::hir::Invariant> = Vec::new();
    for _ in 0..16 {
        let Some(def) = ty.def_id() else { break };
        let Some(decl) = sigs.type_decl(def) else {
            break;
        };
        let Some(rep) = decl.representation.as_ref().and_then(|r| r.resolved()) else {
            break;
        };
        if let Some(inv) = &decl.invariant {
            bounds.push(inv.clone());
        }
        ty = rep.clone();
    }
    let measured = |n: i128, measure: crate::hir::Measure| {
        bounds
            .iter()
            .filter(|b| b.measure == measure)
            .find(|b| !b.holds(n))
            .map(|b| format!("`{value}` does not hold `{}`", b.written))
    };
    match (ty.as_primitive(), value) {
        (Some(Prim::Int), Literal::Int(n)) => {
            measured(i128::from(*n), crate::hir::Measure::Value).map_or(Ok(()), Err)
        }
        (Some(Prim::Str), Literal::Str(s)) => {
            measured(s.chars().count() as i128, crate::hir::Measure::Length).map_or(Ok(()), Err)
        }
        (Some(Prim::Bool), Literal::Bool(_)) => Ok(()),
        _ => Err(format!(
            "`{value}` is no `{ty}`: a backfill is a literal of the field's stored type"
        )),
    }
}

/// A `refers`: its field is the row's, and holds the key of a table this
/// module sees.
#[allow(clippy::too_many_arguments)]
fn referring(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    sigs: &Signatures,
    row: &str,
    r: &Clause<Reference>,
    field: Option<Option<&ResolvedType>>,
) -> Vec<Diagnostic> {
    let reference = &r.value;
    let Some(field) = field else {
        return vec![malformed(
            format!(
                "`refers {} to {}` names `{}`, which is no field of `{row}`",
                reference.field, reference.table, reference.field
            ),
            r.span.clone(),
            Vec::new(),
            format!("name a field of `{row}`"),
        )];
    };
    let Some((target_unit, target)) = table_module(ws, hirs, unit, &reference.table) else {
        return vec![malformed(
            format!(
                "`refers {} to {}` names no table this module sees",
                reference.field, reference.table
            ),
            r.span.clone(),
            Vec::new(),
            format!(
                "import the module of the table `{}`, or name a table",
                reference.table
            ),
        )];
    };
    let (Some(field), Some(row), Some(key)) = (field, &target.row, &target.key) else {
        // A field whose type does not resolve is PW0026's; a table with no
        // row or key is refused where it is declared.
        return Vec::new();
    };
    if key.value.len() != 1 {
        return vec![malformed(
            format!(
                "`refers {} to {}`: `{}` is keyed by {} fields, and a reference holds one",
                reference.field,
                reference.table,
                reference.table,
                key.value.len()
            ),
            r.span.clone(),
            Vec::new(),
            "refer to a table keyed by one field".to_string(),
        )];
    }
    let target_ty = match ws.resolve_in(target_unit, Namespace::Type, &row.value) {
        Resolution::Local(def) => sigs
            .type_decl(def)
            .and_then(|d| d.record.as_ref())
            .and_then(|fs| fs.iter().find(|(n, _)| *n == key.value[0]))
            .and_then(|(_, ty)| ty.resolved().cloned()),
        _ => None,
    };
    let Some(target_ty) = target_ty else {
        return Vec::new();
    };
    // The field holds the key, or may hold none: `Option` of it.
    let held = field.same_as(&target_ty)
        || (field.as_builtin() == Some(crate::resolved::Builtin::Option)
            && field.args().first().is_some_and(|a| a.same_as(&target_ty)));
    if held {
        return Vec::new();
    }
    vec![malformed(
        format!(
            "`refers {} to {}`: `{}` is `{field}`, and `{}` is keyed by `{target_ty}`",
            reference.field, reference.table, reference.field, reference.table
        ),
        r.span.clone(),
        Vec::new(),
        format!(
            "type `{}` as `{}`'s key, `{target_ty}`, or `Option<{target_ty}>`",
            reference.field, reference.table
        ),
    )]
}

/// **PW6924: a reference stays within one owner's rows** (track
/// `row-ownership`, the review's finding 1; ADR-0384's H1). The database
/// checks a reference across every row of both tables, whoever owns them, so
/// a reference between two owners' rows is a read of another's: an owned
/// table refers to an owned table from its own owner field alone (the
/// referred table's key is its owner, PW0368, so the row it names is the
/// referring row's owner's own); a table no one owns refers to no owned
/// table; and an owned table to no table no one owns, whose delete a private
/// row would refuse, until a reference states what that delete does
/// (ADR-0384's H3 and H9). `None` for a reference that stays within.
fn within_one_owner(
    hirs: &[&Hir],
    unit: usize,
    ws: &Workspace,
    t: &Table,
    r: &Clause<Reference>,
) -> Option<Diagnostic> {
    let (target_unit, target) = table_module(ws, hirs, unit, &r.value.table)?;
    let field = &r.value.field;
    let theirs = &target.name;
    let (message, repair) = match (t.owner_field(), target.owner_field()) {
        (None, None) => return None,
        (Some(owner), Some(_)) if field == owner => return None,
        (Some(owner), Some(by)) => (
            format!(
                "`refers {field} to {theirs}`: `{theirs}`'s rows are each owned by their `{by}`, \
                 and `{field}` is not `{}`'s owner, `{owner}`: whether the row it names exists \
                 is its owner's to know, and a write here would tell it, and keep its owner from \
                 deleting it",
                t.name
            ),
            format!(
                "refer to `{theirs}` from `{owner}`, naming the owner's own row, or keep the id \
                 without `refers` and read `{theirs}` by its reader's handle where it is needed"
            ),
        ),
        (None, Some(by)) => (
            format!(
                "`refers {field} to {theirs}`: `{theirs}`'s rows are each owned by their `{by}`, \
                 and `{}`'s rows are no one's: a write here would tell whether another owner's \
                 row exists, and keep its owner from deleting it",
                t.name
            ),
            format!(
                "keep the id without `refers`, handling `None` where `{theirs}` is read, or own \
                 `{}` by the field that names its owner",
                t.name
            ),
        ),
        (Some(owner), None) => (
            format!(
                "`refers {field} to {theirs}`: `{}`'s rows are each owned by their `{owner}`, and \
                 `{theirs}`'s are no one's: deleting a row of `{theirs}` would be refused while \
                 an owner's private row names it, and the refusal would tell whoever deletes it \
                 that the row exists",
                t.name
            ),
            format!(
                "keep the id without `refers`, handling `None` where `{theirs}` is read: what \
                 deleting a row an owned row names does is not declared yet (ADR-0384's H3 and \
                 H9)"
            ),
        ),
    };
    Some(refusal(
        codes::REFERENCE_WITHIN_ONE_OWNER,
        "reference_within_one_owner",
        Severity::Error,
        message,
        r.span.clone(),
        vec![Related::in_unit(
            target_unit,
            target.span.clone(),
            match target.owner_field() {
                Some(by) => format!("`{theirs}` is owned by `{by}` here"),
                None => format!("`{theirs}` is declared here, owned by no one"),
            },
        )],
        WITHIN_ONE_OWNER,
        repair,
    ))
}

/// **PW0365: a reference to a table another source holds**, warned whether
/// its field is optional or required (ADR-0338, decision 4). Within one source a
/// reference is a foreign key the database holds; across two, nothing holds
/// it, and the row it names may be missing.
fn across_sources(hirs: &[&Hir], unit: usize, ws: &Workspace, t: &Table) -> Vec<Diagnostic> {
    let mine = source_of(hirs, &t.name);
    t.references
        .iter()
        .filter_map(|r| {
            let (target_unit, target) = table_module(ws, hirs, unit, &r.value.table)?;
            let theirs = source_of(hirs, &r.value.table);
            (theirs != mine).then(|| {
                refusal(
                    codes::REFERS_ACROSS_SOURCES,
                    "refers_across_sources",
                    Severity::Warning,
                    format!(
                        "`{}.{}` refers to `{}` in {}, and `{}` is in {}: no database holds the \
                         reference, so the referred row may be missing",
                        t.name,
                        r.value.field,
                        r.value.table,
                        source_named(&theirs),
                        t.name,
                        source_named(&mine),
                    ),
                    r.span.clone(),
                    vec![Related::in_unit(
                        target_unit,
                        target.span.clone(),
                        format!("`{}` is held by {}", target.name, source_named(&theirs)),
                    )],
                    "Within one database a reference is a foreign key, which the database \
                     holds: a row is not written referring to one that is not there, and a row \
                     referred to is not deleted. Two databases hold nothing of each other, so the \
                     row a reference across them names may be missing, deleted by the other \
                     database's writes, and `get` answers it as `None`. This is true whether \
                     the field holding the key is optional or required.",
                    format!(
                        "handle `None` from `{}`'s `get`, or hold `{}` and `{}` in one source",
                        r.value.table, t.name, r.value.table
                    ),
                )
            })
        })
        .collect()
}

const NOT_DECLARED: &str = "A table's rows are reached only through the functions its \
     declaration gives: its reads by its key, a prefix of the key, an index, and whole where it is \
     bounded, and its writes. Each is a primitive every adapter implements for any table, so no \
     app's data layer is written by hand, and a read the declaration does not give does not \
     exist: a filter on a field no key or index leads with is the program's own, over a read it \
     gives.";

/// PW0362: a binding a program writes to a table's rows.
fn bindings(hirs: &[&Hir], unit: usize, ws: &Workspace) -> Vec<Diagnostic> {
    let Some(hir) = hirs.get(unit) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, d) in hir.all_decls() {
        if d.kind != DeclKind::Fn || hir.generated.contains_key(&id) {
            continue;
        }
        let Some(host) = d.policy("host") else {
            continue;
        };
        let op = host.value.trim().trim_matches('"');
        if op.starts_with("pw:tables/") {
            out.push(refusal(
                codes::DATA_NOT_DECLARED,
                "data_not_declared",
                Severity::Error,
                format!(
                    "`{}` binds `{op}` by hand: a table's operations are generated from its \
                     declaration",
                    d.name
                ),
                host.span.clone(),
                vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` is bound here", d.name),
                )],
                NOT_DECLARED,
                "declare the read the table gives (a key, a prefix of it, an index) and call \
                 its function"
                    .to_string(),
            ));
            continue;
        }
        for e in d.declared_effects.iter().flatten() {
            if !matches!(e.path.as_str(), "database.read" | "database.write") {
                continue;
            }
            let Some(table) = e.args.first() else {
                continue;
            };
            if table_module(ws, hirs, unit, table).is_none() {
                continue;
            }
            out.push(refusal(
                codes::DATA_NOT_DECLARED,
                "data_not_declared",
                Severity::Error,
                format!(
                    "`{}` {} `{table}` through `host \"{op}\"`, and `{table}` is a table",
                    d.name,
                    match e.path.as_str() {
                        "database.write" => "writes",
                        _ => "reads",
                    }
                ),
                e.span.clone(),
                vec![Related::new(
                    hir.decl_span(id),
                    format!("`{}` is bound here", d.name),
                )],
                NOT_DECLARED,
                format!(
                    "reach `{table}`'s rows through the functions its declaration gives, and \
                     write what this does in Pleris over them"
                ),
            ));
        }
    }
    out
}

/// **Is `member` shaped as a table's read or write?**
fn read_shaped(member: &str) -> bool {
    member == "all"
        || member.starts_with("get")
        || member.starts_with("of_")
        || member.starts_with("by_")
        || member.starts_with("insert")
        || member.starts_with("put")
        || member.starts_with("delete")
}

/// **A call of a read or a write a table does not give** (PW0362, PW0364):
/// `head.member`, or `member` alone in the table's own module (`head` is
/// `None`), where the table's module names no `member`. `None` where it is
/// no table's, or shaped as no read or write: a name that resolves to
/// nothing, as any other.
pub fn undeclared(
    ws: &Workspace,
    hirs: &[&Hir],
    unit: usize,
    head: Option<&str>,
    member: &str,
    span: Span,
) -> Option<Diagnostic> {
    if !read_shaped(member) {
        return None;
    }
    let name = match head {
        Some(h) => h.to_string(),
        None => ws.module_of(unit)?.name.clone(),
    };
    let (table_unit, t) = table_module(ws, hirs, unit, &name)?;
    let written = match head {
        Some(h) => format!("{h}.{member}"),
        None => member.to_string(),
    };
    // An owned table is never read whole (track `row-ownership`): `all`
    // would answer every owner's rows.
    if member == "all"
        && let Some(owner) = t.owner_field()
    {
        return Some(refusal(
            codes::OWNED_READ_BY_ITS_READER,
            "owned_read_by_its_reader",
            Severity::Error,
            format!(
                "`{written}()` reads every row of `{}`, whose rows are each their owner's",
                t.name
            ),
            span,
            vec![Related::in_unit(
                table_unit,
                t.span.clone(),
                format!("`{}` is owned by `{owner}` here", t.name),
            )],
            READ_BY_ITS_READER,
            format!(
                "read the reader's own rows by a read `{}` gives that leads with `{owner}`, \
                 given their handle, `current_user()`",
                t.name
            ),
        ));
    }
    if member == "all" && t.bounded.is_none() {
        return Some(refusal(
            codes::ALL_UNBOUNDED,
            "all_unbounded",
            Severity::Error,
            format!(
                "`{written}()` reads every row of `{}`, which states no `bounded`",
                t.name
            ),
            span,
            vec![Related::in_unit(
                table_unit,
                t.span.clone(),
                format!("`{}` is declared here", t.name),
            )],
            "A table read whole costs what the table holds, which no key bounds. A table a page \
             reads whole states the most rows it holds, `bounded 50`, and every other read is \
             by its key, a prefix of the key, or an index.",
            format!(
                "state `bounded` and the most rows `{}` holds, or read it by its key or an index",
                t.name
            ),
        ));
    }
    let gives: Vec<String> = hirs
        .get(table_unit)
        .map(|hir| {
            hir.generated
                .iter()
                .filter(|(_, g)| g.table == t.decl && matches!(g.what, Wrote::Function { .. }))
                .map(|(id, _)| format!("`{}`", hir.decl(*id).name))
                .collect()
        })
        .unwrap_or_default();
    Some(refusal(
        codes::DATA_NOT_DECLARED,
        "read_not_declared",
        Severity::Error,
        format!("`{written}` is no read or write `{}` gives", t.name),
        span,
        vec![Related::in_unit(
            table_unit,
            t.span.clone(),
            match gives.is_empty() {
                true => format!("`{}` is declared here, and gives none", t.name),
                false => format!("`{}` gives {}", t.name, gives.join(", ")),
            },
        )],
        NOT_DECLARED,
        format!(
            "read `{}` by its key, a prefix of it or an index it declares (`index by_x on x`), \
             or filter what a read gives",
            t.name
        ),
    ))
}

/// The tables the program declares, each with the unit it is in.
pub fn every(hirs: &[&Hir]) -> Vec<(usize, Table)> {
    hirs.iter()
        .enumerate()
        .flat_map(|(u, hir)| declared(hir).into_iter().map(move |t| (u, t)))
        .collect()
}

/// The functions a table's declaration gave its module, in order.
pub fn functions_of(hir: &Hir, table: DeclId) -> Vec<(DeclId, Primitive, Vec<String>)> {
    hir.generated
        .iter()
        .filter(|(_, g)| g.table == table)
        .filter_map(|(id, g)| match &g.what {
            Wrote::Function { primitive, by } => Some((*id, *primitive, by.clone())),
            Wrote::Tuple { .. } | Wrote::Counted { .. } | Wrote::Seen { .. } => None,
        })
        .collect()
}

/// Every DefId the compiler wrote, across units.
pub fn generated_defs(hirs: &[&Hir]) -> BTreeSet<DefId> {
    hirs.iter()
        .enumerate()
        .flat_map(|(unit, hir)| {
            hir.generated
                .keys()
                .map(move |id| DefId { unit, decl: id.0 })
        })
        .collect()
}

// --- the schema, and its change ---------------------------------------------

/// **What a database stores a field's value as** (ADR-0338, 1.4): its type,
/// whatever it is called. An opaque type is its representation, with the
/// bounds its invariant states (ADR-0179); a record and a type of cases are
/// their fields and cases. An adapter maps each to its database's own: a
/// column (`BIGINT` and a `CHECK`), a document's field, a CQL type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stored {
    /// An `Int`, within the bounds an opaque type over it states.
    Int {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at_least: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at_most: Option<i64>,
    },
    /// A `String`, its length in code points within the bounds stated.
    String {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_length: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<i64>,
    },
    Bool,
    Float,
    Unit,
    /// `Option<T>`: none, or a `T`.
    Option(Box<Stored>),
    /// `List<T>`, in its order.
    List(Box<Stored>),
    /// `Set<T>`: distinct, ascending (ADR-0057).
    Set(Box<Stored>),
    /// `Map<K, V>`: by key, ascending (ADR-0057).
    Map(Box<Stored>, Box<Stored>),
    /// A record: its fields, in order.
    Record(Vec<StoredField>),
    /// A type of cases: each case's name and what it carries.
    Cases(Vec<StoredCase>),
    /// A type met again inside itself, by its name: its value is stored as
    /// the outer one is.
    Recursive(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredField {
    pub name: String,
    pub stored: Stored,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredCase {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carries: Vec<Stored>,
}

impl Stored {
    /// The same type, its bounds aside: what a change of bounds keeps.
    fn unbounded(&self) -> Stored {
        match self {
            Stored::Int { .. } => Stored::Int {
                at_least: None,
                at_most: None,
            },
            Stored::String { .. } => Stored::String {
                min_length: None,
                max_length: None,
            },
            Stored::Option(t) => Stored::Option(Box::new(t.unbounded())),
            Stored::List(t) => Stored::List(Box::new(t.unbounded())),
            Stored::Set(t) => Stored::Set(Box::new(t.unbounded())),
            Stored::Map(k, v) => Stored::Map(Box::new(k.unbounded()), Box::new(v.unbounded())),
            Stored::Record(fs) => Stored::Record(
                fs.iter()
                    .map(|f| StoredField {
                        name: f.name.clone(),
                        stored: f.stored.unbounded(),
                    })
                    .collect(),
            ),
            Stored::Cases(cs) => Stored::Cases(
                cs.iter()
                    .map(|c| StoredCase {
                        name: c.name.clone(),
                        carries: c.carries.iter().map(Stored::unbounded).collect(),
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

/// **What `ty` is stored as**, its type parameters given by `args` where
/// `ty` is a declaration's own (`Box<T>`'s `T`).
pub fn stored(sigs: &Signatures, ty: &ResolvedType) -> Stored {
    stored_in(sigs, ty, &mut Vec::new(), &BTreeMap::new())
}

fn stored_in(
    sigs: &Signatures,
    ty: &ResolvedType,
    open: &mut Vec<DefId>,
    parameters: &BTreeMap<(DefId, u32), Stored>,
) -> Stored {
    use crate::resolved::Builtin;
    if let Some(binding) = ty.parameter_binding() {
        return parameters
            .get(&binding)
            .cloned()
            .unwrap_or_else(|| Stored::Recursive(ty.display_name()));
    }
    if let Some(p) = ty.as_primitive() {
        return match p {
            Prim::Int => Stored::Int {
                at_least: None,
                at_most: None,
            },
            Prim::Str => Stored::String {
                min_length: None,
                max_length: None,
            },
            Prim::Bool => Stored::Bool,
            Prim::Float => Stored::Float,
            Prim::Unit => Stored::Unit,
        };
    }
    let arg = |i: usize, open: &mut Vec<DefId>| {
        ty.args()
            .get(i)
            .map_or(Stored::Unit, |a| stored_in(sigs, a, open, parameters))
    };
    if let Some(b) = ty.as_builtin() {
        return match b {
            Builtin::Option => Stored::Option(Box::new(arg(0, open))),
            Builtin::List => Stored::List(Box::new(arg(0, open))),
            Builtin::Set => Stored::Set(Box::new(arg(0, open))),
            Builtin::Map => Stored::Map(Box::new(arg(0, open)), Box::new(arg(1, open))),
            // A `Result` is a case of two; a function is no value a table
            // holds, which the type's boundary refuses where it is built.
            Builtin::Result => Stored::Cases(vec![
                StoredCase {
                    name: "Ok".to_string(),
                    carries: vec![arg(0, open)],
                },
                StoredCase {
                    name: "Err".to_string(),
                    carries: vec![arg(1, open)],
                },
            ]),
            Builtin::Function => Stored::Unit,
        };
    }
    let Some(def) = ty.def_id() else {
        return Stored::Recursive(ty.display_name());
    };
    if open.contains(&def) {
        return Stored::Recursive(ty.display_name());
    }
    let Some(decl) = sigs.type_decl(def) else {
        return Stored::Recursive(ty.display_name());
    };
    // Project supplied arguments before opening the definition: Box<Box<Int>>
    // is finite, whereas a Box<T> field inside Box<T>'s definition recurs.
    // The environment holds storage projections, not a second semantic type
    // substitution; nested constructors look up parameters by binder identity.
    let mut given = parameters.clone();
    for (index, argument) in ty.args().iter().enumerate() {
        given.insert(
            (def, index as u32),
            stored_in(sigs, argument, open, parameters),
        );
    }
    open.push(def);
    let out = if let Some(rep) = decl.representation.as_ref().and_then(|r| r.resolved()) {
        let mut inner = stored_in(sigs, rep, open, &given);
        if let Some(inv) = &decl.invariant {
            let clamp = |n: Option<i128>| n.and_then(|n| i64::try_from(n).ok());
            match (&mut inner, inv.measure) {
                (Stored::Int { at_least, at_most }, crate::hir::Measure::Value) => {
                    *at_least = clamp(inv.at_least).or(*at_least);
                    *at_most = clamp(inv.at_most).or(*at_most);
                }
                (
                    Stored::String {
                        min_length,
                        max_length,
                    },
                    crate::hir::Measure::Length,
                ) => {
                    *min_length = clamp(inv.at_least).or(*min_length);
                    *max_length = clamp(inv.at_most).or(*max_length);
                }
                _ => {}
            }
        }
        inner
    } else if let Some(fields) = &decl.record {
        Stored::Record(
            fields
                .iter()
                .map(|(name, t)| StoredField {
                    name: name.clone(),
                    stored: t
                        .resolved()
                        .map_or(Stored::Unit, |t| stored_in(sigs, t, open, &given)),
                })
                .collect(),
        )
    } else if let Some(cases) = &decl.variants {
        Stored::Cases(
            cases
                .iter()
                .map(|(name, payload)| StoredCase {
                    name: name.clone(),
                    carries: payload
                        .iter()
                        .map(|t| {
                            t.resolved()
                                .map_or(Stored::Unit, |t| stored_in(sigs, t, open, &given))
                        })
                        .collect(),
                })
                .collect(),
        )
    } else {
        Stored::Recursive(ty.display_name())
    };
    open.pop();
    out
}

/// **A table's schema**: what a database holds of it, and what a release
/// fixes (ADR-0338, 1.4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableSchema {
    pub name: String,
    /// The source that holds it, `None` for the host's own database
    /// (ADR-0207).
    pub source: Option<String>,
    pub fields: Vec<FieldSchema>,
    pub key: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexSchema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ReferenceSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldSchema {
    pub name: String,
    /// The type as the row's record writes it: `PositiveInt`.
    #[serde(rename = "type")]
    pub ty: String,
    pub stored: Stored,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReferenceSchema {
    pub field: String,
    pub table: String,
}

/// **A schema, as `schema.lock` holds it**: every table the program declares,
/// by name, and the hash of them (ADR-0338, 1.4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Schema {
    /// The lock's format.
    pub schema: u32,
    /// FNV-1a over the tables as written here, hex: what a database records
    /// of the schema it holds (`pw_schema`).
    pub hash: String,
    pub tables: Vec<TableSchema>,
}

/// The format `schema.lock` is written in.
pub const LOCK_FORMAT: u32 = 1;

impl Schema {
    /// **The schema of `tables`**, sorted by name, with its hash.
    pub fn of(mut tables: Vec<TableSchema>) -> Schema {
        tables.sort_by(|a, b| a.name.cmp(&b.name));
        let hash = Schema::hash_of(&tables);
        Schema {
            schema: LOCK_FORMAT,
            hash,
            tables,
        }
    }

    fn hash_of(tables: &[TableSchema]) -> String {
        let text = serde_json::to_string(tables).unwrap_or_default();
        format!(
            "{:016x}",
            crate::build::fnv1a(crate::build::FNV_OFFSET, text.as_bytes())
        )
    }

    /// **A `schema.lock`, read**: its format known, its hash its tables'.
    pub fn read(text: &str) -> Result<Schema, String> {
        let lock: Schema =
            serde_json::from_str(text).map_err(|e| format!("not a schema lock: {e}"))?;
        if lock.schema != LOCK_FORMAT {
            return Err(format!(
                "a schema lock of format {}, and this compiler reads format {LOCK_FORMAT}",
                lock.schema
            ));
        }
        let hash = Schema::hash_of(&lock.tables);
        if hash != lock.hash {
            return Err(format!(
                "its hash is {}, and its tables' is {hash}: it was changed by hand, and is no \
                 schema a build released",
                lock.hash
            ));
        }
        Ok(lock)
    }

    /// As `schema.lock` is written.
    pub fn written(&self) -> String {
        format!(
            "{}\n",
            serde_json::to_string_pretty(self).unwrap_or_default()
        )
    }
}

/// **Each table the program declares, as its schema**, with the unit and the
/// declaration it is from. A table outside the module of its name, or whose
/// row is no record declared there, has none (PW0361).
pub fn schemas(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
) -> Vec<(usize, Table, TableSchema)> {
    let mut out = Vec::new();
    for (unit, t) in every(hirs) {
        if ws.module_of(unit).is_none_or(|m| m.name != t.name) {
            continue;
        }
        let Some(row) = &t.row else { continue };
        let Resolution::Local(def) = ws.resolve_in(unit, Namespace::Type, &row.value) else {
            continue;
        };
        let Some(record) = sigs.type_decl(def).and_then(|d| d.record.as_ref()) else {
            continue;
        };
        let fields: Vec<FieldSchema> = record
            .iter()
            .filter_map(|(name, ty)| {
                let ty = ty.resolved()?;
                Some(FieldSchema {
                    name: name.clone(),
                    ty: ty.display_name(),
                    stored: stored(sigs, ty),
                })
            })
            .collect();
        let schema = TableSchema {
            name: t.name.clone(),
            source: source_of(hirs, &t.name),
            fields,
            key: t.key.as_ref().map(|k| k.value.clone()).unwrap_or_default(),
            order: t.order.as_ref().map(|o| o.value.clone()),
            indexes: t
                .indexes
                .iter()
                .map(|i| IndexSchema {
                    name: i.value.name.clone(),
                    fields: i.value.fields.clone(),
                    unique: i.value.unique,
                })
                .collect(),
            references: t
                .references
                .iter()
                .map(|r| ReferenceSchema {
                    field: r.value.field.clone(),
                    table: r.value.table.clone(),
                })
                .collect(),
        };
        out.push((unit, t, schema));
    }
    out
}

/// **One step of a migration** (ADR-0338, 1.4): what an adapter applies, in
/// its own terms, to bring a database from the schema released to the one
/// built. Each is additive, or a change the program stated.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// A table no release had.
    CreateTable {
        table: String,
    },
    /// A field no release had: an `Option`, which every row released holds
    /// as `None`, or stated `backfilled with` a value every row holds.
    AddField {
        table: String,
        field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backfill: Option<Literal>,
    },
    /// A field released, stated `dropped`: its values are given up.
    DropField {
        table: String,
        field: String,
    },
    /// A field released under another name, stated `renamed from` it.
    RenameField {
        table: String,
        from: String,
        to: String,
    },
    /// A field whose stored type changed, stated `backfilled with` the value
    /// every row released holds in it.
    RetypeField {
        table: String,
        field: String,
        backfill: Literal,
    },
    /// A field whose type's bounds changed: a database holding one as a
    /// check holds the new one, and refuses it where a row released does not.
    ChangeBounds {
        table: String,
        field: String,
    },
    /// The order a prefix's or an index's rows are read in.
    ChangeOrder {
        table: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    AddIndex {
        table: String,
        index: String,
    },
    DropIndex {
        table: String,
        index: String,
    },
    AddReference {
        table: String,
        field: String,
        to: String,
    },
    DropReference {
        table: String,
        field: String,
        to: String,
    },
}

/// **A migration**: from the schema released (its hash, `None` for a
/// database that holds none), to the one built, by its steps.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Plan {
    pub from: Option<String>,
    pub to: String,
    pub steps: Vec<Step>,
}

const UNSTATED: &str = "A change from the schema released that loses or reinterprets data runs \
     against rows a database already holds. It is planned only where the program states it: a \
     field `renamed from` its old name, a field `dropped`, or a field added or retyped \
     `backfilled with` the value each row released holds. A key changed, a table dropped and a \
     table moved between sources have no statement: each moves rows, which no migration does.";

/// **The program held to the schema released** (ADR-0338, 1.4; PW0366): the
/// steps from `released` to what the program declares, and a refusal, in
/// the unit it is about, for each change that loses or reinterprets data
/// with no `migrate` stating it, each change no statement makes, and each
/// `migrate` that states nothing.
pub fn held_to(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    released: Option<&Schema>,
) -> (Plan, Vec<(usize, Diagnostic)>) {
    let now = schemas(hirs, ws, sigs);
    let built = Schema::of(now.iter().map(|(_, _, s)| s.clone()).collect());
    let mut steps = Vec::new();
    let mut refused: Vec<(usize, Diagnostic)> = Vec::new();
    let refuse = |reason: &'static str, message: String, span: Span, repair: String| {
        refusal(
            codes::MIGRATION_UNSTATED,
            reason,
            Severity::Error,
            message,
            span,
            Vec::new(),
            UNSTATED,
            repair,
        )
    };
    let empty: Vec<TableSchema> = Vec::new();
    let was = released.map_or(&empty, |r| &r.tables);
    // A table released and declared no more.
    for gone in was
        .iter()
        .filter(|w| !now.iter().any(|(_, _, s)| s.name == w.name))
    {
        let (unit, span) = place_of_gone(hirs, gone, &now);
        refused.push((
            unit,
            refuse(
                "table_dropped",
                format!(
                    "`{}` was released and is declared no more: its rows would be lost",
                    gone.name
                ),
                span,
                format!(
                    "declare `{}` again; dropping a table is not a step",
                    gone.name
                ),
            ),
        ));
    }
    for (unit, t, schema) in &now {
        let stated: Vec<&Clause<Migration>> = t.migrations.iter().collect();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        let Some(old) = was.iter().find(|w| w.name == schema.name) else {
            steps.push(Step::CreateTable {
                table: schema.name.clone(),
            });
            if released.is_some() {
                for m in &stated {
                    refused.push((
                        *unit,
                        refuse(
                            "migration_stale",
                            format!(
                                "`migrate {}` states a change of `{}`, which no release had",
                                written_migration(&m.value),
                                t.name
                            ),
                            m.span.clone(),
                            "take it out: a table no release had is created whole".to_string(),
                        ),
                    ));
                }
            }
            continue;
        };
        let table = schema.name.clone();
        if old.source != schema.source {
            refused.push((
                *unit,
                refuse(
                    "migration_refused",
                    format!(
                        "`{table}` was released in {} and is in {}: its rows would stay in the \
                         other database",
                        source_named(&old.source),
                        source_named(&schema.source)
                    ),
                    t.span.clone(),
                    format!(
                        "hold `{table}` where it was released; a table moved between databases \
                         is a deployment's work"
                    ),
                ),
            ));
        }
        // The renames stated, old name to new.
        let mut renamed: BTreeMap<String, String> = BTreeMap::new();
        for (i, m) in stated.iter().enumerate() {
            if let Change::Renamed { from } = &m.value.change
                && old.fields.iter().any(|f| f.name == *from)
                && !old.fields.iter().any(|f| f.name == m.value.field)
            {
                renamed.insert(from.clone(), m.value.field.clone());
                used.insert(i);
            }
        }
        let now_name = |n: &str| renamed.get(n).cloned().unwrap_or_else(|| n.to_string());
        let backfill_of = |field: &str| {
            stated
                .iter()
                .enumerate()
                .find_map(|(i, m)| match &m.value.change {
                    Change::Backfilled(v) if m.value.field == field => Some((i, v.clone())),
                    _ => None,
                })
        };
        let dropped_of = |field: &str| {
            stated
                .iter()
                .position(|m| m.value.field == field && matches!(m.value.change, Change::Dropped))
        };
        for f in &old.fields {
            let to = now_name(&f.name);
            match schema.fields.iter().find(|g| g.name == to) {
                Some(g) => {
                    if to != f.name {
                        steps.push(Step::RenameField {
                            table: table.clone(),
                            from: f.name.clone(),
                            to: to.clone(),
                        });
                    }
                    if g.stored.unbounded() != f.stored.unbounded() {
                        match backfill_of(&to) {
                            Some((i, v)) => {
                                used.insert(i);
                                steps.push(Step::RetypeField {
                                    table: table.clone(),
                                    field: to.clone(),
                                    backfill: v,
                                });
                            }
                            None => refused.push((
                                *unit,
                                refuse(
                                    "migration_unstated",
                                    format!(
                                        "`{table}.{to}` was released as `{}` and is `{}`: each \
                                         row's value would be read as another type",
                                        f.ty, g.ty
                                    ),
                                    field_span(t),
                                    format!(
                                        "state the value each row released holds: `migrate {to} \
                                         backfilled with ..`, or keep its type"
                                    ),
                                ),
                            )),
                        }
                    } else if g.stored != f.stored {
                        steps.push(Step::ChangeBounds {
                            table: table.clone(),
                            field: to.clone(),
                        });
                    }
                }
                None => match dropped_of(&f.name) {
                    Some(i) => {
                        used.insert(i);
                        steps.push(Step::DropField {
                            table: table.clone(),
                            field: f.name.clone(),
                        });
                    }
                    None => refused.push((
                        *unit,
                        refuse(
                            "migration_unstated",
                            format!(
                                "`{table}.{}` was released and is no field of `{table}`'s row: \
                                 its values would be lost",
                                f.name
                            ),
                            field_span(t),
                            format!(
                                "state it: `migrate {0} dropped`, or `migrate <new> renamed from \
                                 {0}` where it has another name",
                                f.name
                            ),
                        ),
                    )),
                },
            }
        }
        // The fields no release had.
        let released_as: BTreeSet<String> = old.fields.iter().map(|f| now_name(&f.name)).collect();
        for g in schema
            .fields
            .iter()
            .filter(|g| !released_as.contains(&g.name))
        {
            if let Stored::Option(_) = g.stored {
                steps.push(Step::AddField {
                    table: table.clone(),
                    field: g.name.clone(),
                    backfill: None,
                });
                continue;
            }
            match backfill_of(&g.name) {
                Some((i, v)) => {
                    used.insert(i);
                    steps.push(Step::AddField {
                        table: table.clone(),
                        field: g.name.clone(),
                        backfill: Some(v),
                    });
                }
                None => refused.push((
                    *unit,
                    refuse(
                        "migration_unstated",
                        format!(
                            "`{table}.{}` is new, of `{}`, and the rows released hold no value for \
                             it",
                            g.name, g.ty
                        ),
                        field_span(t),
                        format!(
                            "make it an `Option`, or state the value each row released holds: \
                             `migrate {} backfilled with ..`",
                            g.name
                        ),
                    ),
                )),
            }
        }
        // The key, read through the renames.
        let old_key: Vec<String> = old.key.iter().map(|k| now_name(k)).collect();
        if old_key != schema.key {
            refused.push((
                *unit,
                refuse(
                    "migration_refused",
                    format!(
                        "`{table}`'s key was `{}` and is `{}`: each row would be found by \
                         another key",
                        old.key.join(", "),
                        schema.key.join(", ")
                    ),
                    t.key.as_ref().map_or(t.span.clone(), |k| k.span.clone()),
                    format!(
                        "keep `{table}`'s key; a key changed is a new table, its rows moved by a \
                         program"
                    ),
                ),
            ));
        }
        let old_order = old.order.as_deref().map(now_name);
        if old_order != schema.order {
            steps.push(Step::ChangeOrder {
                table: table.clone(),
                from: old.order.clone(),
                to: schema.order.clone(),
            });
        }
        // The indexes, by name, read through the renames.
        let mapped = |i: &IndexSchema| IndexSchema {
            name: i.name.clone(),
            fields: i.fields.iter().map(|f| now_name(f)).collect(),
            unique: i.unique,
        };
        for i in &old.indexes {
            if !schema.indexes.contains(&mapped(i)) {
                steps.push(Step::DropIndex {
                    table: table.clone(),
                    index: i.name.clone(),
                });
            }
        }
        for i in &schema.indexes {
            if !old.indexes.iter().any(|o| mapped(o) == *i) {
                steps.push(Step::AddIndex {
                    table: table.clone(),
                    index: i.name.clone(),
                });
            }
        }
        let mapped = |r: &ReferenceSchema| ReferenceSchema {
            field: now_name(&r.field),
            table: r.table.clone(),
        };
        for r in &old.references {
            if !schema.references.contains(&mapped(r)) {
                steps.push(Step::DropReference {
                    table: table.clone(),
                    field: r.field.clone(),
                    to: r.table.clone(),
                });
            }
        }
        for r in &schema.references {
            if !old.references.iter().any(|o| mapped(o) == *r) {
                steps.push(Step::AddReference {
                    table: table.clone(),
                    field: r.field.clone(),
                    to: r.table.clone(),
                });
            }
        }
        // A statement the change does not use states nothing. One the row
        // itself refuses is PW0361's, told once (ADR-0112).
        for (i, m) in stated.iter().enumerate() {
            if used.contains(&i) || !shaped(&m.value, &schema.fields) {
                continue;
            }
            refused.push((
                *unit,
                refuse(
                    "migration_stale",
                    format!(
                        "`migrate {}` states no change of `{table}` from the schema released",
                        written_migration(&m.value)
                    ),
                    m.span.clone(),
                    "take it out: it was released, or states a change the row does not make"
                        .to_string(),
                ),
            ));
        }
    }
    let plan = Plan {
        from: released.map(|r| r.hash.clone()),
        to: built.hash.clone(),
        steps,
    };
    // Each told beside the table it is about, as this build declares it
    // (charter §16.3's boundary span): one table to a module.
    for (unit, d) in &mut refused {
        if d.related.is_empty() {
            d.related
                .push(match now.iter().find(|(u, _, _)| u == unit) {
                    Some((_, t, _)) => Related::new(
                        t.span.clone(),
                        format!("`{}`, as this build declares it", t.name),
                    ),
                    None => Related::new(
                        d.primary_span.clone(),
                        "where the table released was held".to_string(),
                    ),
                });
        }
    }
    (plan, refused)
}

/// **Does the row admit `m`**, as `shape` holds it (PW0361)? A field dropped
/// that the row has not, a field renamed that it has from one it has not,
/// a field backfilled that it has with a value of its stored type.
fn shaped(m: &Migration, fields: &[FieldSchema]) -> bool {
    let field = |f: &str| fields.iter().find(|g| g.name == f);
    match &m.change {
        Change::Dropped => field(&m.field).is_none(),
        Change::Renamed { from } => field(&m.field).is_some() && field(from).is_none(),
        Change::Backfilled(v) => field(&m.field).is_some_and(|f| f.stored.admits(v)),
    }
}

impl Stored {
    /// **Is `v` a value of this stored type**, within its bounds?
    pub fn admits(&self, v: &Literal) -> bool {
        let within = |n: i64, lo: &Option<i64>, hi: &Option<i64>| {
            lo.is_none_or(|lo| n >= lo) && hi.is_none_or(|hi| n <= hi)
        };
        match (self, v) {
            (Stored::Int { at_least, at_most }, Literal::Int(n)) => within(*n, at_least, at_most),
            (
                Stored::String {
                    min_length,
                    max_length,
                },
                Literal::Str(s),
            ) => within(
                i64::try_from(s.chars().count()).unwrap_or(i64::MAX),
                min_length,
                max_length,
            ),
            (Stored::Bool, Literal::Bool(_)) => true,
            _ => false,
        }
    }
}

/// `title renamed from name`, as written.
fn written_migration(m: &Migration) -> String {
    match &m.change {
        Change::Renamed { from } => format!("{} renamed from {from}", m.field),
        Change::Dropped => format!("{} dropped", m.field),
        Change::Backfilled(v) => format!("{} backfilled with {v}", m.field),
    }
}

/// Where a field's change is told: the table's `row` clause.
fn field_span(t: &Table) -> Span {
    t.row.as_ref().map_or(t.span.clone(), |r| r.span.clone())
}

/// Where a table released and declared no more is told: the source that
/// held it, if the program still declares it; else the first table the
/// program declares; else the first unit's start.
fn place_of_gone(
    hirs: &[&Hir],
    gone: &TableSchema,
    now: &[(usize, Table, TableSchema)],
) -> (usize, Span) {
    if let Some(source) = &gone.source {
        for (unit, hir) in hirs.iter().enumerate() {
            if let Some((id, _)) = hir
                .all_decls()
                .find(|(_, d)| d.kind == DeclKind::Source && d.name == *source)
            {
                return (unit, hir.decl_span(id));
            }
        }
    }
    match now.first() {
        Some((unit, t, _)) => (*unit, t.span.clone()),
        None => (0, 0..0),
    }
}

// --- what a host reads: `tables.json` ----------------------------------------

/// **A table's function, as a host binds its import** (ADR-0338, 1.2 and
/// 1.3): the import, the primitive one adapter function serves for every
/// table, the fields it reads or writes by, and the order its rows come in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionJson {
    pub name: String,
    pub import: String,
    pub primitive: Primitive,
    /// `read` or `write`.
    pub effect: String,
    /// The key's fields, a prefix's or an index's; none for `all`, `insert`
    /// and `put`.
    pub by: Vec<String>,
    /// What it is given, by name, each type as written.
    pub params: Vec<ParamJson>,
    pub returns: String,
    /// **The order of the rows it answers**: each field ascending, the first
    /// first (an `Int` by value, a `String` by code point, `false` before
    /// `true`, a case by its name). Empty for what answers one row or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sorted_by: Vec<String>,
    /// **Whose handle it is given, in which field's place** (track
    /// `row-ownership`, ADR-0384 H1): an owned table's function's. A read is
    /// given the reader's handle in the place of the field it leads with,
    /// its owner or one it shares with; a write its owner's. A host holds
    /// the handle to the reader it serves, and never takes a plain id there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParamJson {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// **A table, as a host serves it**: its schema, the module its functions
/// are in, the interface they are imported from, its bound, and its
/// functions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableJson {
    #[serde(flatten)]
    pub schema: TableSchema,
    pub module: String,
    pub interface: String,
    /// The record type of its rows.
    pub row: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounded: Option<u64>,
    /// **Whose its rows are** (track `row-ownership`, ADR-0384 H1): the
    /// field holding each row's owner's id. Not the schema's: who reads a
    /// row is no part of what a database stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The fields holding the ids of the others who read a row, as its seen
    /// row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shares: Vec<String>,
    /// The fields its owner alone reads: no seen row holds them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private: Vec<String>,
    pub functions: Vec<FunctionJson>,
}

/// **`tables.json`**: every table a program declares, the schema it builds
/// (its hash, which `schema.lock` holds once released), and the migration
/// from the schema released.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TablesJson {
    pub hash: String,
    pub tables: Vec<TableJson>,
    pub migration: Plan,
}

/// The order a read's rows come in: grouped by what a set form was given,
/// then by `order`, then by the key; each field once.
fn sorted_by(t: &TableSchema, primitive: Primitive, by: &[String]) -> Vec<String> {
    if primitive == Primitive::Count {
        return Vec::new();
    }
    if primitive == Primitive::CountMany {
        return by.to_vec();
    }
    let one_row = matches!(primitive, Primitive::Get)
        || (primitive == Primitive::Index
            && t.indexes
                .iter()
                .any(|i| i.unique && i.fields.as_slice() == by));
    if primitive.writes() || one_row {
        return Vec::new();
    }
    let grouped: &[String] = match primitive {
        Primitive::GetMany | Primitive::PrefixMany | Primitive::IndexMany => by,
        _ => &[],
    };
    let mut out: Vec<String> = Vec::new();
    for f in grouped.iter().chain(t.order.iter()).chain(t.key.iter()) {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// **The field whose place a function's handle takes** (track
/// `row-ownership`): an owned table's insert and put are given its owner's;
/// every other function the field its selector leads with, which the table's
/// declaration holds to be its owner or a field it shares with (PW0368).
fn reader_of(t: &Table, primitive: Primitive, by: &[String]) -> Option<String> {
    let owner = t.owner_field()?;
    match primitive {
        Primitive::Insert | Primitive::Put => Some(owner.to_string()),
        _ => by.first().cloned(),
    }
}

/// **What `pw build` writes as `tables.json`** (ADR-0338, 1.4), with the
/// migration from `released`, and the refusals of PW0366.
pub fn tables_json(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    released: Option<&Schema>,
) -> (TablesJson, Schema, Vec<(usize, Diagnostic)>) {
    let (migration, refused) = held_to(hirs, ws, sigs, released);
    let now = schemas(hirs, ws, sigs);
    let lock = Schema::of(now.iter().map(|(_, _, s)| s.clone()).collect());
    let mut tables: Vec<TableJson> = now
        .iter()
        .map(|(unit, t, schema)| {
            let hir = hirs[*unit];
            let functions = functions_of(hir, t.decl)
                .into_iter()
                .map(|(id, primitive, by)| {
                    let d = hir.decl(id);
                    let reader = reader_of(t, primitive, &by);
                    FunctionJson {
                        name: d.name.clone(),
                        import: import_of(&t.name, &d.name),
                        primitive,
                        effect: match primitive.writes() {
                            true => "write",
                            false => "read",
                        }
                        .to_string(),
                        sorted_by: sorted_by(schema, primitive, &by),
                        by,
                        params: d
                            .params
                            .iter()
                            .map(|p| ParamJson {
                                name: p.name.clone(),
                                ty: p.ty.as_ref().map(DeclaredType::written).unwrap_or_default(),
                            })
                            .collect(),
                        returns: d
                            .ret
                            .as_ref()
                            .map(DeclaredType::written)
                            .unwrap_or_default(),
                        reader,
                    }
                })
                .collect();
            TableJson {
                module: t.name.clone(),
                interface: interface_of(&t.name),
                row: t.row.as_ref().map(|r| r.value.clone()).unwrap_or_default(),
                schema: schema.clone(),
                bounded: t.bounded.as_ref().map(|b| b.value),
                owner: t.owner_field().map(str::to_string),
                shares: t.sharers().to_vec(),
                private: t.private_fields().to_vec(),
                functions,
            }
        })
        .collect();
    tables.sort_by(|a, b| a.schema.name.cmp(&b.schema.name));
    (
        TablesJson {
            hash: lock.hash.clone(),
            tables,
            migration,
        },
        lock,
        refused,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered(src: &str) -> Hir {
        let parsed = pw_syntax::parse_tree(src);
        assert!(parsed.ok(), "{:?}", parsed.errors);
        crate::lower::lower_file(src, &parsed.green)
    }

    /// A declaration with every span and id left out: what it says.
    fn said(d: &Decl) -> String {
        format!(
            "{:?} {} ({}) -> {} {} {} [{}]",
            d.kind,
            d.name,
            d.params
                .iter()
                .map(|p| format!(
                    "{}: {}",
                    p.name,
                    p.ty.as_ref().map(DeclaredType::written).unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(", "),
            d.ret
                .as_ref()
                .map(DeclaredType::written)
                .unwrap_or_default(),
            d.declared_effects
                .iter()
                .flatten()
                .map(|e| e.written.clone())
                .collect::<Vec<_>>()
                .join(", "),
            d.policies
                .iter()
                .map(|p| format!("{} {}", p.name, p.value))
                .collect::<Vec<_>>()
                .join("; "),
            d.fields
                .iter()
                .flatten()
                .map(|f| format!(
                    "{}: {}",
                    f.name,
                    f.ty.as_ref().map(DeclaredType::written).unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    const LINES: &str = "module CartLines\n\nopaque type Owner = String\nopaque type ItemId = String\nopaque type StoreId = String\n\ntype CartLine = CartLine { owner: Owner, store: StoreId, item: ItemId, quantity: Int, added: Int }\n\ntable CartLines\n    row     CartLine\n    key     owner, store, item\n    order   added\n    index   by_store on store\n    unique  by_owner_item on owner, item\n    bounded 100\n";

    /// **What the compiler writes is what a programmer would write**: each
    /// generated declaration says what the same function or record, written
    /// in Pleris and lowered, says (every span aside).
    #[test]
    fn each_generated_declaration_is_what_its_pleris_lowers_to() {
        let hir = lowered(LINES);
        let generated: Vec<String> = hir.generated.keys().map(|id| said(hir.decl(*id))).collect();
        let written = "module CartLines\n\
            type CartLinesOwnerStoreItem = CartLinesOwnerStoreItem { owner: Owner, store: StoreId, item: ItemId }\n\
            type CartLinesOwnerStore = CartLinesOwnerStore { owner: Owner, store: StoreId }\n\
            type CartLinesOwnerItem = CartLinesOwnerItem { owner: Owner, item: ItemId }\n\
            type CartLinesOwnerCount = CartLinesOwnerCount { key: Owner, count: Int }\n\
            type CartLinesOwnerStoreCount = CartLinesOwnerStoreCount { key: CartLinesOwnerStore, count: Int }\n\
            type CartLinesOwnerStoreItemCount = CartLinesOwnerStoreItemCount { key: CartLinesOwnerStoreItem, count: Int }\n\
            type CartLinesStoreCount = CartLinesStoreCount { key: StoreId, count: Int }\n\
            type CartLinesOwnerItemCount = CartLinesOwnerItemCount { key: CartLinesOwnerItem, count: Int }\n\
            fn get(owner: Owner, store: StoreId, item: ItemId) -> Option<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#get\"\n\
            fn get_many(keys: List<CartLinesOwnerStoreItem>) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#get-many\"\n\
            fn of_owner(owner: Owner) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#of-owner\"\n\
            fn of_owner_many(prefixes: List<Owner>) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#of-owner-many\"\n\
            fn of_owner_store(owner: Owner, store: StoreId) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#of-owner-store\"\n\
            fn of_owner_store_many(prefixes: List<CartLinesOwnerStore>) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#of-owner-store-many\"\n\
            fn by_store(store: StoreId) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#by-store\"\n\
            fn by_store_many(values: List<StoreId>) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#by-store-many\"\n\
            fn by_owner_item(owner: Owner, item: ItemId) -> Option<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#by-owner-item\"\n\
            fn by_owner_item_many(values: List<CartLinesOwnerItem>) -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#by-owner-item-many\"\n\
            fn count_by_owner(owner: Owner) -> Int !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner\"\n\
            fn count_by_owner_many(values: List<Owner>) -> List<CartLinesOwnerCount> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-many\"\n\
            fn count_by_owner_store(owner: Owner, store: StoreId) -> Int !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-store\"\n\
            fn count_by_owner_store_many(values: List<CartLinesOwnerStore>) -> List<CartLinesOwnerStoreCount> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-store-many\"\n\
            fn count_by_owner_store_item(owner: Owner, store: StoreId, item: ItemId) -> Int !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-store-item\"\n\
            fn count_by_owner_store_item_many(values: List<CartLinesOwnerStoreItem>) -> List<CartLinesOwnerStoreItemCount> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-store-item-many\"\n\
            fn count_by_store(store: StoreId) -> Int !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-store\"\n\
            fn count_by_store_many(values: List<StoreId>) -> List<CartLinesStoreCount> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-store-many\"\n\
            fn count_by_owner_item(owner: Owner, item: ItemId) -> Int !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-item\"\n\
            fn count_by_owner_item_many(values: List<CartLinesOwnerItem>) -> List<CartLinesOwnerItemCount> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#count-by-owner-item-many\"\n\
            fn all() -> List<CartLine> !{ database.read<CartLines> }\n    host \"pw:tables/cart-lines#all\"\n\
            fn insert(row: CartLine) -> Result<(), CartLine> !{ database.write<CartLines> }\n    host \"pw:tables/cart-lines#insert\"\n\
            fn put(row: CartLine) -> Result<(), CartLine> !{ database.write<CartLines> }\n    host \"pw:tables/cart-lines#put\"\n\
            fn delete(owner: Owner, store: StoreId, item: ItemId) -> () !{ database.write<CartLines> }\n    host \"pw:tables/cart-lines#delete\"\n\
            fn delete_of_owner(owner: Owner) -> () !{ database.write<CartLines> }\n    host \"pw:tables/cart-lines#delete-of-owner\"\n\
            fn delete_of_owner_store(owner: Owner, store: StoreId) -> () !{ database.write<CartLines> }\n    host \"pw:tables/cart-lines#delete-of-owner-store\"\n";
        let by_hand = lowered(written);
        let expected: Vec<String> = by_hand.all_decls().map(|(_, d)| said(d)).collect();
        assert_eq!(generated, expected);
        // Each is the module's own declaration.
        let (_, module, _) = hir.modules.iter().next().expect("a module");
        for id in hir.generated.keys() {
            assert!(module.decls.contains(id), "{}", hir.decl(*id).name);
        }
    }

    #[test]
    fn a_table_outside_the_module_of_its_name_gives_nothing() {
        let hir = lowered(&LINES.replace("module CartLines", "module store.lines"));
        assert!(hir.generated.is_empty());
    }

    #[test]
    fn a_table_with_no_bound_has_no_all_and_no_unique_put_refused() {
        let hir = lowered(
            &LINES
                .replace("    bounded 100\n", "")
                .replace("unique  by_owner_item", "index   by_owner_item"),
        );
        let names: Vec<&str> = hir
            .generated
            .keys()
            .map(|id| hir.decl(*id).name.as_str())
            .collect();
        assert!(!names.contains(&"all"), "{names:?}");
        let put = hir
            .generated
            .keys()
            .map(|id| hir.decl(*id))
            .find(|d| d.name == "put")
            .expect("put");
        assert_eq!(
            put.ret.as_ref().map(DeclaredType::written).as_deref(),
            Some("()")
        );
    }

    #[test]
    fn clauses_read_as_written_and_their_faults_named() {
        let hir = lowered(
            "module T\ntype R = R { a: Int }\ntable T\n    row R\n    key a, a\n    index stores\n    refers a Users\n    migrate a renamed\n",
        );
        let t = &declared(&hir)[0];
        let faults: Vec<&str> = t.unread.iter().map(|(_, w)| w.as_str()).collect();
        assert_eq!(faults.len(), 4, "{faults:?}");
        assert!(faults[0].contains("names `a` twice"), "{faults:?}");
        assert!(faults[1].contains("`<name> on <fields>`"), "{faults:?}");
        assert!(faults[2].contains("`<field> to <table>`"), "{faults:?}");
        assert!(faults[3].contains("renamed from"), "{faults:?}");
    }

    #[test]
    fn a_migration_reads_its_three_forms() {
        assert_eq!(
            migration("title renamed from name"),
            Ok(Migration {
                field: "title".into(),
                change: Change::Renamed {
                    from: "name".into()
                }
            })
        );
        assert_eq!(
            migration("notes dropped").map(|m| m.change),
            Ok(Change::Dropped)
        );
        assert_eq!(
            migration("rank backfilled with -3").map(|m| m.change),
            Ok(Change::Backfilled(Literal::Int(-3)))
        );
        assert_eq!(
            migration("label backfilled with \"none yet\"").map(|m| m.change),
            Ok(Change::Backfilled(Literal::Str("none yet".into())))
        );
        assert!(migration("rank backfilled with nothing").is_err());
        assert!(migration("rank changed").is_err());
    }
}
