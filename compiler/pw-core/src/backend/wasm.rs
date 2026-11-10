//! **Backend IR → a core module that speaks the Canonical ABI.**
//!
//! E10-I step 4. Architect ruling, 2026-08-20:
//!
//! > Pleris is the ABI authority. […] Do **not** hand-implement the Component
//! > Model binary format. Pleris owns the lowering facts; upstream owns the
//! > component-format encoding.
//!
//! So this file emits a **core module** whose imports and exports are exactly
//! what `wit-component` needs to wrap it as the component a world describes,
//! and it decides nothing about the Canonical ABI itself:
//!
//! ```text
//! which core signature a function has   Resolve::wasm_signature
//! how a value flattens                  Resolve::push_flat
//! a type's size, alignment, offsets     wit_parser::SizeAlign
//! a core import's module and name       Resolve::wasm_import_name (Legacy, Sync)
//! a core export's name                  Resolve::wasm_export_name (Legacy, Sync)
//! ```
//!
//! What it decides is **bytes**: which core instructions connect the values
//! the IR names. And, as before, nothing from a spelling:
//!
//! ```text
//! which calls leave the module   the IR already says: ImportCall vs Call
//! which operation one names      the ImportCall's own ImportId
//! what an operation's ABI is     the world, which the contract generated
//! what the export is             the world's declaration, by DefId
//! ```
//!
//! # How a value is held
//!
//! Every IR value is held one of two ways, and the Canonical ABI decides which:
//!
//! ```text
//! Flat     core values in locals, as the value flattens    a parameter, a scalar
//! Memory   the value's canonical layout at an address      a result returned by pointer
//! ```
//!
//! A value moves between the two only through [`load`], which reads a layout
//! into its flat values using `SizeAlign`'s offsets. Passing a result to a
//! call, or returning it, needs no copy when the two positions have one
//! component type — the case `add_to_cart` is: `carts#add`'s result IS the
//! command's result.
//!
//! # The invocation region
//!
//! Linear memory is one bump region: `cabi_realloc` hands out addresses from a
//! mutable global, growing memory as needed and trapping when it cannot, and
//! the export's post-return function resets the global. Everything a call
//! allocates — its arguments, its callees' results, its own result — is
//! reclaimed once the caller has lifted the result.
//!
//! > Invocation-region memory stays explicitly provisional. […] don't let the
//! > fact that E10-A can reclaim everything at invocation end become the
//! > language's permanent escape/lifetime model.
//!
//! Nothing here can express a value outliving its invocation, and that is a
//! property of this file, stated once, rather than an assumption spread through
//! the encoder.

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ElementSection, Elements, EntityType, ExportKind,
    ExportSection, Function, FunctionSection, GlobalSection, GlobalType, ImportSection, MemArg,
    MemorySection, MemoryType, Module, NameMap, NameSection, RefType, TableSection, TableType,
    TypeSection, ValType,
};
use wit_parser::abi::{AbiVariant, FlatTypes, WasmSignature, WasmType};
use wit_parser::{
    Case as WitCase, Field, Function as WitFunction, Int, LiftLowerAbi, ManglingAndAbi, Record,
    Resolve, Result_, SizeAlign, Tuple, Type as WitType, TypeDef as WitTypeDef, TypeDefKind,
    TypeOwner, Variant, WasmExport, WasmExportKind, WasmImport, WorldId, WorldItem, WorldKey,
};

use super::case::{self, Case};
use super::ir::{
    BinaryOp, BuiltinCase, CallableImport, Case as VariantCase, Const, Instr, Shape, Terminator,
    Type, TypeDef, UnaryOp, ValueId, all_instrs,
};
use crate::resolve::DefId;

/// **What an encoding produced, and never silently nothing.**
///
/// Architect ruling, 2026-08-19:
///
/// > **Unsupported IR is a hard backend result.** Never emit `nop`, zero, empty
/// > block, dummy result, etc. for an instruction you haven't implemented.
///
/// A construct encoded to `nop` produces a module that validates, links,
/// instantiates and does the wrong thing — and every check downstream of here
/// reads bytes, so nothing would notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoding<T> {
    Encoded(T),
    /// The encoder does not represent this construct yet, and says which.
    Unsupported {
        construct: &'static str,
        reason: String,
    },
    /// An upstream stage did not produce what this one needs. Not a violation
    /// of anything, and emphatically not something to fill in.
    Blocked {
        why: String,
    },
}

impl<T> Encoding<T> {
    pub fn is_encoded(&self) -> bool {
        matches!(self, Encoding::Encoded(_))
    }

    pub fn encoded(self) -> Option<T> {
        match self {
            Encoding::Encoded(t) => Some(t),
            _ => None,
        }
    }

    fn map<U>(self, f: impl FnOnce(T) -> U) -> Encoding<U> {
        match self {
            Encoding::Encoded(t) => Encoding::Encoded(f(t)),
            Encoding::Unsupported { construct, reason } => {
                Encoding::Unsupported { construct, reason }
            }
            Encoding::Blocked { why } => Encoding::Blocked { why },
        }
    }
}

impl<T> std::fmt::Display for Encoding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Encoding::Encoded(_) => f.write_str("encoded"),
            Encoding::Unsupported { construct, reason } => {
                write!(f, "`{construct}` does not encode: {reason}")
            }
            Encoding::Blocked { why } => write!(f, "blocked upstream: {why}"),
        }
    }
}

macro_rules! refuse {
    ($construct:expr, $($reason:tt)*) => {
        return Encoding::Unsupported { construct: $construct, reason: format!($($reason)*) }
    };
}

macro_rules! blocked {
    ($($why:tt)*) => {
        return Encoding::Blocked { why: format!($($why)*) }
    };
}

/// The legacy (`wit-component`) name mangling with the synchronous ABI: the
/// names `wit-component` and `wasm-tools component new` recognise.
const MANGLING: ManglingAndAbi = ManglingAndAbi::Legacy(LiftLowerAbi::Sync);

/// Where the bump region starts. Address 0 is never handed out, so a null
/// pointer cannot alias an allocation.
const HEAP_BASE: i32 = 16;

/// **The core module for one exported function of one world.**
///
/// `export` is the WIT function the world exports for `function`, as
/// `wit::World::functions` names it. `imports` are the program's callables, of
/// which the module imports exactly those the function calls.
pub fn core_module(
    resolve: &Resolve,
    world: WorldId,
    function: &super::ir::Function,
    export: &str,
    imports: &[CallableImport],
) -> Encoding<Vec<u8>> {
    core_module_with(
        resolve,
        world,
        function,
        export,
        imports,
        &[],
        &BTreeMap::new(),
    )
}

/// [`core_module`], with the program's declared types (ADR-0039 §5): their
/// shapes, and the WIT identifier each declaration's type has in the world's
/// `pw:types` interface, as `wit::type_idents` records them. A body may make a
/// value whose type the world never mentions; the encoder gives it a
/// definition in a private copy of the `Resolve`, and the component is
/// wrapped with the world's own.
pub fn core_module_with(
    world_resolve: &Resolve,
    world: WorldId,
    function: &super::ir::Function,
    export: &str,
    imports: &[CallableImport],
    declared: &[TypeDef],
    idents: &BTreeMap<DefId, String>,
) -> Encoding<Vec<u8>> {
    // The export, and every function compiled beside it (ADR-0050).
    let all: Vec<&super::ir::Function> = std::iter::once(function)
        .chain(function.callees.iter())
        .chain(function.closures.iter().map(|c| &c.function))
        .collect();
    let mut private = world_resolve.clone();
    let (wit, cells) = internal_types(&mut private, &all, declared, idents);
    let resolve = &private;
    let mut sizes = SizeAlign::default();
    sizes.fill(resolve);
    let literals = Literals::of(&all);

    // --- the export, by the world's own description --------------------------
    let Some((export_key, export_fn)) = world_function(resolve, world, true, export) else {
        blocked!(
            "the world `{}` exports no function `{export}`",
            resolve.worlds[world].name
        );
    };
    if export_fn.params.len() != function.params.len() {
        blocked!(
            "`{}` has {} parameters and the world's `{export}` has {}",
            function.export,
            function.params.len(),
            export_fn.params.len()
        );
    }

    // --- the imports this function actually calls, in first-use order --------
    if function.entry().is_none() {
        blocked!("`{}` has no entry block", function.export);
    }
    if function.blocks.len() > 1 {
        refuse!(
            "a function with more than one block",
            "`{}` has {} blocks; the component backend encodes straight-line bodies",
            function.export,
            function.blocks.len()
        );
    }
    let mut used: Vec<(super::ir::ImportId, WorldKey, WitFunction)> = Vec::new();
    // Inside a match arm or an `if` too: until 2026-09-25 only the top of the
    // body was read, so an import called only in an arm had no core import.
    // And in every function compiled beside the export (ADR-0050).
    let calls: Vec<&Instr> = all
        .iter()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| all_instrs(&b.instrs))
        .collect();
    for i in calls {
        let Instr::ImportCall { import, .. } = i else {
            continue;
        };
        if used.iter().any(|(id, _, _)| id == import) {
            continue;
        }
        if !imports.iter().any(|c| &c.id == import) {
            blocked!(
                "`{}` calls `{}` and the program declares no such callable",
                function.export,
                import.qualified()
            );
        }
        let Some((key, func)) = world_import(resolve, world, &import.interface, &import.name)
        else {
            blocked!(
                "`{}` calls `{}` and the world `{}` does not import it",
                function.export,
                import.qualified(),
                resolve.worlds[world].name
            );
        };
        used.push((import.clone(), key, func));
    }

    // --- the module's shape -----------------------------------------------------
    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut type_index: BTreeMap<(Vec<ValType>, Vec<ValType>), u32> = BTreeMap::new();
    let mut ty = |params: Vec<ValType>, results: Vec<ValType>| -> u32 {
        let next = type_index.len() as u32;
        *type_index
            .entry((params.clone(), results.clone()))
            .or_insert_with(|| {
                types.ty().function(params, results);
                next
            })
    };

    let mut import_section = ImportSection::new();
    let mut import_index: BTreeMap<String, (u32, WasmSignature, WitFunction)> = BTreeMap::new();
    for (n, (id, key, func)) in used.iter().enumerate() {
        let sig = resolve.wasm_signature(AbiVariant::GuestImport, func);
        if sig.indirect_params {
            refuse!(
                "a call whose arguments exceed the flat limit",
                "`{}` takes more than {} flat values, which the Canonical ABI passes \
                 through memory",
                id.qualified(),
                Resolve::MAX_FLAT_PARAMS
            );
        }
        let (module_name, field) = resolve.wasm_import_name(
            MANGLING,
            WasmImport::Func {
                interface: Some(key),
                func,
            },
        );
        let t = ty(core_types(&sig.params), core_types(&sig.results));
        import_section.import(&module_name, &field, EntityType::Function(t));
        import_index.insert(id.qualified(), (n as u32, sig, func.clone()));
    }
    let imported = used.len() as u32;
    let realloc_index = imported;
    let export_index = imported + 1;
    let post_index = imported + 2;

    // Past the flat limit, an export's parameters arrive in memory, at the one
    // pointer the core function takes (ADR-0266).
    let export_sig = resolve.wasm_signature(AbiVariant::GuestExport, &export_fn);

    let realloc_ty = ty(vec![ValType::I32; 4], vec![ValType::I32]);
    let export_ty = ty(
        core_types(&export_sig.params),
        core_types(&export_sig.results),
    );
    let post_ty = ty(core_types(&export_sig.results), vec![]);

    // --- the functions compiled beside the export (ADR-0050) --------------------
    let mut callees: BTreeMap<(DefId, Vec<Type>), Callee> = BTreeMap::new();
    let mut callee_types = Vec::new();
    for (n, c) in function.callees.iter().enumerate() {
        let passing = |t: &Type| -> Option<(WitType, Passed)> {
            let w = wit.get(t).copied()?;
            Some((w, passed(resolve, &w)))
        };
        let mut params = Vec::new();
        for (_, t) in &c.params {
            let Some(p) = passing(t) else {
                refuse!(
                    "a parameter with no component type",
                    "`{}` takes a {t:?}, which has no component type here",
                    c.export
                );
            };
            params.push(p);
        }
        let result = match &c.ret {
            Type::Unit => None,
            t => match passing(t) {
                Some(r) => Some(r),
                None => refuse!(
                    "a result with no component type",
                    "`{}` returns a {t:?}, which has no component type here",
                    c.export
                ),
            },
        };
        let core_params: Vec<ValType> = params.iter().flat_map(|(_, p)| p.core()).collect();
        let core_results: Vec<ValType> = result.iter().flat_map(|(_, p)| p.core()).collect();
        callee_types.push(ty(core_params, core_results));
        callees.insert(
            (c.def, c.instance.clone()),
            Callee {
                index: post_index + 1 + CAUSES + n as u32,
                params,
                result,
            },
        );
    }

    // --- function values (ADR-0052) -------------------------------------------
    // Each call through a value is `call_indirect` at the core type of the
    // value's function type: its environment, then its parameters.
    let mut fn_sigs: BTreeMap<Type, FnSig> = BTreeMap::new();
    let mut sig_of = |t: &Type, ty: &mut dyn FnMut(Vec<ValType>, Vec<ValType>) -> u32| {
        if fn_sigs.contains_key(t) {
            return Some(());
        }
        let Type::Function(ps, r) = t else {
            return None;
        };
        let mut params = Vec::new();
        for p in ps {
            let w = wit.get(p).copied()?;
            params.push((w, passed(resolve, &w)));
        }
        let result = match &**r {
            Type::Unit => None,
            r => {
                let w = wit.get(r).copied()?;
                Some((w, passed(resolve, &w)))
            }
        };
        let mut core = vec![ValType::I32];
        core.extend(params.iter().flat_map(|(_, p)| p.core()));
        let results: Vec<ValType> = result.iter().flat_map(|(_, p)| p.core()).collect();
        let type_index = ty(core, results);
        fn_sigs.insert(
            t.clone(),
            FnSig {
                type_index,
                params,
                result,
            },
        );
        Some(())
    };
    for i in all
        .iter()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| all_instrs(&b.instrs))
    {
        if let Instr::Apply { function_ty, .. }
        | Instr::Closure {
            ty: function_ty, ..
        } = i
            && sig_of(function_ty, &mut ty).is_none()
        {
            refuse!(
                "a function value with no component type",
                "`{}` makes or calls a {function_ty:?}, which has no core signature here",
                function.export
            );
        }
    }
    let first_closure = post_index + 1 + CAUSES + function.callees.len() as u32;
    let mut closures: BTreeMap<u32, ClosureCode> = BTreeMap::new();
    for (n, c) in function.closures.iter().enumerate() {
        let f = &c.function;
        let mut captures = Vec::new();
        for (_, t) in &f.params[..c.captures] {
            let Some(w) = wit.get(t).copied() else {
                refuse!(
                    "a capture with no component type",
                    "a function value in `{}` captures a {t:?}",
                    function.export
                );
            };
            captures.push(w);
        }
        let mut env_types = vec![WitType::U32];
        env_types.extend(captures.iter().copied());
        let offsets: Vec<u64> = sizes
            .field_offsets(env_types.iter())
            .into_iter()
            .map(|(o, _)| o.size_wasm32() as u64)
            .collect();
        let env = sizes.record(env_types.iter());
        let signature = Type::Function(
            f.params[c.captures..]
                .iter()
                .map(|(_, t)| t.clone())
                .collect(),
            Box::new(f.ret.clone()),
        );
        if sig_of(&signature, &mut ty).is_none() {
            refuse!(
                "a function value with no component type",
                "a function value in `{}` is a {signature:?}",
                function.export
            );
        }
        closures.insert(
            n as u32,
            ClosureCode {
                index: first_closure + n as u32,
                captures: captures
                    .into_iter()
                    .zip(offsets.into_iter().skip(1))
                    .collect(),
                size: env.size.size_wasm32() as u32,
                align: env.align.align_wasm32() as u32,
                signature,
            },
        );
    }

    // --- the export's body --------------------------------------------------------
    let mut helpers = Helpers {
        first: first_closure + function.closures.len() as u32,
        used: Vec::new(),
        graphs: Vec::new(),
    };
    let shared = Shared {
        resolve,
        sizes: &sizes,
        wit: &wit,
        literals: &literals,
        imports: &import_index,
        realloc_index,
        callees: &callees,
        closures: &closures,
        fn_sigs: &fn_sigs,
        cells: &cells,
    };
    let body = match export_body(shared, function, &export_fn, &export_sig, &mut helpers) {
        Encoding::Encoded(b) => b,
        other => return other.map(|_| unreachable!()),
    };
    let mut internal_bodies = Vec::new();
    for c in &function.callees {
        match internal_body(shared, c, &mut helpers) {
            Encoding::Encoded(b) => internal_bodies.push(b),
            other => return other.map(|_| unreachable!()),
        }
    }
    let mut closure_bodies = Vec::new();
    for (n, c) in function.closures.iter().enumerate() {
        match closure_body(shared, n as u32, c, &mut helpers) {
            Encoding::Encoded(b) => closure_bodies.push(b),
            other => return other.map(|_| unreachable!()),
        }
    }

    let mut funcs = FunctionSection::new();
    funcs.function(realloc_ty);
    funcs.function(export_ty);
    funcs.function(post_ty);
    // A function for each cause a trap has, after `post-return` (ADR-0267).
    let trap_ty = ty(vec![], vec![]);
    for _ in Cause::ALL {
        funcs.function(trap_ty);
    }
    for t in &callee_types {
        funcs.function(*t);
    }
    for c in closures.values() {
        funcs.function(fn_sigs[&c.signature].type_index);
    }
    for h in &helpers.used {
        let (params, results) = h.signature();
        funcs.function(ty(params, results));
    }

    // Enough pages for the literals below the region.
    let heap_base = literals.heap_base();
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: (heap_base as u64).div_ceil(65536).max(1),
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });

    let mut globals = GlobalSection::new();
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(heap_base),
    );

    let mut exports = ExportSection::new();
    exports.export(
        &resolve.wasm_export_name(MANGLING, WasmExport::Memory),
        ExportKind::Memory,
        0,
    );
    exports.export(
        &resolve.wasm_export_name(MANGLING, WasmExport::Realloc),
        ExportKind::Func,
        realloc_index,
    );
    exports.export(
        &resolve.wasm_export_name(
            MANGLING,
            WasmExport::Func {
                interface: Some(&export_key),
                func: &export_fn,
                kind: WasmExportKind::Normal,
            },
        ),
        ExportKind::Func,
        export_index,
    );
    exports.export(
        &resolve.wasm_export_name(
            MANGLING,
            WasmExport::Func {
                interface: Some(&export_key),
                func: &export_fn,
                kind: WasmExportKind::PostReturn,
            },
        ),
        ExportKind::Func,
        post_index,
    );

    let mut code = CodeSection::new();
    code.function(&realloc(realloc_index));
    code.function(&body);
    code.function(&post_return(heap_base));
    for _ in Cause::ALL {
        let mut f = Function::new(vec![]);
        f.instruction(&wasm_encoder::Instruction::Unreachable);
        f.instruction(&wasm_encoder::Instruction::End);
        code.function(&f);
    }
    for b in &internal_bodies {
        code.function(b);
    }
    for b in &closure_bodies {
        code.function(b);
    }
    let layouts: Vec<GraphLayout> = helpers.graphs.iter().map(|(_, l)| l.clone()).collect();
    for h in &helpers.used {
        code.function(&h.body(realloc_index, &layouts));
    }

    module.section(&types);
    module.section(&import_section);
    module.section(&funcs);
    // The function values' code, in a table a value's environment indexes
    // (ADR-0052).
    if !closures.is_empty() {
        let n = closures.len() as u64;
        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: n,
            maximum: Some(n),
            shared: false,
        });
        module.section(&tables);
    }
    module.section(&memories);
    module.section(&globals);
    module.section(&exports);
    if !closures.is_empty() {
        let indices: Vec<u32> = closures.values().map(|c| c.index).collect();
        let mut elements = ElementSection::new();
        elements.active(
            Some(0),
            &ConstExpr::i32_const(0),
            Elements::Functions(std::borrow::Cow::Owned(indices)),
        );
        module.section(&elements);
    }
    module.section(&code);
    if !literals.bytes.is_empty() {
        let mut data = DataSection::new();
        data.active(
            0,
            &ConstExpr::i32_const(DATA_BASE),
            literals.bytes.iter().copied(),
        );
        module.section(&data);
    }
    // **Each trap's function named by its cause** (ADR-0267), so the host
    // reads why a component stopped from the frame it stopped in.
    let mut names = NameMap::new();
    for cause in Cause::ALL {
        names.append(
            trap_index(realloc_index, cause),
            &format!("{TRAP_PREFIX}{}", cause.words()),
        );
    }
    let mut section = NameSection::new();
    section.functions(&names);
    module.section(&section);
    Encoding::Encoded(module.finish())
}

/// **Why a component stopped** (ADR-0267): each cause of a trap of the
/// backend's own, called by its trap as a function the module's name section
/// names `pw-trap: <its words>`, so the host can say which. The words are the
/// browser's module's where it traps for the same cause. A zero divisor and
/// a stack too deep are Wasm's own traps, which the host names by their code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    IntOverflow,
    RepeatedKey,
    ListsOfTwoLengths,
    NotAScalar,
    TooLongForMemory,
    MemoryExhausted,
    NotATree,
}

/// How many causes, so how many functions follow `post-return`.
const CAUSES: u32 = Cause::ALL.len() as u32;

/// What a trap's function is named before its cause's words.
pub const TRAP_PREFIX: &str = "pw-trap: ";

impl Cause {
    const ALL: [Cause; 7] = [
        Cause::IntOverflow,
        Cause::RepeatedKey,
        Cause::ListsOfTwoLengths,
        Cause::NotAScalar,
        Cause::TooLongForMemory,
        Cause::MemoryExhausted,
        Cause::NotATree,
    ];

    pub(crate) fn words(self) -> &'static str {
        match self {
            Cause::IntOverflow => "Int overflow",
            Cause::RepeatedKey => "a map or set from outside repeats a key",
            Cause::ListsOfTwoLengths => "lists of two lengths",
            Cause::NotAScalar => "not a Unicode scalar value",
            Cause::TooLongForMemory => "a value too long for memory",
            Cause::MemoryExhausted => "memory exhausted",
            Cause::NotATree => "a value from outside whose nodes are no tree",
        }
    }
}

/// **The function a trap of `cause` calls**: the causes' functions follow
/// `cabi_realloc`, the export and its `post-return`, in `Cause::ALL`'s order.
fn trap_index(realloc_index: u32, cause: Cause) -> u32 {
    realloc_index + 3 + cause as u32
}

/// A map's value (ADR-0057): its type and its offset in the entry. A set's
/// entry has none.
type MapValue = Option<(WitType, u64)>;

/// Where the string literals start. The region starts after them.
const DATA_BASE: i32 = HEAP_BASE;

/// **Every string constant the body names, at its address** (ADR-0039 §3):
/// one data segment below the invocation region. A body with none has none,
/// and its region starts where it always did.
struct Literals {
    at: BTreeMap<String, u32>,
    /// Each case table a body maps with (ADR-0056): the address of its
    /// ranges, their count, the address of its multi entries, their count.
    tables: BTreeMap<Case, [u32; 4]>,
    bytes: Vec<u8>,
}

impl Literals {
    fn of(functions: &[&super::ir::Function]) -> Literals {
        let mut out = Literals {
            at: BTreeMap::new(),
            tables: BTreeMap::new(),
            bytes: Vec::new(),
        };
        let mut cases = BTreeSet::new();
        let mut add = |s: &str| {
            if !out.at.contains_key(s) {
                out.at
                    .insert(s.to_string(), DATA_BASE as u32 + out.bytes.len() as u32);
                out.bytes.extend_from_slice(s.as_bytes());
            }
        };
        for b in functions.iter().flat_map(|f| f.blocks.iter()) {
            for i in all_instrs(&b.instrs) {
                match i {
                    Instr::Const {
                        value: Const::Str(s),
                        ..
                    } => add(s),
                    // A `Bool` written as text is one of these two.
                    Instr::Format { .. } => {
                        add("true");
                        add("false");
                    }
                    Instr::Intrinsic {
                        op: super::ir::Intrinsic::StrToLower,
                        ..
                    } => {
                        cases.insert(Case::Lower);
                    }
                    Instr::Intrinsic {
                        op: super::ir::Intrinsic::StrToUpper,
                        ..
                    } => {
                        cases.insert(Case::Upper);
                    }
                    _ => {}
                }
            }
        }
        // After the strings, so a body that maps no case keeps its layout;
        // each table's words 4-aligned.
        for case in cases {
            let t = case::table(case);
            while !(DATA_BASE as usize + out.bytes.len()).is_multiple_of(4) {
                out.bytes.push(0);
            }
            let ranges = DATA_BASE as u32 + out.bytes.len() as u32;
            let multi = ranges + 16 * t.ranges.len() as u32;
            out.bytes.extend(t.bytes());
            out.tables.insert(
                case,
                [ranges, t.ranges.len() as u32, multi, t.multi.len() as u32],
            );
        }
        out
    }

    /// The region's first address: past the literals, 8-aligned.
    fn heap_base(&self) -> i32 {
        let end = DATA_BASE as u32 + self.bytes.len() as u32;
        (end.div_ceil(8) * 8) as i32
    }
}

/// **The WIT type of each IR type the function uses** (ADR-0039 §5).
///
/// A primitive is itself. A declaration that crosses the boundary is the
/// world's own type, found by the identifier `wit::type_idents` records for
/// it; one that does not is defined here, in the private `Resolve`, from its
/// shape. `option`, `result` and `list` are anonymous: `same_type` compares
/// them by their arguments, so one defined here equals the world's.
/// A type with no WIT form here (a declared variant the world does not name)
/// is left out, and the encoder refuses whatever needs it.
fn internal_types(
    resolve: &mut Resolve,
    functions: &[&super::ir::Function],
    declared: &[TypeDef],
    idents: &BTreeMap<DefId, String>,
) -> (BTreeMap<Type, WitType>, Cells) {
    let world_types = resolve.packages.iter().find_map(|(_, p)| {
        (p.name.namespace == "pw" && p.name.name == "types")
            .then(|| p.interfaces.get("types").copied())
            .flatten()
    });
    let mut wanted: Vec<Type> = Vec::new();
    for function in functions {
        wanted.extend(function.params.iter().map(|(_, t)| t.clone()));
        wanted.push(function.ret.clone());
        for b in &function.blocks {
            for i in all_instrs(&b.instrs) {
                wanted.push(i.ty().clone());
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut cells = Cells::new();
    let mut cx = TypeCx {
        world_types,
        declared,
        idents,
        out: &mut out,
        visiting: Vec::new(),
        holders: holders_of_cycles(declared),
        boxed: boxed_cycles(declared),
        cells: &mut cells,
        deferred: Vec::new(),
        broken: BTreeSet::new(),
    };
    for t in wanted {
        cx.wit(resolve, &t);
    }
    // A list whose element never resolved has no layout, and neither has
    // anything holding one: left out, so the encoder refuses what needs it.
    let broken: BTreeSet<wit_parser::TypeId> = cx
        .broken
        .iter()
        .copied()
        .chain(cx.deferred.iter().map(|(id, _)| *id))
        .collect();
    if !broken.is_empty() {
        out.retain(|_, w| !reaches_any(resolve, *w, &broken, &mut BTreeSet::new()));
    }
    (out, cells)
}

/// **Each box's cell** (ADR-0202): a value of a type that holds itself in
/// place is the address of its cell, so its WIT type is a `u32` of its own,
/// whose cell is the record or variant the type is.
type Cells = BTreeMap<wit_parser::TypeId, wit_parser::TypeId>;

/// Does `t` reach one of `ids`, through every type it is built of?
fn reaches_any(
    resolve: &Resolve,
    t: WitType,
    ids: &BTreeSet<wit_parser::TypeId>,
    seen: &mut BTreeSet<wit_parser::TypeId>,
) -> bool {
    let WitType::Id(id) = t else {
        return false;
    };
    if ids.contains(&id) {
        return true;
    }
    if !seen.insert(id) {
        return false;
    }
    let mut each = |t: &WitType| reaches_any(resolve, *t, ids, seen);
    match &resolve.types[id].kind {
        TypeDefKind::Record(r) => r.fields.iter().any(|f| each(&f.ty)),
        TypeDefKind::Tuple(t) => t.types.iter().any(&mut each),
        TypeDefKind::Variant(v) => v.cases.iter().filter_map(|c| c.ty.as_ref()).any(&mut each),
        TypeDefKind::Option(t) | TypeDefKind::List(t) | TypeDefKind::Type(t) => each(t),
        TypeDefKind::Result(r) => r.ok.iter().chain(r.err.iter()).any(&mut each),
        _ => false,
    }
}

/// A declaration's instance: its identity and the types it is applied to.
type Instance = (DefId, Vec<Type>);

/// **The instances whose values hold a type that contains itself**
/// (ADR-0194): each instance in a cycle of the declarations' shapes, and
/// each that reaches one. A value of one is never the world's type, which
/// holds the cycle as its nodes.
fn holders_of_cycles(declared: &[TypeDef]) -> BTreeSet<Instance> {
    let edges: BTreeMap<Instance, Vec<Instance>> = declared
        .iter()
        .map(|d| {
            let mut next = Vec::new();
            match &d.shape {
                Shape::Alias(of) => next.extend(of.nominals()),
                Shape::Record { fields } => {
                    for (_, t) in fields {
                        next.extend(t.nominals());
                    }
                }
                Shape::Variant { cases } => {
                    for (_, fs) in cases {
                        for t in fs {
                            next.extend(t.nominals());
                        }
                    }
                }
            }
            ((d.def, d.args.clone()), next)
        })
        .collect();
    let reach = |from: &(DefId, Vec<Type>)| {
        let mut seen: BTreeSet<(DefId, Vec<Type>)> = BTreeSet::new();
        let mut stack: Vec<(DefId, Vec<Type>)> = edges.get(from).cloned().unwrap_or_default();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                stack.extend(edges.get(&n).cloned().unwrap_or_default());
            }
        }
        seen
    };
    let cyclic: BTreeSet<(DefId, Vec<Type>)> = edges
        .keys()
        .filter(|k| reach(k).contains(*k))
        .cloned()
        .collect();
    edges
        .keys()
        .filter(|k| cyclic.contains(*k) || reach(k).iter().any(|r| cyclic.contains(r)))
        .cloned()
        .collect()
}

/// **The instances that hold themselves in place** (ADR-0202): each in a
/// cycle of the declarations' shapes that passes through no list, map, set or
/// function value, as `next: Option<Node>` or `Add(Expr, Expr)` does. No
/// layout by type holds one, so a value of one is boxed: the address of its
/// cell.
fn boxed_cycles(declared: &[TypeDef]) -> BTreeSet<Instance> {
    fn in_place(t: &Type, out: &mut Vec<Instance>) {
        match t {
            Type::Nominal(d, args) => out.push((*d, args.clone())),
            Type::Option(a) => in_place(a, out),
            Type::Result(a, b) => {
                in_place(a, out);
                in_place(b, out);
            }
            // Where the value points, or its environment's address.
            Type::List(_) | Type::Set(_) | Type::Map(..) | Type::Function(..) => {}
            Type::Int | Type::Float | Type::Bool | Type::Str | Type::Unit => {}
        }
    }
    let edges: BTreeMap<Instance, Vec<Instance>> = declared
        .iter()
        .map(|d| {
            let mut next = Vec::new();
            match &d.shape {
                Shape::Alias(of) => in_place(of, &mut next),
                Shape::Record { fields } => {
                    for (_, t) in fields {
                        in_place(t, &mut next);
                    }
                }
                Shape::Variant { cases } => {
                    for (_, fs) in cases {
                        for t in fs {
                            in_place(t, &mut next);
                        }
                    }
                }
            }
            ((d.def, d.args.clone()), next)
        })
        .collect();
    edges
        .keys()
        .filter(|k| {
            let mut seen: BTreeSet<&Instance> = BTreeSet::new();
            let mut stack: Vec<&Instance> = edges.get(*k).into_iter().flatten().collect();
            while let Some(n) = stack.pop() {
                if n == *k {
                    return true;
                }
                if seen.insert(n) {
                    stack.extend(edges.get(n).into_iter().flatten());
                }
            }
            false
        })
        .cloned()
        .collect()
}

struct TypeCx<'a> {
    world_types: Option<wit_parser::InterfaceId>,
    declared: &'a [TypeDef],
    idents: &'a BTreeMap<DefId, String>,
    out: &'a mut BTreeMap<Type, WitType>,
    /// Declarations being defined: one met again, with no list between, has
    /// no canonical layout.
    visiting: Vec<(DefId, Vec<Type>)>,
    /// [`holders_of_cycles`].
    holders: BTreeSet<(DefId, Vec<Type>)>,
    /// [`boxed_cycles`]: each defined as a box, whose cell is its shape.
    boxed: BTreeSet<Instance>,
    cells: &'a mut Cells,
    /// **Lists whose elements hold a declaration being defined** (ADR-0194),
    /// each allocated before it with a placeholder element, and its element
    /// set once nothing it holds is being defined. A list's layout does not
    /// read its element's, so the declaration that holds it is laid out
    /// after it, as `SizeAlign` lays out each type after the types it reads.
    deferred: Vec<(wit_parser::TypeId, Element)>,
    /// Deferred lists whose element had no WIT form.
    broken: BTreeSet<wit_parser::TypeId>,
}

/// An anonymous type in the private `Resolve`: an `option`, a `list`, a
/// `tuple`, a `result`, compared by `same_type` by its arguments.
fn anonymous(resolve: &mut Resolve, kind: TypeDefKind) -> WitType {
    WitType::Id(resolve.types.alloc(WitTypeDef {
        name: None,
        kind,
        owner: TypeOwner::None,
        docs: Default::default(),
        stability: Default::default(),
        span: Default::default(),
        external_id: None,
    }))
}

/// A deferred list's element: a value, or a map's entry.
#[derive(Debug, Clone)]
enum Element {
    Value(Type),
    Entry(Type, Type),
}

