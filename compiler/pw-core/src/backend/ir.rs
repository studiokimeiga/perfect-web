//! **The Backend IR — what every backend lowers from, and what none of them
//! may re-derive.**
//!
//! Architect ruling, 2026-08-09, scoping E10-A:
//!
//! > Do not lower directly from HIR into Wasm instructions. […] E10 will later
//! > want multiple backends. The stable division should be:
//! >
//! > ```text
//! > Pleris semantics
//! >       ↓
//! > Backend IR
//! >     /       \
//! >  Wasm       future native backend
//! > ```
//!
//! and the rule that shapes every type in this file:
//!
//! > The backend consumes semantics already established upstream. It must not
//! > rediscover effects, capabilities, placement, privacy, or ABI types.
//!
//! # Nothing here is a name
//!
//! > `DefId`, not names. `DeclaredType`, not reconstructed strings.
//! > `CapabilityId`, not `"database.write"`.
//!
//! This project has spent its life deleting spelling-based resolution —
//! `docs/RISK_QUEUE.md` is mostly instances of it — and a backend that took a
//! string would be reintroducing it at the point where the answer becomes
//! machine code. So [`Type::Nominal`] holds a `DefId`, a call names a `DefId`,
//! and a host call names a [`CapabilityId`] the contract already derived.
//!
//! If a lowering finds itself formatting a name to decide something, that is
//! the bug this module is shaped to prevent.
//!
//! # Three outcomes, never a silent one
//!
//! > Lowering should return `Lowered | Unsupported(reason) | Blocked(upstream
//! > error)`, not turn an unknown HIR node into no instructions.
//!
//! [`Lowering`] is that. A construct the backend does not support yet produces
//! a diagnostic naming it; an upstream error produces `Blocked`, because a
//! program that did not check is not a program with a backend problem. Neither
//! is an empty instruction list, which is what "silently lower differently"
//! looks like from the outside — and which would produce a component that runs
//! and does the wrong thing.

use std::fmt;

use crate::hir::Span;
use crate::resolve::DefId;

/// One value, in one function. SSA: assigned once, at the instruction that
/// produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(pub u32);

/// One basic block, in one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

/// **A capability, as the contract already derived it.**
///
/// A newtype over `contract::Capability` rather than a second derivation: that
/// type is ADR-0020's frozen boundary, and E9's ontology pull-forward exists
/// precisely so this identity survives changes to inference, row representation
/// and syntax. A backend inventing its own capability name would be the second
/// answer the pull-forward was meant to prevent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityId(pub crate::contract::Capability);

impl CapabilityId {
    /// The canonical text, for a host import name. Read from the contract's own
    /// spelling so the two cannot drift.
    pub fn name(&self) -> String {
        self.0.name()
    }
}

/// **A fully resolved type.**
///
/// No strings, and no `DeclaredType` either — that is the SOURCE form, and
/// carrying it here would make every backend re-resolve `Cart` against a
/// workspace it does not have. A nominal type is a `DefId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    /// Nothing crosses. A command with no declared return.
    Unit,
    /// A record, variant or opaque type, by the declaration that defines it,
    /// and the types it is applied to: none for a type that declares no
    /// parameters, `[Int]` for `Box<Int>` (ADR-0062). An instance is laid
    /// out by its arguments, so `Box<Int>` and `Box<String>` are two
    /// layouts.
    Nominal(DefId, Vec<Type>),
    Result(Box<Type>, Box<Type>),
    Option(Box<Type>),
    List(Box<Type>),
    /// **`Map<K, V>`** (ADR-0057): its entries in ascending key order, each
    /// key once, laid out as `list<tuple<K, V>>`. A key is an `Int` or a
    /// `String`.
    Map(Box<Type>, Box<Type>),
    /// **`Set<T>`** (ADR-0057): its elements ascending, each once, laid out
    /// as `list<T>`.
    Set(Box<Type>),
    /// **A function value**, by its parameters and its result (ADR-0052).
    /// It never crosses the component boundary.
    Function(Vec<Type>, Box<Type>),
}

impl Type {
    /// Every nominal type reachable inside this one.
    ///
    /// The backend needs the transitive set to emit type definitions, and
    /// walking here rather than at each call site is the same lesson
    /// `DeclaredType` encodes: a type read one level deep is not the type.
    pub fn nominals(&self) -> Vec<(DefId, Vec<Type>)> {
        let mut out = Vec::new();
        self.walk(&mut |t| {
            if let Type::Nominal(d, args) = t {
                out.push((*d, args.clone()));
            }
        });
        out
    }

