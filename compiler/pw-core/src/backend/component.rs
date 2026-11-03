//! **E10-I steps 5 and 6: the component, and the independent check of it.**
//!
//! ```text
//! the program's WIT package        wit::package, from the contracts
//!       ↓  wit-parser
//! Resolve + the component's world
//!       ↓  wasm::core_module         Pleris owns the lowering facts
//! a core module
//!       ↓  wit-component             upstream owns the component format
//! a component
//!       ↓  wit-component::decode     read back by the parser, not by us
//! audit: every function the component imports and exports has exactly the
//!        component-level type the world fixed
//! ```
//!
//! # Why the audit is at the component level
//!
//! NEXT.md, "the component-level audit is mandatory": `result<domain-cart, ..>`
//! and `result<domain-store, ..>` flatten to one core signature — a pointer.
//! A check of core imports can never tell whether an artifact imports the
//! operation the contract meant. So the audit reads the component's own types
//! back through `wit-component`'s decoder and compares them with the world's,
//! structurally for anonymous constructors and by declaration for named types.

use wit_component::{ComponentEncoder, DecodedWasm, StringEncoding, embed_component_metadata};
use wit_parser::{
    Function as WitFunction, PackageId, Resolve, Type as WitType, TypeDefKind, WorldId, WorldItem,
};

use super::ir::Function;
use super::wasm::{Encoding, core_module_with};

/// A compiled component and the facts it was built from.
#[derive(Debug, Clone)]
pub struct Component {
    /// The core module, before wrapping.
    pub core: Vec<u8>,
    /// The component binary.
    pub bytes: Vec<u8>,
    /// The world it implements.
    pub world: String,
    /// The core module's imports as `interface#function`, from the module
    /// itself: what the artifact asks the host for.
    pub imports: Vec<String>,
}

/// Parse a WIT package text, as `wit::package` emits it, into a `Resolve`.
pub fn parse(wit: &str) -> Result<(Resolve, PackageId), String> {
    parse_located(wit).map_err(|failure| failure.message)
}

/// Upstream's typed error span locates the emitted ABI name; its existing
/// world identity locates the responsible Pleris declaration. This metadata
/// never decides support: the same upstream parser remains the authority.
pub(crate) struct PreparationError {
    pub message: String,
    generated: Option<std::ops::Range<usize>>,
}
impl PreparationError {
    pub(crate) fn declaration(
        &self,
        wit: &str,
        worlds: &[crate::wit::World],
    ) -> Option<crate::resolve::DefId> {
        let name = wit.get(self.generated.clone()?)?.trim_start_matches('%');
        worlds
            .iter()
            .rev()
            .find(|w| w.exports == name || w.name == name)
            .map(|w| w.declaration)
    }
}
fn parse_located(wit: &str) -> Result<(Resolve, PackageId), PreparationError> {
    let mut resolve = Resolve::new();
    let pkg = resolve.push_str("pw-app.wit", wit).map_err(|error| {
        let generated = error.chain().find_map(|layer| {
            let span = layer
                .downcast_ref::<wit_parser::ParseError>()
                .map(|e| e.kind().span())
                .or_else(|| {
                    layer
                        .downcast_ref::<wit_parser::ResolveError>()
                        .map(|e| e.kind().span())
                })?;
            span.is_known()
                .then(|| span.start() as usize..span.end() as usize)
        });
        PreparationError {
            message: format!("the generated WIT does not parse: {error:#}"),
            generated,
        }
    })?;
    Ok((resolve, pkg))
}

/// **Compile one exported function of one world into a component.**
///
/// `program` is the lowered program the function belongs to: its imports, and
/// the declared types a body may build without the world naming them, found
/// in the world by `idents` when it does (ADR-0039 §5).
pub fn build(
    wit: &str,
    world: &crate::wit::World,
    function: &Function,
    program: &super::ir::Program,
    idents: &std::collections::BTreeMap<crate::resolve::DefId, String>,
) -> Encoding<Component> {
    let (resolve, pkg) = match parse(wit) {
        Ok(r) => r,
        Err(why) => return Encoding::Blocked { why },
    };
    build_in(&resolve, pkg, world, function, program, idents)
}