impl TypeCx<'_> {
    fn wit(&mut self, resolve: &mut Resolve, t: &Type) -> Option<WitType> {
        if let Some(w) = self.out.get(t) {
            return Some(*w);
        }
        let w = match t {
            Type::Int => WitType::S64,
            Type::Float => WitType::F64,
            Type::Bool => WitType::Bool,
            Type::Str => WitType::String,
            Type::Unit => return None,
            // A function value is the address of its environment, which
            // names its code (ADR-0052). It never crosses the boundary.
            Type::Function(..) => WitType::U32,
            Type::Option(inner) => {
                let inner = self.wit(resolve, inner)?;
                anonymous(resolve, TypeDefKind::Option(inner))
            }
            // A list of a declaration being defined, or of anything holding
            // one, is deferred (ADR-0194): its layout is a pointer and a
            // length, whatever its element is.
            Type::List(inner) | Type::Set(inner) if self.holds_visiting(inner) => {
                return Some(self.defer(resolve, Element::Value((**inner).clone())));
            }
            Type::Map(k, v) if self.holds_visiting(k) || self.holds_visiting(v) => {
                return Some(self.defer(resolve, Element::Entry((**k).clone(), (**v).clone())));
            }
            Type::List(inner) => {
                let inner = self.wit(resolve, inner)?;
                anonymous(resolve, TypeDefKind::List(inner))
            }
            // `list<tuple<K, V>>` and `list<T>`, as the world writes them
            // (ADR-0057).
            Type::Map(k, v) => {
                let types = vec![self.wit(resolve, k)?, self.wit(resolve, v)?];
                let entry = anonymous(resolve, TypeDefKind::Tuple(Tuple { types }));
                anonymous(resolve, TypeDefKind::List(entry))
            }
            Type::Set(t) => {
                let t = self.wit(resolve, t)?;
                anonymous(resolve, TypeDefKind::List(t))
            }
            Type::Result(ok, err) => {
                let ok = match **ok {
                    Type::Unit => None,
                    ref t => Some(self.wit(resolve, t)?),
                };
                let err = match **err {
                    Type::Unit => None,
                    ref t => Some(self.wit(resolve, t)?),
                };
                anonymous(resolve, TypeDefKind::Result(Result_ { ok, err }))
            }
            Type::Nominal(def, args) => {
                // The world's own, when the declaration crosses the boundary:
                // one type, whatever a phantom argument is. Never for a type
                // that contains itself, or holds one: the world holds that
                // as its nodes (ADR-0194), and the body as its values.
                if !self.holders.contains(&(*def, args.clone()))
                    && let (Some(iface), Some(ident)) = (self.world_types, self.idents.get(def))
                    && let Some(id) = resolve.interfaces[iface].types.get(ident)
                {
                    WitType::Id(*id)
                } else {
                    // Otherwise this instance's own, under its arguments
                    // (ADR-0062).
                    let d = self
                        .declared
                        .iter()
                        .find(|d| d.def == *def && d.args == *args)?;
                    let key = (*def, args.clone());
                    // A type that holds itself in place (ADR-0202): its value
                    // is the address of its cell, a `u32` of its own, known
                    // before the cell, whose parts that hold the type are the
                    // box.
                    if self.boxed.contains(&key) {
                        let ident = self
                            .idents
                            .get(def)
                            .cloned()
                            .unwrap_or_else(|| crate::wit::ident(&d.name));
                        let boxed = resolve.types.alloc(WitTypeDef {
                            name: Some(format!("{ident}-box")),
                            kind: TypeDefKind::Type(WitType::U32),
                            owner: TypeOwner::None,
                            docs: Default::default(),
                            stability: Default::default(),
                            span: Default::default(),
                            external_id: None,
                        });
                        self.out.insert(t.clone(), WitType::Id(boxed));
                        let Some(WitType::Id(cell)) = self.shape(resolve, def, d) else {
                            self.out.remove(t);
                            return None;
                        };
                        self.cells.insert(boxed, cell);
                        self.settle_deferred(resolve);
                        return Some(WitType::Id(boxed));
                    }
                    if self.visiting.contains(&key) {
                        return None;
                    }
                    self.visiting.push(key);
                    let defined = self.shape(resolve, def, d);
                    self.visiting.pop();
                    let defined = defined?;
                    // Known before the lists that wait for it are given
                    // their elements, which may be this very type.
                    self.out.insert(t.clone(), defined);
                    self.settle_deferred(resolve);
                    defined
                }
            }
        };
        self.out.insert(t.clone(), w);
        Some(w)
    }

    /// A declaration's shape, as a record, a variant, or what an alias is.
    fn shape(&mut self, resolve: &mut Resolve, def: &DefId, d: &TypeDef) -> Option<WitType> {
        match &d.shape {
            Shape::Alias(of) => self.wit(resolve, of),
            Shape::Record { fields } => {
                let mut wit_fields = Vec::new();
                let mut whole = true;
                for (name, ft) in fields {
                    match self.wit(resolve, ft) {
                        Some(ty) => wit_fields.push(Field {
                            name: crate::wit::ident(name),
                            ty,
                            docs: Default::default(),
                            span: Default::default(),
                        }),
                        None => whole = false,
                    }
                }
                whole.then(|| {
                    WitType::Id(
                        resolve.types.alloc(WitTypeDef {
                            name: Some(
                                self.idents
                                    .get(def)
                                    .cloned()
                                    .unwrap_or_else(|| crate::wit::ident(&d.name)),
                            ),
                            kind: TypeDefKind::Record(Record { fields: wit_fields }),
                            owner: TypeOwner::None,
                            docs: Default::default(),
                            stability: Default::default(),
                            span: Default::default(),
                            external_id: None,
                        }),
                    )
                })
            }
            // A declared sum type (ADR-0059): a case of one
            // field carries it; of several, a tuple of them, as
            // `wit.rs` writes the world's.
            Shape::Variant { cases } => {
                let mut wit_cases = Vec::new();
                let mut whole = true;
                for (name, fields) in cases {
                    let mut types = Vec::new();
                    for f in fields {
                        match self.wit(resolve, f) {
                            Some(t) => types.push(t),
                            None => whole = false,
                        }
                    }
                    let ty = match types.as_slice() {
                        [] => None,
                        [one] => Some(*one),
                        _ => Some(anonymous(resolve, TypeDefKind::Tuple(Tuple { types }))),
                    };
                    wit_cases.push(WitCase {
                        name: crate::wit::ident(name),
                        ty,
                        docs: Default::default(),
                        span: Default::default(),
                    });
                }
                whole.then(|| {
                    WitType::Id(
                        resolve.types.alloc(WitTypeDef {
                            name: Some(
                                self.idents
                                    .get(def)
                                    .cloned()
                                    .unwrap_or_else(|| crate::wit::ident(&d.name)),
                            ),
                            kind: TypeDefKind::Variant(Variant { cases: wit_cases }),
                            owner: TypeOwner::None,
                            docs: Default::default(),
                            stability: Default::default(),
                            span: Default::default(),
                            external_id: None,
                        }),
                    )
                })
            }
        }
    }

    /// Does `t` hold a declaration being defined, at any depth?
    fn holds_visiting(&self, t: &Type) -> bool {
        fn walk(cx: &TypeCx<'_>, t: &Type, seen: &mut BTreeSet<(DefId, Vec<Type>)>) -> bool {
            match t {
                Type::Nominal(def, args) => {
                    let key = (*def, args.clone());
                    if cx.visiting.contains(&key) {
                        return true;
                    }
                    if !seen.insert(key) {
                        return false;
                    }
                    let Some(d) = cx
                        .declared
                        .iter()
                        .find(|d| d.def == *def && d.args == *args)
                    else {
                        return false;
                    };
                    match &d.shape {
                        Shape::Alias(of) => walk(cx, of, seen),
                        Shape::Record { fields } => fields.iter().any(|(_, t)| walk(cx, t, seen)),
                        Shape::Variant { cases } => cases
                            .iter()
                            .any(|(_, fs)| fs.iter().any(|t| walk(cx, t, seen))),
                    }
                }
                Type::List(t) | Type::Option(t) | Type::Set(t) => walk(cx, t, seen),
                Type::Result(a, b) | Type::Map(a, b) => walk(cx, a, seen) || walk(cx, b, seen),
                Type::Function(..)
                | Type::Int
                | Type::Float
                | Type::Bool
                | Type::Str
                | Type::Unit => false,
            }
        }
        walk(self, t, &mut BTreeSet::new())
    }

    /// A list allocated now, its element set by [`TypeCx::settle_deferred`].
    fn defer(&mut self, resolve: &mut Resolve, element: Element) -> WitType {
        let id = resolve.types.alloc(WitTypeDef {
            name: None,
            // A placeholder, replaced before any layout is read: a list's
            // own layout does not read it.
            kind: TypeDefKind::List(WitType::U8),
            owner: TypeOwner::None,
            docs: Default::default(),
            stability: Default::default(),
            span: Default::default(),
            external_id: None,
        });
        self.deferred.push((id, element));
        WitType::Id(id)
    }

    /// Give each deferred list whose element no longer holds a declaration
    /// being defined that element.
    fn settle_deferred(&mut self, resolve: &mut Resolve) {
        let mut i = 0;
        while i < self.deferred.len() {
            let ready = match &self.deferred[i].1 {
                Element::Value(t) => !self.holds_visiting(t),
                Element::Entry(k, v) => !self.holds_visiting(k) && !self.holds_visiting(v),
            };
            if !ready {
                i += 1;
                continue;
            }
            let (id, element) = self.deferred.remove(i);
            let wit = match element {
                Element::Value(t) => self.wit(resolve, &t),
                Element::Entry(k, v) => match (self.wit(resolve, &k), self.wit(resolve, &v)) {
                    (Some(k), Some(v)) => Some(WitType::Id(resolve.types.alloc(WitTypeDef {
                        name: None,
                        kind: TypeDefKind::Tuple(Tuple { types: vec![k, v] }),
                        owner: TypeOwner::None,
                        docs: Default::default(),
                        stability: Default::default(),
                        span: Default::default(),
                        external_id: None,
                    }))),
                    _ => None,
                },
            };
            match wit {
                Some(w) => resolve.types[id].kind = TypeDefKind::List(w),
                None => {
                    self.broken.insert(id);
                }
            }
            // Resolving an element may have settled others: start over.
            i = 0;
        }
    }
}

/// The function a world imports or exports, by the interface's world key and
/// the function's WIT name. The key is the one `wit::package` generated from
/// the same `ImportId` the IR carries, so this is a lookup of an identity the
/// world was built from, not a guess from a spelling.
fn world_import(
    resolve: &Resolve,
    world: WorldId,
    interface: &str,
    name: &str,
) -> Option<(WorldKey, WitFunction)> {
    resolve.worlds[world]
        .imports
        .iter()
        .find_map(|(key, item)| match item {
            WorldItem::Interface { id, .. } if resolve.name_world_key(key) == interface => resolve
                .interfaces[*id]
                .functions
                .get(name)
                .map(|f| (key.clone(), f.clone())),
            _ => None,
        })
}

/// The exported function named `name`, from whichever exported interface
/// declares it. Exactly one must.
fn world_function(
    resolve: &Resolve,
    world: WorldId,
    exported: bool,
    name: &str,
) -> Option<(WorldKey, WitFunction)> {
    let items = match exported {
        true => &resolve.worlds[world].exports,
        false => &resolve.worlds[world].imports,
    };
    let found: Vec<(WorldKey, WitFunction)> = items
        .iter()
        .filter_map(|(key, item)| match item {
            WorldItem::Interface { id, .. } => resolve.interfaces[*id]
                .functions
                .get(name)
                .map(|f| (key.clone(), f.clone())),
            WorldItem::Function(f) if f.name == name => Some((key.clone(), f.clone())),
            _ => None,
        })
        .collect();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

fn core_types(ts: &[WasmType]) -> Vec<ValType> {
    ts.iter().map(|t| core_type(*t)).collect()
}

/// A flat Canonical ABI type as a core value type, for 32-bit memories.
fn core_type(t: WasmType) -> ValType {
    match t {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => ValType::I32,
        WasmType::I64 | WasmType::PointerOrI64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
    }
}

/// The flat types of one component type, from `wit-parser`.
fn flat(resolve: &Resolve, t: &WitType) -> Option<Vec<WasmType>> {
    let mut storage = [WasmType::I32; Resolve::MAX_FLAT_PARAMS];
    let mut out = FlatTypes::new(&mut storage);
    resolve.push_flat(t, &mut out).then(|| out.to_vec())
}

/// **Do these two positions hold one component type?**
///
/// Aliases are followed to what they name. A NAMED type — a record, a variant,
/// an enum — is its declaration and is equal only to itself: `domain-store`
/// and `domain-cart` flatten identically and are different types, which is
/// the whole reason the component-level audit exists. An ANONYMOUS
/// constructor — `result<..>`, `list<..>`, `option<..>`, a tuple — has no
/// identity of its own, so each interface that writes one gets its own
/// `TypeId`, and it is compared by its arguments.
fn same_type(resolve: &Resolve, a: &WitType, b: &WitType) -> bool {
    let (a, b) = (dealias(resolve, *a), dealias(resolve, *b));
    match (a, b) {
        (WitType::Id(x), WitType::Id(y)) => {
            if x == y {
                return true;
            }
            let (kx, ky) = (&resolve.types[x], &resolve.types[y]);
            if kx.name.is_some() || ky.name.is_some() {
                return false;
            }
            let opt = |p: &Option<WitType>, q: &Option<WitType>| match (p, q) {
                (Some(p), Some(q)) => same_type(resolve, p, q),
                (None, None) => true,
                _ => false,
            };
            match (&kx.kind, &ky.kind) {
                (TypeDefKind::Result(p), TypeDefKind::Result(q)) => {
                    opt(&p.ok, &q.ok) && opt(&p.err, &q.err)
                }
                (TypeDefKind::List(p), TypeDefKind::List(q))
                | (TypeDefKind::Option(p), TypeDefKind::Option(q)) => same_type(resolve, p, q),
                (TypeDefKind::Tuple(p), TypeDefKind::Tuple(q)) => {
                    p.types.len() == q.types.len()
                        && p.types
                            .iter()
                            .zip(&q.types)
                            .all(|(p, q)| same_type(resolve, p, q))
                }
                _ => false,
            }
        }
        (p, q) => p == q,
    }
}

/// Follow `type a = b` to `b`.
fn dealias(resolve: &Resolve, t: WitType) -> WitType {
    let mut t = t;
    while let WitType::Id(id) = t {
        match &resolve.types[id].kind {
            TypeDefKind::Type(inner) => t = *inner,
            _ => break,
        }
    }
    t
}

/// **Does a world's type hold a node index** (ADR-0194)? No Pleris type is
/// a `u32`, and a function value never crosses, so a `u32` in a world is
/// always a node's index: the type holds a type that contains itself, as its
/// nodes, and the body's value of it is laid out otherwise.
fn holds_index(resolve: &Resolve, t: &WitType) -> bool {
    // A body's own types may contain themselves through a list; a world's
    // never do, but the walk is safe on either.
    fn walk(resolve: &Resolve, t: &WitType, seen: &mut BTreeSet<wit_parser::TypeId>) -> bool {
        match dealias(resolve, *t) {
            WitType::U32 => true,
            WitType::Id(id) if seen.insert(id) => match &resolve.types[id].kind {
                TypeDefKind::Record(r) => r.fields.iter().any(|f| walk(resolve, &f.ty, seen)),
                TypeDefKind::Tuple(t) => t.types.iter().any(|t| walk(resolve, t, seen)),
                TypeDefKind::Variant(v) => v
                    .cases
                    .iter()
                    .filter_map(|c| c.ty.as_ref())
                    .any(|t| walk(resolve, t, seen)),
                TypeDefKind::Option(t) | TypeDefKind::List(t) => walk(resolve, t, seen),
                TypeDefKind::Result(r) => {
                    r.ok.iter()
                        .chain(r.err.iter())
                        .any(|t| walk(resolve, t, seen))
                }
                _ => false,
            },
            _ => false,
        }
    }
    walk(resolve, t, &mut BTreeSet::new())
}

/// **A world's type that is a type that contains itself, as its nodes**
/// (ADR-0194): `list<node>`, where the node is a record or a variant one of
/// whose fields holds its children's indices: a `list<u32>`, or for a type
/// that holds itself in place a `u32` or an `option<u32>` (ADR-0202). The
/// node's type.
fn node_of(resolve: &Resolve, t: &WitType) -> Option<wit_parser::TypeId> {
    let WitType::Id(list) = dealias(resolve, *t) else {
        return None;
    };
    let TypeDefKind::List(element) = &resolve.types[list].kind else {
        return None;
    };
    let WitType::Id(node) = dealias(resolve, *element) else {
        return None;
    };
    // `list<u32>`, `u32` or `option<u32>` (ADR-0194, ADR-0202).
    let indices = |t: &WitType| match dealias(resolve, *t) {
        WitType::U32 => true,
        WitType::Id(l) => matches!(
            resolve.types[l].kind,
            TypeDefKind::List(e) | TypeDefKind::Option(e) if dealias(resolve, e) == WitType::U32
        ),
        _ => false,
    };
    let holds = match &resolve.types[node].kind {
        TypeDefKind::Record(r) => r.fields.iter().any(|f| indices(&f.ty)),
        TypeDefKind::Variant(v) => v.cases.iter().filter_map(|c| c.ty).any(|p| {
            indices(&p)
                || matches!(dealias(resolve, p), WitType::Id(t)
                    if matches!(&resolve.types[t].kind, TypeDefKind::Tuple(tu)
                        if tu.types.iter().any(indices)))
        }),
        _ => false,
    };
    holds.then_some(node)
}

/// **Where a type that contains itself holds itself, and how it is laid
/// out** (ADR-0194): the body's value and its node have one layout, but for
/// each part that holds the type, which in the node holds indices.
#[derive(Debug, Clone)]
struct GraphLayout {
    size: u32,
    align: u32,
    /// A variant's discriminant, where the type is one.
    tag: Option<Int>,
    /// Each part that holds the type: the case that holds it, for a variant,
    /// where it sits, and how it holds the type.
    slots: Vec<(Option<u32>, u64, SlotKind)>,
}

/// **How a part holds the type, in the body and in the node.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    /// A list of the type, its elements where the list points, one after
    /// another (ADR-0194); a `list<u32>` in the node.
    Inline,
    /// A list of boxes, its elements their cells' addresses (ADR-0202); a
    /// `list<u32>` in the node.
    Boxes,
    /// A box, its cell's address (ADR-0202); a `u32` in the node.
    Box,
    /// An option of a box, its payload this far into it (ADR-0202); an
    /// `option<u32>` in the node.
    Option(u64),
}