    fn walk(&self, f: &mut impl FnMut(&Type)) {
        f(self);
        match self {
            Type::Result(a, b) | Type::Map(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Type::Option(a) | Type::List(a) | Type::Set(a) => a.walk(f),
            // `Box<Cart>` uses `Cart` too.
            Type::Nominal(_, args) => {
                for a in args {
                    a.walk(f);
                }
            }
            Type::Function(ps, r) => {
                for p in ps {
                    p.walk(f);
                }
                r.walk(f);
            }
            _ => {}
        }
    }
}

/// A literal the backend can materialize without calling anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
}

/// **How a handler sends a command again when no answer came** (ADR-0173),
/// from the command's `retry` clause: at most `max` more times, each after a
/// delay that doubles, drawn at random below it where the clause says
/// `jitter`. A delay that did not double, `fixed`, left the language with
/// ADR-0215.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resend {
    pub max: u32,
    pub jitter: bool,
}

/// One instruction. Every one names the value it produces and that value's
/// type, so a consumer never infers either.
#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    Const {
        result: ValueId,
        value: Const,
        ty: Type,
    },
    /// **A call to another Pleris declaration, compiled beside this one**
    /// (ADR-0050): one of the exported function's [`Function::callees`],
    /// named by its identity and the types it was instantiated at. A call the
    /// lowering can inline is inlined (ADR-0039 §4); this is a recursive one.
    Call {
        result: ValueId,
        callee: DefId,
        /// The callee's type arguments, in the order it declares them: which
        /// of its compiled instances this call reaches.
        instance: Vec<Type>,
        args: Vec<ValueId>,
        ty: Type,
    },
    /// **A call to something this component does not contain.**
    ///
    /// Separate from `Call` because it is a different kind of thing, not a call
    /// that happens to be external: the implementation comes from somewhere
    /// else, that somewhere is named by an `ImportId` the artifact carries, and
    /// the E8 audit compares the built thing's imports against exactly these.
    ///
    /// It was `HostCall { capability }` until 2026-08-20, and that was the
    /// model error the encoder found: a capability authorizes an operation and
    /// does not identify one, so two functions sharing an authority had one
    /// import with two ABIs. The authority is now a property of the IMPORT, not
    /// of the call.
    ImportCall {
        result: ValueId,
        import: ImportId,
        args: Vec<ValueId>,
        ty: Type,
    },
    /// Build a record.
    Construct {
        result: ValueId,
        ctor: DefId,
        args: Vec<ValueId>,
        ty: Type,
    },
    /// **Build a case of a declared sum type** (ADR-0059): its `case`th
    /// case, counted from zero in the order the declaration writes them,
    /// which is its discriminant, with its payload's fields in order. `ty`
    /// is the type, a [`Type::Nominal`].
    Case {
        result: ValueId,
        case: u32,
        fields: Vec<ValueId>,
        ty: Type,
    },
    /// Read a field, by index. The name was resolved upstream.
    Project {
        result: ValueId,
        of: ValueId,
        field: u32,
        ty: Type,
    },
    /// Build a case of the language's own `Option` or `Result`: `Some(x)`,
    /// `None`, `Ok(x)`, `Err(e)`. Separate from `Construct`, whose constructor
    /// is a declaration: these cases have no `DefId` to name.
    Variant {
        result: ValueId,
        case: BuiltinCase,
        payload: Option<ValueId>,
        ty: Type,
    },
    /// **Choose by a variant's case**: `match x { Some(y) => .., None => .. }`.
    ///
    /// Structured rather than a branch between blocks. Each arm is a region
    /// whose last value is the arm's value, and the arms' values are the
    /// result. Wasm's control flow is structured, so a region is what the
    /// encoder emits directly, with no reconstruction of structure from a
    /// graph. A value defined inside a region is visible only there.
    Match {
        result: ValueId,
        scrutinee: ValueId,
        arms: Vec<MatchArm>,
        ty: Type,
    },
    /// Arithmetic or a comparison on two values of one primitive type
    /// (ADR-0039). `Int` arithmetic traps rather than wrapping, and `/` and
    /// `%` are Euclidean.
    Binary {
        result: ValueId,
        op: BinaryOp,
        lhs: ValueId,
        rhs: ValueId,
        ty: Type,
    },
    /// `-x` on an `Int` or a `Float`, and `!b`.
    Unary {
        result: ValueId,
        op: UnaryOp,
        operand: ValueId,
        ty: Type,
    },
    /// **`if c { .. } else { .. }`**, structured as a match is: two regions,
    /// whose values are the result. `&` and `|`, the language's logical
    /// operators, lower to it, so their right side is evaluated only when it
    /// decides the result.
    If {
        result: ValueId,
        cond: ValueId,
        then: Region,
        els: Region,
        ty: Type,
    },
    /// A string made of other strings, in order: an interpolation's pieces.
    Concat {
        result: ValueId,
        parts: Vec<ValueId>,
        ty: Type,
    },
    /// A value written as text: an `Int` in decimal, a `Bool` as `true` or
    /// `false`. What an interpolation's hole holds.
    Format {
        result: ValueId,
        value: ValueId,
        ty: Type,
    },
    /// **A loop over a list** (ADR-0040): `body` runs once per element, with
    /// `params` bound. For a fold the parameters are the accumulator, which
    /// `seed` starts, then the element; for a sort, the two elements being
    /// compared; otherwise the element. The function argument the program
    /// wrote, a lambda or a named declaration, is the body.
    Each {
        result: ValueId,
        kind: EachKind,
        list: ValueId,
        seed: Option<ValueId>,
        params: Vec<ValueId>,
        body: Region,
        ty: Type,
    },
    /// **An operation the standard library declares and the compiler
    /// supplies** (ADR-0040), on values.
    Intrinsic {
        result: ValueId,
        op: Intrinsic,
        args: Vec<ValueId>,
        ty: Type,
    },
    /// `[a, b, c]`.
    MakeList {
        result: ValueId,
        items: Vec<ValueId>,
        ty: Type,
    },
    /// **Leave the function with `value`** (ADR-0051): `return e`, and the
    /// failing side of `e?`. Nothing after it in its region runs. `result`
    /// stands for the region's value, of the type the region needs, and is
    /// never read.
    Return {
        result: ValueId,
        value: ValueId,
        ty: Type,
    },
    /// **A mutable binding**, `let mut x = e` (ADR-0051): `result` names the
    /// variable, which holds `init` until a [`Instr::Set`].
    Local {
        result: ValueId,
        init: ValueId,
        ty: Type,
    },
    /// `x = e`: the variable `local` holds `value` from here on. Its own
    /// value is the unit value.
    Set {
        result: ValueId,
        local: ValueId,
        value: ValueId,
        ty: Type,
    },
    /// What the variable `local` holds here: a copy, which a later `Set` does
    /// not change.
    Get {
        result: ValueId,
        local: ValueId,
        ty: Type,
    },
    /// **A function value** (ADR-0052): the export's closure `index`, with
    /// the values it captures, in the order its code takes them.
    Closure {
        result: ValueId,
        index: u32,
        captures: Vec<ValueId>,
        ty: Type,
    },
    /// **A command a resumable handler calls** (ADR-0058), by its
    /// component id, through the handler's context: in the browser, awaited,
    /// its arguments sent as JSON. Only a handler's body holds one, and its
    /// answer is not read: its value is the unit value.
    Command {
        result: ValueId,
        command: String,
        args: Vec<ValueId>,
        ty: Type,
        /// How the handler sends it again when no answer came, as its
        /// declaration's `retry` says (ADR-0173). `None`: once.
        resend: Option<Resend>,
    },
    /// **A handler goes to a page once its command commits** (ADR-0280): the
    /// page's route, and the value of each parameter it gives, by name, each
    /// text (PW0621). The browser fills each `{name}` segment of the route
    /// with its value, encoded as a URI component, and goes there once the
    /// commit's answer is in. Only a handler's body holds one, last in the
    /// `Ok` arm of a command's answer (PW5043). Its value is the unit value.
    Navigate {
        result: ValueId,
        route: String,
        args: Vec<(String, ValueId)>,
        ty: Type,
    },
    /// **A page's signal, read** (ADR-0130): the value the browser holds now,
    /// through the handler's context. Only a handler's body holds one.
    SignalGet {
        result: ValueId,
        signal: String,
        ty: Type,
    },
    /// **A page's signal, changed** (ADR-0130): the browser holds `value`
    /// from now, and renders again each part that reads it. Its value is the
    /// unit value. Only a handler's body holds one.
    SignalSet {
        result: ValueId,
        signal: String,
        value: ValueId,
        ty: Type,
    },
    /// **The same value, as another type with its representation**
    /// (ADR-0054): an opaque type built from its representation,
    /// `PositiveInt(1)`, and its representation read back, `n.value`. The
    /// bits do not change.
    Retype {
        result: ValueId,
        value: ValueId,
        ty: Type,
    },
    /// **A call through a function value** (ADR-0052). `function_ty` is the
    /// value's type, which fixes how it is called.
    Apply {
        result: ValueId,
        function: ValueId,
        function_ty: Type,
        args: Vec<ValueId>,
        ty: Type,
    },
}

