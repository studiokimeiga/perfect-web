//! **A WIT world per component, generated from what the compiler already
//! decided.**
//!
//! Charter §14 M8 task 2, and the E8 gate item:
//!
//! > Generate a WIT world per `ComponentContract`: the world's imports are
//! > exactly `contract.imports`, and `wit-bindgen` accepts it.
//!
//! `imports_are_exactly_the_contracts_imports` is that criterion as a test, and
//! `wit_parser_resolves_the_generated_package` is the second half — validated
//! by the parser `wasm-tools` and `wit-bindgen` are built on, not by a reader
//! written here. A second WIT parser in this repo would be the shape the
//! project keeps deleting, and it would be a particularly bad instance: the
//! whole point of emitting WIT is that somebody else's toolchain reads it.
//!
//! # What comes from where
//!
//! ```text
//! world name, imports     the ComponentContract          authority
//! export signatures       the declaration's Interface    the same one binding.rs reads
//! type definitions        the program's type decls       records, variants, aliases
//! ```
//!
//! The contract is the authority boundary (ADR-0018/ADR-0020) and carries no
//! types, which is correct — a host deciding whether to instantiate does not
//! need them. WIT does, so this reads the program. What it must NOT do is
//! re-derive an import or a placement, and
//! `imports_are_exactly_the_contracts_imports` is what says it does not.
//!
//! # Refusal, not invention
//!
//! A type this generator cannot map produces [`WitError::Unmappable`] and no
//! output. The alternative is a plausible guess — `s64` for anything numeric,
//! `string` for anything opaque — which produces a world that parses, links,
//! and decodes a value into something it never was. Charter §7.10 is about
//! exactly that boundary.
//!
//! Name collisions are the same: WIT identifiers are kebab-case, so
//! `store.page.Cart` and `store.page.cart` mangle to one name. That is refused
//! rather than resolved, because silently merging two identities is
//! `docs/RISK_QUEUE.md` 34 in a new place.

use std::collections::{BTreeMap, BTreeSet};

use crate::binding::Interface;
use crate::contract::{ComponentContract, ImportKind};
use crate::hir::{Decl, DeclKind, Hir};
use crate::resolve::{DefId, Workspace};
use crate::resolved::{self, Builtin, Primitive, ResolvedType, TypeResolution};
use crate::signatures::Signatures;

/// The package version every generated world carries.
///
/// Fixed rather than derived: a world's version says which SHAPE of world this
/// is, and deriving it from a program would make every edit look like a
/// protocol change. `CAPABILITY_MAPPING` is the field that tracks meaning.
pub const PACKAGE: &str = "pw:app@0.1.0";

/// Why a world could not be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitError {
    /// A type in an exported signature has no WIT form this generator knows.
    Unmappable { ty: String, at: String },
    /// Two distinct names mangle to one WIT identifier.
    Collision {
        wit: String,
        first: String,
        second: String,
    },
    /// **Two declarations claim one `interface#operation`.**
    ///
    /// An operation has one ABI, so two claimants are two ABIs for one name —
    /// exactly what `contract::abi` refuses when the second one comes from an
    /// artifact. Reported rather than resolved by order, because "whichever was
    /// met last" is an answer that looks like every other answer.
    Claimed { operation: String, count: usize },
    /// **A type that contains itself, in a form no node holds yet**
    /// (ADR-0194): one that crosses a boundary crosses as its nodes, and a
    /// node holds the type only as a `List` of it.
    Recursive { ty: String, why: String },
}

impl std::fmt::Display for WitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WitError::Unmappable { ty, at } => {
                write!(f, "`{ty}` at {at} has no WIT form")
            }
            WitError::Collision { wit, first, second } => {
                write!(f, "`{first}` and `{second}` both mangle to `{wit}`")
            }
            WitError::Claimed { operation, count } => {
                write!(f, "{count} declarations claim `{operation}`")
            }
            WitError::Recursive { ty, why } => {
                write!(
                    f,
                    "`{ty}` is a type that contains itself, and crosses no boundary yet: {why}"
                )
            }
        }
    }
}

/// Source ownership for executable validation; the underlying public error and
/// its wording are unchanged for existing WIT consumers.
pub(crate) struct LocatedError {
    pub error: WitError,
    pub declaration: Option<DefId>,
}
impl LocatedError {
    fn at(error: WitError, declaration: DefId) -> Self {
        Self {
            error,
            declaration: Some(declaration),
        }
    }
}
impl From<WitError> for LocatedError {
    fn from(error: WitError) -> Self {
        Self {
            error,
            declaration: None,
        }
    }
}

/// **A WIT identifier from a Pleris name.**
///
/// `store.page.StorePage` → `store-page-store-page`. Lowercase kebab, because
/// that is what WIT accepts; the dots and the case are what a Pleris name uses
/// to distinguish things, and both are lost here — which is why every caller
/// checks for a collision rather than trusting the mangling.
pub fn ident(name: &str) -> String {
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
    // A WIT identifier may not start with a digit, and an empty one is not an
    // identifier at all.
    if trimmed.is_empty() {
        "x".to_string()
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        format!("x-{trimmed}")
    } else {
        trimmed
    }
}

/// **WIT's keywords**, as `wit-parser` 0.257.1's lexer reads them
/// (`src/ast/lex.rs`).
const KEYWORDS: &[&str] = &[
    "use",
    "type",
    "func",
    "u8",
    "u16",
    "u32",
    "u64",
    "s8",
    "s16",
    "s32",
    "s64",
    "f32",
    "f64",
    "char",
    "resource",
    "own",
    "borrow",
    "record",
    "flags",
    "variant",
    "enum",
    "bool",
    "string",
    "option",
    "result",
    "future",
    "stream",
    "error-context",
    "list",
    "map",
    "as",
    "from",
    "static",
    "interface",
    "tuple",
    "world",
    "import",
    "export",
    "package",
    "constructor",
    "include",
    "with",
    "async",
];

/// **An identifier as WIT text writes it**: `%` before a keyword, which WIT
/// reads as the identifier without it. A case `List`, or a field `own`, is
/// the identifier `list` or `own` in the `Resolve`, where nothing is escaped.
/// Until 2026-09-26 such a name was written bare, and the whole package did
/// not parse, so every component of the program was refused.
fn escaped(id: &str) -> String {
    match KEYWORDS.contains(&id) {
        true => format!("%{id}"),
        false => id.to_string(),
    }
}

/// Every name that must survive mangling, checked for collisions in one place.
fn no_collisions(names: impl IntoIterator<Item = String>) -> Result<(), WitError> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for name in names {
        let wit = ident(&name);
        if let Some(first) = seen.get(&wit)
            && *first != name
        {
            return Err(WitError::Collision {
                wit,
                first: first.clone(),
                second: name,
            });
        }
        seen.insert(wit, name);
    }
    Ok(())
}