/// The layout of `tree`, the body's value, against `node`, the world's, or
/// why the two do not match.
fn graph_layout(
    resolve: &Resolve,
    sizes: &SizeAlign,
    cells: &Cells,
    tree: wit_parser::TypeId,
    node: wit_parser::TypeId,
) -> Result<GraphLayout, String> {
    // The element of a list, or the payload of an option, undealiased: a
    // box is an alias whose cell is `tree`.
    let inner = |t: &WitType, list: bool| match dealias(resolve, *t) {
        WitType::Id(l) => match (&resolve.types[l].kind, list) {
            (TypeDefKind::List(e), true) | (TypeDefKind::Option(e), false) => Some(*e),
            _ => None,
        },
        _ => None,
    };
    let is_box = |t: &WitType| matches!(t, WitType::Id(b) if cells.get(b) == Some(&tree));
    let is_u32 = |t: &WitType| dealias(resolve, *t) == WitType::U32;
    // One part of the value against the node's: a part that holds the type
    // against indices, anything else against one type.
    let part = |a: &WitType, b: &WitType| -> Result<Option<SlotKind>, String> {
        let a_list = inner(a, true);
        let b_list = inner(b, true).is_some_and(|e| is_u32(&e));
        let a_option = inner(a, false);
        let b_option = inner(b, false).is_some_and(|e| is_u32(&e));
        let kind = if a_list.is_some_and(|e| dealias(resolve, e) == WitType::Id(tree)) && b_list {
            Some(SlotKind::Inline)
        } else if a_list.is_some_and(|e| is_box(&e)) && b_list {
            Some(SlotKind::Boxes)
        } else if is_box(a) && is_u32(b) {
            Some(SlotKind::Box)
        } else if let Some(payload) = a_option.filter(|e| is_box(e)).filter(|_| b_option) {
            let offset = sizes
                .payload_offset(Int::U8, [None, Some(&payload)])
                .size_wasm32() as u64;
            Some(SlotKind::Option(offset))
        } else if same_type(resolve, a, b) {
            None
        } else {
            return Err("a field of the value and of its node differ".to_string());
        };
        Ok(kind)
    };
    let size = sizes.size(&WitType::Id(tree)).size_wasm32() as u32;
    let align = sizes.align(&WitType::Id(tree)).align_wasm32() as u32;
    if size != sizes.size(&WitType::Id(node)).size_wasm32() as u32 {
        return Err("the value and its node have different sizes".to_string());
    }
    let mut slots = Vec::new();
    let tag = match (&resolve.types[tree].kind, &resolve.types[node].kind) {
        (TypeDefKind::Record(a), TypeDefKind::Record(b)) if a.fields.len() == b.fields.len() => {
            let offsets = sizes.field_offsets(a.fields.iter().map(|f| &f.ty));
            for ((o, _), (fa, fb)) in offsets.iter().zip(a.fields.iter().zip(&b.fields)) {
                if let Some(kind) = part(&fa.ty, &fb.ty)? {
                    slots.push((None, o.size_wasm32() as u64, kind));
                }
            }
            None
        }
        (TypeDefKind::Variant(a), TypeDefKind::Variant(b)) if a.cases.len() == b.cases.len() => {
            let payload = sizes
                .payload_offset(a.tag(), a.cases.iter().map(|c| c.ty.as_ref()))
                .size_wasm32() as u64;
            for (c, (ca, cb)) in a.cases.iter().zip(&b.cases).enumerate() {
                match (ca.ty, cb.ty) {
                    (None, None) => {}
                    (Some(pa), Some(pb)) => {
                        let tuples = match (dealias(resolve, pa), dealias(resolve, pb)) {
                            (WitType::Id(x), WitType::Id(y)) => {
                                match (&resolve.types[x].kind, &resolve.types[y].kind) {
                                    (TypeDefKind::Tuple(x), TypeDefKind::Tuple(y))
                                        if x.types.len() == y.types.len() =>
                                    {
                                        Some((x.types.clone(), y.types.clone()))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        match tuples {
                            Some((xs, ys)) => {
                                let offsets = sizes.field_offsets(xs.iter());
                                for ((o, _), (x, y)) in offsets.iter().zip(xs.iter().zip(&ys)) {
                                    if let Some(kind) = part(x, y)? {
                                        slots.push((
                                            Some(c as u32),
                                            payload + o.size_wasm32() as u64,
                                            kind,
                                        ));
                                    }
                                }
                            }
                            None => {
                                if let Some(kind) = part(&pa, &pb)? {
                                    slots.push((Some(c as u32), payload, kind));
                                }
                            }
                        }
                    }
                    _ => return Err("a case of the value and of its node differ".to_string()),
                }
            }
            Some(a.tag())
        }
        _ => return Err("the value and its node are not one kind of type".to_string()),
    };
    if slots.is_empty() {
        return Err("the value holds nothing of itself".to_string());
    }
    Ok(GraphLayout {
        size,
        align,
        tag,
        slots,
    })
}

/// How one IR value is held while the export runs.
#[derive(Debug, Clone)]
enum Held {
    Flat {
        ty: WitType,
        locals: Vec<u32>,
    },
    Memory {
        ty: WitType,
        ptr: u32,
    },
    /// A call with no result.
    Nothing,
}

impl Held {
    fn ty(&self) -> Option<&WitType> {
        match self {
            Held::Flat { ty, .. } | Held::Memory { ty, .. } => Some(ty),
            Held::Nothing => None,
        }
    }
}

/// Locals beyond the parameters, allocated as they are needed.
struct Locals {
    first: u32,
    types: Vec<ValType>,
}

impl Locals {
    fn fresh(&mut self, t: ValType) -> u32 {
        self.types.push(t);
        self.first + self.types.len() as u32 - 1
    }
}

/// What every part of one module's encoding reads.
#[derive(Clone, Copy)]
struct Shared<'a> {
    resolve: &'a Resolve,
    sizes: &'a SizeAlign,
    /// Each IR type's WIT type (ADR-0039 §5).
    wit: &'a BTreeMap<Type, WitType>,
    literals: &'a Literals,
    imports: &'a BTreeMap<String, (u32, WasmSignature, WitFunction)>,
    realloc_index: u32,
    /// The functions compiled beside the export, by instance (ADR-0050).
    callees: &'a BTreeMap<(DefId, Vec<Type>), Callee>,
    /// The function values' code, by closure index (ADR-0052).
    closures: &'a BTreeMap<u32, ClosureCode>,
    /// How each function type is called (ADR-0052).
    fn_sigs: &'a BTreeMap<Type, FnSig>,
    /// Each box's cell (ADR-0202).
    cells: &'a Cells,
}

/// **How a function value of one type is called** (ADR-0052): the core type
/// `call_indirect` checks, and its parameters and result as a callee passes
/// them.
struct FnSig {
    type_index: u32,
    params: Vec<(WitType, Passed)>,
    result: Option<(WitType, Passed)>,
}

/// **A function value's code** (ADR-0052): its function index, the table
/// slot is its closure index; where each capture sits in the environment,
/// after the slot; the environment's size and alignment; and its type.
struct ClosureCode {
    index: u32,
    captures: Vec<(WitType, u64)>,
    size: u32,
    align: u32,
    signature: Type,
}

/// **How a function compiled beside the export passes one value**
/// (ADR-0050): a primitive, a string or a list as its flat values; anything
/// else, a record or a variant, as a pointer to its canonical layout in the
/// region, where one built in the body already lives. A choice, not a
/// necessity: until ADR-0059 this encoder could not read a variant's joined
/// flat slots back from memory, and passing one flat failed; `load_variant`
/// reads them, and either way computes the same (found 2026-10-07, when the
/// mutant passing them flat survived).
#[derive(Clone)]
enum Passed {
    Flat(Vec<ValType>),
    Pointer,
}

impl Passed {
    fn core(&self) -> Vec<ValType> {
        match self {
            Passed::Flat(v) => v.clone(),
            Passed::Pointer => vec![ValType::I32],
        }
    }
}

fn passed(resolve: &Resolve, t: &WitType) -> Passed {
    let compound = match dealias(resolve, *t) {
        WitType::Id(id) => !matches!(resolve.types[id].kind, TypeDefKind::List(_)),
        _ => false,
    };
    match flat(resolve, t) {
        Some(f) if !compound => Passed::Flat(core_types(&f)),
        _ => Passed::Pointer,
    }
}

/// **How the function being encoded is left** (ADR-0051), at its end and at
/// every `return`: an export's result as the world's signature takes it, a
/// callee's as [`Passed`] says.
#[derive(Clone)]
enum Exit {
    Export {
        result: Option<WitType>,
        retptr: bool,
    },
    Callee {
        result: Option<(WitType, Passed)>,
    },
}

/// A function compiled beside the export: its index, and how it passes its
/// parameters and its result.
struct Callee {
    index: u32,
    params: Vec<(WitType, Passed)>,
    result: Option<(WitType, Passed)>,
}

/// The instructions of the export's body. Encoded against a scratch list so
/// the locals it needs are known before the `Function` is created.
fn export_body(
    shared: Shared<'_>,
    function: &super::ir::Function,
    export_fn: &WitFunction,
    export_sig: &WasmSignature,
    helpers: &mut Helpers,
) -> Encoding<Function> {
    use wasm_encoder::Instruction as I;
    let Shared {
        resolve,
        sizes,
        imports,
        realloc_index,
        ..
    } = shared;

    let mut enc = Enc {
        resolve,
        sizes,
        imports,
        realloc_index,
        wit: shared.wit,
        cells: shared.cells,
        literals: shared.literals,
        helpers,
        callees: shared.callees,
        closures: shared.closures,
        fn_sigs: shared.fn_sigs,
        exit: Exit::Export {
            result: export_fn.result,
            retptr: export_sig.retptr,
        },
        export: &function.export,
        ops: Vec::new(),
        locals: Locals {
            first: export_sig.params.len() as u32,
            types: Vec::new(),
        },
        held: BTreeMap::new(),
        // A result that holds a type that contains itself is built in the
        // body's own types, and converted where it leaves (ADR-0194).
        expected: expected_types(
            resolve,
            shared.wit,
            function,
            export_fn.result.filter(|rt| !holds_index(resolve, rt)),
            imports,
            shared.callees,
        ),
    };

    // Parameters arrive flat, in the order the world lists them; or, where
    // they flatten past the Canonical ABI's limit (`MAX_FLAT_PARAMS`, 16), in
    // memory, a tuple of them that the host stores and passes the one pointer
    // to, each held where it sits (ADR-0266). Until ADR-0266 that export was
    // refused, and a row's record could not grow past it.
    let mut next_param = 0u32;
    let mut offset = 0u32;
    for ((value, own), param) in function.params.iter().zip(&export_fn.params) {
        let arrived = if export_sig.indirect_params {
            let (size, align) = enc.layout(&param.ty);
            offset = offset.next_multiple_of(align);
            let at = enc.locals.fresh(ValType::I32);
            enc.ops.extend([
                I::LocalGet(0),
                I::I32Const(offset as i32),
                I::I32Add,
                I::LocalSet(at),
            ]);
            offset += size;
            Held::Memory {
                ty: param.ty,
                ptr: at,
            }
        } else {
            let Some(flats) = flat(resolve, &param.ty) else {
                refuse!(
                    "a parameter that does not flatten",
                    "`{}` does not flatten within the core parameter limit",
                    param.name
                );
            };
            let ls: Vec<u32> = (next_param..next_param + flats.len() as u32).collect();
            next_param += flats.len() as u32;
            Held::Flat {
                ty: param.ty,
                locals: ls,
            }
        };
        // A type that contains itself arrives as its nodes, and is the
        // body's value from here (ADR-0194).
        let held = match shared.wit.get(own).copied() {
            Some(own) if holds_index(resolve, &param.ty) => match enc.convert(arrived, own) {
                Encoding::Encoded(h) => h,
                other => return other.map(|_| unreachable!()),
            },
            _ => arrived,
        };
        enc.held.insert(*value, held);
    }

    let Some(entry) = function.entry() else {
        blocked!("`{}` has no entry block", function.export);
    };
    if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
        enc.region(&entry.instrs)
    {
        return other.map(|_| unreachable!());
    }

    // The result: returned flat, or by pointer when it exceeds the flat limit.
    let Terminator::Return(v) = &entry.terminator else {
        refuse!(
            "a terminator other than `return`",
            "`{}` ends in {:?}",
            function.export,
            entry.terminator
        );
    };
    match enc.leave(*v) {
        Encoding::Encoded(()) => {}
        other => return other.map(|_| unreachable!()),
    }
    let _ = (sizes, realloc_index);
    enc.ops.push(I::End);

    Encoding::Encoded(enc.finish())
}

/// **A function value's code** (ADR-0052): its environment first, from which
/// each capture is read where it sits; then its parameters and its result as
/// a callee passes them.
fn closure_body(
    shared: Shared<'_>,
    n: u32,
    closure: &super::ir::Closure,
    helpers: &mut Helpers,
) -> Encoding<Function> {
    use wasm_encoder::Instruction as I;
    let function = &closure.function;
    let (Some(code), Some(sig)) = (
        shared.closures.get(&n),
        shared
            .closures
            .get(&n)
            .and_then(|c| shared.fn_sigs.get(&c.signature)),
    ) else {
        blocked!("a function value's code has no signature");
    };
    let first: usize = 1 + sig
        .params
        .iter()
        .map(|(_, p)| p.core().len())
        .sum::<usize>();
    let result_ty = sig.result.as_ref().map(|(t, _)| *t);
    let mut enc = Enc {
        resolve: shared.resolve,
        sizes: shared.sizes,
        imports: shared.imports,
        realloc_index: shared.realloc_index,
        wit: shared.wit,
        cells: shared.cells,
        literals: shared.literals,
        helpers,
        callees: shared.callees,
        closures: shared.closures,
        fn_sigs: shared.fn_sigs,
        exit: Exit::Callee {
            result: sig.result.clone(),
        },
        export: &function.export,
        ops: Vec::new(),
        locals: Locals {
            first: first as u32,
            types: Vec::new(),
        },
        held: BTreeMap::new(),
        expected: expected_types(
            shared.resolve,
            shared.wit,
            function,
            result_ty,
            shared.imports,
            shared.callees,
        ),
    };
    for ((value, _), (ty, offset)) in function.params.iter().zip(&code.captures) {
        let at = enc.address(0, *offset);
        enc.held.insert(*value, Held::Memory { ty: *ty, ptr: at });
    }
    let mut next = 1u32;
    for ((value, _), (ty, passing)) in function.params[closure.captures..].iter().zip(&sig.params) {
        let held = match passing {
            Passed::Flat(v) => {
                let locals: Vec<u32> = (next..next + v.len() as u32).collect();
                next += v.len() as u32;
                Held::Flat { ty: *ty, locals }
            }
            Passed::Pointer => {
                next += 1;
                Held::Memory {
                    ty: *ty,
                    ptr: next - 1,
                }
            }
        };
        enc.held.insert(*value, held);
    }
    let Some(entry) = function.entry() else {
        blocked!("a function value's code has no entry block");
    };
    if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
        enc.region(&entry.instrs)
    {
        return other.map(|_| unreachable!());
    }
    let Terminator::Return(v) = &entry.terminator else {
        refuse!(
            "a terminator other than `return`",
            "a function value's code ends in {:?}",
            entry.terminator
        );
    };
    match enc.leave(*v) {
        Encoding::Encoded(()) => {}
        other => return other.map(|_| unreachable!()),
    }
    enc.ops.push(I::End);
    Encoding::Encoded(enc.finish())
}

/// **A function compiled beside the export** (ADR-0050): its parameters as
/// its [`Callee`] entry passes them, its body, and its result the same way.
fn internal_body(
    shared: Shared<'_>,
    function: &super::ir::Function,
    helpers: &mut Helpers,
) -> Encoding<Function> {
    use wasm_encoder::Instruction as I;
    let Some(me) = shared
        .callees
        .get(&(function.def, function.instance.clone()))
    else {
        blocked!(
            "`{}` has no entry among the functions compiled",
            function.export
        );
    };
    let first: usize = me.params.iter().map(|(_, p)| p.core().len()).sum();
    let result_ty = me.result.as_ref().map(|(t, _)| *t);
    let mut enc = Enc {
        resolve: shared.resolve,
        sizes: shared.sizes,
        imports: shared.imports,
        realloc_index: shared.realloc_index,
        wit: shared.wit,
        cells: shared.cells,
        literals: shared.literals,
        helpers,
        callees: shared.callees,
        closures: shared.closures,
        fn_sigs: shared.fn_sigs,
        exit: Exit::Callee {
            result: me.result.clone(),
        },
        export: &function.export,
        ops: Vec::new(),
        locals: Locals {
            first: first as u32,
            types: Vec::new(),
        },
        held: BTreeMap::new(),
        expected: expected_types(
            shared.resolve,
            shared.wit,
            function,
            result_ty,
            shared.imports,
            shared.callees,
        ),
    };
    let mut next = 0u32;
    for ((value, _), (ty, passing)) in function.params.iter().zip(&me.params) {
        let held = match passing {
            Passed::Flat(v) => {
                let locals: Vec<u32> = (next..next + v.len() as u32).collect();
                next += v.len() as u32;
                Held::Flat { ty: *ty, locals }
            }
            Passed::Pointer => {
                next += 1;
                Held::Memory {
                    ty: *ty,
                    ptr: next - 1,
                }
            }
        };
        enc.held.insert(*value, held);
    }
    let Some(entry) = function.entry() else {
        blocked!("`{}` has no entry block", function.export);
    };
    if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
        enc.region(&entry.instrs)
    {
        return other.map(|_| unreachable!());
    }
    let Terminator::Return(v) = &entry.terminator else {
        refuse!(
            "a terminator other than `return`",
            "`{}` ends in {:?}",
            function.export,
            entry.terminator
        );
    };
    match enc.leave(*v) {
        Encoding::Encoded(()) => {}
        other => return other.map(|_| unreachable!()),
    }
    enc.ops.push(I::End);
    Encoding::Encoded(enc.finish())
}

/// **The component type each value must have where it is used.**
///
/// A constructed variant cannot say what it is: `None` is `None` of whatever
/// its use needs. The world fixes the types of the export's result and of
/// every import's parameters, and a match's arms produce its result, so the
/// type flows backwards from those uses. Only a value with no instruction of
/// its own to type it (a variant, and through it its payload) reads this; every
/// other value is typed where it is made and checked with `same_type` where it
/// is used.
fn expected_types(
    resolve: &Resolve,
    wit: &BTreeMap<Type, WitType>,
    function: &super::ir::Function,
    result: Option<WitType>,
    imports: &BTreeMap<String, (u32, WasmSignature, WitFunction)>,
    callees: &BTreeMap<(DefId, Vec<Type>), Callee>,
) -> BTreeMap<ValueId, WitType> {
    let mut out = BTreeMap::new();
    let Some(entry) = function.entry() else {
        return out;
    };
    if let (Some(rt), Terminator::Return(v)) = (result, &entry.terminator) {
        out.insert(*v, rt);
    }
    let cx = Expecting {
        resolve,
        wit,
        imports,
        callees,
        result,
    };
    cx.region(&entry.instrs, &mut out);
    out
}

/// What fixes a value's component type from its use: the world's imports,
/// and the functions compiled beside the export (ADR-0050).
struct Expecting<'a> {
    resolve: &'a Resolve,
    wit: &'a BTreeMap<Type, WitType>,
    imports: &'a BTreeMap<String, (u32, WasmSignature, WitFunction)>,
    callees: &'a BTreeMap<(DefId, Vec<Type>), Callee>,
    /// The function's own result, which every `return` gives.
    result: Option<WitType>,
}

impl Expecting<'_> {
    fn region(&self, instrs: &[Instr], out: &mut BTreeMap<ValueId, WitType>) {
        expect_region(self, instrs, out);
    }
}

fn expect_region(cx: &Expecting<'_>, instrs: &[Instr], out: &mut BTreeMap<ValueId, WitType>) {
    let (resolve, wit, imports) = (cx.resolve, cx.wit, cx.imports);
    // Backwards: a use comes after the value it uses.
    for instr in instrs.iter().rev() {
        match instr {
            Instr::ImportCall { import, args, .. } => {
                if let Some((_, _, func)) = imports.get(&import.qualified()) {
                    for (a, p) in args.iter().zip(&func.params) {
                        // Converted where it is passed (ADR-0194).
                        if !holds_index(resolve, &p.ty) {
                            out.entry(*a).or_insert(p.ty);
                        }
                    }
                }
            }
            Instr::Call {
                callee,
                instance,
                args,
                ..
            } => {
                if let Some(c) = cx.callees.get(&(*callee, instance.clone())) {
                    for (a, (t, _)) in args.iter().zip(&c.params) {
                        out.entry(*a).or_insert(*t);
                    }
                }
            }
            Instr::Return { value, .. } => {
                if let Some(t) = cx.result {
                    out.entry(*value).or_insert(t);
                }
            }
            Instr::Local { result, init, ty } => {
                if let Some(t) = out.get(result).copied().or_else(|| wit.get(ty).copied()) {
                    out.entry(*init).or_insert(t);
                }
            }
            Instr::Each { body, .. } => cx.region(&body.instrs, out),
            Instr::Match { result, arms, .. } => {
                if let Some(t) = out.get(result).copied() {
                    for arm in arms {
                        out.entry(arm.body.value).or_insert(t);
                    }
                }
                for arm in arms {
                    cx.region(&arm.body.instrs, out);
                }
            }
            Instr::If {
                result, then, els, ..
            } => {
                if let Some(t) = out.get(result).copied() {
                    out.entry(then.value).or_insert(t);
                    out.entry(els.value).or_insert(t);
                }
                cx.region(&then.instrs, out);
                cx.region(&els.instrs, out);
            }
            // A record's field fixes the type of what is stored in it: the
            // `None` in `Entry { redirect: None, .. }`.
            Instr::Construct { args, ty, .. } => {
                if let Some(WitType::Id(id)) = wit.get(ty).map(|t| dealias(resolve, *t))
                    && let TypeDefKind::Record(r) = &resolve.types[id].kind
                {
                    for (a, f) in args.iter().zip(&r.fields) {
                        out.entry(*a).or_insert(f.ty);
                    }
                }
            }
            Instr::Variant {
                result,
                case,
                payload: Some(p),
                ..
            } => {
                if let Some(pt) = out
                    .get(result)
                    .and_then(|t| payload_type(resolve, *t, *case))
                {
                    out.entry(*p).or_insert(pt);
                }
            }
            // A case's fields take its payload's types (ADR-0059).
            Instr::Case {
                result,
                case,
                fields,
                ty,
            } => {
                let t = out.get(result).copied().or_else(|| wit.get(ty).copied());
                let payload = t.and_then(|t| {
                    let (cases, _) = variant_cases(resolve, t)?;
                    cases.get(*case as usize).copied()
                });
                if let Some(types) =
                    payload.and_then(|p| case_field_types(resolve, p, fields.len()))
                {
                    for (f, ft) in fields.iter().zip(types) {
                        out.entry(*f).or_insert(ft);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The payload type of `case` in the variant type `t`, if it has one.
fn payload_type(resolve: &Resolve, t: WitType, case: BuiltinCase) -> Option<WitType> {
    let WitType::Id(id) = dealias(resolve, t) else {
        return None;
    };
    match (&resolve.types[id].kind, case) {
        (TypeDefKind::Option(p), BuiltinCase::Some) => Some(*p),
        (TypeDefKind::Result(r), BuiltinCase::Ok) => r.ok,
        (TypeDefKind::Result(r), BuiltinCase::Err) => r.err,
        _ => None,
    }
}

/// **A variant type's cases**, each its payload's type, and its
/// discriminant's width: an `option` (`none`, `some`), a `result` (`ok`,
/// `err`), or a declared `variant` (ADR-0059), as the Canonical ABI numbers
/// and lays out each. `None` when `t` is not a variant.
fn variant_cases(resolve: &Resolve, t: WitType) -> Option<(Vec<Option<WitType>>, Int)> {
    let WitType::Id(id) = dealias(resolve, t) else {
        return None;
    };
    match &resolve.types[id].kind {
        TypeDefKind::Option(p) => Some((vec![None, Some(*p)], Int::U8)),
        TypeDefKind::Result(r) => Some((vec![r.ok, r.err], Int::U8)),
        TypeDefKind::Variant(v) => Some((v.cases.iter().map(|c| c.ty).collect(), v.tag())),
        _ => None,
    }
}

/// A variant type's discriminant for `case`, its payload type, and where the
/// payload sits: every number from `SizeAlign`. `None` when `t` is not a
/// variant that has this case.
fn case_layout(
    resolve: &Resolve,
    sizes: &SizeAlign,
    t: WitType,
    case: VariantCase,
) -> Option<(u32, Option<WitType>, u64)> {
    let WitType::Id(id) = dealias(resolve, t) else {
        return None;
    };
    let disc: u32 = match (&resolve.types[id].kind, case) {
        (TypeDefKind::Option(_), VariantCase::Builtin(BuiltinCase::None)) => 0,
        (TypeDefKind::Option(_), VariantCase::Builtin(BuiltinCase::Some)) => 1,
        (TypeDefKind::Result(_), VariantCase::Builtin(BuiltinCase::Ok)) => 0,
        (TypeDefKind::Result(_), VariantCase::Builtin(BuiltinCase::Err)) => 1,
        (TypeDefKind::Variant(v), VariantCase::Declared(i)) if (i as usize) < v.cases.len() => i,
        _ => return None,
    };
    let (cases, tag) = variant_cases(resolve, t)?;
    let offset = sizes
        .payload_offset(tag, cases.iter().map(Option::as_ref))
        .size_wasm32() as u64;
    Some((disc, cases[disc as usize], offset))
}

/// The types of a case's `count` fields, from its payload (ADR-0059): none
/// without one, the payload itself for one field, a tuple's types for
/// several, as `wit.rs` writes a case of several fields.
fn case_field_types(
    resolve: &Resolve,
    payload: Option<WitType>,
    count: usize,
) -> Option<Vec<WitType>> {
    match (payload, count) {
        (None, _) => Some(Vec::new()),
        (Some(p), 1) => Some(vec![p]),
        (Some(p), n) => {
            let WitType::Id(id) = dealias(resolve, p) else {
                return None;
            };
            match &resolve.types[id].kind {
                TypeDefKind::Tuple(t) if t.types.len() == n => Some(t.types.clone()),
                _ => None,
            }
        }
    }
}

/// Where each of a case's `count` fields sits within its payload, and its
/// type: [`case_field_types`], at the offsets `SizeAlign` gives a tuple.
fn case_fields(
    resolve: &Resolve,
    sizes: &SizeAlign,
    payload: Option<WitType>,
    count: usize,
) -> Option<Vec<(u64, WitType)>> {
    let types = case_field_types(resolve, payload, count)?;
    if types.len() < 2 {
        return Some(types.into_iter().map(|t| (0, t)).collect());
    }
    Some(
        sizes
            .field_offsets(types.iter())
            .into_iter()
            .map(|(o, t)| (o.size_wasm32() as u64, *t))
            .collect(),
    )
}

/// Store the discriminant on the stack, of `tag`'s width, at `offset` past
/// the address below it.
fn store_tag(tag: Int, offset: u64) -> wasm_encoder::Instruction<'static> {
    use wasm_encoder::Instruction as I;
    let at = |align| MemArg {
        offset,
        align,
        memory_index: 0,
    };
    match tag {
        Int::U8 => I::I32Store8(at(0)),
        Int::U16 => I::I32Store16(at(1)),
        Int::U32 | Int::U64 => I::I32Store(at(2)),
    }
}

/// Load a discriminant of `tag`'s width from `offset` past the address on
/// the stack.
fn load_tag(tag: Int, offset: u64) -> wasm_encoder::Instruction<'static> {
    use wasm_encoder::Instruction as I;
    let at = |align| MemArg {
        offset,
        align,
        memory_index: 0,
    };
    match tag {
        Int::U8 => I::I32Load8U(at(0)),
        Int::U16 => I::I32Load16U(at(1)),
        Int::U32 | Int::U64 => I::I32Load(at(2)),
    }
}

/// A case's flat value `have`, widened into the variant's joined slot
/// `want`: the Canonical ABI's `lower_flat_variant`.
fn to_joined(have: ValType, want: ValType) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    match (have, want) {
        (ValType::F32, ValType::I32) => vec![I::I32ReinterpretF32],
        (ValType::I32, ValType::I64) => vec![I::I64ExtendI32U],
        (ValType::F32, ValType::I64) => vec![I::I32ReinterpretF32, I::I64ExtendI32U],
        (ValType::F64, ValType::I64) => vec![I::I64ReinterpretF64],
        _ => Vec::new(),
    }
}

/// A joined slot `have`, read back as the case's flat value `want`: the
/// Canonical ABI's `lift_flat_variant`.
fn from_joined(have: ValType, want: ValType) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    match (have, want) {
        (ValType::I32, ValType::F32) => vec![I::F32ReinterpretI32],
        (ValType::I64, ValType::I32) => vec![I::I32WrapI64],
        (ValType::I64, ValType::F32) => vec![I::I32WrapI64, I::F32ReinterpretI32],
        (ValType::I64, ValType::F64) => vec![I::F64ReinterpretI64],
        _ => Vec::new(),
    }
}

/// The zero of a core type, for a joined slot the named case leaves empty.
fn zero(t: ValType) -> wasm_encoder::Instruction<'static> {
    use wasm_encoder::Instruction as I;
    match t {
        ValType::I64 => I::I64Const(0),
        ValType::F32 => I::F32Const(0.0f32.into()),
        ValType::F64 => I::F64Const(0.0f64.into()),
        _ => I::I32Const(0),
    }
}

/// The encoder's state while one export's body is emitted.
struct Enc<'a> {
    resolve: &'a Resolve,
    sizes: &'a SizeAlign,
    imports: &'a BTreeMap<String, (u32, WasmSignature, WitFunction)>,
    realloc_index: u32,
    wit: &'a BTreeMap<Type, WitType>,
    literals: &'a Literals,
    helpers: &'a mut Helpers,
    callees: &'a BTreeMap<(DefId, Vec<Type>), Callee>,
    closures: &'a BTreeMap<u32, ClosureCode>,
    fn_sigs: &'a BTreeMap<Type, FnSig>,
    cells: &'a Cells,
    exit: Exit,
    export: &'a str,
    ops: Vec<wasm_encoder::Instruction<'static>>,
    locals: Locals,
    held: BTreeMap<ValueId, Held>,
    expected: BTreeMap<ValueId, WitType>,
}

impl Enc<'_> {
    /// The encoded function: its locals, grouped, and its instructions.
    fn finish(self) -> Function {
        let mut groups: Vec<(u32, ValType)> = Vec::new();
        for t in &self.locals.types {
            match groups.last_mut() {
                Some((n, seen)) if seen == t => *n += 1,
                _ => groups.push((1, *t)),
            }
        }
        let mut f = Function::new(groups);
        for op in &self.ops {
            f.instruction(op);
        }
        f
    }

    /// **Put the function's result where its caller takes it** (ADR-0051):
    /// at its end, and before every `return`.
    fn leave(&mut self, v: ValueId) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let exit = self.exit.clone();
        let result = match &exit {
            Exit::Export { result, .. } => *result,
            Exit::Callee { result } => result.as_ref().map(|(t, _)| *t),
        };
        let Some(rt) = result else {
            return Encoding::Encoded(());
        };
        let Some(h) = self.held.get(&v).cloned() else {
            blocked!("`{}` returns {v:?} and nothing defines it", self.export);
        };
        let Some(t) = h.ty() else {
            blocked!("`{}` returns a call with no result", self.export);
        };
        let h = if same_type(resolve, t, &rt) {
            h
        } else if holds_index(resolve, &rt) {
            // A type that contains itself leaves as its nodes (ADR-0194).
            match self.convert(h, rt) {
                Encoding::Encoded(h) => h,
                other => return other.map(|_| ()),
            }
        } else {
            blocked!(
                "`{}` returns a value of another component type than it declares",
                self.export
            );
        };
        match exit {
            Exit::Callee {
                result: Some((ty, passing)),
            } => self.pass(&h, &ty, &passing),
            Exit::Callee { result: None } => Encoding::Encoded(()),
            Exit::Export { retptr, .. } => {
                match (retptr, &h) {
                    // The value already has its canonical layout at an
                    // address: that address IS the result.
                    (true, Held::Memory { ptr, .. }) => self.ops.push(I::LocalGet(*ptr)),
                    (true, Held::Nothing) => {
                        blocked!("`{}` returns a call with no result", self.export)
                    }
                    // A flat value returned by pointer is stored into the
                    // region first, the way a constructed variant is.
                    (true, Held::Flat { ty, locals: flats }) => {
                        let area = self.locals.fresh(ValType::I32);
                        allocate(sizes, ty, self.realloc_index, area, &mut self.ops);
                        match store(
                            resolve,
                            sizes,
                            &mut self.locals,
                            ty,
                            area,
                            0,
                            flats,
                            &mut self.ops,
                        ) {
                            Encoding::Encoded(_) => {}
                            other => return other.map(|_| unreachable!()),
                        }
                        self.ops.push(I::LocalGet(area));
                    }
                    (false, h) => {
                        match push_flat_values(resolve, sizes, &mut self.locals, h, &mut self.ops) {
                            Encoding::Encoded(()) => {}
                            other => return other,
                        }
                    }
                }
                Encoding::Encoded(())
            }
        }
    }

    /// **A value, in another component type's layout** (ADR-0194): the
    /// world's, where a type that contains itself crosses as its nodes, or
    /// the body's own, where such a value arrives. Every part that is one
    /// type in both is copied as it is.
    fn convert(&mut self, h: Held, to: WitType) -> Encoding<Held> {
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(from) = h.ty().copied() else {
            blocked!("`{}` converts a call with no result", self.export);
        };
        if same_type(resolve, &from, &to) {
            return Encoding::Encoded(h);
        }
        let src = match h {
            Held::Memory { ptr, .. } => ptr,
            Held::Flat { ty, locals } => {
                let area = self.locals.fresh(ValType::I32);
                allocate(sizes, &ty, self.realloc_index, area, &mut self.ops);
                match store(
                    resolve,
                    sizes,
                    &mut self.locals,
                    &ty,
                    area,
                    0,
                    &locals,
                    &mut self.ops,
                ) {
                    Encoding::Encoded(_) => {}
                    other => return other.map(|_| unreachable!()),
                }
                area
            }
            Held::Nothing => blocked!("`{}` converts a call with no result", self.export),
        };
        let dst = self.locals.fresh(ValType::I32);
        allocate(sizes, &to, self.realloc_index, dst, &mut self.ops);
        match self.convert_at(src, 0, from, dst, 0, to) {
            Encoding::Encoded(()) => {}
            other => return other.map(|_| unreachable!()),
        }
        Encoding::Encoded(Held::Memory { ty: to, ptr: dst })
    }

    /// Write the value of type `from` at `src + so` as a value of type `to`
    /// at `dst + d`. Each `to` is the same type as `from`, or the two differ
    /// only where one holds a type that contains itself and the other its
    /// nodes.
    fn convert_at(
        &mut self,
        src: u32,
        so: u64,
        from: WitType,
        dst: u32,
        d: u64,
        to: WitType,
    ) -> Encoding<()> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let word = |offset: u64| MemArg {
            offset,
            align: 2,
            memory_index: 0,
        };
        if same_type(resolve, &from, &to) {
            let size = sizes.size(&from).size_wasm32() as i32;
            let (s, t) = (self.address(src, so), self.address(dst, d));
            self.ops.extend([
                I::LocalGet(t),
                I::LocalGet(s),
                I::I32Const(size),
                I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                },
            ]);
            return Encoding::Encoded(());
        }
        // A box, as its nodes, and its nodes as a box (ADR-0202): the box is
        // its cell's address, one word read or written.
        if let Some(WitType::Id(cell)) = self.cell_of(from)
            && let Some(node) = node_of(resolve, &to)
        {
            let layout = match graph_layout(resolve, sizes, self.cells, cell, node) {
                Ok(l) => l,
                Err(why) => refuse!(
                    "a type that contains itself whose nodes this backend does not lay out",
                    "`{}`: {why}",
                    self.export
                ),
            };
            let n = self.helpers.graph((cell, node), layout);
            let call = self.helpers.index(Helper::Encode(n));
            let (ptr, len) = (
                self.locals.fresh(ValType::I32),
                self.locals.fresh(ValType::I32),
            );
            self.ops.extend([
                I::LocalGet(src),
                I::I32Load(word(so)),
                I::Call(call),
                I::LocalSet(len),
                I::LocalSet(ptr),
                I::LocalGet(dst),
                I::LocalGet(ptr),
                I::I32Store(word(d)),
                I::LocalGet(dst),
                I::LocalGet(len),
                I::I32Store(word(d + 4)),
            ]);
            return Encoding::Encoded(());
        }
        if let Some(WitType::Id(cell)) = self.cell_of(to)
            && let Some(node) = node_of(resolve, &from)
        {
            let layout = match graph_layout(resolve, sizes, self.cells, cell, node) {
                Ok(l) => l,
                Err(why) => refuse!(
                    "a type that contains itself whose nodes this backend does not lay out",
                    "`{}`: {why}",
                    self.export
                ),
            };
            let n = self.helpers.graph((cell, node), layout);
            let call = self.helpers.index(Helper::Decode(n));
            self.ops.extend([
                I::LocalGet(dst),
                I::LocalGet(src),
                I::I32Load(word(so)),
                I::LocalGet(src),
                I::I32Load(word(so + 4)),
                I::Call(call),
                I::I32Store(word(d)),
            ]);
            return Encoding::Encoded(());
        }
        let (f, t) = (dealias(resolve, from), dealias(resolve, to));
        let (WitType::Id(fid), WitType::Id(tid)) = (f, t) else {
            blocked!(
                "`{}` converts between two component types that are not one",
                self.export
            );
        };
        let kinds = (&resolve.types[fid].kind, &resolve.types[tid].kind);
        // The body's value of a type that contains itself, as its nodes.
        if matches!(kinds.0, TypeDefKind::Record(_) | TypeDefKind::Variant(_))
            && let Some(node) = node_of(resolve, &to)
        {
            let layout = match graph_layout(resolve, sizes, self.cells, fid, node) {
                Ok(l) => l,
                Err(why) => refuse!(
                    "a type that contains itself whose nodes this backend does not lay out",
                    "`{}`: {why}",
                    self.export
                ),
            };
            let n = self.helpers.graph((fid, node), layout);
            let call = self.helpers.index(Helper::Encode(n));
            let at = self.address(src, so);
            let (ptr, len) = (
                self.locals.fresh(ValType::I32),
                self.locals.fresh(ValType::I32),
            );
            self.ops.extend([
                I::LocalGet(at),
                I::Call(call),
                I::LocalSet(len),
                I::LocalSet(ptr),
                I::LocalGet(dst),
                I::LocalGet(ptr),
                I::I32Store(word(d)),
                I::LocalGet(dst),
                I::LocalGet(len),
                I::I32Store(word(d + 4)),
            ]);
            return Encoding::Encoded(());
        }
        // Nodes, as the body's value: checked, and rebuilt where they lie.
        if matches!(kinds.1, TypeDefKind::Record(_) | TypeDefKind::Variant(_))
            && let Some(node) = node_of(resolve, &from)
        {
            let layout = match graph_layout(resolve, sizes, self.cells, tid, node) {
                Ok(l) => l,
                Err(why) => refuse!(
                    "a type that contains itself whose nodes this backend does not lay out",
                    "`{}`: {why}",
                    self.export
                ),
            };
            let size = layout.size as i32;
            let n = self.helpers.graph((tid, node), layout);
            let call = self.helpers.index(Helper::Decode(n));
            let root = self.locals.fresh(ValType::I32);
            let at = self.address(dst, d);
            self.ops.extend([
                I::LocalGet(src),
                I::I32Load(word(so)),
                I::LocalGet(src),
                I::I32Load(word(so + 4)),
                I::Call(call),
                I::LocalSet(root),
                I::LocalGet(at),
                I::LocalGet(root),
                I::I32Const(size),
                I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                },
            ]);
            return Encoding::Encoded(());
        }
        match kinds {
            (TypeDefKind::List(fe), TypeDefKind::List(te)) => {
                let (fe, te) = (*fe, *te);
                let (fs, ts) = (
                    sizes.size(&fe).size_wasm32() as u32,
                    sizes.size(&te).size_wasm32() as u32,
                );
                let ta = sizes.align(&te).align_wasm32() as u32;
                let (ptr, len, i) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                self.ops.extend([
                    I::LocalGet(src),
                    I::I32Load(word(so)),
                    I::LocalSet(ptr),
                    I::LocalGet(src),
                    I::I32Load(word(so + 4)),
                    I::LocalSet(len),
                ]);
                let out = self.alloc_array(len, ts, ta);
                self.ops.extend([
                    I::I32Const(0),
                    I::LocalSet(i),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(i),
                    I::LocalGet(len),
                    I::I32GeU,
                    I::BrIf(1),
                ]);
                let from_at = self.element_address(ptr, i, fs);
                let to_at = self.element_address(out, i, ts);
                match self.convert_at(from_at, 0, fe, to_at, 0, te) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.extend(increment(i));
                self.ops.extend([
                    I::Br(0),
                    I::End,
                    I::End,
                    I::LocalGet(dst),
                    I::LocalGet(out),
                    I::I32Store(word(d)),
                    I::LocalGet(dst),
                    I::LocalGet(len),
                    I::I32Store(word(d + 4)),
                ]);
                Encoding::Encoded(())
            }
            (TypeDefKind::Record(a), TypeDefKind::Record(b))
                if a.fields.len() == b.fields.len() =>
            {
                let fa: Vec<WitType> = a.fields.iter().map(|f| f.ty).collect();
                let fb: Vec<WitType> = b.fields.iter().map(|f| f.ty).collect();
                self.convert_fields(src, so, &fa, dst, d, &fb)
            }
            (TypeDefKind::Tuple(a), TypeDefKind::Tuple(b)) if a.types.len() == b.types.len() => {
                let (fa, fb) = (a.types.clone(), b.types.clone());
                self.convert_fields(src, so, &fa, dst, d, &fb)
            }
            (
                TypeDefKind::Option(_) | TypeDefKind::Result(_) | TypeDefKind::Variant(_),
                TypeDefKind::Option(_) | TypeDefKind::Result(_) | TypeDefKind::Variant(_),
            ) => {
                let (Some((ca, ta)), Some((cb, tb))) =
                    (variant_cases(resolve, from), variant_cases(resolve, to))
                else {
                    blocked!("`{}` converts a variant with no cases", self.export);
                };
                if ca.len() != cb.len() || ta != tb {
                    blocked!(
                        "`{}` converts between two variants with different cases",
                        self.export
                    );
                }
                let pa = sizes
                    .payload_offset(ta, ca.iter().map(Option::as_ref))
                    .size_wasm32() as u64;
                let pb = sizes
                    .payload_offset(tb, cb.iter().map(Option::as_ref))
                    .size_wasm32() as u64;
                let tag = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::LocalGet(src),
                    load_tag(ta, so),
                    I::LocalSet(tag),
                    I::LocalGet(dst),
                    I::LocalGet(tag),
                    store_tag(tb, d),
                ]);
                for (c, (x, y)) in ca.iter().zip(&cb).enumerate() {
                    match (x, y) {
                        (None, None) => {}
                        (Some(x), Some(y)) => {
                            self.ops.extend([
                                I::LocalGet(tag),
                                I::I32Const(c as i32),
                                I::I32Eq,
                                I::If(Empty),
                            ]);
                            match self.convert_at(src, so + pa, *x, dst, d + pb, *y) {
                                Encoding::Encoded(()) => {}
                                other => return other,
                            }
                            self.ops.push(I::End);
                        }
                        _ => blocked!(
                            "`{}` converts between two variants with different payloads",
                            self.export
                        ),
                    }
                }
                Encoding::Encoded(())
            }
            _ => blocked!(
                "`{}` converts between two component types that are not one",
                self.export
            ),
        }
    }

    /// A record's or a tuple's fields, each converted where it sits.
    fn convert_fields(
        &mut self,
        src: u32,
        so: u64,
        from: &[WitType],
        dst: u32,
        d: u64,
        to: &[WitType],
    ) -> Encoding<()> {
        let sizes = self.sizes;
        let oa: Vec<u64> = sizes
            .field_offsets(from.iter())
            .into_iter()
            .map(|(o, _)| o.size_wasm32() as u64)
            .collect();
        let ob: Vec<u64> = sizes
            .field_offsets(to.iter())
            .into_iter()
            .map(|(o, _)| o.size_wasm32() as u64)
            .collect();
        for (i, (x, y)) in from.iter().zip(to).enumerate() {
            match self.convert_at(src, so + oa[i], *x, dst, d + ob[i], *y) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
        }
        Encoding::Encoded(())
    }

    /// Push a value the way a function compiled beside the export takes it
    /// (ADR-0050): its flat values, or a pointer to its layout, stored first
    /// when the value is held flat.
    /// A call's arguments, each passed the way its parameter takes it.
    fn pass_args(&mut self, args: &[ValueId], params: &[(WitType, Passed)]) -> Encoding<()> {
        for (a, (ty, passing)) in args.iter().zip(params) {
            let Some(h) = self.held.get(a).cloned() else {
                blocked!("`{}` passes {a:?} and nothing defines it", self.export);
            };
            match self.pass(&h, ty, passing) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
        }
        Encoding::Encoded(())
    }

    /// A call's result, taken off the stack the way the callee passes it.
    fn returned(&mut self, ret: Option<(WitType, Passed)>) -> Held {
        use wasm_encoder::Instruction as I;
        match ret {
            None => Held::Nothing,
            Some((ty, Passed::Flat(vs))) => {
                let ls: Vec<u32> = vs.iter().map(|v| self.locals.fresh(*v)).collect();
                for l in ls.iter().rev() {
                    self.ops.push(I::LocalSet(*l));
                }
                Held::Flat { ty, locals: ls }
            }
            Some((ty, Passed::Pointer)) => {
                let p = self.locals.fresh(ValType::I32);
                self.ops.push(I::LocalSet(p));
                Held::Memory { ty, ptr: p }
            }
        }
    }

    fn pass(&mut self, h: &Held, ty: &WitType, passing: &Passed) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        match (passing, h) {
            (Passed::Flat(_), h) => {
                push_flat_values(resolve, sizes, &mut self.locals, h, &mut self.ops)
            }
            (Passed::Pointer, Held::Memory { ptr, .. }) => {
                self.ops.push(I::LocalGet(*ptr));
                Encoding::Encoded(())
            }
            (Passed::Pointer, Held::Flat { locals, .. }) => {
                let area = self.locals.fresh(ValType::I32);
                allocate(sizes, ty, self.realloc_index, area, &mut self.ops);
                match store(
                    resolve,
                    sizes,
                    &mut self.locals,
                    ty,
                    area,
                    0,
                    locals,
                    &mut self.ops,
                ) {
                    Encoding::Encoded(_) => {}
                    other => return other.map(|_| unreachable!()),
                }
                self.ops.push(I::LocalGet(area));
                Encoding::Encoded(())
            }
            (Passed::Pointer, Held::Nothing) => {
                blocked!("`{}` passes a call with no result", self.export)
            }
        }
    }

    /// Emit a region's instructions in order.
    fn region(&mut self, instrs: &[Instr]) -> Encoding<()> {
        for instr in instrs {
            match self.instr(instr) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
        }
        Encoding::Encoded(())
    }

    /// A fresh local holding `base + offset`.
    fn address(&mut self, base: u32, offset: u64) -> u32 {
        use wasm_encoder::Instruction as I;
        let l = self.locals.fresh(ValType::I32);
        self.ops.push(I::LocalGet(base));
        if offset != 0 {
            self.ops.push(I::I32Const(offset as i32));
            self.ops.push(I::I32Add);
        }
        self.ops.push(I::LocalSet(l));
        l
    }

    fn instr(&mut self, instr: &Instr) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        match instr {
            Instr::ImportCall {
                result,
                import,
                args,
                ty,
            } => {
                let Some((index, sig, func)) = self.imports.get(&import.qualified()) else {
                    blocked!("`{}` has no core import", import.qualified());
                };
                if args.len() != func.params.len() {
                    blocked!(
                        "`{}` passes {} arguments to `{}`, whose world function takes {}",
                        self.export,
                        args.len(),
                        import.qualified(),
                        func.params.len()
                    );
                }
                for (a, param) in args.iter().zip(&func.params) {
                    let Some(h) = self.held.get(a).cloned() else {
                        blocked!(
                            "`{}` passes {a:?} to `{}` and nothing defines it",
                            self.export,
                            import.qualified()
                        );
                    };
                    let Some(t) = h.ty() else {
                        blocked!("`{}` passes a call with no result", self.export);
                    };
                    // **The component-level check.** Two positions that
                    // flatten alike are not therefore one type.
                    let h = if same_type(resolve, t, &param.ty) {
                        h
                    } else if holds_index(resolve, &param.ty) {
                        // A type that contains itself is passed as its
                        // nodes (ADR-0194).
                        match self.convert(h, param.ty) {
                            Encoding::Encoded(h) => h,
                            other => return other.map(|_| ()),
                        }
                    } else {
                        blocked!(
                            "`{}` passes a value of another component type as `{}` of `{}`",
                            self.export,
                            param.name,
                            import.qualified()
                        );
                    };
                    match push_flat_values(resolve, sizes, &mut self.locals, &h, &mut self.ops) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                }
                let out = match (&func.result, sig.retptr) {
                    (Some(rt), true) => {
                        let area = self.locals.fresh(ValType::I32);
                        allocate(sizes, rt, self.realloc_index, area, &mut self.ops);
                        self.ops.push(I::LocalGet(area));
                        self.ops.push(I::Call(*index));
                        Held::Memory { ty: *rt, ptr: area }
                    }
                    (Some(rt), false) => {
                        self.ops.push(I::Call(*index));
                        let ls: Vec<u32> = sig
                            .results
                            .iter()
                            .map(|t| self.locals.fresh(core_type(*t)))
                            .collect();
                        for l in ls.iter().rev() {
                            self.ops.push(I::LocalSet(*l));
                        }
                        Held::Flat {
                            ty: *rt,
                            locals: ls,
                        }
                    }
                    (None, _) => {
                        self.ops.push(I::Call(*index));
                        Held::Nothing
                    }
                };
                // An answer that holds a type that contains itself arrives
                // as its nodes, and is the body's value from here (ADR-0194).
                let out = match (&func.result, self.wit.get(ty).copied()) {
                    (Some(rt), Some(own)) if holds_index(resolve, rt) => {
                        match self.convert(out, own) {
                            Encoding::Encoded(h) => h,
                            other => return other.map(|_| ()),
                        }
                    }
                    _ => out,
                };
                self.held.insert(*result, out);
            }
            Instr::Const {
                result,
                value: Const::Str(text),
                ty: Type::Str,
            } => {
                let Some(at) = self.literals.at.get(text).copied() else {
                    blocked!(
                        "`{}` names a string with no place in the data segment",
                        self.export
                    );
                };
                let (ptr, len) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                self.ops.push(I::I32Const(at as i32));
                self.ops.push(I::LocalSet(ptr));
                self.ops.push(I::I32Const(text.len() as i32));
                self.ops.push(I::LocalSet(len));
                self.held.insert(
                    *result,
                    Held::Flat {
                        ty: WitType::String,
                        locals: vec![ptr, len],
                    },
                );
            }
            // The unit value: a statement's, which nothing holds.
            Instr::Const {
                result,
                value: Const::Unit,
                ..
            } => {
                self.held.insert(*result, Held::Nothing);
            }
            Instr::Const { result, value, ty } => {
                let (wit, op, vt) = match (value, ty) {
                    (Const::Int(n), Type::Int) => (WitType::S64, I::I64Const(*n), ValType::I64),
                    (Const::Bool(b), Type::Bool) => {
                        (WitType::Bool, I::I32Const(i32::from(*b)), ValType::I32)
                    }
                    (Const::Float(x), Type::Float) => {
                        (WitType::F64, I::F64Const((*x).into()), ValType::F64)
                    }
                    (other, _) => refuse!(
                        "a constant that needs the invocation region",
                        "`{}` holds {other:?}; writing it into memory needs a data \
                         segment this encoder does not emit",
                        self.export
                    ),
                };
                let l = self.locals.fresh(vt);
                self.ops.push(op);
                self.ops.push(I::LocalSet(l));
                self.held.insert(
                    *result,
                    Held::Flat {
                        ty: wit,
                        locals: vec![l],
                    },
                );
            }
            // A call to a function compiled beside the export (ADR-0050).
            Instr::Call {
                result,
                callee,
                instance,
                args,
                ..
            } => {
                let Some(c) = self.callees.get(&(*callee, instance.clone())) else {
                    blocked!(
                        "`{}` calls an instance of {callee:?} that nothing compiled",
                        self.export
                    );
                };
                let (index, params, ret) = (c.index, c.params.clone(), c.result.clone());
                match self.pass_args(args, &params) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.push(I::Call(index));
                let held = self.returned(ret);
                self.held.insert(*result, held);
            }
            Instr::Construct {
                result, args, ty, ..
            } => return self.construct(*result, args, ty),
            Instr::Project {
                result, of, field, ..
            } => return self.project(*result, *of, *field),
            Instr::Variant {
                result,
                case,
                payload,
                ty,
            } => return self.variant(*result, *case, *payload, ty),
            Instr::Case {
                result,
                case,
                fields,
                ty,
            } => return self.case(*result, *case, fields, ty),
            Instr::Match {
                result,
                scrutinee,
                arms,
                ty,
            } => return self.matched(*result, *scrutinee, arms, ty),
            Instr::Binary {
                result,
                op,
                lhs,
                rhs,
                ty,
            } => return self.binary(*result, *op, *lhs, *rhs, ty),
            Instr::Unary {
                result,
                op,
                operand,
                ty,
            } => return self.unary(*result, *op, *operand, ty),
            Instr::If {
                result,
                cond,
                then,
                els,
                ty,
            } => return self.branch(*result, *cond, then, els, ty),
            Instr::Concat { result, parts, .. } => return self.concat(*result, parts),
            Instr::Format { result, value, .. } => return self.format(*result, *value),
            Instr::MakeList { result, items, ty } => return self.make_list(*result, items, ty),
            // `return e` and `?`'s failure (ADR-0051): the result where the
            // caller takes it, and out. What follows in the region is never
            // reached; the placeholder is a holder of the type it needs.
            Instr::Return { result, value, ty } => {
                match self.leave(*value) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.push(I::Return);
                let placeholder = match ty {
                    Type::Unit => Held::Nothing,
                    t => match self.type_of(*result, t) {
                        Some(rt) => self.holder(rt),
                        None => Held::Nothing,
                    },
                };
                self.held.insert(*result, placeholder);
            }
            // The same value as another type (ADR-0054): the same locals, or
            // the same address. An opaque type's component type is an alias
            // of its representation's.
            // A handler's (ADR-0058): a component calls no command.
            Instr::Command { command, .. } => refuse!(
                "a command called from inside a component",
                "`{}` calls `{command}`; only a handler's module calls a command",
                self.export
            ),
            // A handler's (ADR-0280): the browser goes to the page.
            Instr::Navigate { route, .. } => refuse!(
                "a navigation from inside a component",
                "`{}` goes to `{route}`; only a handler's module goes to a page",
                self.export
            ),
            // A handler's (ADR-0130): a signal lives in the browser.
            Instr::SignalGet { signal, .. } | Instr::SignalSet { signal, .. } => refuse!(
                "a signal reached from inside a component",
                "`{}` reaches `{signal}`; a signal lives in the browser, and only a handler's \
                 module reaches one",
                self.export
            ),
            Instr::Retype { result, value, ty } => {
                let Some(h) = self.held.get(value).cloned() else {
                    blocked!("`{}` retypes {value:?} and nothing defines it", self.export);
                };
                let Some(rt) = self.type_of(*result, ty) else {
                    refuse!(
                        "an opaque value whose component type nothing fixes",
                        "`{}` makes a {ty:?} with no component type here",
                        self.export
                    );
                };
                let held = match h {
                    Held::Flat { ty: from, locals } if same_type(self.resolve, &from, &rt) => {
                        Held::Flat { ty: rt, locals }
                    }
                    Held::Memory { ty: from, ptr } if same_type(self.resolve, &from, &rt) => {
                        Held::Memory { ty: rt, ptr }
                    }
                    _ => blocked!(
                        "`{}` retypes a value to a component type with another layout",
                        self.export
                    ),
                };
                self.held.insert(*result, held);
            }
            // A function value (ADR-0052): an environment in the region, its
            // first word the table slot of its code, then each capture.
            Instr::Closure {
                result,
                index,
                captures,
                ..
            } => {
                let Some(code) = self.closures.get(index) else {
                    blocked!("`{}` makes a function value nothing compiled", self.export);
                };
                let (size, align, slots) = (code.size, code.align, code.captures.clone());
                let env = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::I32Const(0),
                    I::I32Const(0),
                    I::I32Const(align as i32),
                    I::I32Const(size as i32),
                    I::Call(self.realloc_index),
                    I::LocalSet(env),
                    I::LocalGet(env),
                    I::I32Const(*index as i32),
                    I::I32Store(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                ]);
                for (v, (ty, offset)) in captures.iter().zip(slots) {
                    match self.store_at(*v, ty, env, offset) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                }
                self.held.insert(
                    *result,
                    Held::Flat {
                        ty: WitType::U32,
                        locals: vec![env],
                    },
                );
            }
            // A call through a function value (ADR-0052): its environment
            // and its arguments, then `call_indirect` through the slot the
            // environment names.
            Instr::Apply {
                result,
                function,
                function_ty,
                args,
                ..
            } => {
                let Some(sig) = self.fn_sigs.get(function_ty) else {
                    blocked!(
                        "`{}` calls a {function_ty:?} with no signature",
                        self.export
                    );
                };
                let (type_index, params, ret) =
                    (sig.type_index, sig.params.clone(), sig.result.clone());
                let env = match self.flat_locals(*function) {
                    Encoding::Encoded((_, ls)) => ls[0],
                    other => return other.map(|_| unreachable!()),
                };
                self.ops.push(I::LocalGet(env));
                match self.pass_args(args, &params) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.extend([
                    I::LocalGet(env),
                    I::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    I::CallIndirect {
                        type_index,
                        table_index: 0,
                    },
                ]);
                let held = self.returned(ret);
                self.held.insert(*result, held);
            }
            // A mutable binding (ADR-0051): a holder of its own, which each
            // assignment moves a value into and each read copies out of.
            Instr::Local { result, init, ty } => {
                let Some(rt) = self.type_of(*result, ty) else {
                    refuse!(
                        "a variable whose component type nothing fixes",
                        "`{}` binds a {ty:?} with no component type here",
                        self.export
                    );
                };
                let holder = self.holder(rt);
                match self.move_into(*init, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.held.insert(*result, holder);
            }
            Instr::Set {
                result,
                local,
                value,
                ..
            } => {
                let Some(holder) = self.held.get(local).cloned() else {
                    blocked!("`{}` assigns a variable nothing binds", self.export);
                };
                match self.move_into(*value, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.held.insert(*result, Held::Nothing);
            }
            Instr::Get { result, local, .. } => {
                let Some(Some(rt)) = self.held.get(local).map(|h| h.ty().copied()) else {
                    blocked!("`{}` reads a variable nothing binds", self.export);
                };
                let copy = self.holder(rt);
                match self.move_into(*local, &copy) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.held.insert(*result, copy);
            }
            Instr::Intrinsic {
                result,
                op,
                args,
                ty,
            } => return self.intrinsic(*result, *op, args, ty),
            Instr::Each {
                result,
                kind,
                list,
                seed,
                params,
                body,
                ty,
            } => return self.each(*result, *kind, *list, *seed, params, body, ty),
        }
        Encoding::Encoded(())
    }

    /// The component type a value must have: what its use fixes, else what
    /// its IR type is (ADR-0039 §5).
    fn type_of(&self, result: ValueId, ty: &Type) -> Option<WitType> {
        self.expected
            .get(&result)
            .copied()
            .or_else(|| self.wit.get(ty).copied())
    }

    /// **A value's flat core values, in locals**: its own when it is held
    /// flat, read from its layout when it is held in memory.
    fn flat_locals(&mut self, v: ValueId) -> Encoding<(WitType, Vec<u32>)> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        match self.held.get(&v).cloned() {
            Some(Held::Flat { ty, locals }) => Encoding::Encoded((ty, locals)),
            Some(Held::Memory { ty, ptr }) => {
                let Some(flats) = flat(resolve, &ty) else {
                    refuse!(
                        "a value that does not flatten",
                        "`{}` reads a value too large to hold in locals",
                        self.export
                    );
                };
                match load(resolve, sizes, &mut self.locals, &ty, ptr, 0, &mut self.ops) {
                    Encoding::Encoded(()) => {}
                    other => return other.map(|_| unreachable!()),
                }
                let ls: Vec<u32> = flats
                    .iter()
                    .map(|t| self.locals.fresh(core_type(*t)))
                    .collect();
                for l in ls.iter().rev() {
                    self.ops.push(I::LocalSet(*l));
                }
                Encoding::Encoded((ty, ls))
            }
            Some(Held::Nothing) => blocked!("`{}` uses a call with no result", self.export),
            None => blocked!("`{}` uses {v:?} and nothing defines it", self.export),
        }
    }

    /// **Where a value chosen by a branch is put**: locals, for a type that
    /// flattens without variant slots; an address otherwise.
    fn holder(&mut self, ty: WitType) -> Held {
        match flat(self.resolve, &ty) {
            Some(flats) if plain(self.resolve, &ty) => Held::Flat {
                ty,
                locals: flats
                    .iter()
                    .map(|t| self.locals.fresh(core_type(*t)))
                    .collect(),
            },
            _ => Held::Memory {
                ty,
                ptr: self.locals.fresh(ValType::I32),
            },
        }
    }

    /// Emit a region, and move its value into `holder`.
    fn settle(&mut self, region: &super::ir::Region, holder: &Held) -> Encoding<()> {
        match self.region(&region.instrs) {
            Encoding::Encoded(()) => {}
            other => return other,
        }
        self.move_into(region.value, holder)
    }

    /// Move a value into a branch's holder, checked against its type.
    fn move_into(&mut self, value: ValueId, holder: &Held) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(value) = self.held.get(&value).cloned() else {
            blocked!("`{}`'s branch ends in a value nothing defines", self.export);
        };
        let Some(vt) = value.ty() else {
            blocked!("`{}`'s branch ends in a call with no result", self.export);
        };
        let rt = *holder.ty().expect("a holder has a type");
        if !same_type(resolve, vt, &rt) {
            blocked!(
                "`{}`'s branches produce values of different component types",
                self.export
            );
        }
        match (holder, &value) {
            (Held::Memory { ptr: out, .. }, Held::Memory { ptr: v, .. }) => {
                self.ops.push(I::LocalGet(*v));
                self.ops.push(I::LocalSet(*out));
            }
            (Held::Memory { ptr: out, .. }, Held::Flat { locals: flats, .. }) => {
                allocate(sizes, &rt, self.realloc_index, *out, &mut self.ops);
                match store(
                    resolve,
                    sizes,
                    &mut self.locals,
                    &rt,
                    *out,
                    0,
                    flats,
                    &mut self.ops,
                ) {
                    Encoding::Encoded(_) => {}
                    other => return other.map(|_| unreachable!()),
                }
            }
            (Held::Flat { locals: outs, .. }, v) => {
                match push_flat_values(resolve, sizes, &mut self.locals, v, &mut self.ops) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                for l in outs.iter().rev() {
                    self.ops.push(I::LocalSet(*l));
                }
            }
            (_, Held::Nothing) | (Held::Nothing, _) => {
                blocked!("`{}`'s branch ends in a call with no result", self.export)
            }
        }
        Encoding::Encoded(())
    }

    /// **Arithmetic and comparisons** (ADR-0039 §1, §2).
    fn binary(
        &mut self,
        result: ValueId,
        op: BinaryOp,
        lhs: ValueId,
        rhs: ValueId,
        ty: &Type,
    ) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (lt, l) = match self.flat_locals(lhs) {
            Encoding::Encoded(v) => v,
            other => return other.map(|_| unreachable!()),
        };
        let (rt, r) = match self.flat_locals(rhs) {
            Encoding::Encoded(v) => v,
            other => return other.map(|_| unreachable!()),
        };
        if !same_type(self.resolve, &lt, &rt) {
            blocked!(
                "`{}` applies `{op:?}` to two values of different component types",
                self.export
            );
        }
        let Some(out_ty) = self.type_of(result, ty) else {
            blocked!("`{}`'s `{op:?}` has no component type", self.export);
        };
        let out = match dealias(self.resolve, lt) {
            WitType::S64 => self.int_binary(op, l[0], r[0]),
            WitType::F64 => {
                let (code, vt) = match op {
                    BinaryOp::Add => (I::F64Add, ValType::F64),
                    BinaryOp::Sub => (I::F64Sub, ValType::F64),
                    BinaryOp::Mul => (I::F64Mul, ValType::F64),
                    BinaryOp::Div => (I::F64Div, ValType::F64),
                    BinaryOp::Eq => (I::F64Eq, ValType::I32),
                    BinaryOp::Ne => (I::F64Ne, ValType::I32),
                    BinaryOp::Lt => (I::F64Lt, ValType::I32),
                    BinaryOp::Le => (I::F64Le, ValType::I32),
                    BinaryOp::Gt => (I::F64Gt, ValType::I32),
                    BinaryOp::Ge => (I::F64Ge, ValType::I32),
                    BinaryOp::Rem => refuse!(
                        "`%` on a Float",
                        "`{}`: its semantics are not decided",
                        self.export
                    ),
                };
                let out = self.locals.fresh(vt);
                self.ops.push(I::LocalGet(l[0]));
                self.ops.push(I::LocalGet(r[0]));
                self.ops.push(code);
                self.ops.push(I::LocalSet(out));
                out
            }
            WitType::Bool => {
                let code = match op {
                    BinaryOp::Eq => I::I32Eq,
                    BinaryOp::Ne => I::I32Ne,
                    other => refuse!(
                        "an operator on a Bool",
                        "`{}` applies `{other:?}` to a Bool",
                        self.export
                    ),
                };
                let out = self.locals.fresh(ValType::I32);
                self.ops.push(I::LocalGet(l[0]));
                self.ops.push(I::LocalGet(r[0]));
                self.ops.push(code);
                self.ops.push(I::LocalSet(out));
                out
            }
            WitType::String => {
                let (helper, test) = match op {
                    BinaryOp::Eq => (Helper::StrEq, None),
                    BinaryOp::Ne => (Helper::StrEq, Some(I::I32Eqz)),
                    BinaryOp::Lt => (Helper::StrCmp, Some(I::I32LtS)),
                    BinaryOp::Le => (Helper::StrCmp, Some(I::I32LeS)),
                    BinaryOp::Gt => (Helper::StrCmp, Some(I::I32GtS)),
                    BinaryOp::Ge => (Helper::StrCmp, Some(I::I32GeS)),
                    other => refuse!(
                        "an operator on a String",
                        "`{}` applies `{other:?}` to a String; an interpolation joins strings",
                        self.export
                    ),
                };
                let index = self.helpers.index(helper);
                for x in [l[0], l[1], r[0], r[1]] {
                    self.ops.push(I::LocalGet(x));
                }
                self.ops.push(I::Call(index));
                match test {
                    Some(I::I32Eqz) => self.ops.push(I::I32Eqz),
                    Some(cmp) => {
                        self.ops.push(I::I32Const(0));
                        self.ops.push(cmp);
                    }
                    None => {}
                }
                let out = self.locals.fresh(ValType::I32);
                self.ops.push(I::LocalSet(out));
                out
            }
            other => refuse!(
                "an operator on a value of this type",
                "`{}` applies `{op:?}` to {other:?}",
                self.export
            ),
        };
        self.held.insert(
            result,
            Held::Flat {
                ty: out_ty,
                locals: vec![out],
            },
        );
        Encoding::Encoded(())
    }

    /// **`Int` arithmetic that traps rather than wraps**, with Euclidean `/`
    /// and `%` (ADR-0039 §1). Returns the local holding the result.
    fn int_binary(&mut self, op: BinaryOp, a: u32, b: u32) -> u32 {
        use wasm_encoder::Instruction as I;
        let get = |l: u32| I::LocalGet(l);
        if op.compares() {
            let out = self.locals.fresh(ValType::I32);
            let code = match op {
                BinaryOp::Eq => I::I64Eq,
                BinaryOp::Ne => I::I64Ne,
                BinaryOp::Lt => I::I64LtS,
                BinaryOp::Le => I::I64LeS,
                BinaryOp::Gt => I::I64GtS,
                _ => I::I64GeS,
            };
            self.ops.extend([get(a), get(b), code, I::LocalSet(out)]);
            return out;
        }
        let r = self.locals.fresh(ValType::I64);
        let code = match op {
            BinaryOp::Add => I::I64Add,
            BinaryOp::Sub => I::I64Sub,
            BinaryOp::Mul => I::I64Mul,
            // Wasm truncates, and traps on a zero divisor and on MIN / -1.
            BinaryOp::Div => I::I64DivS,
            BinaryOp::Rem => I::I64RemS,
            _ => unreachable!("comparisons returned above"),
        };
        self.ops.extend([get(a), get(b), code, I::LocalSet(r)]);
        let trap = trap_index(self.realloc_index, Cause::IntOverflow);
        let ops = &mut self.ops;
        match op {
            BinaryOp::Add => trap_on_add_overflow(ops, a, b, r, trap),
            BinaryOp::Sub => trap_on_sub_overflow(ops, a, b, r, trap),
            BinaryOp::Mul => trap_on_mul_overflow(ops, a, b, r, trap),
            BinaryOp::Div => euclidean_quotient(ops, a, b, r),
            BinaryOp::Rem => euclidean_remainder(ops, b, r),
            _ => {}
        }
        r
    }

    /// `-x` and `!b`.
    fn unary(&mut self, result: ValueId, op: UnaryOp, operand: ValueId, ty: &Type) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (t, v) = match self.flat_locals(operand) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        let Some(out_ty) = self.type_of(result, ty) else {
            blocked!("`{}`'s `{op:?}` has no component type", self.export);
        };
        let out = match (op, dealias(self.resolve, t)) {
            (UnaryOp::Neg, WitType::S64) => {
                let out = self.locals.fresh(ValType::I64);
                let trap = trap_index(self.realloc_index, Cause::IntOverflow);
                trap_on_negation_overflow(&mut self.ops, v[0], trap);
                self.ops.extend([
                    I::I64Const(0),
                    I::LocalGet(v[0]),
                    I::I64Sub,
                    I::LocalSet(out),
                ]);
                out
            }
            (UnaryOp::Neg, WitType::F64) => {
                let out = self.locals.fresh(ValType::F64);
                self.ops
                    .extend([I::LocalGet(v[0]), I::F64Neg, I::LocalSet(out)]);
                out
            }
            (UnaryOp::Not, WitType::Bool) => {
                let out = self.locals.fresh(ValType::I32);
                self.ops
                    .extend([I::LocalGet(v[0]), I::I32Eqz, I::LocalSet(out)]);
                out
            }
            (op, other) => refuse!(
                "an operator on a value of this type",
                "`{}` applies `{op:?}` to {other:?}",
                self.export
            ),
        };
        self.held.insert(
            result,
            Held::Flat {
                ty: out_ty,
                locals: vec![out],
            },
        );
        Encoding::Encoded(())
    }

    /// **`if c { .. } else { .. }`**: each side's value into one holder.
    fn branch(
        &mut self,
        result: ValueId,
        cond: ValueId,
        then: &super::ir::Region,
        els: &super::ir::Region,
        ty: &Type,
    ) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (ct, c) = match self.flat_locals(cond) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        if dealias(self.resolve, ct) != WitType::Bool {
            blocked!("`{}` branches on a value that is not a Bool", self.export);
        }
        // A statement (ADR-0051): each branch runs for what it does.
        if *ty == Type::Unit {
            self.ops.push(I::LocalGet(c[0]));
            self.ops.push(I::If(wasm_encoder::BlockType::Empty));
            match self.region(&then.instrs) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
            self.ops.push(I::Else);
            match self.region(&els.instrs) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
            self.ops.push(I::End);
            self.held.insert(result, Held::Nothing);
            return Encoding::Encoded(());
        }
        let Some(rt) = self.type_of(result, ty) else {
            refuse!(
                "an `if` whose component type nothing fixes",
                "`{}`'s `if` produces a value with no component type",
                self.export
            );
        };
        let holder = self.holder(rt);
        self.ops.push(I::LocalGet(c[0]));
        self.ops.push(I::If(wasm_encoder::BlockType::Empty));
        match self.settle(then, &holder) {
            Encoding::Encoded(()) => {}
            other => return other,
        }
        self.ops.push(I::Else);
        match self.settle(els, &holder) {
            Encoding::Encoded(()) => {}
            other => return other,
        }
        self.ops.push(I::End);
        self.held.insert(result, holder);
        Encoding::Encoded(())
    }

    /// **Strings joined**: one allocation of their total length, each copied
    /// in after the one before.
    fn concat(&mut self, result: ValueId, parts: &[ValueId]) -> Encoding<()> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let mut pieces = Vec::new();
        for p in parts {
            match self.flat_locals(*p) {
                Encoding::Encoded((t, ls)) if dealias(self.resolve, t) == WitType::String => {
                    pieces.push((ls[0], ls[1]))
                }
                Encoding::Encoded(_) => {
                    blocked!("`{}` joins a value that is not a String", self.export)
                }
                other => return other.map(|_| unreachable!()),
            }
        }
        // The total, in 64 bits, refused past what one region could hold.
        let total = self.locals.fresh(ValType::I32);
        self.ops.push(I::I64Const(0));
        for (_, len) in &pieces {
            self.ops
                .extend([I::LocalGet(*len), I::I64ExtendI32U, I::I64Add]);
        }
        let wide = self.locals.fresh(ValType::I64);
        self.ops.extend([
            I::LocalTee(wide),
            I::I64Const(i32::MAX as i64),
            I::I64GtU,
            I::If(Empty),
            I::Call(trap_index(self.realloc_index, Cause::TooLongForMemory)),
            I::Unreachable,
            I::End,
            I::LocalGet(wide),
            I::I32WrapI64,
            I::LocalSet(total),
        ]);
        let dst = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::I32Const(0),
            I::I32Const(0),
            I::I32Const(1),
            I::LocalGet(total),
            I::Call(self.realloc_index),
            I::LocalSet(dst),
        ]);
        let cursor = self.locals.fresh(ValType::I32);
        self.ops.extend([I::LocalGet(dst), I::LocalSet(cursor)]);
        for (ptr, len) in pieces {
            self.ops.extend([
                I::LocalGet(cursor),
                I::LocalGet(ptr),
                I::LocalGet(len),
                I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                },
                I::LocalGet(cursor),
                I::LocalGet(len),
                I::I32Add,
                I::LocalSet(cursor),
            ]);
        }
        self.held.insert(
            result,
            Held::Flat {
                ty: WitType::String,
                locals: vec![dst, total],
            },
        );
        Encoding::Encoded(())
    }

    /// **A value as text**: an `Int` in decimal, a `Bool` as `true`/`false`.
    fn format(&mut self, result: ValueId, value: ValueId) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (t, v) = match self.flat_locals(value) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        let (ptr, len) = (
            self.locals.fresh(ValType::I32),
            self.locals.fresh(ValType::I32),
        );
        match dealias(self.resolve, t) {
            WitType::S64 => {
                let index = self.helpers.index(Helper::IntToString);
                self.ops.extend([
                    I::LocalGet(v[0]),
                    I::Call(index),
                    I::LocalSet(len),
                    I::LocalSet(ptr),
                ]);
            }
            WitType::Bool => {
                let (Some(yes), Some(no)) = (
                    self.literals.at.get("true").copied(),
                    self.literals.at.get("false").copied(),
                ) else {
                    blocked!("`{}` formats a Bool with no literals for it", self.export);
                };
                self.ops.extend([
                    I::I32Const(yes as i32),
                    I::I32Const(no as i32),
                    I::LocalGet(v[0]),
                    I::Select,
                    I::LocalSet(ptr),
                    I::I32Const(4),
                    I::I32Const(5),
                    I::LocalGet(v[0]),
                    I::Select,
                    I::LocalSet(len),
                ]);
            }
            other => refuse!(
                "a value of this type as text",
                "`{}` formats {other:?}",
                self.export
            ),
        }
        self.held.insert(
            result,
            Held::Flat {
                ty: WitType::String,
                locals: vec![ptr, len],
            },
        );
        Encoding::Encoded(())
    }

    /// **A record, built in the region**: its canonical layout, each field
    /// stored at the offset `SizeAlign` gives (ADR-0039 §6).
    /// **The cell a box holds** (ADR-0202), where `t` is a box: the WIT type
    /// of a value of a type that holds itself in place.
    fn cell_of(&self, t: WitType) -> Option<WitType> {
        match t {
            WitType::Id(id) => self.cells.get(&id).map(|c| WitType::Id(*c)),
            _ => None,
        }
    }

    /// **A box, as its cell** (ADR-0202): a box held flat is its cell's
    /// address, and one held in memory is read from there first. Anything
    /// else is held as it is.
    fn unboxed(&mut self, h: Held) -> Held {
        use wasm_encoder::Instruction as I;
        let Some(cell) = h.ty().and_then(|t| self.cell_of(*t)) else {
            return h;
        };
        match h {
            Held::Flat { locals, .. } => Held::Memory {
                ty: cell,
                ptr: locals[0],
            },
            Held::Memory { ptr, .. } => {
                let at = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::LocalGet(ptr),
                    I::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    I::LocalSet(at),
                ]);
                Held::Memory { ty: cell, ptr: at }
            }
            Held::Nothing => Held::Nothing,
        }
    }

    fn construct(&mut self, result: ValueId, args: &[ValueId], ty: &Type) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(rt) = self.type_of(result, ty) else {
            refuse!(
                "a record whose component type nothing fixes",
                "`{}` builds a {ty:?}, which has no component type here",
                self.export
            );
        };
        // A type that holds itself in place is built in its cell, and the
        // value is the cell's address (ADR-0202).
        let (rt, boxed) = match self.cell_of(rt) {
            Some(cell) => (cell, Some(rt)),
            None => (rt, None),
        };
        let WitType::Id(id) = dealias(resolve, rt) else {
            blocked!(
                "`{}` builds a record of a type that is not one",
                self.export
            );
        };
        // A sum type's case is `Instr::Case` (ADR-0059); a record built
        // over another kind of type is an IR error, never laid out as one.
        let TypeDefKind::Record(r) = &resolve.types[id].kind else {
            blocked!(
                "`{}` builds a record of a type that is not one",
                self.export
            );
        };
        if r.fields.len() != args.len() {
            blocked!(
                "`{}` builds a record of {} fields with {} values",
                self.export,
                r.fields.len(),
                args.len()
            );
        }
        let area = self.locals.fresh(ValType::I32);
        allocate(sizes, &rt, self.realloc_index, area, &mut self.ops);
        let offsets = sizes.field_offsets(r.fields.iter().map(|f| &f.ty));
        for ((offset, field_ty), a) in offsets.into_iter().zip(args) {
            let offset = offset.size_wasm32() as u64;
            let Some(h) = self.held.get(a).cloned() else {
                blocked!("`{}` stores {a:?} and nothing defines it", self.export);
            };
            let Some(ht) = h.ty() else {
                blocked!("`{}` stores a call with no result", self.export);
            };
            if !same_type(resolve, ht, field_ty) {
                blocked!(
                    "`{}` stores a value of another component type in a record field",
                    self.export
                );
            }
            match h {
                Held::Memory { ptr, .. } => {
                    self.ops.push(I::LocalGet(area));
                    if offset != 0 {
                        self.ops.push(I::I32Const(offset as i32));
                        self.ops.push(I::I32Add);
                    }
                    self.ops.push(I::LocalGet(ptr));
                    self.ops
                        .push(I::I32Const(sizes.size(field_ty).size_wasm32() as i32));
                    self.ops.push(I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    });
                }
                Held::Flat { locals, .. } => {
                    match store(
                        resolve,
                        sizes,
                        &mut self.locals,
                        field_ty,
                        area,
                        offset,
                        &locals,
                        &mut self.ops,
                    ) {
                        Encoding::Encoded(_) => {}
                        other => return other.map(|_| unreachable!()),
                    }
                }
                Held::Nothing => blocked!("`{}` stores a call with no result", self.export),
            }
        }
        self.held.insert(
            result,
            match boxed {
                Some(b) => Held::Flat {
                    ty: b,
                    locals: vec![area],
                },
                None => Held::Memory { ty: rt, ptr: area },
            },
        );
        Encoding::Encoded(())
    }

    /// The element type of a list's component type.
    fn element_of(&self, list: WitType) -> Option<WitType> {
        match dealias(self.resolve, list) {
            WitType::Id(id) => match &self.resolve.types[id].kind {
                TypeDefKind::List(e) => Some(*e),
                _ => None,
            },
            _ => None,
        }
    }

    /// A type's size and alignment in memory, from `SizeAlign`.
    fn layout(&self, t: &WitType) -> (u32, u32) {
        (
            self.sizes.size(t).size_wasm32() as u32,
            self.sizes.align(t).align_wasm32() as u32,
        )
    }

    /// A value's flat core values, loaded from `addr + offset` into fresh
    /// locals.
    fn load_flat(&mut self, ty: WitType, addr: u32, offset: u64) -> Encoding<Vec<u32>> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(flats) = flat(resolve, &ty) else {
            refuse!(
                "a value that does not flatten",
                "`{}` reads a key too large to hold in locals",
                self.export
            );
        };
        match load(
            resolve,
            sizes,
            &mut self.locals,
            &ty,
            addr,
            offset,
            &mut self.ops,
        ) {
            Encoding::Encoded(()) => {}
            other => return other.map(|_| unreachable!()),
        }
        let ls: Vec<u32> = flats
            .iter()
            .map(|t| self.locals.fresh(core_type(*t)))
            .collect();
        for l in ls.iter().rev() {
            self.ops.push(I::LocalSet(*l));
        }
        Encoding::Encoded(ls)
    }

    /// **The order of two keys** (ADR-0057), each its flat values: -1, 0 or
    /// 1, in a fresh `i32`. An `Int` by value; a `String` by its bytes,
    /// which is code point order; a `Bool`, `false` before `true` (ADR-0248).
    /// An opaque type's alias is its representation's.
    fn key_order(&mut self, key: WitType, x: &[u32], y: &[u32]) -> Encoding<u32> {
        use wasm_encoder::Instruction as I;
        let out = self.locals.fresh(ValType::I32);
        match dealias(self.resolve, key) {
            WitType::S64 => self.ops.extend([
                I::LocalGet(x[0]),
                I::LocalGet(y[0]),
                I::I64GtS,
                I::LocalGet(x[0]),
                I::LocalGet(y[0]),
                I::I64LtS,
                I::I32Sub,
                I::LocalSet(out),
            ]),
            WitType::Bool => self.ops.extend([
                I::LocalGet(x[0]),
                I::LocalGet(y[0]),
                I::I32GtU,
                I::LocalGet(x[0]),
                I::LocalGet(y[0]),
                I::I32LtU,
                I::I32Sub,
                I::LocalSet(out),
            ]),
            WitType::String => {
                let index = self.helpers.index(Helper::StrCmp);
                self.ops.extend([
                    I::LocalGet(x[0]),
                    I::LocalGet(x[1]),
                    I::LocalGet(y[0]),
                    I::LocalGet(y[1]),
                    I::Call(index),
                    I::LocalSet(out),
                ]);
            }
            other => refuse!(
                "a map or set keyed by a type other than Int, String or Bool",
                "`{}` orders keys of {other:?}",
                self.export
            ),
        }
        Encoding::Encoded(out)
    }

    /// **The order of the keys at two addresses** (ADR-0057).
    fn order_at(&mut self, key: WitType, a: u32, b: u32) -> Encoding<u32> {
        let x = match self.load_flat(key, a, 0) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        let y = match self.load_flat(key, b, 0) {
            Encoding::Encoded(y) => y,
            other => return other.map(|_| unreachable!()),
        };
        self.key_order(key, &x, &y)
    }

    /// **Where `probe` goes** among `len` entries of `size` bytes at `ptr`,
    /// each led by its key (ADR-0057): the first entry whose key is not below
    /// `probe`, and whether that key is `probe`. Two fresh `i32`s.
    fn search(
        &mut self,
        key: WitType,
        ptr: u32,
        len: u32,
        size: u32,
        probe: &[u32],
    ) -> Encoding<(u32, u32)> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
        let (lo, hi, mid, found) = (fresh(self), fresh(self), fresh(self), fresh(self));
        self.ops.extend([
            I::I32Const(0),
            I::LocalSet(lo),
            I::LocalGet(len),
            I::LocalSet(hi),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(lo),
            I::LocalGet(hi),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(lo),
            I::LocalGet(hi),
            I::I32Add,
            I::I32Const(1),
            I::I32ShrU,
            I::LocalSet(mid),
        ]);
        let at = self.element_address(ptr, mid, size);
        let k = match self.load_flat(key, at, 0) {
            Encoding::Encoded(k) => k,
            other => return other.map(|_| unreachable!()),
        };
        let o = match self.key_order(key, &k, probe) {
            Encoding::Encoded(o) => o,
            other => return other.map(|_| unreachable!()),
        };
        self.ops.extend([
            I::LocalGet(o),
            I::I32Const(0),
            I::I32LtS,
            I::If(Empty),
            I::LocalGet(mid),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(lo),
            I::Else,
            I::LocalGet(mid),
            I::LocalSet(hi),
            I::End,
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(0),
            I::LocalSet(found),
            I::LocalGet(lo),
            I::LocalGet(len),
            I::I32LtU,
            I::If(Empty),
        ]);
        let at = self.element_address(ptr, lo, size);
        let k = match self.load_flat(key, at, 0) {
            Encoding::Encoded(k) => k,
            other => return other.map(|_| unreachable!()),
        };
        let o = match self.key_order(key, &k, probe) {
            Encoding::Encoded(o) => o,
            other => return other.map(|_| unreachable!()),
        };
        self.ops
            .extend([I::LocalGet(o), I::I32Eqz, I::LocalSet(found), I::End]);
        Encoding::Encoded((lo, found))
    }

    /// **Two sets merged** (ADR-0057): both ascending, so one pass, into
    /// room for both. Its address and count.
    fn merged(
        &mut self,
        op: super::ir::Intrinsic,
        entry: WitType,
        key: WitType,
        (pa, la): (u32, u32),
        (pb, lb): (u32, u32),
    ) -> Encoding<(u32, u32)> {
        use super::ir::Intrinsic as N;
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (esize, ealign) = self.layout(&entry);
        let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
        let (total, i, j, count, o) = (
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
        );
        self.ops.extend([
            I::LocalGet(la),
            I::LocalGet(lb),
            I::I32Add,
            I::LocalSet(total),
        ]);
        let out = self.alloc_array(total, esize, ealign);
        self.ops.extend([
            I::I32Const(0),
            I::LocalSet(i),
            I::I32Const(0),
            I::LocalSet(j),
            I::I32Const(0),
            I::LocalSet(count),
            I::Block(Empty),
            I::Loop(Empty),
        ]);
        // Done: a union when both are, an intersection when either is, a
        // difference when the first is.
        match op {
            N::SetUnion => self.ops.extend([
                I::LocalGet(i),
                I::LocalGet(la),
                I::I32GeU,
                I::LocalGet(j),
                I::LocalGet(lb),
                I::I32GeU,
                I::I32And,
                I::BrIf(1),
            ]),
            N::SetIntersection => self.ops.extend([
                I::LocalGet(i),
                I::LocalGet(la),
                I::I32GeU,
                I::LocalGet(j),
                I::LocalGet(lb),
                I::I32GeU,
                I::I32Or,
                I::BrIf(1),
            ]),
            _ => self
                .ops
                .extend([I::LocalGet(i), I::LocalGet(la), I::I32GeU, I::BrIf(1)]),
        }
        // o: below zero takes the first's element, above the second's, zero
        // both; one that has run out gives way to the other.
        self.ops.extend([
            I::LocalGet(j),
            I::LocalGet(lb),
            I::I32GeU,
            I::If(Empty),
            I::I32Const(-1),
            I::LocalSet(o),
            I::Else,
            I::LocalGet(i),
            I::LocalGet(la),
            I::I32GeU,
            I::If(Empty),
            I::I32Const(1),
            I::LocalSet(o),
            I::Else,
        ]);
        let a = self.element_address(pa, i, esize);
        let b = self.element_address(pb, j, esize);
        let order = match self.order_at(key, a, b) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        self.ops
            .extend([I::LocalGet(order), I::LocalSet(o), I::End, I::End]);
        let take = |this: &mut Self, from: u32, at: u32| {
            let to = this.element_address(out, count, esize);
            let src = this.element_address(from, at, esize);
            this.copy(to, src, esize);
            this.ops.extend(increment(count));
        };
        self.ops
            .extend([I::LocalGet(o), I::I32Const(0), I::I32LtS, I::If(Empty)]);
        if op != N::SetIntersection {
            take(self, pa, i);
        }
        self.ops.extend(increment(i));
        self.ops.extend([
            I::Else,
            I::LocalGet(o),
            I::I32Const(0),
            I::I32GtS,
            I::If(Empty),
        ]);
        if op == N::SetUnion {
            take(self, pb, j);
        }
        self.ops.extend(increment(j));
        self.ops.push(I::Else);
        if op != N::SetDifference {
            take(self, pa, i);
        }
        self.ops.extend(increment(i));
        self.ops.extend(increment(j));
        self.ops.extend([I::End, I::End, I::Br(0), I::End, I::End]);
        Encoding::Encoded((out, count))
    }

    /// **A map's or a set's shape** (ADR-0057): its element, the element's
    /// key, and, for a map, the value's type and its offset in the entry.
    fn entry_of(&self, list: WitType) -> Option<(WitType, WitType, MapValue)> {
        let entry = self.element_of(list)?;
        match dealias(self.resolve, entry) {
            WitType::Id(id) => match &self.resolve.types[id].kind {
                TypeDefKind::Tuple(t) if t.types.len() == 2 => {
                    let offsets = self.sizes.field_offsets(t.types.iter());
                    let value_at = offsets[1].0.size_wasm32() as u64;
                    Some((entry, t.types[0], Some((t.types[1], value_at))))
                }
                _ => None,
            },
            _ => Some((entry, entry, None)),
        }
    }

    /// **A map's or a set's entries sorted by key, each key once**
    /// (ADR-0057): `len` entries at `ptr` into a new array, stably, and of
    /// each run of equal keys the last kept. Its address and count.
    fn sorted(&mut self, ptr: u32, len: u32, entry: WitType, key: WitType) -> Encoding<(u32, u32)> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (esize, ealign) = self.layout(&entry);
        let sorted = match self.merge_sort(ptr, len, entry, &mut |this, a, b, take| {
            let o = match this.order_at(key, a, b) {
                Encoding::Encoded(o) => o,
                other => return other.map(|_| unreachable!()),
            };
            this.ops
                .extend([I::LocalGet(o), I::I32Const(0), I::I32LeS, I::LocalSet(take)]);
            Encoding::Encoded(())
        }) {
            Encoding::Encoded(s) => s,
            other => return other.map(|_| unreachable!()),
        };
        let out = self.alloc_array(len, esize, ealign);
        let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
        let (i, count, next, keep) = (fresh(self), fresh(self), fresh(self), fresh(self));
        self.ops.extend([
            I::I32Const(0),
            I::LocalSet(i),
            I::I32Const(0),
            I::LocalSet(count),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(i),
            I::LocalGet(len),
            I::I32GeU,
            I::BrIf(1),
            // Kept when it is the last, or the next key is another.
            I::I32Const(1),
            I::LocalSet(keep),
            I::LocalGet(i),
            I::I32Const(1),
            I::I32Add,
            I::LocalTee(next),
            I::LocalGet(len),
            I::I32LtU,
            I::If(Empty),
        ]);
        let a = self.element_address(sorted, i, esize);
        let b = self.element_address(sorted, next, esize);
        let o = match self.order_at(key, a, b) {
            Encoding::Encoded(o) => o,
            other => return other.map(|_| unreachable!()),
        };
        self.ops.extend([
            I::LocalGet(o),
            I::I32Const(0),
            I::I32Ne,
            I::LocalSet(keep),
            I::End,
            I::LocalGet(keep),
            I::If(Empty),
        ]);
        let to = self.element_address(out, count, esize);
        let from = self.element_address(sorted, i, esize);
        self.copy(to, from, esize);
        self.ops.extend([
            I::LocalGet(count),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(count),
            I::End,
            I::LocalGet(next),
            I::LocalSet(i),
            I::Br(0),
            I::End,
            I::End,
        ]);
        Encoding::Encoded((out, count))
    }

    /// **`min(max(n, 0), len)`**, as an `i32`: a count or a position, an
    /// `i64` local, clamped to a list of `len` elements, an `i32` local.
    fn clamped(&mut self, n: u32, len: u32) -> u32 {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let wide = self.locals.fresh(ValType::I64);
        let out = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::LocalGet(len),
            I::I64ExtendI32U,
            I::LocalSet(wide),
            I::LocalGet(n),
            I::I64Const(0),
            I::I64LtS,
            I::If(Empty),
            I::I64Const(0),
            I::LocalSet(wide),
            I::Else,
            I::LocalGet(n),
            I::LocalGet(wide),
            I::I64LtS,
            I::If(Empty),
            I::LocalGet(n),
            I::LocalSet(wide),
            I::End,
            I::End,
            I::LocalGet(wide),
            I::I32WrapI64,
            I::LocalSet(out),
        ]);
        out
    }

    /// Allocate `count * size` bytes, `count` an `i32` local: the product in
    /// 64 bits, and a trap past what one region could hold.
    fn alloc_array(&mut self, count: u32, size: u32, align: u32) -> u32 {
        use wasm_encoder::Instruction as I;
        let bytes = self.locals.fresh(ValType::I64);
        self.ops.extend([
            I::LocalGet(count),
            I::I64ExtendI32U,
            I::I64Const(size as i64),
            I::I64Mul,
            I::LocalTee(bytes),
            I::I64Const(i32::MAX as i64),
            I::I64GtU,
            I::If(wasm_encoder::BlockType::Empty),
            I::Call(trap_index(self.realloc_index, Cause::TooLongForMemory)),
            I::Unreachable,
            I::End,
        ]);
        let out = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::I32Const(0),
            I::I32Const(0),
            I::I32Const(align as i32),
            I::LocalGet(bytes),
            I::I32WrapI64,
            I::Call(self.realloc_index),
            I::LocalSet(out),
        ]);
        out
    }

    /// A fresh local holding `base + index * size`.
    fn element_address(&mut self, base: u32, index: u32, size: u32) -> u32 {
        use wasm_encoder::Instruction as I;
        let at = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::LocalGet(base),
            I::LocalGet(index),
            I::I32Const(size as i32),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(at),
        ]);
        at
    }

    /// Store a value's layout at `addr + offset`: copied when it is in memory,
    /// written from its flat values otherwise.
    fn store_at(&mut self, value: ValueId, ty: WitType, addr: u32, offset: u64) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(h) = self.held.get(&value).cloned() else {
            blocked!("`{}` stores {value:?} and nothing defines it", self.export);
        };
        let Some(ht) = h.ty() else {
            blocked!("`{}` stores a call with no result", self.export);
        };
        if !same_type(resolve, ht, &ty) {
            blocked!(
                "`{}` stores a value of another component type in a list",
                self.export
            );
        }
        match h {
            Held::Memory { ptr, .. } => {
                self.ops.push(I::LocalGet(addr));
                if offset != 0 {
                    self.ops.push(I::I32Const(offset as i32));
                    self.ops.push(I::I32Add);
                }
                self.ops.push(I::LocalGet(ptr));
                self.ops
                    .push(I::I32Const(sizes.size(&ty).size_wasm32() as i32));
                self.ops.push(I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                });
                Encoding::Encoded(())
            }
            Held::Flat { locals, .. } => {
                match store(
                    resolve,
                    sizes,
                    &mut self.locals,
                    &ty,
                    addr,
                    offset,
                    &locals,
                    &mut self.ops,
                ) {
                    Encoding::Encoded(_) => Encoding::Encoded(()),
                    other => other.map(|_| ()),
                }
            }
            Held::Nothing => blocked!("`{}` stores a call with no result", self.export),
        }
    }

    /// `memory.copy(dst, src, size)`, from locals and a constant size.
    fn copy(&mut self, dst: u32, src: u32, size: u32) {
        use wasm_encoder::Instruction as I;
        self.ops.extend([
            I::LocalGet(dst),
            I::LocalGet(src),
            I::I32Const(size as i32),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
        ]);
    }

    /// **`[a, b, c]`**: the elements, one after another, in the region.
    fn make_list(&mut self, result: ValueId, items: &[ValueId], ty: &Type) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let Some(lt) = self.type_of(result, ty) else {
            refuse!(
                "a list whose component type nothing fixes",
                "`{}` builds a {ty:?}",
                self.export
            );
        };
        let Some(et) = self.element_of(lt) else {
            blocked!("`{}` builds a list of a type that is not one", self.export);
        };
        let (size, align) = self.layout(&et);
        let base = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::I32Const(0),
            I::I32Const(0),
            I::I32Const(align as i32),
            I::I32Const((size as usize * items.len()) as i32),
            I::Call(self.realloc_index),
            I::LocalSet(base),
        ]);
        for (i, item) in items.iter().enumerate() {
            match self.store_at(*item, et, base, (i as u64) * size as u64) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
        }
        let len = self.locals.fresh(ValType::I32);
        self.ops
            .extend([I::I32Const(items.len() as i32), I::LocalSet(len)]);
        self.held.insert(
            result,
            Held::Flat {
                ty: lt,
                locals: vec![base, len],
            },
        );
        Encoding::Encoded(())
    }

    /// Bind a loop's parameters and emit its body: the region's value is left
    /// in `held`.
    fn loop_body(&mut self, binds: &[(ValueId, Held)], body: &super::ir::Region) -> Encoding<()> {
        for (v, h) in binds {
            self.held.insert(*v, h.clone());
        }
        self.region(&body.instrs)
    }

    /// **A loop over a list** (ADR-0040): the body is emitted once, inside
    /// the loop, with its parameters' locals set for each element.
    #[allow(clippy::too_many_arguments)]
    fn each(
        &mut self,
        result: ValueId,
        kind: super::ir::EachKind,
        list: ValueId,
        seed: Option<ValueId>,
        params: &[ValueId],
        body: &super::ir::Region,
        ty: &Type,
    ) -> Encoding<()> {
        use super::ir::EachKind as K;
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (lt, ls) = match self.flat_locals(list) {
            Encoding::Encoded(x) => x,
            other => return other.map(|_| unreachable!()),
        };
        let Some(et) = self.element_of(lt) else {
            blocked!("`{}` loops over a value that is not a list", self.export);
        };
        let (esize, ealign) = self.layout(&et);
        let (ptr, len) = (ls[0], ls[1]);
        // `for x in xs` (ADR-0051): the body once per element, its value
        // discarded.
        if kind == K::For {
            let i = self.locals.fresh(ValType::I32);
            self.ops.extend([I::I32Const(0), I::LocalSet(i)]);
            self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
            self.ops
                .extend([I::LocalGet(i), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
            let at = self.element_address(ptr, i, esize);
            let item = Held::Memory { ty: et, ptr: at };
            if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
                self.loop_body(&[(params[0], item)], body)
            {
                return other;
            }
            self.ops.extend([
                I::LocalGet(i),
                I::I32Const(1),
                I::I32Add,
                I::LocalSet(i),
                I::Br(0),
                I::End,
                I::End,
            ]);
            self.held.insert(result, Held::Nothing);
            return Encoding::Encoded(());
        }
        let Some(rt) = self.type_of(result, ty) else {
            refuse!(
                "a list operation whose component type nothing fixes",
                "`{}`'s `{kind:?}` produces a {ty:?}",
                self.export
            );
        };
        let item_of = |this: &mut Self, i: u32| {
            let at = this.element_address(ptr, i, esize);
            Held::Memory { ty: et, ptr: at }
        };
        if kind == K::SortBy {
            return self.sort_by(result, rt, ptr, len, et, params, body);
        }
        if kind == K::GroupBy {
            return self.group_by(result, rt, ptr, len, et, params, body);
        }
        let i = self.locals.fresh(ValType::I32);
        self.ops.extend([I::I32Const(0), I::LocalSet(i)]);
        match kind {
            K::Map => {
                let Some(ut) = self.element_of(rt) else {
                    blocked!("`{}` maps into a type that is not a list", self.export);
                };
                let (usz, ualign) = self.layout(&ut);
                let out = self.alloc_array(len, usz, ualign);
                self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
                self.ops
                    .extend([I::LocalGet(i), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
                let item = item_of(self, i);
                if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
                    self.loop_body(&[(params[0], item)], body)
                {
                    return other;
                }
                let dst = self.element_address(out, i, usz);
                match self.store_at(body.value, ut, dst, 0) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.extend([
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(i),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                self.held.insert(
                    result,
                    Held::Flat {
                        ty: rt,
                        locals: vec![out, len],
                    },
                );
            }
            K::Filter => {
                let out = self.alloc_array(len, esize, ealign);
                let count = self.locals.fresh(ValType::I32);
                self.ops.extend([I::I32Const(0), I::LocalSet(count)]);
                self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
                self.ops
                    .extend([I::LocalGet(i), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
                let item = item_of(self, i);
                let Held::Memory { ptr: at, .. } = item else {
                    unreachable!("an element is in memory")
                };
                if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
                    self.loop_body(&[(params[0], item)], body)
                {
                    return other;
                }
                let keep = match self.flat_locals(body.value) {
                    Encoding::Encoded((_, c)) => c[0],
                    other => return other.map(|_| unreachable!()),
                };
                self.ops.extend([I::LocalGet(keep), I::If(Empty)]);
                let dst = self.element_address(out, count, esize);
                self.copy(dst, at, esize);
                self.ops.extend([
                    I::LocalGet(count),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(count),
                    I::End,
                ]);
                self.ops.extend([
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(i),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                self.held.insert(
                    result,
                    Held::Flat {
                        ty: rt,
                        locals: vec![out, count],
                    },
                );
            }
            K::Fold => {
                let Some(seed) = seed else {
                    blocked!("`{}`'s fold has no seed", self.export);
                };
                let holder = self.holder(rt);
                match self.move_into(seed, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
                self.ops
                    .extend([I::LocalGet(i), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
                let item = item_of(self, i);
                if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
                    self.loop_body(&[(params[0], holder.clone()), (params[1], item)], body)
                {
                    return other;
                }
                match self.move_into(body.value, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.extend([
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(i),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                self.held.insert(result, holder);
            }
            K::Any | K::All | K::Find => {
                // `any` stops at the first `true`, `all` at the first
                // `false`, `find` at the first `true`, keeping its address.
                let out = self.locals.fresh(ValType::I32);
                let initial = i32::from(kind == K::All);
                self.ops.extend([I::I32Const(initial), I::LocalSet(out)]);
                self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
                self.ops
                    .extend([I::LocalGet(i), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
                let item = item_of(self, i);
                let Held::Memory { ptr: at, .. } = item else {
                    unreachable!("an element is in memory")
                };
                if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
                    self.loop_body(&[(params[0], item)], body)
                {
                    return other;
                }
                let c = match self.flat_locals(body.value) {
                    Encoding::Encoded((_, c)) => c[0],
                    other => return other.map(|_| unreachable!()),
                };
                self.ops.push(I::LocalGet(c));
                if kind == K::All {
                    self.ops.push(I::I32Eqz);
                }
                self.ops.push(I::If(Empty));
                match kind {
                    K::Find => self.ops.extend([I::LocalGet(at), I::LocalSet(out)]),
                    K::Any => self.ops.extend([I::I32Const(1), I::LocalSet(out)]),
                    _ => self.ops.extend([I::I32Const(0), I::LocalSet(out)]),
                }
                // Out of the `if`, the loop and the block.
                self.ops.extend([I::Br(2), I::End]);
                self.ops.extend([
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(i),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                if kind == K::Find {
                    // `out` is the found element's address, or 0, which no
                    // allocation has.
                    let Some((_, _, offset)) = case_layout(
                        self.resolve,
                        self.sizes,
                        rt,
                        VariantCase::Builtin(BuiltinCase::Some),
                    ) else {
                        blocked!("`{}` finds into a type that is not an option", self.export);
                    };
                    let area = self.locals.fresh(ValType::I32);
                    allocate(self.sizes, &rt, self.realloc_index, area, &mut self.ops);
                    self.ops.extend([
                        I::LocalGet(area),
                        I::LocalGet(out),
                        I::I32Const(0),
                        I::I32Ne,
                        I::I32Store8(MemArg {
                            offset: 0,
                            align: 0,
                            memory_index: 0,
                        }),
                        I::LocalGet(out),
                        I::If(Empty),
                        I::LocalGet(area),
                        I::I32Const(offset as i32),
                        I::I32Add,
                        I::LocalGet(out),
                        I::I32Const(esize as i32),
                        I::MemoryCopy {
                            src_mem: 0,
                            dst_mem: 0,
                        },
                        I::End,
                    ]);
                    self.held.insert(result, Held::Memory { ty: rt, ptr: area });
                } else {
                    self.held.insert(
                        result,
                        Held::Flat {
                            ty: rt,
                            locals: vec![out],
                        },
                    );
                }
            }
            K::SortBy | K::GroupBy | K::For => unreachable!("encoded above"),
        }
        Encoding::Encoded(())
    }

    /// **`List.sort_by`**: the merge sort below, the comparison the
    /// function argument (ADR-0040 §5), emitted once, in the merge step.
    #[allow(clippy::too_many_arguments)]
    fn sort_by(
        &mut self,
        result: ValueId,
        rt: WitType,
        ptr: u32,
        len: u32,
        et: WitType,
        params: &[ValueId],
        body: &super::ir::Region,
    ) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let sorted = self.merge_sort(ptr, len, et, &mut |this, a, b, take| {
            if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) = this
                .loop_body(
                    &[
                        (params[0], Held::Memory { ty: et, ptr: a }),
                        (params[1], Held::Memory { ty: et, ptr: b }),
                    ],
                    body,
                )
            {
                return other;
            }
            let order = match this.flat_locals(body.value) {
                Encoding::Encoded((_, c)) => c[0],
                other => return other.map(|_| unreachable!()),
            };
            this.ops.extend([
                I::LocalGet(order),
                I::I64Const(0),
                I::I64LeS,
                I::LocalSet(take),
            ]);
            Encoding::Encoded(())
        });
        let sorted = match sorted {
            Encoding::Encoded(s) => s,
            other => return other.map(|_| unreachable!()),
        };
        self.held.insert(
            result,
            Held::Flat {
                ty: rt,
                locals: vec![sorted, len],
            },
        );
        Encoding::Encoded(())
    }

    /// **A stable merge sort**, bottom-up, between two buffers (ADR-0040
    /// §5), into a new array: its address, in an `i32`. `take_first(a, b,
    /// take)`, given the addresses of two competing elements, `a` from the
    /// earlier run, sets the `i32` local `take` to 1 when `a` goes first. It
    /// is emitted once, in the merge step.
    fn merge_sort(
        &mut self,
        ptr: u32,
        len: u32,
        et: WitType,
        take_first: &mut dyn FnMut(&mut Self, u32, u32, u32) -> Encoding<()>,
    ) -> Encoding<u32> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (esize, ealign) = self.layout(&et);
        let (src, dst) = (
            self.alloc_array(len, esize, ealign),
            self.alloc_array(len, esize, ealign),
        );
        let bytes = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::LocalGet(len),
            I::I32Const(esize as i32),
            I::I32Mul,
            I::LocalSet(bytes),
            I::LocalGet(src),
            I::LocalGet(ptr),
            I::LocalGet(bytes),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
        ]);
        let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
        let (width, lo, mid, hi, i, j, k, take, swap) = (
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
        );
        let min = |a: u32, b: u32| {
            [
                I::LocalGet(a),
                I::LocalGet(b),
                I::LocalGet(a),
                I::LocalGet(b),
                I::I32LtU,
                I::Select,
            ]
        };
        self.ops.extend([I::I32Const(1), I::LocalSet(width)]);
        // while width < len
        self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
        self.ops
            .extend([I::LocalGet(width), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
        self.ops.extend([I::I32Const(0), I::LocalSet(lo)]);
        //   while lo < len
        self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
        self.ops
            .extend([I::LocalGet(lo), I::LocalGet(len), I::I32GeU, I::BrIf(1)]);
        let sum = self.locals.fresh(ValType::I32);
        self.ops.extend([
            I::LocalGet(lo),
            I::LocalGet(width),
            I::I32Add,
            I::LocalSet(sum),
        ]);
        self.ops.extend(min(sum, len));
        self.ops.push(I::LocalSet(mid));
        self.ops.extend([
            I::LocalGet(lo),
            I::LocalGet(width),
            I::I32Const(1),
            I::I32Shl,
            I::I32Add,
            I::LocalSet(sum),
        ]);
        self.ops.extend(min(sum, len));
        self.ops.push(I::LocalSet(hi));
        self.ops.extend([
            I::LocalGet(lo),
            I::LocalSet(i),
            I::LocalGet(mid),
            I::LocalSet(j),
            I::LocalGet(lo),
            I::LocalSet(k),
        ]);
        //     while k < hi
        self.ops.extend([I::Block(Empty), I::Loop(Empty)]);
        self.ops
            .extend([I::LocalGet(k), I::LocalGet(hi), I::I32GeU, I::BrIf(1)]);
        // take = j >= hi ? 1 : (i >= mid ? 0 : take_first(src[i], src[j]))
        self.ops.extend([
            I::LocalGet(j),
            I::LocalGet(hi),
            I::I32GeU,
            I::If(Empty),
            I::I32Const(1),
            I::LocalSet(take),
            I::Else,
            I::LocalGet(i),
            I::LocalGet(mid),
            I::I32GeU,
            I::If(Empty),
            I::I32Const(0),
            I::LocalSet(take),
            I::Else,
        ]);
        let a = self.element_address(src, i, esize);
        let b = self.element_address(src, j, esize);
        if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) =
            take_first(self, a, b, take)
        {
            return other.map(|_| unreachable!());
        }
        self.ops.extend([I::End, I::End]);
        let to = self.element_address(dst, k, esize);
        self.ops.extend([I::LocalGet(take), I::If(Empty)]);
        let from_i = self.element_address(src, i, esize);
        self.copy(to, from_i, esize);
        self.ops.extend([
            I::LocalGet(i),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(i),
            I::Else,
        ]);
        let from_j = self.element_address(src, j, esize);
        self.copy(to, from_j, esize);
        self.ops.extend([
            I::LocalGet(j),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(j),
            I::End,
        ]);
        self.ops.extend([
            I::LocalGet(k),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(k),
            I::Br(0),
            I::End,
            I::End,
        ]);
        //   lo += 2 * width
        self.ops.extend([
            I::LocalGet(lo),
            I::LocalGet(width),
            I::I32Const(1),
            I::I32Shl,
            I::I32Add,
            I::LocalSet(lo),
            I::Br(0),
            I::End,
            I::End,
        ]);
        // swap the buffers; width *= 2
        self.ops.extend([
            I::LocalGet(src),
            I::LocalSet(swap),
            I::LocalGet(dst),
            I::LocalSet(src),
            I::LocalGet(swap),
            I::LocalSet(dst),
            I::LocalGet(width),
            I::I32Const(1),
            I::I32Shl,
            I::LocalSet(width),
            I::Br(0),
            I::End,
            I::End,
        ]);
        Encoding::Encoded(src)
    }

    /// **Runs of equal keys**, as views: each group is a pointer into the
    /// list and a length, so nothing is copied (ADR-0040). The key is a
    /// `String`, compared by bytes with the previous element's.
    #[allow(clippy::too_many_arguments)]
    fn group_by(
        &mut self,
        result: ValueId,
        rt: WitType,
        ptr: u32,
        len: u32,
        et: WitType,
        params: &[ValueId],
        body: &super::ir::Region,
    ) -> Encoding<()> {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let (esize, _) = self.layout(&et);
        let Some(group_ty) = self.element_of(rt) else {
            blocked!("`{}` groups into a type that is not a list", self.export);
        };
        let (gsize, galign) = self.layout(&group_ty);
        let out = self.alloc_array(len, gsize, galign);
        let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
        let (i, groups, start, prev_p, prev_l, differs, at) = (
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
            fresh(self),
        );
        let eq = self.helpers.index(Helper::StrEq);
        // Close the run [start, end): its view at `out + groups * gsize`.
        let close = |this: &mut Self, end: Vec<I<'static>>| {
            let mut v = vec![
                I::LocalGet(out),
                I::LocalGet(groups),
                I::I32Const(gsize as i32),
                I::I32Mul,
                I::I32Add,
                I::LocalSet(at),
                I::LocalGet(at),
                I::LocalGet(ptr),
                I::LocalGet(start),
                I::I32Const(esize as i32),
                I::I32Mul,
                I::I32Add,
                I::I32Store(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }),
                I::LocalGet(at),
            ];
            v.extend(end);
            v.extend([
                I::LocalGet(start),
                I::I32Sub,
                I::I32Store(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }),
                I::LocalGet(groups),
                I::I32Const(1),
                I::I32Add,
                I::LocalSet(groups),
            ]);
            this.ops.extend(v);
        };
        self.ops.extend([
            I::I32Const(0),
            I::LocalSet(i),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(i),
            I::LocalGet(len),
            I::I32GeU,
            I::BrIf(1),
        ]);
        let item_at = self.element_address(ptr, i, esize);
        if let other @ (Encoding::Unsupported { .. } | Encoding::Blocked { .. }) = self.loop_body(
            &[(
                params[0],
                Held::Memory {
                    ty: et,
                    ptr: item_at,
                },
            )],
            body,
        ) {
            return other;
        }
        let key = match self.flat_locals(body.value) {
            Encoding::Encoded((t, ls)) if dealias(self.resolve, t) == WitType::String => ls,
            Encoding::Encoded(_) => {
                blocked!("`{}` groups by a key that is not a String", self.export)
            }
            other => return other.map(|_| unreachable!()),
        };
        // A run ends where the key differs from the previous element's.
        self.ops.extend([
            I::LocalGet(i),
            I::If(Empty),
            I::LocalGet(prev_p),
            I::LocalGet(prev_l),
            I::LocalGet(key[0]),
            I::LocalGet(key[1]),
            I::Call(eq),
            I::I32Eqz,
            I::LocalSet(differs),
            I::LocalGet(differs),
            I::If(Empty),
        ]);
        close(self, vec![I::LocalGet(i)]);
        self.ops.extend([
            I::LocalGet(i),
            I::LocalSet(start),
            I::End,
            I::End,
            I::LocalGet(key[0]),
            I::LocalSet(prev_p),
            I::LocalGet(key[1]),
            I::LocalSet(prev_l),
            I::LocalGet(i),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(i),
            I::Br(0),
            I::End,
            I::End,
            // The last run, when there was any element.
            I::LocalGet(len),
            I::If(Empty),
        ]);
        close(self, vec![I::LocalGet(len)]);
        self.ops.push(I::End);
        self.held.insert(
            result,
            Held::Flat {
                ty: rt,
                locals: vec![out, groups],
            },
        );
        Encoding::Encoded(())
    }

    /// **A standard-library operation on values** (ADR-0040).
    fn intrinsic(
        &mut self,
        result: ValueId,
        op: super::ir::Intrinsic,
        args: &[ValueId],
        ty: &Type,
    ) -> Encoding<()> {
        use super::ir::Intrinsic as N;
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let mut flats = Vec::new();
        for a in args {
            match self.flat_locals(*a) {
                Encoding::Encoded(x) => flats.push(x),
                other => return other.map(|_| unreachable!()),
            }
        }
        let Some(rt) = self.type_of(result, ty) else {
            refuse!(
                "an intrinsic whose component type nothing fixes",
                "`{}`'s `{op:?}` produces a {ty:?}",
                self.export
            );
        };
        // A helper's results, into fresh locals of their core types.
        let call = |this: &mut Self, h: Helper, inputs: &[u32]| -> Vec<u32> {
            let index = this.helpers.index(h);
            for l in inputs {
                this.ops.push(I::LocalGet(*l));
            }
            this.ops.push(I::Call(index));
            let (_, results) = h.signature();
            let outs: Vec<u32> = results.iter().map(|t| this.locals.fresh(*t)).collect();
            for l in outs.iter().rev() {
                this.ops.push(I::LocalSet(*l));
            }
            outs
        };
        let held = match op {
            // `f64.convert_i64_s`: IEEE 754's nearest value, ties to even.
            N::FloatFromInt => {
                let f = self.locals.fresh(ValType::F64);
                self.ops.extend([
                    I::LocalGet(flats[0].1[0]),
                    I::F64ConvertI64S,
                    I::LocalSet(f),
                ]);
                Held::Flat {
                    ty: rt,
                    locals: vec![f],
                }
            }
            N::ListLength => {
                let n = self.locals.fresh(ValType::I64);
                self.ops
                    .extend([I::LocalGet(flats[0].1[1]), I::I64ExtendI32U, I::LocalSet(n)]);
                Held::Flat {
                    ty: rt,
                    locals: vec![n],
                }
            }
            N::ListTake => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let out = self.clamped(flats[1].1[0], len);
                // A view: lists do not change.
                Held::Flat {
                    ty: rt,
                    locals: vec![ptr, out],
                }
            }
            // Views too (ADR-0055): the elements from the first kept one.
            N::ListDrop | N::ListSlice => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some(et) = self.element_of(flats[0].0) else {
                    blocked!("`{}` slices a value that is not a list", self.export);
                };
                let (esize, _) = self.layout(&et);
                let start = self.clamped(flats[1].1[0], len);
                let end = match op {
                    N::ListSlice => {
                        // max(clamped end, start): an end before the start is
                        // the start, and the slice is empty.
                        let end = self.clamped(flats[2].1[0], len);
                        self.ops.extend([
                            I::LocalGet(end),
                            I::LocalGet(start),
                            I::LocalGet(end),
                            I::LocalGet(start),
                            I::I32GtU,
                            I::Select,
                            I::LocalSet(end),
                        ]);
                        end
                    }
                    _ => len,
                };
                let from = self.element_address(ptr, start, esize);
                let count = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::LocalGet(end),
                    I::LocalGet(start),
                    I::I32Sub,
                    I::LocalSet(count),
                ]);
                Held::Flat {
                    ty: rt,
                    locals: vec![from, count],
                }
            }
            // A new list, each element its index from `start` (`Helper::Range`).
            N::ListRange => Held::Flat {
                ty: rt,
                locals: call(self, Helper::Range, &[flats[0].1[0], flats[1].1[0]]),
            },
            // A copy, the last element first, each element's bytes moved
            // whole: what an element points to is shared, and never changes.
            N::ListReverse => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some(et) = self.element_of(flats[0].0) else {
                    blocked!("`{}` reverses a value that is not a list", self.export);
                };
                let (esize, ealign) = self.layout(&et);
                let out = self.alloc_array(len, esize, ealign);
                let i = self.locals.fresh(ValType::I32);
                let size = esize as i32;
                self.ops.extend([
                    I::I32Const(0),
                    I::LocalSet(i),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(i),
                    I::LocalGet(len),
                    I::I32GeU,
                    I::BrIf(1),
                    // out[len - 1 - i] = items[i]
                    I::LocalGet(out),
                    I::LocalGet(len),
                    I::I32Const(1),
                    I::I32Sub,
                    I::LocalGet(i),
                    I::I32Sub,
                    I::I32Const(size),
                    I::I32Mul,
                    I::I32Add,
                    I::LocalGet(ptr),
                    I::LocalGet(i),
                    I::I32Const(size),
                    I::I32Mul,
                    I::I32Add,
                    I::I32Const(size),
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(i),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                Held::Flat {
                    ty: rt,
                    locals: vec![out, len],
                }
            }
            N::ListGet => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let index = flats[1].1[0];
                let Some(et) = self.element_of(flats[0].0) else {
                    blocked!("`{}` indexes a value that is not a list", self.export);
                };
                let (esize, _) = self.layout(&et);
                let Some((_, _, offset)) = case_layout(
                    self.resolve,
                    self.sizes,
                    rt,
                    VariantCase::Builtin(BuiltinCase::Some),
                ) else {
                    blocked!(
                        "`{}` indexes into a type that is not an option",
                        self.export
                    );
                };
                let area = self.locals.fresh(ValType::I32);
                allocate(self.sizes, &rt, self.realloc_index, area, &mut self.ops);
                let inside = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::LocalGet(index),
                    I::I64Const(0),
                    I::I64GeS,
                    I::LocalGet(index),
                    I::LocalGet(len),
                    I::I64ExtendI32U,
                    I::I64LtS,
                    I::I32And,
                    I::LocalSet(inside),
                    I::LocalGet(area),
                    I::LocalGet(inside),
                    I::I32Store8(MemArg {
                        offset: 0,
                        align: 0,
                        memory_index: 0,
                    }),
                    I::LocalGet(inside),
                    I::If(Empty),
                    I::LocalGet(area),
                    I::I32Const(offset as i32),
                    I::I32Add,
                    I::LocalGet(ptr),
                    I::LocalGet(index),
                    I::I32WrapI64,
                    I::I32Const(esize as i32),
                    I::I32Mul,
                    I::I32Add,
                    I::I32Const(esize as i32),
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                    I::End,
                ]);
                Held::Memory { ty: rt, ptr: area }
            }
            N::ListConcat => {
                let (pa, la) = (flats[0].1[0], flats[0].1[1]);
                let (pb, lb) = (flats[1].1[0], flats[1].1[1]);
                let Some(et) = self.element_of(flats[0].0) else {
                    blocked!("`{}` joins a value that is not a list", self.export);
                };
                let (esize, ealign) = self.layout(&et);
                let total = self.locals.fresh(ValType::I32);
                self.ops.extend([
                    I::LocalGet(la),
                    I::LocalGet(lb),
                    I::I32Add,
                    I::LocalSet(total),
                ]);
                let out = self.alloc_array(total, esize, ealign);
                let second = self.element_address(out, la, esize);
                self.ops.extend([
                    I::LocalGet(out),
                    I::LocalGet(pa),
                    I::LocalGet(la),
                    I::I32Const(esize as i32),
                    I::I32Mul,
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                    I::LocalGet(second),
                    I::LocalGet(pb),
                    I::LocalGet(lb),
                    I::I32Const(esize as i32),
                    I::I32Mul,
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                ]);
                Held::Flat {
                    ty: rt,
                    locals: vec![out, total],
                }
            }
            N::StrLength => Held::Flat {
                ty: rt,
                locals: call(self, Helper::Utf8Count, &flats[0].1),
            },
            // --- maps and sets (ADR-0057) ------------------------------------
            N::MapEmpty | N::SetEmpty => {
                let Some(et) = self.element_of(rt) else {
                    blocked!(
                        "`{}` makes an empty map of a type that is not one",
                        self.export
                    );
                };
                let (esize, ealign) = self.layout(&et);
                let zero = self.locals.fresh(ValType::I32);
                self.ops.extend([I::I32Const(0), I::LocalSet(zero)]);
                let out = self.alloc_array(zero, esize, ealign);
                Held::Flat {
                    ty: rt,
                    locals: vec![out, zero],
                }
            }
            N::MapSize | N::SetSize => {
                let n = self.locals.fresh(ValType::I64);
                self.ops
                    .extend([I::LocalGet(flats[0].1[1]), I::I64ExtendI32U, I::LocalSet(n)]);
                Held::Flat {
                    ty: rt,
                    locals: vec![n],
                }
            }
            N::MapContains | N::SetContains => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some((entry, key, _)) = self.entry_of(flats[0].0) else {
                    blocked!("`{}` looks into a value that is not a map", self.export);
                };
                let (esize, _) = self.layout(&entry);
                let (_, found) = match self.search(key, ptr, len, esize, &flats[1].1) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                Held::Flat {
                    ty: rt,
                    locals: vec![found],
                }
            }
            N::MapGet => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some((entry, key, Some((vt, voff)))) = self.entry_of(flats[0].0) else {
                    blocked!("`{}` looks into a value that is not a map", self.export);
                };
                let ((esize, _), (vsize, _)) = (self.layout(&entry), self.layout(&vt));
                let (at, found) = match self.search(key, ptr, len, esize, &flats[1].1) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                let Some((_, _, offset)) = case_layout(
                    self.resolve,
                    self.sizes,
                    rt,
                    VariantCase::Builtin(BuiltinCase::Some),
                ) else {
                    blocked!("`{}` answers a map's value in a non-option", self.export);
                };
                let area = self.locals.fresh(ValType::I32);
                allocate(self.sizes, &rt, self.realloc_index, area, &mut self.ops);
                self.ops.extend([
                    I::LocalGet(area),
                    I::LocalGet(found),
                    I::I32Store8(MemArg {
                        offset: 0,
                        align: 0,
                        memory_index: 0,
                    }),
                    I::LocalGet(found),
                    I::If(Empty),
                ]);
                let (payload, value) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                let from = self.element_address(ptr, at, esize);
                self.ops.extend([
                    I::LocalGet(area),
                    I::I32Const(offset as i32),
                    I::I32Add,
                    I::LocalSet(payload),
                    I::LocalGet(from),
                    I::I32Const(voff as i32),
                    I::I32Add,
                    I::LocalSet(value),
                ]);
                self.copy(payload, value, vsize);
                self.ops.push(I::End);
                Held::Memory { ty: rt, ptr: area }
            }
            // A new array: the entries before the key's place, the new entry,
            // then the rest, less the one it replaces.
            N::MapInsert | N::SetInsert | N::MapRemove | N::SetRemove => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some((entry, key, value)) = self.entry_of(flats[0].0) else {
                    blocked!("`{}` changes a value that is not a map", self.export);
                };
                let (esize, ealign) = self.layout(&entry);
                let (at, found) = match self.search(key, ptr, len, esize, &flats[1].1) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                let adds = matches!(op, N::MapInsert | N::SetInsert);
                let fresh = |this: &mut Self| this.locals.fresh(ValType::I32);
                let (count, head, skip, tail, gap) = (
                    fresh(self),
                    fresh(self),
                    fresh(self),
                    fresh(self),
                    fresh(self),
                );
                // count = len - found (+ 1 when adding); skip = at + found;
                // tail = len - skip; gap = at (+ 1 when adding).
                self.ops.extend([
                    I::LocalGet(len),
                    I::LocalGet(found),
                    I::I32Sub,
                    I::I32Const(i32::from(adds)),
                    I::I32Add,
                    I::LocalSet(count),
                    I::LocalGet(at),
                    I::LocalGet(found),
                    I::I32Add,
                    I::LocalSet(skip),
                    I::LocalGet(len),
                    I::LocalGet(skip),
                    I::I32Sub,
                    I::LocalSet(tail),
                    I::LocalGet(at),
                    I::I32Const(i32::from(adds)),
                    I::I32Add,
                    I::LocalSet(gap),
                    I::LocalGet(at),
                    I::I32Const(esize as i32),
                    I::I32Mul,
                    I::LocalSet(head),
                ]);
                let out = self.alloc_array(count, esize, ealign);
                self.ops.extend([
                    I::LocalGet(out),
                    I::LocalGet(ptr),
                    I::LocalGet(head),
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                ]);
                let from = self.element_address(ptr, skip, esize);
                let to = self.element_address(out, gap, esize);
                self.ops.extend([
                    I::LocalGet(to),
                    I::LocalGet(from),
                    I::LocalGet(tail),
                    I::I32Const(esize as i32),
                    I::I32Mul,
                    I::MemoryCopy {
                        src_mem: 0,
                        dst_mem: 0,
                    },
                ]);
                if adds {
                    let slot = self.element_address(out, at, esize);
                    match self.store_at(args[1], key, slot, 0) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                    if let Some((vt, voff)) = value {
                        match self.store_at(args[2], vt, slot, voff) {
                            Encoding::Encoded(()) => {}
                            other => return other,
                        }
                    }
                }
                Held::Flat {
                    ty: rt,
                    locals: vec![out, count],
                }
            }
            N::MapKeys | N::MapValues => {
                let (ptr, len) = (flats[0].1[0], flats[0].1[1]);
                let Some((entry, key, Some((vt, voff)))) = self.entry_of(flats[0].0) else {
                    blocked!("`{}` reads a value that is not a map", self.export);
                };
                let (field, at) = match op {
                    N::MapKeys => (key, 0),
                    _ => (vt, voff),
                };
                let ((esize, _), (fsize, falign)) = (self.layout(&entry), self.layout(&field));
                let out = self.alloc_array(len, fsize, falign);
                let (i, src) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                self.ops.extend([
                    I::I32Const(0),
                    I::LocalSet(i),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(i),
                    I::LocalGet(len),
                    I::I32GeU,
                    I::BrIf(1),
                ]);
                let to = self.element_address(out, i, fsize);
                let entry_at = self.element_address(ptr, i, esize);
                self.ops.extend([
                    I::LocalGet(entry_at),
                    I::I32Const(at as i32),
                    I::I32Add,
                    I::LocalSet(src),
                ]);
                self.copy(to, src, fsize);
                self.ops.extend(increment(i));
                self.ops.extend([I::Br(0), I::End, I::End]);
                Held::Flat {
                    ty: rt,
                    locals: vec![out, len],
                }
            }
            // A set is its list, ascending.
            N::SetToList => Held::Flat {
                ty: rt,
                locals: flats[0].1.clone(),
            },
            N::MapFromLists => {
                let ((kp, kl), (vp, vl)) = (
                    (flats[0].1[0], flats[0].1[1]),
                    (flats[1].1[0], flats[1].1[1]),
                );
                let Some((entry, key, Some((vt, voff)))) = self.entry_of(rt) else {
                    blocked!("`{}` builds a map of a type that is not one", self.export);
                };
                let ((esize, ealign), (ksize, _), (vsize, _)) =
                    (self.layout(&entry), self.layout(&key), self.layout(&vt));
                // Lists of two lengths stop the invocation.
                self.ops
                    .extend([I::LocalGet(kl), I::LocalGet(vl), I::I32Ne]);
                trap_if(
                    &mut self.ops,
                    trap_index(self.realloc_index, Cause::ListsOfTwoLengths),
                );
                let pairs = self.alloc_array(kl, esize, ealign);
                let (i, value_to) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                self.ops.extend([
                    I::I32Const(0),
                    I::LocalSet(i),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(i),
                    I::LocalGet(kl),
                    I::I32GeU,
                    I::BrIf(1),
                ]);
                let to = self.element_address(pairs, i, esize);
                let k_from = self.element_address(kp, i, ksize);
                self.copy(to, k_from, ksize);
                let v_from = self.element_address(vp, i, vsize);
                self.ops.extend([
                    I::LocalGet(to),
                    I::I32Const(voff as i32),
                    I::I32Add,
                    I::LocalSet(value_to),
                ]);
                self.copy(value_to, v_from, vsize);
                self.ops.extend(increment(i));
                self.ops.extend([I::Br(0), I::End, I::End]);
                let (out, count) = match self.sorted(pairs, kl, entry, key) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                Held::Flat {
                    ty: rt,
                    locals: vec![out, count],
                }
            }
            N::SetFromList => {
                let Some((entry, key, _)) = self.entry_of(rt) else {
                    blocked!("`{}` builds a set of a type that is not one", self.export);
                };
                let (out, count) = match self.sorted(flats[0].1[0], flats[0].1[1], entry, key) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                Held::Flat {
                    ty: rt,
                    locals: vec![out, count],
                }
            }
            N::SetUnion | N::SetIntersection | N::SetDifference => {
                let Some((entry, key, _)) = self.entry_of(rt) else {
                    blocked!("`{}` merges values that are not sets", self.export);
                };
                let a = (flats[0].1[0], flats[0].1[1]);
                let b = (flats[1].1[0], flats[1].1[1]);
                let (out, count) = match self.merged(op, entry, key, a, b) {
                    Encoding::Encoded(x) => x,
                    other => return other.map(|_| unreachable!()),
                };
                Held::Flat {
                    ty: rt,
                    locals: vec![out, count],
                }
            }
            // From outside (ADR-0057): sorted by key as it arrives, stably,
            // by the merge sort `List.sort_by` and `Map.from_lists` use, and a
            // key twice traps (ruling 0057-c, ADR-0259). Until ADR-0259 one
            // out of order trapped too.
            N::MapCheck | N::SetCheck => {
                let (given, len) = (flats[0].1[0], flats[0].1[1]);
                let Some((entry, key, _)) = self.entry_of(flats[0].0) else {
                    blocked!("`{}` checks a value that is not a map", self.export);
                };
                let (esize, _) = self.layout(&entry);
                let ptr = match self.merge_sort(given, len, entry, &mut |this, a, b, take| {
                    let o = match this.order_at(key, a, b) {
                        Encoding::Encoded(o) => o,
                        other => return other.map(|_| unreachable!()),
                    };
                    this.ops
                        .extend([I::LocalGet(o), I::I32Const(0), I::I32LeS, I::LocalSet(take)]);
                    Encoding::Encoded(())
                }) {
                    Encoding::Encoded(s) => s,
                    other => return other.map(|_| unreachable!()),
                };
                let (i, before) = (
                    self.locals.fresh(ValType::I32),
                    self.locals.fresh(ValType::I32),
                );
                self.ops.extend([
                    I::I32Const(1),
                    I::LocalSet(i),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(i),
                    I::LocalGet(len),
                    I::I32GeU,
                    I::BrIf(1),
                    I::LocalGet(i),
                    I::I32Const(1),
                    I::I32Sub,
                    I::LocalSet(before),
                ]);
                let a = self.element_address(ptr, before, esize);
                let b = self.element_address(ptr, i, esize);
                let o = match self.order_at(key, a, b) {
                    Encoding::Encoded(o) => o,
                    other => return other.map(|_| unreachable!()),
                };
                // Sorted, so a key twice is two neighbours of one key.
                self.ops.extend([I::LocalGet(o), I::I32Eqz]);
                trap_if(
                    &mut self.ops,
                    trap_index(self.realloc_index, Cause::RepeatedKey),
                );
                self.ops.extend(increment(i));
                self.ops.extend([I::Br(0), I::End, I::End]);
                Held::Flat {
                    ty: rt,
                    locals: vec![ptr, len],
                }
            }
            N::StrToLower | N::StrToUpper => {
                let case = match op {
                    N::StrToLower => Case::Lower,
                    _ => Case::Upper,
                };
                let Some(table) = self.literals.tables.get(&case).copied() else {
                    blocked!(
                        "`{}` maps case with no table in the data segment",
                        self.export
                    );
                };
                let mut inputs = vec![flats[0].1[0], flats[0].1[1]];
                for x in table {
                    let l = self.locals.fresh(ValType::I32);
                    self.ops.extend([I::I32Const(x as i32), I::LocalSet(l)]);
                    inputs.push(l);
                }
                Held::Flat {
                    ty: rt,
                    locals: call(self, Helper::CaseMap, &inputs),
                }
            }
            N::StrSlice => {
                let inputs = [flats[0].1[0], flats[0].1[1], flats[1].1[0], flats[2].1[0]];
                Held::Flat {
                    ty: rt,
                    locals: call(self, Helper::Slice, &inputs),
                }
            }
            N::StrCodepoints => Held::Flat {
                ty: rt,
                locals: call(self, Helper::Codepoints, &flats[0].1),
            },
            N::StrFromCodepoints => Held::Flat {
                ty: rt,
                locals: call(self, Helper::FromCodepoints, &flats[0].1),
            },
            N::StrStartsWith | N::StrEndsWith | N::StrContains => {
                let h = match op {
                    N::StrStartsWith => Helper::StartsWith,
                    N::StrEndsWith => Helper::EndsWith,
                    _ => Helper::Contains,
                };
                let inputs = [flats[0].1[0], flats[0].1[1], flats[1].1[0], flats[1].1[1]];
                Held::Flat {
                    ty: rt,
                    locals: call(self, h, &inputs),
                }
            }
            N::StrJoin => {
                let inputs = [flats[0].1[0], flats[0].1[1], flats[1].1[0], flats[1].1[1]];
                Held::Flat {
                    ty: rt,
                    locals: call(self, Helper::Join, &inputs),
                }
            }
            N::StrTrim => Held::Flat {
                ty: rt,
                locals: call(self, Helper::Trim, &flats[0].1),
            },
            N::StrToLowerAscii => Held::Flat {
                ty: rt,
                locals: call(self, Helper::LowerAscii, &flats[0].1),
            },
        };
        self.held.insert(result, held);
        Encoding::Encoded(())
    }

    /// **A field of a record**: an address inside its layout, or a slice of
    /// its flat values.
    fn project(&mut self, result: ValueId, of: ValueId, field: u32) -> Encoding<()> {
        let resolve = self.resolve;
        let Some(h) = self.held.get(&of).cloned() else {
            blocked!(
                "`{}` reads a field of {of:?} and nothing defines it",
                self.export
            );
        };
        // A box's field is its cell's (ADR-0202).
        let h = self.unboxed(h);
        let Some(t) = h.ty().copied() else {
            blocked!("`{}` reads a field of a call with no result", self.export);
        };
        let WitType::Id(id) = dealias(resolve, t) else {
            blocked!(
                "`{}` reads a field of a value that is not a record",
                self.export
            );
        };
        let TypeDefKind::Record(r) = &resolve.types[id].kind else {
            blocked!(
                "`{}` reads a field of a value that is not a record",
                self.export
            );
        };
        let Some(field_ty) = r.fields.get(field as usize).map(|f| f.ty) else {
            blocked!(
                "`{}` reads field {field} of a record with {} fields",
                self.export,
                r.fields.len()
            );
        };
        let out = match h {
            Held::Memory { ptr, .. } => {
                let offsets = self.sizes.field_offsets(r.fields.iter().map(|f| &f.ty));
                let offset = offsets[field as usize].0.size_wasm32() as u64;
                Held::Memory {
                    ty: field_ty,
                    ptr: self.address(ptr, offset),
                }
            }
            Held::Flat { locals, .. } => {
                // The fields' flat values, in field order: this one's are the
                // slice after every earlier field's.
                let mut skip = 0;
                for f in &r.fields[..field as usize] {
                    let Some(flats) = flat(resolve, &f.ty) else {
                        refuse!(
                            "a field of a record that does not flatten",
                            "`{}` reads a field past one that does not flatten",
                            self.export
                        );
                    };
                    skip += flats.len();
                }
                let Some(n) = flat(resolve, &field_ty).map(|f| f.len()) else {
                    refuse!(
                        "a field that does not flatten",
                        "`{}` reads a field that does not flatten",
                        self.export
                    );
                };
                Held::Flat {
                    ty: field_ty,
                    locals: locals[skip..skip + n].to_vec(),
                }
            }
            Held::Nothing => blocked!("`{}` reads a field of a call with no result", self.export),
        };
        self.held.insert(result, out);
        Encoding::Encoded(())
    }

    /// **`Some(x)`, `None`, `Ok(x)`, `Err(e)`**, written into the invocation
    /// region: the discriminant, then the payload at the offset `SizeAlign`
    /// gives. The type is the one its use fixes (`expected_types`).
    fn variant(
        &mut self,
        result: ValueId,
        case: BuiltinCase,
        payload: Option<ValueId>,
        ty: &Type,
    ) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(t) = self.type_of(result, ty) else {
            refuse!(
                "a variant whose component type nothing fixes",
                "`{}` builds `{case:?}`, and no use of it names a type in the world",
                self.export
            );
        };
        let Some((disc, payload_ty, offset)) =
            case_layout(resolve, sizes, t, VariantCase::Builtin(case))
        else {
            blocked!(
                "`{}` builds `{case:?}` where its use needs another type",
                self.export
            );
        };
        let area = self.locals.fresh(ValType::I32);
        allocate(sizes, &t, self.realloc_index, area, &mut self.ops);
        self.ops.push(I::LocalGet(area));
        self.ops.push(I::I32Const(disc as i32));
        self.ops.push(I::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        match (payload, payload_ty) {
            (None, None) => {}
            (Some(p), Some(pt)) => {
                let Some(h) = self.held.get(&p).cloned() else {
                    blocked!("`{}` wraps {p:?} and nothing defines it", self.export);
                };
                let Some(ht) = h.ty() else {
                    blocked!("`{}` wraps a call with no result", self.export);
                };
                if !same_type(resolve, ht, &pt) {
                    blocked!(
                        "`{}` wraps a value of another component type in `{case:?}`",
                        self.export
                    );
                }
                match h {
                    // The payload's layout is already in the region: copy it.
                    Held::Memory { ptr, .. } => {
                        self.ops.push(I::LocalGet(area));
                        if offset != 0 {
                            self.ops.push(I::I32Const(offset as i32));
                            self.ops.push(I::I32Add);
                        }
                        self.ops.push(I::LocalGet(ptr));
                        self.ops
                            .push(I::I32Const(sizes.size(&pt).size_wasm32() as i32));
                        self.ops.push(I::MemoryCopy {
                            src_mem: 0,
                            dst_mem: 0,
                        });
                    }
                    Held::Flat { locals, .. } => {
                        match store(
                            resolve,
                            sizes,
                            &mut self.locals,
                            &pt,
                            area,
                            offset,
                            &locals,
                            &mut self.ops,
                        ) {
                            Encoding::Encoded(_) => {}
                            other => return other.map(|_| unreachable!()),
                        }
                    }
                    Held::Nothing => {
                        blocked!("`{}` wraps a call with no result", self.export)
                    }
                }
            }
            _ => blocked!(
                "`{}` builds `{case:?}` with a payload its type does not have, or without \
                 one it does",
                self.export
            ),
        }
        self.held.insert(result, Held::Memory { ty: t, ptr: area });
        Encoding::Encoded(())
    }

    /// **A declared sum type's case, built** (ADR-0059), into the invocation
    /// region as the Canonical ABI lays a variant out: the discriminant, of
    /// the width its number of cases gives, then each field at the offset
    /// `SizeAlign` gives it within the payload.
    fn case(&mut self, result: ValueId, case: u32, fields: &[ValueId], ty: &Type) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        let Some(t) = self.type_of(result, ty) else {
            refuse!(
                "a sum type with no component type",
                "`{}` builds a {ty:?}, which has no component type here",
                self.export
            );
        };
        // Built in its cell where the type holds itself in place (ADR-0202).
        let (t, boxed) = match self.cell_of(t) {
            Some(cell) => (cell, Some(t)),
            None => (t, None),
        };
        let (Some((disc, payload, offset)), Some((_, tag))) = (
            case_layout(resolve, sizes, t, VariantCase::Declared(case)),
            variant_cases(resolve, t),
        ) else {
            blocked!("`{}` builds a case its type does not have", self.export);
        };
        let Some(places) = case_fields(resolve, sizes, payload, fields.len()) else {
            blocked!(
                "`{}` builds a case of {} fields its payload does not hold",
                self.export,
                fields.len()
            );
        };
        if places.len() != fields.len() {
            blocked!(
                "`{}` builds a case of {} fields its payload does not hold",
                self.export,
                fields.len()
            );
        }
        let area = self.locals.fresh(ValType::I32);
        allocate(sizes, &t, self.realloc_index, area, &mut self.ops);
        self.ops.push(I::LocalGet(area));
        self.ops.push(I::I32Const(disc as i32));
        self.ops.push(store_tag(tag, 0));
        for ((at, field_ty), v) in places.into_iter().zip(fields) {
            match self.store_at(*v, field_ty, area, offset + at) {
                Encoding::Encoded(()) => {}
                other => return other,
            }
        }
        self.held.insert(
            result,
            match boxed {
                Some(b) => Held::Flat {
                    ty: b,
                    locals: vec![area],
                },
                None => Held::Memory { ty: t, ptr: area },
            },
        );
        Encoding::Encoded(())
    }

    /// **Choose by a variant's case**: the discriminant, read from the
    /// scrutinee's layout, selects an arm; each arm binds its payload's fields
    /// and moves its value into one holder, checked against the match's
    /// component type. A scrutinee held flat, a parameter, is written to the
    /// region first (ADR-0059).
    fn matched(
        &mut self,
        result: ValueId,
        scrutinee: ValueId,
        arms: &[super::ir::MatchArm],
        ty: &Type,
    ) -> Encoding<()> {
        use wasm_encoder::Instruction as I;
        let (resolve, sizes) = (self.resolve, self.sizes);
        // The result's holder: locals for a value that flattens without
        // variant slots, an address for anything else. A match whose arms
        // are statements has none (ADR-0051).
        // A box is matched by its cell (ADR-0202).
        let scrutinee_held = self.held.get(&scrutinee).cloned().map(|h| self.unboxed(h));
        let (st, ptr, holder) = match scrutinee_held {
            Some(Held::Memory { ty: st, ptr }) => (st, ptr, self.match_holder(result, ty)),
            Some(Held::Flat { ty: st, locals }) => {
                let holder = self.match_holder(result, ty);
                // Written here, for this match: the value stays held flat
                // for every other use.
                let area = self.locals.fresh(ValType::I32);
                allocate(sizes, &st, self.realloc_index, area, &mut self.ops);
                match store(
                    resolve,
                    sizes,
                    &mut self.locals,
                    &st,
                    area,
                    0,
                    &locals,
                    &mut self.ops,
                ) {
                    Encoding::Encoded(_) => {}
                    other => return other.map(|_| unreachable!()),
                }
                (st, area, holder)
            }
            _ => blocked!("`{}` matches a value nothing defines", self.export),
        };
        let holder = match holder {
            Encoding::Encoded(h) => h,
            other => return other.map(|_| unreachable!()),
        };
        let Some((cases, tag)) = variant_cases(resolve, st) else {
            blocked!("`{}` matches a value that is not a variant", self.export);
        };
        // Each case taken by exactly one arm, so no case falls through.
        let mut owner: Vec<Option<usize>> = vec![None; cases.len()];
        for (k, arm) in arms.iter().enumerate() {
            for c in &arm.cases {
                let Some((d, ..)) = case_layout(resolve, sizes, st, *c) else {
                    blocked!(
                        "`{}` matches cases the scrutinee's type does not have",
                        self.export
                    );
                };
                if owner[d as usize].replace(k).is_some() {
                    blocked!("`{}` matches one case in two arms", self.export);
                }
            }
        }
        if owner.iter().any(Option::is_none) {
            blocked!("`{}` leaves a case with no arm", self.export);
        }
        match arms {
            // Two cases, an arm each: the discriminant is the condition, and
            // 1 selects the second-numbered case (`Some`, `Err`).
            [first, second] if cases.len() == 2 => {
                let (one, zero) = match owner[1] {
                    Some(0) => (first, second),
                    _ => (second, first),
                };
                self.ops.push(I::LocalGet(ptr));
                self.ops.push(load_tag(tag, 0));
                self.ops.push(I::If(wasm_encoder::BlockType::Empty));
                match self.arm(ptr, st, one, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.push(I::Else);
                match self.arm(ptr, st, zero, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                self.ops.push(I::End);
            }
            // Otherwise the discriminant once, then each arm but the last
            // under a test of its cases; the last takes what is left.
            _ => {
                let Some((last, rest)) = arms.split_last() else {
                    blocked!("`{}` matches with no arms", self.export);
                };
                let d = self.locals.fresh(ValType::I32);
                if !rest.is_empty() {
                    self.ops.push(I::LocalGet(ptr));
                    self.ops.push(load_tag(tag, 0));
                    self.ops.push(I::LocalSet(d));
                }
                for arm in rest {
                    for (j, c) in arm.cases.iter().enumerate() {
                        let (disc, ..) = case_layout(resolve, sizes, st, *c).expect("checked");
                        self.ops.push(I::LocalGet(d));
                        self.ops.push(I::I32Const(disc as i32));
                        self.ops.push(I::I32Eq);
                        if j > 0 {
                            self.ops.push(I::I32Or);
                        }
                    }
                    self.ops.push(I::If(wasm_encoder::BlockType::Empty));
                    match self.arm(ptr, st, arm, &holder) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                    self.ops.push(I::Else);
                }
                match self.arm(ptr, st, last, &holder) {
                    Encoding::Encoded(()) => {}
                    other => return other,
                }
                for _ in rest {
                    self.ops.push(I::End);
                }
            }
        }
        self.held.insert(result, holder);
        Encoding::Encoded(())
    }

    /// Where a match's value is put: [`Enc::holder`] for its component type,
    /// or nothing for a match whose arms are statements (ADR-0051).
    fn match_holder(&mut self, result: ValueId, ty: &Type) -> Encoding<Held> {
        match ty {
            Type::Unit => Encoding::Encoded(Held::Nothing),
            _ => match self.type_of(result, ty) {
                Some(rt) => Encoding::Encoded(self.holder(rt)),
                None => refuse!(
                    "a match whose component type nothing fixes",
                    "`{}`'s match is used where the world names no type",
                    self.export
                ),
            },
        }
    }

    /// One arm: bind its payload's fields, emit the region, move its value.
    fn arm(
        &mut self,
        ptr: u32,
        st: WitType,
        arm: &super::ir::MatchArm,
        holder: &Held,
    ) -> Encoding<()> {
        let (resolve, sizes) = (self.resolve, self.sizes);
        // An arm of one case binds its payload's fields; an arm of several
        // binds none.
        if let [case] = arm.cases.as_slice()
            && arm.bindings.iter().any(Option::is_some)
        {
            let Some((_, payload, offset)) = case_layout(resolve, sizes, st, *case) else {
                blocked!(
                    "`{}` matches a case its scrutinee does not have",
                    self.export
                );
            };
            let Some(places) = case_fields(resolve, sizes, payload, arm.bindings.len()) else {
                blocked!(
                    "`{}` binds fields its case's payload does not hold",
                    self.export
                );
            };
            for ((at, field_ty), b) in places.into_iter().zip(&arm.bindings) {
                if let Some(b) = b {
                    let addr = self.address(ptr, offset + at);
                    self.held.insert(
                        *b,
                        Held::Memory {
                            ty: field_ty,
                            ptr: addr,
                        },
                    );
                }
            }
        }
        if matches!(holder, Held::Nothing) {
            return self.region(&arm.body.instrs);
        }
        self.settle(&arm.body, holder)
    }
}

/// `i32.load8_u` at `offset` past the address on the stack.
fn byte_at(offset: u64) -> wasm_encoder::Instruction<'static> {
    wasm_encoder::Instruction::I32Load8U(MemArg {
        offset,
        align: 0,
        memory_index: 0,
    })
}

/// `i32.store8` at `offset` past the address below the value on the stack.
fn byte_to(offset: u64) -> wasm_encoder::Instruction<'static> {
    wasm_encoder::Instruction::I32Store8(MemArg {
        offset,
        align: 0,
        memory_index: 0,
    })
}

/// Decode the UTF-8 code point at the address in `at` into `cp`, its byte
/// length into `n`. The text is valid UTF-8: every string a component holds
/// came through the Canonical ABI, which lifts only valid UTF-8, or was made
/// here from valid pieces.
fn decode_utf8(at: u32, b0: u32, cp: u32, n: u32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    let continuation = |offset: u64, shift: i32| {
        let mut v = vec![
            I::LocalGet(at),
            byte_at(offset),
            I::I32Const(0x3F),
            I::I32And,
        ];
        if shift > 0 {
            v.extend([I::I32Const(shift), I::I32Shl]);
        }
        v
    };
    let mut v = vec![
        I::LocalGet(at),
        byte_at(0),
        I::LocalTee(b0),
        I::I32Const(0x80),
        I::I32LtU,
        I::If(Empty),
        I::LocalGet(b0),
        I::LocalSet(cp),
        I::I32Const(1),
        I::LocalSet(n),
        I::Else,
        I::LocalGet(b0),
        I::I32Const(0xE0),
        I::I32LtU,
        I::If(Empty),
        I::LocalGet(b0),
        I::I32Const(0x1F),
        I::I32And,
        I::I32Const(6),
        I::I32Shl,
    ];
    v.extend(continuation(1, 0));
    v.extend([
        I::I32Or,
        I::LocalSet(cp),
        I::I32Const(2),
        I::LocalSet(n),
        I::Else,
        I::LocalGet(b0),
        I::I32Const(0xF0),
        I::I32LtU,
        I::If(Empty),
        I::LocalGet(b0),
        I::I32Const(0x0F),
        I::I32And,
        I::I32Const(12),
        I::I32Shl,
    ]);
    v.extend(continuation(1, 6));
    v.push(I::I32Or);
    v.extend(continuation(2, 0));
    v.extend([
        I::I32Or,
        I::LocalSet(cp),
        I::I32Const(3),
        I::LocalSet(n),
        I::Else,
        I::LocalGet(b0),
        I::I32Const(0x07),
        I::I32And,
        I::I32Const(18),
        I::I32Shl,
    ]);
    v.extend(continuation(1, 12));
    v.push(I::I32Or);
    v.extend(continuation(2, 6));
    v.push(I::I32Or);
    v.extend(continuation(3, 0));
    v.extend([
        I::I32Or,
        I::LocalSet(cp),
        I::I32Const(4),
        I::LocalSet(n),
        I::End,
        I::End,
        I::End,
    ]);
    v
}

/// Push whether the code point in `cp` has Unicode's `White_Space` property:
/// the set Rust's `char::is_whitespace` tests.
fn white_space(cp: u32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    // U+0009..=U+000D
    let mut v = vec![
        I::LocalGet(cp),
        I::I32Const(0x09),
        I::I32Sub,
        I::I32Const(5),
        I::I32LtU,
    ];
    for single in [
        0x20, 0x85, 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000,
    ] {
        v.extend([I::LocalGet(cp), I::I32Const(single), I::I32Eq, I::I32Or]);
    }
    // U+2000..=U+200A
    v.extend([
        I::LocalGet(cp),
        I::I32Const(0x2000),
        I::I32Sub,
        I::I32Const(11),
        I::I32LtU,
        I::I32Or,
    ]);
    v
}

/// The bytes at `a + i` and `b + i` differ: push that as an `i32`.
fn bytes_differ(a: u32, b: u32, i: u32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::Instruction as I;
    vec![
        I::LocalGet(a),
        I::LocalGet(i),
        I::I32Add,
        byte_at(0),
        I::LocalGet(b),
        I::LocalGet(i),
        I::I32Add,
        byte_at(0),
        I::I32Ne,
    ]
}

/// `i += 1`.
/// Encode the code point in `c` as UTF-8 at the address in `at`, and move
/// `at` past it.
fn encode_utf8(at: u32, c: u32) -> Vec<wasm_encoder::Instruction<'static>> {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    let unit = |shift: i32, mask: i32, tag: i32, offset: u64| {
        let mut v = vec![I::LocalGet(at), I::LocalGet(c)];
        if shift > 0 {
            v.extend([I::I32Const(shift), I::I32ShrU]);
        }
        v.extend([
            I::I32Const(mask),
            I::I32And,
            I::I32Const(tag),
            I::I32Or,
            byte_to(offset),
        ]);
        v
    };
    let advance = |n: i32| [I::LocalGet(at), I::I32Const(n), I::I32Add, I::LocalSet(at)];
    let mut v = vec![
        I::LocalGet(c),
        I::I32Const(0x80),
        I::I32LtU,
        I::If(Empty),
        I::LocalGet(at),
        I::LocalGet(c),
        byte_to(0),
    ];
    v.extend(advance(1));
    v.extend([
        I::Else,
        I::LocalGet(c),
        I::I32Const(0x800),
        I::I32LtU,
        I::If(Empty),
    ]);
    v.extend(unit(6, 0x1F, 0xC0, 0));
    v.extend(unit(0, 0x3F, 0x80, 1));
    v.extend(advance(2));
    v.extend([
        I::Else,
        I::LocalGet(c),
        I::I32Const(0x10000),
        I::I32LtU,
        I::If(Empty),
    ]);
    v.extend(unit(12, 0x0F, 0xE0, 0));
    v.extend(unit(6, 0x3F, 0x80, 1));
    v.extend(unit(0, 0x3F, 0x80, 2));
    v.extend(advance(3));
    v.push(I::Else);
    v.extend(unit(18, 0x07, 0xF0, 0));
    v.extend(unit(12, 0x3F, 0x80, 1));
    v.extend(unit(6, 0x3F, 0x80, 2));
    v.extend(unit(0, 0x3F, 0x80, 3));
    v.extend(advance(4));
    v.extend([I::End, I::End, I::End]);
    v
}