/// What a loop over a list computes (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EachKind {
    /// Each element's value: a list as long as the input.
    Map,
    /// The elements whose value is `true`, in order.
    Filter,
    /// The accumulator after the last element.
    Fold,
    /// Whether any element's value is `true`; stops at the first.
    Any,
    /// Whether every element's value is `true`; stops at the first `false`.
    All,
    /// `Some` of the first element whose value is `true`.
    Find,
    /// The elements ordered by the body, a comparison: a positive value puts
    /// the second first. Stable.
    SortBy,
    /// Runs of consecutive elements whose body values, a key, are equal: a
    /// list of views into the list, in order.
    GroupBy,
    /// **`for x in xs { .. }`** (ADR-0051): the body runs once per element,
    /// for what it does to the variables around it and for a `return` it may
    /// make; its value is discarded, and the loop's is the unit value.
    For,
}

/// A first-order operation of the standard library (ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Intrinsic {
    ListLength,
    ListGet,
    ListTake,
    /// All but the first `count`, clamped: a view (ADR-0055).
    ListDrop,
    /// `start` up to `end`, each clamped: a view (ADR-0055).
    ListSlice,
    /// The whole numbers from `start` up to `end`, none where `end` is not
    /// past it; more than 2^28 - 1 traps.
    ListRange,
    /// A copy, last element first (ADR-0055).
    ListReverse,
    ListConcat,
    /// In code points.
    StrLength,
    StrCodepoints,
    /// Traps on a value that is not a Unicode scalar value.
    StrFromCodepoints,
    StrStartsWith,
    StrEndsWith,
    StrContains,
    StrJoin,
    /// Unicode `White_Space` from both ends.
    StrTrim,
    /// Code points `start` up to `end`, each clamped: a view (ADR-0055).
    StrSlice,
    /// `A`-`Z` only.
    StrToLowerAscii,
    /// Each code point's lower case, from `case::table` (ADR-0056).
    StrToLower,
    /// Each code point's upper case, from `case::table` (ADR-0056).
    StrToUpper,
    /// The nearest `Float`, ties to even: exact up to 2^53 (ADR-0043).
    FloatFromInt,
    // Maps and sets (ADR-0057). A lookup is a binary search on the key; a
    // change is a new value, copied.
    MapEmpty,
    MapSize,
    MapGet,
    MapContains,
    /// Adds the entry, or replaces the value under its key.
    MapInsert,
    MapRemove,
    MapKeys,
    MapValues,
    /// The value at each position under the key at the same position; a
    /// later key replaces an earlier one; lists of two lengths trap.
    MapFromLists,
    /// A map from outside, sorted by key as it arrives, stably; a key twice
    /// traps (ruling 0057-c, ADR-0259). Its value is the map sorted.
    MapCheck,
    SetEmpty,
    SetFromList,
    SetSize,
    SetContains,
    SetInsert,
    SetRemove,
    SetToList,
    SetUnion,
    SetIntersection,
    SetDifference,
    /// A set from outside, sorted as `MapCheck` sorts a map.
    SetCheck,
}