/// What the program declares about each type, in the form WIT needs.
///
/// **Keyed by qualified path, never by bare name.** `capability.SessionId` and
/// `domain.SessionId` are two declarations of two different types, and the
/// first version of this keyed by `d.name` — so both became `session-id` and
/// `wit-parser` refused the package for a duplicate definition. It refused; a
/// generator that had deduplicated instead would have exported one type where
/// the program has two, which is `docs/RISK_QUEUE.md` 34 on a wire.
///
/// Which of the two a signature means is a resolution question, so `Types` is
/// built with the workspace and every reference goes through it.
#[derive(Debug, Default)]
pub struct Types {
    /// Declaration order preserved, so the emitted package is stable.
    defs: Vec<TypeDef>,
    /// Qualified path → the WIT identifier it is emitted under.
    known: BTreeMap<String, String>,
    /// `DefId` → qualified path, so a resolved reference finds its definition.
    by_def: BTreeMap<crate::resolve::DefId, String>,
    qualifiers: BTreeSet<DefId>,
    /// Generic declarations whose representation never mentions their type
    /// parameters — `opaque type Money<C> = Int`. Every
    /// specialization has the one layout, so `Money<USD>` is `domain-money` on
    /// the wire. The argument is kept by the semantic contract and dropped by
    /// the boundary, the same one-directional loss an opaque alias and a
    /// privacy qualifier already have. A generic whose layout depends on its
    /// arguments is still refused: that needs specialization, not erasure.
    phantom: BTreeSet<DefId>,
}

#[derive(Debug, Clone)]
enum TypeDef {
    /// `type Store = Store { id: Int }` → a WIT record.
    Record {
        name: String,
        /// Where it was declared, so its field types resolve from there.
        fields: Vec<(String, TypeResolution)>,
    },
    /// A sum type → a WIT variant. Payload-free cases become `enum`-shaped
    /// variants, which WIT allows and which keeps one construct for both.
    Variant {
        name: String,
        cases: Vec<(String, Vec<TypeResolution>)>,
    },
    /// `opaque type StoreId = String`.
    ///
    /// Emitted as an ALIAS of its representation, and that is a real decision:
    /// a WIT boundary has no opacity to offer, so the choice is between
    /// exposing the representation and refusing to export anything that uses
    /// an opaque type. `types::opaque_types_do_not_expose_their_representation`
    /// is about the Pleris type system, where the distinction is enforced;
    /// once a value is on a wire there is nothing left to enforce it with, and
    /// pretending otherwise would be the lie.
    Alias { name: String, of: TypeResolution },
}

impl Types {
    /// Collect every type the program declares, by resolved identity.
    pub fn build(hirs: &[&Hir], ws: &Workspace, sigs: &Signatures) -> Types {
        let mut out = Types::default();
        for (unit, hir) in hirs.iter().enumerate() {
            for (id, d) in hir.all_decls() {
                if !matches!(d.kind, DeclKind::Type | DeclKind::Opaque) {
                    continue;
                }
                let module = hir.module_of(id).unwrap_or_default();
                let path = if module.is_empty() {
                    d.name.clone()
                } else {
                    format!("{module}.{}", d.name)
                };
                out.by_def
                    .insert(crate::resolve::DefId { unit, decl: id.0 }, path.clone());
                let def = DefId { unit, decl: id.0 };
                if sigs.privacy_kind(def).is_some() {
                    out.qualifiers.insert(def);
                }
                match d.kind {
                    DeclKind::Opaque => {
                        let Some(of) = &d.opaque_of else { continue };
                        out.known.insert(path.clone(), ident(&path));
                        out.defs.push(TypeDef::Alias {
                            name: path,
                            of: resolved::resolve(
                                ws,
                                unit,
                                Some(def),
                                &d.type_params,
                                of,
                                d.name_span.clone(),
                            ),
                        });
                    }
                    _ => out.push_type(&path, unit, d, ws, def),
                }
                // A generic declaration whose representation never mentions
                // its parameters has one layout: a record's, a variant's, and
                // since ADR-0327 an opaque type's, `Money<C> = Int`, which
                // crosses as its representation as every opaque type does.
                // One whose representation mentions them still has no WIT
                // form: its layout would be its argument's, a privacy
                // qualifier's transparency, and opacity is not ABI
                // transparency (the ruling of 2026-08-20).
                if matches!(d.kind, DeclKind::Type | DeclKind::Opaque)
                    && !d.type_params.is_empty()
                    && out.defs.last().is_some_and(|t| {
                        t.referenced().iter().all(|r| {
                            r.resolved()
                                .is_some_and(|ty| !mentions_parameter_of(ty, def))
                        })
                    })
                {
                    out.phantom.insert(def);
                }
            }
        }
        out
    }