fn increment(i: u32) -> [wasm_encoder::Instruction<'static>; 4] {
    use wasm_encoder::Instruction as I;
    [I::LocalGet(i), I::I32Const(1), I::I32Add, I::LocalSet(i)]
}

/// **A value of a type that contains itself, as its nodes** (ADR-0194).
///
/// `(root) -> (ptr, len)`. The nodes are in level order, and the node list is
/// its own queue: node `i` is a copy of a value, so its lists of the type
/// still point at its children, which are copied after the nodes so far, in
/// one copy per list, since a list holds its elements one after another.
/// Each list is then made a run of one array of indices, `1, 2, ..`: in
/// level order every node but the first is claimed once, in node order, so
/// node `i`'s children are the run that continues the runs before it. Both
/// arrays grow by doubling. Nothing here recurses, so a value of any depth
/// is encoded.
///
/// params: 0 root; locals: 1 cap, 2 nodes, 3 idx, 4 n, 5 i, 6 node, 7 src,
/// 8 k, 9 j, 10 tmp, 11 grown, 12 tag
fn encode_graph(
    layout: &GraphLayout,
    realloc: u32,
) -> (Vec<(u32, ValType)>, Vec<wasm_encoder::Instruction<'static>>) {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    let (root, cap, nodes, idx, n, i, node, src, k, j, tmp, grown, tag) =
        (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
    let size = layout.size as i32;
    let word = |offset: u64| MemArg {
        offset,
        align: 2,
        memory_index: 0,
    };
    let copy = I::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    };
    // `base + at * scale`
    let address = |base: u32, at: u32, scale: i32| {
        vec![
            I::LocalGet(base),
            I::LocalGet(at),
            I::I32Const(scale),
            I::I32Mul,
            I::I32Add,
        ]
    };
    let alloc = |count: u32, scale: i32, align: i32, into: u32| {
        vec![
            I::I32Const(0),
            I::I32Const(0),
            I::I32Const(align),
            I::LocalGet(count),
            I::I32Const(scale),
            I::I32Mul,
            I::Call(realloc),
            I::LocalSet(into),
        ]
    };
    let mut ops = vec![I::I32Const(8), I::LocalSet(cap)];
    ops.extend(alloc(cap, size, layout.align as i32, nodes));
    ops.extend(alloc(cap, 4, 4, idx));
    ops.extend([
        I::LocalGet(nodes),
        I::LocalGet(root),
        I::I32Const(size),
        copy.clone(),
        I::I32Const(1),
        I::LocalSet(n),
        I::I32Const(0),
        I::LocalSet(i),
        I::Block(Empty),
        I::Loop(Empty),
        I::LocalGet(i),
        I::LocalGet(n),
        I::I32GeU,
        I::BrIf(1),
    ]);
    ops.extend(address(nodes, i, size));
    ops.push(I::LocalSet(node));
    if let Some(t) = layout.tag {
        ops.extend([I::LocalGet(node), load_tag(t, 0), I::LocalSet(tag)]);
    }
    let byte = |offset: u64| MemArg {
        offset,
        align: 0,
        memory_index: 0,
    };
    // Room for `k` more nodes, and their indices: both grown to the larger
    // of twice as many and as many as needed, within what one region holds,
    // and `node` found again in the grown nodes.
    let grow = || {
        let mut v = vec![
            I::LocalGet(n),
            I::LocalGet(k),
            I::I32Add,
            I::LocalGet(cap),
            I::I32GtU,
            I::If(Empty),
            I::LocalGet(cap),
            I::I32Const(1),
            I::I32Shl,
            I::LocalTee(grown),
            I::LocalGet(n),
            I::LocalGet(k),
            I::I32Add,
            I::LocalTee(tmp),
            I::LocalGet(grown),
            I::LocalGet(tmp),
            I::I32GtU,
            I::Select,
            I::LocalTee(grown),
            I::I32Const(i32::MAX / size),
            I::I32GtU,
            I::If(Empty),
            I::Call(trap_index(realloc, Cause::TooLongForMemory)),
            I::Unreachable,
            I::End,
        ];
        v.extend(alloc(grown, size, layout.align as i32, tmp));
        v.extend([I::LocalGet(tmp), I::LocalGet(nodes), I::LocalGet(n)]);
        v.extend([I::I32Const(size), I::I32Mul, copy.clone()]);
        v.extend([I::LocalGet(tmp), I::LocalSet(nodes)]);
        v.extend(alloc(grown, 4, 4, tmp));
        v.extend([
            I::LocalGet(tmp),
            I::LocalGet(idx),
            I::LocalGet(n),
            I::I32Const(1),
            I::I32Sub,
            I::I32Const(4),
            I::I32Mul,
            copy.clone(),
            I::LocalGet(tmp),
            I::LocalSet(idx),
            I::LocalGet(grown),
            I::LocalSet(cap),
        ]);
        v.extend(address(nodes, i, size));
        v.extend([I::LocalSet(node), I::End]);
        v
    };
    // `k` children from a list at `offset` in the node, made a run of
    // indices: copied after the nodes so far, as one run of cells, or, a
    // list of boxes, each from its cell (ADR-0202).
    let list = |offset: u64, boxes: bool| {
        let mut body = vec![
            I::LocalGet(node),
            I::I32Load(word(offset)),
            I::LocalSet(src),
            I::LocalGet(node),
            I::I32Load(word(offset + 4)),
            I::LocalSet(k),
        ];
        body.extend(grow());
        if boxes {
            body.extend([
                I::I32Const(0),
                I::LocalSet(j),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(j),
                I::LocalGet(k),
                I::I32GeU,
                I::BrIf(1),
                I::LocalGet(nodes),
                I::LocalGet(n),
                I::LocalGet(j),
                I::I32Add,
                I::I32Const(size),
                I::I32Mul,
                I::I32Add,
                I::LocalGet(src),
                I::LocalGet(j),
                I::I32Const(4),
                I::I32Mul,
                I::I32Add,
                I::I32Load(word(0)),
                I::I32Const(size),
                copy.clone(),
            ]);
            body.extend(increment(j));
            body.extend([I::Br(0), I::End, I::End]);
        } else {
            body.extend(address(nodes, n, size));
            body.extend([
                I::LocalGet(src),
                I::LocalGet(k),
                I::I32Const(size),
                I::I32Mul,
                copy.clone(),
            ]);
        }
        // Their indices: n, n + 1, .., at idx[n - 1 ..].
        body.extend([
            I::I32Const(0),
            I::LocalSet(j),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(j),
            I::LocalGet(k),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(idx),
            I::LocalGet(n),
            I::I32Const(1),
            I::I32Sub,
            I::LocalGet(j),
            I::I32Add,
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            I::LocalGet(n),
            I::LocalGet(j),
            I::I32Add,
            I::I32Store(word(0)),
        ]);
        body.extend(increment(j));
        body.extend([
            I::Br(0),
            I::End,
            I::End,
            // The list, as its run of indices.
            I::LocalGet(node),
            I::LocalGet(idx),
            I::LocalGet(n),
            I::I32Const(1),
            I::I32Sub,
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            I::I32Store(word(offset)),
            I::LocalGet(n),
            I::LocalGet(k),
            I::I32Add,
            I::LocalSet(n),
        ]);
        body
    };
    // One child from a box at `offset`: its cell copied after the nodes so
    // far, and its index written where its address was (ADR-0202).
    let one = |offset: u64| {
        let mut body = vec![
            I::LocalGet(node),
            I::I32Load(word(offset)),
            I::LocalSet(src),
            I::I32Const(1),
            I::LocalSet(k),
        ];
        body.extend(grow());
        body.extend(address(nodes, n, size));
        body.extend([
            I::LocalGet(src),
            I::I32Const(size),
            copy.clone(),
            I::LocalGet(node),
            I::LocalGet(n),
            I::I32Store(word(offset)),
        ]);
        body.extend(increment(n));
        body
    };
    for (case, offset, kind) in &layout.slots {
        let body = match kind {
            SlotKind::Inline => list(*offset, false),
            SlotKind::Boxes => list(*offset, true),
            SlotKind::Box => one(*offset),
            // An option of a box holds a child where it is `some`.
            SlotKind::Option(payload) => {
                let mut b = vec![I::LocalGet(node), I::I32Load8U(byte(*offset)), I::If(Empty)];
                b.extend(one(offset + payload));
                b.push(I::End);
                b
            }
        };
        match case {
            Some(c) => {
                ops.extend([
                    I::LocalGet(tag),
                    I::I32Const(*c as i32),
                    I::I32Eq,
                    I::If(Empty),
                ]);
                ops.extend(body);
                ops.push(I::End);
            }
            None => ops.extend(body),
        }
    }
    ops.extend(increment(i));
    ops.extend([
        I::Br(0),
        I::End,
        I::End,
        I::LocalGet(nodes),
        I::LocalGet(n),
        I::End,
    ]);
    (vec![(12, ValType::I32)], ops)
}