fn build_in(
    resolve: &Resolve,
    pkg: PackageId,
    world: &crate::wit::World,
    function: &Function,
    program: &super::ir::Program,
    idents: &std::collections::BTreeMap<crate::resolve::DefId, String>,
) -> Encoding<Component> {
    let world_id = match resolve.select_world(&[pkg], Some(&world.name)) {
        Ok(w) => w,
        Err(e) => {
            return Encoding::Blocked {
                why: format!("the package has no world `{}`: {e:#}", world.name),
            };
        }
    };
    if world.declaration != function.def {
        return Encoding::Blocked {
            why: format!(
                "`{}` is not the declaration the world `{}` was generated for",
                function.export, world.name
            ),
        };
    }
    let [export] = world.functions.as_slice() else {
        return Encoding::Unsupported {
            construct: "a component exporting more than one function",
            reason: format!("`{}` exports {:?}", world.name, world.functions),
        };
    };

    let core = match core_module_with(
        resolve,
        world_id,
        function,
        export,
        &program.imports,
        &program.types,
        idents,
    ) {
        Encoding::Encoded(c) => c,
        Encoding::Unsupported { construct, reason } => {
            return Encoding::Unsupported { construct, reason };
        }
        Encoding::Blocked { why } => return Encoding::Blocked { why },
    };

    let mut with_type = core.clone();
    if let Err(e) =
        embed_component_metadata(&mut with_type, resolve, world_id, StringEncoding::UTF8)
    {
        return Encoding::Blocked {
            why: format!("wit-component could not embed the world: {e:#}"),
        };
    }
    let bytes = match ComponentEncoder::default()
        .validate(true)
        .module(&with_type)
        .and_then(|e| e.encode())
    {
        Ok(b) => b,
        Err(e) => {
            return Encoding::Blocked {
                why: format!("wit-component refused the core module: {e:#}"),
            };
        }
    };

    let imports = match core_imports(&core) {
        Ok(i) => i,
        Err(why) => return Encoding::Blocked { why },
    };

    Encoding::Encoded(Component {
        core,
        bytes,
        world: world.name.clone(),
        imports,
    })
}

/// The core module's function imports, as `module#field`.
fn core_imports(core: &[u8]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(core) {
        if let wasmparser::Payload::ImportSection(reader) = payload.map_err(|e| e.to_string())? {
            for import in reader.into_imports() {
                let import = import.map_err(|e| e.to_string())?;
                out.push(format!("{}#{}", import.module, import.name));
            }
        }
    }
    Ok(out)
}

/// **The component-level audit.** Every function the component imports or
/// exports must have exactly the type `world` gives it in `wit`, and the
/// component may import nothing the world does not.
///
/// Reads the component with `wit-component`'s decoder — the component
/// format's own reader — and never with anything in this repository.
/// Returns how many functions were compared, so a caller can hold the audit
/// to a floor: an audit that compared nothing agrees with everything.
pub fn audit(component: &[u8], wit: &str, world: &str) -> Result<usize, Vec<String>> {
    let (expected, pkg) = parse(wit).map_err(|e| vec![e])?;
    audit_in(component, &expected, pkg, world)
}

fn audit_in(
    component: &[u8],
    expected: &Resolve,
    pkg: PackageId,
    world: &str,
) -> Result<usize, Vec<String>> {
    let expected_world = expected
        .select_world(&[pkg], Some(world))
        .map_err(|e| vec![format!("the package has no world `{world}`: {e:#}")])?;
    let (found, found_world) = match wit_component::decode(component) {
        Ok(DecodedWasm::Component(resolve, world)) => (resolve, world),
        Ok(DecodedWasm::WitPackage(..)) => {
            return Err(vec![
                "the artifact is a WIT package, not a component".into(),
            ]);
        }
        Err(e) => return Err(vec![format!("the artifact does not decode: {e:#}")]),
    };

    let mut wrong = Vec::new();
    let mut compared = 0usize;
    let sides = [
        (
            "import",
            &found.worlds[found_world].imports,
            &expected.worlds[expected_world].imports,
        ),
        (
            "export",
            &found.worlds[found_world].exports,
            &expected.worlds[expected_world].exports,
        ),
    ];
    for (side, found_items, expected_items) in sides {
        for (key, item) in found_items {
            let WorldItem::Interface { id, .. } = item else {
                continue;
            };
            let name = found.name_world_key(key);
            let want = expected_items.iter().find_map(|(k, i)| match i {
                WorldItem::Interface { id, .. } if expected.name_world_key(k) == name => Some(*id),
                _ => None,
            });
            let Some(want) = want else {
                // A type-only interface a component imports because its
                // functions `use` it is not an operation.
                if found.interfaces[*id].functions.is_empty() {
                    continue;
                }
                wrong.push(format!(
                    "the component {side}s `{name}`, which the world does not"
                ));
                continue;
            };
            for (fname, f) in &found.interfaces[*id].functions {
                let Some(g) = expected.interfaces[want].functions.get(fname) else {
                    wrong.push(format!(
                        "the component {side}s `{name}#{fname}`, which the world does not declare"
                    ));
                    continue;
                };
                compared += 1;
                let (a, b) = (render_fn(&found, f), render_fn(expected, g));
                if a != b {
                    wrong.push(format!(
                        "`{name}#{fname}` is `{a}` in the component and `{b}` in the world"
                    ));
                }
            }
        }
    }
    match wrong.is_empty() {
        true => Ok(compared),
        false => Err(wrong),
    }
}

