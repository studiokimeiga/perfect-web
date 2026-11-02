//! **E9-V — the ordinary value relations.**
//!
//! Architect ruling, 2026-08-21, reopening E9:
//!
//! > A milestone called *permanent value type checker* cannot honestly remain
//! > complete while `takes_str(42)`, `fn wrong_return() -> String { 42 }` and
//! > `takes_store(makes_cart())` all pass. That is not an edge case. It is a
//! > missing central relation.
//!
//! This module is that relation. It answers, for every place a value meets a
//! declared type, whether the value has that type:
//!
//! ```text
//! Arity      a call supplies exactly the arguments its callee declares  PW0604
//! Argument   each argument has its parameter's type                     PW0605
//! Field      each constructed field has its declared type               PW0605
//! Return     a body produces its declared result                         PW0606
//! Binding    `let x: T = e` initialises `x` with a `T`                  PW0607
//! Annotation every written type names a type visible from where it is    PW0026
//! ```
//!
//! # Three-valued, like every analysis here
//!
//! A relation is [`Outcome::Agree`], [`Outcome::Disagree`] or
//! [`Outcome::Undecided`]. Undecided is not agreement and is never reported as
//! a violation: it says the program does not state enough for this analysis to
//! decide. Diagnostics are a **projection** of [`relations`], so an audit can
//! count what was decided rather than infer it from silence — the hole
//! `MatchOutcome::Blocked` closed for exhaustiveness.
//!
//! # Identity, never representation
//!
//! `opaque type Tag = String` in two modules is two types, and a `Tag` is not
//! a `String`. Every comparison goes through the resolved identity
//! ([`crate::resolved::TypeKey`]): a DefId for a declaration, the binder and
//! position for a type parameter. Nothing here reads a spelling to decide.
//!
//! # Generic callees are instantiated per call
//!
//! `fn first<T>(items: List<T>) -> Option<T>` has one signature and a fresh
//! `T` at every call. The callee's own parameters become inference variables,
//! the arguments are unified against them left to right, and the result is the
//! declared result under that substitution. A variable no argument determined
//! leaves the result's corresponding part unknown — never guessed.
//!
//! Inside a generic body its own parameters are **rigid**: `fn id<T>(x: T) ->
//! T { 42 }` is refused, because nothing makes `42` a `T`.
//!
//! # What it deliberately does not decide
//!
//! Recorded in `docs/KNOWN_LIMITATIONS.md` rather than approximated here: a
//! call with named arguments is Undecided because a signature does not carry
//! parameter names. A sum type's case is typed where it is written through its
//! type, `Shape.Circle(3)`, since ADR-0059; the workspace still resolves no
//! case as a term.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Detector, Diagnostic};
use crate::hir::{
    BinOp, Body, Decl, DeclId, DeclKind, Expr, ExprId, Hir, Literal, NodeId, Pattern, Span, UnOp,
};
use crate::lexical::{Binder, Lexical};
use crate::resolve::{DefId, Namespace, Resolution, UnitId, Workspace};
use crate::resolved::{Builtin, Primitive, ResolvedType, TypeKey, TypeResolution};
use crate::signatures::{Receiver, Signature, Signatures};

// --- the type inference holds -------------------------------------------------

/// **A type as inference knows it**: a resolved identity, with holes where
/// the program does not say.
///
/// Not a second semantic type. Every non-hole node is a projection of a
/// [`ResolvedType`]'s [`TypeKey`] — the same identity `same_as` compares —
/// and [`Ty::Unknown`] marks exactly the parts no declaration determined.
/// `Result<Int, ?>` is what `Ok(1)` is, and forcing it into a `ResolvedType`
/// would mean inventing the `?`.
///
/// No derived comparison, for `ResolvedType`'s reason: [`unify`] is the only
/// relation, and a second one would be free to disagree with it.
#[derive(Debug, Clone)]
pub enum Ty {
    Primitive(Primitive),
    Builtin(Builtin, Vec<Ty>),
    Nominal(DefId, Vec<Ty>),
    /// A type parameter, rigid: the enclosing declaration's own `T`.
    Parameter {
        binder: DefId,
        index: u32,
    },
    /// An inference variable, alive only while one call is being solved.
    Var(u32),
    /// The program does not say.
    Unknown,
    /// **Any type: the value holds at every one** (ADR-0065). `None` is an
    /// `Option` of anything, `[]` a `List` of anything, `todo` any value,
    /// and `Maybe.Nothing` a `Maybe` of anything. It agrees with every type,
    /// where [`Ty::Unknown`], which the program does not state, decides
    /// nothing. Never a generic function's unknown result, which one of its
    /// parameters may link to another: that stays unknown.
    Any,
}

impl Ty {
    /// Does this mention a type parameter of a declaration other than `own`:
    /// a callee's `T` that no call instantiated?
    fn mentions_foreign_parameter(&self, own: Option<DefId>) -> bool {
        match self {
            Ty::Parameter { binder, .. } => Some(*binder) != own,
            Ty::Builtin(_, args) | Ty::Nominal(_, args) => {
                args.iter().any(|a| a.mentions_foreign_parameter(own))
            }
            _ => false,
        }
    }

    pub fn of(t: &ResolvedType) -> Ty {
        Ty::from_key(&t.semantic_key())
    }

    fn from_key(k: &TypeKey) -> Ty {
        match k {
            TypeKey::Primitive(p) => Ty::Primitive(*p),
            TypeKey::Builtin(b, args) => Ty::Builtin(*b, args.iter().map(Ty::from_key).collect()),
            TypeKey::Nominal(d, args) => Ty::Nominal(*d, args.iter().map(Ty::from_key).collect()),
            TypeKey::Parameter { binder, index } => Ty::Parameter {
                binder: *binder,
                index: *index,
            },
        }
    }

    fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    /// **What it is, where it is known whole** (ADR-0226): `None` where any
    /// part of it is unknown, any type, a variable or a parameter.
    pub(crate) fn key(&self) -> Option<TypeKey> {
        let all = |args: &[Ty]| args.iter().map(Ty::key).collect::<Option<Vec<_>>>();
        Some(match self {
            Ty::Primitive(p) => TypeKey::Primitive(*p),
            Ty::Builtin(b, args) => TypeKey::Builtin(*b, all(args)?),
            Ty::Nominal(d, args) => TypeKey::Nominal(*d, all(args)?),
            Ty::Parameter { .. } | Ty::Var(_) | Ty::Unknown | Ty::Any => return None,
        })
    }

    /// Fully known: no hole and no inference variable anywhere in it.
    fn is_closed(&self) -> bool {
        match self {
            Ty::Unknown | Ty::Var(_) => false,
            Ty::Builtin(_, args) | Ty::Nominal(_, args) => args.iter().all(Ty::is_closed),
            _ => true,
        }
    }

    /// The constructor a member lookup keys on. A hole in an argument does not
    /// change which members a `List<?>` has.
    pub(crate) fn receiver(&self) -> Option<Receiver> {
        match self {
            Ty::Primitive(p) => Some(Receiver::Primitive(*p)),
            Ty::Builtin(b, _) => Some(Receiver::Builtin(*b)),
            Ty::Nominal(d, _) => Some(Receiver::Nominal(*d)),
            _ => None,
        }
    }

    /// Replace the given binder's parameters with fresh variables numbered by
    /// position. Every other node is kept.
    fn instantiate(&self, binder: DefId) -> Ty {
        match self {
            Ty::Parameter { binder: b, index } if *b == binder => Ty::Var(*index),
            Ty::Builtin(c, args) => {
                Ty::Builtin(*c, args.iter().map(|a| a.instantiate(binder)).collect())
            }
            Ty::Nominal(d, args) => {
                Ty::Nominal(*d, args.iter().map(|a| a.instantiate(binder)).collect())
            }
            other => other.clone(),
        }
    }
}

// --- unification ---------------------------------------------------------------

/// The verdict of one relation between an expected and an actual type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Agree,
    Disagree,
    /// Some part the relation depends on is unknown. Not agreement.
    Undecided,
}

impl Verdict {
    fn and(self, other: Verdict) -> Verdict {
        match (self, other) {
            (Verdict::Disagree, _) | (_, Verdict::Disagree) => Verdict::Disagree,
            (Verdict::Undecided, _) | (_, Verdict::Undecided) => Verdict::Undecided,
            _ => Verdict::Agree,
        }
    }
}

/// One call's substitution. Created per call and dropped after it, so a
/// variable can never leak into another expression's type.
#[derive(Debug, Default)]
struct Subst {
    bound: BTreeMap<u32, Ty>,
    /// Variables met by a value of any type and by nothing that fixes one:
    /// `T` in `List.get([], 0)`. Each closes to [`Ty::Any`] (ADR-0065).
    any: BTreeSet<u32>,
    /// How many variables [`Subst::holes`] has made, numbered from
    /// [`HOLES`], above any callee's type parameters.
    holes: u32,
}

/// The first variable a part of any type is given (ADR-0283). A callee's own
/// type parameters are its variables `0..n`.
const HOLES: u32 = 1 << 30;

impl Subst {
    fn resolve(&self, t: &Ty) -> Ty {
        match t {
            Ty::Var(v) => match self.bound.get(v) {
                Some(b) => self.resolve(b),
                None => Ty::Var(*v),
            },
            other => other.clone(),
        }
    }

    /// Apply the substitution, turning every variable nothing determined into
    /// a hole. The only way a type leaves a call.
    fn close(&self, t: &Ty) -> Ty {
        match self.resolve(t) {
            Ty::Var(v) if self.any.contains(&v) => Ty::Any,
            Ty::Var(_) => Ty::Unknown,
            Ty::Builtin(c, args) => Ty::Builtin(c, args.iter().map(|a| self.close(a)).collect()),
            Ty::Nominal(d, args) => Ty::Nominal(d, args.iter().map(|a| self.close(a)).collect()),
            other => other,
        }
    }

    /// **A type a variable is bound to, each part of any type a variable of
    /// its own** (ADR-0283): what else the call meets may fix one, and one
    /// nothing fixes closes to [`Ty::Any`], as it was. A fold's accumulator
    /// met by `[]` was bound to `List<_>` whole, so the function's result fixed
    /// nothing and agreed with it whatever list it built:
    /// `List.fold(xs, [], (out, x) => List.concat(out, [1]))` was a
    /// `List<String>` where one was declared.
    fn holes(&mut self, t: &Ty) -> Ty {
        match t {
            Ty::Any => {
                let v = HOLES + self.holes;
                self.holes += 1;
                self.any.insert(v);
                Ty::Var(v)
            }
            Ty::Builtin(c, args) => Ty::Builtin(*c, args.iter().map(|a| self.holes(a)).collect()),
            Ty::Nominal(d, args) => Ty::Nominal(*d, args.iter().map(|a| self.holes(a)).collect()),
            other => other.clone(),
        }
    }

    fn occurs(&self, v: u32, t: &Ty) -> bool {
        match self.resolve(t) {
            Ty::Var(w) => w == v,
            Ty::Builtin(_, args) | Ty::Nominal(_, args) => args.iter().any(|a| self.occurs(v, a)),
            _ => false,
        }
    }
}

/// **The value relation**: may a value of `actual` stand where `expected` is
/// declared?
///
/// By identity, recursively through arguments. Two nominal types are the same
/// type only when they are the same declaration applied to the same
/// arguments; a primitive is only itself; a rigid type parameter is only
/// itself. A variable is bound by the first thing it meets.
fn unify(s: &mut Subst, expected: &Ty, actual: &Ty) -> Verdict {
    let (e, a) = (s.resolve(expected), s.resolve(actual));
    match (&e, &a) {
        (Ty::Var(v), Ty::Var(w)) if v == w => Verdict::Agree,
        // A value of any type fixes no variable: `List.concat([], ["a"])`
        // is a `List<String>`, whichever side comes first (ADR-0065).
        (Ty::Var(v), Ty::Any) | (Ty::Any, Ty::Var(v)) => {
            s.any.insert(*v);
            Verdict::Agree
        }
        (Ty::Var(v), t) | (t, Ty::Var(v)) => {
            if t.is_unknown() || s.occurs(*v, t) {
                return Verdict::Undecided;
            }
            let t = s.holes(t);
            s.bound.insert(*v, t);
            Verdict::Agree
        }
        (Ty::Any, _) | (_, Ty::Any) => Verdict::Agree,
        (Ty::Unknown, _) | (_, Ty::Unknown) => Verdict::Undecided,
        (
            Ty::Parameter {
                binder: b1,
                index: i1,
            },
            Ty::Parameter {
                binder: b2,
                index: i2,
            },
        ) => match b1 == b2 && i1 == i2 {
            true => Verdict::Agree,
            false => Verdict::Disagree,
        },
        (Ty::Primitive(p), Ty::Primitive(q)) => match p == q {
            true => Verdict::Agree,
            false => Verdict::Disagree,
        },
        (Ty::Builtin(c1, a1), Ty::Builtin(c2, a2)) => {
            if c1 != c2 || a1.len() != a2.len() {
                return Verdict::Disagree;
            }
            all(s, a1, a2)
        }
        (Ty::Nominal(d1, a1), Ty::Nominal(d2, a2)) => {
            if d1 != d2 {
                return Verdict::Disagree;
            }
            // One declaration applied to a different number of arguments is
            // a malformed type, which resolution refuses; reaching here with
            // one means an upstream stage did not, and that is not this
            // relation's verdict to give.
            if a1.len() != a2.len() {
                return Verdict::Undecided;
            }
            all(s, a1, a2)
        }
        _ => Verdict::Disagree,
    }
}

fn all(s: &mut Subst, expected: &[Ty], actual: &[Ty]) -> Verdict {
    let mut verdict = Verdict::Agree;
    for (e, a) in expected.iter().zip(actual) {
        verdict = verdict.and(unify(s, e, a));
        if verdict == Verdict::Disagree {
            break;
        }
    }
    verdict
}

/// The information in both, or a hole where they conflict. For the branches
/// of an `if` or a `match`, whose value is whichever one ran.
fn join(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, t) | (t, Ty::Unknown) => t,
        (Ty::Any, t) | (t, Ty::Any) => t,
        (Ty::Primitive(p), Ty::Primitive(q)) if p == q => Ty::Primitive(p),
        (Ty::Builtin(c1, a1), Ty::Builtin(c2, a2)) if c1 == c2 && a1.len() == a2.len() => {
            Ty::Builtin(
                c1,
                a1.into_iter().zip(a2).map(|(x, y)| join(x, y)).collect(),
            )
        }
        (Ty::Nominal(d1, a1), Ty::Nominal(d2, a2)) if d1 == d2 && a1.len() == a2.len() => {
            Ty::Nominal(
                d1,
                a1.into_iter().zip(a2).map(|(x, y)| join(x, y)).collect(),
            )
        }
        (
            Ty::Parameter {
                binder: b1,
                index: i1,
            },
            Ty::Parameter {
                binder: b2,
                index: i2,
            },
        ) if b1 == b2 && i1 == i2 => Ty::Parameter {
            binder: b1,
            index: i1,
        },
        _ => Ty::Unknown,
    }
}

// --- what the analysis reports ---------------------------------------------------

/// Which relation a record is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RelationKind {
    Arity,
    Argument,
    Field,
    Return,
    Binding,
    Annotation,
    /// An operator's operand, or an `if`'s condition, against the type it
    /// takes (PW0609).
    Operand,
    /// `x = e`: `e` against the type `x` holds (PW0607, ADR-0051).
    Assignment,
    /// `value.name`: the member a read or a call names, against the members
    /// the value's type has (PW0610, ADR-0048).
    Member,
    /// `Shape.Circle`: the case a path names, against the cases its type
    /// declares (PW0608, ADR-0059).
    Case,
    /// `Empty`, a case written alone that several types have: the type
    /// expected where it is written, against the types that have it (PW0022,
    /// ADR-0201).
    BareCase,
    /// `Box { value: 1, label: "a" }`: the fields a record is built with,
    /// against the fields its type declares, each once (PW0612, ADR-0067).
    Fields,
    /// The branches of an `if` or a `match` whose value is used, against each
    /// other (PW0613, ADR-0068).
    Branches,
    /// `f(x)`: a called value, against a function type (PW0614, ADR-0068).
    Callee,
    /// `[a, b]`: a list's items, against each other (PW0615, ADR-0069).
    Items,
    /// `9223372036854775808`: an `Int` literal, against the range an `Int`
    /// holds (PW0616, ADR-0069).
    Literal,
    /// `on:press={h}`: an event attribute's value, against a function to call
    /// (PW0614, ADR-0071).
    Handler,
    /// `{#if c}`: a template's condition, against a value that has a truth
    /// (PW0609, ADR-0071).
    Truth,
    /// `{v}`, `title={v}`, a loop's key: a value a template writes as text,
    /// against a type that has a text form (PW0609, ADR-0074).
    Text,
    /// `{o}` or `{:else if o}` over an `Option` or a `Result`: a value that may
    /// be absent, which a template takes apart with `{#match}` (PW0600,
    /// ADR-0074).
    Absent,
    /// `f(z = 1)`: a named argument, against the parameters the callee names
    /// (PW0617, ADR-0081).
    Named,
    /// `<MenuRow item={item} />`: the props a view is given, against its
    /// parameters, each once and nothing else (PW0619, ADR-0136). Each prop's
    /// type is an `Argument` relation.
    Props,
    /// `PositiveInt(n)`: the value an opaque type is built from, against the
    /// invariant the type states, as far as the build can bound it (PW0622,
    /// ADR-0179).
    Invariant,
    /// `optimistic Thing(x) as t => e`: a transition's value, against the
    /// value of the entry it replaces (PW0331, ADR-0241).
    Transition,
    /// `Map<K, V>`, `Set<K>`: the key, against the types that have an order
    /// every backend shares (PW0627, ADR-0248).
    MapKey,
}

/// What a map's key may be, for a message (ADR-0248).
const MAP_KEYS: &str = "an `Int`, a `String`, a `Bool`, or an opaque type over one";

/// **Is `key` a map's key** (ADR-0248, ruling 0057-a): an `Int`, a `String`,
/// a `Bool`, or an opaque type over one, each ordered as its representation:
/// an `Int` by value, a `String` by code point, `false` before `true`.
/// `None` where it is not known: a type parameter's, which each
/// instantiation is held to, or what the program does not say.
fn map_key(sigs: &Signatures, key: &Ty) -> Option<bool> {
    let ordered = |t: &Ty| {
        matches!(
            t,
            Ty::Primitive(Primitive::Int | Primitive::Str | Primitive::Bool)
        )
    };
    match key {
        Ty::Nominal(def, _) => Some(
            sigs.type_decl(*def)
                .and_then(|d| d.representation.as_ref())
                .is_some_and(|r| matches!(r, TypeResolution::Resolved(t) if ordered(&Ty::of(t)))),
        ),
        Ty::Primitive(_) | Ty::Builtin(..) => Some(ordered(key)),
        Ty::Parameter { .. } | Ty::Var(_) | Ty::Unknown | Ty::Any => None,
    }
}

/// The key of each map and set in `ty`, outermost first, that is no map's
/// key.
fn unordered_keys(sigs: &Signatures, ty: &Ty, out: &mut Vec<Ty>) {
    if let Ty::Builtin(Builtin::Map | Builtin::Set, args) = ty
        && let Some(key) = args.first()
        && map_key(sigs, key) == Some(false)
    {
        out.push(key.clone());
    }
    if let Ty::Builtin(_, args) | Ty::Nominal(_, args) = ty {
        for a in args {
            unordered_keys(sigs, a, out);
        }
    }
}

/// Does `t` hold a map or a set keyed by a type parameter, which a call
/// instantiates?
fn keyed_by_a_parameter(t: &ResolvedType) -> bool {
    matches!(t.as_builtin(), Some(Builtin::Map | Builtin::Set))
        && t.args()
            .first()
            .is_some_and(|k| k.type_parameter().is_some())
        || t.args().iter().any(keyed_by_a_parameter)
}

/// A type as a diagnostic shows it where no body's typer is at hand
/// (ADR-0248): a written annotation's.
fn shown(sigs: &Signatures, t: &Ty) -> String {
    let args = |a: &[Ty]| {
        a.iter()
            .map(|t| shown(sigs, t))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match t {
        Ty::Primitive(p) => p.name().to_string(),
        Ty::Builtin(b, a) => format!("{}<{}>", b.name(), args(a)),
        Ty::Nominal(d, a) => {
            let base = sigs.path_of(*d).unwrap_or("?").to_string();
            match a.is_empty() {
                true => base,
                false => format!("{base}<{}>", args(a)),
            }
        }
        Ty::Parameter { index, .. } => format!("type parameter {index}"),
        Ty::Var(_) | Ty::Unknown => "?".to_string(),
        Ty::Any => "_".to_string(),
    }
}

/// A fields relation's expectation for a field its type does not declare.
const FIELD_UNDECLARED: &str = "a field its type declares";
/// A fields relation's expectation for a field given more than once.
const FIELD_ONCE: &str = "each field once";
/// A fields relation's expectation for a field left out.
const FIELD_GIVEN: &str = "every field its type declares";

/// A member relation's expectation when the member is an opaque type's
/// representation, read outside the module that declares the type.
const PRIVATE_REPRESENTATION: &str = "its representation, which only its own module reads";

/// A member relation's expectation when the member is the contents' of an
/// `Option` or a `Result`, read from the container (PW0600).
const OPTIONAL_CONTENTS: &str = "its contents, taken apart first";

/// A member relation's expectation, before the declarations it could mean,
/// when several modules declare it and this one cannot tell which
/// (ADR-0254, PW0628).
const AMBIGUOUS_MEMBER: &str = "one of ";

/// Why a relation was not decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Undecided {
    /// Part of the value's type is not stated by anything the program declares.
    Unknown,
    /// The declared type did not resolve; the relation refuses to run on it.
    ExpectedUnresolved,
    /// The call has the wrong number of arguments; `Arity` owns that.
    ArityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Agree,
    /// What was declared and what was found, as a diagnostic shows them.
    Disagree {
        expected: String,
        actual: String,
    },
    Undecided(Undecided),
}

/// **One place a value meets a declared type**, and what was concluded.
#[derive(Debug, Clone)]
pub struct ValueRelation {
    /// The declaration the relation is written in.
    pub declaration: String,
    pub kind: RelationKind,
    /// Where the value (or the annotation) is.
    pub span: Span,
    /// What it is checked against: a callee, a field, a declaration.
    pub target: String,
    /// The argument or field position, where there is one.
    pub index: Option<usize>,
    pub outcome: Outcome,
    /// Where the expectation comes from, when it has a place in source: the
    /// unit it is written in, and where. A span is only meaningful against its
    /// own file, and a diagnostic's related spans are rendered against the
    /// file the diagnostic is in.
    pub declared_at: Option<(UnitId, Span)>,
    /// The boundary the relation sits on, in this unit: the call, the
    /// declaration whose result is checked, the binding. Charter §16.3 asks
    /// every diagnostic for one, and it is always in the file the diagnostic
    /// is about, where a callee's own declaration may not be.
    pub boundary: (Span, String),
}

// --- the typer ---------------------------------------------------------------------

/// What a call resolves to.
pub(crate) enum Target<'s> {
    /// `f(a, b)` and `m.f(a, b)`: every supplied argument is a parameter.
    Callable(&'s Signature),
    /// `x.f(a)`: `x` is the first parameter.
    Member(&'s Signature, ExprId),
    /// `Cart(lines, total)`: a record's fields, positionally.
    Record(DefId),
    /// `PositiveInt(1)`: an opaque type built from its representation.
    Opaque(DefId),
    /// `Shape.Circle(3)`: a sum type's case, by its position in the
    /// declaration, built from its payload's fields positionally (ADR-0059).
    Case(DefId, usize),
}

struct Typer<'a> {
    sigs: &'a Signatures,
    ws: &'a Workspace,
    at: UnitId,
    module: Option<&'a str>,
    decl: &'a Decl,
    body: &'a Body,
    /// The binding each local name means (ADR-0063).
    lexical: Lexical,
    /// Every binding given a known type in this body, by where it is bound,
    /// never by its name: two bindings of one name are two entries
    /// (ADR-0063). A cell, because typing a lambda against the function type
    /// it is passed as binds its parameters for the duration of that one
    /// question.
    locals: RefCell<BTreeMap<Binder, Ty>>,
    /// The expression each `|>` feeds, keyed by the call on its right.
    piped: BTreeMap<ExprId, ExprId>,
    /// Lambda parameters given a type while their use was solved, known
    /// whole or in part, waiting to join `locals` (see
    /// [`Typer::solve_lambdas`]). A part a later round knows completes it
    /// ([`Typer::bind`]).
    solved: RefCell<BTreeMap<Binder, Ty>>,
    /// The `if`s and `match`es whose value is used (ADR-0068).
    used: BTreeSet<ExprId>,
    /// Each template attribute written as a string with holes: its holes are
    /// `template_relations`', which names the attribute (ADR-0084).
    attribute_strings: BTreeSet<ExprId>,
    /// Each expression's parent, for the `if`s a construction is inside
    /// (ADR-0179).
    parents: BTreeMap<ExprId, ExprId>,
    /// Each `let`'s initializer, by the pattern it binds (ADR-0179).
    lets: BTreeMap<crate::hir::PatternId, ExprId>,
    /// The expressions whose expected type is being asked, so a question
    /// that leads back to itself, `Empty == Empty`, answers nothing rather
    /// than loops (ADR-0201).
    expecting: RefCell<BTreeSet<ExprId>>,
}