/// **Nodes, checked, made in place into the value they encode** (ADR-0194).
///
/// `(ptr, len) -> root`. A node's lists of indices must each continue the
/// run of indices the lists before it hold, from 1, with each index after
/// its own node's, and the runs must end at the last node: each node but
/// the first then has one list holding it, from a node before it, so the
/// nodes are one tree, with no cycle and none shared or left over. Anything
/// else traps. Each list is then rewritten in place to point at its
/// children, which in level order lie one after another in the nodes
/// themselves: a value's list of the type is exactly that. Nothing is
/// copied, and nothing here recurses.
///
/// params: 0 nodes, 1 len; locals: 2 i, 3 next, 4 node, 5 ptr, 6 k, 7 j,
/// 8 tag
fn decode_graph(
    layout: &GraphLayout,
    realloc_index: u32,
) -> (Vec<(u32, ValType)>, Vec<wasm_encoder::Instruction<'static>>) {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    let (nodes, len, i, next, node, ptr, k, j, tag) = (0, 1, 2, 3, 4, 5, 6, 7, 8);
    let size = layout.size as i32;
    let word = |offset: u64| MemArg {
        offset,
        align: 2,
        memory_index: 0,
    };
    let trap_if = |mut cond: Vec<I<'static>>| {
        cond.extend([
            I::If(Empty),
            I::Call(trap_index(realloc_index, Cause::NotATree)),
            I::Unreachable,
            I::End,
        ]);
        cond
    };
    let mut ops = trap_if(vec![I::LocalGet(len), I::I32Eqz]);
    ops.extend([
        I::I32Const(1),
        I::LocalSet(next),
        I::I32Const(0),
        I::LocalSet(i),
        I::Block(Empty),
        I::Loop(Empty),
        I::LocalGet(i),
        I::LocalGet(len),
        I::I32GeU,
        I::BrIf(1),
        I::LocalGet(nodes),
        I::LocalGet(i),
        I::I32Const(size),
        I::I32Mul,
        I::I32Add,
        I::LocalSet(node),
    ]);
    if let Some(t) = layout.tag {
        ops.extend([I::LocalGet(node), load_tag(t, 0), I::LocalSet(tag)]);
    }
    let byte = |offset: u64| MemArg {
        offset,
        align: 0,
        memory_index: 0,
    };
    // A child's cell: `nodes + at * size`.
    let cell = |at: Vec<I<'static>>| {
        let mut v = vec![I::LocalGet(nodes)];
        v.extend(at);
        v.extend([I::I32Const(size), I::I32Mul, I::I32Add]);
        v
    };
    // `k` children's indices, a list at `offset` in the node, each
    // continuing the run. A list of cells is made to point at its children,
    // which in level order lie one after another; a list of boxes is made
    // its children's addresses, each where its index was (ADR-0202).
    let list = |offset: u64, boxes: bool| {
        let mut body = vec![
            I::LocalGet(node),
            I::I32Load(word(offset)),
            I::LocalSet(ptr),
            I::LocalGet(node),
            I::I32Load(word(offset + 4)),
            I::LocalSet(k),
        ];
        // A node's children come after it.
        body.extend(trap_if(vec![
            I::LocalGet(k),
            I::I32Const(0),
            I::I32Ne,
            I::LocalGet(next),
            I::LocalGet(i),
            I::I32LeU,
            I::I32And,
        ]));
        // Each index continues the run. One past the last node is caught
        // at the end, as a run that does not end there.
        body.extend([
            I::I32Const(0),
            I::LocalSet(j),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(j),
            I::LocalGet(k),
            I::I32GeU,
            I::BrIf(1),
        ]);
        body.extend(trap_if(vec![
            I::LocalGet(ptr),
            I::LocalGet(j),
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            I::I32Load(word(0)),
            I::LocalGet(next),
            I::LocalGet(j),
            I::I32Add,
            I::I32Ne,
        ]));
        if boxes {
            body.extend([
                I::LocalGet(ptr),
                I::LocalGet(j),
                I::I32Const(4),
                I::I32Mul,
                I::I32Add,
            ]);
            body.extend(cell(vec![I::LocalGet(next), I::LocalGet(j), I::I32Add]));
            body.push(I::I32Store(word(0)));
        }
        body.extend(increment(j));
        body.extend([I::Br(0), I::End, I::End]);
        if !boxes {
            // The list, as its children: in level order, where they lie.
            body.push(I::LocalGet(node));
            body.extend(cell(vec![I::LocalGet(next)]));
            body.push(I::I32Store(word(offset)));
        }
        body.extend([
            I::LocalGet(next),
            I::LocalGet(k),
            I::I32Add,
            I::LocalSet(next),
        ]);
        body
    };
    // One child's index, a box at `offset`: continuing the run, after its
    // own node, and made its child's address (ADR-0202).
    let one = |offset: u64| {
        let mut body = trap_if(vec![I::LocalGet(next), I::LocalGet(i), I::I32LeU]);
        body.extend(trap_if(vec![
            I::LocalGet(node),
            I::I32Load(word(offset)),
            I::LocalGet(next),
            I::I32Ne,
        ]));
        body.push(I::LocalGet(node));
        body.extend(cell(vec![I::LocalGet(next)]));
        body.push(I::I32Store(word(offset)));
        body.extend(increment(next));
        body
    };
    for (case, offset, kind) in &layout.slots {
        let body = match kind {
            SlotKind::Inline => list(*offset, false),
            SlotKind::Boxes => list(*offset, true),
            SlotKind::Box => one(*offset),
            SlotKind::Option(payload) => {
                let mut b = vec![I::LocalGet(node), I::I32Load8U(byte(*offset)), I::If(Empty)];
                b.extend(one(offset + payload));
                b.push(I::End);
                b
            }
        };
        match case {
            Some(c) => {
                ops.extend([
                    I::LocalGet(tag),
                    I::I32Const(*c as i32),
                    I::I32Eq,
                    I::If(Empty),
                ]);
                ops.extend(body);
                ops.push(I::End);
            }
            None => ops.extend(body),
        }
    }
    ops.extend(increment(i));
    ops.extend([I::Br(0), I::End, I::End]);
    // Every node but the first is some list's.
    ops.extend(trap_if(vec![I::LocalGet(next), I::LocalGet(len), I::I32Ne]));
    ops.extend([I::LocalGet(nodes), I::End]);
    (vec![(7, ValType::I32)], ops)
}