/// A function's component-level type, written out with every alias followed
/// and every named type by its qualified name. Two renderings are equal
/// exactly when the types are, which is what makes this comparison sound for
/// types from two different `Resolve`s.
fn render_fn(resolve: &Resolve, f: &WitFunction) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, render(resolve, &p.ty)))
        .collect();
    match &f.result {
        Some(r) => format!("func({}) -> {}", params.join(", "), render(resolve, r)),
        None => format!("func({})", params.join(", ")),
    }
}

fn render(resolve: &Resolve, t: &WitType) -> String {
    let t = {
        let mut t = *t;
        while let WitType::Id(id) = t {
            match &resolve.types[id].kind {
                TypeDefKind::Type(inner) => t = *inner,
                _ => break,
            }
        }
        t
    };
    match t {
        WitType::Id(id) => {
            let def = &resolve.types[id];
            if let Some(name) = &def.name {
                // A named type is its declaration: the interface and package
                // it lives in, and its name.
                let owner = match def.owner {
                    wit_parser::TypeOwner::Interface(i) => {
                        let iface = &resolve.interfaces[i];
                        let pkg = iface
                            .package
                            .map(|p| resolve.packages[p].name.to_string())
                            .unwrap_or_default();
                        format!("{pkg}/{}", iface.name.clone().unwrap_or_default())
                    }
                    _ => String::new(),
                };
                return format!("{owner}.{name}");
            }
            match &def.kind {
                TypeDefKind::Result(r) => format!(
                    "result<{}, {}>",
                    r.ok.map(|t| render(resolve, &t))
                        .unwrap_or_else(|| "_".into()),
                    r.err
                        .map(|t| render(resolve, &t))
                        .unwrap_or_else(|| "_".into())
                ),
                TypeDefKind::List(t) => format!("list<{}>", render(resolve, t)),
                TypeDefKind::Option(t) => format!("option<{}>", render(resolve, t)),
                TypeDefKind::Tuple(t) => format!(
                    "tuple<{}>",
                    t.types
                        .iter()
                        .map(|t| render(resolve, t))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                other => format!("{other:?}"),
            }
        }
        other => format!("{other:?}"),
    }
}

/// The world a component was built for, and whether the component's own type
/// agrees with it — the question step 6 asks of every artifact.
pub fn world_of(wit: &str, world: &str) -> Result<(Resolve, WorldId), String> {
    let (resolve, pkg) = parse(wit)?;
    let id = resolve
        .select_world(&[pkg], Some(world))
        .map_err(|e| format!("{e:#}"))?;
    Ok((resolve, id))
}

/// A component compiled from a checked program, with the two artifacts that
/// describe it: the WIT it implements and the contract the host admits it by.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub component: Component,
    pub wit: String,
    pub contract: crate::contract::ComponentContract,
}

/// A checked, lowered program with the WIT its contracts fix: what every
/// component of it is built from. Built once per program by [`with_program`].
pub(crate) struct Program<'p> {
    expected: &'p Resolve,
    package: PackageId,

    hirs: &'p [&'p crate::hir::Hir],
    wit: &'p str,
    /// One world per contract, in contract order: a world is found through the
    /// contract it was generated from, never by re-deriving its name.
    worlds: Vec<(
        &'p crate::contract::ComponentContract,
        &'p crate::wit::World,
    )>,
    lowered: &'p super::ir::Program,
    /// Each declared type's identifier in the WIT package.
    idents: &'p std::collections::BTreeMap<crate::resolve::DefId, String>,
    /// What the lowering refused, per declaration.
    refusals: &'p [(
        crate::resolve::DefId,
        super::ir::Lowering<super::ir::Function>,
    )],
}