/// **An `intrinsic` declaration's operation**, by the name its policy gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Each(EachKind),
    Intrinsic(Intrinsic),
}

impl Operation {
    /// The operation an `intrinsic "..."` clause names, if the backend knows
    /// it. The table is the standard library's, one entry per declaration in
    /// `packages/pw-std`.
    pub fn named(name: &str) -> Option<Operation> {
        use EachKind as E;
        use Intrinsic as I;
        Some(match name {
            "list.map" => Operation::Each(E::Map),
            "list.filter" => Operation::Each(E::Filter),
            "list.fold" => Operation::Each(E::Fold),
            "list.any" => Operation::Each(E::Any),
            "list.all" => Operation::Each(E::All),
            "list.find" => Operation::Each(E::Find),
            "list.sort_by" => Operation::Each(E::SortBy),
            "list.group_by" => Operation::Each(E::GroupBy),
            "list.length" => Operation::Intrinsic(I::ListLength),
            "list.get" => Operation::Intrinsic(I::ListGet),
            "list.take" => Operation::Intrinsic(I::ListTake),
            "list.drop" => Operation::Intrinsic(I::ListDrop),
            "list.slice" => Operation::Intrinsic(I::ListSlice),
            "list.range" => Operation::Intrinsic(I::ListRange),
            "list.reverse" => Operation::Intrinsic(I::ListReverse),
            "list.concat" => Operation::Intrinsic(I::ListConcat),
            "string.length" => Operation::Intrinsic(I::StrLength),
            "string.codepoints" => Operation::Intrinsic(I::StrCodepoints),
            "string.from_codepoints" => Operation::Intrinsic(I::StrFromCodepoints),
            "string.starts_with" => Operation::Intrinsic(I::StrStartsWith),
            "string.ends_with" => Operation::Intrinsic(I::StrEndsWith),
            "string.contains" => Operation::Intrinsic(I::StrContains),
            "string.join" => Operation::Intrinsic(I::StrJoin),
            "string.trim" => Operation::Intrinsic(I::StrTrim),
            "string.slice" => Operation::Intrinsic(I::StrSlice),
            "string.to_lower_ascii" => Operation::Intrinsic(I::StrToLowerAscii),
            "string.to_lower" => Operation::Intrinsic(I::StrToLower),
            "string.to_upper" => Operation::Intrinsic(I::StrToUpper),
            "float.from_int" => Operation::Intrinsic(I::FloatFromInt),
            "map.empty" => Operation::Intrinsic(I::MapEmpty),
            "map.size" => Operation::Intrinsic(I::MapSize),
            "map.get" => Operation::Intrinsic(I::MapGet),
            "map.contains" => Operation::Intrinsic(I::MapContains),
            "map.insert" => Operation::Intrinsic(I::MapInsert),
            "map.remove" => Operation::Intrinsic(I::MapRemove),
            "map.keys" => Operation::Intrinsic(I::MapKeys),
            "map.values" => Operation::Intrinsic(I::MapValues),
            "map.from_lists" => Operation::Intrinsic(I::MapFromLists),
            "set.empty" => Operation::Intrinsic(I::SetEmpty),
            "set.from_list" => Operation::Intrinsic(I::SetFromList),
            "set.size" => Operation::Intrinsic(I::SetSize),
            "set.contains" => Operation::Intrinsic(I::SetContains),
            "set.insert" => Operation::Intrinsic(I::SetInsert),
            "set.remove" => Operation::Intrinsic(I::SetRemove),
            "set.to_list" => Operation::Intrinsic(I::SetToList),
            "set.union" => Operation::Intrinsic(I::SetUnion),
            "set.intersection" => Operation::Intrinsic(I::SetIntersection),
            "set.difference" => Operation::Intrinsic(I::SetDifference),
            _ => return None,
        })
    }
}