/// The body of each string helper after the first three. Returns the extra
/// locals and the instructions; the caller wraps them in a `Function`.
fn string_helper(
    h: Helper,
    realloc_index: u32,
) -> (Vec<(u32, ValType)>, Vec<wasm_encoder::Instruction<'static>>) {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    let alloc = |align: i32, size: Vec<I<'static>>, into: u32| {
        let mut v = vec![I::I32Const(0), I::I32Const(0), I::I32Const(align)];
        v.extend(size);
        v.extend([I::Call(realloc_index), I::LocalSet(into)]);
        v
    };
    // `while i < n { body; i += 1 }`, `i` starting at 0.
    let each = |i: u32, n: u32, body: Vec<I<'static>>| {
        let mut v = vec![
            I::I32Const(0),
            I::LocalSet(i),
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(i),
            I::LocalGet(n),
            I::I32GeU,
            I::BrIf(1),
        ];
        v.extend(body);
        v.extend(increment(i));
        v.extend([I::Br(0), I::End, I::End]);
        v
    };
    match h {
        // params 0 p, 1 l; locals 2 i, 3 n (i64)
        Helper::Utf8Count => {
            let mut ops = each(
                2,
                1,
                vec![
                    I::LocalGet(0),
                    I::LocalGet(2),
                    I::I32Add,
                    byte_at(0),
                    I::I32Const(0xC0),
                    I::I32And,
                    I::I32Const(0x80),
                    I::I32Ne,
                    I::If(Empty),
                    I::LocalGet(3),
                    I::I64Const(1),
                    I::I64Add,
                    I::LocalSet(3),
                    I::End,
                ],
            );
            ops.extend([I::LocalGet(3), I::End]);
            (vec![(1, ValType::I32), (1, ValType::I64)], ops)
        }
        // params 0 p, 1 l; locals 2 i, 3 k, 4 out, 5 count, 6 b0, 7 cp, 8 n, 9 at
        Helper::Codepoints => {
            let mut ops = each(
                2,
                1,
                vec![
                    I::LocalGet(0),
                    I::LocalGet(2),
                    I::I32Add,
                    byte_at(0),
                    I::I32Const(0xC0),
                    I::I32And,
                    I::I32Const(0x80),
                    I::I32Ne,
                    I::LocalGet(5),
                    I::I32Add,
                    I::LocalSet(5),
                ],
            );
            ops.extend(alloc(8, vec![I::LocalGet(5), I::I32Const(8), I::I32Mul], 4));
            ops.extend([
                I::I32Const(0),
                I::LocalSet(2),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(2),
                I::LocalGet(1),
                I::I32GeU,
                I::BrIf(1),
                I::LocalGet(0),
                I::LocalGet(2),
                I::I32Add,
                I::LocalSet(9),
            ]);
            ops.extend(decode_utf8(9, 6, 7, 8));
            ops.extend([
                I::LocalGet(4),
                I::LocalGet(3),
                I::I32Const(8),
                I::I32Mul,
                I::I32Add,
                I::LocalGet(7),
                I::I64ExtendI32U,
                I::I64Store(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }),
                I::LocalGet(2),
                I::LocalGet(8),
                I::I32Add,
                I::LocalSet(2),
            ]);
            ops.extend(increment(3));
            ops.extend([
                I::Br(0),
                I::End,
                I::End,
                I::LocalGet(4),
                I::LocalGet(5),
                I::End,
            ]);
            (vec![(8, ValType::I32)], ops)
        }
        // params 0 start, 1 end (i64); locals 2 count, 3 out, 4 i (i32)
        Helper::Range => {
            let mut ops = vec![
                // A range of more than 2^28 - 1 elements traps: its bytes are
                // no `i32`, and a length that wrapped would be a shorter list.
                // Unsigned: past `end > start`, the difference's bits are the
                // count even where the subtraction wrapped (`MIN` to `MAX`).
                I::LocalGet(1),
                I::LocalGet(0),
                I::I64GtS,
                I::LocalGet(1),
                I::LocalGet(0),
                I::I64Sub,
                I::I64Const(0x0fff_ffff),
                I::I64GtU,
                I::I32And,
                I::If(Empty),
                I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),
                I::Unreachable,
                I::End,
                // count = end > start ? end - start : 0
                I::LocalGet(1),
                I::LocalGet(0),
                I::I64Sub,
                I::I32WrapI64,
                I::I32Const(0),
                I::LocalGet(1),
                I::LocalGet(0),
                I::I64GtS,
                I::Select,
                I::LocalSet(2),
            ];
            ops.extend(alloc(8, vec![I::LocalGet(2), I::I32Const(8), I::I32Mul], 3));
            ops.extend(each(
                4,
                2,
                vec![
                    I::LocalGet(3),
                    I::LocalGet(4),
                    I::I32Const(8),
                    I::I32Mul,
                    I::I32Add,
                    I::LocalGet(0),
                    I::LocalGet(4),
                    I::I64ExtendI32U,
                    I::I64Add,
                    I::I64Store(MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }),
                ],
            ));
            ops.extend([I::LocalGet(3), I::LocalGet(2), I::End]);
            (vec![(3, ValType::I32)], ops)
        }
        // params 0 lp, 1 ll; locals 2 i, 3 bytes, 4 c, 5 out, 6 cur; 7 cp (i64)
        Helper::FromCodepoints => {
            let point = |ops: &mut Vec<I<'static>>| {
                ops.extend([
                    I::LocalGet(0),
                    I::LocalGet(2),
                    I::I32Const(8),
                    I::I32Mul,
                    I::I32Add,
                    I::I64Load(MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }),
                    I::LocalSet(7),
                ]);
            };
            // Pass 1: every value a scalar value, and the byte count.
            let mut first = Vec::new();
            point(&mut first);
            first.extend([
                I::LocalGet(7),
                I::I64Const(0),
                I::I64LtS,
                I::LocalGet(7),
                I::I64Const(0x10FFFF),
                I::I64GtS,
                I::I32Or,
                I::LocalGet(7),
                I::I64Const(0xD800),
                I::I64GeS,
                I::LocalGet(7),
                I::I64Const(0xDFFF),
                I::I64LeS,
                I::I32And,
                I::I32Or,
                I::If(Empty),
                I::Call(trap_index(realloc_index, Cause::NotAScalar)),
                I::Unreachable,
                I::End,
                I::LocalGet(7),
                I::I32WrapI64,
                I::LocalSet(4),
                // bytes += 1 + (c >= 0x80) + (c >= 0x800) + (c >= 0x10000)
                I::LocalGet(3),
                I::I32Const(1),
                I::I32Add,
                I::LocalGet(4),
                I::I32Const(0x80),
                I::I32GeU,
                I::I32Add,
                I::LocalGet(4),
                I::I32Const(0x800),
                I::I32GeU,
                I::I32Add,
                I::LocalGet(4),
                I::I32Const(0x10000),
                I::I32GeU,
                I::I32Add,
                I::LocalSet(3),
            ]);
            let mut ops = each(2, 1, first);
            ops.extend(alloc(1, vec![I::LocalGet(3)], 5));
            ops.extend([I::LocalGet(5), I::LocalSet(6)]);
            // Pass 2: encode.
            let mut second = Vec::new();
            point(&mut second);
            let unit = |shift: i32, mask: i32, tag: i32, offset: u64| {
                let mut v = vec![I::LocalGet(6), I::LocalGet(4)];
                if shift > 0 {
                    v.extend([I::I32Const(shift), I::I32ShrU]);
                }
                v.extend([
                    I::I32Const(mask),
                    I::I32And,
                    I::I32Const(tag),
                    I::I32Or,
                    byte_to(offset),
                ]);
                v
            };
            second.extend([I::LocalGet(7), I::I32WrapI64, I::LocalSet(4)]);
            second.extend([
                I::LocalGet(4),
                I::I32Const(0x80),
                I::I32LtU,
                I::If(Empty),
                I::LocalGet(6),
                I::LocalGet(4),
                byte_to(0),
                I::LocalGet(6),
                I::I32Const(1),
                I::I32Add,
                I::LocalSet(6),
                I::Else,
                I::LocalGet(4),
                I::I32Const(0x800),
                I::I32LtU,
                I::If(Empty),
            ]);
            second.extend(unit(6, 0x1F, 0xC0, 0));
            second.extend(unit(0, 0x3F, 0x80, 1));
            second.extend([
                I::LocalGet(6),
                I::I32Const(2),
                I::I32Add,
                I::LocalSet(6),
                I::Else,
                I::LocalGet(4),
                I::I32Const(0x10000),
                I::I32LtU,
                I::If(Empty),
            ]);
            second.extend(unit(12, 0x0F, 0xE0, 0));
            second.extend(unit(6, 0x3F, 0x80, 1));
            second.extend(unit(0, 0x3F, 0x80, 2));
            second.extend([
                I::LocalGet(6),
                I::I32Const(3),
                I::I32Add,
                I::LocalSet(6),
                I::Else,
            ]);
            second.extend(unit(18, 0x07, 0xF0, 0));
            second.extend(unit(12, 0x3F, 0x80, 1));
            second.extend(unit(6, 0x3F, 0x80, 2));
            second.extend(unit(0, 0x3F, 0x80, 3));
            second.extend([
                I::LocalGet(6),
                I::I32Const(4),
                I::I32Add,
                I::LocalSet(6),
                I::End,
                I::End,
                I::End,
            ]);
            ops.extend(each(2, 1, second));
            ops.extend([I::LocalGet(5), I::LocalGet(3), I::End]);
            (vec![(5, ValType::I32), (1, ValType::I64)], ops)
        }
        // params 0 p1, 1 l1, 2 p2, 3 l2; local 4 i
        Helper::StartsWith => {
            let mut ops = vec![
                I::LocalGet(3),
                I::LocalGet(1),
                I::I32GtU,
                I::If(Empty),
                I::I32Const(0),
                I::Return,
                I::End,
            ];
            let mut body = bytes_differ(0, 2, 4);
            body.extend([I::If(Empty), I::I32Const(0), I::Return, I::End]);
            ops.extend(each(4, 3, body));
            ops.extend([I::I32Const(1), I::End]);
            (vec![(1, ValType::I32)], ops)
        }
        // params 0 p1, 1 l1, 2 p2, 3 l2; locals 4 i, 5 base
        Helper::EndsWith => {
            let mut ops = vec![
                I::LocalGet(3),
                I::LocalGet(1),
                I::I32GtU,
                I::If(Empty),
                I::I32Const(0),
                I::Return,
                I::End,
                I::LocalGet(0),
                I::LocalGet(1),
                I::I32Add,
                I::LocalGet(3),
                I::I32Sub,
                I::LocalSet(5),
            ];
            let mut body = bytes_differ(5, 2, 4);
            body.extend([I::If(Empty), I::I32Const(0), I::Return, I::End]);
            ops.extend(each(4, 3, body));
            ops.extend([I::I32Const(1), I::End]);
            (vec![(2, ValType::I32)], ops)
        }
        // params 0 p1, 1 l1, 2 p2, 3 l2; locals 4 start, 5 i, 6 last, 7 here
        Helper::Contains => {
            let mut ops = vec![
                I::LocalGet(3),
                I::LocalGet(1),
                I::I32GtU,
                I::If(Empty),
                I::I32Const(0),
                I::Return,
                I::End,
                I::LocalGet(1),
                I::LocalGet(3),
                I::I32Sub,
                I::LocalSet(6),
                I::I32Const(0),
                I::LocalSet(4),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(4),
                I::LocalGet(6),
                I::I32GtU,
                I::BrIf(1),
                I::LocalGet(0),
                I::LocalGet(4),
                I::I32Add,
                I::LocalSet(7),
                I::I32Const(0),
                I::LocalSet(5),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(5),
                I::LocalGet(3),
                I::I32GeU,
                I::If(Empty),
                I::I32Const(1),
                I::Return,
                I::End,
            ];
            ops.extend(bytes_differ(7, 2, 5));
            ops.push(I::BrIf(1));
            ops.extend(increment(5));
            ops.extend([I::Br(0), I::End, I::End]);
            ops.extend(increment(4));
            ops.extend([I::Br(0), I::End, I::End, I::I32Const(0), I::End]);
            (vec![(4, ValType::I32)], ops)
        }
        // params 0 lp, 1 ll, 2 sp, 3 sl; locals 4 i, 5 out, 6 cur, 7 ep, 8 el, 9 total; 10 wide (i64)
        Helper::Join => {
            let element = |field: u64, into: u32| {
                vec![
                    I::LocalGet(0),
                    I::LocalGet(4),
                    I::I32Const(8),
                    I::I32Mul,
                    I::I32Add,
                    I::I32Load(MemArg {
                        offset: field,
                        align: 2,
                        memory_index: 0,
                    }),
                    I::LocalSet(into),
                ]
            };
            // The total length, in 64 bits: the parts, and a separator between
            // each two.
            let mut sum = element(4, 8);
            sum.extend([
                I::LocalGet(10),
                I::LocalGet(8),
                I::I64ExtendI32U,
                I::I64Add,
                I::LocalSet(10),
            ]);
            let mut ops = each(4, 1, sum);
            ops.extend([
                I::LocalGet(1),
                I::If(Empty),
                I::LocalGet(10),
                I::LocalGet(3),
                I::I64ExtendI32U,
                I::LocalGet(1),
                I::I32Const(1),
                I::I32Sub,
                I::I64ExtendI32U,
                I::I64Mul,
                I::I64Add,
                I::LocalSet(10),
                I::End,
                I::LocalGet(10),
                I::I64Const(i32::MAX as i64),
                I::I64GtU,
                I::If(Empty),
                I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),
                I::Unreachable,
                I::End,
                I::LocalGet(10),
                I::I32WrapI64,
                I::LocalSet(9),
            ]);
            ops.extend(alloc(1, vec![I::LocalGet(9)], 5));
            ops.extend([I::LocalGet(5), I::LocalSet(6)]);
            let mut copy = vec![
                I::LocalGet(4),
                I::If(Empty),
                I::LocalGet(6),
                I::LocalGet(2),
                I::LocalGet(3),
                I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                },
                I::LocalGet(6),
                I::LocalGet(3),
                I::I32Add,
                I::LocalSet(6),
                I::End,
            ];
            copy.extend(element(0, 7));
            copy.extend(element(4, 8));
            copy.extend([
                I::LocalGet(6),
                I::LocalGet(7),
                I::LocalGet(8),
                I::MemoryCopy {
                    src_mem: 0,
                    dst_mem: 0,
                },
                I::LocalGet(6),
                I::LocalGet(8),
                I::I32Add,
                I::LocalSet(6),
            ]);
            ops.extend(each(4, 1, copy));
            ops.extend([I::LocalGet(5), I::LocalGet(9), I::End]);
            (vec![(6, ValType::I32), (1, ValType::I64)], ops)
        }
        // params 0 p, 1 l; locals 2 start, 3 end, 4 cp, 5 n, 6 b0, 7 at, 8 k
        Helper::Trim => {
            let mut ops = vec![
                I::I32Const(0),
                I::LocalSet(2),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(2),
                I::LocalGet(1),
                I::I32GeU,
                I::BrIf(1),
                I::LocalGet(0),
                I::LocalGet(2),
                I::I32Add,
                I::LocalSet(7),
            ];
            ops.extend(decode_utf8(7, 6, 4, 5));
            ops.extend(white_space(4));
            ops.extend([
                I::I32Eqz,
                I::BrIf(1),
                I::LocalGet(2),
                I::LocalGet(5),
                I::I32Add,
                I::LocalSet(2),
                I::Br(0),
                I::End,
                I::End,
                I::LocalGet(1),
                I::LocalSet(3),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(3),
                I::LocalGet(2),
                I::I32LeU,
                I::BrIf(1),
                // The last code point starts at the last byte that is not a
                // continuation byte.
                I::LocalGet(3),
                I::I32Const(1),
                I::I32Sub,
                I::LocalSet(8),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(0),
                I::LocalGet(8),
                I::I32Add,
                byte_at(0),
                I::I32Const(0xC0),
                I::I32And,
                I::I32Const(0x80),
                I::I32Ne,
                I::BrIf(1),
                I::LocalGet(8),
                I::I32Const(1),
                I::I32Sub,
                I::LocalSet(8),
                I::Br(0),
                I::End,
                I::End,
                I::LocalGet(0),
                I::LocalGet(8),
                I::I32Add,
                I::LocalSet(7),
            ]);
            ops.extend(decode_utf8(7, 6, 4, 5));
            ops.extend(white_space(4));
            ops.extend([
                I::I32Eqz,
                I::BrIf(1),
                I::LocalGet(8),
                I::LocalSet(3),
                I::Br(0),
                I::End,
                I::End,
                // A view of the same bytes.
                I::LocalGet(0),
                I::LocalGet(2),
                I::I32Add,
                I::LocalGet(3),
                I::LocalGet(2),
                I::I32Sub,
                I::End,
            ]);
            (vec![(7, ValType::I32)], ops)
        }
        // params 0 p, 1 l; locals 2 i, 3 out, 4 b
        Helper::LowerAscii => {
            let mut ops = alloc(1, vec![I::LocalGet(1)], 3);
            ops.extend(each(
                2,
                1,
                vec![
                    I::LocalGet(3),
                    I::LocalGet(2),
                    I::I32Add,
                    I::LocalGet(0),
                    I::LocalGet(2),
                    I::I32Add,
                    byte_at(0),
                    I::LocalTee(4),
                    // b + 32 for `A`..=`Z`: (b - 65) < 26 is 1 or 0, times 32.
                    I::LocalGet(4),
                    I::I32Const(65),
                    I::I32Sub,
                    I::I32Const(26),
                    I::I32LtU,
                    I::I32Const(5),
                    I::I32Shl,
                    I::I32Add,
                    byte_to(0),
                ],
            ));
            ops.extend([I::LocalGet(3), I::LocalGet(1), I::End]);
            (vec![(3, ValType::I32)], ops)
        }
        // params 0 p, 1 l, 2 start (i64), 3 end (i64); locals 4 i, 5 cp (i64),
        // 6 from, 7 to. Each bound below zero is zero, and one past the last
        // code point is the string's end.
        Helper::Slice => {
            let mut ops = Vec::new();
            for bound in [2, 3] {
                ops.extend([
                    I::LocalGet(bound),
                    I::I64Const(0),
                    I::LocalGet(bound),
                    I::I64Const(0),
                    I::I64GtS,
                    I::Select,
                    I::LocalSet(bound),
                ]);
            }
            ops.extend([
                I::LocalGet(1),
                I::LocalSet(6),
                I::LocalGet(1),
                I::LocalSet(7),
            ]);
            // At each code point's first byte, `cp` is its index.
            let at_bound = |bound: u32, into: u32| {
                vec![
                    I::LocalGet(5),
                    I::LocalGet(bound),
                    I::I64Eq,
                    I::If(Empty),
                    I::LocalGet(4),
                    I::LocalSet(into),
                    I::End,
                ]
            };
            let mut body = vec![
                I::LocalGet(0),
                I::LocalGet(4),
                I::I32Add,
                byte_at(0),
                I::I32Const(0xC0),
                I::I32And,
                I::I32Const(0x80),
                I::I32Ne,
                I::If(Empty),
            ];
            body.extend(at_bound(2, 6));
            body.extend(at_bound(3, 7));
            body.extend([
                I::LocalGet(5),
                I::I64Const(1),
                I::I64Add,
                I::LocalSet(5),
                I::End,
            ]);
            ops.extend(each(4, 1, body));
            // An end before the start is the start: "".
            ops.extend([
                I::LocalGet(7),
                I::LocalGet(6),
                I::LocalGet(7),
                I::LocalGet(6),
                I::I32GtU,
                I::Select,
                I::LocalSet(7),
                I::LocalGet(0),
                I::LocalGet(6),
                I::I32Add,
                I::LocalGet(7),
                I::LocalGet(6),
                I::I32Sub,
                I::End,
            ]);
            (
                vec![(1, ValType::I32), (1, ValType::I64), (2, ValType::I32)],
                ops,
            )
        }
        // params 0 p, 1 l, 2 ranges, 3 ranges', 4 multi, 5 multi'; locals
        // 6 i, 7 out, 8 at (the output's end), 9 b0, 10 cp, 11 n, 12 from,
        // 13 lo, 14 hi, 15 mid, 16 entry, 17 c. Each entry is 16 bytes, its
        // first word the code point it is found by.
        Helper::CaseMap => {
            let word = |offset: u64| {
                I::I32Load(MemArg {
                    offset,
                    align: 2,
                    memory_index: 0,
                })
            };
            // lo = how many of the `count` entries at `base` come before
            // cp: their first word below it, or, `inclusive`, not above it.
            let search = |base: u32, count: u32, inclusive: bool| {
                vec![
                    I::I32Const(0),
                    I::LocalSet(13),
                    I::LocalGet(count),
                    I::LocalSet(14),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(13),
                    I::LocalGet(14),
                    I::I32GeU,
                    I::BrIf(1),
                    I::LocalGet(13),
                    I::LocalGet(14),
                    I::I32Add,
                    I::I32Const(1),
                    I::I32ShrU,
                    I::LocalSet(15),
                    I::LocalGet(base),
                    I::LocalGet(15),
                    I::I32Const(4),
                    I::I32Shl,
                    I::I32Add,
                    word(0),
                    I::LocalGet(10),
                    if inclusive { I::I32LeU } else { I::I32LtU },
                    I::If(Empty),
                    I::LocalGet(15),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(13),
                    I::Else,
                    I::LocalGet(15),
                    I::LocalSet(14),
                    I::End,
                    I::Br(0),
                    I::End,
                    I::End,
                ]
            };
            let mut ops = alloc(
                1,
                vec![I::LocalGet(1), I::I32Const(case::GROWTH as i32), I::I32Mul],
                7,
            );
            ops.extend([
                I::LocalGet(7),
                I::LocalSet(8),
                I::I32Const(0),
                I::LocalSet(6),
                I::Block(Empty),
                I::Loop(Empty),
                I::LocalGet(6),
                I::LocalGet(1),
                I::I32GeU,
                I::BrIf(1),
                I::LocalGet(0),
                I::LocalGet(6),
                I::I32Add,
                I::LocalSet(12),
            ]);
            ops.extend(decode_utf8(12, 9, 10, 11));
            ops.extend([I::LocalGet(6), I::LocalGet(11), I::I32Add, I::LocalSet(6)]);
            // A code point that maps to more than one: up to three, the
            // first zero ending them. Then the next code point.
            ops.extend(search(4, 5, false));
            ops.extend([
                I::LocalGet(13),
                I::LocalGet(5),
                I::I32LtU,
                I::If(Empty),
                I::LocalGet(4),
                I::LocalGet(13),
                I::I32Const(4),
                I::I32Shl,
                I::I32Add,
                I::LocalTee(16),
                word(0),
                I::LocalGet(10),
                I::I32Eq,
                I::If(Empty),
                I::Block(Empty),
            ]);
            for offset in [4, 8, 12] {
                ops.extend([
                    I::LocalGet(16),
                    word(offset),
                    I::LocalTee(17),
                    I::I32Eqz,
                    I::BrIf(0),
                ]);
                ops.extend(encode_utf8(8, 17));
            }
            ops.extend([I::End, I::Br(2), I::End, I::End]);
            // Otherwise the last range starting at or before it, if it
            // covers it on its stride; else it is itself.
            ops.extend(search(2, 3, true));
            ops.extend([
                I::LocalGet(10),
                I::LocalSet(17),
                I::LocalGet(13),
                I::If(Empty),
                I::LocalGet(2),
                I::LocalGet(13),
                I::I32Const(1),
                I::I32Sub,
                I::I32Const(4),
                I::I32Shl,
                I::I32Add,
                I::LocalSet(16),
                I::LocalGet(10),
                I::LocalGet(16),
                word(4),
                I::I32LeU,
                I::If(Empty),
                I::LocalGet(10),
                I::LocalGet(16),
                word(0),
                I::I32Sub,
                I::LocalGet(16),
                word(12),
                I::I32RemU,
                I::I32Eqz,
                I::If(Empty),
                I::LocalGet(10),
                I::LocalGet(16),
                word(8),
                I::I32Add,
                I::LocalSet(17),
                I::End,
                I::End,
                I::End,
            ]);
            ops.extend(encode_utf8(8, 17));
            ops.extend([
                I::Br(0),
                I::End,
                I::End,
                I::LocalGet(7),
                I::LocalGet(8),
                I::LocalGet(7),
                I::I32Sub,
                I::End,
            ]);
            (vec![(12, ValType::I32)], ops)
        }
        Helper::StrEq
        | Helper::StrCmp
        | Helper::IntToString
        | Helper::Encode(_)
        | Helper::Decode(_) => {
            unreachable!("written in Helper::body")
        }
    }
}