impl Program<'_> {
    pub(crate) fn lowered(&self) -> &super::ir::Program {
        self.lowered
    }

    /// Why this declaration did not lower, in the lowering's own words.
    fn refused(&self, def: crate::resolve::DefId) -> String {
        self.refusals
            .iter()
            .filter(|(d, _)| *d == def)
            .map(|(_, r)| r.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Is this declaration's body a placeholder the author has not written?
    fn placeholder(&self, def: crate::resolve::DefId) -> bool {
        self.refusals.iter().any(|(d, r)| {
            *d == def
                && matches!(
                    r,
                    super::ir::Lowering::Unsupported { construct, .. }
                        if *construct == super::lower::PLACEHOLDER
                )
        })
    }
}

/// Check (the backend only ever sees a program that checked), lower, generate
/// the WIT, and hand the result to `f`. The one setup [`compile`] and
/// [`compile_all`] share, so the two cannot build a component differently.
fn with_program<R>(
    units: &[crate::check::Unit],
    f: impl FnOnce(&Program<'_>) -> Result<R, String>,
) -> Result<R, String> {
    crate::program::with_backend(units, |checked| with_checked(checked, f))
}

pub(crate) fn with_checked<R>(
    checked: &super::lower::Checked<'_>,
    f: impl FnOnce(&Program<'_>) -> Result<R, String>,
) -> Result<R, String> {
    let cx = checked.context();
    let (hirs, ws, contracts) = (cx.hirs, cx.ws, cx.contracts);
    let (wit, worlds) =
        crate::wit::package_prepared(hirs, ws, contracts, cx.sigs, &checked.pages().derived)
            .map_err(|e| format!("WIT: {}", e.error))?;
    with_package(checked, &wit, &worlds, f).map_err(|failure| failure.message)
}

pub(crate) fn with_package<R>(
    checked: &super::lower::Checked<'_>,
    wit: &str,
    worlds: &[crate::wit::World],
    f: impl FnOnce(&Program<'_>) -> Result<R, String>,
) -> Result<R, PreparationError> {
    let cx = checked.context();
    let (hirs, ws, contracts) = (cx.hirs, cx.ws, cx.contracts);
    let (expected, package) = parse_located(wit)?;
    let (lowered, refusals) = super::lower::program_by_declaration(checked);
    let idents = crate::wit::type_idents(hirs, ws);
    f(&Program {
        expected: &expected,
        package,
        hirs,
        wit,
        worlds: contracts.iter().zip(worlds).collect(),
        lowered: &lowered,
        idents: &idents,
        refusals: &refusals,
    })
    .map_err(|message| PreparationError {
        message,
        generated: None,
    })
}

/// **The lowered function a component id names**, and the program it is in,
/// for a second encoder of the same IR (ADR-0044). The same preparation
/// [`compile`] makes, so the two backends cannot lower one declaration
/// differently.
pub(crate) fn lowered<R>(
    units: &[crate::check::Unit],
    component_id: &str,
    f: impl FnOnce(&super::ir::Function, &super::ir::Program) -> Result<R, String>,
) -> Result<R, String> {
    with_program(units, |p| {
        let (_, world) = p
            .worlds
            .iter()
            .find(|(c, _)| c.component_id == component_id)
            .ok_or_else(|| format!("no component `{component_id}` in this program"))?;
        let function = p
            .lowered
            .functions
            .iter()
            .find(|f| finds(f, world))
            .ok_or_else(|| {
                format!(
                    "`{component_id}` did not lower: {}",
                    p.refused(world.declaration)
                )
            })?;
        f(function, p.lowered)
    })
}

/// Every query that reaches no host, by component id, with its lowered
/// function: what a second encoder can compile as pure computation
/// (ADR-0044). One preparation for all of them.
pub(crate) fn pure_queries<R>(
    units: &[crate::check::Unit],
    f: impl FnOnce(&[(String, &super::ir::Function)], &super::ir::Program) -> Result<R, String>,
) -> Result<R, String> {
    with_program(units, |p| pure_prepared(p, f))
}

pub(crate) fn pure_prepared<R>(
    p: &Program<'_>,
    f: impl FnOnce(&[(String, &super::ir::Function)], &super::ir::Program) -> Result<R, String>,
) -> Result<R, String> {
    f(&pure_functions(p), p.lowered)
}

pub(crate) fn pure_functions<'a>(p: &'a Program<'_>) -> Vec<(String, &'a super::ir::Function)> {
    let mut pure = Vec::new();
    for (contract, world) in &p.worlds {
        let query = crate::resolve::declaration(p.hirs, world.declaration)
            .is_some_and(|d| d.kind == crate::hir::DeclKind::Query);
        if let Some(function) = p.lowered.functions.iter().find(|f| finds(f, world))
            && query
            && function.capabilities.is_empty()
        {
            pure.push((contract.component_id.clone(), function));
        }
    }
    pure
}

/// **Compile one component of a program, end to end.**
///
/// The one path the CLI, the tests and the development server share, so no
/// two of them can compile the same command differently.
pub fn compile(units: &[crate::check::Unit], component_id: &str) -> Result<Compiled, String> {
    with_program(units, |p| {
        let (contract, world) = p
            .worlds
            .iter()
            .find(|(c, _)| c.component_id == component_id)
            .ok_or_else(|| format!("no component `{component_id}` in this program"))?;
        let Some(function) = p.lowered.functions.iter().find(|f| finds(f, world)) else {
            return Err(format!(
                "`{component_id}` did not lower: {}",
                p.refused(world.declaration)
            ));
        };
        match build_in(p.expected, p.package, world, function, p.lowered, p.idents) {
            Encoding::Encoded(component) => Ok(Compiled {
                component,
                wit: p.wit.to_string(),
                contract: (*contract).clone(),
            }),
            other => Err(format!("`{component_id}`: {other}")),
        }
    })
}

/// What one contract of a program built to.
#[derive(Debug, Clone)]
pub enum Built {
    /// A command or query: compiled, and audited against its world. `audited`
    /// is how many functions the audit compared.
    Component {
        compiled: Box<Compiled>,
        audited: usize,
    },
    /// A declaration with no component body: a page or a view renders through
    /// the template IR, and a resource declares a lifecycle.
    NoBody { kind: crate::hir::DeclKind },
    /// A command or query whose body is `todo`: its author has not written it,
    /// which is not the backend declining it. A build fails only if something
    /// it builds depends on one (`build::Build::refusals`).
    Placeholder,
    /// A command or query the backend refused, with the reasons.
    Refused(String),
}

/// **Is `f` the function `world` exports?** Its declaration's, or for a
/// computed part (ADR-0226) the one lifted beside that declaration, by its
/// name, which has a `$` no declaration's has: a page has its own function,
/// and one for each part a host computes.
fn finds(f: &Function, world: &crate::wit::World) -> bool {
    f.def == world.declaration
        && match &world.derived {
            Some(name) => f.export == *name,
            None => !f.export.contains('$'),
        }
}

/// **Every component of a program, compiled and audited**, in contract order.
///
/// Checked and lowered once. A command or query that does not compile is
/// reported as refused rather than left out: a build that listed only what
/// compiled would make the backend look finished.
pub fn compile_all(units: &[crate::check::Unit]) -> Result<Vec<(String, Built)>, String> {
    with_program(units, compile_prepared)
}

pub(crate) fn compile_prepared(p: &Program<'_>) -> Result<Vec<(String, Built)>, String> {
    let mut out = Vec::new();
    for (contract, world) in &p.worlds {
        let kind = crate::resolve::declaration(p.hirs, world.declaration).map(|d| d.kind);
        let function = p.lowered.functions.iter().find(|f| finds(f, world));
        let built = match (function, kind) {
            (Some(function), _) => {
                match build_in(p.expected, p.package, world, function, p.lowered, p.idents) {
                    Encoding::Encoded(component) => {
                        match audit_in(&component.bytes, p.expected, p.package, &component.world) {
                            Ok(audited) => Built::Component {
                                compiled: Box::new(Compiled {
                                    component,
                                    wit: p.wit.to_string(),
                                    contract: (*contract).clone(),
                                }),
                                audited,
                            },
                            Err(wrong) => Built::Refused(format!(
                                "the artifact disagrees with its world: {}",
                                wrong.join("; ")
                            )),
                        }
                    }
                    other => Built::Refused(other.to_string()),
                }
            }
            (None, Some(crate::hir::DeclKind::Command | crate::hir::DeclKind::Query))
                if p.placeholder(world.declaration) =>
            {
                Built::Placeholder
            }
            (
                None,
                Some(
                    crate::hir::DeclKind::Command
                    | crate::hir::DeclKind::Query
                    | crate::hir::DeclKind::Fn
                    | crate::hir::DeclKind::Materialize
                    // A predicate with a contract has a body (ADR-0384's H2).
                    | crate::hir::DeclKind::Predicate,
                ),
            ) => Built::Refused(format!("did not lower: {}", p.refused(world.declaration))),
            (None, Some(kind)) => Built::NoBody { kind },
            (None, None) => {
                return Err(format!(
                    "`{}`'s world names a declaration no unit holds",
                    contract.component_id
                ));
            }
        };
        out.push((contract.component_id.clone(), built));
    }
    Ok(out)
}