/// An operator on two values of one primitive type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    /// Euclidean: `-7 / 2` is `-4`.
    Div,
    /// Euclidean: in `[0, |b|)`.
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinaryOp {
    /// Does this operator produce a `Bool` rather than its operands' type?
    pub fn compares(self) -> bool {
        matches!(
            self,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// A case of the language's own variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltinCase {
    Some,
    None,
    Ok,
    Err,
}

impl BuiltinCase {
    /// Does this case carry a payload?
    pub fn has_payload(self) -> bool {
        !matches!(self, BuiltinCase::None)
    }
}

/// A case of a variant type: the language's own, or a declared sum type's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Case {
    Builtin(BuiltinCase),
    /// A declared sum type's case, by its position in the declaration
    /// (ADR-0059).
    Declared(u32),
}

/// One arm of a [`Instr::Match`].
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// The cases this arm takes: one for `Some(y)` or `Circle(r)`; several
    /// for `A | B`, and for `_` or a name, which take every case no other arm
    /// names (ADR-0059). Every case of the scrutinee's type is taken by
    /// exactly one arm, so an encoder needs no fallthrough.
    pub cases: Vec<Case>,
    /// The payload's fields, bound for the arm's body, in order: `y` in
    /// `Some(y)`, `w` and `h` in `Rect(w, h)`, and `None` for a field no name
    /// binds. Empty for an arm of several cases, which binds none.
    pub bindings: Vec<Option<ValueId>>,
    pub body: Region,
}

/// Instructions whose last value is the region's value.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub instrs: Vec<Instr>,
    pub value: ValueId,
}

impl Instr {
    pub fn result(&self) -> ValueId {
        match self {
            Instr::Const { result, .. }
            | Instr::Call { result, .. }
            | Instr::ImportCall { result, .. }
            | Instr::Construct { result, .. }
            | Instr::Case { result, .. }
            | Instr::Project { result, .. }
            | Instr::Variant { result, .. }
            | Instr::Match { result, .. }
            | Instr::Binary { result, .. }
            | Instr::Unary { result, .. }
            | Instr::If { result, .. }
            | Instr::Concat { result, .. }
            | Instr::Format { result, .. }
            | Instr::Each { result, .. }
            | Instr::Intrinsic { result, .. }
            | Instr::MakeList { result, .. }
            | Instr::Return { result, .. }
            | Instr::Local { result, .. }
            | Instr::Set { result, .. }
            | Instr::Get { result, .. }
            | Instr::Closure { result, .. }
            | Instr::Retype { result, .. }
            | Instr::Command { result, .. }
            | Instr::Navigate { result, .. }
            | Instr::SignalGet { result, .. }
            | Instr::SignalSet { result, .. }
            | Instr::Apply { result, .. } => *result,
        }
    }