type Ops = Vec<wasm_encoder::Instruction<'static>>;

/// **A trap when the `i32` on the stack is not zero**: a call of its cause's
/// function, `trap`, which is `unreachable` (ADR-0267).
fn trap_if(ops: &mut Ops, trap: u32) {
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::If(wasm_encoder::BlockType::Empty),
        I::Call(trap),
        I::Unreachable,
        I::End,
    ]);
}

/// After `r = a + b`: it overflowed iff both operands' signs differ from the
/// result's.
fn trap_on_add_overflow(ops: &mut Ops, a: u32, b: u32, r: u32, trap: u32) {
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::LocalGet(a),
        I::LocalGet(r),
        I::I64Xor,
        I::LocalGet(b),
        I::LocalGet(r),
        I::I64Xor,
        I::I64And,
        I::I64Const(0),
        I::I64LtS,
    ]);
    trap_if(ops, trap);
}

/// After `r = a - b`: it overflowed iff the operands' signs differ and the
/// result's differs from the left one's.
fn trap_on_sub_overflow(ops: &mut Ops, a: u32, b: u32, r: u32, trap: u32) {
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::LocalGet(a),
        I::LocalGet(b),
        I::I64Xor,
        I::LocalGet(a),
        I::LocalGet(r),
        I::I64Xor,
        I::I64And,
        I::I64Const(0),
        I::I64LtS,
    ]);
    trap_if(ops, trap);
}