/// **The integers a value can be, as far as the build can bound it**
/// (ADR-0179): each end included, or open where nothing bounds it. Exact over
/// `Int`, since both backends trap where an `Int` operation overflows: a value
/// that is computed at all is the integer the expression means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    lo: Option<i128>,
    hi: Option<i128>,
}

impl Interval {
    const ANY: Interval = Interval { lo: None, hi: None };

    fn exactly(k: i128) -> Interval {
        Interval {
            lo: Some(k),
            hi: Some(k),
        }
    }

    fn add(self, o: Interval) -> Interval {
        Interval {
            lo: self.lo.zip(o.lo).and_then(|(a, b)| a.checked_add(b)),
            hi: self.hi.zip(o.hi).and_then(|(a, b)| a.checked_add(b)),
        }
    }

    fn neg(self) -> Interval {
        Interval {
            lo: self.hi.and_then(i128::checked_neg),
            hi: self.lo.and_then(i128::checked_neg),
        }
    }

    /// The least and the greatest of the four products of the ends, an open
    /// end an infinity: `[1, ∞) × 2` is `[2, ∞)`. The values are integers, so
    /// an end of 0 times an infinity is 0.
    fn mul(self, o: Interval) -> Interval {
        #[derive(Clone, Copy, PartialEq, PartialOrd)]
        enum End {
            Below,
            At(i128),
            Above,
        }
        let low = |b: Option<i128>| b.map_or(End::Below, End::At);
        let high = |b: Option<i128>| b.map_or(End::Above, End::At);
        let times = |a: End, b: End| -> Option<End> {
            Some(match (a, b) {
                (End::At(x), End::At(y)) => End::At(x.checked_mul(y)?),
                (End::At(0), _) | (_, End::At(0)) => End::At(0),
                (End::At(x), far) | (far, End::At(x)) => match (x > 0) == (far == End::Above) {
                    true => End::Above,
                    false => End::Below,
                },
                (a, b) => match a == b {
                    true => End::Above,
                    false => End::Below,
                },
            })
        };
        let corners = [
            times(low(self.lo), low(o.lo)),
            times(low(self.lo), high(o.hi)),
            times(high(self.hi), low(o.lo)),
            times(high(self.hi), high(o.hi)),
        ];
        let Some(corners) = corners.into_iter().collect::<Option<Vec<End>>>() else {
            return Interval::ANY;
        };
        let least = corners
            .iter()
            .copied()
            .fold(End::Above, |a, b| if b < a { b } else { a });
        let most = corners
            .iter()
            .copied()
            .fold(End::Below, |a, b| if b > a { b } else { a });
        Interval {
            lo: match least {
                End::At(x) => Some(x),
                _ => None,
            },
            hi: match most {
                End::At(x) => Some(x),
                _ => None,
            },
        }
    }

    /// Either: the value of a branch, one of two.
    fn hull(self, o: Interval) -> Interval {
        Interval {
            lo: self.lo.zip(o.lo).map(|(a, b)| a.min(b)),
            hi: self.hi.zip(o.hi).map(|(a, b)| a.max(b)),
        }
    }

    /// Both: what a test established of a value, and what it was.
    fn meet(self, o: Interval) -> Interval {
        let pick = |a: Option<i128>, b: Option<i128>, f: fn(i128, i128) -> i128| match (a, b) {
            (Some(x), Some(y)) => Some(f(x, y)),
            (x, None) => x,
            (None, y) => y,
        };
        Interval {
            lo: pick(self.lo, o.lo, i128::max),
            hi: pick(self.hi, o.hi, i128::min),
        }
    }

    fn within(self, inv: &crate::hir::Invariant) -> bool {
        let above = inv.at_least.is_none_or(|b| self.lo.is_some_and(|l| l >= b));
        let below = inv.at_most.is_none_or(|b| self.hi.is_some_and(|h| h <= b));
        above && below
    }

    /// As a diagnostic says it: "at least 0", "from -3 to 5", "any `Int`".
    fn described(self) -> String {
        match (self.lo, self.hi) {
            (Some(a), Some(b)) if a == b => format!("{a}"),
            (Some(a), Some(b)) => format!("from {a} to {b}"),
            (Some(a), None) => format!("at least {a}"),
            (None, Some(b)) => format!("at most {b}"),
            (None, None) => "any `Int`".to_string(),
        }
    }

    /// A length, as a diagnostic says it (ADR-0225): "12 code points long",
    /// "a `String` of any length".
    fn described_length(self) -> String {
        match (self.lo.map(|l| l.max(0)), self.hi) {
            (Some(a), Some(b)) if a == b => format!("{a} code points long"),
            (Some(a), Some(b)) => format!("from {a} to {b} code points long"),
            (Some(0) | None, None) => "a `String` of any length".to_string(),
            (Some(a), None) => format!("at least {a} code points long"),
            (None, Some(b)) => format!("at most {b} code points long"),
        }
    }

    /// What `x OP k` makes of `x`, holding or not: `>=` and `1` give
    /// "at least 1", and not holding, "at most 0".
    fn compared(op: &str, k: i128, holds: bool) -> Option<Interval> {
        let op = match (op, holds) {
            (op, true) => op,
            ("<", false) => ">=",
            ("<=", false) => ">",
            (">", false) => "<=",
            (">=", false) => "<",
            ("==", false) => "!=",
            ("!=", false) => "==",
            _ => return None,
        };
        Some(match op {
            "<" => Interval {
                lo: None,
                hi: k.checked_sub(1),
            },
            "<=" => Interval {
                lo: None,
                hi: Some(k),
            },
            ">" => Interval {
                lo: k.checked_add(1),
                hi: None,
            },
            ">=" => Interval {
                lo: Some(k),
                hi: None,
            },
            "==" => Interval::exactly(k),
            _ => return None,
        })
    }
}

/// A place a value is read from that a test can narrow: a local, and the
/// fields read through it, `n.value`.
type Path = (Binder, Vec<String>);

/// The name a `String`'s length is read through in a [`Path`] (ADR-0225):
/// no field is named so, since no name begins with `#`.
const LENGTH: &str = "#length";

/// What a resolved call is checked against: its parameters, where each is
/// declared, and its result, all under the binder whose type parameters the
/// call instantiates.
struct Callee {
    name: String,
    binder: DefId,
    params: Vec<Option<TypeResolution>>,
    declared: Vec<Option<(UnitId, Span)>>,
    result: Option<Ty>,
}

/// What resolving and solving one call produced.
struct Solved {
    result: Ty,
    relations: Vec<ValueRelation>,
}

/// **The type the value relations give one expression, and its name.**
///
/// For an analysis that needs a type where no annotation says one: the
/// exhaustiveness check's scrutinee, when it is a call rather than a declared
/// name (`match get(w) { .. }`). `Ty::Unknown` where the typer does not know,
/// never a guess.
pub(crate) fn type_of(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    module: Option<&str>,
    decl: &Decl,
    body: &Body,
    e: ExprId,
) -> (Ty, String) {
    BodyTypes::new(sigs, ws, at, module, decl, body).of(e)
}

/// **One body's expressions, typed by one typer** (the checker's latency,
/// 2026-10-13). A typer solves its whole body as it is built, and `type_of`
/// builds one for the one expression it is asked about: an analysis that
/// asked about every value a page prints built one for each, and a page of
/// a few hundred holes solved itself a few hundred times.
pub(crate) struct BodyTypes<'a> {
    typer: Typer<'a>,
}

impl<'a> BodyTypes<'a> {
    pub(crate) fn new(
        sigs: &'a Signatures,
        ws: &'a Workspace,
        at: UnitId,
        module: Option<&'a str>,
        decl: &'a Decl,
        body: &'a Body,
    ) -> BodyTypes<'a> {
        let lexical = Lexical::build(decl, body);
        BodyTypes {
            typer: Typer::new(sigs, ws, at, module, decl, body, lexical, Vec::new()),
        }
    }

    /// The type the value relations give `e`, and its name.
    pub(crate) fn of(&self, e: ExprId) -> (Ty, String) {
        let t = self.typer.of(e);
        let name = self.typer.display(&t);
        (t, name)
    }
}

/// What a call's written path names.
pub(crate) enum Named<'s> {
    /// The callee is not a path, or its path names nothing. `x.f(a)` may still
    /// be a member call.
    Nothing,
    /// It names something ambiguously, or names something a call cannot
    /// target.
    Refused,
    Target(Target<'s>),
}

/// **Which declaration a call's path names.** The one rule, with two readers:
/// the typer, and the handler backend (`backend::js`). A handler compiled to
/// call a different declaration than the one the checker checked would run
/// unchecked code, and nothing downstream would notice.
///
/// A dotted path resolves as a path. A bare name is a term first, and a type
/// only where no term has that name: `PositiveInt(1)` builds an opaque value,
/// and `Store(id)` calls the query `Store` even where a type `Store` exists
/// too (A-003).
/// Expression lookup shares the canonical lexical namespace with typing.
pub(crate) fn named_at<'s>(
    sigs: &'s Signatures,
    ws: &Workspace,
    at: UnitId,
    body: &Body,
    lexical: &Lexical,
    callee: ExprId,
) -> Named<'s> {
    if !lexical.unbound_path(body, callee) {
        return Named::Nothing;
    }
    named(sigs, ws, at, body, callee)
}

fn named<'s>(
    sigs: &'s Signatures,
    ws: &Workspace,
    at: UnitId,
    body: &Body,
    callee: ExprId,
) -> Named<'s> {
    let path = path_of(body, callee);
    if path.is_empty() {
        return Named::Nothing;
    }
    let term = match path.contains('.') {
        true => ws.resolve_path(at, &path),
        false => ws.resolve_in(at, Namespace::Term, &path),
    };
    let def = match term {
        Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
        // Ambiguity is its own diagnostic; never a type verdict.
        Resolution::Ambiguous(_) => return Named::Refused,
        Resolution::Unresolved if !path.contains('.') => {
            match ws.resolve_in(at, Namespace::Type, &path) {
                Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
                // `Circle(3)` alone: the case of the one type this unit sees
                // with it, as `Empty` alone is (ADR-0198).
                _ => {
                    return match bare_case(sigs, ws, at, &path) {
                        Some((def, index)) => Named::Target(Target::Case(def, index)),
                        None => Named::Nothing,
                    };
                }
            }
        }
        // `Shape.Circle(3)`, a case through its type (ADR-0059). A case its
        // type lacks is refused here and reported by the case relation.
        Resolution::Unresolved => {
            return match case_named(sigs, ws, at, &path) {
                Some(CaseNamed::Case(def, index)) => Named::Target(Target::Case(def, index)),
                Some(CaseNamed::Unknown(..)) => Named::Refused,
                None => Named::Nothing,
            };
        }
    };
    if let Some(sig) = sigs.by_def(def) {
        return Named::Target(Target::Callable(sig));
    }
    let Some(facts) = sigs.type_decl(def) else {
        return Named::Refused;
    };
    if facts.record.is_some() {
        return Named::Target(Target::Record(def));
    }
    if facts.representation.is_some() {
        return Named::Target(Target::Opaque(def));
    }
    Named::Refused
}

/// **The value the member `sig` gives, read from a value of type
/// `receiver`** without a call: a record's field under the receiver's
/// arguments, or a function whose only parameter is the receiver, read as a
/// property (`self.style`, `cart.line_count`, ADR-0125), its result under
/// what the receiver gives its type parameters. `Unknown` where a part is
/// not known: a function with more parameters, named without a call, is a
/// function value, which has no type here.
///
/// The one rule for both typers: the value relations' [`Typer`] reads a
/// member by it, and so does `infer.rs` (ADR-0351, the generic-effects
/// track), which until then gave `Box<Item>.v`, where `v: T`, the type `T`,
/// and a member read on it named nothing.
pub(crate) fn member_value(sigs: &Signatures, sig: &Signature, receiver: &Ty) -> Ty {
    // A callable member whose only parameter is the receiver is read as a
    // property — `self.style`, `snapshot.value` — exactly as `infer.rs`
    // resolves it.
    if sigs.by_def(sig.definition).is_some() {
        let Some(recv) = sigs.property_receiver(sig) else {
            return Ty::Unknown;
        };
        let Some(TypeResolution::Resolved(result)) = &sig.returns else {
            return Ty::Unknown;
        };
        let mut s = Subst::default();
        let binder = sig.definition;
        if unify(&mut s, &Ty::of(recv).instantiate(binder), receiver) == Verdict::Disagree {
            return Ty::Unknown;
        }
        return s.close(&Ty::of(result).instantiate(binder));
    }
    let (Some(Some(TypeResolution::Resolved(recv))), Some(result)) =
        (sig.params.first(), sig.result())
    else {
        return Ty::Unknown;
    };
    // `Box<Int>.v` where `v: T`: the field's type under the receiver's
    // arguments, by unifying the declared receiver with the actual one.
    let mut s = Subst::default();
    let owner = sig.definition;
    unify(&mut s, &Ty::of(recv).instantiate(owner), receiver);
    s.close(&Ty::of(result).instantiate(owner))
}

/// **What the value relations' typer solves in one body** (ADR-0351, the
/// generic-effects track): the type of each expression and of each binding,
/// holes included, owned, for `infer.rs`'s [`crate::infer::Types`] to read
/// where the program's declarations leave a type it cannot state alone: a
/// generic call's result, with the callee's type parameters solved from its
/// arguments; a `for` loop's binder; a `match` arm's; a lambda's parameter
/// over a list a call computes.
#[derive(Debug, Clone, Default)]
pub(crate) struct SolvedBody {
    exprs: BTreeMap<ExprId, Ty>,
    locals: BTreeMap<Binder, Ty>,
}

impl SolvedBody {
    /// The type the typer gives `e`: `Unknown` where it does not know.
    pub(crate) fn expr(&self, e: ExprId) -> &Ty {
        self.exprs.get(&e).unwrap_or(&Ty::Unknown)
    }

    /// The type the typer gives the binding `b`: `Unknown` where it does not
    /// know.
    pub(crate) fn local(&self, b: Binder) -> &Ty {
        self.locals.get(&b).unwrap_or(&Ty::Unknown)
    }

    /// Every binding the typer gives a type.
    pub(crate) fn locals(&self) -> impl Iterator<Item = (Binder, &Ty)> {
        self.locals.iter().map(|(b, t)| (*b, t))
    }
}

/// **Solve `decl`'s body** as the value relations do, reading its names by
/// `lexical`, with `outer` the types of the bindings around a nested
/// declaration, in the order [`Lexical::outer_bindings`] numbers them
/// (ADR-0066). Nothing where `module` names no unit.
pub(crate) fn solve_body(
    sigs: &Signatures,
    module: Option<&str>,
    decl: &Decl,
    body: &Body,
    lexical: Lexical,
    outer: Vec<Ty>,
) -> SolvedBody {
    let ws = sigs.workspace();
    let Some(at) = sigs.unit_of(module) else {
        return SolvedBody::default();
    };
    let typer = Typer::new(sigs, ws, at, module, decl, body, lexical, outer);
    let exprs = body.exprs().map(|(id, _, _)| (id, typer.of(id))).collect();
    let locals = typer.locals.borrow().clone();
    SolvedBody { exprs, locals }
}

/// **`t` with `binder`'s type parameters replaced by `args`**: a declared
/// case's field under the arguments its type is applied to. A parameter no
/// argument gives is unknown.
pub(crate) fn substituted(t: &Ty, binder: DefId, args: &[Ty]) -> Ty {
    match t {
        Ty::Parameter { binder: b, index } if *b == binder => {
            args.get(*index as usize).cloned().unwrap_or(Ty::Unknown)
        }
        Ty::Builtin(c, xs) => Ty::Builtin(
            *c,
            xs.iter().map(|x| substituted(x, binder, args)).collect(),
        ),
        Ty::Nominal(d, xs) => Ty::Nominal(
            *d,
            xs.iter().map(|x| substituted(x, binder, args)).collect(),
        ),
        other => other.clone(),
    }
}

/// What a qualified path names when it names a sum type's case (ADR-0059).
pub(crate) enum CaseNamed {
    /// `Shape.Circle`: the type, and the case's position in its declaration.
    Case(DefId, usize),
    /// `Shape.Bogus`: a sum type, and a name none of its cases has.
    Unknown(DefId, String),
}

/// **Which case a qualified path names** (ADR-0059): `Shape.Circle`, or
/// `geometry.Shape.Circle` through a module. The type is resolved in the Type
/// namespace where the path is written, and the case is found among the cases
/// it declares. `None` where the path is not a sum type's: a path that
/// resolves as a term is that term.
///
/// The one rule, with three readers: the typer, the check that a case exists,
/// and the backend (`backend::lower`), so the case the checker typed is the
/// case that is built.
/// Expression case lookup excludes canonical lexical value receivers.
/// Pattern lookup deliberately uses case_named directly.
pub(crate) fn case_named_at(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    body: &Body,
    lexical: &Lexical,
    expr: ExprId,
) -> Option<CaseNamed> {
    if !lexical.unbound_path(body, expr) {
        return None;
    }
    case_named(sigs, ws, at, &path_of(body, expr))
}

pub(crate) fn case_named(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    path: &str,
) -> Option<CaseNamed> {
    let (ty, case) = path.rsplit_once('.')?;
    if !matches!(ws.resolve_path(at, path), Resolution::Unresolved) {
        return None;
    }
    let found = match ty.contains('.') {
        true => ws.resolve_path_in(at, Namespace::Type, ty),
        false => ws.resolve_in(at, Namespace::Type, ty),
    };
    let (Resolution::Local(def) | Resolution::Imported { def, .. }) = found else {
        return None;
    };
    let cases = sigs.type_decl(def)?.variants.as_ref()?;
    Some(match cases.iter().position(|(name, _)| name == case) {
        Some(index) => CaseNamed::Case(def, index),
        None => CaseNamed::Unknown(def, case.to_string()),
    })
}

/// **The case a bare name is** (ADR-0059): `Empty`, where no term has the
/// name, is the case of that name of the one sum type `at` sees with one,
/// in the set ADR-0047 resolves a bare constructor against. `None` where no
/// type has it, or several do, which the name alone cannot choose between.
/// The language's own cases keep their names: a program's `None` is written
/// through its type.
pub(crate) fn bare_case(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    name: &str,
) -> Option<(DefId, usize)> {
    let found = case_owners(sigs, ws, at, name);
    match found.len() {
        1 => found.into_iter().next(),
        _ => None,
    }
}

/// **Each type `at` sees with a case `name`**, and the case's position in
/// it, where `name` alone is a case written alone: no term has the name, and
/// it is none of the language's own cases. One is the case `bare_case` names;
/// several, the type expected where it is written chooses (ADR-0201).
pub(crate) fn case_owners(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    name: &str,
) -> BTreeSet<(DefId, usize)> {
    if matches!(name, "Some" | "None" | "Ok" | "Err")
        || !matches!(
            ws.resolve_in(at, Namespace::Term, name),
            Resolution::Unresolved
        )
    {
        return BTreeSet::new();
    }
    ws.visible_types(at)
        .into_iter()
        .filter_map(|def| {
            let cases = sigs.type_decl(def)?.variants.as_ref()?;
            Some((def, cases.iter().position(|(n, _)| n == name)?))
        })
        .collect()
}

/// **The case a case written alone names at `e`** (ADR-0201, ADR-0195's
/// ruling 5): the one type this unit sees with it, or, where several have
/// it, the one expected where it is written. `e` is the name. The backend
/// asks this, so it builds the case the checker typed.
pub(crate) fn case_at(
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    module: Option<&str>,
    decl: &Decl,
    body: &Body,
    e: ExprId,
) -> Option<(DefId, usize)> {
    let Expr::Name(name) = body.expr(e) else {
        return None;
    };
    let owners = case_owners(sigs, ws, at, name);
    match owners.len() {
        0 => None,
        1 => owners.into_iter().next(),
        _ => {
            let lexical = Lexical::build(decl, body);
            Typer::new(sigs, ws, at, module, decl, body, lexical, Vec::new()).case_from_expected(e)
        }
    }
}

/// **The type parameters of `binder` no field mentions** (ADR-0065): each is
/// free in a value built from those fields, which holds at every type it
/// could be.
fn unmentioned(ws: &Workspace, binder: DefId, fields: &[Option<TypeResolution>]) -> BTreeSet<u32> {
    fn mentions(t: &ResolvedType, binder: DefId, i: u32) -> bool {
        t.parameter_binding() == Some((binder, i))
            || t.args().iter().any(|a| mentions(a, binder, i))
    }
    let arity = ws.type_arity(binder).unwrap_or(0) as u32;
    (0..arity)
        .filter(|i| {
            !fields.iter().any(|f| {
                f.as_ref()
                    .and_then(TypeResolution::resolved)
                    .is_some_and(|t| mentions(t, binder, *i))
            })
        })
        .collect()
}

/// `t` with every [`Ty::Any`] in it made unknown: a mutable binding holds
/// one type, whatever its first value holds at (ADR-0065).
fn one_type(t: Ty) -> Ty {
    match t {
        Ty::Any => Ty::Unknown,
        Ty::Builtin(c, args) => Ty::Builtin(c, args.into_iter().map(one_type).collect()),
        Ty::Nominal(d, args) => Ty::Nominal(d, args.into_iter().map(one_type).collect()),
        other => other,
    }
}