    pub fn ty(&self) -> &Type {
        match self {
            Instr::Const { ty, .. }
            | Instr::Call { ty, .. }
            | Instr::ImportCall { ty, .. }
            | Instr::Construct { ty, .. }
            | Instr::Case { ty, .. }
            | Instr::Project { ty, .. }
            | Instr::Variant { ty, .. }
            | Instr::Match { ty, .. }
            | Instr::Binary { ty, .. }
            | Instr::Unary { ty, .. }
            | Instr::If { ty, .. }
            | Instr::Concat { ty, .. }
            | Instr::Format { ty, .. }
            | Instr::Each { ty, .. }
            | Instr::Intrinsic { ty, .. }
            | Instr::MakeList { ty, .. }
            | Instr::Return { ty, .. }
            | Instr::Local { ty, .. }
            | Instr::Set { ty, .. }
            | Instr::Get { ty, .. }
            | Instr::Closure { ty, .. }
            | Instr::Retype { ty, .. }
            | Instr::Command { ty, .. }
            | Instr::Navigate { ty, .. }
            | Instr::SignalGet { ty, .. }
            | Instr::SignalSet { ty, .. }
            | Instr::Apply { ty, .. } => ty,
        }
    }

    /// The regions this instruction holds: a match's arms, an `if`'s two
    /// sides. A walk that stops at a region's edge is how an import called
    /// only inside an arm went missing from the module's imports.
    pub fn regions(&self) -> Vec<&Region> {
        match self {
            Instr::Match { arms, .. } => arms.iter().map(|a| &a.body).collect(),
            Instr::If { then, els, .. } => vec![then, els],
            Instr::Each { body, .. } => vec![body],
            _ => Vec::new(),
        }
    }

    /// This instruction and every instruction inside its regions, in order.
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Instr>) {
        out.push(self);
        for r in self.regions() {
            for i in &r.instrs {
                i.walk(out);
            }
        }
    }
}

/// Every instruction in `instrs`, regions included.
pub fn all_instrs(instrs: &[Instr]) -> Vec<&Instr> {
    let mut out = Vec::new();
    for i in instrs {
        i.walk(&mut out);
    }
    out
}

/// How a block ends. Every block has one — there is no falling off the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    Return(ValueId),
    Jump(BlockId),
    Branch {
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    /// Reached only by a program that already failed to check. Emitted so the
    /// IR stays well-formed rather than leaving a block with no terminator,
    /// which is the shape a later pass silently walks off the end of.
    Unreachable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub instrs: Vec<Instr>,
    pub terminator: Terminator,
}

/// One lowered declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// The declaration this is, by resolved identity.
    pub def: DefId,
    /// Its exported name, for the component's interface. Carried rather than
    /// derived: `contract::contracts` decided it, and a second construction is
    /// how the two come to disagree.
    pub export: String,
    pub params: Vec<(ValueId, Type)>,
    pub ret: Type,
    pub blocks: Vec<Block>,
    /// Every capability this function calls across. **Not re-derived** — it is
    /// the contract's `required_capabilities`, narrowed to this declaration,
    /// and the E8 audit compares the built artifact against it.
    pub capabilities: Vec<CapabilityId>,
    /// For a function compiled beside an export: the type arguments it was
    /// instantiated at. Empty for an export, and for a callee that declares
    /// no type parameters.
    pub instance: Vec<Type>,
    /// **The declarations this one calls rather than inlines** (ADR-0050),
    /// every instance the export reaches, transitively, each once. Only an
    /// exported function has any: a callee's own calls are in this list too.
    pub callees: Vec<Function>,
    /// **The code of every function value the export makes** (ADR-0052),
    /// by the index a [`Instr::Closure`] names. Only an export has any.
    pub closures: Vec<Closure>,
}

/// **A function value's code** (ADR-0052): a function whose first
/// `captures` parameters are what the value captured, and the rest the
/// parameters its callers pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub captures: usize,
    pub function: Function,
}

impl Function {
    pub fn entry(&self) -> Option<&Block> {
        self.blocks.first()
    }
}

/// A whole program, lowered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
    /// Nominal types the functions use, in dependency order.
    pub types: Vec<TypeDef>,
    /// **The host functions this program imports, as the CONTRACT names them.**
    ///
    /// Carried rather than derived, for the reason `Function::export` is:
    /// `contract::contracts` decided which interface serves a capability, from
    /// the effect's own declaration, and an encoder that worked it out again
    /// would be the second derivation this project keeps deleting. The E8 audit
    /// compares the built artifact's imports against the contract — so if the
    /// encoder invented the name, the audit would be comparing a guess with
    /// itself.
    pub imports: Vec<CallableImport>,
}