/// After `r = a * b`: it is exact iff dividing back gives the other operand.
/// `r / a` traps by itself only for `MIN / -1`, which is `b == MIN`: an
/// overflow either way.
fn trap_on_mul_overflow(ops: &mut Ops, a: u32, b: u32, r: u32, trap: u32) {
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::LocalGet(a),
        I::I64Eqz,
        I::I32Eqz,
        I::If(wasm_encoder::BlockType::Empty),
        I::LocalGet(r),
        I::LocalGet(a),
        I::I64DivS,
        I::LocalGet(b),
        I::I64Ne,
    ]);
    trap_if(ops, trap);
    ops.push(I::End);
}

/// After `q = a / b`, truncated: a negative remainder moves the quotient one
/// step away from the divisor's sign. `-7 / 2` is -4, `-7 / -2` is 4.
fn euclidean_quotient(ops: &mut Ops, a: u32, b: u32, q: u32) {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::LocalGet(a),
        I::LocalGet(b),
        I::I64RemS,
        I::I64Const(0),
        I::I64LtS,
        I::If(Empty),
        I::LocalGet(b),
        I::I64Const(0),
        I::I64GtS,
        I::If(Empty),
        I::LocalGet(q),
        I::I64Const(1),
        I::I64Sub,
        I::LocalSet(q),
        I::Else,
        I::LocalGet(q),
        I::I64Const(1),
        I::I64Add,
        I::LocalSet(q),
        I::End,
        I::End,
    ]);
}

/// After `m = a % b`, truncated: into `[0, |b|)`. For `b == MIN`, `m - b` is
/// exact, because `m` is above MIN.
fn euclidean_remainder(ops: &mut Ops, b: u32, m: u32) {
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::Instruction as I;
    ops.extend([
        I::LocalGet(m),
        I::I64Const(0),
        I::I64LtS,
        I::If(Empty),
        I::LocalGet(b),
        I::I64Const(0),
        I::I64GtS,
        I::If(Empty),
        I::LocalGet(m),
        I::LocalGet(b),
        I::I64Add,
        I::LocalSet(m),
        I::Else,
        I::LocalGet(m),
        I::LocalGet(b),
        I::I64Sub,
        I::LocalSet(m),
        I::End,
        I::End,
    ]);
}

/// Before `0 - a`: `-MIN` does not fit.
fn trap_on_negation_overflow(ops: &mut Ops, a: u32, trap: u32) {
    use wasm_encoder::Instruction as I;
    ops.extend([I::LocalGet(a), I::I64Const(i64::MIN), I::I64Eq]);
    trap_if(ops, trap);
}

/// **Write flat values into a value's canonical layout**: the inverse of
/// [`load`], with the same offsets from `SizeAlign`. Returns how many flat
/// values it consumed.
#[allow(clippy::too_many_arguments)]
fn store(
    resolve: &Resolve,
    sizes: &SizeAlign,
    locals: &mut Locals,
    ty: &WitType,
    base: u32,
    offset: u64,
    flats: &[u32],
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<usize> {
    use wasm_encoder::Instruction as I;
    let mem = |align: u32, extra: u64| MemArg {
        offset: offset + extra,
        align,
        memory_index: 0,
    };
    let one = |ops: &mut Vec<I<'static>>, op: I<'static>| -> Encoding<usize> {
        let Some(v) = flats.first() else {
            blocked!("a store ran out of flat values");
        };
        ops.push(I::LocalGet(base));
        ops.push(I::LocalGet(*v));
        ops.push(op);
        Encoding::Encoded(1)
    };
    match dealias(resolve, *ty) {
        WitType::Bool | WitType::U8 | WitType::S8 => one(ops, I::I32Store8(mem(0, 0))),
        WitType::U16 | WitType::S16 => one(ops, I::I32Store16(mem(1, 0))),
        WitType::U32 | WitType::S32 | WitType::Char => one(ops, I::I32Store(mem(2, 0))),
        WitType::U64 | WitType::S64 => one(ops, I::I64Store(mem(3, 0))),
        WitType::F32 => one(ops, I::F32Store(mem(2, 0))),
        WitType::F64 => one(ops, I::F64Store(mem(3, 0))),
        WitType::String => pointer_and_length(base, offset, flats, ops),
        WitType::Id(id) => match &resolve.types[id].kind {
            TypeDefKind::List(_) => pointer_and_length(base, offset, flats, ops),
            // A map's entry (ADR-0057): laid out as a record of its types.
            TypeDefKind::Tuple(t) => {
                let mut used = 0;
                for (field_offset, field_ty) in sizes.field_offsets(t.types.iter()) {
                    let o = field_offset.size_wasm32() as u64;
                    match store(
                        resolve,
                        sizes,
                        locals,
                        field_ty,
                        base,
                        offset + o,
                        &flats[used..],
                        ops,
                    ) {
                        Encoding::Encoded(n) => used += n,
                        other => return other,
                    }
                }
                Encoding::Encoded(used)
            }
            TypeDefKind::Record(r) => {
                let mut used = 0;
                for (field_offset, field_ty) in sizes.field_offsets(r.fields.iter().map(|f| &f.ty))
                {
                    let o = field_offset.size_wasm32() as u64;
                    match store(
                        resolve,
                        sizes,
                        locals,
                        field_ty,
                        base,
                        offset + o,
                        &flats[used..],
                        ops,
                    ) {
                        Encoding::Encoded(n) => used += n,
                        other => return other,
                    }
                }
                Encoding::Encoded(used)
            }
            TypeDefKind::Option(_) | TypeDefKind::Result(_) | TypeDefKind::Variant(_) => {
                store_variant(resolve, sizes, locals, *ty, base, offset, flats, ops)
            }
            other => refuse!(
                "writing a value of this kind into memory",
                "{other:?} has no store here yet"
            ),
        },
        other => refuse!(
            "writing a value of this kind into memory",
            "{other:?} has no store here yet"
        ),
    }
}

/// A string or a list: its pointer and its length, each a 32-bit word.
fn pointer_and_length(
    base: u32,
    offset: u64,
    flats: &[u32],
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<usize> {
    use wasm_encoder::Instruction as I;
    let [ptr, len, ..] = flats else {
        blocked!("a pointer and length need two flat values");
    };
    for (v, extra) in [(ptr, 0), (len, 4)] {
        ops.push(I::LocalGet(base));
        ops.push(I::LocalGet(*v));
        ops.push(I::I32Store(MemArg {
            offset: offset + extra,
            align: 2,
            memory_index: 0,
        }));
    }
    Encoding::Encoded(2)
}

/// Push a held value's flat core values onto the stack.
fn push_flat_values(
    resolve: &Resolve,
    sizes: &SizeAlign,
    locals: &mut Locals,
    h: &Held,
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<()> {
    use wasm_encoder::Instruction as I;
    match h {
        Held::Flat { locals, .. } => {
            for l in locals {
                ops.push(I::LocalGet(*l));
            }
            Encoding::Encoded(())
        }
        Held::Memory { ty, ptr } => load(resolve, sizes, locals, ty, *ptr, 0, ops),
        Held::Nothing => blocked!("a call with no result was used as a value"),
    }
}

/// **Read a value's canonical layout into its flat values.** Offsets and sizes
/// from `SizeAlign`; the order is the one `push_flat` gives.
fn load(
    resolve: &Resolve,
    sizes: &SizeAlign,
    locals: &mut Locals,
    ty: &WitType,
    ptr: u32,
    offset: u64,
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<()> {
    use wasm_encoder::Instruction as I;
    let mem = |align: u32| MemArg {
        offset,
        align,
        memory_index: 0,
    };
    let at = |ops: &mut Vec<I<'static>>| ops.push(I::LocalGet(ptr));
    match dealias(resolve, *ty) {
        WitType::Bool | WitType::U8 => {
            at(ops);
            ops.push(I::I32Load8U(mem(0)));
        }
        WitType::S8 => {
            at(ops);
            ops.push(I::I32Load8S(mem(0)));
        }
        WitType::U16 => {
            at(ops);
            ops.push(I::I32Load16U(mem(1)));
        }
        WitType::S16 => {
            at(ops);
            ops.push(I::I32Load16S(mem(1)));
        }
        WitType::U32 | WitType::S32 | WitType::Char => {
            at(ops);
            ops.push(I::I32Load(mem(2)));
        }
        WitType::U64 | WitType::S64 => {
            at(ops);
            ops.push(I::I64Load(mem(3)));
        }
        WitType::F32 => {
            at(ops);
            ops.push(I::F32Load(mem(2)));
        }
        WitType::F64 => {
            at(ops);
            ops.push(I::F64Load(mem(3)));
        }
        // A string is a pointer and a length, each a 32-bit word.
        WitType::String => {
            at(ops);
            ops.push(I::I32Load(mem(2)));
            at(ops);
            ops.push(I::I32Load(MemArg {
                offset: offset + 4,
                align: 2,
                memory_index: 0,
            }));
        }
        WitType::Id(id) => match &resolve.types[id].kind {
            TypeDefKind::Record(r) => {
                let offsets = sizes.field_offsets(r.fields.iter().map(|f| &f.ty));
                for (field_offset, field_ty) in offsets {
                    let o = field_offset.size_wasm32() as u64;
                    match load(resolve, sizes, locals, field_ty, ptr, offset + o, ops) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                }
            }
            // A map's entry (ADR-0057), as a record of its types.
            TypeDefKind::Tuple(t) => {
                for (field_offset, field_ty) in sizes.field_offsets(t.types.iter()) {
                    let o = field_offset.size_wasm32() as u64;
                    match load(resolve, sizes, locals, field_ty, ptr, offset + o, ops) {
                        Encoding::Encoded(()) => {}
                        other => return other,
                    }
                }
            }
            TypeDefKind::List(_) => {
                at(ops);
                ops.push(I::I32Load(mem(2)));
                at(ops);
                ops.push(I::I32Load(MemArg {
                    offset: offset + 4,
                    align: 2,
                    memory_index: 0,
                }));
            }
            TypeDefKind::Option(_) | TypeDefKind::Result(_) | TypeDefKind::Variant(_) => {
                return load_variant(resolve, sizes, locals, *ty, ptr, offset, ops);
            }
            other => refuse!(
                "reading a value of this kind from memory",
                "{other:?} has no load here yet"
            ),
        },
        other => refuse!(
            "reading a value of this kind from memory",
            "{other:?} has no load here yet"
        ),
    }
    Encoding::Encoded(())
}

/// **A variant's flat values, written to memory** (ADR-0059): the
/// discriminant, then the payload of the case it names. Each of that case's
/// flat values is read back from the slot the variant's flattening joined it
/// into, as the Canonical ABI's `lift_flat_variant` reads it; a slot another
/// case shares may hold a wider type.
#[allow(clippy::too_many_arguments)]
fn store_variant(
    resolve: &Resolve,
    sizes: &SizeAlign,
    locals: &mut Locals,
    t: WitType,
    base: u32,
    offset: u64,
    flats: &[u32],
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<usize> {
    use wasm_encoder::Instruction as I;
    let (Some((cases, tag)), Some(all)) = (variant_cases(resolve, t), flat(resolve, &t)) else {
        refuse!(
            "writing a value of this kind into memory",
            "a variant too large to hold in locals"
        );
    };
    let joined = core_types(&all[1..]);
    let used = 1 + joined.len();
    let Some((disc, slots)) = flats.get(..used).and_then(<[u32]>::split_first) else {
        blocked!("a store ran out of flat values");
    };
    ops.push(I::LocalGet(base));
    ops.push(I::LocalGet(*disc));
    ops.push(store_tag(tag, offset));
    let payload_at = offset
        + sizes
            .payload_offset(tag, cases.iter().map(Option::as_ref))
            .size_wasm32() as u64;
    for (i, case) in cases.iter().enumerate() {
        let Some(pt) = case else { continue };
        let Some(own) = flat(resolve, pt) else {
            refuse!(
                "writing a value of this kind into memory",
                "a case too large to hold in locals"
            );
        };
        let mut body = Vec::new();
        let mut values = Vec::new();
        for (k, want) in core_types(&own).into_iter().enumerate() {
            let have = joined[k];
            if have == want {
                values.push(slots[k]);
                continue;
            }
            let l = locals.fresh(want);
            body.push(I::LocalGet(slots[k]));
            body.extend(from_joined(have, want));
            body.push(I::LocalSet(l));
            values.push(l);
        }
        match store(
            resolve, sizes, locals, pt, base, payload_at, &values, &mut body,
        ) {
            Encoding::Encoded(_) => {}
            other => return other,
        }
        ops.extend([
            I::LocalGet(*disc),
            I::I32Const(i as i32),
            I::I32Eq,
            I::If(wasm_encoder::BlockType::Empty),
        ]);
        ops.extend(body);
        ops.push(I::End);
    }
    Encoding::Encoded(used)
}

/// **A variant's layout, read into its flat values** (ADR-0059): the
/// discriminant, then each joined slot, holding the named case's flat values
/// widened into it and zero where that case has none, as the Canonical ABI's
/// `lower_flat_variant` writes them.
fn load_variant(
    resolve: &Resolve,
    sizes: &SizeAlign,
    locals: &mut Locals,
    t: WitType,
    ptr: u32,
    offset: u64,
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) -> Encoding<()> {
    use wasm_encoder::Instruction as I;
    let (Some((cases, tag)), Some(all)) = (variant_cases(resolve, t), flat(resolve, &t)) else {
        refuse!(
            "reading a value of this kind from memory",
            "a variant too large to hold in locals"
        );
    };
    let joined = core_types(&all[1..]);
    let disc = locals.fresh(ValType::I32);
    ops.push(I::LocalGet(ptr));
    ops.push(load_tag(tag, offset));
    ops.push(I::LocalSet(disc));
    let slots: Vec<u32> = joined
        .iter()
        .map(|t| {
            let l = locals.fresh(*t);
            ops.push(zero(*t));
            ops.push(I::LocalSet(l));
            l
        })
        .collect();
    let payload_at = offset
        + sizes
            .payload_offset(tag, cases.iter().map(Option::as_ref))
            .size_wasm32() as u64;
    for (i, case) in cases.iter().enumerate() {
        let Some(pt) = case else { continue };
        let Some(own) = flat(resolve, pt) else {
            refuse!(
                "reading a value of this kind from memory",
                "a case too large to hold in locals"
            );
        };
        let own = core_types(&own);
        ops.extend([
            I::LocalGet(disc),
            I::I32Const(i as i32),
            I::I32Eq,
            I::If(wasm_encoder::BlockType::Empty),
        ]);
        match load(resolve, sizes, locals, pt, ptr, payload_at, ops) {
            Encoding::Encoded(()) => {}
            other => return other,
        }
        for k in (0..own.len()).rev() {
            ops.extend(to_joined(own[k], joined[k]));
            ops.push(I::LocalSet(slots[k]));
        }
        ops.push(I::End);
    }
    ops.push(I::LocalGet(disc));
    for s in &slots {
        ops.push(I::LocalGet(*s));
    }
    Encoding::Encoded(())
}

/// Allocate a result area for `ty` from the region, into `into`.
fn allocate(
    sizes: &SizeAlign,
    ty: &WitType,
    realloc_index: u32,
    into: u32,
    ops: &mut Vec<wasm_encoder::Instruction<'static>>,
) {
    use wasm_encoder::Instruction as I;
    ops.push(I::I32Const(0));
    ops.push(I::I32Const(0));
    ops.push(I::I32Const(sizes.align(ty).align_wasm32() as i32));
    ops.push(I::I32Const(sizes.size(ty).size_wasm32() as i32));
    ops.push(I::Call(realloc_index));
    ops.push(I::LocalSet(into));
}

/// **`cabi_realloc`: the bump allocator over the invocation region.**
///
/// `(old_ptr, old_size, align, new_size) -> ptr`. Aligns the top of the
/// region, grows memory when the allocation does not fit, and traps rather
/// than returning an address it does not own. A reallocation copies what the
/// old block held.
fn realloc(realloc_index: u32) -> Function {
    use wasm_encoder::Instruction as I;
    // params: 0 old_ptr, 1 old_size, 2 align, 3 new_size; locals: 4 ptr, 5 end
    let mut f = Function::new([(2, ValType::I32)]);
    let ops = [
        // ptr = (top + align - 1) & -align
        I::GlobalGet(0),
        I::LocalGet(2),
        I::I32Add,
        I::I32Const(1),
        I::I32Sub,
        I::I32Const(0),
        I::LocalGet(2),
        I::I32Sub,
        I::I32And,
        I::LocalSet(4),
        // end = ptr + new_size, trapping on wrap-around
        I::LocalGet(4),
        I::LocalGet(3),
        I::I32Add,
        I::LocalTee(5),
        I::LocalGet(4),
        I::I32LtU,
        I::If(wasm_encoder::BlockType::Empty),
        I::Call(trap_index(realloc_index, Cause::MemoryExhausted)),
        I::Unreachable,
        I::End,
        // grow when end is past the current memory
        I::Block(wasm_encoder::BlockType::Empty),
        I::LocalGet(5),
        I::MemorySize(0),
        I::I32Const(16),
        I::I32Shl,
        I::I32LeU,
        I::BrIf(0),
        I::LocalGet(5),
        I::MemorySize(0),
        I::I32Const(16),
        I::I32Shl,
        I::I32Sub,
        I::I32Const(65535),
        I::I32Add,
        I::I32Const(16),
        I::I32ShrU,
        I::MemoryGrow(0),
        I::I32Const(-1),
        I::I32Ne,
        I::BrIf(0),
        I::Call(trap_index(realloc_index, Cause::MemoryExhausted)),
        I::Unreachable,
        I::End,
        I::LocalGet(5),
        I::GlobalSet(0),
        // copy the old block's contents on a reallocation
        I::Block(wasm_encoder::BlockType::Empty),
        I::LocalGet(0),
        I::I32Eqz,
        I::BrIf(0),
        I::LocalGet(4),
        I::LocalGet(0),
        I::LocalGet(1),
        I::LocalGet(3),
        I::LocalGet(1),
        I::LocalGet(3),
        I::I32LtU,
        I::Select,
        I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        },
        I::End,
        I::LocalGet(4),
        I::End,
    ];
    for op in &ops {
        f.instruction(op);
    }
    f
}

/// The export's post-return: the caller has lifted the result, so the whole
/// invocation region is reclaimed, down to the literals below it.
fn post_return(heap_base: i32) -> Function {
    use wasm_encoder::Instruction as I;
    let mut f = Function::new([]);
    f.instruction(&I::I32Const(heap_base));
    f.instruction(&I::GlobalSet(0));
    f.instruction(&I::End);
    f
}

/// Does `ty` flatten without a variant's joined slots: primitives, strings,
/// lists, and records of those?
fn plain(resolve: &Resolve, ty: &WitType) -> bool {
    match dealias(resolve, *ty) {
        WitType::Id(id) => match &resolve.types[id].kind {
            TypeDefKind::Record(r) => r.fields.iter().all(|f| plain(resolve, &f.ty)),
            TypeDefKind::List(_) => true,
            TypeDefKind::Tuple(t) => t.types.iter().all(|t| plain(resolve, t)),
            _ => false,
        },
        _ => true,
    }
}

/// **The module's own functions, beyond the three every module has**: small
/// routines a body calls rather than repeating inline. Each is emitted once,
/// after `post_return`, only if a body uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Helper {
    /// `(p1, l1, p2, l2) -> 1 | 0`: byte equality of two strings.
    StrEq,
    /// `(p1, l1, p2, l2) -> -1 | 0 | 1`: byte order of two strings, which is
    /// code point order for UTF-8.
    StrCmp,
    /// `(i64) -> (ptr, len)`: decimal text, allocated in the region.
    IntToString,
    /// `(p, l) -> i64`: code points in a string (ADR-0040).
    Utf8Count,
    /// `(p, l) -> (ptr, len)`: a string's code points, as a `list<s64>`.
    Codepoints,
    /// `(ptr, len) -> (p, l)`: UTF-8 from a `list<s64>`; traps on a value
    /// that is not a Unicode scalar value.
    FromCodepoints,
    /// `(start, end) -> (ptr, len)`: the `list<s64>` from `start` up to
    /// `end`; traps on one of more than 2^28 - 1 elements, whose bytes no
    /// `i32` length counts.
    Range,
    /// `(p1, l1, p2, l2) -> 1 | 0`.
    StartsWith,
    EndsWith,
    Contains,
    /// `(list_ptr, list_len, sep_p, sep_l) -> (p, l)`.
    Join,
    /// `(p, l) -> (p, l)`: a view without `White_Space` at either end.
    Trim,
    /// `(p, l) -> (p, l)`: `A`-`Z` to `a`-`z`.
    LowerAscii,
    /// `(p, l, start, end) -> (p, l)`: a view of code points `start` up to
    /// `end`, each an `i64` clamped to the string (ADR-0055).
    Slice,
    /// `(p, l, ranges, ranges', multi, multi') -> (p, l)`: each code point
    /// mapped by a case table in the data segment (ADR-0056).
    CaseMap,
    /// `(root) -> (ptr, len)`: a value of a type that contains itself, as its
    /// nodes (ADR-0194), by [`Helpers::graphs`]' layout at this index.
    Encode(u32),
    /// `(ptr, len) -> root`: nodes checked, and made in place into the value
    /// they encode (ADR-0194).
    Decode(u32),
}

struct Helpers {
    /// The index the first helper gets.
    first: u32,
    used: Vec<Helper>,
    /// Each type that contains itself a body converts (ADR-0194): the body's
    /// type and its node's, and their layout.
    graphs: Vec<((wit_parser::TypeId, wit_parser::TypeId), GraphLayout)>,
}

impl Helpers {
    fn index(&mut self, h: Helper) -> u32 {
        let at = match self.used.iter().position(|u| *u == h) {
            Some(i) => i,
            None => {
                self.used.push(h);
                self.used.len() - 1
            }
        };
        self.first + at as u32
    }

    /// The layout of a type that contains itself, by its index.
    fn graph(&mut self, key: (wit_parser::TypeId, wit_parser::TypeId), layout: GraphLayout) -> u32 {
        match self.graphs.iter().position(|(k, _)| *k == key) {
            Some(i) => i as u32,
            None => {
                self.graphs.push((key, layout));
                (self.graphs.len() - 1) as u32
            }
        }
    }
}

impl Helper {
    fn signature(self) -> (Vec<ValType>, Vec<ValType>) {
        match self {
            Helper::StrEq
            | Helper::StrCmp
            | Helper::StartsWith
            | Helper::EndsWith
            | Helper::Contains => (vec![ValType::I32; 4], vec![ValType::I32]),
            Helper::IntToString => (vec![ValType::I64], vec![ValType::I32, ValType::I32]),
            Helper::Utf8Count => (vec![ValType::I32; 2], vec![ValType::I64]),
            Helper::Codepoints | Helper::FromCodepoints | Helper::Trim | Helper::LowerAscii => {
                (vec![ValType::I32; 2], vec![ValType::I32; 2])
            }
            Helper::Range => (vec![ValType::I64; 2], vec![ValType::I32; 2]),
            Helper::Join => (vec![ValType::I32; 4], vec![ValType::I32; 2]),
            Helper::Slice => (
                vec![ValType::I32, ValType::I32, ValType::I64, ValType::I64],
                vec![ValType::I32; 2],
            ),
            Helper::CaseMap => (vec![ValType::I32; 6], vec![ValType::I32; 2]),
            Helper::Encode(_) => (vec![ValType::I32], vec![ValType::I32; 2]),
            Helper::Decode(_) => (vec![ValType::I32; 2], vec![ValType::I32]),
        }
    }

    fn body(self, realloc_index: u32, graphs: &[GraphLayout]) -> Function {
        use wasm_encoder::BlockType::Empty;
        use wasm_encoder::Instruction as I;
        let byte = |offset: u64| {
            I::I32Load8U(MemArg {
                offset,
                align: 0,
                memory_index: 0,
            })
        };
        let (locals, ops): (Vec<(u32, ValType)>, Vec<I<'static>>) = match self {
            Helper::Utf8Count
            | Helper::Codepoints
            | Helper::FromCodepoints
            | Helper::Range
            | Helper::StartsWith
            | Helper::EndsWith
            | Helper::Contains
            | Helper::Join
            | Helper::Trim
            | Helper::LowerAscii
            | Helper::Slice
            | Helper::CaseMap => string_helper(self, realloc_index),
            Helper::Encode(i) => encode_graph(&graphs[i as usize], realloc_index),
            Helper::Decode(i) => decode_graph(&graphs[i as usize], realloc_index),
            // params: 0 p1, 1 l1, 2 p2, 3 l2; local 4 i
            Helper::StrEq => (
                vec![(1, ValType::I32)],
                vec![
                    I::LocalGet(1),
                    I::LocalGet(3),
                    I::I32Ne,
                    I::If(Empty),
                    I::I32Const(0),
                    I::Return,
                    I::End,
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(4),
                    I::LocalGet(1),
                    I::I32GeU,
                    I::BrIf(1),
                    I::LocalGet(0),
                    I::LocalGet(4),
                    I::I32Add,
                    byte(0),
                    I::LocalGet(2),
                    I::LocalGet(4),
                    I::I32Add,
                    byte(0),
                    I::I32Ne,
                    I::If(Empty),
                    I::I32Const(0),
                    I::Return,
                    I::End,
                    I::LocalGet(4),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(4),
                    I::Br(0),
                    I::End,
                    I::End,
                    I::I32Const(1),
                    I::End,
                ],
            ),
            // params: 0 p1, 1 l1, 2 p2, 3 l2; locals 4 i, 5 n, 6 a, 7 b
            Helper::StrCmp => (
                vec![(4, ValType::I32)],
                vec![
                    // n = min(l1, l2)
                    I::LocalGet(1),
                    I::LocalGet(3),
                    I::LocalGet(1),
                    I::LocalGet(3),
                    I::I32LtU,
                    I::Select,
                    I::LocalSet(5),
                    I::Block(Empty),
                    I::Loop(Empty),
                    I::LocalGet(4),
                    I::LocalGet(5),
                    I::I32GeU,
                    I::BrIf(1),
                    I::LocalGet(0),
                    I::LocalGet(4),
                    I::I32Add,
                    byte(0),
                    I::LocalSet(6),
                    I::LocalGet(2),
                    I::LocalGet(4),
                    I::I32Add,
                    byte(0),
                    I::LocalSet(7),
                    I::LocalGet(6),
                    I::LocalGet(7),
                    I::I32Ne,
                    I::If(Empty),
                    I::I32Const(-1),
                    I::I32Const(1),
                    I::LocalGet(6),
                    I::LocalGet(7),
                    I::I32LtU,
                    I::Select,
                    I::Return,
                    I::End,
                    I::LocalGet(4),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(4),
                    I::Br(0),
                    I::End,
                    I::End,
                    // A proper prefix orders first.
                    I::LocalGet(1),
                    I::LocalGet(3),
                    I::I32LtU,
                    I::If(Empty),
                    I::I32Const(-1),
                    I::Return,
                    I::End,
                    I::LocalGet(1),
                    I::LocalGet(3),
                    I::I32GtU,
                    I::End,
                ],
            ),
            // param: 0 v; locals 1 end, 2 pos, 3 neg, 4 u (i64)
            Helper::IntToString => (
                vec![(3, ValType::I32), (1, ValType::I64)],
                vec![
                    // 20 bytes hold "-9223372036854775808".
                    I::I32Const(0),
                    I::I32Const(0),
                    I::I32Const(1),
                    I::I32Const(20),
                    I::Call(realloc_index),
                    I::I32Const(20),
                    I::I32Add,
                    I::LocalTee(1),
                    I::LocalSet(2),
                    I::LocalGet(0),
                    I::I64Const(0),
                    I::I64LtS,
                    I::LocalSet(3),
                    // The magnitude, unsigned: `0 - MIN` wraps to 2^63, which
                    // is MIN's magnitude read unsigned.
                    I::I64Const(0),
                    I::LocalGet(0),
                    I::I64Sub,
                    I::LocalGet(0),
                    I::LocalGet(3),
                    I::Select,
                    I::LocalSet(4),
                    I::Loop(Empty),
                    I::LocalGet(2),
                    I::I32Const(1),
                    I::I32Sub,
                    I::LocalTee(2),
                    I::LocalGet(4),
                    I::I64Const(10),
                    I::I64RemU,
                    I::I32WrapI64,
                    I::I32Const(48),
                    I::I32Add,
                    I::I32Store8(MemArg {
                        offset: 0,
                        align: 0,
                        memory_index: 0,
                    }),
                    I::LocalGet(4),
                    I::I64Const(10),
                    I::I64DivU,
                    I::LocalTee(4),
                    I::I64Const(0),
                    I::I64Ne,
                    I::BrIf(0),
                    I::End,
                    I::LocalGet(3),
                    I::If(Empty),
                    I::LocalGet(2),
                    I::I32Const(1),
                    I::I32Sub,
                    I::LocalTee(2),
                    I::I32Const(45),
                    I::I32Store8(MemArg {
                        offset: 0,
                        align: 0,
                        memory_index: 0,
                    }),
                    I::End,
                    I::LocalGet(2),
                    I::LocalGet(1),
                    I::LocalGet(2),
                    I::I32Sub,
                    I::End,
                ],
            ),
        };
        let mut f = Function::new(locals);
        for op in &ops {
            f.instruction(op);
        }
        f
    }
}