/// **Mark the `if`s and `match`es whose value is used** (ADR-0068), under
/// `id`, whose own value is used where `used` says. A block's value is its
/// last statement's; a branch's is its `if`'s or `match`'s; a condition, a
/// scrutinee, an initialiser, an argument and an operand are used. A `for`
/// loop's body and a lambda's are not read here: the first is a statement,
/// and the second is its function's result, which its use relates.
fn mark_used(body: &Body, id: ExprId, used: bool, out: &mut BTreeSet<ExprId>) {
    match body.expr(id) {
        Expr::Block { stmts } => {
            let last = stmts.len().saturating_sub(1);
            for (i, s) in stmts.iter().enumerate() {
                mark_used(body, *s, used && i == last, out);
            }
        }
        Expr::If { cond, then, els } => {
            if used && els.is_some() {
                out.insert(id);
            }
            mark_used(body, *cond, true, out);
            mark_used(body, *then, used, out);
            if let Some(e) = els {
                mark_used(body, *e, used, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            if used {
                out.insert(id);
            }
            mark_used(body, *scrutinee, true, out);
            for a in arms {
                mark_used(body, a.body, used, out);
            }
        }
        Expr::For {
            iterable, body: b, ..
        } => {
            mark_used(body, *iterable, true, out);
            mark_used(body, *b, false, out);
        }
        Expr::Lambda {
            descriptor,
            body: b,
            ..
        } => {
            if let Some(d) = descriptor {
                mark_used(body, *d, true, out);
            }
            mark_used(body, *b, false, out);
        }
        Expr::Keyword { args, block, .. } => {
            for a in args {
                mark_used(body, *a, true, out);
            }
            if let Some(b) = block {
                mark_used(body, *b, false, out);
            }
        }
        _ => {
            for c in body.children(id) {
                mark_used(body, c, true, out);
            }
        }
    }
}

/// The rounds the typer's bindings are solved in, at most. Each round adds a
/// binding or completes one's type, or ends the solving; a chain written in
/// order is solved in one.
const ROUNDS: usize = 8;

impl<'a> Typer<'a> {
    /// The module this body is in, which a member is looked up as (ADR-0254).
    fn module(&self) -> Option<&'a str> {
        self.ws.module_of(self.at).map(|m| m.name.as_str())
    }

    /// A typer for `decl`'s body, reading names by `lexical`. `outer` types
    /// the bindings around a nested declaration, in the order the resolution
    /// numbers them (ADR-0066).
    #[allow(clippy::too_many_arguments)]
    fn new(
        sigs: &'a Signatures,
        ws: &'a Workspace,
        at: UnitId,
        module: Option<&'a str>,
        decl: &'a Decl,
        body: &'a Body,
        lexical: Lexical,
        outer: Vec<Ty>,
    ) -> Typer<'a> {
        let mut piped = BTreeMap::new();
        for id in body.walk() {
            if let Expr::Binary {
                op: BinOp::Pipe,
                lhs,
                rhs,
            } = body.expr(id)
            {
                piped.insert(*rhs, *lhs);
            }
        }

        // What the program writes: each parameter's declared type, and each
        // annotated `let`'s. Around a nested declaration, what its enclosing
        // declaration's bindings were typed as there (ADR-0066).
        let mut locals = BTreeMap::new();
        for (i, t) in outer.into_iter().enumerate() {
            if !t.is_unknown() {
                locals.insert(Binder::Outer(i as u32), t);
            }
        }
        for (i, p) in decl.params.iter().enumerate() {
            if let Some(t) = &p.ty
                && let Some(ty) = sigs
                    .resolve_type(module, decl, t, p.span.clone())
                    .resolved()
            {
                locals.insert(Binder::Param(i), Ty::of(ty));
            }
        }
        for id in body.walk() {
            if let Expr::Let {
                pat: Some(pat),
                ty: Some(ty),
                ..
            } = body.expr(id)
                && let Pattern::Bind { .. } = body.pat(*pat)
                && let Some(written) = crate::resolved::written_in_body(body, *ty)
                && let Some(t) = sigs
                    .resolve_type(module, decl, &written, body.expr_span(id))
                    .resolved()
            {
                locals.insert(Binder::Pattern(*pat), Ty::of(t));
            }
        }
        // A lambda's written parameter type, `(e: InputEvent) => ..`, and an
        // `on:` handler's event where none is written: the record its event
        // gives it (ADR-0138).
        for (pat, ty) in &body.param_types {
            if let Pattern::Bind { .. } = body.pat(*pat)
                && let Some(written) = crate::resolved::written_in_body(body, *ty)
                && let Some(t) = sigs.resolve_type(module, decl, &written, 0..0).resolved()
            {
                locals.insert(Binder::Pattern(*pat), Ty::of(t));
            }
        }
        for (event, lambda) in crate::resume::handler_events(body) {
            let Expr::Lambda { params, .. } = body.expr(lambda) else {
                continue;
            };
            if let [p] = lambda_params(body, params).as_slice()
                && let Pattern::Bind { .. } = body.pat(*p)
                && let Some(t) = crate::annotations::event_type(sigs, &event)
            {
                locals
                    .entry(Binder::Pattern(*p))
                    .or_insert_with(|| Ty::of(t));
            }
        }

        let mut used = BTreeSet::new();
        mark_used(body, body.root, false, &mut used);
        for (_, r) in decl.term_roots() {
            mark_used(body, r.root, false, &mut used);
        }
        let mut roots = Vec::new();
        for id in body.walk() {
            if let Expr::Template { roots: r, .. } = body.expr(id) {
                roots.extend(r.iter().copied());
            }
        }
        let attribute_strings: BTreeSet<ExprId> = body
            .walk_markup(&roots)
            .into_iter()
            .filter_map(|n| match body.node(n) {
                crate::hir::Node::Element { attrs, .. } => Some(attrs.clone()),
                _ => None,
            })
            .flatten()
            .filter_map(|a| match a.value {
                crate::hir::AttrValue::Expr(e) => Some(e),
                _ => None,
            })
            .collect();
        let typer = Typer {
            sigs,
            ws,
            at,
            module,
            decl,
            body,
            lexical,
            locals: RefCell::new(locals),
            piped,
            solved: RefCell::new(BTreeMap::new()),
            used,
            attribute_strings,
            parents: body
                .exprs()
                .flat_map(|(id, _, _)| body.children(id).into_iter().map(move |c| (c, id)))
                .collect(),
            lets: body
                .exprs()
                .filter_map(|(_, e, _)| match e {
                    Expr::Let {
                        pat: Some(pat),
                        init: Some(init),
                        ..
                    } => Some((*pat, *init)),
                    _ => None,
                })
                .collect(),
            expecting: RefCell::new(BTreeSet::new()),
        };

        // What the program implies: an unannotated `let` or `use` takes its
        // initialiser's type, a lambda's parameters the types its use gives
        // them, a match arm's names its scrutinee's payload, a `for` loop's
        // name its list's element, and a template's names their block's
        // (ADR-0063). Iterated, because `let a = f()` then `let b = g(a)`
        // needs `a` first, and a lambda's call may need a binding typed;
        // bounded, because a round only adds a binding or makes one's type
        // more complete, and a complete type is never changed.
        for _ in 0..ROUNDS {
            let mut added = typer.solve_lambdas();
            added |= typer.solve_bindings();
            if !added {
                break;
            }
        }
        typer
    }

    /// **The type of an expression**, holes included.
    fn of(&self, id: ExprId) -> Ty {
        match self.body.expr(id) {
            Expr::Literal(l) => Ty::Primitive(match l {
                Literal::Int(_) => Primitive::Int,
                Literal::Float(_) => Primitive::Float,
                Literal::Str(_) | Literal::UnterminatedStr(_) => Primitive::Str,
                Literal::Unit => Primitive::Unit,
            }),
            Expr::Interpolated { .. } => Ty::Primitive(Primitive::Str),
            Expr::Name(n) => self.name(id, n),
            Expr::Field { base, name } => self.field(id, *base, name),
            Expr::Call { .. } | Expr::Address { .. } => self.call(id).result,
            // `let store = query Store(id)` binds what the query produces when
            // it succeeds: the page renders `{store.name}`, and its loading and
            // failure states are the page's to render, not the binding's. The
            // same reading `infer.rs` gives `{#each}` over a query. Recorded
            // in docs/ASSUMPTIONS.md.
            Expr::Keyword {
                keyword, modifiers, ..
            } if matches!(keyword.as_str(), "query" | "subscription") && !modifiers.is_empty() => {
                match self.call(id).result {
                    Ty::Builtin(Builtin::Result, mut args) if args.len() == 2 => {
                        args.swap_remove(0)
                    }
                    other => other,
                }
            }
            // A navigation produces nothing (ADR-0280): the page goes. Nor does
            // a schedule: the run comes later (the scheduled-commands track).
            Expr::Keyword { keyword, .. }
                if matches!(keyword.as_str(), "navigate" | "schedule" | "unschedule") =>
            {
                Ty::Primitive(Primitive::Unit)
            }
            Expr::Binary { op, lhs, rhs } => match op {
                BinOp::Cmp(_) | BinOp::And | BinOp::Or => Ty::Primitive(Primitive::Bool),
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                    match (self.of(*lhs), self.of(*rhs)) {
                        (Ty::Primitive(p), Ty::Primitive(q))
                            if p == q && matches!(p, Primitive::Int | Primitive::Float) =>
                        {
                            Ty::Primitive(p)
                        }
                        // A value of any type is the number the other side is
                        // (ADR-0065).
                        (Ty::Primitive(p), Ty::Any) | (Ty::Any, Ty::Primitive(p))
                            if matches!(p, Primitive::Int | Primitive::Float) =>
                        {
                            Ty::Primitive(p)
                        }
                        (Ty::Any, Ty::Any) => Ty::Any,
                        _ => Ty::Unknown,
                    }
                }
                BinOp::Pipe => self.of(*rhs),
                BinOp::Transition | BinOp::Assign => Ty::Unknown,
            },
            Expr::Unary { op, operand } => match op {
                UnOp::Not => Ty::Primitive(Primitive::Bool),
                UnOp::Neg => match self.of(*operand) {
                    Ty::Primitive(p) if matches!(p, Primitive::Int | Primitive::Float) => {
                        Ty::Primitive(p)
                    }
                    _ => Ty::Unknown,
                },
            },
            // `e?` is `e`'s success value.
            Expr::Try { value } => match self.of(*value) {
                Ty::Builtin(Builtin::Result, mut args) if args.len() == 2 => args.swap_remove(0),
                Ty::Builtin(Builtin::Option, mut args) if args.len() == 1 => args.swap_remove(0),
                _ => Ty::Unknown,
            },
            Expr::Cast { ty, .. } => crate::resolved::written_in_body(self.body, *ty)
                .and_then(|t| {
                    self.sigs
                        .resolve_type(self.module, self.decl, &t, self.body.expr_span(id))
                        .resolved()
                        .map(Ty::of)
                })
                .unwrap_or(Ty::Unknown),
            Expr::Block { stmts } => match stmts.last() {
                Some(last) if !matches!(self.body.expr(*last), Expr::Let { .. }) => self.of(*last),
                _ => Ty::Unknown,
            },
            Expr::If {
                then, els: Some(e), ..
            } => join(self.of(*then), self.of(*e)),
            // Without `else` it is a statement, whose value is the unit value
            // whichever way it goes, as the backend has it (ADR-0051). It was
            // of no stated type until 2026-09-26, so one ending a body that
            // declares an `Int` passed (ADR-0067).
            Expr::If { els: None, .. } => Ty::Primitive(Primitive::Unit),
            // Each arm's body. Its pattern's names are bound to the payload
            // types the scrutinee's type gives them when the typer is built:
            // `entry` in `Some(entry) => ..` is the option's `T`.
            Expr::Match { arms, .. } => arms
                .iter()
                .map(|a| self.of(a.body))
                .reduce(join)
                .unwrap_or(Ty::Unknown),
            Expr::Record { name: Some(_), .. } => self.construct(id).result,
            // `[]` is a list of anything (ADR-0065).
            Expr::List { items } => Ty::Builtin(
                Builtin::List,
                vec![
                    items
                        .iter()
                        .map(|i| self.of(*i))
                        .reduce(join)
                        .unwrap_or(Ty::Any),
                ],
            ),
            _ => Ty::Unknown,
        }
    }

    /// **A name where `id` writes it**: the binding it means, or, where no
    /// binding in scope has it, the program's own (ADR-0063).
    fn name(&self, id: ExprId, n: &str) -> Ty {
        match self.lexical.binder(id) {
            Some(b) => self.local(b),
            // A listener's `_` is matched by every value at its position, as
            // the materializer compares it (ADR-0091): a value of any type.
            // Until ADR-0178 it was typed as a name nothing declares, and
            // `invalidates_on InventoryChanged(id, _)` stayed undecided.
            None if n == "_" && self.listens_with(id) => Ty::Any,
            // A case written alone that several types have: the one expected
            // here (ADR-0201).
            None => match self.global(n) {
                Ty::Unknown => self
                    .case_from_expected(id)
                    .map_or(Ty::Unknown, |(def, index)| self.case_value(def, index)),
                t => t,
            },
        }
    }

    /// Is `id` an argument of a listener's key, which binds rather than
    /// evaluates (ADR-0091)?
    fn listens_with(&self, id: ExprId) -> bool {
        self.decl
            .roots_in(crate::hir::ExecutionContext::Listener)
            .any(|(_, r)| r.root == id)
    }

    /// **Whether a listener's argument `id`, of type `actual`, is a user's
    /// handle over the event's `expected`** (track `notifications`, ADR-0270,
    /// amending ADR-0091): `invalidates_on Notified(reader)`, `reader` a
    /// `User<UserId>` and `Notified`'s value a `UserId`. The entries at that
    /// user are dropped, in each of their sessions: the platform's `User` is
    /// ABI-transparent, so the event's value is the entry's key. A listener
    /// alone, `User` alone, and its own argument alone.
    fn handle_over(&self, id: ExprId, actual: &Ty, expected: &Ty) -> bool {
        let Ty::Nominal(def, args) = actual else {
            return false;
        };
        let [over] = args.as_slice() else {
            return false;
        };
        self.listens_with(id)
            && matches!(
                self.sigs.privacy_kind(*def),
                Some(crate::signatures::PrivacyQualifier::User)
            )
            && unify(&mut Subst::default(), expected, over) == Verdict::Agree
    }

    /// The type a binding holds, as far as it is known.
    fn local(&self, b: Binder) -> Ty {
        self.locals.borrow().get(&b).cloned().unwrap_or(Ty::Unknown)
    }

    /// A name no local binding has: a declaration, the language's own value,
    /// or a sum type's case.
    fn global(&self, n: &str) -> Ty {
        // A declared callable named as a value: `List.map(xs, line_total)`.
        let term = self.ws.resolve_in(self.at, Namespace::Term, n);
        if let Resolution::Local(d) | Resolution::Imported { def: d, .. } = term
            && let Some(sig) = self.sigs.by_def(d)
        {
            return function_type(sig);
        }
        // A module's signal, provided to this body: a value of the type it
        // declares (ADR-0144).
        if let Resolution::Local(d) | Resolution::Imported { def: d, .. } = term
            && let Some(declared) = self.sigs.signal_type(d)
        {
            return match declared {
                TypeResolution::Resolved(t) => Ty::of(t),
                _ => Ty::Unknown,
            };
        }
        // A predicate a page asks of its reader (ADR-0307): `{#if SignedIn}`
        // is whether the deployment holds it for the page's principal.
        if matches!(term, Resolution::Unresolved)
            && self.is_ui()
            && let Resolution::Local(d) | Resolution::Imported { def: d, .. } =
                self.ws.resolve_in(self.at, Namespace::Predicate, n)
            && self.sigs.by_def(d).is_some_and(|s| s.params.is_empty())
        {
            return Ty::Primitive(Primitive::Bool);
        }
        // The language's own values, only where the program has not declared
        // something of that name; then a sum type's case (ADR-0059).
        let own = matches!(term, Resolution::Unresolved);
        match n {
            "true" | "false" if own => Ty::Primitive(Primitive::Bool),
            "None" if own => Ty::Builtin(Builtin::Option, vec![Ty::Any]),
            // It never returns, so it is a value of every type (ADR-0065).
            "todo" if own => Ty::Any,
            // Charter §7.5A: `self` in a UI declaration is its own element, a
            // language fact rather than something a library declares.
            "self" if own && self.is_ui() => self
                .sigs
                .language_type("browser", "ElementRef")
                .map(|t| Ty::of(&t))
                .unwrap_or(Ty::Unknown),
            _ if own => match bare_case(self.sigs, self.ws, self.at, n) {
                Some((def, index)) => self.case_value(def, index),
                None => Ty::Unknown,
            },
            _ => Ty::Unknown,
        }
    }

    /// **A case written alone that several types have, by the type expected
    /// where it is written** (ADR-0201, ADR-0195's ruling 5): `Empty` where a
    /// `Shape` is expected is `Shape.Empty`. `name` is the case's name; its
    /// value goes where it is written, or where the call it is the callee of
    /// is, `Circle(3)`. `None` where one type alone has it (`bare_case`'s),
    /// or the expected type is none of them.
    fn case_from_expected(&self, name: ExprId) -> Option<(DefId, usize)> {
        let Expr::Name(n) = self.body.expr(name) else {
            return None;
        };
        if self.lexical.binder(name).is_some() {
            return None;
        }
        let owners = case_owners(self.sigs, self.ws, self.at, n);
        if owners.len() < 2 {
            return None;
        }
        let site = match self.parents.get(&name) {
            Some(&call) if matches!(self.body.expr(call), Expr::Call { callee, .. } if *callee == name) => {
                call
            }
            _ => name,
        };
        let Some(Ty::Nominal(def, _)) = self.expected(site) else {
            return None;
        };
        owners.into_iter().find(|(owner, _)| *owner == def)
    }

    /// **The type the program expects at `e`**, from where `e` is written,
    /// never from what it is (ADR-0201): a result of the declaration, its
    /// declared result; a `let`'s initializer, its annotation; an argument,
    /// its parameter's type; a field, the field's; an assignment, what is
    /// assigned to; a comparison's side, the other side's; a list's item, the
    /// list's element or another item's; and a branch, or a block's last
    /// value, what its `if`, `match` or block is expected to be. `None` where
    /// nothing written says.
    fn expected(&self, e: ExprId) -> Option<Ty> {
        if !self.expecting.borrow_mut().insert(e) {
            return None;
        }
        let found = self.expected_at(e);
        self.expecting.borrow_mut().remove(&e);
        found.filter(|t| !matches!(t, Ty::Unknown | Ty::Var(_) | Ty::Any))
    }

    fn expected_at(&self, e: ExprId) -> Option<Ty> {
        if self.result_sites().contains(&e) {
            return self.declared_result();
        }
        let parent = *self.parents.get(&e)?;
        match self.body.expr(parent) {
            Expr::Let {
                ty: Some(t),
                init: Some(init),
                ..
            } if *init == e => self.written(*t),
            Expr::Call { callee, .. } if *callee == e => self.expected(parent),
            Expr::Call { callee, args } => {
                let k = args.iter().position(|a| a.value == e)?;
                self.parameter(parent, *callee, args, k)
            }
            Expr::Binary {
                op: BinOp::Assign,
                lhs,
                rhs,
            } if *rhs == e => Some(self.of(*lhs)),
            Expr::Binary {
                op: BinOp::Cmp(_),
                lhs,
                rhs,
            } => Some(self.of(if *lhs == e { *rhs } else { *lhs })),
            Expr::Record {
                name: Some(record),
                fields,
            } => {
                let field = fields.iter().find(|f| f.value == Some(e))?;
                self.field_type(record, &field.name)
            }
            Expr::List { items } => match self.expected(parent) {
                Some(Ty::Builtin(Builtin::List, element)) => element.into_iter().next(),
                _ => items
                    .iter()
                    .filter(|i| **i != e)
                    .map(|i| self.of(*i))
                    .find(|t| !matches!(t, Ty::Unknown | Ty::Var(_) | Ty::Any)),
            },
            Expr::If { then, els, .. } if *then == e || *els == Some(e) => self.expected(parent),
            Expr::Match { arms, .. } if arms.iter().any(|a| a.body == e) => self.expected(parent),
            Expr::Block { stmts } if stmts.last() == Some(&e) => self.expected(parent),
            _ => None,
        }
    }

    /// **`Empty` written alone, where several types have it** (ADR-0201):
    /// the type expected where it is written chooses, or PW0022 names them.
    fn bare_case_relation(&self, id: ExprId, n: &str) -> Option<ValueRelation> {
        if self.lexical.binder(id).is_some() {
            return None;
        }
        let owners = case_owners(self.sigs, self.ws, self.at, n);
        if owners.len() < 2 {
            return None;
        }
        let outcome = match self.case_from_expected(id) {
            Some(_) => Outcome::Agree,
            None => Outcome::Disagree {
                expected: owners
                    .iter()
                    .map(|(def, _)| self.declared_name(*def))
                    .collect::<Vec<_>>()
                    .join(" and "),
                actual: n.to_string(),
            },
        };
        Some(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::BareCase,
            span: self.body.expr_span(id),
            target: n.to_string(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (
                self.body.expr_span(id),
                format!("written alone inside `{}`", self.decl.name),
            ),
        })
    }

    /// A declaration's own name, `Shape` for `t.Shape`.
    fn declared_name(&self, def: DefId) -> String {
        let path = self.display_def(def);
        self.ws
            .module_of(def.unit)
            .and_then(|m| path.strip_prefix(m.name.as_str())?.strip_prefix('.'))
            .map_or(path.clone(), str::to_string)
    }

    /// What this declaration declares it returns, where its body is its
    /// result.
    fn declared_result(&self) -> Option<Ty> {
        if !returns_its_body(self.decl) {
            return None;
        }
        let written = self.decl.ret.as_ref()?;
        self.sigs
            .resolve_type(self.module, self.decl, written, 0..0)
            .resolved()
            .map(Ty::of)
    }

    /// A type written in this body, `let s: Shape = ..`'s.
    fn written(&self, t: crate::hir::TypeRefId) -> Option<Ty> {
        let written = crate::resolved::written_in_body(self.body, t)?;
        self.sigs
            .resolve_type(self.module, self.decl, &written, 0..0)
            .resolved()
            .map(Ty::of)
    }

    /// The type of the parameter the `k`th written argument of `call` is
    /// given to, by the rule `apply` arranges them by (ADR-0081).
    fn parameter(
        &self,
        call: ExprId,
        callee: ExprId,
        args: &[crate::hir::Arg],
        k: usize,
    ) -> Option<Ty> {
        let target = self.target(call, callee)?;
        let (names, params): (Vec<String>, Vec<Option<TypeResolution>>) = match &target {
            Target::Callable(sig) | Target::Member(sig, _) => {
                (sig.names.clone(), sig.params.clone())
            }
            Target::Record(def) => {
                let fields = self.sigs.type_decl(*def)?.record.clone()?;
                (
                    fields.iter().map(|(n, _)| n.clone()).collect(),
                    fields.into_iter().map(|(_, r)| Some(r)).collect(),
                )
            }
            Target::Case(def, index) => {
                let (_, fields) = self.sigs.type_decl(*def)?.variants.as_ref()?.get(*index)?;
                (Vec::new(), fields.iter().cloned().map(Some).collect())
            }
            Target::Opaque(def) => (
                Vec::new(),
                vec![self.sigs.type_decl(*def)?.representation.clone()],
            ),
        };
        let leading = usize::from(matches!(target, Target::Member(..)))
            + usize::from(self.piped.contains_key(&call));
        let written: Vec<Option<&str>> = args.iter().map(|a| a.name.as_deref()).collect();
        let i = *crate::signatures::arrange(&names, leading, &written)
            .ok()?
            .get(k)?;
        params.get(i)?.as_ref()?.resolved().map(Ty::of)
    }

    /// The type a record type's field `name` is declared.
    fn field_type(&self, record: &str, name: &str) -> Option<Ty> {
        let found = match record.contains('.') {
            true => self.ws.resolve_path_in(self.at, Namespace::Type, record),
            false => self.ws.resolve_in(self.at, Namespace::Type, record),
        };
        let (Resolution::Local(def) | Resolution::Imported { def, .. }) = found else {
            return None;
        };
        let fields = self.sigs.type_decl(def)?.record.as_ref()?;
        let (_, t) = fields.iter().find(|(n, _)| n == name)?;
        t.resolved().map(Ty::of)
    }

    /// **A case named as a value** (ADR-0059): one without a payload is a
    /// value of its type, `Shape.Empty`; one with a payload is the function
    /// that builds it, `Shape.Circle`, which a list operation can be passed.
    fn case_value(&self, def: DefId, index: usize) -> Ty {
        let Some((_, fields)) = self
            .sigs
            .type_decl(def)
            .and_then(|t| t.variants.as_ref())
            .and_then(|cases| cases.get(index))
        else {
            return Ty::Unknown;
        };
        // A parameter the case's fields do not mention holds at every type:
        // `Maybe.Nothing` is a `Maybe` of anything (ADR-0065). One they do
        // mention links the function's parameter to its result, and stays
        // unknown.
        let s = Subst {
            any: unmentioned(
                self.ws,
                def,
                &fields.iter().map(|f| Some(f.clone())).collect::<Vec<_>>(),
            ),
            ..Subst::default()
        };
        let result = s.close(&self.applied(def).instantiate(def));
        if fields.is_empty() {
            return result;
        }
        let mut shape: Vec<Ty> = fields
            .iter()
            .map(|f| match f {
                TypeResolution::Resolved(t) => s.close(&Ty::of(t).instantiate(def)),
                _ => Ty::Unknown,
            })
            .collect();
        shape.push(result);
        Ty::Builtin(Builtin::Function, shape)
    }

    /// The names an arm's pattern binds, each typed from the type the part
    /// of the pattern binding it is read against: `Some(x)` binds the
    /// option's `T`; `Ok(x)` and `Err(e)` the result's two sides, where the
    /// program declares nothing of that name; `Rect(w, h)` each field of a
    /// declared case's payload (ADR-0059); `Some(Circle(r))` its field inside
    /// a case inside the option; and a name alone the whole value, unless it
    /// names a case (ADR-0060). Until 2026-09-26 only a name directly under a
    /// case was bound, and a nested one was unknown to every relation.
    /// Each name an arm's pattern binds, by where it binds it, and the type
    /// the scrutinee's type gives it.
    fn arm_bindings(&self, scrutinee: &Ty, pat: crate::hir::PatternId) -> Vec<(Binder, Ty)> {
        let mut out = Vec::new();
        self.bind_pattern(scrutinee, pat, &mut out);
        out
    }

    fn bind_pattern(&self, ty: &Ty, pat: crate::hir::PatternId, out: &mut Vec<(Binder, Ty)>) {
        match self.body.pat(pat) {
            Pattern::Bind { .. } => out.push((Binder::Pattern(pat), ty.clone())),
            Pattern::Ctor { path, args } => {
                if let Some(fields) = self.pattern_fields(ty, path, args.len()) {
                    for (arg, field) in args.iter().zip(&fields) {
                        self.bind_pattern(field, *arg, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// The types of the fields a constructor pattern takes apart, read
    /// against `ty`: a builtin case's payload, or a declared case's fields
    /// under the type's arguments (`Tree<Int>`'s `Node(Tree<T>, T)` has a
    /// `Tree<Int>` and an `Int`). `None` for a case `ty` lacks, a qualifier
    /// naming another type, or a field count the case does not declare; the
    /// exhaustiveness check reports each (PW0608, PW0603).
    fn pattern_fields(&self, ty: &Ty, path: &str, arity: usize) -> Option<Vec<Ty>> {
        if let Ty::Nominal(def, args) = ty
            && let Some(cases) = self.sigs.type_decl(*def).and_then(|t| t.variants.as_ref())
        {
            let index = match path.rsplit_once('.') {
                Some(_) => match case_named(self.sigs, self.ws, self.at, path)? {
                    CaseNamed::Case(d, index) if d == *def => index,
                    _ => return None,
                },
                None => cases.iter().position(|(n, _)| n == path)?,
            };
            let (_, fields) = cases.get(index)?;
            if fields.len() != arity {
                return None;
            }
            return Some(
                fields
                    .iter()
                    .map(|f| match f {
                        TypeResolution::Resolved(t) => substituted(&Ty::of(t), *def, args),
                        _ => Ty::Unknown,
                    })
                    .collect(),
            );
        }
        let own = matches!(
            self.ws.resolve_in(self.at, Namespace::Term, path),
            Resolution::Unresolved
        );
        let payload = match (path, ty) {
            ("Some", Ty::Builtin(Builtin::Option, a)) if own => a.first(),
            ("Ok", Ty::Builtin(Builtin::Result, a)) if own => a.first(),
            ("Err", Ty::Builtin(Builtin::Result, a)) if own => a.get(1),
            _ => None,
        }?;
        (arity == 1).then(|| vec![payload.clone()])
    }

    /// **A policy's term roots**: the target and transition of an optimistic
    /// clause, a resource's `acquire` and `release`, a painter. They are
    /// separate trees in the body's arena, reached from no statement, so the
    /// body walk does not see them — `optimistic Cart(..) as cart =>
    /// Carts.with_line(cart, item, quantity)` is an ordinary call, and it was
    /// checked by nothing until this.
    ///
    /// A binder takes the type its header gives it where the header gives
    /// one: an optimistic transition's binder is the target's value
    /// (ADR-0025), a `release(h)`'s is what the resource declares it
    /// produces. Any other binder is bound as unknown, so it can never borrow
    /// the type of a parameter that happens to share its name.
    fn term_relations(&self, out: &mut Vec<ValueRelation>) {
        for (_, root) in self.decl.term_roots() {
            self.relations_from(root.root, out);
        }
        // **A transition produces its target's value** (ADR-0241): the entry
        // it replaces holds one, and the platform restores it. This typer
        // types every expression. PW0331 read the transition through the
        // older one, which has no answer for a literal, so `=> "no"` checked
        // over an entry holding an `Int`.
        for (_, target, transition) in self.decl.optimistic_clauses() {
            let expected = match self.of(target.root) {
                Ty::Builtin(Builtin::Result, mut args) if args.len() == 2 => args.swap_remove(0),
                other => other,
            };
            let boundary = (
                self.body.expr_span(target.root),
                format!("this resource holds `{}`", self.display(&expected)),
            );
            // Its target, the name the repair says to produce it from.
            let binder = transition
                .binders
                .first()
                .map_or_else(|| "the bound value".to_string(), |(n, _)| format!("`{n}`"));
            out.push(self.relate_types(
                RelationKind::Transition,
                expected,
                self.of(transition.root),
                binder,
                self.body.expr_span(transition.root),
                boundary,
            ));
        }
        for policy in &self.decl.policies {
            for key in &policy.keys {
                if let Some(sig) = self.keyed(&policy.name, key) {
                    let args = key.args.iter().map(|a| (a.name.clone(), a.value));
                    let solved = self.apply(
                        key.span.clone(),
                        key.name_span.clone(),
                        None,
                        Target::Callable(sig),
                        args.collect(),
                    );
                    out.extend(solved.relations);
                }
            }
        }
    }

    /// **The declaration a clause's key names**, where it is one of the kind
    /// the clause names (ADR-0088): a resource for `invalidates`, an event
    /// for `emits`. Looked up in the clause's namespace, never as a term. A
    /// name of another kind, or of nothing, is the graph's (PW5100, PW5103).
    fn keyed(&self, head: &str, key: &crate::hir::ClauseKey) -> Option<&'a Signature> {
        crate::policy::keyed(head)?.iter().find_map(|(ns, kinds)| {
            let found = match key.name.contains('.') {
                true => self.ws.resolve_path_in(self.at, *ns, &key.name),
                false => self.ws.resolve_in(self.at, *ns, &key.name),
            };
            let (Resolution::Local(def) | Resolution::Imported { def, .. }) = found else {
                return None;
            };
            if !self.sigs.kind_of(def).is_some_and(|k| kinds.contains(&k)) {
                return None;
            }
            self.sigs.by_def(def)
        })
    }

    /// The type a policy term's binders take from its header, where it gives
    /// one (see [`Typer::term_relations`]).
    fn term_binders(&self) -> Vec<(Binder, Ty)> {
        use crate::hir::ExecutionContext as Cx;
        let success = |t: Ty| match t {
            Ty::Builtin(Builtin::Result, mut args) if args.len() == 2 => args.swap_remove(0),
            other => other,
        };
        let own_result = self
            .own_def()
            .and_then(|d| self.sigs.by_def(d))
            .and_then(Signature::result)
            .map(Ty::of);
        let mut out = Vec::new();
        for (i, (policy, root)) in self.decl.term_roots().enumerate() {
            let bound = match root.context {
                // Its own arm's target (ADR-0238): the one before it.
                Cx::OptimisticTransition => policy
                    .roots
                    .iter()
                    .take_while(|r| !std::ptr::eq(*r, root))
                    .filter(|r| r.context == Cx::TargetSelection)
                    .last()
                    .map(|t| success(self.of(t.root)))
                    .unwrap_or(Ty::Unknown),
                Cx::Release => own_result.clone().map(success).unwrap_or(Ty::Unknown),
                // The platform declares what a draw block can do (ADR-0351),
                // just as it declares the event a handler receives.
                Cx::Draw => self
                    .sigs
                    .language_type("browser", "PaintContext")
                    .as_ref()
                    .map(Ty::of)
                    .unwrap_or(Ty::Unknown),
                _ => Ty::Unknown,
            };
            out.extend((0..root.binders.len()).map(|j| (Binder::Term(i, j), bound.clone())));
        }
        out
    }

    /// A lambda, typed against the function type it is passed as: its
    /// parameters take that type's parameter types, and its type is theirs
    /// with its body's type as the result. Without an expected function type
    /// a lambda has no type here — an unannotated parameter is not a claim.
    ///
    /// A parameter whose type is known only in part keeps that part, and is
    /// checked by it (ADR-0053, amended 2026-10-11): `xs` in
    /// `each_list(xs => xs + 1)` is a `List<?>`, and a list is no number.
    /// Until then only a type known whole was kept, a guard from before
    /// [`Typer::bind`] completed a type a later round knows more of, and
    /// such a parameter was not checked at all.
    fn lambda(&self, params: &[crate::hir::PatternId], body: ExprId, expected: &[Ty]) -> Ty {
        let Some(params) = lambda_binders(self.body, params) else {
            return Ty::Unknown;
        };
        let Some((_, expected_params)) = expected.split_last() else {
            return Ty::Unknown;
        };
        if params.len() != expected_params.len() {
            // A callback of the wrong arity is a different function type.
            let mut shape = vec![Ty::Unknown; params.len()];
            shape.push(Ty::Unknown);
            return Ty::Builtin(Builtin::Function, shape);
        }
        let own = self.own_def();
        let mut saved = Vec::new();
        for (p, ty) in params.iter().zip(expected_params) {
            let b = Binder::Pattern(*p);
            if !ty.mentions_foreign_parameter(own) {
                self.solved.borrow_mut().insert(b, ty.clone());
            }
            let old = self.locals.borrow_mut().insert(b, ty.clone());
            saved.push((b, old));
        }
        let result = self.of(body);
        for (b, old) in saved {
            match old {
                Some(t) => self.locals.borrow_mut().insert(b, t),
                None => self.locals.borrow_mut().remove(&b),
            };
        }
        let mut shape = expected_params.to_vec();
        shape.push(result);
        Ty::Builtin(Builtin::Function, shape)
    }

    /// `base.name`: a record field, read through the receiver's type.
    fn field(&self, id: ExprId, base: ExprId, name: &str) -> Ty {
        // `Shape.Empty` and `Shape.Circle`, a case through its type
        // (ADR-0059).
        if let Some(c) = self.case_path(id) {
            return match c {
                CaseNamed::Case(def, index) => self.case_value(def, index),
                CaseNamed::Unknown(..) => Ty::Unknown,
            };
        }
        // A module path: `decode.string` named as a value is that function.
        if self.resolves_as_path(id) {
            let path = path_of(self.body, id);
            return match self.ws.resolve_path_in(self.at, Namespace::Term, &path) {
                Resolution::Local(d) | Resolution::Imported { def: d, .. } => self
                    .sigs
                    .by_def(d)
                    .map(function_type)
                    .or_else(|| self.sigs.signal_type(d)?.resolved().map(Ty::of))
                    .unwrap_or(Ty::Unknown),
                _ => Ty::Unknown,
            };
        }
        self.member_type(&self.of(base), name)
    }

    /// The type of `name` read from a value of type `receiver`: a record's
    /// field under its instance, an opaque value's representation in its own
    /// module, or a property.
    fn member_type(&self, receiver: &Ty, name: &str) -> Ty {
        let receiver = receiver.clone();
        let Some(sig) = receiver
            .receiver()
            .and_then(|r| self.sigs.member_by_in(self.module(), r, name))
        else {
            // An opaque type's `.value`, read in its own module (ADR-0048).
            return match self.representation(&receiver, name) {
                Some((def, rep)) if def.unit == self.at => rep,
                _ => Ty::Unknown,
            };
        };
        member_value(self.sigs, sig, &receiver)
    }

    fn case_path(&self, id: ExprId) -> Option<CaseNamed> {
        case_named_at(self.sigs, self.ws, self.at, self.body, &self.lexical, id)
    }

    fn resolves_as_path(&self, id: ExprId) -> bool {
        let path = path_of(self.body, id);
        self.lexical.unbound_path(self.body, id)
            && !path.is_empty()
            && path.contains('.')
            && !matches!(self.ws.resolve_path(self.at, &path), Resolution::Unresolved)
    }

    /// Resolve a call's target by identity. `None` when it reaches nothing
    /// this analysis can describe — which is silence, not a verdict.
    fn target(&self, call: ExprId, callee: ExprId) -> Option<Target<'a>> {
        match named_at(
            self.sigs,
            self.ws,
            self.at,
            self.body,
            &self.lexical,
            callee,
        ) {
            Named::Target(t) => return Some(t),
            Named::Refused => return None,
            Named::Nothing => {}
        }
        // `Circle(3)`, where several types have `Circle`: the one the call's
        // expected type is (ADR-0201).
        if let Some((def, index)) = self.case_from_expected(callee) {
            return Some(Target::Case(def, index));
        }
        // `x.f(a)`, where `x` is a value whose type declares `f`.
        let Expr::Field { base, name } = self.body.expr(callee) else {
            return None;
        };
        if self.piped.contains_key(&call) {
            return None;
        }
        let receiver = self.of(*base).receiver()?;
        let sig = self.sigs.member_by_in(self.module(), receiver, name)?;
        self.sigs.by_def(sig.definition)?;
        Some(Target::Member(sig, *base))
    }

    /// `name` read from a value of type `receiver` calls a function declared
    /// on its type, read as a property: `cart.line_count` (ADR-0125).
    fn calls_a_member(&self, receiver: &Ty, name: &str) -> bool {
        receiver
            .receiver()
            .and_then(|r| self.sigs.member_by_in(self.module(), r, name))
            .is_some_and(|sig| self.sigs.by_def(sig.definition).is_some())
    }

    /// [`MemberReads`] in this body (ADR-0169).
    fn member_reads(&self) -> MemberReads {
        let exprs = self
            .body
            .exprs()
            .filter_map(|(id, e, _)| match e {
                Expr::Field { base, name } if self.calls_a_member(&self.of(*base), name) => {
                    Some(id)
                }
                _ => None,
            })
            .collect();
        // A loop's list, read as it is typed.
        let lists = self
            .lexical
            .each_blocks()
            .filter(|(_, list, _)| self.reads_a_member(*list))
            .map(|(n, ..)| n)
            .collect();
        MemberReads { exprs, lists }
    }

    /// `r.f`, where `f` is a field of `r`'s type rather than a declaration
    /// that takes it (ADR-0077).
    fn calls_a_field(&self, callee: ExprId) -> bool {
        let Expr::Field { base, name } = self.body.expr(callee) else {
            return false;
        };
        self.of(*base)
            .receiver()
            .and_then(|r| self.sigs.member_by_in(self.module(), r, name))
            .is_some_and(|sig| self.sigs.by_def(sig.definition).is_none())
    }

    /// **Resolve, instantiate and solve one call.**
    fn call(&self, id: ExprId) -> Solved {
        let unknown = Solved {
            result: Ty::Unknown,
            relations: Vec::new(),
        };
        let callee_span = match self.body.expr(id) {
            Expr::Call { callee, .. } => self.body.expr_span(*callee),
            _ => self.body.expr_span(id),
        };
        // **A page's address** (ADR-0311): the page named among the views,
        // as `navigate` names it (ADR-0280), given its parameters as a call
        // gives them; the address it makes is text. What else the path
        // names is PW5051's to say.
        if let Expr::Address { page, args } = self.body.expr(id) {
            let Some(sig) = crate::canonical::page_named(self.ws, self.at, page)
                .and_then(|d| self.sigs.by_def(d))
            else {
                return unknown;
            };
            let given = args.iter().map(|a| (a.name.clone(), a.value)).collect();
            let mut solved = self.apply(
                self.body.expr_span(id),
                callee_span,
                None,
                Target::Callable(sig),
                given,
            );
            solved.result = Ty::Primitive(Primitive::Str);
            return solved;
        }
        // A call through a binding in scope: a function value, whatever
        // declaration shares its name (ADR-0068).
        if let Expr::Call { callee, args } = self.body.expr(id)
            && matches!(self.body.expr(*callee), Expr::Name(_))
            && self.lexical.binder(*callee).is_some()
        {
            return self.value_call(id, *callee, args);
        }
        let (target, args): (Option<Target<'a>>, Vec<(Option<String>, ExprId)>) =
            match self.body.expr(id) {
                Expr::Call { callee, args } => {
                    if let Some(t) = self.intrinsic(*callee, args) {
                        return Solved {
                            result: t,
                            relations: Vec::new(),
                        };
                    }
                    let target = self.target(id, *callee);
                    // `r.f(x)`, where `f` is a field of `r`'s type: the call
                    // is through the value the field holds, as a call through
                    // a binding is (ADR-0068). Until 2026-09-26 it resolved to
                    // nothing, and neither its arguments nor its result were
                    // related (ADR-0077).
                    if target.is_none() && self.calls_a_field(*callee) {
                        return self.value_call(id, *callee, args);
                    }
                    (
                        target,
                        args.iter().map(|a| (a.name.clone(), a.value)).collect(),
                    )
                }
                // `query Menu(id)` invokes the query `Menu`; `navigate
                // OrderPage()` gives the page its parameters, and a page is
                // named among the views (ADR-0280).
                Expr::Keyword {
                    keyword,
                    modifiers,
                    args,
                    ..
                } => {
                    let namespace = match keyword.as_str() {
                        "navigate" => Namespace::Ui,
                        _ => Namespace::Term,
                    };
                    let target = match self.ws.resolve_in(self.at, namespace, &modifiers[0]) {
                        Resolution::Local(d) | Resolution::Imported { def: d, .. } => {
                            self.sigs.by_def(d).map(Target::Callable)
                        }
                        _ => None,
                    };
                    // `schedule cmd(args) at when`: the command's arguments,
                    // and its time after them, which is the schedule's
                    // (`scheduled.rs` checks it), not the command's.
                    let given = match (keyword.as_str(), modifiers.get(1).map(String::as_str)) {
                        ("schedule", Some("at" | "after")) => &args[..args.len().saturating_sub(1)],
                        _ => &args[..],
                    };
                    (target, given.iter().map(|a| (None, *a)).collect())
                }
                _ => return unknown,
            };
        let Some(target) = target else {
            return unknown;
        };
        // **A map or set a call builds, keyed by what it instantiates**
        // (ADR-0248): `Set.from_list([1.5])` is a `Set<Float>`, which no
        // annotation wrote.
        let instantiates = match &target {
            Target::Callable(sig) | Target::Member(sig, _) => sig.returns.as_ref().is_some_and(
                |r| matches!(r, TypeResolution::Resolved(t) if keyed_by_a_parameter(t)),
            ),
            _ => false,
        };
        let path = match &target {
            Target::Callable(sig) | Target::Member(sig, _) => sig.path.clone(),
            _ => String::new(),
        };
        let piped = self.piped.get(&id).copied();
        let mut solved = self.apply(self.body.expr_span(id), callee_span, piped, target, args);
        if instantiates {
            let mut keys = Vec::new();
            unordered_keys(self.sigs, &solved.result, &mut keys);
            for key in keys {
                solved.relations.push(ValueRelation {
                    declaration: self.decl.name.clone(),
                    kind: RelationKind::MapKey,
                    span: self.body.expr_span(id),
                    target: format!("`{path}`'s result"),
                    index: None,
                    outcome: Outcome::Disagree {
                        expected: MAP_KEYS.to_string(),
                        actual: self.display(&key),
                    },
                    declared_at: None,
                    boundary: (self.body.expr_span(id), format!("the call to `{path}`")),
                });
            }
        }
        solved
    }

    /// **Relate what a call supplies to what its target declares:** how many
    /// arguments, each one's name and type, and the result that follows. A
    /// clause's key is related the same way (ADR-0088).
    fn apply(
        &self,
        span: Span,
        callee_span: Span,
        piped: Option<ExprId>,
        target: Target<'a>,
        args: Vec<(Option<String>, ExprId)>,
    ) -> Solved {
        // What is supplied, in parameter order: the receiver of a member call,
        // then a piped value, then the written arguments.
        let mut supplied: Vec<(Option<String>, ExprId)> = Vec::new();
        if let Target::Member(_, base) = &target {
            supplied.push((None, *base));
        }
        if let Some(p) = piped {
            supplied.push((None, p));
        }
        let leading = supplied.len();
        supplied.extend(args);

        let Callee {
            name,
            binder,
            params,
            declared,
            result,
        } = match &target {
            Target::Callable(sig) | Target::Member(sig, _) => Callee {
                name: sig.path.clone(),
                binder: sig.definition,
                params: sig.params.clone(),
                declared: sig
                    .params
                    .iter()
                    .map(|p| {
                        p.as_ref()
                            .and_then(TypeResolution::resolved)
                            .map(|t| (sig.definition.unit, t.span()))
                    })
                    .collect(),
                // No annotation: the call's value is not stated. A command
                // answers its caller whether it committed, and its declared
                // error if not, never the value (ADR-0157), as the checker's
                // typer and the backend type it.
                result: sig.returns.as_ref().map(|r| match r {
                    TypeResolution::Resolved(t)
                        if self.sigs.kind_of(sig.definition) == Some(DeclKind::Command) =>
                    {
                        Ty::of(&ResolvedType::answered(t))
                    }
                    TypeResolution::Resolved(t) => Ty::of(t),
                    _ => Ty::Unknown,
                }),
            },
            Target::Record(def) => {
                let fields = self
                    .sigs
                    .type_decl(*def)
                    .and_then(|f| f.record.clone())
                    .unwrap_or_default();
                Callee {
                    name: self.display_def(*def),
                    binder: *def,
                    params: fields.iter().map(|(_, r)| Some(r.clone())).collect(),
                    declared: fields
                        .iter()
                        .map(|(_, r)| r.resolved().map(|t| (def.unit, t.span())))
                        .collect(),
                    result: Some(self.applied(*def)),
                }
            }
            Target::Case(def, index) => {
                let (case, fields) = self
                    .sigs
                    .type_decl(*def)
                    .and_then(|t| t.variants.as_ref())
                    .and_then(|cases| cases.get(*index))
                    .cloned()
                    .unwrap_or_default();
                Callee {
                    name: format!("{}.{case}", self.display_def(*def)),
                    binder: *def,
                    params: fields.iter().map(|r| Some(r.clone())).collect(),
                    declared: fields
                        .iter()
                        .map(|r| r.resolved().map(|t| (def.unit, t.span())))
                        .collect(),
                    result: Some(self.applied(*def)),
                }
            }
            Target::Opaque(def) => {
                let rep = self
                    .sigs
                    .type_decl(*def)
                    .and_then(|f| f.representation.clone());
                let span = rep
                    .as_ref()
                    .and_then(|r| r.resolved().map(|t| (def.unit, t.span())));
                Callee {
                    name: self.display_def(*def),
                    binder: *def,
                    params: vec![rep],
                    declared: vec![span],
                    result: Some(self.applied(*def)),
                }
            }
        };
        let field_names: Option<Vec<String>> = match &target {
            Target::Record(def) => self
                .sigs
                .type_decl(*def)
                .and_then(|f| f.record.as_ref())
                .map(|r| r.iter().map(|(n, _)| n.clone()).collect()),
            _ => None,
        };
        let kind = match &target {
            Target::Record(_) | Target::Opaque(_) => RelationKind::Field,
            _ => RelationKind::Argument,
        };

        let mut relations = Vec::new();
        let mut s = Subst::default();
        // A value built of fields that mention none of its type's `U` holds
        // at every `U`: `Either.Left(1)` is an `Either<Int, _>` (ADR-0065).
        if matches!(
            target,
            Target::Record(_) | Target::Case(..) | Target::Opaque(_)
        ) {
            s.any.extend(unmentioned(self.ws, binder, &params));
        }
        let result = result.map(|r| r.instantiate(binder));

        // A declaration with no parameters and a call with no arguments has
        // nothing to relate — a type, an event.
        if params.is_empty() && supplied.is_empty() {
            return Solved {
                result: result.map(|r| s.close(&r)).unwrap_or(Ty::Unknown),
                relations,
            };
        }
        let arity_ok = supplied.len() == params.len();
        relations.push(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Arity,
            span: span.clone(),
            target: name.clone(),
            index: None,
            outcome: match arity_ok {
                true => Outcome::Agree,
                false => Outcome::Disagree {
                    expected: params.len().to_string(),
                    actual: supplied.len().to_string(),
                },
            },
            declared_at: None,
            boundary: (
                callee_span.clone(),
                format!(
                    "`{name}` is declared with {} parameter{}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" }
                ),
            ),
        });
        // Each named argument, to the parameter of its name (ADR-0081). Until
        // 2026-09-26 a call that named one related none of its arguments.
        let mut named_fault = None;
        if supplied.iter().any(|(n, _)| n.is_some()) {
            let names: Vec<String> = match &target {
                Target::Callable(sig) | Target::Member(sig, _) => sig.names.clone(),
                Target::Record(_) => field_names.clone().unwrap_or_default(),
                // A case's fields and an opaque type's representation are
                // not named.
                Target::Case(..) | Target::Opaque(_) => Vec::new(),
            };
            let written: Vec<Option<&str>> = supplied[leading..]
                .iter()
                .map(|(n, _)| n.as_deref())
                .collect();
            match crate::signatures::arrange(&names, leading, &written) {
                Ok(order) if arity_ok => {
                    let mut arranged = supplied.clone();
                    for (k, i) in order.into_iter().enumerate() {
                        arranged[i] = supplied[leading + k].clone();
                    }
                    supplied = arranged;
                }
                Ok(_) => {}
                Err(fault) => named_fault = Some(fault),
            }
        }
        if let Some(fault) = &named_fault {
            use crate::signatures::ArgFault;
            relations.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Named,
                span: span.clone(),
                target: name.clone(),
                index: None,
                outcome: Outcome::Disagree {
                    expected: match fault {
                        ArgFault::Unknown(_) => "unknown",
                        ArgFault::Twice(_) => "twice",
                        ArgFault::AfterNamed => "after",
                    }
                    .to_string(),
                    actual: match fault {
                        ArgFault::Unknown(n) | ArgFault::Twice(n) => n.clone(),
                        ArgFault::AfterNamed => String::new(),
                    },
                },
                declared_at: None,
                boundary: (
                    callee_span.clone(),
                    format!("`{name}` names its parameters"),
                ),
            });
        }

        for (i, (_, value)) in supplied.iter().enumerate() {
            let expected = params.get(i).cloned().flatten();
            let outcome = if !arity_ok || named_fault.is_some() {
                Outcome::Undecided(Undecided::ArityMismatch)
            } else {
                match &expected {
                    Some(TypeResolution::Resolved(t)) => {
                        let e = Ty::of(t).instantiate(binder);
                        // A lambda is typed against the function type it is
                        // passed as, under what earlier arguments determined.
                        let a = match (self.body.expr(*value), s.resolve(&e)) {
                            (
                                Expr::Lambda {
                                    descriptor: None,
                                    params,
                                    body,
                                },
                                Ty::Builtin(Builtin::Function, fargs),
                            ) => {
                                let known: Vec<Ty> = fargs.iter().map(|f| s.close(f)).collect();
                                self.lambda(params, *body, &known)
                            }
                            _ => self.of(*value),
                        };
                        match unify(&mut s, &e, &a) {
                            Verdict::Agree => Outcome::Agree,
                            Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
                            // Track `notifications` (ADR-0270, amending
                            // ADR-0091): a listener's parameter that is a
                            // user's handle binds the event's value of that
                            // user's id.
                            Verdict::Disagree if self.handle_over(*value, &a, &e) => Outcome::Agree,
                            Verdict::Disagree => Outcome::Disagree {
                                expected: self.display(&s.close(&e)),
                                actual: self.display(&a),
                            },
                        }
                    }
                    Some(_) => Outcome::Undecided(Undecided::ExpectedUnresolved),
                    // A parameter the author left unannotated is not a claim.
                    None => Outcome::Undecided(Undecided::Unknown),
                }
            };
            relations.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind,
                span: self.body.expr_span(*value),
                target: match (&field_names, kind) {
                    (Some(names), RelationKind::Field) => {
                        format!("{name}.{}", names.get(i).map(String::as_str).unwrap_or("?"))
                    }
                    _ => name.clone(),
                },
                index: Some(i),
                outcome,
                declared_at: declared.get(i).cloned().flatten(),
                boundary: (span.clone(), format!("in this call to `{name}`")),
            });
        }
        // The value an opaque type is built from, against what the type
        // states every value of it holds (ADR-0179).
        if let Target::Opaque(def) = &target
            && let Some(inv) = self.sigs.type_decl(*def).and_then(|t| t.invariant.as_ref())
            && inv.unread.is_empty()
            && arity_ok
            && let Some((_, value)) = supplied.first()
        {
            // An `Int`'s value, or a `String`'s length (ADR-0225).
            let (found, described) = match inv.measure {
                crate::hir::Measure::Value => {
                    let found = self.interval_at(*value);
                    (found, found.described())
                }
                crate::hir::Measure::Length => {
                    let found = self.length_at(*value);
                    (found, found.described_length())
                }
            };
            relations.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Invariant,
                span: self.body.expr_span(*value),
                target: name.clone(),
                index: Some(0),
                outcome: match found.within(inv) {
                    true => Outcome::Agree,
                    false => Outcome::Disagree {
                        expected: inv.written.clone(),
                        actual: described,
                    },
                },
                declared_at: Some((def.unit, inv.span.clone())),
                boundary: (span.clone(), format!("`{name}` holds `{}`", inv.written)),
            });
        }
        Solved {
            result: result.map(|r| s.close(&r)).unwrap_or(Ty::Unknown),
            relations,
        }
    }

    /// **What `e` can be, where it is read** (ADR-0179): its interval,
    /// narrowed by every test made around it.
    fn interval_at(&self, e: ExprId) -> Interval {
        let facts = self.facts_at(e);
        self.interval(e, &facts, 0)
    }

    /// What the tests around `e` establish of the values it reads: an `if`'s
    /// condition in its branch, its negation in its `else`, and the left of
    /// `&` on its right, as the left of `|` fails there.
    fn facts_at(&self, e: ExprId) -> Vec<(Path, Interval)> {
        let mut out = Vec::new();
        let mut child = e;
        while let Some(&parent) = self.parents.get(&child) {
            match self.body.expr(parent) {
                Expr::If { cond, then, els } => {
                    if *then == child {
                        out.extend(self.facts(*cond, true));
                    } else if *els == Some(child) {
                        out.extend(self.facts(*cond, false));
                    }
                }
                Expr::Binary {
                    op: BinOp::And,
                    lhs,
                    rhs,
                } if *rhs == child => out.extend(self.facts(*lhs, true)),
                Expr::Binary {
                    op: BinOp::Or,
                    lhs,
                    rhs,
                } if *rhs == child => out.extend(self.facts(*lhs, false)),
                _ => {}
            }
            child = parent;
        }
        out
    }

    /// What `cond` establishes where it holds, or where it does not: a local,
    /// or a field read through one, compared with an integer.
    fn facts(&self, cond: ExprId, holds: bool) -> Vec<(Path, Interval)> {
        match self.body.expr(cond) {
            Expr::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } if holds => {
                let mut both = self.facts(*lhs, true);
                both.extend(self.facts(*rhs, true));
                both
            }
            Expr::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } if !holds => {
                let mut neither = self.facts(*lhs, false);
                neither.extend(self.facts(*rhs, false));
                neither
            }
            Expr::Unary {
                op: UnOp::Not,
                operand,
            } => self.facts(*operand, !holds),
            Expr::Binary {
                op: BinOp::Cmp(c),
                lhs,
                rhs,
            } => {
                // `n.value > 1`, or `1 < n.value` read the other way round.
                let flipped = match c.as_str() {
                    "<" => ">",
                    "<=" => ">=",
                    ">" => "<",
                    ">=" => "<=",
                    other => other,
                };
                // And `String.length(text) <= 280` (ADR-0225).
                let (path, op, k) = match (self.measured_path(*lhs), self.integer(*rhs)) {
                    (Some(path), Some(k)) => (path, c.as_str(), k),
                    _ => match (self.integer(*lhs), self.measured_path(*rhs)) {
                        (Some(k), Some(path)) => (path, flipped, k),
                        _ => return Vec::new(),
                    },
                };
                Interval::compared(op, k, holds)
                    .map(|i| vec![(path, i)])
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// A local, and the fields read through it: `n`, `n.value`.
    fn path_of(&self, e: ExprId) -> Option<Path> {
        match self.body.expr(e) {
            Expr::Name(_) => self.lexical.binder(e).map(|b| (b, Vec::new())),
            Expr::Field { base, name } => self.path_of(*base).map(|(b, mut fields)| {
                fields.push(name.clone());
                (b, fields)
            }),
            _ => None,
        }
    }

    /// **`e` is `String.length(text)`: the `text`** (ADR-0225), however the
    /// call is written, by what it resolves to.
    fn length_of(&self, e: ExprId) -> Option<ExprId> {
        let Expr::Call { callee, args } = self.body.expr(e) else {
            return None;
        };
        let [text] = args.as_slice() else {
            return None;
        };
        match named(self.sigs, self.ws, self.at, self.body, *callee) {
            Named::Target(Target::Callable(sig)) if sig.path == "String.length" => Some(text.value),
            _ => None,
        }
    }

    /// [`Typer::path_of`], or a `String`'s length (ADR-0225): its text's
    /// path, with [`LENGTH`] after it, which no field is named.
    fn measured_path(&self, e: ExprId) -> Option<Path> {
        match self.length_of(e) {
            Some(text) => self.path_of(text).map(|(b, mut fields)| {
                fields.push(LENGTH.to_string());
                (b, fields)
            }),
            None => self.path_of(e),
        }
    }

    /// **What a `String`'s length can be, where it is read** (ADR-0225),
    /// narrowed by every test made around it.
    fn length_at(&self, e: ExprId) -> Interval {
        let facts = self.facts_at(e);
        self.length(e, &facts, 0)
    }

    /// **The interval of a `String`'s length, in code points, under
    /// `facts`** (ADR-0225), as [`Typer::interval`] is an `Int`'s: exact for
    /// a literal; an opaque value's representation as its type bounds it; a
    /// `let`'s local as its initializer, where the `let` is; either branch of
    /// an `if` or a `match`. Anything else is any length, and a test of it
    /// narrows it.
    fn length(&self, e: ExprId, facts: &[(Path, Interval)], depth: u32) -> Interval {
        let any = Interval {
            lo: Some(0),
            hi: None,
        };
        if depth > 64 {
            return any;
        }
        let next = depth + 1;
        let natural = match self.body.expr(e) {
            Expr::Literal(l) => l
                .string_value()
                .map_or(any, |s| Interval::exactly(s.chars().count() as i128)),
            Expr::Field { base, name } if name == "value" => match self.of(*base) {
                Ty::Nominal(def, _) => self
                    .sigs
                    .type_decl(def)
                    .and_then(|t| t.invariant.as_ref())
                    .filter(|i| i.unread.is_empty() && i.measure == crate::hir::Measure::Length)
                    .map_or(any, |i| Interval {
                        lo: Some(i.at_least.unwrap_or(0).max(0)),
                        hi: i.at_most,
                    }),
                _ => any,
            },
            Expr::Name(_) => match self.lexical.binder(e) {
                Some(Binder::Pattern(p)) => match self.lets.get(&p) {
                    Some(init) => {
                        let there = self.facts_at(*init);
                        self.length(*init, &there, next)
                    }
                    None => any,
                },
                _ => any,
            },
            Expr::Block { stmts } => stmts
                .last()
                .map_or(any, |last| self.length(*last, facts, next)),
            Expr::If {
                cond,
                then,
                els: Some(els),
            } => {
                let mut yes = facts.to_vec();
                yes.extend(self.facts(*cond, true));
                let mut no = facts.to_vec();
                no.extend(self.facts(*cond, false));
                self.length(*then, &yes, next)
                    .hull(self.length(*els, &no, next))
            }
            Expr::Match { arms, .. } => arms
                .iter()
                .map(|a| self.length(a.body, facts, next))
                .reduce(Interval::hull)
                .unwrap_or(any),
            _ => any,
        };
        match self.path_of(e) {
            Some((binder, mut fields)) => {
                fields.push(LENGTH.to_string());
                let path = (binder, fields);
                facts
                    .iter()
                    .filter(|(p, _)| *p == path)
                    .fold(natural, |acc, (_, i)| acc.meet(*i))
            }
            None => natural,
        }
    }

    /// An integer literal, or one negated.
    fn integer(&self, e: ExprId) -> Option<i128> {
        match self.body.expr(e) {
            Expr::Literal(Literal::Int(s)) => s.replace('_', "").parse().ok(),
            Expr::Unary {
                op: UnOp::Neg,
                operand,
            } => self.integer(*operand).and_then(i128::checked_neg),
            _ => None,
        }
    }

    /// **The interval of an `Int` expression** under `facts` (ADR-0179):
    /// exact for a literal and for arithmetic on bounded values; an opaque
    /// value's representation as its type bounds it; a `let`'s local as its
    /// initializer, where the `let` is; either branch of an `if` or a
    /// `match`. Anything else is any `Int`, and a test of it narrows it.
    fn interval(&self, e: ExprId, facts: &[(Path, Interval)], depth: u32) -> Interval {
        // A chain of `let`s, each read through the last, ends; this bounds
        // the walk where one would not.
        if depth > 64 {
            return Interval::ANY;
        }
        let next = depth + 1;
        let natural = match self.body.expr(e) {
            _ if self.integer(e).is_some() => {
                self.integer(e).map_or(Interval::ANY, Interval::exactly)
            }
            Expr::Unary {
                op: UnOp::Neg,
                operand,
            } => self.interval(*operand, facts, next).neg(),
            Expr::Binary { op, lhs, rhs } if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) => {
                let (a, b) = (
                    self.interval(*lhs, facts, next),
                    self.interval(*rhs, facts, next),
                );
                match op {
                    BinOp::Add => a.add(b),
                    BinOp::Sub => a.add(b.neg()),
                    _ => a.mul(b),
                }
            }
            Expr::Field { base, name } if name == "value" => match self.of(*base) {
                Ty::Nominal(def, _) => self
                    .sigs
                    .type_decl(def)
                    .and_then(|t| t.invariant.as_ref())
                    .filter(|i| i.unread.is_empty() && i.measure == crate::hir::Measure::Value)
                    .map_or(Interval::ANY, |i| Interval {
                        lo: i.at_least,
                        hi: i.at_most,
                    }),
                _ => Interval::ANY,
            },
            Expr::Name(_) => match self.lexical.binder(e) {
                Some(Binder::Pattern(p)) => match self.lets.get(&p) {
                    Some(init) => {
                        let there = self.facts_at(*init);
                        self.interval(*init, &there, next)
                    }
                    None => Interval::ANY,
                },
                _ => Interval::ANY,
            },
            Expr::Block { stmts } => stmts
                .last()
                .map_or(Interval::ANY, |last| self.interval(*last, facts, next)),
            Expr::If {
                cond,
                then,
                els: Some(els),
            } => {
                let mut yes = facts.to_vec();
                yes.extend(self.facts(*cond, true));
                let mut no = facts.to_vec();
                no.extend(self.facts(*cond, false));
                self.interval(*then, &yes, next)
                    .hull(self.interval(*els, &no, next))
            }
            Expr::Match { arms, .. } => arms
                .iter()
                .map(|a| self.interval(a.body, facts, next))
                .reduce(Interval::hull)
                .unwrap_or(Interval::ANY),
            _ => Interval::ANY,
        };
        match self.path_of(e) {
            Some(path) => facts
                .iter()
                .filter(|(p, _)| *p == path)
                .fold(natural, |acc, (_, i)| acc.meet(*i)),
            None => natural,
        }
    }

    /// **A call through a function value** (ADR-0068): a local, a parameter,
    /// or a binding around a nested declaration, called. It is checked
    /// against the value's function type as a declared callee's call is: its
    /// arity, each argument, and its result. A value of another type is not
    /// called (PW0614). Until 2026-09-26 nothing was checked: `f("a")`, where
    /// `f` is a `fn(Int) -> Int`, passed.
    fn value_call(&self, id: ExprId, callee: ExprId, args: &[crate::hir::Arg]) -> Solved {
        let name = path_of(self.body, callee);
        let span = self.body.expr_span(id);
        let boundary = || (span.clone(), format!("in this call to `{name}`"));
        let relation =
            |kind: RelationKind, at: Span, index: Option<usize>, outcome: Outcome| ValueRelation {
                declaration: self.decl.name.clone(),
                kind,
                span: at,
                target: name.clone(),
                index,
                outcome,
                declared_at: None,
                boundary: boundary(),
            };
        let supplied: Vec<ExprId> = self
            .piped
            .get(&id)
            .copied()
            .into_iter()
            .chain(args.iter().map(|a| a.value))
            .collect();
        let (params, result) = match self.of(callee) {
            Ty::Builtin(Builtin::Function, mut xs) if !xs.is_empty() => {
                let r = xs.pop().unwrap_or(Ty::Unknown);
                (xs, r)
            }
            // A value of any type is a function too, of anything.
            Ty::Any => {
                return Solved {
                    result: Ty::Any,
                    relations: Vec::new(),
                };
            }
            Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. } => {
                return Solved {
                    result: Ty::Unknown,
                    relations: Vec::new(),
                };
            }
            other => {
                return Solved {
                    result: Ty::Unknown,
                    relations: vec![relation(
                        RelationKind::Callee,
                        self.body.expr_span(callee),
                        None,
                        Outcome::Disagree {
                            expected: "a function".to_string(),
                            actual: self.display(&other),
                        },
                    )],
                };
            }
        };
        let mut relations = vec![relation(
            RelationKind::Arity,
            span.clone(),
            None,
            match supplied.len() == params.len() {
                true => Outcome::Agree,
                false => Outcome::Disagree {
                    expected: params.len().to_string(),
                    actual: supplied.len().to_string(),
                },
            },
        )];
        // A function value's parameters have no names (ADR-0081).
        let named = args.iter().find_map(|a| a.name.clone());
        if let Some(n) = &named {
            relations.push(relation(
                RelationKind::Named,
                span.clone(),
                None,
                Outcome::Disagree {
                    expected: "unnamed".to_string(),
                    actual: n.clone(),
                },
            ));
        }
        if supplied.len() == params.len() {
            for (i, (value, expected)) in supplied.iter().zip(&params).enumerate() {
                let outcome = if named.is_some() {
                    Outcome::Undecided(Undecided::ArityMismatch)
                } else {
                    // A lambda is typed against the function type it is
                    // passed as.
                    let actual = match (self.body.expr(*value), expected) {
                        (
                            Expr::Lambda {
                                descriptor: None,
                                params,
                                body,
                            },
                            Ty::Builtin(Builtin::Function, fargs),
                        ) => self.lambda(params, *body, fargs),
                        _ => self.of(*value),
                    };
                    match unify(&mut Subst::default(), expected, &actual) {
                        Verdict::Agree => Outcome::Agree,
                        Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
                        Verdict::Disagree => Outcome::Disagree {
                            expected: self.display(expected),
                            actual: self.display(&actual),
                        },
                    }
                };
                relations.push(relation(
                    RelationKind::Argument,
                    self.body.expr_span(*value),
                    Some(i),
                    outcome,
                ));
            }
        }
        Solved { result, relations }
    }

    /// `Ok(x)`, `Err(e)`, `Some(x)` — the language's own constructors, where the
    /// program has not declared a term of that name.
    fn intrinsic(&self, callee: ExprId, args: &[crate::hir::Arg]) -> Option<Ty> {
        let Expr::Name(n) = self.body.expr(callee) else {
            return None;
        };
        if !matches!(n.as_str(), "Ok" | "Err" | "Some")
            || !matches!(
                self.ws.resolve_in(self.at, Namespace::Term, n),
                Resolution::Unresolved
            )
        {
            return None;
        }
        let [arg] = args else {
            return Some(Ty::Unknown);
        };
        let v = self.of(arg.value);
        Some(match n.as_str() {
            "Ok" => Ty::Builtin(Builtin::Result, vec![v, Ty::Any]),
            "Err" => Ty::Builtin(Builtin::Result, vec![Ty::Any, v]),
            _ => Ty::Builtin(Builtin::Option, vec![v]),
        })
    }

    /// A type declaration applied to fresh variables for its parameters.
    fn applied(&self, def: DefId) -> Ty {
        let arity = self.ws.type_arity(def).unwrap_or(0);
        Ty::Nominal(
            def,
            (0..arity as u32)
                .map(|index| Ty::Parameter { binder: def, index })
                .collect(),
        )
    }

    /// `Cart { lines: .., total: .. }`: a record built by field name.
    fn construct(&self, id: ExprId) -> Solved {
        let unknown = Solved {
            result: Ty::Unknown,
            relations: Vec::new(),
        };
        let Expr::Record {
            name: Some(name),
            fields,
        } = self.body.expr(id)
        else {
            return unknown;
        };
        let found = match name.contains('.') {
            true => self.ws.resolve_path_in(self.at, Namespace::Type, name),
            false => self.ws.resolve_in(self.at, Namespace::Type, name),
        };
        let def = match found {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
            _ => return unknown,
        };
        let Some(declared) = self.sigs.type_decl(def).and_then(|f| f.record.as_ref()) else {
            return unknown;
        };
        let mut s = Subst::default();
        let mut relations = Vec::new();
        // The fields it is built with, against the fields its type declares,
        // each once (ADR-0067).
        let record = self.display_def(def);
        let fields_relation = |span: Span, expected: &str, field: &str| ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Fields,
            span,
            target: record.clone(),
            index: None,
            outcome: match expected.is_empty() {
                true => Outcome::Agree,
                false => Outcome::Disagree {
                    expected: expected.to_string(),
                    actual: field.to_string(),
                },
            },
            declared_at: None,
            boundary: (
                self.body.expr_span(id),
                format!("in this construction of `{record}`"),
            ),
        };
        let mut given: BTreeSet<&str> = BTreeSet::new();
        let mut whole = true;
        for (i, init) in fields.iter().enumerate() {
            let Some((index, (_, expected))) = declared
                .iter()
                .enumerate()
                .find(|(_, (n, _))| *n == init.name)
            else {
                relations.push(fields_relation(
                    init.span.clone(),
                    FIELD_UNDECLARED,
                    &init.name,
                ));
                whole = false;
                continue;
            };
            if !given.insert(init.name.as_str()) {
                relations.push(fields_relation(init.span.clone(), FIELD_ONCE, &init.name));
                whole = false;
            }
            let actual = match init.value {
                Some(v) => self.of(v),
                // `Point { x, y }` — the shorthand names a binding.
                None => match self.lexical.shorthand(id, i) {
                    Some(b) => self.local(b),
                    None => self.global(&init.name),
                },
            };
            let outcome = match expected {
                TypeResolution::Resolved(t) => {
                    let e = Ty::of(t).instantiate(def);
                    match unify(&mut s, &e, &actual) {
                        Verdict::Agree => Outcome::Agree,
                        Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
                        Verdict::Disagree => Outcome::Disagree {
                            expected: self.display(&s.close(&e)),
                            actual: self.display(&actual),
                        },
                    }
                }
                _ => Outcome::Undecided(Undecided::ExpectedUnresolved),
            };
            relations.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Field,
                span: match init.value {
                    Some(v) => self.body.expr_span(v),
                    None => init.span.clone(),
                },
                target: format!("{}.{}", self.display_def(def), init.name),
                index: Some(index),
                outcome,
                declared_at: expected.resolved().map(|t| (def.unit, t.span())),
                boundary: (
                    self.body.expr_span(id),
                    format!("in this construction of `{}`", self.display_def(def)),
                ),
            });
        }
        for (name, _) in declared {
            if !given.contains(name.as_str()) {
                relations.push(fields_relation(self.body.expr_span(id), FIELD_GIVEN, name));
                whole = false;
            }
        }
        if whole {
            relations.push(fields_relation(self.body.expr_span(id), "", ""));
        }
        s.any.extend(unmentioned(
            self.ws,
            def,
            &declared
                .iter()
                .map(|(_, r)| Some(r.clone()))
                .collect::<Vec<_>>(),
        ));
        Solved {
            result: s.close(&self.applied(def).instantiate(def)),
            relations,
        }
    }

    // --- display ---------------------------------------------------------------------

    /// A type as a diagnostic shows it. Short names, qualified only where two
    /// declarations in one message would otherwise read the same — the case
    /// this checker exists to tell apart.
    fn display(&self, t: &Ty) -> String {
        match t {
            Ty::Primitive(p) => p.name().to_string(),
            Ty::Builtin(Builtin::Function, args) => {
                let (result, params) = match args.split_last() {
                    Some((r, p)) => (self.display(r), p),
                    None => ("?".to_string(), &args[..]),
                };
                format!(
                    "fn({}) -> {result}",
                    params
                        .iter()
                        .map(|a| self.display(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Ty::Builtin(b, args) => format!(
                "{}<{}>",
                b.name(),
                args.iter()
                    .map(|a| self.display(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Nominal(d, args) => {
                let base = self.display_def(*d);
                match args.is_empty() {
                    true => base,
                    false => format!(
                        "{base}<{}>",
                        args.iter()
                            .map(|a| self.display(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }
            }
            Ty::Parameter { binder, index } => {
                let name = self
                    .decl
                    .type_params
                    .get(*index as usize)
                    .filter(|_| self.own_def() == Some(*binder))
                    .cloned();
                name.unwrap_or_else(|| format!("type parameter {index}"))
            }
            Ty::Var(_) | Ty::Unknown => "?".to_string(),
            Ty::Any => "_".to_string(),
        }
    }

    /// The declaration's qualified path. The qualification is kept: two
    /// `SessionId`s are the reason this checker exists, and a message showing
    /// `SessionId` against `SessionId` would describe a defect it cannot name.
    fn display_def(&self, d: DefId) -> String {
        self.sigs
            .path_of(d)
            .map(str::to_string)
            .unwrap_or_else(|| "?".to_string())
    }

    /// **Every binding the body's own shape types** (ADR-0063): an
    /// unannotated `let` or `use` by its initialiser, a match arm's names by
    /// the scrutinee's payload, a `for` loop's name by its list's element, a
    /// template's `{#each}` and `{#match}` arm names by their block's, and a
    /// policy term's binders by its header. Returns whether any binding was
    /// newly typed, or more completely.
    fn solve_bindings(&self) -> bool {
        let mut added = false;
        let mut trees = vec![self.body.root];
        trees.extend(self.decl.term_roots().map(|(_, r)| r.root));
        for id in trees.iter().flat_map(|t| self.body.walk_from(*t)) {
            match self.body.expr(id) {
                Expr::Let {
                    pat: Some(pat),
                    ty: None,
                    init: Some(init),
                } if matches!(self.body.pat(*pat), Pattern::Bind { .. }) => {
                    // A `let mut` holds one type: `let mut xs = []` is a list
                    // of what is assigned to it, not of anything (ADR-0065).
                    let t = match self.body.pat(*pat) {
                        Pattern::Bind { mutable: true, .. } => one_type(self.of(*init)),
                        _ => self.of(*init),
                    };
                    added |= self.bind(Binder::Pattern(*pat), t);
                }
                // `x = e` completes a `let mut`'s type where its first value
                // left it incomplete.
                Expr::Binary {
                    op: BinOp::Assign,
                    lhs,
                    rhs,
                } => {
                    if let Some(b) = self.lexical.binder(*lhs) {
                        added |= self.bind(b, one_type(self.of(*rhs)));
                    }
                }
                Expr::Keyword { keyword, args, .. } if keyword == "use" => {
                    if let Some(init) = args.first() {
                        added |= self.bind(Binder::Use(id), self.of(*init));
                    }
                }
                Expr::Match { scrutinee, arms } => {
                    let st = self.of(*scrutinee);
                    for arm in arms {
                        for (b, t) in self.arm_bindings(&st, arm.pat) {
                            added |= self.bind(b, t);
                        }
                    }
                }
                // `for x in xs` over a `List<T>`: each `x` is a `T`. The loop
                // takes a list and binds one name (ADR-0051).
                Expr::For {
                    pat: Some(pat),
                    iterable,
                    ..
                } if matches!(self.body.pat(*pat), Pattern::Bind { .. }) => {
                    if let Ty::Builtin(Builtin::List, args) = self.of(*iterable)
                        && let [element] = args.as_slice()
                    {
                        added |= self.bind(Binder::Pattern(*pat), element.clone());
                    }
                }
                _ => {}
            }
        }
        for (b, t) in self.term_binders() {
            added |= self.bind(b, t);
        }
        // `{#each xs as x}`: each `x` is an element of `xs`, the list typed
        // as any expression is (ADR-0345). Until 2026-10-15 it was read from
        // the directive's text, a name and the fields read from it, and a
        // computed list's rows had no type.
        for (node, list, _) in self.lexical.each_blocks() {
            if let Ty::Builtin(Builtin::List, args) = self.of(list)
                && let [element] = args.as_slice()
            {
                added |= self.bind(Binder::Each(node), element.clone());
            }
        }
        // `release(h) { .. }`: what the resource's `acquire { .. }` produced,
        // its success where it can fail.
        for (clause, acquired) in self.lexical.released() {
            let t = match self.of(acquired) {
                Ty::Builtin(Builtin::Result, mut args) if args.len() == 2 => args.swap_remove(0),
                t => t,
            };
            added |= self.bind(Binder::Clause(clause, 0), t);
        }
        // `<ready as={x}>`: the stream's answer when it succeeds, and
        // `<failed as={e}>` its failure: `Some` of its declared error, or
        // `None` for the host's (ADR-0148).
        for (node, query, ok) in self.lexical.stream_parts() {
            let t = match (self.of(query), ok) {
                (Ty::Builtin(Builtin::Result, mut args), true) if args.len() == 2 => {
                    args.swap_remove(0)
                }
                (Ty::Builtin(Builtin::Result, mut args), false) if args.len() == 2 => {
                    Ty::Builtin(Builtin::Option, vec![args.swap_remove(1)])
                }
                (t, true) => t,
                (_, false) => Ty::Unknown,
            };
            added |= self.bind(Binder::Stream(node), t);
        }
        // `{:Some(x)}`, `{:Rect(w, h)}`: each name is its field of the case
        // the block's subject holds, as a match arm's is.
        for (node, subject) in self.lexical.template_arms() {
            let crate::hir::Node::Branch { arm: Some(arm), .. } = self.body.node(node) else {
                continue;
            };
            let st = self.of(subject);
            let Some(fields) = self.pattern_fields(&st, &arm.case, arm.bindings.len()) else {
                continue;
            };
            for (i, f) in fields.into_iter().enumerate() {
                added |= self.bind(Binder::Arm(node, i), f);
            }
        }
        added
    }

    /// **Whether a loop's list is read through a member function** on the
    /// way (ADR-0169): `store.items`, where `items` is declared on `Store`
    /// rather than a field of it. A path's reads, each as it is typed.
    fn reads_a_member(&self, list: ExprId) -> bool {
        let mut at = list;
        while let Expr::Field { base, name } = self.body.expr(at) {
            if self.calls_a_member(&self.of(*base), name) {
                return true;
            }
            at = *base;
        }
        false
    }

    /// **Record the type a binding holds.** A binding keeps the first type
    /// it is given, unless a later round gives it a complete type its
    /// incomplete one agrees with: `List<?>` becomes `List<Int>`, and a
    /// complete type is never changed. So the rounds end.
    ///
    /// Every type recorded is the typer's own, closed over each callee's
    /// parameters. Until ADR-0063 the bindings began as `infer.rs`'s, which
    /// typed `let ys = List.filter(xs, ..)` by `filter`'s declared `List<T>`,
    /// and a correct `List.sort_by(ys, compare)` was refused (PW0605) for a
    /// mismatch the typer had made (found 2026-09-25).
    fn bind(&self, b: Binder, t: Ty) -> bool {
        if t.is_unknown() {
            return false;
        }
        let mut locals = self.locals.borrow_mut();
        match locals.get(&b) {
            None => {
                locals.insert(b, t);
                true
            }
            Some(old)
                if !old.is_closed()
                    && t.is_closed()
                    && unify(&mut Subst::default(), old, &t) != Verdict::Disagree =>
            {
                locals.insert(b, t);
                true
            }
            Some(_) => false,
        }
    }

    /// Is this declaration one whose body renders an element: a view, a
    /// component, a page?
    fn is_ui(&self) -> bool {
        matches!(
            self.decl.kind,
            DeclKind::View | DeclKind::Component | DeclKind::Page | DeclKind::Layout
        )
    }

    fn own_def(&self) -> Option<DefId> {
        let ns = Namespace::of(self.decl.kind)?;
        match self.ws.resolve_in(self.at, ns, &self.decl.name) {
            Resolution::Local(d) => Some(d),
            _ => None,
        }
    }

    /// **A lambda's parameters, as its use gives them**: the function type of
    /// the parameter it is passed as, of a `let`'s written type, or of the
    /// declared result it is returned as. Returns whether any parameter was
    /// newly typed.
    ///
    /// The relations walk reads a lambda's body on its own, with the body's
    /// bindings. A parameter was typed only while its call was solved, so in
    /// that walk it was untyped, or typed by `infer.rs`'s narrower rule
    /// (ADR-0053). A parameter keeps the closed type its use gives it, by
    /// where it is bound, whatever else binds its name (ADR-0063).
    fn solve_lambdas(&self) -> bool {
        let is_lambda = |id: ExprId| matches!(self.body.expr(id), Expr::Lambda { .. });
        for id in self.body.walk() {
            match self.body.expr(id) {
                Expr::Call { args, .. } if args.iter().any(|a| is_lambda(a.value)) => {
                    let _ = self.call(id);
                }
                Expr::Let {
                    ty: Some(ty),
                    init: Some(init),
                    ..
                } if is_lambda(*init) => {
                    let Some(written) = crate::resolved::written_in_body(self.body, *ty) else {
                        continue;
                    };
                    let span = self.body.expr_span(*init);
                    if let Some(t) = self
                        .sigs
                        .resolve_type(self.module, self.decl, &written, span)
                        .resolved()
                    {
                        self.lambda_as(*init, &Ty::of(t));
                    }
                }
                _ => {}
            }
        }
        let result = self
            .own_def()
            .and_then(|d| self.sigs.by_def(d))
            .and_then(Signature::result)
            .map(Ty::of);
        if let Some(result) = result {
            for site in self.result_sites() {
                self.lambda_as(site, &result);
            }
        }
        let solved = std::mem::take(&mut *self.solved.borrow_mut());
        let mut added = false;
        for (b, t) in solved {
            added |= self.bind(b, t);
        }
        added
    }

    /// Type a lambda against a function type it is used as, for the
    /// parameters that records.
    fn lambda_as(&self, id: ExprId, expected: &Ty) {
        if let (
            Expr::Lambda {
                descriptor: None,
                params,
                body,
            },
            Ty::Builtin(Builtin::Function, fargs),
        ) = (self.body.expr(id), expected)
        {
            let _ = self.lambda(params, *body, fargs);
        }
    }

    // --- the relations -----------------------------------------------------------------

    /// Every place in this body a value meets a declared type.
    fn relations(&self, out: &mut Vec<ValueRelation>) {
        self.relations_from(self.body.root, out);
        self.template_relations(out);
    }

    /// **What a template's blocks and events take** (ADR-0071), as the
    /// renderer runs them:
    /// - `{#each xs}` runs over a list;
    /// - `{#if c}` and `{:else if c}` test a value that has a truth, which an
    ///   `Option`, a `Result` or a sum type has not: each is taken apart with
    ///   `{#match}`, and the renderer refuses it;
    /// - an event attribute is given a function to call.
    ///
    /// Until 2026-09-26 none was related, so each passed `pw check`, and the
    /// first two failed only when rendered.
    fn template_relations(&self, out: &mut Vec<ValueRelation>) {
        let unknown = |t: &Ty| matches!(t, Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. });
        for (node, list, _) in self.lexical.each_blocks() {
            let t = self.of(list);
            let outcome = match &t {
                Ty::Builtin(Builtin::List, _) | Ty::Any => Outcome::Agree,
                t if unknown(t) => Outcome::Undecided(Undecided::Unknown),
                other => Outcome::Disagree {
                    expected: "List<_>".to_string(),
                    actual: self.display(other),
                },
            };
            let span = self.body.node_span(node);
            out.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Operand,
                span: span.clone(),
                target: "the list an `{#each}` runs over".to_string(),
                index: None,
                outcome,
                declared_at: None,
                boundary: (span.clone(), "this block".to_string()),
            });
            // The fields the list and the key are read through are related
            // where each is read, as any field read is (ADR-0074): the head
            // is expressions (ADR-0345), which the walk reaches. The key is
            // written as text, where it is read from the row; a key read
            // from another name is PW5021's.
            let crate::hir::Node::Block {
                each: Some(each), ..
            } = self.body.node(node)
            else {
                continue;
            };
            let Some(key) = each.key else {
                continue;
            };
            let from_row = crate::lexical::path_root(self.body, key)
                .and_then(|root| self.lexical.binder(root))
                == Some(Binder::Each(node));
            if !from_row {
                continue;
            }
            let written = each.written.key.clone().unwrap_or_default();
            out.extend(self.text(self.of(key), format!("({written})"), span.clone()));
        }
        let mut roots = Vec::new();
        for id in self.body.walk() {
            if let Expr::Template { roots: r, .. } = self.body.expr(id) {
                roots.extend(r.iter().copied());
            }
        }
        for n in self.body.walk_markup(&roots) {
            match self.body.node(n) {
                crate::hir::Node::Block {
                    directive,
                    subject: Some(c),
                    ..
                } if directive.trim_start().starts_with("{#if") => {
                    out.extend(self.truth(*c, "{#if}", true));
                }
                crate::hir::Node::Branch {
                    condition: Some(c), ..
                } => out.extend(self.truth(*c, "{:else if}", false)),
                crate::hir::Node::Interpolation(e) => {
                    out.extend(self.text(self.of(*e), "{..}".to_string(), self.body.expr_span(*e)));
                }
                crate::hir::Node::Element { tag, attrs, .. } if self.view_named(tag).is_some() => {
                    // A view's attributes are its props (ADR-0136), not text.
                    out.extend(self.props(n, tag, attrs));
                }
                crate::hir::Node::Element { tag, attrs, .. } => {
                    // `<map-container resource={StoreMap} center={c} />` mounts
                    // a resource, and its attributes are the resource's
                    // arguments (A-007), not text.
                    let mounts = attrs.iter().any(crate::template_ir::mounts);
                    for a in attrs {
                        // A value the element writes: as text, or as a boolean
                        // attribute's presence (ADR-0074). Not written: a
                        // directive, which does not build unless it is `on:`
                        // (ADR-0073); a stream's query, and the name a stream's
                        // part binds.
                        let directive = a.name.split_once(':').is_some_and(|(prefix, _)| {
                            !matches!(prefix, "xml" | "xlink" | "xmlns")
                        });
                        let written = !directive
                            && !mounts
                            && !(tag == "stream" && a.name == "query")
                            && !(matches!(tag.as_str(), "ready" | "failed") && a.name == "as");
                        if let crate::hir::AttrValue::Expr(e) = &a.value
                            && written
                        {
                            match self.body.expr(*e) {
                                Expr::Interpolated { parts, .. } => {
                                    for h in parts {
                                        out.extend(self.text(
                                            self.of(*h),
                                            a.name.clone(),
                                            self.body.expr_span(*h),
                                        ));
                                    }
                                }
                                _ if crate::template_ir::BOOLEAN_ATTRIBUTES
                                    .contains(&a.name.as_str()) =>
                                {
                                    out.extend(self.truth(*e, &a.name, false));
                                }
                                _ => out.extend(self.text(
                                    self.of(*e),
                                    a.name.clone(),
                                    self.body.expr_span(*e),
                                )),
                            }
                        }
                        let (Some(event), crate::hir::AttrValue::Expr(e)) =
                            (a.name.strip_prefix("on:"), &a.value)
                        else {
                            continue;
                        };
                        let outcome = match self.body.expr(*e) {
                            Expr::Lambda { .. } => Outcome::Agree,
                            _ => match self.of(*e) {
                                Ty::Builtin(Builtin::Function, _) | Ty::Any => Outcome::Agree,
                                t if unknown(&t) => Outcome::Undecided(Undecided::Unknown),
                                other => Outcome::Disagree {
                                    expected: "a function".to_string(),
                                    actual: self.display(&other),
                                },
                            },
                        };
                        out.push(ValueRelation {
                            declaration: self.decl.name.clone(),
                            kind: RelationKind::Handler,
                            span: self.body.expr_span(*e),
                            target: format!("on:{event}"),
                            index: None,
                            outcome,
                            declared_at: None,
                            boundary: (a.span.clone(), "this attribute".to_string()),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// **The view an element named with a capital letter uses** (ADR-0136),
    /// as this unit resolves the name, and its signature.
    fn view_named(&self, tag: &str) -> Option<(DefId, &crate::signatures::Signature)> {
        if !tag.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        // A view is rendered, never called: it is named in the namespace a
        // page and a component are.
        let def = match self.ws.resolve_in(self.at, Namespace::Ui, tag) {
            Resolution::Local(d) | Resolution::Imported { def: d, .. } => d,
            _ => return None,
        };
        if self.sigs.kind_of(def) != Some(crate::hir::DeclKind::View) {
            return None;
        }
        Some((def, self.sigs.by_def(def)?))
    }

    /// **A view's props, against its parameters** (ADR-0136): each prop names
    /// a parameter and has its type, and each parameter is given. A prop
    /// written twice is an attribute written twice, which the markup rules
    /// refuse on every element (PW0028); the second is not read again here.
    fn props(
        &self,
        node: crate::hir::NodeId,
        tag: &str,
        attrs: &[crate::hir::Attr],
    ) -> Vec<ValueRelation> {
        let Some((def, sig)) = self.view_named(tag) else {
            return Vec::new();
        };
        let span = self.body.node_span(node);
        let presence = |at: Span, expected: &str, actual: &str| ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Props,
            span: at,
            target: tag.to_string(),
            index: None,
            outcome: Outcome::Disagree {
                expected: expected.to_string(),
                actual: actual.to_string(),
            },
            declared_at: None,
            boundary: (span.clone(), format!("this use of `{tag}`")),
        };
        let mut out = Vec::new();
        let mut given = BTreeSet::new();
        for a in attrs {
            let Some(i) = sig.names.iter().position(|n| *n == a.name) else {
                out.push(presence(a.span.clone(), "unknown", &a.name));
                continue;
            };
            if !given.insert(i) {
                continue;
            }
            let crate::hir::AttrValue::Expr(e) = &a.value else {
                out.push(presence(a.span.clone(), "a value", &a.name));
                continue;
            };
            let actual = self.of(*e);
            let outcome = match sig.params.get(i) {
                Some(Some(TypeResolution::Resolved(t))) => {
                    let expected = Ty::of(t);
                    match unify(&mut Subst::default(), &expected, &actual) {
                        Verdict::Agree => Outcome::Agree,
                        Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
                        Verdict::Disagree => Outcome::Disagree {
                            expected: self.display(&expected),
                            actual: self.display(&actual),
                        },
                    }
                }
                _ => Outcome::Undecided(Undecided::ExpectedUnresolved),
            };
            out.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Argument,
                span: self.body.expr_span(*e),
                target: format!("{tag}'s prop `{}`", a.name),
                index: Some(i),
                outcome,
                declared_at: sig.params[i]
                    .as_ref()
                    .and_then(TypeResolution::resolved)
                    .map(|t| (def.unit, t.span())),
                boundary: (span.clone(), format!("this use of `{tag}`")),
            });
        }
        for (i, name) in sig.names.iter().enumerate() {
            if !given.contains(&i) {
                out.push(presence(span.clone(), "given", name));
            }
        }
        out
    }

    /// A template condition: a value that has a truth (ADR-0071). An
    /// `Option` and a `Result` may be absent (PW0600): as an `{#if}`'s
    /// subject `template_blocks` says so, and anywhere else this relation
    /// does (ADR-0074). Until 2026-09-26 an `{:else if}` over one was
    /// reported by neither.
    /// **A template's condition has a truth** (ADR-0071, ruling 0071-a): a
    /// `Bool`, or a `List` or a `String`, tested non-empty. A sum type is
    /// taken apart with `{#match}`; a number, a record or anything else has
    /// none (PW0609): a record is always true, and a number tested as one
    /// does not say what it tests, true for a negative one. Until 0071-a both
    /// were the renderer's truth, and checked clean.
    fn truth(&self, c: ExprId, block: &str, if_subject: bool) -> Option<ValueRelation> {
        let t = self.of(c);
        let outcome = match &t {
            Ty::Builtin(Builtin::Option | Builtin::Result, _) if if_subject => return None,
            Ty::Builtin(Builtin::Option | Builtin::Result, _) => {
                return Some(self.absent(&t, block.to_string(), self.body.expr_span(c)));
            }
            Ty::Primitive(Primitive::Bool | Primitive::Str)
            | Ty::Builtin(Builtin::List, _)
            | Ty::Any => Outcome::Agree,
            Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. } => {
                Outcome::Undecided(Undecided::Unknown)
            }
            Ty::Nominal(d, _)
                if self
                    .sigs
                    .type_decl(*d)
                    .is_some_and(|t| t.variants.is_some()) =>
            {
                Outcome::Disagree {
                    expected: String::new(),
                    actual: self.display(&t),
                }
            }
            Ty::Primitive(Primitive::Int | Primitive::Float) => Outcome::Disagree {
                expected: "a number".to_string(),
                actual: self.display(&t),
            },
            other => Outcome::Disagree {
                expected: "no truth".to_string(),
                actual: self.display(other),
            },
        };
        Some(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Truth,
            span: self.body.expr_span(c),
            target: block.to_string(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (self.body.expr_span(c), "this condition".to_string()),
        })
    }

    /// **A value a template writes as text** (ADR-0074): a `String`, an `Int`
    /// or a `Bool`, or an opaque type over one, which is written as its
    /// representation. An `Option` or a `Result` may be absent (PW0600). A
    /// `Float` has no format yet (KNOWN_LIMITATIONS). Until 2026-09-26 none
    /// was related, and the renderer refused each when it rendered.
    fn text(&self, t: Ty, target: String, span: crate::hir::Span) -> Option<ValueRelation> {
        let outcome = match &t {
            Ty::Primitive(Primitive::Str | Primitive::Int | Primitive::Bool) | Ty::Any => {
                Outcome::Agree
            }
            Ty::Builtin(Builtin::Option | Builtin::Result, _) => {
                return Some(self.absent(&t, target, span));
            }
            Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. } => {
                Outcome::Undecided(Undecided::Unknown)
            }
            other => match self.representation(other, "value") {
                Some((_, rep)) => return self.text(rep, target, span),
                None => Outcome::Disagree {
                    expected: "a `String`, an `Int` or a `Bool`".to_string(),
                    actual: self.display(other),
                },
            },
        };
        Some(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Text,
            span: span.clone(),
            target,
            index: None,
            outcome,
            declared_at: None,
            boundary: (span, "written here".to_string()),
        })
    }

    /// A value that may be absent, where a template reads it as present.
    fn absent(&self, t: &Ty, target: String, span: crate::hir::Span) -> ValueRelation {
        ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Absent,
            span: span.clone(),
            target,
            index: None,
            outcome: Outcome::Disagree {
                expected: String::new(),
                actual: self.display(t),
            },
            declared_at: None,
            boundary: (span, "read here".to_string()),
        }
    }

    /// The same relations, over one tree: the body, or a policy's term root,
    /// in the order `Body::walk_from` visits it. An arm's names are typed by
    /// where they are bound, so `x` in `Some(x) => x + 1` is the option's `T`
    /// and `r` in `Circle(r) => ..` the case's field (ADR-0059), whatever
    /// else binds an `x` or an `r` (ADR-0063). Until 2026-09-26 every
    /// relation inside an arm read them as unknown, and `Some(x) => x + "a"`
    /// over an `Option<Int>` passed `pw check`.
    fn relations_from(&self, root: ExprId, out: &mut Vec<ValueRelation>) {
        self.relations_at(root, out);
        for c in self.body.children(root) {
            self.relations_from(c, out);
        }
    }

    /// The relations one expression is the site of.
    fn relations_at(&self, id: ExprId, out: &mut Vec<ValueRelation>) {
        match self.body.expr(id) {
            // A string's holes are written as text, as a template's are
            // (ADR-0084): until 2026-09-26 `"{xs}"` over a list checked, and
            // the backend was the first to refuse it. An attribute's are
            // `template_relations`', which names the attribute.
            Expr::Interpolated { parts, .. } if !self.attribute_strings.contains(&id) => {
                for p in parts {
                    out.extend(self.text(
                        self.of(*p),
                        "\"{..}\"".to_string(),
                        self.body.expr_span(*p),
                    ));
                }
            }
            Expr::Call { .. } | Expr::Address { .. } => out.extend(self.call(id).relations),
            // `navigate Page(..)` gives the page its parameters (ADR-0280):
            // each argument related as a call's is.
            Expr::Keyword {
                keyword, modifiers, ..
            } if matches!(
                keyword.as_str(),
                "query" | "subscription" | "navigate" | "schedule" | "unschedule"
            ) && !modifiers.is_empty() =>
            {
                out.extend(self.call(id).relations)
            }
            Expr::Record { name: Some(_), .. } => out.extend(self.construct(id).relations),
            Expr::Field { base, name } => out.extend(self.member(id, *base, name)),
            // An operator's operands, and a condition. Until 2026-09-25 a
            // comparison was typed `Bool` whatever it compared, so
            // `1 == "a"` checked, and the backend was the first to refuse.
            Expr::Binary { op, lhs, rhs } => out.extend(self.operands(id, op, *lhs, *rhs)),
            Expr::Name(n) => out.extend(self.bare_case_relation(id, n)),
            Expr::Unary { op, operand } => {
                let (what, expected) = match op {
                    UnOp::Not => ("the operand of `!`", Some(Ty::Primitive(Primitive::Bool))),
                    UnOp::Neg => ("the operand of `-`", None),
                };
                out.push(match expected {
                    Some(t) => self.operand(id, *operand, what, &t),
                    None => self.number(id, *operand, what),
                });
            }
            Expr::If { cond, .. } => {
                out.push(self.operand(
                    id,
                    *cond,
                    "the condition of `if`",
                    &Ty::Primitive(Primitive::Bool),
                ));
                out.extend(self.branches(id));
            }
            Expr::Match { .. } => out.extend(self.branches(id)),
            // `[1, "a"]`: one list, of one element type (ADR-0069).
            Expr::List { items } if !items.is_empty() => {
                let mut first: Option<Ty> = None;
                let mut outcome = Outcome::Agree;
                let mut at = self.body.expr_span(id);
                for i in items {
                    let t = self.of(*i);
                    if t.is_unknown() || !t.is_closed() {
                        outcome = Outcome::Undecided(Undecided::Unknown);
                        continue;
                    }
                    match &first {
                        None => first = Some(t),
                        Some(f) => {
                            if unify(&mut Subst::default(), f, &t) == Verdict::Disagree {
                                outcome = Outcome::Disagree {
                                    expected: self.display(f),
                                    actual: self.display(&t),
                                };
                                at = self.body.expr_span(*i);
                                break;
                            }
                        }
                    }
                }
                out.push(ValueRelation {
                    declaration: self.decl.name.clone(),
                    kind: RelationKind::Items,
                    span: at,
                    target: "list".to_string(),
                    index: None,
                    outcome,
                    declared_at: None,
                    boundary: (self.body.expr_span(id), "in this list".to_string()),
                });
            }
            // An `Int` is 64 bits (ADR-0039, ADR-0069).
            Expr::Literal(Literal::Int(n)) => out.push(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Literal,
                span: self.body.expr_span(id),
                target: n.clone(),
                index: None,
                outcome: match n.parse::<i64>() {
                    Ok(_) => Outcome::Agree,
                    Err(_) => Outcome::Disagree {
                        expected: "an Int".to_string(),
                        actual: n.clone(),
                    },
                },
                declared_at: None,
                boundary: (self.body.expr_span(id), "this literal".to_string()),
            }),
            // `for x in xs`: the loop runs over a list (ADR-0051, ADR-0068).
            Expr::For { iterable, .. } => {
                let actual = self.of(*iterable);
                let outcome = match &actual {
                    Ty::Builtin(Builtin::List, _) | Ty::Any => Outcome::Agree,
                    Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. } => {
                        Outcome::Undecided(Undecided::Unknown)
                    }
                    other => Outcome::Disagree {
                        expected: "List<_>".to_string(),
                        actual: self.display(other),
                    },
                };
                out.push(self.operand_relation(
                    id,
                    *iterable,
                    "the list a `for` loop runs over",
                    outcome,
                ));
            }
            // `e?`: an `Option` or a `Result`, whose failure it returns
            // (ADR-0051, ADR-0068).
            Expr::Try { value } => {
                let actual = self.of(*value);
                let outcome = match &actual {
                    Ty::Builtin(Builtin::Option | Builtin::Result, _) | Ty::Any => Outcome::Agree,
                    Ty::Unknown | Ty::Var(_) | Ty::Parameter { .. } => {
                        Outcome::Undecided(Undecided::Unknown)
                    }
                    other => Outcome::Disagree {
                        expected: "Option<_> or Result<_, _>".to_string(),
                        actual: self.display(other),
                    },
                };
                out.push(self.operand_relation(id, *value, "the operand of `?`", outcome));
            }
            Expr::Let {
                pat: Some(pat),
                ty: Some(ty),
                init: Some(init),
            } => {
                let Some(written) = crate::resolved::written_in_body(self.body, *ty) else {
                    return;
                };
                let span = self.body.expr_span(*init);
                let declared =
                    self.sigs
                        .resolve_type(self.module, self.decl, &written, span.clone());
                let name = match self.body.pat(*pat) {
                    Pattern::Bind { name, .. } => name.clone(),
                    _ => "_".to_string(),
                };
                let boundary = (
                    self.body.expr_span(id),
                    format!("`{name}` is annotated in this binding"),
                );
                out.push(self.relate(
                    RelationKind::Binding,
                    &declared,
                    self.of(*init),
                    name,
                    span,
                    boundary,
                ));
            }
            _ => {}
        }
    }

    /// **`value.name` names a member the value's type has** (PW0610,
    /// ADR-0048): a record's field, or a declaration whose first parameter
    /// takes the type, read as a property or called as a method. Until
    /// 2026-09-25 a member no type had was unknown, so A-015 read `box.x`
    /// from a snapshot of a `Rect`, which has no `x`, and every analysis
    /// after it read nothing.
    ///
    /// Not a member: a path through a module (`List.map`, a name, ADR-0047's),
    /// and a unit on a numeric literal (`900.px`, `30.seconds`), which is a
    /// dimensioned literal. A value whose type is not known is undecided.
    fn member(&self, id: ExprId, base: ExprId, name: &str) -> Option<ValueRelation> {
        // `Shape.Circle`: not a member of a value, a case of a type, which the
        // type must declare (PW0608, ADR-0059). Until 2026-09-26 it was
        // undecided, and `Shape.Bogus(1)` passed `pw check`.
        if let Some(c) = self.case_path(id) {
            let (def, outcome) = match c {
                CaseNamed::Case(def, _) => (def, Outcome::Agree),
                CaseNamed::Unknown(def, written) => (
                    def,
                    Outcome::Disagree {
                        expected: self
                            .sigs
                            .type_decl(def)
                            .and_then(|t| t.variants.as_ref())
                            .map(|cases| {
                                cases
                                    .iter()
                                    .map(|(n, _)| format!("`{n}`"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default(),
                        actual: written,
                    },
                ),
            };
            return Some(ValueRelation {
                declaration: self.decl.name.clone(),
                kind: RelationKind::Case,
                span: self.body.expr_span(id),
                target: self.display_def(def),
                index: None,
                outcome,
                declared_at: None,
                boundary: (self.body.expr_span(base), "the type named".to_string()),
            });
        }
        if self.resolves_as_path(id)
            || matches!(
                self.body.expr(base),
                Expr::Literal(Literal::Int(_) | Literal::Float(_))
            )
        {
            return None;
        }
        let receiver = self.of(base);
        let outcome = match receiver.receiver() {
            // **A value of a type parameter has no member** (ADR-0351, the
            // generic-effects track): whatever `T` is instantiated with, the
            // body does not say which `name` it reads, nor what that
            // performs. Until then undecided, and `fn f<T>(x: T) -> String
            // !{} { x.label }` read a member whose effect no analysis saw.
            None if matches!(receiver, Ty::Parameter { .. }) => Outcome::Disagree {
                expected: format!("a member `{name}`"),
                actual: self.display(&receiver),
            },
            None => Outcome::Undecided(Undecided::Unknown),
            Some(r) if self.sigs.member_by_in(self.module(), r, name).is_some() => Outcome::Agree,
            // Several modules declare it, and this one sees several or none
            // of them (ADR-0254): refused, and never one chosen.
            Some(r) if !self.sigs.member_choices(self.module(), r, name).is_empty() => {
                Outcome::Disagree {
                    expected: format!(
                        "{AMBIGUOUS_MEMBER}{}",
                        self.sigs.member_choices(self.module(), r, name).join(", ")
                    ),
                    actual: self.display(&receiver),
                }
            }
            Some(_) => match self.representation(&receiver, name) {
                Some((def, _)) if def.unit == self.at => Outcome::Agree,
                Some(_) => Outcome::Disagree {
                    expected: PRIVATE_REPRESENTATION.to_string(),
                    actual: self.display(&receiver),
                },
                // `store.name` on an `Option<Store>`: the contents have the
                // member and the container does not. PW0600's invariant, an
                // optional value used as its contents.
                None if self.contents_have(&receiver, name) => Outcome::Disagree {
                    expected: OPTIONAL_CONTENTS.to_string(),
                    actual: self.display(&receiver),
                },
                None => Outcome::Disagree {
                    expected: format!("a member `{name}`"),
                    actual: self.display(&receiver),
                },
            },
        };
        Some(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Member,
            span: self.body.expr_span(id),
            target: name.to_string(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (self.body.expr_span(base), "the value read".to_string()),
        })
    }

    /// Does the `Some` or `Ok` side of an `Option` or a `Result` have the
    /// member `name`?
    fn contents_have(&self, receiver: &Ty, name: &str) -> bool {
        let Ty::Builtin(Builtin::Option | Builtin::Result, args) = receiver else {
            return false;
        };
        args.first()
            .and_then(Ty::receiver)
            .is_some_and(|r| self.sigs.member_by_in(self.module(), r, name).is_some())
    }

    /// **An opaque type's representation, as `.value`** (ADR-0048): the
    /// declaring type and the representation under the receiver's
    /// arguments, when `receiver` is an opaque type that declares no member
    /// of that name. Who may read it is the caller's question: only the
    /// module that declares the type, for which it is not opaque.
    fn representation(&self, receiver: &Ty, name: &str) -> Option<(DefId, Ty)> {
        let Ty::Nominal(def, _) = receiver else {
            return None;
        };
        if name != "value" {
            return None;
        }
        let TypeResolution::Resolved(rep) = self.sigs.type_decl(*def)?.representation.as_ref()?
        else {
            return None;
        };
        let mut s = Subst::default();
        unify(&mut s, &self.applied(*def).instantiate(*def), receiver);
        Some((*def, s.close(&Ty::of(rep).instantiate(*def))))
    }

    /// **An operator's operands** (PW0609). A logical operator takes `Bool`s;
    /// a comparison, two values of one type; arithmetic, two `Int`s or two
    /// `Float`s. Which operators a type supports beyond that is the backend's
    /// to refuse, not a type disagreement.
    fn operands(&self, id: ExprId, op: &BinOp, lhs: ExprId, rhs: ExprId) -> Vec<ValueRelation> {
        let sym = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Cmp(c) => c.as_str(),
            BinOp::And => "&",
            BinOp::Or => "|",
            BinOp::Assign => return self.assignment(id, lhs, rhs),
            BinOp::Pipe | BinOp::Transition => return Vec::new(),
        };
        let left = format!("the left side of `{sym}`");
        let right = format!("the right side of `{sym}`, like its left,");
        match op {
            BinOp::And | BinOp::Or => {
                let b = Ty::Primitive(Primitive::Bool);
                vec![
                    self.operand(id, lhs, &left, &b),
                    self.operand(id, rhs, &format!("the right side of `{sym}`"), &b),
                ]
            }
            BinOp::Cmp(_) => vec![self.operand(id, rhs, &right, &self.of(lhs))],
            _ => vec![
                self.number(id, lhs, &left),
                self.operand(id, rhs, &right, &self.of(lhs)),
            ],
        }
    }

    /// **`x = e`**: `e` has the type `x` holds (ADR-0051). An assignment to a
    /// field, or to a name whose type is not known, relates nothing here.
    fn assignment(&self, id: ExprId, lhs: ExprId, rhs: ExprId) -> Vec<ValueRelation> {
        // What `bind:value` wrote: a field's text into its signal, whose type
        // PW5304 holds to `String` (ADR-0142).
        if self
            .body
            .bound
            .iter()
            .any(|l| matches!(self.body.expr(*l), Expr::Lambda { body, .. } if *body == id))
        {
            return Vec::new();
        }
        // A `provide` of what is not a module's signal is PW5306's
        // (ADR-0144); one of a signal is checked as any assignment is.
        if self.body.provides.contains(&id)
            && let Expr::Name(x) = self.body.expr(lhs)
            && !matches!(
                self.ws.resolve_in(self.at, Namespace::Term, x),
                Resolution::Local(d) | Resolution::Imported { def: d, .. }
                    if self.sigs.signal_type(d).is_some()
            )
        {
            return Vec::new();
        }
        // A name, or a field read from a value: `b.value = e` holds the
        // field's type, where it related nothing until 2026-09-26 (ADR-0070).
        let (x, declared) = match self.body.expr(lhs) {
            Expr::Name(x) => (x.clone(), self.name(lhs, x)),
            Expr::Field { .. } => (path_of(self.body, lhs), self.of(lhs)),
            _ => return Vec::new(),
        };
        let actual = self.of(rhs);
        let mut s = Subst::default();
        let outcome = match unify(&mut s, &declared, &actual) {
            Verdict::Agree => Outcome::Agree,
            Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
            Verdict::Disagree => Outcome::Disagree {
                expected: self.display(&declared),
                actual: self.display(&actual),
            },
        };
        vec![ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Assignment,
            span: self.body.expr_span(rhs),
            target: x.clone(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (self.body.expr_span(id), format!("`{x}` is assigned here")),
        }]
    }

    /// One operand against the type it takes.
    fn operand(&self, id: ExprId, value: ExprId, what: &str, expected: &Ty) -> ValueRelation {
        let actual = self.of(value);
        let mut s = Subst::default();
        let outcome = match unify(&mut s, expected, &actual) {
            Verdict::Agree => Outcome::Agree,
            Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
            Verdict::Disagree => Outcome::Disagree {
                expected: self.display(expected),
                actual: self.display(&actual),
            },
        };
        self.operand_relation(id, value, what, outcome)
    }

    /// An operand arithmetic takes: an `Int` or a `Float`.
    fn number(&self, id: ExprId, value: ExprId, what: &str) -> ValueRelation {
        let actual = self.of(value);
        let outcome = match &actual {
            Ty::Primitive(Primitive::Int | Primitive::Float) | Ty::Any => Outcome::Agree,
            Ty::Unknown | Ty::Parameter { .. } | Ty::Var(_) => {
                Outcome::Undecided(Undecided::Unknown)
            }
            // A list, a record or a string is never a number, whatever its
            // arguments are.
            other => Outcome::Disagree {
                expected: "Int or Float".to_string(),
                actual: self.display(other),
            },
        };
        self.operand_relation(id, value, what, outcome)
    }

    fn operand_relation(
        &self,
        id: ExprId,
        value: ExprId,
        what: &str,
        outcome: Outcome,
    ) -> ValueRelation {
        ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Operand,
            span: self.body.expr_span(value),
            target: what.to_string(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (self.body.expr_span(id), "the operator".to_string()),
        }
    }

    /// Relate one value to one declared type, outside any call.
    #[allow(clippy::too_many_arguments)]
    fn relate(
        &self,
        kind: RelationKind,
        declared: &TypeResolution,
        actual: Ty,
        target: String,
        span: Span,
        boundary: (Span, String),
    ) -> ValueRelation {
        let outcome = match declared {
            TypeResolution::Resolved(t) => {
                let e = Ty::of(t);
                let a = actual;
                let mut s = Subst::default();
                match unify(&mut s, &e, &a) {
                    Verdict::Agree => Outcome::Agree,
                    Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
                    Verdict::Disagree => Outcome::Disagree {
                        expected: self.display(&e),
                        actual: self.display(&a),
                    },
                }
            }
            _ => Outcome::Undecided(Undecided::ExpectedUnresolved),
        };
        ValueRelation {
            declaration: self.decl.name.clone(),
            kind,
            span,
            target,
            index: None,
            outcome,
            declared_at: declared.resolved().map(|t| (self.at, t.span())),
            boundary,
        }
    }

    /// **Two types related where the expected one is inferred**, not
    /// declared: a transition's value against its target's (ADR-0241).
    fn relate_types(
        &self,
        kind: RelationKind,
        expected: Ty,
        actual: Ty,
        target: String,
        span: Span,
        boundary: (Span, String),
    ) -> ValueRelation {
        let mut s = Subst::default();
        let outcome = match unify(&mut s, &expected, &actual) {
            Verdict::Agree => Outcome::Agree,
            Verdict::Undecided => Outcome::Undecided(Undecided::Unknown),
            Verdict::Disagree => Outcome::Disagree {
                expected: self.display(&expected),
                actual: self.display(&actual),
            },
        };
        ValueRelation {
            declaration: self.decl.name.clone(),
            kind,
            span,
            target,
            index: None,
            outcome,
            declared_at: None,
            boundary,
        }
    }

    /// **Every expression whose value the body returns.** The tail, seen
    /// through blocks and the branches of an `if`/`match`, and each `return`.
    ///
    /// `return e` lowers as two statements, `Name("return")` then `e`, so the
    /// statement after one is a returned value. A `return` inside a lambda
    /// returns from the lambda and is not collected.
    /// **An `if`'s or a `match`'s branches, against each other** (PW0613,
    /// ADR-0068), where its value is used: bound, passed, operated on or
    /// read. Where it is a statement its value is discarded, and where it is
    /// the body's result the result relation reads each branch.
    fn branches(&self, id: ExprId) -> Option<ValueRelation> {
        if !self.used.contains(&id) {
            return None;
        }
        let branches: Vec<ExprId> = match self.body.expr(id) {
            Expr::If {
                then, els: Some(e), ..
            } => vec![*then, *e],
            Expr::Match { arms, .. } => arms.iter().map(|a| a.body).collect(),
            _ => return None,
        };
        let mut first: Option<Ty> = None;
        let mut outcome = Outcome::Agree;
        let mut at = self.body.expr_span(id);
        for b in branches {
            let t = self.of(b);
            if t.is_unknown() || !t.is_closed() {
                outcome = Outcome::Undecided(Undecided::Unknown);
                continue;
            }
            match &first {
                None => first = Some(t),
                Some(f) => {
                    if unify(&mut Subst::default(), f, &t) == Verdict::Disagree {
                        outcome = Outcome::Disagree {
                            expected: self.display(f),
                            actual: self.display(&t),
                        };
                        at = self.body.expr_span(b);
                        break;
                    }
                }
            }
        }
        let what = match self.body.expr(id) {
            Expr::If { .. } => "if",
            _ => "match",
        };
        Some(ValueRelation {
            declaration: self.decl.name.clone(),
            kind: RelationKind::Branches,
            span: at,
            target: what.to_string(),
            index: None,
            outcome,
            declared_at: None,
            boundary: (
                self.body.expr_span(id),
                format!("this `{what}`'s value is used"),
            ),
        })
    }

    fn result_sites(&self) -> Vec<ExprId> {
        let mut out = Vec::new();
        self.tail_sites(self.body.root, &mut out);
        let mut lambdas: BTreeSet<ExprId> = BTreeSet::new();
        for id in self.body.walk() {
            if let Expr::Lambda { body, .. } = self.body.expr(id) {
                lambdas.extend(self.body.walk_from(*body));
            }
        }
        for id in self.body.walk() {
            if lambdas.contains(&id) {
                continue;
            }
            let Expr::Block { stmts } = self.body.expr(id) else {
                continue;
            };
            for pair in stmts.windows(2) {
                if matches!(self.body.expr(pair[0]), Expr::Name(n) if n == "return") {
                    self.tail_sites(pair[1], &mut out);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Every `?` that returns from this declaration — not from a lambda.
    fn propagations(&self) -> Vec<ExprId> {
        let mut lambdas: BTreeSet<ExprId> = BTreeSet::new();
        for id in self.body.walk() {
            if let Expr::Lambda { body, .. } = self.body.expr(id) {
                lambdas.extend(self.body.walk_from(*body));
            }
        }
        self.body
            .walk()
            .into_iter()
            .filter(|id| !lambdas.contains(id) && matches!(self.body.expr(*id), Expr::Try { .. }))
            .collect()
    }

    fn tail_sites(&self, id: ExprId, out: &mut Vec<ExprId>) {
        match self.body.expr(id) {
            Expr::Block { stmts } => {
                if let Some(last) = stmts.last() {
                    self.tail_sites(*last, out);
                }
            }
            Expr::If {
                then, els: Some(e), ..
            } => {
                self.tail_sites(*then, out);
                self.tail_sites(*e, out);
            }
            Expr::Match { arms, .. } => {
                for a in arms {
                    self.tail_sites(a.body, out);
                }
            }
            // `return` as the last statement returns nothing — not a value.
            Expr::Name(n) if n == "return" => {}
            _ => out.push(id),
        }
    }
}

pub fn path_of(body: &Body, id: ExprId) -> String {
    crate::infer::path_of(body, id)
}

/// A declared callable's type as a value: `fn(params..) -> result`. A
/// parameter or result the author left unannotated is a hole, and so is every
/// part that mentions the callable's own type parameters — a generic function
/// named as a value is not instantiated by anything here.
fn function_type(sig: &Signature) -> Ty {
    let mut shape: Vec<Ty> = sig
        .params
        .iter()
        .map(|p| match p {
            Some(TypeResolution::Resolved(t)) => Ty::of(t),
            _ => Ty::Unknown,
        })
        .collect();
    shape.push(match &sig.returns {
        Some(TypeResolution::Resolved(t)) => Ty::of(t),
        _ => Ty::Unknown,
    });
    let s = Subst::default();
    s.close(&Ty::Builtin(Builtin::Function, shape).instantiate(sig.definition))
}

/// A lambda's parameter names, in order, when every parameter is a plain
/// binding. `fn(a, b) e` lowers its list as one call-shaped pattern around the
/// bindings; `x => e` as the binding itself.
/// A lambda's parameter names: `x => ..`, `fn(a, b) ..` and `(a, b) => ..`,
/// the last two lowered as one unnamed constructor pattern. The backend reads
/// parameters through this too (ADR-0040), so the two cannot disagree.
pub(crate) fn lambda_names(body: &Body, params: &[crate::hir::PatternId]) -> Option<Vec<String>> {
    lambda_binders(body, params)?
        .into_iter()
        .map(|p| match body.pat(p) {
            Pattern::Bind { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// **Where a lambda binds each of its parameters**: `(t, w) => ..` is one
/// parenthesised pattern holding two names. `None` where a parameter is not
/// a name.
/// **A lambda's parameters, one pattern each** (ADR-0138): `(a, b) => ..`
/// lowers to one pattern with no path whose arguments are the parameters,
/// and `x => ..` to the one parameter.
pub(crate) fn lambda_params(
    body: &Body,
    params: &[crate::hir::PatternId],
) -> Vec<crate::hir::PatternId> {
    if let [only] = params
        && let Pattern::Ctor { path, args } = body.pat(*only)
        && path.is_empty()
    {
        return args.clone();
    }
    params.to_vec()
}

pub(crate) fn lambda_binders(
    body: &Body,
    params: &[crate::hir::PatternId],
) -> Option<Vec<crate::hir::PatternId>> {
    let named = |p: &crate::hir::PatternId| matches!(body.pat(*p), Pattern::Bind { .. });
    if let [only] = params
        && let Pattern::Ctor { path, args } = body.pat(*only)
        && path.is_empty()
    {
        return args.iter().all(named).then(|| args.clone());
    }
    params.iter().all(named).then(|| params.to_vec())
}

// --- running it -----------------------------------------------------------------------

/// **Every value relation in one unit.** The single walk both the checker and
/// [`analysis`] use, so the two cannot disagree about which relations exist.
pub fn relations(hir: &Hir, sigs: &Signatures, ws: &Workspace, at: UnitId) -> Vec<ValueRelation> {
    let mut out = Vec::new();
    // Each declaration's bindings as typed, for the declarations nested in
    // it, which come after it (ADR-0066).
    let mut typed: BTreeMap<DeclId, BTreeMap<Binder, Ty>> = BTreeMap::new();
    for (id, decl) in hir.all_decls() {
        annotations(hir, sigs, ws, at, id, decl, &mut out);
        // An interface's keys are typed too, in their own arena (ADR-0240).
        let Some(body_id) = decl.terms_body() else {
            continue;
        };
        let body = hir.body(body_id);
        let lexical = Lexical::build_in(hir, id).unwrap_or_else(|| Lexical::build(decl, body));
        let outer: Vec<Ty> = lexical
            .outer_bindings()
            .map(|(d, b)| {
                typed
                    .get(&d)
                    .and_then(|l| l.get(&b))
                    .cloned()
                    .unwrap_or(Ty::Unknown)
            })
            .collect();
        let typer = Typer::new(sigs, ws, at, hir.module_of(id), decl, body, lexical, outer);
        typer.relations(&mut out);
        typer.term_relations(&mut out);
        returns(&typer, sigs, at, id, decl, &mut out);
        typed.insert(id, typer.locals.borrow().clone());
    }
    out
}

/// **What a template reads through a member function** (ADR-0169): each
/// `.name` that names a function declared on its value's type, and each
/// `{#each}` whose list is read through one. A template's value is a path,
/// so each such name in one is read as a property and calls the function.
/// The renderer reads fields alone, so a host computes each of these for a
/// page, or the page's plan refuses it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MemberReads {
    /// The field expression that names it: `item.price.display`'s
    /// `.display`.
    pub exprs: BTreeSet<ExprId>,
    /// The `{#each}` block, by its node: its list expression reads a
    /// member function somewhere in its evaluation (ADR-0345).
    pub lists: BTreeSet<NodeId>,
}

/// [`MemberReads`] in `id`'s body, typed as [`relations`] types it: each
/// declaration around it first, for the names it binds (ADR-0066).
pub fn member_reads(
    hir: &Hir,
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    id: DeclId,
) -> MemberReads {
    let lexical_of = |d: DeclId, decl: &Decl, body: &Body| {
        Lexical::build_in(hir, d).unwrap_or_else(|| Lexical::build(decl, body))
    };
    // `id`, and each declaration whose bindings it reads.
    let mut needed = BTreeSet::from([id]);
    let mut around = vec![id];
    while let Some(d) = around.pop() {
        let decl = hir.decl(d);
        let Some(body) = decl.body.map(|b| hir.body(b)) else {
            continue;
        };
        for (o, _) in lexical_of(d, decl, body).outer_bindings() {
            if needed.insert(o) {
                around.push(o);
            }
        }
    }
    let mut typed: BTreeMap<DeclId, BTreeMap<Binder, Ty>> = BTreeMap::new();
    for (d, decl) in hir.all_decls() {
        if !needed.contains(&d) {
            continue;
        }
        let Some(body_id) = decl.body else { continue };
        let body = hir.body(body_id);
        let lexical = lexical_of(d, decl, body);
        let outer: Vec<Ty> = lexical
            .outer_bindings()
            .map(|(o, b)| {
                typed
                    .get(&o)
                    .and_then(|l| l.get(&b))
                    .cloned()
                    .unwrap_or(Ty::Unknown)
            })
            .collect();
        let typer = Typer::new(sigs, ws, at, hir.module_of(d), decl, body, lexical, outer);
        if d == id {
            return typer.member_reads();
        }
        typed.insert(d, typer.locals.borrow().clone());
    }
    MemberReads::default()
}

/// The declared result against every value the body returns.
fn returns(
    typer: &Typer<'_>,
    sigs: &Signatures,
    at: UnitId,
    id: DeclId,
    decl: &Decl,
    out: &mut Vec<ValueRelation>,
) {
    if !returns_its_body(decl) {
        return;
    }
    let def = DefId {
        unit: at,
        decl: id.0,
    };
    let Some(sig) = sigs.by_def(def) else { return };
    let Some(declared) = &sig.returns else { return };
    // A body that returns nothing declares `()`: its last statement is a
    // statement, not a value — the language has no statement terminator with
    // which to discard one. Recorded in docs/ASSUMPTIONS.md.
    if declared
        .resolved()
        .and_then(ResolvedType::as_primitive)
        .is_some_and(|p| p == Primitive::Unit)
    {
        return;
    }
    let boundary = || {
        (
            decl.name_span.clone(),
            format!("`{}` declares its result here", decl.name),
        )
    };
    for site in typer.result_sites() {
        out.push(typer.relate(
            RelationKind::Return,
            declared,
            typer.of(site),
            sig.path.clone(),
            typer.body.expr_span(site),
            boundary(),
        ));
    }
    // Each `e?` returns `e`'s failure early: an `Err` of the same error type,
    // or a `None`. Its success type is not returned, so it is any type
    // (ADR-0065).
    for site in typer.propagations() {
        let Expr::Try { value } = typer.body.expr(site) else {
            continue;
        };
        let returned = match typer.of(*value) {
            Ty::Builtin(Builtin::Result, args) if args.len() == 2 => {
                Ty::Builtin(Builtin::Result, vec![Ty::Any, args[1].clone()])
            }
            Ty::Builtin(Builtin::Option, _) => Ty::Builtin(Builtin::Option, vec![Ty::Any]),
            _ => Ty::Unknown,
        };
        out.push(typer.relate(
            RelationKind::Return,
            declared,
            returned,
            sig.path.clone(),
            typer.body.expr_span(site),
            boundary(),
        ));
    }
}

/// The declarations whose body's value IS their result.
///
/// A `view`, `component` or `page` renders markup; a `subscription` or
/// `resource` body declares a lifecycle. None is checked against a result
/// here, because none says its body is one. A `materialize` that declares
/// its type derives it in its body (ADR-0273): one that declares none is a
/// fragment the host renders, and its body is its clauses.
fn returns_its_body(decl: &Decl) -> bool {
    matches!(
        decl.kind,
        DeclKind::Fn | DeclKind::Query | DeclKind::Command | DeclKind::Task
    ) || (decl.kind == DeclKind::Materialize && decl.ret.is_some())
        // A predicate the program gives a body answers its `Bool` (ADR-0384
        // H2): whether it holds.
        || (decl.kind == DeclKind::Predicate && decl.body.is_some())
}

/// **Every written type resolves, or the program is told.** E9-V5.
///
/// `fn f(x: Stroe) -> Lisst<Int>` passed `pw check` with nothing to say until
/// 2026-09-24: the signature held `Unresolved` for both, every relation
/// declined to run on them — correctly — and nothing reported why. A relation
/// that refuses an unresolved type is only half of V5; the other half is that
/// the refusal reaches the author.
fn annotations(
    hir: &Hir,
    sigs: &Signatures,
    ws: &Workspace,
    at: UnitId,
    id: DeclId,
    decl: &Decl,
    out: &mut Vec<ValueRelation>,
) {
    let module = hir.module_of(id);
    let mut check = |written: &crate::hir::DeclaredType, span: Span, what: String| {
        let r = sigs.resolve_type(module, decl, written, span.clone());
        // **A written map's key** (ADR-0248): refused here, where it is
        // written, and not first where the backend builds it.
        if let TypeResolution::Resolved(t) = &r {
            let mut keys = Vec::new();
            unordered_keys(sigs, &Ty::of(t), &mut keys);
            for key in keys {
                out.push(ValueRelation {
                    declaration: decl.name.clone(),
                    kind: RelationKind::MapKey,
                    span: span.clone(),
                    target: what.clone(),
                    index: None,
                    outcome: Outcome::Disagree {
                        expected: MAP_KEYS.to_string(),
                        actual: shown(sigs, &key),
                    },
                    declared_at: None,
                    boundary: (
                        decl.name_span.clone(),
                        format!("in the declaration of `{}`", decl.name),
                    ),
                });
            }
        }
        out.push(ValueRelation {
            declaration: decl.name.clone(),
            kind: RelationKind::Annotation,
            span,
            target: what,
            index: None,
            outcome: match &r {
                TypeResolution::Resolved(_) => Outcome::Agree,
                // An import that failed already says why: `PW0020` for a
                // module that is not there, `PW0021` for a name it does not
                // declare. A second diagnostic about the same cause would make
                // the count say two things were wrong.
                TypeResolution::Unresolved { name, .. }
                    if owned_by_a_failed_import(ws, at, name) =>
                {
                    Outcome::Undecided(Undecided::ExpectedUnresolved)
                }
                TypeResolution::Unresolved { name, written } => Outcome::Disagree {
                    expected: why_unresolved(ws, at, name, written),
                    actual: written.clone(),
                },
                TypeResolution::Blocked { .. } => Outcome::Undecided(Undecided::ExpectedUnresolved),
            },
            declared_at: None,
            boundary: (
                decl.name_span.clone(),
                format!("in the declaration of `{}`", decl.name),
            ),
        });
    };
    for p in &decl.params {
        // An owned table's generated handle (track `row-ownership`) names
        // the platform's `User` over the owner's type, which resolves where
        // the owner is a `capability.UserId`; where it is not, PW0367 says
        // so once, and this would say it again.
        let handle = hir.generated.contains_key(&id)
            && p.ty
                .as_ref()
                .is_some_and(|t| t.constructor_head_only() == "capability.User");
        if let Some(t) = p.ty.as_ref().filter(|_| !handle) {
            check(t, p.span.clone(), format!("parameter `{}`", p.name));
        }
    }
    if let Some(t) = &decl.ret {
        check(t, decl.name_span.clone(), "the declared result".to_string());
    }
    for f in decl.fields.iter().flatten() {
        if let Some(t) = &f.ty {
            check(t, f.span.clone(), format!("field `{}`", f.name));
        }
    }
    let Some(body_id) = decl.body else { return };
    let body = hir.body(body_id);
    for e in body.walk() {
        let (ty, what) = match body.expr(e) {
            Expr::Let { ty: Some(ty), .. } => (*ty, "this binding's annotation"),
            Expr::Cast { ty, .. } => (*ty, "this cast's target"),
            _ => continue,
        };
        if let Some(written) = crate::resolved::written_in_body(body, ty) {
            check(&written, body.expr_span(e), what.to_string());
        }
    }
    // A lambda's written parameter type (ADR-0138): kept since 2026-10-02,
    // and as checked as a binding's.
    for (pat, ty) in &body.param_types {
        if let Some(written) = crate::resolved::written_in_body(body, *ty) {
            let what = match body.pat(*pat) {
                Pattern::Bind { name, .. } => format!("parameter `{name}`"),
                _ => "a lambda's parameter".to_string(),
            };
            check(&written, body.pat_span(*pat), what);
        }
    }
}

/// Is `name` something this unit imports from a module that does not exist,
/// or by name from a module that does not declare it? Either import is itself
/// a diagnostic, and it is the cause.
fn owned_by_a_failed_import(ws: &Workspace, at: UnitId, name: &str) -> bool {
    let Some(m) = ws.module_of(at) else {
        return false;
    };
    let (module, member) = match name.rsplit_once('.') {
        Some((m, n)) => (Some(m), n),
        None => (None, name),
    };
    m.imports.iter().any(|imp| {
        let names_it = match module {
            Some(q) => imp.module == q,
            None => imp.names.iter().any(|n| n == member),
        };
        names_it
            && match ws.modules.iter().find(|t| t.name == imp.module) {
                None => true,
                Some(t) => !t
                    .defines
                    .contains_key(&(Namespace::Type, member.to_string())),
            }
    })
}

/// What a diagnostic says about an unresolved written type: that a name is not
/// a type here, or that a type is applied to the wrong number of arguments.
fn why_unresolved(ws: &Workspace, at: UnitId, name: &str, written: &str) -> String {
    let found = match name.contains('.') {
        true => ws.resolve_path_in(at, Namespace::Type, name),
        false => ws.resolve_in(at, Namespace::Type, name),
    };
    match found {
        Resolution::Local(d) | Resolution::Imported { def: d, .. } => {
            let n = ws.type_arity(d).unwrap_or(0);
            format!(
                "`{name}` takes {n} type argument{}, and `{written}` applies it to a \
                 different number",
                if n == 1 { "" } else { "s" }
            )
        }
        Resolution::Ambiguous(_) => format!("`{name}` names more than one type here"),
        Resolution::Unresolved => match crate::resolved::Builtin::of(name) {
            Some(_) => format!("`{written}` does not apply `{name}` to its parameters"),
            None => format!("`{name}` is not a type visible here"),
        },
    }
}

/// **Diagnostics, as a projection of the relations.**
pub fn diagnostics(relations: &[ValueRelation], at: UnitId) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for r in relations {
        let Outcome::Disagree { expected, actual } = &r.outcome else {
            continue;
        };
        let d: Diagnostic = match r.kind {
            RelationKind::Arity => {
                let (m, n) = (expected, actual);
                let word = |k: &str| if k == "1" { "argument" } else { "arguments" };
                Diagnostic::error(
                    crate::codes::CALL_ARITY.id,
                    crate::codes::CALL_ARITY.invariant,
                    Detector::Signature,
                    format!(
                        "`{}` declares {m} {} and this call passes {n}",
                        r.target,
                        word(m)
                    ),
                    r.span.clone(),
                )
                .reason("call_arity_disagrees_with_declaration")
                .explain(
                    "the callee's declaration fixes how many arguments a call supplies; a call \
                     that passes a different number is not the call the declaration describes",
                )
                .repair(match n.parse::<usize>().ok() < m.parse::<usize>().ok() {
                    true => "pass the missing arguments",
                    false => "remove the extra arguments",
                })
            }
            RelationKind::Argument | RelationKind::Field => {
                let position = match (r.kind, r.index) {
                    (RelationKind::Field, _) => format!("the field `{}`", r.target),
                    (_, Some(i)) => format!("argument {} of `{}`", i + 1, r.target),
                    _ => format!("an argument of `{}`", r.target),
                };
                let mut d = Diagnostic::error(
                    crate::codes::ARGUMENT_TYPE.id,
                    crate::codes::ARGUMENT_TYPE.invariant,
                    Detector::Signature,
                    format!("{position} is declared `{expected}` and this is `{actual}`"),
                    r.span.clone(),
                )
                .reason("argument_type_disagrees_with_declaration")
                .explain(
                    "a value passed to a declared parameter or field must have the declared \
                     type; two types with one representation are still two types",
                )
                .repair(format!("pass a `{expected}`, or change the declaration"));
                if let Some((unit, span)) = &r.declared_at
                    && *unit == at
                {
                    d = d.related(span.clone(), format!("declared `{expected}` here"));
                }
                d
            }
            RelationKind::Return => Diagnostic::error(
                crate::codes::RETURN_TYPE.id,
                crate::codes::RETURN_TYPE.invariant,
                Detector::Signature,
                format!(
                    "`{}` declares its result `{expected}` and this produces `{actual}`",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("result_disagrees_with_declaration")
            .explain(
                "every value a body returns — its last expression, each branch of it, and \
                 each `return` — must have the result type its signature declares",
            )
            .repair(format!(
                "produce a `{expected}` here, or change the declared result"
            )),
            RelationKind::MapKey => Diagnostic::error(
                crate::codes::MAP_KEY.id,
                crate::codes::MAP_KEY.invariant,
                Detector::Signature,
                format!(
                    "{} keys a map or a set by `{actual}`, and a key is {expected}",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("map_key_has_no_order")
            .explain(
                "a map keeps its entries, and a set its elements, in the order of their keys, \
                 the same in the component and the browser: an `Int` by value, a `String` by \
                 code point, `false` before `true`, and an opaque type over one as its \
                 representation. Any other type has no order they share",
            )
            .repair(format!("key it by {expected}")),
            RelationKind::Transition => Diagnostic::error(
                crate::codes::OPTIMISTIC_TARGET_MISMATCH.id,
                crate::codes::OPTIMISTIC_TARGET_MISMATCH.invariant,
                Detector::Signature,
                format!(
                    "`{}`'s optimistic transition produces `{actual}`, but it targets a \
                     resource whose value is `{expected}`",
                    r.declaration
                ),
                r.span.clone(),
            )
            .reason("optimistic_transition_type_mismatch")
            .explain(format!(
                "An optimistic transition replaces the value the client is displaying, and \
                 the platform abandons it by restoring the value it held. Both are \
                 `{expected}`. A transition producing `{actual}` would put something else in \
                 that entry, and there would be nothing meaningful to restore."
            ))
            .repair(format!(
                "produce {} `{expected}` from {}",
                article(expected),
                r.target
            )),
            RelationKind::Binding => Diagnostic::error(
                crate::codes::BINDING_TYPE.id,
                crate::codes::BINDING_TYPE.invariant,
                Detector::Signature,
                format!(
                    "`{}` is declared `{expected}` and initialised with `{actual}`",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("binding_type_disagrees_with_annotation")
            .explain("an annotated binding must be initialised with a value of its declared type")
            .repair(format!(
                "initialise it with a `{expected}`, or change the annotation"
            )),
            RelationKind::Operand => Diagnostic::error(
                crate::codes::OPERAND_TYPE.id,
                crate::codes::OPERAND_TYPE.invariant,
                Detector::Signature,
                format!("{} must be `{expected}`, and this is `{actual}`", r.target),
                r.span.clone(),
            )
            .reason("operand_type_disagrees_with_operator")
            .explain(
                "a comparison's sides share a type, arithmetic takes two `Int`s or two \
                 `Float`s, and `&`, `|`, `!` and an `if` take `Bool`s; two types with one \
                 representation are still two types",
            )
            .repair(format!("make it a `{expected}`")),
            RelationKind::Assignment => Diagnostic::error(
                crate::codes::BINDING_TYPE.id,
                crate::codes::BINDING_TYPE.invariant,
                Detector::Signature,
                format!(
                    "`{}` holds `{expected}` and is assigned `{actual}`",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("assignment_disagrees_with_binding")
            .explain("a mutable binding holds values of the type it was first bound with")
            .repair(format!("assign a `{expected}`, or bind a new name")),
            RelationKind::Member if expected == OPTIONAL_CONTENTS => Diagnostic::error(
                crate::codes::OPTION_USED_AS_VALUE.id,
                crate::codes::OPTION_USED_AS_VALUE.invariant,
                Detector::Signature,
                format!(
                    "`{actual}` has no member `{}`: what it may hold does, and must be taken \
                     out first",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("member_of_optional_contents")
            .explain(
                "an `Option` or a `Result` is a value that may be absent or failed; its \
                 contents' members are reached by taking it apart, which says what happens \
                 when there are none",
            )
            .repair("`match` it, and read the member in the `Some` or `Ok` arm"),
            RelationKind::Member if expected.starts_with(AMBIGUOUS_MEMBER) => {
                let choices: Vec<&str> = expected[AMBIGUOUS_MEMBER.len()..].split(", ").collect();
                let named = match choices.as_slice() {
                    [rest @ .., last] if !rest.is_empty() => format!(
                        "{} or `{last}`",
                        rest.iter()
                            .map(|c| format!("`{c}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => format!("`{}`", choices.join("")),
                };
                Diagnostic::error(
                    crate::codes::AMBIGUOUS_MEMBER.id,
                    crate::codes::AMBIGUOUS_MEMBER.invariant,
                    Detector::Signature,
                    format!(
                        "`{}` of a `{actual}` could be {named}, and this module does not say \
                         which",
                        r.target
                    ),
                    r.span.clone(),
                )
                .reason("member_declared_by_several_modules")
                .explain(
                    "a value's member is its type's field, or a declaration whose first \
                     parameter takes the type; where several modules declare one of a name, \
                     it is the one this module declares or imports, and here none is, or \
                     more than one",
                )
                .repair(format!(
                    "call it by its path, `{}(..)`, or import only the module whose `{}` \
                     is meant",
                    choices[0], r.target
                ))
            }
            RelationKind::Member if expected == PRIVATE_REPRESENTATION => Diagnostic::error(
                crate::codes::UNKNOWN_MEMBER.id,
                crate::codes::UNKNOWN_MEMBER.invariant,
                Detector::Signature,
                format!(
                    "`{actual}` is opaque here: its `.value` is read only in the module that \
                     declares it"
                ),
                r.span.clone(),
            )
            .reason("opaque_representation_outside_its_module")
            .explain(
                "an opaque type's representation is its own module's; everywhere else the \
                 type has only the members that module declares for it",
            )
            .repair(format!(
                "declare an accessor for `{actual}` in its module, and read that"
            )),
            RelationKind::Member => Diagnostic::error(
                crate::codes::UNKNOWN_MEMBER.id,
                crate::codes::UNKNOWN_MEMBER.invariant,
                Detector::Signature,
                format!("`{actual}` has no member `{}`", r.target),
                r.span.clone(),
            )
            .reason("member_not_declared_by_type")
            .explain(
                "a value's members are its type's fields and the declarations whose first \
                 parameter takes that type; a name the type does not have reads nothing",
            )
            .repair(format!(
                "read a member `{actual}` declares, or declare `{}` for it",
                r.target
            )),
            RelationKind::Case => Diagnostic::error(
                crate::codes::PATTERN_CONSTRUCTOR.id,
                crate::codes::PATTERN_CONSTRUCTOR.invariant,
                Detector::Signature,
                format!("`{actual}` is not a constructor of `{}`", r.target),
                r.span.clone(),
            )
            .reason("path_names_a_case_its_type_lacks")
            .explain(
                "a sum type's values are built by the cases it declares; a case it does not \
                 declare builds nothing, and a value of the type is not one of them",
            )
            .repair(format!("`{}`'s constructors are {expected}", r.target)),
            RelationKind::BareCase => {
                let types: Vec<&str> = expected.split(" and ").collect();
                let qualified: Vec<String> =
                    types.iter().map(|t| format!("`{t}.{actual}`")).collect();
                let named: Vec<String> = types.iter().map(|t| format!("`{t}`")).collect();
                Diagnostic::error(
                    crate::codes::AMBIGUOUS_NAME.id,
                    crate::codes::AMBIGUOUS_NAME.invariant,
                    Detector::Signature,
                    format!("`{actual}` is a case of {}", named.join(" and ")),
                    r.span.clone(),
                )
                .reason("ambiguous_case")
                .explain(format!(
                    "a case written alone is the case of that name of the one sum type in \
                     scope that declares it, or of the type expected where it is written. \
                     Here {} types declare `{actual}`, and nothing expected says which is \
                     meant",
                    types.len()
                ))
                .repair(format!(
                    "write it through its type: {}, or where one of them is expected",
                    qualified.join(" or ")
                ))
            }
            RelationKind::Branches => Diagnostic::error(
                crate::codes::BRANCH_TYPES.id,
                crate::codes::BRANCH_TYPES.invariant,
                Detector::Signature,
                format!(
                    "this branch of the `{}` produces `{actual}`, where the first produces \
                     `{expected}`",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("branches_produce_two_types")
            .explain(
                "an `if` or a `match` whose value is used is one value, whichever branch \
                 runs, so every branch produces its type",
            )
            .repair(format!("make this branch a `{expected}`")),
            // A condition is a `Bool`, or a `List` or `String` tested
            // non-empty (ruling 0071-a).
            RelationKind::Truth => match expected.as_str() {
                "a number" => Diagnostic::error(
                    crate::codes::OPERAND_TYPE.id,
                    crate::codes::OPERAND_TYPE.invariant,
                    Detector::Signature,
                    format!(
                        "`{}` tests {} `{actual}`, a number, which has no truth",
                        r.target,
                        article(actual)
                    ),
                    r.span.clone(),
                )
                .reason("template_condition_is_a_number")
                .explain(
                    "a template's condition is a `Bool`, or a `List` or a `String` tested \
                     non-empty; a number tested as one does not say what it tests: here it \
                     was true for a negative number, and JSX's `{count && …}` shows the `0`",
                )
                .repair("say what it tests: `n > 0`"),
                "no truth" => Diagnostic::error(
                    crate::codes::OPERAND_TYPE.id,
                    crate::codes::OPERAND_TYPE.invariant,
                    Detector::Signature,
                    format!(
                        "`{}` tests {} `{actual}`, which has no truth",
                        r.target,
                        article(actual)
                    ),
                    r.span.clone(),
                )
                .reason("template_condition_has_no_truth")
                .explain(
                    "a template's condition is a `Bool`, or a `List` or a `String` tested \
                     non-empty; a record is always true, and any other value is no answer \
                     to a question",
                )
                .repair("test a `Bool` it holds, or compare it"),
                _ => Diagnostic::error(
                    crate::codes::OPERAND_TYPE.id,
                    crate::codes::OPERAND_TYPE.invariant,
                    Detector::Signature,
                    format!(
                        "`{}` tests {} `{actual}`, which is taken apart with `{{#match}}`",
                        r.target,
                        article(actual)
                    ),
                    r.span.clone(),
                )
                .reason("template_condition_is_a_case")
                .explain(
                    "a template's condition is a `Bool`, or a `List` or a `String` tested \
                     non-empty; a `Result` or a sum type has no truth, and the renderer refuses \
                     it",
                )
                .repair("take it apart with `{#match}`"),
            },
            RelationKind::Invariant => {
                let mut d = Diagnostic::error(
                    crate::codes::INVARIANT_NOT_SHOWN.id,
                    crate::codes::INVARIANT_NOT_SHOWN.invariant,
                    Detector::Signature,
                    format!(
                        "a `{}` holds `{expected}`, and the build cannot show this value \
                         does: it is {actual}",
                        r.target
                    ),
                    r.span.clone(),
                )
                .reason("invariant_not_shown")
                .explain(
                    "a value of an opaque type that states an invariant is built only where \
                     the build shows the invariant holds: from a literal, from values whose \
                     bounds give it, or inside an `if` that tests it. A value the build \
                     cannot bound is tested first, and what builds from it answers an \
                     `Option` (ADR-0179)",
                )
                .repair(format!(
                    "test the value first, `if .. {{ Some({}(..)) }} else {{ None }}`, or \
                     build from a value its bounds give",
                    r.target
                ));
                if let Some((unit, span)) = &r.declared_at
                    && *unit == at
                {
                    d = d.related(
                        span.clone(),
                        format!("`{}` states `{expected}` here", r.target),
                    );
                }
                d
            }
            RelationKind::Props => Diagnostic::error(
                crate::codes::VIEW_PROPS.id,
                crate::codes::VIEW_PROPS.invariant,
                Detector::Signature,
                match expected.as_str() {
                    "unknown" => format!("`<{}>` takes no prop `{actual}`", r.target),
                    "a value" => format!(
                        "`<{}>`'s prop `{actual}` is given as text; a prop is a value, \
                         `{actual}={{..}}`",
                        r.target
                    ),
                    _ => format!("`<{}>` is used without its prop `{actual}`", r.target),
                },
                r.span.clone(),
            )
            .reason("view_given_other_props")
            .explain(
                "a view used in another is given each of its parameters as a prop, by name, \
                 and nothing else (ADR-0136)",
            )
            .repair(match expected.as_str() {
                "unknown" => format!("remove `{actual}`, or declare it on `{}`", r.target),
                "a value" => format!("write `{actual}={{..}}`"),
                _ => format!("give `{actual}={{..}}`"),
            }),
            RelationKind::Named => Diagnostic::error(
                crate::codes::NAMED_ARGUMENT.id,
                crate::codes::NAMED_ARGUMENT.invariant,
                Detector::Signature,
                match expected.as_str() {
                    "unknown" => format!("`{}` has no parameter `{actual}`", r.target),
                    "twice" => format!("`{}`'s parameter `{actual}` is given twice", r.target),
                    "unnamed" => format!(
                        "`{}` is a function value, whose parameters have no names, and this \
                         names `{actual}`",
                        r.target
                    ),
                    _ => format!(
                        "a positional argument follows a named one in this call to `{}`",
                        r.target
                    ),
                },
                r.span.clone(),
            )
            .reason("named_argument_names_no_parameter")
            .explain(
                "an argument written with a name is given to the parameter of that name; \
                 the positional ones come first, and fill the parameters in order",
            )
            .repair("name a parameter the declaration has, once, after the positional arguments"),
            RelationKind::Text => Diagnostic::error(
                crate::codes::OPERAND_TYPE.id,
                crate::codes::OPERAND_TYPE.invariant,
                Detector::Signature,
                format!("`{}` is `{actual}`, which has no text form", r.target),
                r.span.clone(),
            )
            .reason("template_value_has_no_text_form")
            .explain(
                "a template writes a `String`, an `Int` or a `Bool` as text, or an opaque \
                 type over one; a `Float` has no format yet, and the renderer refuses \
                 anything else when it renders",
            )
            .repair("write a field of it, or a `String` made from it"),
            // A string's hole, `"{..}"`, in code (ADR-0084), where `match` takes
            // the value apart; anything else is a template's.
            RelationKind::Absent if r.target.starts_with('"') => Diagnostic::error(
                crate::codes::OPTION_USED_AS_VALUE.id,
                crate::codes::OPTION_USED_AS_VALUE.invariant,
                Detector::Signature,
                format!(
                    "`{}` is `{actual}`, a value that may be absent; `match` takes it apart",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("string_reads_optional_as_present")
            .explain(
                "an `Option` or a `Result` is a value that may be absent or failed; a \
                 string holds text, and `match` says what it holds when there is none",
            )
            .repair("take it apart with `match`, and write the value in its `Some` or `Ok` arm"),
            RelationKind::Absent => Diagnostic::error(
                crate::codes::OPTION_USED_AS_VALUE.id,
                crate::codes::OPTION_USED_AS_VALUE.invariant,
                Detector::Signature,
                format!(
                    "`{}` is `{actual}`, a value that may be absent; `{{#match}}` takes it apart",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("template_reads_optional_as_present")
            .explain(
                "an `Option` or a `Result` is a value that may be absent or failed; a \
                 template takes it apart with `{#match}`, which says what renders when \
                 there is none",
            )
            .repair("write `{#match v}{:Some(x)} .. {:None} .. {/match}`"),
            RelationKind::Handler => Diagnostic::error(
                crate::codes::NOT_CALLABLE.id,
                crate::codes::NOT_CALLABLE.invariant,
                Detector::Signature,
                format!(
                    "`{}` is given `{actual}`, which is not a function to call",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("event_attribute_is_not_a_function")
            .explain("an event attribute's value is called when the event arrives")
            .repair("give it a lambda, or a function the program declares"),
            RelationKind::Items => Diagnostic::error(
                crate::codes::LIST_ITEMS.id,
                crate::codes::LIST_ITEMS.invariant,
                Detector::Signature,
                format!("this item of the list is `{actual}`, where the first is `{expected}`"),
                r.span.clone(),
            )
            .reason("list_items_of_two_types")
            .explain("a list holds values of one type, whichever item is read")
            .repair(format!("make this item a `{expected}`")),
            RelationKind::Literal => Diagnostic::error(
                crate::codes::INT_LITERAL_RANGE.id,
                crate::codes::INT_LITERAL_RANGE.invariant,
                Detector::Signature,
                format!("`{}` does not fit in an `Int`, which is 64 bits", r.target),
                r.span.clone(),
            )
            .reason("int_literal_out_of_range")
            .explain(
                "an `Int` holds -9223372036854775808 to 9223372036854775807; a literal \
                 is its magnitude, so the least is written as an operation on it",
            )
            .repair("write a value in the range, or use a `Float`"),
            RelationKind::Callee => Diagnostic::error(
                crate::codes::NOT_CALLABLE.id,
                crate::codes::NOT_CALLABLE.invariant,
                Detector::Signature,
                format!(
                    "`{}` is called, and it is `{actual}`, not a function",
                    r.target
                ),
                r.span.clone(),
            )
            .reason("called_value_is_not_a_function")
            .explain("a call runs a function; a value of another type has nothing to run")
            .repair(format!(
                "call a function, or use `{}` as the value it is",
                r.target
            )),
            RelationKind::Fields => Diagnostic::error(
                crate::codes::RECORD_FIELDS.id,
                crate::codes::RECORD_FIELDS.invariant,
                Detector::Signature,
                match expected.as_str() {
                    FIELD_UNDECLARED => format!("`{}` has no field `{actual}`", r.target),
                    FIELD_ONCE => format!("`{}` is given its field `{actual}` twice", r.target),
                    _ => format!("`{}` is built without its field `{actual}`", r.target),
                },
                r.span.clone(),
            )
            .reason("record_built_with_other_fields")
            .explain(
                "a record's value holds each field its type declares, so it is built with each \
                 of them, once, and with nothing else",
            )
            .repair(match expected.as_str() {
                FIELD_UNDECLARED => format!("remove `{actual}`, or declare it on `{}`", r.target),
                FIELD_ONCE => format!("give `{actual}` once"),
                _ => format!("give `{actual}` a value"),
            }),
            RelationKind::Annotation => Diagnostic::error(
                crate::codes::UNRESOLVED_TYPE.id,
                crate::codes::UNRESOLVED_TYPE.invariant,
                Detector::Signature,
                format!("{} names `{actual}`: {expected}", r.target),
                r.span.clone(),
            )
            .reason("written_type_does_not_resolve")
            .explain(
                "a written type must name a type declaration visible from where it is written, \
                 applied to exactly the parameters that declaration binds; an annotation that \
                 resolves to nothing checks nothing",
            )
            .repair("import the type, correct its spelling, or supply its type arguments"),
        };
        out.push(d.related(r.boundary.0.clone(), r.boundary.1.clone()));
    }
    out
}

/// **Every value relation in the program, and what was concluded.** The audit
/// view: the same walk the checker projects diagnostics from.
pub fn analysis(units: &[crate::check::Unit]) -> Vec<(String, Vec<ValueRelation>)> {
    let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = Workspace::build(&hirs);
    let sigs = Signatures::build(&ws, &hirs);
    units
        .iter()
        .enumerate()
        .map(|(i, u)| (u.path.clone(), relations(&u.hir, &sigs, &ws, i)))
        .collect()
}

/// **"a" or "an" before a type's name**, as it is said: "an `Int`", "a
/// `Shape`". By its first letter, which is all a type's name gives.
fn article(name: &str) -> &'static str {
    match name.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}