/// **A callable this component depends on, with the ABI to call it.**
///
/// Architect ruling, 2026-08-20, after the encoder proved the previous model
/// wrong:
///
/// > **A capability authorizes an operation. It does not identify the
/// > operation.** So `database.write<Carts>` must never be used as the callable
/// > import identity.
///
/// `Carts.add(s, item, qty)` and `Carts.clear(s)` both require
/// `database.write<Carts>` and have different ABIs. An import keyed on the
/// capability had no signature it could honestly carry, and the first thing the
/// validator said was *"expected i32 but nothing on stack"*.
///
/// Four separate facts, kept separate:
///
/// ```text
/// CapabilityId       authority
/// ImportId           callable operation identity
/// BackendSignature   callable ABI
/// interface          physical/component-model grouping
/// ```
///
/// The grouping is an ABI/package-layout decision and must not determine the
/// capability semantics — the same distinction E8 already draws between
/// semantic component granularity and physical bundling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableImport {
    pub id: ImportId,
    /// The source declaration this is, for provenance. A compiler-internal
    /// identity: deliberately NOT what the component boundary depends on,
    /// because a `DefId` does not survive past this compiler.
    pub callee: DefId,
    /// How the implementation is supplied.
    pub binding: ImportBinding,
    /// The complete checked ABI. **This is what the encoder reads** — a
    /// signature it derived from call sites would be a second answer to a
    /// question the front end settled, and the arity conflict is what happens
    /// when there is no first answer.
    pub signature: BackendSignature,
    /// Authority required to invoke it. A **set**: an operation may need
    /// several, and one capability may authorize many operations — proven by
    /// `Carts.add` and `Carts.clear` sharing `database.write<Carts>`.
    ///
    /// May be empty. An operation can be placement-constrained and need no
    /// authority at all, which is already true of the browser-semantic effects.
    pub required_capabilities: Vec<CapabilityId>,
}

/// A callable import's identity, stable across the component boundary.
///
/// The interface and the operation within it — `pw:host/carts` and `add`. Not a
/// `DefId`: this is what a WIT world and a built artifact name, and it has to
/// mean something to a toolchain that never saw this compiler.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportId {
    pub interface: String,
    pub name: String,
}

impl ImportId {
    pub fn qualified(&self) -> String {
        format!("{}#{}", self.interface, self.name)
    }
}

/// How a callable import's implementation is supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportBinding {
    /// **The Pleris platform defines it and the host implements it.**
    /// `pw:host/session#read`. Platform ABI-stability expectations apply.
    PlatformHost,
    /// **The deployment supplies it, and the semantics are the
    /// application's.** `store:data/carts#add`.
    ///
    /// Architect ruling, 2026-08-20: *"The host process currently provides the
    /// implementation" is a deployment fact; it doesn't need to collapse their
    /// semantic ownership.* An application repository method is externally
    /// implemented today and is expected to become compiled Pleris over
    /// narrower platform data primitives — recorded in `docs/NEXT.md` so
    /// `pw:host/carts` cannot become the standard library by default.
    External,
    /// Another component exports it.
    Component,
}

/// A callable's ABI, as the checker established it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSignature {
    pub params: Vec<Type>,
    pub result: Type,
}

/// A nominal type's shape, resolved: one per instance, its fields and cases
/// under its arguments (ADR-0062).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub def: DefId,
    /// The arguments this instance applies the declaration to.
    pub args: Vec<Type>,
    pub name: String,
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Record {
        fields: Vec<(String, Type)>,
    },
    /// A declared sum type (ADR-0059): each case, in declaration order,
    /// with its payload's fields. A case of several fields crosses a
    /// boundary as one `tuple<..>`, as `wit.rs` writes it.
    Variant {
        cases: Vec<(String, Vec<Type>)>,
    },
    /// An opaque type, as its representation. A wire has no opacity to offer —
    /// the same decision `wit.rs` records and for the same reason.
    Alias(Box<Type>),
}

/// **What a lowering produced, and never silently nothing.**
///
/// Architect ruling, 2026-08-09: *"Lowering should return `Lowered |
/// Unsupported(reason) | Blocked(upstream error)`, not turn an unknown HIR node
/// into no instructions."*
#[derive(Debug, Clone, PartialEq)]
pub enum Lowering<T> {
    Lowered(T),
    /// The backend does not handle this construct yet.
    ///
    /// A real diagnostic, naming the construct. E10-A supports what
    /// `add_to_cart` needs and nothing more, so this is the ordinary answer for
    /// most of the language — and it must stay loud, because a construct
    /// lowered to nothing produces a component that runs and does the wrong
    /// thing.
    Unsupported {
        construct: &'static str,
        span: Span,
        reason: String,
    },
    /// The program did not check. Not the backend's problem, and not the
    /// backend's diagnostic to invent — reporting it here would deliver one
    /// defect twice in two vocabularies.
    Blocked {
        why: String,
        span: Span,
    },
}

impl<T> Lowering<T> {
    pub fn is_lowered(&self) -> bool {
        matches!(self, Lowering::Lowered(_))
    }

    pub fn lowered(self) -> Option<T> {
        match self {
            Lowering::Lowered(t) => Some(t),
            _ => None,
        }
    }