    fn push_type(&mut self, path: &str, unit: usize, d: &Decl, ws: &Workspace, def: DefId) {
        // A union of any number of cases is a variant: one of one case was
        // written as an empty record until 2026-09-26, and its payload was
        // lost (ADR-0059).
        if let Some(variants) = &d.variants
            && !variants.is_empty()
        {
            self.known.insert(path.to_string(), ident(path));
            self.defs.push(TypeDef::Variant {
                name: path.to_string(),
                cases: variants
                    .iter()
                    .map(|v| {
                        (
                            v.name.clone(),
                            v.fields
                                .iter()
                                .map(|t| {
                                    resolved::resolve(
                                        ws,
                                        unit,
                                        Some(def),
                                        &d.type_params,
                                        t,
                                        d.name_span.clone(),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            });
            return;
        }
        // Otherwise a record: `type Store = Store { id: Int }` is the corpus's
        // shape, and its fields live on the declaration.
        // `f.ty` is the HEAD and `f.ty_args` its arguments, so a field written
        // `List<MenuItem>` is `("List", ["MenuItem"])` and reading only the
        // head produces `list` with nothing in it. `wit-parser` caught that on
        // the store demo the first time this ran, which is exactly why the
        // real parser decides here instead of a reader written in this repo.
        let fields = d
            .fields
            .as_ref()
            .map(|fs| {
                fs.iter()
                    .filter_map(|f| {
                        f.ty.as_ref().map(|t| {
                            (
                                f.name.clone(),
                                resolved::resolve(
                                    ws,
                                    unit,
                                    Some(def),
                                    &d.type_params,
                                    t,
                                    f.span.clone(),
                                ),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.known.insert(path.to_string(), ident(path));
        self.defs.push(TypeDef::Record {
            name: path.to_string(),
            fields,
        });
    }

    /// The declarations the ABI representation actually names. A transparent
    /// privacy qualifier contributes its argument, not a phantom WIT wrapper.
    fn declared_within(&self, ty: &ResolvedType, out: &mut BTreeSet<String>) {
        if let Some(def) = ty.def_id()
            && !self.qualifiers.contains(&def)
            && let Some(path) = self.by_def.get(&def)
        {
            out.insert(path.clone());
            // A phantom argument is not part of the representation.
            if self.phantom.contains(&def) {
                return;
            }
        }
        for arg in ty.args() {
            self.declared_within(arg, out);
        }
    }
}

/// Does this type mention one of `binder`'s own type parameters?
fn mentions_parameter_of(ty: &ResolvedType, binder: DefId) -> bool {
    ty.parameter_binding().is_some_and(|(b, _)| b == binder)
        || ty.args().iter().any(|a| mentions_parameter_of(a, binder))
}

impl Types {
    fn is_phantom(&self, def: DefId) -> bool {
        self.phantom.contains(&def)
    }

    /// The declaration a qualified path names.
    fn def_of(&self, path: &str) -> Option<DefId> {
        self.by_def
            .iter()
            .find_map(|(def, p)| (p == path).then_some(*def))
    }
}

impl TypeDef {
    fn name(&self) -> &str {
        match self {
            TypeDef::Record { name, .. }
            | TypeDef::Variant { name, .. }
            | TypeDef::Alias { name, .. } => name,
        }
    }

    /// The types this definition mentions, as written.
    fn referenced(&self) -> Vec<TypeResolution> {
        match self {
            TypeDef::Record { fields, .. } => fields.iter().map(|(_, t)| t.clone()).collect(),
            TypeDef::Variant { cases, .. } => cases.iter().flat_map(|(_, f)| f.clone()).collect(),
            TypeDef::Alias { of, .. } => vec![of.clone()],
        }
    }
}

/// A WIT type is projected from resolved semantic identity, never re-resolved
/// from a signature's printed spelling in a possibly different module.
fn wit_type(resolution: &TypeResolution, types: &Types, at: &str) -> Result<String, WitError> {
    let ty = resolution.resolved().ok_or_else(|| WitError::Unmappable {
        ty: match resolution {
            TypeResolution::Unresolved { written, .. } => written.clone(),
            _ => resolution.to_string(),
        },
        at: at.to_string(),
    })?;
    wit_resolved(ty, types, at)
}

fn wit_resolved(ty: &ResolvedType, types: &Types, at: &str) -> Result<String, WitError> {
    let bad = || WitError::Unmappable {
        ty: ty.display_name(),
        at: at.to_string(),
    };
    if let Some(p) = ty.as_primitive() {
        return Ok(match p {
            Primitive::Int => "s64",
            Primitive::Float => "f64",
            Primitive::Bool => "bool",
            Primitive::Str => "string",
            Primitive::Unit => "tuple<>",
        }
        .to_string());
    }
    let mapped = |index: usize| wit_resolved(ty.args().get(index).ok_or_else(bad)?, types, at);
    // **A case of a `Result` that carries nothing** (track `tables`): WIT
    // writes it `_`, as `result<_, e>`, and has no empty tuple, which
    // wit-component refuses ("tuple type must have at least one type"). A
    // command answering `Result<(), E>`, and a table's `insert`, did not
    // build until 2026-10-15. The canonical ABI lays out `result<_, e>` as
    // it would an empty tuple's, so only the text changes.
    let unit = |index: usize| {
        ty.args().get(index).and_then(ResolvedType::as_primitive) == Some(Primitive::Unit)
    };
    if let Some(builtin) = ty.as_builtin() {
        return Ok(match builtin {
            Builtin::List => format!("list<{}>", mapped(0)?),
            Builtin::Option => format!("option<{}>", mapped(0)?),
            Builtin::Result => match (unit(0), unit(1)) {
                (true, true) => "result".to_string(),
                (true, false) => format!("result<_, {}>", mapped(1)?),
                (false, true) => format!("result<{}>", mapped(0)?),
                (false, false) => format!("result<{}, {}>", mapped(0)?, mapped(1)?),
            },
            // In ascending key order, each key once (ADR-0057).
            Builtin::Map => format!("list<tuple<{}, {}>>", mapped(0)?, mapped(1)?),
            Builtin::Set => format!("list<{}>", mapped(0)?),
            // A function is not a value that crosses a component boundary.
            Builtin::Function => return Err(bad()),
        });
    }
    let def = ty.def_id().ok_or_else(bad)?;
    if types.qualifiers.contains(&def) {
        if ty.args().len() != 1 {
            return Err(bad());
        }
        return mapped(0);
    }
    // Generic nominal layouts require specialization. Refuse rather than erase
    // arguments and accidentally publish the unspecialized representation —
    // unless the declaration's layout never mentions its parameters, in which
    // case the unspecialized representation IS every specialization.
    if !ty.args().is_empty() && !types.is_phantom(def) {
        return Err(bad());
    }
    types
        .by_def
        .get(&def)
        .map(|path| escaped(&ident(path)))
        .ok_or_else(bad)
}

/// One exported declaration's signature, as WIT, plus the declared types it
/// names.
///
/// The second half is not decoration: a WIT interface referring to a type from
/// another interface must `use` it by name, so a signature that does not report
/// what it touched produces a package that does not resolve.
/// One signature as WIT.
///
/// `ident` is the operation's WIT identifier and is **already mangled** —
/// the caller decides, because it is the caller that knows where the name came
/// from. An export's name is a Pleris name and needs [`ident`]; a host
/// operation's name came out of `host "store:data/menus#for-store"` and is a
/// WIT identifier the author wrote, which mangling would turn into `forstore`.
fn wit_func(
    ident: &str,
    sig: &Interface,
    types: &Types,
) -> Result<(String, BTreeSet<String>), WitError> {
    let name = ident;
    let at = format!("`{name}`");
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut params: Vec<String> = Vec::new();
    for (i, p) in sig.params.iter().enumerate() {
        let Some(ty) = p else {
            return Err(WitError::Unmappable {
                ty: format!("argument {i}"),
                at: at.clone(),
            });
        };
        params.push(format!("arg{i}: {}", wit_type(ty, types, &at)?));
        if let Some(ty) = ty.resolved() {
            types.declared_within(ty, &mut used);
        }
    }
    let ret = match sig.returns.as_ref() {
        None => String::new(),
        Some(resolution) => {
            let mapped = wit_type(resolution, types, &at)?;
            if let Some(ty) = resolution.resolved() {
                types.declared_within(ty, &mut used);
            }
            if resolution.resolved().and_then(ResolvedType::as_primitive) == Some(Primitive::Unit) {
                String::new()
            } else {
                format!(" -> {mapped}")
            }
        }
    };
    Ok((
        format!("{}: func({}){ret};", escaped(ident), params.join(", ")),
        used,
    ))
}

/// One component's world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub name: String,
    /// The interfaces this world imports, sorted and deduplicated.
    ///
    /// **Exactly `contract.imports`, projected onto interfaces.** A WIT world
    /// imports an interface, not a function, so two host imports naming
    /// `pw:host/database` are one line. A component dependency becomes this
    /// package's own interface for that component — `store-page-cart-api` —
    /// because a world cannot import a world. The projection is the only
    /// transformation, and
    /// `imports_are_exactly_the_contracts_imports` asserts it inverts.
    pub imports: Vec<String>,
    /// The name of the interface this world exports.
    pub exports: String,
    /// The declaration the component is, by identity. A backend finds a
    /// world through this rather than by re-mangling a name: E10-I's encoder
    /// holds a `DefId`, and a second route from a declaration to its world
    /// would be a second answer to one question.
    pub declaration: DefId,
    /// The exported interface's functions, as the WIT names them, in contract
    /// export order.
    pub functions: Vec<String>,
    /// **A computed part's function** (ADR-0226): its lowered function's
    /// name, which a backend finds it by beside the declaration it is of.
    /// `None` for a declaration's own component.
    pub derived: Option<String>,
}

/// The interface name for a component's exports.
///
/// Suffixed because a world and an interface share one namespace in a WIT
/// package, and every component needs both.
fn api(component_id: &str) -> String {
    format!("{}-api", ident(component_id))
}

/// **Where a contract's export lives in its component.** The one place that
/// names it: the world generated below exports this interface with this
/// function, and the contract carries the same pair for the host.
pub fn component_export(component_id: &str, export: &str) -> crate::contract::ComponentExport {
    let (package, version) = PACKAGE.split_once('@').unwrap_or((PACKAGE, ""));
    let at = match version.is_empty() {
        true => String::new(),
        false => format!("@{version}"),
    };
    crate::contract::ComponentExport {
        interface: format!("{package}/{}{at}", api(component_id)),
        function: ident(export),
        authorization: Vec::new(),
        idempotent_by: None,
        invoked_by: None,
        bounded: Vec::new(),
    }
}

/// **Each declared type's WIT identifier in `pw:types/types`, by declaration.**
///
/// The names [`package`] gives them, from the same table, so the backend finds
/// a declaration's type in a world by the identity the generator used, never
/// by mangling a name a second time (ADR-0039 §5). A declaration only some
/// worlds name is in this map whether or not a given world includes it; the
/// encoder looks it up in the world and defines it itself when it is absent.
pub fn type_idents(hirs: &[&Hir], ws: &Workspace) -> BTreeMap<DefId, String> {
    let sigs = Signatures::build(ws, hirs);
    let types = Types::build(hirs, ws, &sigs);
    types
        .by_def
        .iter()
        .filter_map(|(def, path)| types.known.get(path).map(|ident| (*def, ident.clone())))
        .collect()
}

/// **Generate the WIT package for a checked program.**
///
/// One `world` per contract, plus one `interface types` holding every type the
/// exported signatures name.
pub fn package(
    hirs: &[&Hir],
    ws: &Workspace,
    contracts: &[ComponentContract],
) -> Result<(String, Vec<World>), WitError> {
    let sigs = Signatures::build(ws, hirs);
    let derived = crate::page_values::derived(hirs, ws, &sigs);
    package_prepared(hirs, ws, contracts, &sigs, &derived).map_err(|failure| failure.error)
}

pub(crate) fn package_prepared(
    hirs: &[&Hir],
    ws: &Workspace,
    contracts: &[ComponentContract],
    sigs: &Signatures,
    derived: &[crate::page_values::Derived],
) -> Result<(String, Vec<World>), LocatedError> {
    let types = Types::build(hirs, ws, sigs);
    // A type that contains itself names its node type too (ADR-0194).
    let nodes: Vec<String> = types
        .by_def
        .iter()
        .filter(|(def, _)| crate::recursion::contains_itself(sigs, **def))
        .map(|(_, path)| node_name(path))
        .collect();
    no_collisions(types.known.keys().cloned().chain(nodes))?;
    no_collisions(contracts.iter().map(|c| c.component_id.clone()))?;

    // The declaration behind each contract, by the path the contract names.
    // Contract keys retain their namespace via `contract::component_id` — read
    // rather than reconstructed, for the same reason the
    // planner reads the exporter map rather than reformatting an id.
    let mut decls: BTreeMap<String, (DefId, &Decl)> = BTreeMap::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, d) in hir.all_decls() {
            // Only component/callable namespaces can supply a contract's
            // declaration. An event named as a query otherwise replaces its
            // executable body with NoBody(Event), depending on source order
            // (ADR-0373); types already required the same separation.
            // And a predicate the program gives a body (ADR-0384's H2): a
            // component the host asks, though its name is no term.
            let asked = d.kind == crate::hir::DeclKind::Predicate && d.body.is_some();
            if !asked
                && !matches!(
                    crate::resolve::Namespace::of(d.kind),
                    Some(crate::resolve::Namespace::Term | crate::resolve::Namespace::Ui)
                )
            {
                continue;
            }
            decls.insert(
                crate::contract::component_id(hir, id),
                (DefId { unit, decl: id.0 }, d),
            );
        }
    }

    // The functions a host computes a computed part by (ADR-0226), by their
    // contracts' ids: no declaration is one, so each is typed by what it
    // takes and answers, and found by its name beside the declaration its
    // expression is written in.
    let derived: BTreeMap<String, &crate::page_values::Derived> = derived
        .iter()
        .map(|d| (d.component_id.clone(), d))
        .collect();

    let mut worlds = Vec::new();
    let mut apis: Vec<Api> = Vec::new();
    for c in contracts {
        let mut imports: Vec<String> = c
            .imports
            .iter()
            .map(|i| match i.kind {
                // A host capability's interface is named by the host package,
                // verbatim: `pw:host/database` is a reference into somebody
                // else's WIT and nothing here may rewrite it.
                ImportKind::HostCapability => i.interface.clone(),
                // A component dependency is this package's own interface for
                // that component. `pw:app/store.page.Cart` is not a WIT
                // identifier and a world cannot import a world; both are fixed
                // by naming the interface the exporter provides.
                ImportKind::Component => {
                    api(i.interface.strip_prefix("pw:app/").unwrap_or(&i.interface))
                }
            })
            .collect();
        imports.sort();
        imports.dedup();

        // One contract per DECLARATION, so the export's signature is the
        // component's own declaration — looked up by the path `contracts()`
        // built the id from, not reconstructed.
        let (declaration, sig, lifted) = match derived.get(&c.component_id) {
            Some(d) => (d.declaration(), d.interface(), Some(d.export())),
            None => {
                let declaration =
                    decls
                        .get(&c.component_id)
                        .map(|(def, _)| *def)
                        .ok_or_else(|| WitError::Unmappable {
                            ty: "missing component declaration".into(),
                            at: c.component_id.clone(),
                        })?;
                let sig = sigs
                    .by_def(declaration)
                    .map(Interface::from)
                    .ok_or_else(|| WitError::Unmappable {
                        ty: "missing component signature".into(),
                        at: c.component_id.clone(),
                    })?;
                (declaration, sig, None)
            }
        };
        let mut funcs = Vec::new();
        let mut used: BTreeSet<String> = BTreeSet::new();
        for e in &c.exports {
            let (f, u) = wit_func(&ident(&e.name), &sig, &types)
                .map_err(|error| LocatedError::at(error, declaration))?;
            funcs.push(f);
            used.extend(u);
        }
        apis.push(Api {
            name: api(&c.component_id),
            uses: used,
            funcs,
        });

        worlds.push(World {
            name: ident(&c.component_id),
            imports,
            exports: api(&c.component_id),
            declaration,
            functions: c
                .exports
                .iter()
                .map(|e| component_export(&c.component_id, &e.name).function)
                .collect(),
            derived: lifted,
        });
    }

    // **The host packages, generated from the Pleris declarations.**
    //
    // Only the operations some contract actually imports: a program does not
    // publish an interface for a `host`-bound `fn` nothing reaches, any more
    // than it exports a component nothing instantiates.
    let wanted: BTreeSet<String> = contracts
        .iter()
        .flat_map(|c| &c.imports)
        .filter(|i| i.kind == ImportKind::HostCapability)
        .map(|i| i.key())
        .collect();
    let mut hosts: BTreeMap<String, HostPackage> = BTreeMap::new();
    let mut host_uses: BTreeSet<String> = BTreeSet::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (decl_id, d) in hir.all_decls() {
            let Some(id) = crate::backend::host_binding(d) else {
                continue;
            };
            if !wanted.contains(&id.qualified()) {
                continue;
            }
            // `store:data/carts` → package `store:data`, interface `carts`.
            let Some((pkg, iface)) = id.interface.split_once('/') else {
                continue;
            };
            let sig = sigs
                .by_def(DefId {
                    unit,
                    decl: decl_id.0,
                })
                .map(Interface::from)
                .ok_or_else(|| WitError::Unmappable {
                    ty: "missing host signature".into(),
                    at: id.qualified(),
                })?;
            let (text, used) = wit_func(&id.name, &sig, &types).map_err(|error| {
                LocatedError::at(
                    error,
                    DefId {
                        unit,
                        decl: decl_id.0,
                    },
                )
            })?;
            host_uses.extend(used.iter().cloned());
            let entry = hosts.entry(pkg.to_string()).or_insert_with(|| HostPackage {
                name: pkg.to_string(),
                interfaces: BTreeMap::new(),
            });
            let slot = entry
                .interfaces
                .entry(iface.to_string())
                .or_insert_with(|| (BTreeSet::new(), Vec::new()));
            slot.0.extend(used);
            slot.1.push(text);
        }
    }
    // **The invalidations' functions** (ADR-0209): each query a command
    // invalidates, taking its parameters and answering nothing.
    for (unit, hir) in hirs.iter().enumerate() {
        for (decl_id, d) in hir.all_decls() {
            if !matches!(
                d.kind,
                DeclKind::Query | DeclKind::Subscription | DeclKind::Resource
            ) {
                continue;
            }
            let Some(sig) = sigs.by_def(DefId {
                unit,
                decl: decl_id.0,
            }) else {
                continue;
            };
            // Each of its functions a command calls, by the positions its
            // key leaves to every value (ADR-0256): it takes the rest.
            let shapes: Vec<(String, Vec<usize>)> = wanted
                .iter()
                .filter_map(|w| {
                    let (interface, name) = w.split_once('#')?;
                    (interface == crate::backend::INVALIDATIONS_INTERFACE).then_some(())?;
                    let every = crate::backend::invalidation_shape(&sig.path, name)?;
                    Some((name.to_string(), every))
                })
                .collect();
            for (name, every) in shapes {
                let id = crate::backend::invalidation_binding(&sig.path, &every);
                let Some((pkg, iface)) = id.interface.split_once('/') else {
                    continue;
                };
                let taking = Interface {
                    params: sig
                        .params
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !every.contains(i))
                        .map(|(_, p)| p.clone())
                        .collect(),
                    returns: None,
                };
                let (text, used) = wit_func(&name, &taking, &types).map_err(|error| {
                    LocatedError::at(
                        error,
                        DefId {
                            unit,
                            decl: decl_id.0,
                        },
                    )
                })?;
                host_uses.extend(used.iter().cloned());
                let entry = hosts.entry(pkg.to_string()).or_insert_with(|| HostPackage {
                    name: pkg.to_string(),
                    interfaces: BTreeMap::new(),
                });
                let slot = entry
                    .interfaces
                    .entry(iface.to_string())
                    .or_insert_with(|| (BTreeSet::new(), Vec::new()));
                slot.0.extend(used);
                slot.1.push(text);
            }
        }
    }
    // **The reads' functions** (ADR-0277): each resource a materialization
    // reads, taking its parameters and answering its value, the `Ok` of a
    // `Result` it answers.
    for (unit, hir) in hirs.iter().enumerate() {
        for (decl_id, _) in hir.all_decls() {
            let Some(sig) = sigs.by_def(DefId {
                unit,
                decl: decl_id.0,
            }) else {
                continue;
            };
            let id = crate::backend::read_binding(&sig.path);
            if !wanted.contains(&format!("{}#{}", id.interface, id.name)) {
                continue;
            }
            let Some((pkg, iface)) = id.interface.split_once('/') else {
                continue;
            };
            let returns = sig.returns.as_ref().map(|r| match r.resolved() {
                Some(t) if t.as_builtin() == Some(crate::resolved::Builtin::Result) => t
                    .args()
                    .first()
                    .map(|ok| crate::resolved::TypeResolution::Resolved(ok.clone()))
                    .unwrap_or_else(|| r.clone()),
                _ => r.clone(),
            });
            let answering = Interface {
                params: sig.params.clone(),
                returns,
            };
            let (text, used) = wit_func(&id.name, &answering, &types).map_err(|error| {
                LocatedError::at(
                    error,
                    DefId {
                        unit,
                        decl: decl_id.0,
                    },
                )
            })?;
            host_uses.extend(used.iter().cloned());
            let entry = hosts.entry(pkg.to_string()).or_insert_with(|| HostPackage {
                name: pkg.to_string(),
                interfaces: BTreeMap::new(),
            });
            let slot = entry
                .interfaces
                .entry(iface.to_string())
                .or_insert_with(|| (BTreeSet::new(), Vec::new()));
            slot.0.extend(used);
            slot.1.push(text);
        }
    }
    // **The schedules' functions** (the scheduled-commands track): for each
    // command a body schedules or cancels, `-at` and `-after` taking its
    // parameters and then the time, an `s64`, and `-cancel` its parameters,
    // each answering nothing.
    for (unit, hir) in hirs.iter().enumerate() {
        for (decl_id, d) in hir.all_decls() {
            if d.kind != DeclKind::Command {
                continue;
            }
            let Some(sig) = sigs.by_def(DefId {
                unit,
                decl: decl_id.0,
            }) else {
                continue;
            };
            for how in crate::backend::SCHEDULE_HOWS {
                let id = crate::backend::schedule_binding(&sig.path, how);
                if !wanted.contains(&format!("{}#{}", id.interface, id.name)) {
                    continue;
                }
                let Some((pkg, iface)) = id.interface.split_once('/') else {
                    continue;
                };
                let mut params = sig.params.clone();
                if how != "cancel" {
                    let int = crate::resolved::ResolvedType::inferred(
                        &crate::resolved::TypeKey::Primitive(crate::resolved::Primitive::Int),
                        &|_| None,
                        &d.name_span,
                    )
                    .map(crate::resolved::TypeResolution::Resolved)
                    .ok_or_else(|| WitError::Unmappable {
                        ty: "a schedule's time".into(),
                        at: id.qualified(),
                    })?;
                    params.push(Some(int));
                }
                let taking = Interface {
                    params,
                    returns: None,
                };
                let (text, used) = wit_func(&id.name, &taking, &types).map_err(|error| {
                    LocatedError::at(
                        error,
                        DefId {
                            unit,
                            decl: decl_id.0,
                        },
                    )
                })?;
                host_uses.extend(used.iter().cloned());
                let entry = hosts.entry(pkg.to_string()).or_insert_with(|| HostPackage {
                    name: pkg.to_string(),
                    interfaces: BTreeMap::new(),
                });
                let slot = entry
                    .interfaces
                    .entry(iface.to_string())
                    .or_insert_with(|| (BTreeSet::new(), Vec::new()));
                slot.0.extend(used);
                slot.1.push(text);
            }
        }
    }
    for h in hosts.values_mut() {
        for (_, funcs) in h.interfaces.values_mut() {
            funcs.sort();
            funcs.dedup();
        }
    }
    let hosts: Vec<HostPackage> = hosts.into_values().collect();

    // The type package must reach what the HOST signatures name too, not only
    // what the exports do — `capability.SessionId` is reachable from
    // `pw:host/session#read` and from nothing else.
    let seeds: Vec<String> = apis
        .iter()
        .flat_map(|a| a.uses.iter().cloned())
        .chain(host_uses)
        .collect();
    let type_text = render_types(&types, &reachable(&types, seeds), sigs)?;
    Ok((render(&type_text, &apis, &worlds, &hosts), worlds))
}

/// **Every host operation's WIT signature, as its Pleris declaration implies
/// it**, keyed by `interface#name`.
///
/// Rendered through exactly the machinery an export goes through —
/// `Interface::of` on the declaration, then [`wit_func`] — so the ABI a host
/// operation is given here and the ABI an exported query is given are one
/// derivation, not two that agree.
///
/// # Why this exists before it is used to emit anything
///
/// A host operation's ABI is currently derived **twice**: here, from the Pleris
/// `fn` carrying the `host` binding, and again by the deployment, in the WIT
/// package it publishes. Nothing compares them, and in this repo all six of the
/// store's operations disagree — see `tests/canonical_abi.rs`.
///
/// Which derivation is authoritative is a model question with the architect,
/// and it turns on [`crate::contract::Ownership`]: an operation the application
/// declared is arguably one the compiler should publish, while a platform
/// facility is one the compiler must conform to. This function is what **both**
/// answers need — to emit the WIT, or to compare against a published one — so
/// building it does not presume either.
///
/// The declaration is found by [`crate::backend::host_binding`], the one reader
/// of the `host` policy, so an operation this names and an operation the
/// backend imports cannot come apart.
///
/// # Per operation, not all-or-nothing
///
/// The result is an outcome **per operation**, because one that has no WIT form
/// must not hide the five that do — the same reason the artifact audit reports
/// every undeclared import rather than the first. It is not an `Option` either:
/// absent and refused would then be the same answer, and one of them is a
/// finding.
///
/// `pw:host/session#read` is currently the refused one, and what it refuses is
/// worth reading: it returns `Session<SessionId>`, a **privacy label**, and a
/// label has no ABI. See `tests/canonical_abi.rs`.
pub fn host_signatures(
    hirs: &[&Hir],
    ws: &Workspace,
) -> BTreeMap<String, Result<String, WitError>> {
    let sigs = Signatures::build(ws, hirs);
    let types = Types::build(hirs, ws, &sigs);
    // Collected before rendering, because a second claimant is a defect about
    // the OPERATION and not about either declaration's signature — and
    // `out.insert` on a duplicate key answers with whichever came last, which
    // is how the effect declarations' empty signatures once overwrote the real
    // ones.
    let mut claims: BTreeMap<String, Vec<DefId>> = BTreeMap::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (decl_id, d) in hir.all_decls() {
            if let Some(id) = crate::backend::host_binding(d) {
                claims.entry(id.qualified()).or_default().push(DefId {
                    unit,
                    decl: decl_id.0,
                });
            }
        }
    }

    let mut out = BTreeMap::new();
    for (operation, claimants) in claims {
        let name = operation
            .rsplit('#')
            .next()
            .unwrap_or(&operation)
            .to_string();
        let rendered = match claimants.as_slice() {
            [def] => sigs
                .by_def(*def)
                .ok_or_else(|| WitError::Unmappable {
                    ty: "missing host signature".into(),
                    at: operation.clone(),
                })
                .and_then(|sig| {
                    wit_func(&name, &Interface::from(sig), &types).map(|(text, _)| text)
                }),
            many => Err(WitError::Claimed {
                operation: operation.clone(),
                count: many.len(),
            }),
        };
        out.insert(operation, rendered);
    }
    out
}

/// One component's exported interface.
struct Api {
    name: String,
    /// Declared type names this interface's signatures touch, so it can `use`
    /// them from `types`.
    uses: BTreeSet<String>,
    funcs: Vec<String>,
}

/// The package text.
/// **Every type a published signature can reach, transitively.**
///
/// Seeded rather than derived from `apis` since 2026-08-20: the host packages
/// this file now emits have signatures too, and `capability.SessionId` is
/// reachable from `pw:host/session#read` and from nothing an export names.
///
/// Only these are emitted. The platform packages declare far more — `Decoder`,
/// `Style`, `ElementRef` — and none of them appears in a signature the host
/// calls. Emitting the whole program's types would put shapes on the ABI that
/// nothing crosses it, and would make any one of them unmappable a refusal for
/// a package that never needed it.
fn reachable(types: &Types, seeds: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = seeds.into_iter().collect();
    while let Some(path) = stack.pop() {
        if !out.insert(path.clone()) {
            continue;
        }
        let Some(def) = types.defs.iter().find(|d| d.name() == path) else {
            continue;
        };
        let mut found: BTreeSet<String> = BTreeSet::new();
        for written in def.referenced() {
            if let Some(ty) = written.resolved() {
                types.declared_within(ty, &mut found);
            }
        }
        stack.extend(found);
    }
    out
}

/// The type definitions, in declaration order, refusing anything unmappable.
///
/// **Refusing, not emitting a placeholder.** A record whose field has no WIT
/// form is one a host would decode into something it never was, and this is
/// reached only for types an export actually names.
fn render_types(
    types: &Types,
    wanted: &BTreeSet<String>,
    sigs: &Signatures,
) -> Result<String, WitError> {
    let mut out = String::new();
    for def in &types.defs {
        if !wanted.contains(def.name()) {
            continue;
        }
        let name = def.name();
        if let Some(id) = types.def_of(name)
            && crate::recursion::contains_itself(sigs, id)
        {
            out.push_str(&render_graph(def, id, types, sigs)?);
            continue;
        }
        out.push_str(&render_shape(def, &escaped(&ident(name)), &|ty| {
            wit_type(ty, types, name)
        })?);
    }
    Ok(out)
}

/// **A record, a variant or an alias**, named `id`, each field's type
/// written by `field`: the type's own, or its node's (ADR-0194).
fn render_shape(
    def: &TypeDef,
    id: &str,
    field: &dyn Fn(&TypeResolution) -> Result<String, WitError>,
) -> Result<String, WitError> {
    let mut out = String::new();
    match def {
        TypeDef::Record { fields, .. } => {
            if fields.is_empty() {
                // A WIT record must have at least one field. A Pleris type
                // with none carries no information, and a nameable type
                // carrying none is the nearest honest thing.
                out.push_str(&format!("    type {id} = tuple<>;\n"));
                return Ok(out);
            }
            out.push_str(&format!("    record {id} {{\n"));
            for (f, ty) in fields {
                out.push_str(&format!(
                    "        {}: {},\n",
                    escaped(&ident(f)),
                    field(ty)?
                ));
            }
            out.push_str("    }\n");
        }
        TypeDef::Variant { cases, .. } => {
            out.push_str(&format!("    variant {id} {{\n"));
            for (case, fields) in cases {
                let payload = match fields.as_slice() {
                    [] => String::new(),
                    [ty] => format!("({})", field(ty)?),
                    many => format!(
                        "(tuple<{}>)",
                        many.iter()
                            .map(field)
                            .collect::<Result<Vec<_>, _>>()?
                            .join(", ")
                    ),
                };
                out.push_str(&format!("        {}{},\n", escaped(&ident(case)), payload));
            }
            out.push_str("    }\n");
        }
        TypeDef::Alias { of, .. } => {
            out.push_str(&format!("    type {id} = {};\n", field(of)?));
        }
    }
    Ok(out)
}

/// The WIT identifier of a type's node type (ADR-0194), as a name
/// [`no_collisions`] checks: `rec.thread.Comment` → `rec-thread-comment-node`.
fn node_name(path: &str) -> String {
    format!("{path} node")
}

/// **A type that contains itself, as its nodes** (ADR-0194).
///
/// WIT has no type that contains itself, and the Canonical ABI no layout for
/// one, so the type is a list of its nodes: its fields as written, except
/// that each field holding a `List` of the type holds a `list<u32>` of node
/// indices. Node 0 is the value; the nodes are in level order, so each
/// node's children are the next indices no node before it has claimed.
fn render_graph(
    def: &TypeDef,
    id: DefId,
    types: &Types,
    sigs: &Signatures,
) -> Result<String, WitError> {
    let name = def.name();
    crate::recursion::self_slots(sigs, id).map_err(|why| WitError::Recursive {
        ty: name.to_string(),
        why,
    })?;
    // Each part that holds the type holds node indices (ADR-0194, ADR-0202).
    let field = |ty: &TypeResolution| -> Result<String, WitError> {
        use crate::recursion::Slot;
        match ty.resolved().and_then(|t| crate::recursion::slot_of(t, id)) {
            Some(Slot::List) => Ok("list<u32>".to_string()),
            Some(Slot::Box) => Ok("u32".to_string()),
            Some(Slot::Option) => Ok("option<u32>".to_string()),
            None => wit_type(ty, types, name),
        }
    };
    let me = escaped(&ident(name));
    let node = escaped(&ident(&node_name(name)));
    let mut out = format!(
        "    /// `{name}` contains itself, so it crosses as its nodes: node 0 is the\n    \
         /// value, and each `list<u32>`, `u32` or `option<u32>` holds a node's\n    \
         /// children's indices, which continue the indices the nodes before it\n    \
         /// hold, in level order.\n    \
         type {me} = list<{node}>;\n"
    );
    if let TypeDef::Alias { .. } = def {
        return Err(WitError::Recursive {
            ty: name.to_string(),
            why: "an opaque type that contains itself has no nodes of its own".to_string(),
        });
    }
    out.push_str(&render_shape(def, &node, &field)?);
    Ok(out)
}

/// The package holding every type that crosses a boundary.
///
/// Separate from [`PACKAGE`] since 2026-08-20, and the separation is forced:
/// the host packages this file also emits need these types, and a `pw:app`
/// world imports `store:data/carts`, so putting the types in `pw:app` would
/// make the two packages depend on each other.
///
/// **Unversioned deliberately.** A nested package declaration and a `use` that
/// names it must agree, and `pw:types@0.1.0` declared inside a file is not
/// found by `use pw:types/types` — `wit_parser` reports *package not found*.
pub const TYPES_PACKAGE: &str = "pw:types";

/// One package of host operations, as the Pleris declarations define them.
struct HostPackage {
    /// `store:data`
    name: String,
    /// interface name -> its functions, and the types they name.
    interfaces: BTreeMap<String, (BTreeSet<String>, Vec<String>)>,
}

fn render(type_text: &str, apis: &[Api], worlds: &[World], hosts: &[HostPackage]) -> String {
    let mut out = String::new();
    out.push_str("// Generated by `pw emit-wit`. Do not edit.\n");
    out.push_str("//\n");
    out.push_str("// One world per ComponentContract. A world's imports are exactly the\n");
    out.push_str("// contract's imports, projected onto interfaces — see pw-core/src/wit.rs.\n");
    out.push_str("//\n");
    out.push_str("// The host packages are HERE, generated, because the Pleris declaration is\n");
    out.push_str("// the ABI authority for every operation this program declares. Architect\n");
    out.push_str("// ruling, 2026-08-20: a deployment IMPLEMENTS this interface; it does not\n");
    out.push_str("// independently specify what the interface means. One interface has one\n");
    out.push_str("// ABI authority, never a Pleris signature plus a hand-authored WIT one\n");
    out.push_str("// with a comparison keeping them synchronized.\n\n");
    out.push_str(&format!("package {PACKAGE};\n\n"));

    for a in apis {
        out.push_str(&format!("interface {} {{\n", a.name));
        if !a.uses.is_empty() {
            let names: Vec<String> = a.uses.iter().map(|u| escaped(&ident(u))).collect();
            out.push_str(&format!(
                "    use {TYPES_PACKAGE}/types.{{{}}};\n",
                names.join(", ")
            ));
        }
        for f in &a.funcs {
            out.push_str(&format!("    {f}\n"));
        }
        out.push_str("}\n\n");
    }

    for w in worlds {
        out.push_str(&format!("world {} {{\n", escaped(&w.name)));
        for i in &w.imports {
            out.push_str(&format!("    import {i};\n"));
        }
        out.push_str(&format!("    export {};\n", w.exports));
        out.push_str("}\n\n");
    }

    // The dependency packages, nested. `push_dir` wants the directory's own
    // package unbraced and every other one braced, so this order is not a
    // stylistic choice.
    if !type_text.is_empty() {
        out.push_str(&format!("package {TYPES_PACKAGE} {{\n"));
        out.push_str("    interface types {\n");
        for line in type_text.lines() {
            out.push_str(&format!("    {line}\n"));
        }
        out.push_str("    }\n}\n\n");
    }

    for h in hosts {
        out.push_str(&format!("package {} {{\n", h.name));
        for (iface, (uses, funcs)) in &h.interfaces {
            out.push_str(&format!("    interface {iface} {{\n"));
            if !uses.is_empty() {
                let names: Vec<String> = uses.iter().map(|u| escaped(&ident(u))).collect();
                out.push_str(&format!(
                    "        use {TYPES_PACKAGE}/types.{{{}}};\n",
                    names.join(", ")
                ));
            }
            for f in funcs {
                out.push_str(&format!("        {f}\n"));
            }
            out.push_str("    }\n");
        }
        out.push_str("}\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangling_is_kebab_and_lossy_in_a_way_callers_must_check() {
        assert_eq!(ident("store.page.StorePage"), "store-page-store-page");
        assert_eq!(ident("add_to_cart"), "add-to-cart");
        assert_eq!(ident("StoreId"), "store-id");
        assert_eq!(ident("pw:host/database"), "pw-host-database");
        // A leading digit is not a WIT identifier.
        assert_eq!(ident("2fast"), "x-2fast");
        // And the lossiness, stated: two Pleris names, one WIT identifier.
        assert_eq!(ident("store.page.Cart"), ident("store.page.cart"));
    }

    #[test]
    fn a_collision_is_refused_rather_than_resolved() {
        // Silently merging two identities is `docs/RISK_QUEUE.md` 34 in a new
        // place, and the merged one would be exported to a host.
        let err = no_collisions(["shop.Cart".to_string(), "shop.cart".to_string()])
            .expect_err("a collision");
        assert!(matches!(err, WitError::Collision { .. }), "{err:?}");
        // ...and the same name twice is not a collision.
        assert!(no_collisions(["shop.Cart".to_string(), "shop.Cart".to_string()]).is_ok());
    }

    /// A checked program, since a type name only means something from the unit
    /// it was written in.
    fn analysed(src: &str) -> (Vec<crate::hir::Hir>, Types) {
        let hirs = vec![crate::lower::lower_file(
            src,
            &pw_syntax::parse_tree(src).green,
        )];
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = Workspace::build(&refs);
        let sigs = Signatures::build(&ws, &refs);
        let types = Types::build(&refs, &ws, &sigs);
        (hirs, types)
    }

    // The unit tests enter through the same syntax/resolution boundary as an
    // annotation. The WIT projection itself never accepts a type spelling.
    fn project_written(
        source: &str,
        types: &Types,
        ws: &Workspace,
        unit: usize,
        at: &str,
    ) -> Result<String, WitError> {
        let written = crate::lower::type_fragment(source).expect("valid type syntax");
        let resolved = crate::resolved::resolve(ws, unit, None, &[], &written, 0..source.len());
        wit_type(&resolved, types, at)
    }

    const SHOP: &str = "\
module shop

type Store = Store { id: Int }
";

    #[test]
    fn an_unmappable_type_is_refused_rather_than_guessed() {
        // The direction that matters. `string` for anything unrecognised
        // produces a world that parses, links, and decodes a value into
        // something it never was.
        let (hirs, types) = analysed(SHOP);
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = Workspace::build(&refs);

        let err = project_written("Whatever", &types, &ws, 0, "f").expect_err("unmappable");
        assert_eq!(
            err,
            WitError::Unmappable {
                ty: "Whatever".into(),
                at: "f".into()
            }
        );
        // The control: the primitives and the carriers do map.
        assert_eq!(project_written("Int", &types, &ws, 0, "f").unwrap(), "s64");
        assert_eq!(
            project_written("Result<Int, String>", &types, &ws, 0, "f").unwrap(),
            "result<s64, string>"
        );
        assert_eq!(
            project_written("List<Option<Bool>>", &types, &ws, 0, "f").unwrap(),
            "list<option<bool>>"
        );
    }

    #[test]
    fn a_declared_type_is_named_by_its_qualified_path() {
        // **Not by its bare name.** `capability.SessionId` and
        // `domain.SessionId` are two types, and keying WIT by `Store` rather
        // than `shop.Store` made both of them one `session-id` — which
        // `wit-parser` refused as a duplicate definition. It refused; a
        // generator that deduplicated instead would have put one type on the
        // wire where the program has two.
        let (hirs, types) = analysed(SHOP);
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = Workspace::build(&refs);

        assert_eq!(
            project_written("Store", &types, &ws, 0, "f").unwrap(),
            "shop-store"
        );
        assert_eq!(
            project_written("Result<Store, String>", &types, &ws, 0, "f").unwrap(),
            "result<shop-store, string>"
        );
    }

    #[test]
    fn two_modules_declaring_one_name_stay_two_types() {
        // The case that produced the rule, as a program: `SessionId` in two
        // modules. Both are emitted, under different identifiers, and a
        // signature gets whichever one it can see.
        let src = "\
module a

opaque type SessionId = String
";
        let other = "\
module b

opaque type SessionId = String
";
        let hirs = [
            crate::lower::lower_file(src, &pw_syntax::parse_tree(src).green),
            crate::lower::lower_file(other, &pw_syntax::parse_tree(other).green),
        ];
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = Workspace::build(&refs);
        let sigs = Signatures::build(&ws, &refs);
        let types = Types::build(&refs, &ws, &sigs);
        let ws = Workspace::build(&refs);

        assert_eq!(types.known.len(), 2, "{:?}", types.known);
        assert_eq!(
            project_written("SessionId", &types, &ws, 0, "f").unwrap(),
            "a-session-id"
        );
        assert_eq!(
            project_written("SessionId", &types, &ws, 1, "f").unwrap(),
            "b-session-id"
        );
    }
}