    /// Map the payload, keeping the refusal.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Lowering<U> {
        match self {
            Lowering::Lowered(t) => Lowering::Lowered(f(t)),
            Lowering::Unsupported {
                construct,
                span,
                reason,
            } => Lowering::Unsupported {
                construct,
                span,
                reason,
            },
            Lowering::Blocked { why, span } => Lowering::Blocked { why, span },
        }
    }
}

impl<T> fmt::Display for Lowering<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lowering::Lowered(_) => f.write_str("lowered"),
            Lowering::Unsupported {
                construct, reason, ..
            } => write!(f, "the backend does not lower {construct} yet: {reason}"),
            Lowering::Blocked { why, .. } => write!(f, "blocked upstream: {why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(n: u32) -> DefId {
        DefId { unit: 0, decl: n }
    }

    #[test]
    fn a_type_reports_every_nominal_inside_it() {
        // The lesson `DeclaredType` encodes, in the IR's own vocabulary: a type
        // read one level deep is not the type. `Result<Cart, CartError>` uses
        // two nominals and neither is at the head.
        let t = Type::Result(
            Box::new(Type::Nominal(def(1), vec![])),
            Box::new(Type::Nominal(def(2), vec![])),
        );
        assert_eq!(t.nominals(), vec![(def(1), vec![]), (def(2), vec![])]);

        // And through a carrier of a carrier.
        let nested = Type::Option(Box::new(Type::List(Box::new(Type::Nominal(
            def(3),
            vec![],
        )))));
        assert_eq!(nested.nominals(), vec![(def(3), vec![])]);

        // And an instance's arguments: `Box<Cart>` uses `Cart` (ADR-0062).
        let applied = Type::Nominal(def(4), vec![Type::Nominal(def(1), vec![])]);
        assert_eq!(
            applied.nominals(),
            vec![
                (def(4), vec![Type::Nominal(def(1), vec![])]),
                (def(1), vec![])
            ]
        );

        // The control: a type with no nominal reports none, so "found some" is
        // not what this says about everything.
        assert!(
            Type::Result(Box::new(Type::Int), Box::new(Type::Str))
                .nominals()
                .is_empty()
        );
    }

    #[test]
    fn an_unsupported_construct_is_not_an_empty_lowering() {
        // The distinction the whole enum exists for. Both of these produce no
        // instructions; only one of them is a program the backend understood.
        let empty: Lowering<Vec<Instr>> = Lowering::Lowered(vec![]);
        let refused: Lowering<Vec<Instr>> = Lowering::Unsupported {
            construct: "a pipeline",
            span: 0..1,
            reason: "E10-A lowers what `add_to_cart` needs".into(),
        };
        assert!(empty.is_lowered());
        assert!(!refused.is_lowered());
        assert!(refused.to_string().contains("does not lower a pipeline"));
    }

    #[test]
    fn map_keeps_a_refusal_rather_than_producing_a_default() {
        let refused: Lowering<u32> = Lowering::Blocked {
            why: "the program does not check".into(),
            span: 0..1,
        };
        let mapped = refused.map(|n| n + 1);
        assert!(matches!(mapped, Lowering::Blocked { .. }));
        assert!(mapped.lowered().is_none());
    }

    #[test]
    fn an_import_call_is_not_an_ordinary_call() {
        // They are different kinds of thing, not one with a flag. The E8 audit
        // compares the artifact's imports against exactly these, and a
        // flattened representation would make "which calls come from outside"
        // a question somebody answers by reading names.
        let imported = Instr::ImportCall {
            result: ValueId(0),
            import: ImportId {
                interface: "pw:host/carts".into(),
                name: "add".into(),
            },
            args: vec![],
            ty: Type::Unit,
        };
        let ordinary = Instr::Call {
            result: ValueId(1),
            callee: def(7),
            instance: vec![],
            args: vec![],
            ty: Type::Unit,
        };
        assert!(matches!(imported, Instr::ImportCall { .. }));
        assert!(matches!(ordinary, Instr::Call { .. }));
        let Instr::ImportCall { import, .. } = &imported else {
            unreachable!()
        };
        assert_eq!(import.qualified(), "pw:host/carts#add");
    }

    /// **A call carries no authority.** The capability is a property of the
    /// IMPORT, and this is the type-level statement of the 2026-08-20 ruling:
    /// a capability authorizes an operation and does not identify one.
    #[test]
    fn authority_lives_on_the_import_and_not_on_the_call() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/backend/ir.rs"),
        )
        .expect("ir.rs");
        let decl = src
            .split("pub enum Instr {")
            .nth(1)
            .expect("the Instr enum")
            .split("\n}")
            .next()
            .expect("its body");
        assert!(
            !decl.contains("capability: CapabilityId"),
            "no instruction may carry a capability: an import does"
        );
        assert!(decl.contains("import: ImportId"));
    }
}
