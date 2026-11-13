//! **Pure computation → JavaScript modules** (ADR-0044).
//!
//! Charter §14 M10 task 2, in its stated order:
//!
//! > first: generated modern JavaScript modules for handlers and pure
//! > computation; then: Wasm for pure compute-heavy modules.
//!
//! Handlers compile to modules since ADR-0033. This is the other half: a query
//! that computes and reaches no host becomes an ES module exporting `run`,
//! encoded from the same backend IR the component encoder reads
//! (`backend::ir`). The lowering is shared, so the two backends cannot type,
//! inline or resolve one declaration differently. Only the encoding differs,
//! and a differential test runs both on the same inputs
//! (`pw-conformance/tests/javascript.rs`).
//!
//! # Pleris semantics, not JavaScript's
//!
//! Every place the two languages differ is written out here rather than left
//! to the host language:
//! - an `Int` is 64 bits, checked after every `+`, `-`, `*` and negation,
//!   and `/` and `%` are Euclidean and trap on a zero divisor (ADR-0039 §1):
//!   each is exact, on two numbers where its result is a safe integer and
//!   in `BigInt` otherwise (ADR-0433);
//! - a `String` is ordered by code point, where JavaScript's `<` orders UTF-16
//!   units and puts U+1F600 before U+FF61; its length counts code points;
//! - `String.trim` strips Unicode `White_Space`, which is not ECMAScript's
//!   set: JavaScript strips U+FEFF and keeps U+0085;
//! - a trap is a thrown `Error` whose message begins `trap:`.
//!
//! # Values
//!
//! An `Int` is a number within ±2^53 and a `BigInt` past it, one form for
//! each value, so that two equal ones are `===` (ADR-0433); a `Float` a
//! number, a `Bool` a boolean, a `String` a string, a list an array, and a
//! record an object keyed by its Pleris field names. That is the form JSON
//! carries a value in (ADR-0130), so a module reads a signal's value, and
//! writes one, as it is: nothing copied, a row it leaves alone the very
//! object it read. `Some(v)` is `{ $case: "some", value: v }`, `None` is
//! `{ $case: "none" }`, and `Ok` and `Err` the same; `$` begins no Pleris
//! name, so a case can never be read as a record. A declared sum type's case
//! is named as its WIT case is, `Shape.Circle(r)` being
//! `{ $case: "circle", value: r }`, and a case of several fields holds them
//! in an array, as a tuple is held (ADR-0059). An opaque type is its
//! representation, as it is on a wire.

use std::collections::{BTreeMap, BTreeSet};

use crate::resolve::DefId;

use super::case::{self, Case};
use super::ir::{
    BinaryOp, BuiltinCase, Case as VariantCase, Const, EachKind, Function, Instr, Intrinsic,
    MatchArm, Program, Region, Shape, Terminator, Type, UnaryOp, ValueId,
};

/// **A query's pure computation, as an ES module.** `Err` names what the
/// encoding does not take, as the Wasm encoder's refusals do.
pub fn module(units: &[crate::check::Unit], component_id: &str) -> Result<String, String> {
    super::component::lowered(units, component_id, |function, program| {
        encode(component_id, function, program)
    })
}

/// **Every query of a program that reaches no host, as a module**, by
/// component id: what `pw build` writes to `modules/`.
pub fn modules(units: &[crate::check::Unit]) -> Result<Vec<(String, String)>, String> {
    super::component::pure_queries(units, encode_modules)
}

pub(crate) fn modules_prepared(
    p: &super::component::Program<'_>,
) -> Result<Vec<(String, String)>, crate::program::Refusal> {
    encode_prepared(&super::component::pure_functions(p), p.lowered())
}

fn encode_modules(
    pure: &[(String, &Function)],
    program: &Program,
) -> Result<Vec<(String, String)>, String> {
    encode_prepared(pure, program).map_err(|r| r.message)
}

fn encode_prepared(
    pure: &[(String, &Function)],
    program: &Program,
) -> Result<Vec<(String, String)>, crate::program::Refusal> {
    pure.iter()
        .map(|(id, function)| {
            encode(id, function, program)
                .map(|source| (id.clone(), source))
                .map_err(|why| crate::program::Refusal::new(crate::program::Stage::PureJs, id, why))
        })
        .collect()
}

fn encode(component_id: &str, function: &Function, program: &Program) -> Result<String, String> {
    let refused = |why: String| format!("`{component_id}` is not a JavaScript module: {why}");
    // The functions compiled beside the query (ADR-0050), each a function
    // of the module, named by its position.
    let callees: BTreeMap<(DefId, Vec<Type>), usize> = function
        .callees
        .iter()
        .enumerate()
        .map(|(n, c)| ((c.def, c.instance.clone()), n))
        .collect();
    let mut helpers = BTreeSet::new();
    let mut constants = BTreeMap::new();
    let mut sources = Vec::new();
    // The function values' code (ADR-0052): its captures, then its
    // parameters, as a function of the module.
    for (n, c) in function.closures.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(&c.function).map_err(refused)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants).map_err(refused)?;
        let params: Vec<String> = c.function.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            closure_name(n),
            params.join(", "),
            e.body
        ));
    }
    for (n, c) in function.callees.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(c).map_err(refused)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants).map_err(refused)?;
        let params: Vec<String> = c.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            callee_name(n),
            params.join(", "),
            e.body
        ));
    }
    let mut e = Emitter::new(program, &callees, &function.closures);
    e.function(function).map_err(refused)?;
    e.helpers.extend(helpers);
    constants_into(&mut e.constants, &constants).map_err(refused)?;
    Ok(e.finish(component_id, function, &sources))
}

/// **A resumable handler, as its ES module** (ADR-0058). `paths` are the
/// captured paths its parameters are, in order.
pub(crate) fn handler_source(
    function: &Function,
    program: &Program,
    identity: &str,
    name: &str,
    paths: &[String],
    // Whether the function's last parameter is the event (ADR-0138).
    event: bool,
) -> Result<String, String> {
    let callees: BTreeMap<(DefId, Vec<Type>), usize> = function
        .callees
        .iter()
        .enumerate()
        .map(|(n, c)| ((c.def, c.instance.clone()), n))
        .collect();
    let mut helpers = BTreeSet::new();
    let mut constants = BTreeMap::new();
    let mut sources = Vec::new();
    for (n, c) in function.closures.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(&c.function)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants)?;
        let params: Vec<String> = c.function.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            closure_name(n),
            params.join(", "),
            e.body
        ));
    }
    for (n, c) in function.callees.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(c)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants)?;
        let params: Vec<String> = c.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            callee_name(n),
            params.join(", "),
            e.body
        ));
    }
    let mut e = Emitter::new(program, &callees, &function.closures);
    e.handler = true;
    e.handler_function(function, paths, event)?;
    e.helpers.extend(helpers);
    constants_into(&mut e.constants, &constants)?;
    Ok(e.finish_handler(identity, name, &sources))
}

/// **A function as a JavaScript expression of its own scope** (ADR-0122):
/// `(() => { helpers; callees; return function (..) { body }; })()`. A
/// speculation module holds several, each with the callees it numbered, so
/// each gets its own scope rather than a shared namespace.
pub(crate) fn isolated(function: &Function, program: &Program) -> Result<String, String> {
    let callees: BTreeMap<(DefId, Vec<Type>), usize> = function
        .callees
        .iter()
        .enumerate()
        .map(|(n, c)| ((c.def, c.instance.clone()), n))
        .collect();
    let mut helpers = BTreeSet::new();
    let mut constants = BTreeMap::new();
    let mut sources = Vec::new();
    for (n, c) in function.closures.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(&c.function)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants)?;
        let params: Vec<String> = c.function.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            closure_name(n),
            params.join(", "),
            e.body
        ));
    }
    for (n, c) in function.callees.iter().enumerate() {
        let mut e = Emitter::new(program, &callees, &function.closures);
        e.function(c)?;
        helpers.extend(e.helpers.iter().copied());
        constants_into(&mut constants, &e.constants)?;
        let params: Vec<String> = c.params.iter().map(|(v, _)| val(*v)).collect();
        sources.push(format!(
            "function {}({}) {{\n{}}}",
            callee_name(n),
            params.join(", "),
            e.body
        ));
    }
    let mut e = Emitter::new(program, &callees, &function.closures);
    e.function(function)?;
    e.helpers.extend(helpers);
    constants_into(&mut e.constants, &constants)?;
    let mut before: Vec<String> = e.prelude();
    before.extend(sources);
    let params: Vec<String> = function.params.iter().map(|(v, _)| val(*v)).collect();
    Ok(format!(
        "(() => {{\n{}{}return function ({}) {{\n{}}};\n}})()",
        before.join("\n\n"),
        if before.is_empty() { "" } else { "\n\n" },
        params.join(", "),
        e.body
    ))
}

/// **A step from one version of a value the device keeps to the next**
/// (track `device-store`, ADR-0366, decision 4): `function`, as [`isolated`]
/// writes it, taking the stored value in the browser's wire form and giving
/// the next version's in the same form, with each helper the decoding and the
/// encoding use.
pub(crate) fn wire_to_wire(function: &Function, program: &Program) -> Result<String, String> {
    let f = isolated(function, program)?;
    let [(_, from)] = function.params.as_slice() else {
        return Err(format!("`{}` does not take one value", function.export));
    };
    let none = BTreeMap::new();
    let mut e = Emitter::new(program, &none, &[]);
    let decode = e.decode("j", from)?;
    let encode = e.wire_value("r", &function.ret)?;
    let before = e.prelude().join("\n\n");
    Ok(format!(
        "(() => {{\n{before}{}const f = {f};\nreturn (j) => {{\n  const r = f({decode});\n  return {encode};\n}};\n}})()",
        if before.is_empty() { "" } else { "\n\n" }
    ))
}

/// **A JSON value read as the module's value of `ty`** (ADR-0122), by the
/// rule a handler reads its captures by: an `Int` into a `BigInt`, a record
/// by its field names, an opaque type as its representation.
/// **A constant, as JSON carries it** (ADR-0130): a signal's first value,
/// which the server renders and the document hands the browser. The value
/// a function of no parameters makes from literals, cases, records and
/// lists, in the form `decode` reads and `wire_value` writes, with each
/// case named as the browser's modules name it. Anything computed is
/// refused: the first value is data the build writes, not a program it runs.
pub(crate) fn constant(
    program: &Program,
    function: &Function,
) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    let none = BTreeMap::new();
    let e = Emitter::new(program, &none, &[]);
    let [entry] = function.blocks.as_slice() else {
        return Err("a first value of several blocks".into());
    };
    let Terminator::Return(result) = &entry.terminator else {
        return Err("a first value that returns nothing".into());
    };
    let mut held: BTreeMap<ValueId, J> = BTreeMap::new();
    let get = |held: &BTreeMap<ValueId, J>, v: &ValueId| {
        held.get(v)
            .cloned()
            .ok_or_else(|| format!("{v:?} is not a constant"))
    };
    for i in &entry.instrs {
        let value = match i {
            Instr::Const { value, .. } => match value {
                Const::Int(n) if n.unsigned_abs() <= 1 << 53 => J::from(*n),
                Const::Int(n) => {
                    return Err(format!("{n}, which JSON does not carry exactly (ADR-0033)"));
                }
                Const::Float(f) => serde_json::Number::from_f64(*f)
                    .map(J::Number)
                    .ok_or_else(|| format!("{f}, which JSON does not carry"))?,
                Const::Bool(b) => J::Bool(*b),
                Const::Str(s) => J::String(s.clone()),
                Const::Unit => J::Null,
            },
            Instr::Case {
                case, fields, ty, ..
            } => {
                let (name, _) = e.case_of(ty, VariantCase::Declared(*case))?;
                let mut out = serde_json::Map::new();
                out.insert("$case".into(), J::String(name));
                match fields.as_slice() {
                    [] => {}
                    [one] => {
                        out.insert("value".into(), get(&held, one)?);
                    }
                    many => {
                        let each = many
                            .iter()
                            .map(|f| get(&held, f))
                            .collect::<Result<Vec<_>, _>>()?;
                        out.insert("value".into(), J::Array(each));
                    }
                }
                J::Object(out)
            }
            Instr::Variant { case, payload, .. } => {
                let mut out = serde_json::Map::new();
                out.insert("$case".into(), J::String(case_name(*case).into()));
                if let Some(p) = payload {
                    out.insert("value".into(), get(&held, p)?);
                }
                J::Object(out)
            }
            Instr::Construct { args, ty, .. } => {
                let Type::Nominal(def, targs) = ty else {
                    return Err(format!("a record of type {ty:?}"));
                };
                let Some(Shape::Record { fields }) = e.shape(*def, targs) else {
                    return Err(format!("a record of type {ty:?} with no shape"));
                };
                let mut out = serde_json::Map::new();
                for ((n, _), a) in fields.iter().zip(args) {
                    out.insert(n.clone(), get(&held, a)?);
                }
                J::Object(out)
            }
            Instr::MakeList { items, .. } => J::Array(
                items
                    .iter()
                    .map(|v| get(&held, v))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Instr::Retype { value, .. } => get(&held, value)?,
            other => {
                return Err(format!(
                    "a computed first value ({}); a signal's first value is a literal, a \
                     case, or a record or list of them",
                    instr_name(other)
                ));
            }
        };
        held.insert(i.result(), value);
    }
    get(&held, result)
}

/// **A value as the browser's wire carries it** (ADR-0205): `j`, a value
/// of `ty` written nested, with each value of a type that contains itself
/// made its nodes, in the order `wire_graph` writes them: level order, each
/// node's own values of the type in the order its fields are declared. A
/// signal's first value, which the build writes and the browser holds.
pub(crate) fn wire_json(
    program: &Program,
    ty: &Type,
    j: serde_json::Value,
) -> Result<serde_json::Value, String> {
    wire_walk(program, ty, j, None, &mut Vec::new())
}

/// `j`, a value of `ty`; inside the graph `inside`, each value of its type
/// put on `queue` as the next node, and written as that node.
fn wire_walk(
    program: &Program,
    ty: &Type,
    j: serde_json::Value,
    inside: Option<&Graph>,
    queue: &mut Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    let cases = |j: J, cases: &[(String, Vec<Type>)], queue: &mut Vec<J>| -> Result<J, String> {
        let J::Object(mut o) = j else {
            return Err(format!("{j} is no case"));
        };
        let named = o
            .get("$case")
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_string();
        let Some((_, fields)) = cases.iter().find(|(n, _)| crate::wit::ident(n) == named) else {
            return Err(format!("`{named}` is no case of the type"));
        };
        match (fields.as_slice(), o.remove("value")) {
            ([], _) | (_, None) => {}
            ([one], Some(v)) => {
                o.insert("value".into(), wire_walk(program, one, v, inside, queue)?);
            }
            (many, Some(J::Array(items))) => {
                let mut out = Vec::new();
                for (t, v) in many.iter().zip(items) {
                    out.push(wire_walk(program, t, v, inside, queue)?);
                }
                o.insert("value".into(), J::Array(out));
            }
            (_, Some(other)) => return Err(format!("{other} is no case's fields")),
        }
        Ok(J::Object(o))
    };
    Ok(match ty {
        Type::Nominal(d, a) if inside == Some(&(*d, a.clone())) => {
            queue.push(j);
            serde_json::json!({ "$node": queue.len() - 1 })
        }
        Type::Nominal(d, a) if contains_itself(program, *d, a) => {
            held_back(program, *d, a)?;
            let graph = (*d, a.clone());
            let mut nodes_queue = vec![j];
            let mut nodes = Vec::new();
            let mut at = 0;
            while at < nodes_queue.len() {
                let n = std::mem::take(&mut nodes_queue[at]);
                nodes.push(wire_shape(
                    program,
                    *d,
                    a,
                    n,
                    Some(&graph),
                    &mut nodes_queue,
                )?);
                at += 1;
            }
            serde_json::json!({ "$graph": nodes })
        }
        Type::Nominal(d, a) => wire_shape(program, *d, a, j, inside, queue)?,
        Type::List(t) => match j {
            J::Array(items) => J::Array(
                items
                    .into_iter()
                    .map(|v| wire_walk(program, t, v, inside, queue))
                    .collect::<Result<_, _>>()?,
            ),
            other => return Err(format!("{other} is no list")),
        },
        Type::Option(t) => cases(
            j,
            &[
                ("some".into(), vec![(**t).clone()]),
                ("none".into(), Vec::new()),
            ],
            queue,
        )?,
        Type::Result(t, e) => cases(
            j,
            &[
                ("ok".into(), vec![(**t).clone()]),
                ("err".into(), vec![(**e).clone()]),
            ],
            queue,
        )?,
        _ => j,
    })
}

/// `j` as the shape of `def` under `args`, its fields and cases walked.
fn wire_shape(
    program: &Program,
    def: DefId,
    args: &[Type],
    j: serde_json::Value,
    inside: Option<&Graph>,
    queue: &mut Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    Ok(match shape_of(program, def, args) {
        Some(Shape::Alias(of)) => wire_walk(program, of, j, inside, queue)?,
        Some(Shape::Record { fields }) => {
            let J::Object(mut o) = j else {
                return Err(format!("{j} is no record"));
            };
            // In the order the fields are declared, as `wire_graph` puts
            // them.
            for (n, t) in fields {
                if let Some(v) = o.remove(n) {
                    let v = wire_walk(program, t, v, inside, queue)?;
                    o.insert(n.clone(), v);
                }
            }
            J::Object(o)
        }
        Some(Shape::Variant { cases }) => {
            let cases = cases.clone();
            let J::Object(mut o) = j else {
                return Err(format!("{j} is no case"));
            };
            let named = o
                .get("$case")
                .and_then(J::as_str)
                .unwrap_or_default()
                .to_string();
            let Some((_, fields)) = cases.iter().find(|(n, _)| crate::wit::ident(n) == named)
            else {
                return Err(format!("`{named}` is no case of the type"));
            };
            match (fields.as_slice(), o.remove("value")) {
                ([], _) | (_, None) => {}
                ([one], Some(v)) => {
                    o.insert("value".into(), wire_walk(program, one, v, inside, queue)?);
                }
                (many, Some(J::Array(items))) => {
                    let mut out = Vec::new();
                    for (t, v) in many.iter().zip(items) {
                        out.push(wire_walk(program, t, v, inside, queue)?);
                    }
                    o.insert("value".into(), J::Array(out));
                }
                (_, Some(other)) => return Err(format!("{other} is no case's fields")),
            }
            J::Object(o)
        }
        None => j,
    })
}

/// An instruction's kind, for a refusal.
fn instr_name(i: &Instr) -> &'static str {
    match i {
        Instr::Call { .. } | Instr::ImportCall { .. } | Instr::Apply { .. } => "a call",
        Instr::Binary { .. } | Instr::Unary { .. } => "an operation",
        Instr::Match { .. } | Instr::If { .. } => "a branch",
        _ => "a computation",
    }
}

/// **The type of a graph** (ADR-0205): a declaration, under its arguments,
/// whose values hold values of itself, and cross the browser's wire as their
/// nodes.
type Graph = (DefId, Vec<Type>);

/// A declaration's shape, under its arguments.
fn shape_of<'p>(program: &'p Program, def: DefId, args: &[Type]) -> Option<&'p Shape> {
    program
        .types
        .iter()
        .find(|t| t.def == def && t.args == args)
        .map(|t| &t.shape)
}

/// Each type a shape holds directly.
fn shape_types(shape: &Shape) -> Vec<&Type> {
    match shape {
        Shape::Alias(of) => vec![of],
        Shape::Record { fields } => fields.iter().map(|(_, t)| t).collect(),
        Shape::Variant { cases } => cases.iter().flat_map(|(_, fs)| fs.iter()).collect(),
    }
}

/// Does `ty` reach `target` through the shapes it holds?
fn reaches(program: &Program, ty: &Type, target: &Graph, seen: &mut BTreeSet<Graph>) -> bool {
    match ty {
        Type::Nominal(d, a) => {
            let key = (*d, a.clone());
            if key == *target {
                return true;
            }
            if !seen.insert(key) {
                return false;
            }
            shape_of(program, *d, a).is_some_and(|s| {
                shape_types(s)
                    .into_iter()
                    .any(|t| reaches(program, t, target, seen))
            })
        }
        Type::List(t) | Type::Option(t) | Type::Set(t) => reaches(program, t, target, seen),
        Type::Result(a, b) | Type::Map(a, b) => {
            reaches(program, a, target, seen) || reaches(program, b, target, seen)
        }
        Type::Function(ps, r) => {
            ps.iter().any(|t| reaches(program, t, target, seen))
                || reaches(program, r, target, seen)
        }
        Type::Int | Type::Float | Type::Bool | Type::Str | Type::Unit => false,
    }
}

/// **Does the declaration `def`, under `args`, hold a value of itself**
/// (ADR-0194)? Its values cross the browser's wire as their nodes
/// (ADR-0205).
fn contains_itself(program: &Program, def: DefId, args: &[Type]) -> bool {
    let target = (def, args.to_vec());
    shape_of(program, def, args).is_some_and(|s| {
        let mut seen = BTreeSet::new();
        shape_types(s)
            .into_iter()
            .any(|t| reaches(program, t, &target, &mut seen))
    })
}

/// **Two types that hold each other** (ADR-0205): `Ok` where no other
/// declaration `def`'s shape reaches holds it back. Their values would be
/// nodes of either type, which the browser's wire does not carry yet, as a
/// component's boundary does not (ADR-0202).
fn held_back(program: &Program, def: DefId, args: &[Type]) -> Result<(), String> {
    let target = (def, args.to_vec());
    let mut others: Vec<Graph> = Vec::new();
    let mut stack: Vec<&Type> = shape_of(program, def, args)
        .map(shape_types)
        .unwrap_or_default();
    while let Some(t) = stack.pop() {
        match t {
            Type::Nominal(d, a) => {
                let key = (*d, a.clone());
                if key == target || others.contains(&key) {
                    continue;
                }
                others.push(key);
                if let Some(s) = shape_of(program, *d, a) {
                    stack.extend(shape_types(s));
                }
            }
            Type::List(t) | Type::Option(t) | Type::Set(t) => stack.push(t),
            Type::Result(a, b) | Type::Map(a, b) => stack.extend([&**a, &**b]),
            Type::Function(ps, r) => {
                stack.extend(ps.iter());
                stack.push(r);
            }
            Type::Int | Type::Float | Type::Bool | Type::Str | Type::Unit => {}
        }
    }
    for (d, a) in &others {
        let back = shape_of(program, *d, a).is_some_and(|s| {
            let mut seen = BTreeSet::new();
            shape_types(s)
                .into_iter()
                .any(|t| reaches(program, t, &target, &mut seen))
        });
        if back {
            return Err(format!(
                "a value of type {:?}, which holds {:?} and is held by it: two types that \
                 hold each other are not on the browser's wire yet",
                Type::Nominal(def, args.to_vec()),
                Type::Nominal(*d, a.clone())
            ));
        }
    }
    Ok(())
}

/// **Does a value of `ty` hold a type that contains itself** (ADR-0194)?
/// A decoder or an encoder here is written out by the type's shape, so one
/// for such a type would never end.
fn holds_itself(program: &Program, ty: &Type) -> bool {
    fn walk(
        program: &Program,
        ty: &Type,
        path: &mut Vec<(crate::resolve::DefId, Vec<Type>)>,
        clear: &mut BTreeSet<(crate::resolve::DefId, Vec<Type>)>,
    ) -> bool {
        match ty {
            Type::Nominal(d, args) => {
                let key = (*d, args.clone());
                if path.contains(&key) {
                    return true;
                }
                if clear.contains(&key) {
                    return false;
                }
                let Some(shape) = program
                    .types
                    .iter()
                    .find(|t| t.def == *d && t.args == *args)
                    .map(|t| &t.shape)
                else {
                    return false;
                };
                path.push(key.clone());
                let found = match shape {
                    Shape::Alias(of) => walk(program, of, path, clear),
                    Shape::Record { fields } => {
                        fields.iter().any(|(_, t)| walk(program, t, path, clear))
                    }
                    Shape::Variant { cases } => cases
                        .iter()
                        .any(|(_, fs)| fs.iter().any(|t| walk(program, t, path, clear))),
                };
                path.pop();
                if !found {
                    clear.insert(key);
                }
                found
            }
            Type::List(t) | Type::Option(t) | Type::Set(t) => walk(program, t, path, clear),
            Type::Result(a, b) | Type::Map(a, b) => {
                walk(program, a, path, clear) || walk(program, b, path, clear)
            }
            Type::Function(ps, r) => {
                ps.iter().any(|t| walk(program, t, path, clear)) || walk(program, r, path, clear)
            }
            Type::Int | Type::Float | Type::Bool | Type::Str | Type::Unit => false,
        }
    }
    walk(program, ty, &mut Vec::new(), &mut BTreeSet::new())
}

pub(crate) fn decoder(program: &Program, ty: &Type, expr: &str) -> Result<String, String> {
    let none = BTreeMap::new();
    Emitter::new(program, &none, &[]).decode(expr, ty)
}

/// [`decoder`], for a value a host writes by its type: a speculated value
/// (ADR-0233). A value of a type that contains itself is read from its
/// nodes, as a signal's is (ADR-0205).
pub(crate) fn decoder_of_nodes(program: &Program, ty: &Type, expr: &str) -> Result<String, String> {
    let none = BTreeMap::new();
    let mut emitter = Emitter::new(program, &none, &[]);
    emitter.graphs = true;
    emitter.decode(expr, ty)
}

fn json(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// A function compiled beside the query, in the module (ADR-0050).
fn callee_name(n: usize) -> String {
    format!("callee{n}")
}

/// A function value's code, in the module (ADR-0052).
fn closure_name(n: usize) -> String {
    format!("closure{n}")
}

struct Emitter<'p> {
    program: &'p Program,
    /// The functions compiled beside the query, by instance (ADR-0050).
    callees: &'p BTreeMap<(DefId, Vec<Type>), usize>,
    /// The function values' code (ADR-0052).
    closures: &'p [super::ir::Closure],
    /// Each value's type, as the IR states it.
    types: BTreeMap<ValueId, Type>,
    /// The prelude functions the body calls.
    helpers: BTreeSet<&'static str>,
    /// Each literal value's text, by the value it is (ADR-0427).
    literals: BTreeMap<ValueId, String>,
    /// **The lists of literals the body reads, each a value of the module**
    /// (ADR-0427), by name: its elements' text.
    constants: BTreeMap<String, String>,
    /// The literals such a list was made of, whose lines go where nothing
    /// else reads them.
    hoisted: BTreeSet<ValueId>,
    /// A resumable handler's body (ADR-0058): a command it calls is awaited.
    /// Off in a function value's body, which is not async.
    handler: bool,
    /// Whether what is decoded now is a value the browser's modules wrote,
    /// a signal's, which holds a type that contains itself as its nodes
    /// (ADR-0205).
    graphs: bool,
    /// **Whether what is decoded now comes from outside the page**: a
    /// command's answer, which the server writes as the call returns. Read
    /// case by case, so that a case its type does not have traps where it
    /// arrives rather than being carried on; the page's own values, its
    /// signals, captures and events, are read as they are (ADR-0433).
    outside: bool,
    /// **How many times each value of the body is read** (ADR-0436),
    /// counted before it is written: an `Option` the match after it reads,
    /// and nothing else, is taken apart where it is made.
    reads: BTreeMap<ValueId, usize>,
    /// The values so never made: the body is refused if it names one
    /// (`unmade_unread`).
    unmade: BTreeSet<ValueId>,
    body: String,
    depth: usize,
}

/// **An `Option` as a test and a payload** (ADR-0436): what `List.get`,
/// `Map.get` and `List.find` make, as the lines that come first, the test
/// that holds where it is `Some`, and what `Some` holds, read only where the
/// test holds.
struct Opened {
    before: Vec<String>,
    test: String,
    payload: String,
}

/// The arms of a match that takes `Some` in one and `None` in the other,
/// each case alone: the match an [`Opened`] value is taken apart by.
fn option_arms(arms: &[MatchArm]) -> Option<(&MatchArm, &MatchArm)> {
    let alone = |c: BuiltinCase| arms.iter().find(|a| a.cases == [VariantCase::Builtin(c)]);
    match (
        arms.len(),
        alone(BuiltinCase::Some),
        alone(BuiltinCase::None),
    ) {
        (2, Some(some), Some(none)) => Some((some, none)),
        _ => None,
    }
}

/// How many times `name` is in `body` as a name, not part of a longer one.
fn named(body: &str, name: &str) -> usize {
    let word =
        |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    body.match_indices(name)
        .filter(|(at, _)| {
            !word(body[..*at].chars().next_back()) && !word(body[at + name.len()..].chars().next())
        })
        .count()
}

/// Each prelude function, by name, and what it calls.
const HELPERS: &[(&str, &[&str], &str)] = &[
    (
        "trap",
        &[],
        "function trap(what) {\n  throw new Error(\"trap: \" + what);\n}",
    ),
    (
        "big",
        &["trap"],
        "// An Int is a number within 2^53 of zero and a BigInt past it, 64 bits\n\
         // at most (ADR-0039, ADR-0433): one form for each value, so that two\n\
         // equal ones are `===`, and the form JSON gives each Int it carries. A\n\
         // result 64 bits do not hold traps: one they wrap is not itself.\n\
         function big(v) {\n  if (BigInt.asIntN(64, v) !== v) trap(\"Int overflow\");\n  \
         return v >= -9007199254740992n && v <= 9007199254740992n ? Number(v) : v;\n}",
    ),
    (
        "add",
        &["big"],
        "// Two numbers' sum, difference or product is exact where it is a safe\n\
         // integer, the nearest double to one past 2^53 - 1 being at least 2^53;\n\
         // past that it is made again as a BigInt.\n\
         function add(a, b) {\n  if (typeof a === \"number\" && typeof b === \"number\") {\n    \
         const r = a + b;\n    if (r >= -9007199254740991 && r <= 9007199254740991) return r;\n  }\n  \
         return big(BigInt(a) + BigInt(b));\n}",
    ),
    (
        "sub",
        &["big"],
        "function sub(a, b) {\n  if (typeof a === \"number\" && typeof b === \"number\") {\n    \
         const r = a - b;\n    if (r >= -9007199254740991 && r <= 9007199254740991) return r;\n  }\n  \
         return big(BigInt(a) - BigInt(b));\n}",
    ),
    (
        "mul",
        &["big"],
        "// A product of 0 is 0, never -0.\n\
         function mul(a, b) {\n  if (typeof a === \"number\" && typeof b === \"number\") {\n    \
         const r = a * b;\n    if (r >= -9007199254740991 && r <= 9007199254740991) return r + 0;\n  }\n  \
         return big(BigInt(a) * BigInt(b));\n}",
    ),
    (
        "neg",
        &["big"],
        "function neg(a) {\n  return typeof a === \"number\" ? 0 - a : big(-a);\n}",
    ),
    (
        "div",
        &["trap", "big"],
        "// Euclidean: the remainder is in [0, |b|), so -7 / 2 is -4. Exact on two\n\
         // numbers: `a % b` is, and `a - a % b` is a multiple of `b` no larger\n\
         // than `a`, so their quotient is the whole number it is.\n\
         function div(a, b) {\n  if (b == 0) trap(\"division by zero\");\n  \
         if (typeof a === \"number\" && typeof b === \"number\") {\n    const r = a % b;\n    \
         const q = (a - r) / b;\n    return (r < 0 ? (b > 0 ? q - 1 : q + 1) : q) + 0;\n  }\n  \
         const x = BigInt(a), y = BigInt(b);\n  let q = x / y;\n  \
         if (x % y < 0n) q = y > 0n ? q - 1n : q + 1n;\n  return big(q);\n}",
    ),
    (
        "rem",
        &["trap", "big"],
        "function rem(a, b) {\n  if (b == 0) trap(\"division by zero\");\n  \
         if (typeof a === \"number\" && typeof b === \"number\") {\n    const r = a % b;\n    \
         return (r < 0 ? (b > 0 ? r + b : r - b) : r) + 0;\n  }\n  \
         const x = BigInt(a), y = BigInt(b);\n  const r = x % y;\n  \
         return big(r < 0n ? (y > 0n ? r + y : r - y) : r);\n}",
    ),
    (
        "compare",
        &[],
        "// By code point, as UTF-8's bytes order them. `<` on two JavaScript\n\
         // strings orders UTF-16 units, and puts U+1F600 before U+FF61.\n\
         function compare(a, b) {\n  const x = a[Symbol.iterator](), y = b[Symbol.iterator]();\n  \
         for (;;) {\n    const p = x.next(), q = y.next();\n    if (p.done || q.done) return p.done ? (q.done ? 0 : -1) : 1;\n    \
         const c = p.value.codePointAt(0) - q.value.codePointAt(0);\n    if (c !== 0) return c < 0 ? -1 : 1;\n  }\n}",
    ),
    (
        "sign",
        &[],
        "function sign(n) {\n  return n > 0 ? 1 : n < 0 ? -1 : 0;\n}",
    ),
    (
        "some",
        &[],
        "function some(value) {\n  return { $case: \"some\", value };\n}",
    ),
    (
        "none",
        &[],
        "const none = Object.freeze({ $case: \"none\" });",
    ),
    (
        "codepoints",
        &[],
        "function codepoints(s) {\n  return Array.from(s, (c) => c.codePointAt(0));\n}",
    ),
    (
        "from_codepoints",
        &["trap"],
        "// A value that is not a Unicode scalar value traps (ADR-0040).\n\
         function from_codepoints(points) {\n  let s = \"\";\n  for (const p of points) {\n    \
         if (p < 0 || p > 0x10FFFF || (p >= 0xD800 && p <= 0xDFFF)) trap(\"not a Unicode scalar value\");\n    \
         s += String.fromCodePoint(p);\n  }\n  return s;\n}",
    ),
    (
        "trim",
        &[],
        "// Unicode White_Space, as Rust's `str::trim` and the component's\n\
         // `String.trim`: not ECMAScript's `trim`, which strips U+FEFF and keeps\n\
         // U+0085.\n\
         const WHITE = new Set([0x9, 0xA, 0xB, 0xC, 0xD, 0x20, 0x85, 0xA0, 0x1680, 0x2000, 0x2001,\n  \
         0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A, 0x2028, 0x2029,\n  \
         0x202F, 0x205F, 0x3000]);\n\
         function trim(s) {\n  const cs = Array.from(s);\n  let a = 0, b = cs.length;\n  \
         while (a < b && WHITE.has(cs[a].codePointAt(0))) a++;\n  \
         while (b > a && WHITE.has(cs[b - 1].codePointAt(0))) b--;\n  return cs.slice(a, b).join(\"\");\n}",
    ),
    (
        "range",
        &["trap", "add"],
        "// The whole numbers from `start` up to `end`, as `List.range`; one of\n\
         // more than 2^28 - 1 traps for the component's cause (ADR-0267). Filled\n\
         // to its counted length, as the component writes it.\n\
         function range(start, end) {\n  const n = end > start ? Number(BigInt(end) - BigInt(start)) : 0;\n  \
         if (n > 268435455) trap(\"a value too long for memory\");\n  \
         const out = new Array(n);\n  \
         if (typeof start === \"number\" && start + n <= 9007199254740991) for (let k = 0; k < n; k++) out[k] = start + k;\n  \
         else for (let k = 0; k < n; k++) out[k] = add(start, k);\n  return out;\n}",
    ),
    (
        "swap",
        &[],
        "// The list with the elements at `i` and `j` exchanged, as `List.swap`;\n\
         // the list as it is where either is no place in it.\n\
         function swap(xs, i, j) {\n  \
         if (i < 0 || j < 0 || i >= xs.length || j >= xs.length) return xs;\n  \
         const out = [...xs];\n  [out[i], out[j]] = [out[j], out[i]];\n  return out;\n}",
    ),
    (
        "slice",
        &[],
        "// `start` up to `end`, each clamped to the array, as `List.slice` and\n\
         // `String.slice` (ADR-0055).\n\
         function slice(xs, start, end) {\n  const n = xs.length;\n  \
         const clamp = (i) => (i < 0 ? 0 : i > n ? n : i);\n  const a = clamp(start), b = clamp(end);\n  \
         return b > a ? xs.slice(a, b) : [];\n}",
    ),
    (
        "exact",
        &["trap"],
        "// An Int as JSON carries it (ADR-0058, ADR-0433): a number already within\n\
         // ±2^53, and a trap past it, where it is a BigInt, rather than a\n\
         // different number.\n\
         function exact(v) {\n  \
         if (typeof v === \"bigint\") trap(\"an Int a JavaScript number cannot carry exactly\");\n  \
         return v;\n}",
    ),
    (
        "key_order",
        &["compare"],
        "// The order of two keys (ADR-0057, ADR-0248): an Int by value, a\n\
         // String by code point, false before true, as the component orders\n\
         // them.\n\
         function key_order(a, b) {\n  \
         if (typeof a === \"boolean\") return a === b ? 0 : a ? 1 : -1;\n  \
         return typeof a === \"string\" ? compare(a, b) : a < b ? -1 : a > b ? 1 : 0;\n}",
    ),
    (
        "place",
        &["key_order"],
        "// Where `key` goes among entries sorted by `of`: the first not below it,\n\
         // and whether it is there.\n\
         function place(xs, key, of) {\n  let lo = 0, hi = xs.length;\n  while (lo < hi) {\n    \
         const mid = (lo + hi) >> 1;\n    if (key_order(of(xs[mid]), key) < 0) lo = mid + 1;\n    else hi = mid;\n  }\n  \
         return [lo, lo < xs.length && key_order(of(xs[lo]), key) === 0];\n}",
    ),
    (
        "by_key",
        &["key_order"],
        "// Sorted by key, stably, and of each run of equal keys the last.\n\
         function by_key(xs, of) {\n  const s = [...xs].sort((a, b) => key_order(of(a), of(b)));\n  \
         return s.filter((x, i) => i + 1 === s.length || key_order(of(x), of(s[i + 1])) !== 0);\n}",
    ),
    (
        "checked",
        &["key_order", "trap"],
        "// A map or set from outside, sorted by key as it arrives, stably; a key\n\
         // twice is a trap (ruling 0057-c, ADR-0259).\n\
         function checked(xs, of) {\n  const s = [...xs].sort((a, b) => key_order(of(a), of(b)));\n  \
         for (let i = 1; i < s.length; i++)\n    \
         if (key_order(of(s[i - 1]), of(s[i])) === 0) trap(\"a map or set from outside repeats a key\");\n  \
         return s;\n}",
    ),
    (
        "merged",
        &["key_order"],
        "// Two ascending sets in one pass: what to keep of an element only the\n\
         // first has, only the second has, and both have.\n\
         function merged(a, b, first, second, both) {\n  const out = [];\n  let i = 0, j = 0;\n  \
         while (i < a.length || j < b.length) {\n    \
         const o = j >= b.length ? -1 : i >= a.length ? 1 : key_order(a[i], b[j]);\n    \
         if (o < 0) { if (first) out.push(a[i]); i++; }\n    \
         else if (o > 0) { if (second) out.push(b[j]); j++; }\n    \
         else { if (both) out.push(a[i]); i++; j++; }\n  }\n  return out;\n}",
    ),
    (
        "case_map",
        &[],
        "// Each code point mapped by a case table the component's data segment\n\
         // holds too (ADR-0056): not `toLowerCase`, whose Unicode is the\n\
         // engine's. A table is its ranges and its multi entries, four numbers\n\
         // each, sorted by their first.\n\
         function case_map(s, { ranges, multi }) {\n  \
         // How many entries come before cp: first below it, or not above it.\n  \
         const before = (t, cp, inclusive) => {\n    let lo = 0, hi = t.length / 4;\n    \
         while (lo < hi) {\n      const mid = (lo + hi) >> 1;\n      \
         if (inclusive ? t[mid * 4] <= cp : t[mid * 4] < cp) lo = mid + 1;\n      else hi = mid;\n    }\n    \
         return lo;\n  };\n  let out = \"\";\n  for (const ch of s) {\n    const cp = ch.codePointAt(0);\n    \
         const m = before(multi, cp, false);\n    if (m < multi.length / 4 && multi[m * 4] === cp) {\n      \
         for (let k = 1; k <= 3 && multi[m * 4 + k] !== 0; k++) out += String.fromCodePoint(multi[m * 4 + k]);\n      \
         continue;\n    }\n    const r = before(ranges, cp, true) - 1;\n    \
         const on = r >= 0 && cp <= ranges[r * 4 + 1] && (cp - ranges[r * 4]) % ranges[r * 4 + 3] === 0;\n    \
         out += String.fromCodePoint(on ? cp + ranges[r * 4 + 2] : cp);\n  }\n  return out;\n}",
    ),
    (
        "lower_ascii",
        &[],
        "// `A`-`Z` only (ADR-0040).\n\
         function lower_ascii(s) {\n  return s.replace(/[A-Z]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 32));\n}",
    ),
    (
        "group_by",
        &[],
        "// Runs of adjacent elements with equal keys, as `List.group_by`.\n\
         function group_by(items, key) {\n  const out = [];\n  let last;\n  for (const x of items) {\n    \
         const k = key(x);\n    if (out.length > 0 && k === last) out[out.length - 1].push(x);\n    \
         else out.push([x]);\n    last = k;\n  }\n  return out;\n}",
    ),
];

impl<'p> Emitter<'p> {
    fn new(
        program: &'p Program,
        callees: &'p BTreeMap<(DefId, Vec<Type>), usize>,
        closures: &'p [super::ir::Closure],
    ) -> Emitter<'p> {
        Emitter {
            program,
            callees,
            closures,
            types: BTreeMap::new(),
            helpers: BTreeSet::new(),
            literals: BTreeMap::new(),
            constants: BTreeMap::new(),
            hoisted: BTreeSet::new(),
            handler: false,
            graphs: false,
            outside: false,
            reads: BTreeMap::new(),
            unmade: BTreeSet::new(),
            body: String::new(),
            depth: 1,
        }
    }

    /// The prelude this module's functions call: every helper a used helper
    /// calls, in the table's order, then the case tables used (ADR-0056).
    fn prelude(&self) -> Vec<String> {
        let mut used = self.helpers.clone();
        loop {
            let before = used.len();
            for (name, deps, _) in HELPERS {
                if used.contains(name) {
                    used.extend(deps.iter().copied());
                }
            }
            if used.len() == before {
                break;
            }
        }
        let mut out: Vec<String> = HELPERS
            .iter()
            .filter(|(name, _, _)| used.contains(name))
            .map(|(_, _, source)| source.to_string())
            .collect();
        out.extend(
            [("case_lower", Case::Lower), ("case_upper", Case::Upper)]
                .into_iter()
                .filter(|(name, _)| used.contains(name))
                .map(|(name, c)| case_table(name, c)),
        );
        // Frozen: a list is a value, and no code of the module changes one in
        // place, so one made once serves each call (ADR-0427).
        out.extend(
            self.constants
                .iter()
                .map(|(name, items)| format!("const {name} = Object.freeze([{items}]);")),
        );
        out
    }

    /// A list of literals held as `name`, its elements `items`.
    fn constant(&mut self, name: &str, items: &str) -> Result<(), String> {
        constant_into(&mut self.constants, name, items)
    }

    /// **The body without the literals only a module's list read**
    /// (ADR-0427): a literal's line goes where its name is nowhere else in
    /// the body, as each literal a list was written with is read by it alone.
    fn unread_dropped(&mut self) {
        for v in std::mem::take(&mut self.hoisted) {
            let name = val(v);
            let line = format!("const {name} = ");
            if named(&self.body, &name) != 1 {
                continue;
            }
            let Some(at) = self.body.find(&line) else {
                continue;
            };
            let start = self.body[..at].rfind('\n').map_or(0, |n| n + 1);
            let end = self.body[at..]
                .find('\n')
                .map_or(self.body.len(), |n| at + n + 1);
            self.body.replace_range(start..end, "");
        }
    }

    fn finish(self, component_id: &str, function: &Function, callees: &[String]) -> String {
        let prelude = self.prelude();
        let prelude: Vec<&str> = prelude.iter().map(String::as_str).collect();
        let params: Vec<String> = function.params.iter().map(|(v, _)| val(*v)).collect();
        let mut before: Vec<&str> = prelude;
        before.extend(callees.iter().map(String::as_str));
        format!(
            "// Generated by pw: pure computation `{component_id}` (ADR-0044). Do not edit.\n\n\
             {}{}export function run({}) {{\n{}}}\n",
            before.join("\n\n"),
            if before.is_empty() { "" } else { "\n\n" },
            params.join(", "),
            self.body
        )
    }

    /// **A handler's module** (ADR-0058): its prelude and functions, the names
    /// the runtime reads, and `run(context)`.
    fn finish_handler(self, identity: &str, name: &str, sources: &[String]) -> String {
        let prelude = self.prelude();
        let mut before: Vec<&str> = prelude.iter().map(String::as_str).collect();
        before.extend(sources.iter().map(String::as_str));
        format!(
            "// Generated by pw: resumable handler {identity}. Do not edit.\n\n\
             {}{}export const name = {};\nexport const handler = {};\n\
             export async function run(context) {{\n{}}}\n",
            before.join("\n\n"),
            if before.is_empty() { "" } else { "\n\n" },
            json(name),
            json(identity),
            self.body
        )
    }

    /// **A handler's body** (ADR-0058): each captured path read from the
    /// document and decoded by its type, then the body, its commands awaited.
    fn handler_function(
        &mut self,
        f: &Function,
        paths: &[String],
        event: bool,
    ) -> Result<(), String> {
        let [entry] = f.blocks.as_slice() else {
            return Err(format!("{} blocks; a handler is one", f.blocks.len()));
        };
        let Terminator::Return(result) = &entry.terminator else {
            return Err("its body does not return a value".into());
        };
        for ((v, t), path) in f.params.iter().zip(paths) {
            let at: String = path.split('.').map(|s| format!("[{}]", json(s))).collect();
            let decoded = self.decode(&format!("context.captures{at}"), t)?;
            self.line(&format!("const {} = {decoded};", val(*v)));
            self.types.insert(*v, t.clone());
        }
        // The event the listener read, after the captures (ADR-0138).
        if event {
            let Some((v, t)) = f.params.get(paths.len()) else {
                return Err("a handler given its event has no parameter for it".into());
            };
            let decoded = self.decode("context.event", t)?;
            self.line(&format!("const {} = {decoded};", val(*v)));
            self.types.insert(*v, t.clone());
        }
        self.reads = super::ir::reads(&entry.instrs, *result);
        self.instrs(&entry.instrs)?;
        self.line(&format!("return {};", val(*result)));
        self.unread_dropped();
        self.unmade_unread()
    }

    /// An instance's shape: its declaration's, under its arguments
    /// (ADR-0062).
    fn shape(&self, def: crate::resolve::DefId, args: &[Type]) -> Option<&Shape> {
        self.program
            .types
            .iter()
            .find(|t| t.def == def && t.args == args)
            .map(|t| &t.shape)
    }

    /// **Whether a module holds a value of `ty` as JSON carries it**
    /// (ADR-0433): each `Int` a number within ±2^53 there and here, a
    /// record an object by its field names, a case its name and its payload,
    /// a list an array. A value read from the wire is then itself, as is one
    /// written to it, its `Int`s checked. Not a type that holds `Unit`, which
    /// the wire writes as nothing, nor one that holds a type that contains
    /// itself, which the wire writes as its nodes (ADR-0205).
    fn same_on_wire(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::Str | Type::Bool => true,
            Type::List(t) | Type::Option(t) => self.same_on_wire(t),
            Type::Result(t, e) => self.same_on_wire(t) && self.same_on_wire(e),
            Type::Nominal(def, args) => {
                !contains_itself(self.program, *def, args)
                    && match self.shape(*def, args) {
                        Some(Shape::Alias(of)) => self.same_on_wire(of),
                        Some(Shape::Record { fields }) => {
                            fields.iter().all(|(_, t)| self.same_on_wire(t))
                        }
                        Some(Shape::Variant { cases }) => cases
                            .iter()
                            .all(|(_, ts)| ts.iter().all(|t| self.same_on_wire(t))),
                        None => false,
                    }
            }
            _ => false,
        }
    }

    /// Whether a value of `ty`, one [`Emitter::same_on_wire`] holds, holds an
    /// `Int`, which writing it to the wire checks.
    fn holds_int(&self, ty: &Type) -> bool {
        match ty {
            Type::Int => true,
            Type::List(t) | Type::Option(t) => self.holds_int(t),
            Type::Result(t, e) => self.holds_int(t) || self.holds_int(e),
            Type::Nominal(def, args) => match self.shape(*def, args) {
                Some(Shape::Alias(of)) => self.holds_int(of),
                Some(Shape::Record { fields }) => fields.iter().any(|(_, t)| self.holds_int(t)),
                Some(Shape::Variant { cases }) => cases
                    .iter()
                    .any(|(_, ts)| ts.iter().any(|t| self.holds_int(t))),
                None => false,
            },
            _ => false,
        }
    }

    /// **A value written to the wire as it is, each of its `Int`s checked**
    /// (ADR-0433): `exact` traps on one past ±2^53, a `BigInt`, which no
    /// JavaScript number carries exactly. An expression whose value is the
    /// value itself; nothing is made, so a row a handler left alone is the
    /// very object it read.
    fn checked(&mut self, v: &str, ty: &Type) -> String {
        if !self.holds_int(ty) {
            return v.to_string();
        }
        if let Type::Int = self.unaliased(ty) {
            return format!("{}({v})", self.uses("exact"));
        }
        let mut checks = Vec::new();
        self.int_checks("w", ty, 0, &mut checks);
        format!("((w) => {{ {} return w; }})({v})", checks.join(" "))
    }

    /// `ty` as itself, an alias followed to what it names.
    fn unaliased(&self, ty: &Type) -> Type {
        match ty {
            Type::Nominal(def, args) => match self.shape(*def, args) {
                Some(Shape::Alias(of)) => self.unaliased(&of.clone()),
                _ => ty.clone(),
            },
            _ => ty.clone(),
        }
    }

    /// The statements that check each `Int` of `v`, a value of `ty`, with
    /// `exact`; none where it holds none. `depth` names each loop's element.
    fn int_checks(&mut self, v: &str, ty: &Type, depth: usize, out: &mut Vec<String>) {
        if !self.holds_int(ty) {
            return;
        }
        match ty.clone() {
            Type::Int => {
                let exact = self.uses("exact");
                out.push(format!("{exact}({v});"));
            }
            Type::List(t) => {
                let x = format!("w{depth}");
                let mut each = Vec::new();
                self.int_checks(&x, &t, depth + 1, &mut each);
                out.push(format!("for (const {x} of {v}) {{ {} }}", each.join(" ")));
            }
            Type::Option(t) => {
                let mut some = Vec::new();
                self.int_checks(&format!("{v}.value"), &t, depth, &mut some);
                out.push(format!(
                    "if ({v}.$case === \"some\") {{ {} }}",
                    some.join(" ")
                ));
            }
            Type::Result(t, e) => {
                for (case, payload) in [("ok", t), ("err", e)] {
                    let mut arm = Vec::new();
                    self.int_checks(&format!("{v}.value"), &payload, depth, &mut arm);
                    if !arm.is_empty() {
                        out.push(format!(
                            "if ({v}.$case === \"{case}\") {{ {} }}",
                            arm.join(" ")
                        ));
                    }
                }
            }
            Type::Nominal(def, args) => match self.shape(def, &args).cloned() {
                Some(Shape::Alias(of)) => self.int_checks(v, &of, depth, out),
                Some(Shape::Record { fields }) => {
                    for (n, t) in &fields {
                        self.int_checks(&format!("{v}[{}]", json(n)), t, depth, out);
                    }
                }
                Some(Shape::Variant { cases }) => {
                    for (name, fields) in &cases {
                        let mut arm = Vec::new();
                        match fields.as_slice() {
                            [] => {}
                            [one] => self.int_checks(&format!("{v}.value"), one, depth, &mut arm),
                            many => {
                                for (i, t) in many.iter().enumerate() {
                                    self.int_checks(&format!("{v}.value[{i}]"), t, depth, &mut arm);
                                }
                            }
                        }
                        if !arm.is_empty() {
                            let wire = json(&crate::wit::ident(name));
                            out.push(format!("if ({v}.$case === {wire}) {{ {} }}", arm.join(" ")));
                        }
                    }
                }
                None => {}
            },
            _ => {}
        }
    }

    /// **A captured value, from the JSON the renderer wrote** (ADR-0058). An
    /// `Int` is a JavaScript number there, exact within ±2^53 (ADR-0033 §7),
    /// and here (ADR-0433); a record is an object by field name in both, so
    /// where the type's form is the wire's the value is read as it is.
    ///
    /// `expr` is read as often as the type needs, and not at all for `Unit`:
    /// it names a value, and never does anything.
    fn decode(&self, expr: &str, ty: &Type) -> Result<String, String> {
        // Written out by its shape, a decoder of a type that contains itself
        // would never end (ADR-0194). A signal's value is read as its nodes
        // (ADR-0205); what a host writes, a capture or a command's error, is
        // written nested by what knows no type, and is not read yet.
        if !self.graphs && holds_itself(self.program, ty) {
            return Err(format!(
                "a captured {ty:?}, which holds a type that contains itself: the browser's \
                 wire does not carry one yet"
            ));
        }
        self.decode_in(expr, ty, None)
    }

    /// [`Emitter::decode`], inside the graph of the type `inside` names,
    /// where a value of that type is a node it holds (ADR-0205).
    fn decode_in(&self, expr: &str, ty: &Type, inside: Option<&Graph>) -> Result<String, String> {
        let refused = || format!("a captured {ty:?}, which the document does not carry");
        if !self.outside && self.same_on_wire(ty) {
            return Ok(expr.to_string());
        }
        Ok(match ty {
            Type::Int => expr.to_string(),
            Type::Float | Type::Str | Type::Bool => expr.to_string(),
            // Nothing, whatever the wire holds: a command's `Ok` is answered
            // without its value (ADR-0157).
            Type::Unit => "undefined".to_string(),
            Type::List(t) => format!("{expr}.map((x) => {})", self.decode_in("x", t, inside)?),
            Type::Nominal(def, args) if inside == Some(&(*def, args.clone())) => {
                format!("take({expr})")
            }
            Type::Nominal(def, args) if contains_itself(self.program, *def, args) => {
                self.decode_graph(expr, *def, args)?
            }
            Type::Nominal(def, args) => match self.shape(*def, args) {
                Some(Shape::Alias(of)) => self.decode_in(expr, of, inside)?,
                Some(Shape::Record { fields }) => {
                    let fields: Vec<String> = fields
                        .iter()
                        .map(|(n, t)| {
                            Ok(format!(
                                "{}: {}",
                                json(n),
                                self.decode_in(&format!("o[{}]", json(n)), t, inside)?
                            ))
                        })
                        .collect::<Result<_, String>>()?;
                    format!("((o) => ({{ {} }}))({expr})", fields.join(", "))
                }
                // A case, as the wire holds one (ADR-0130): its name and its
                // payload, one field as the value and several as an array.
                Some(Shape::Variant { cases }) => {
                    let cases = cases.clone();
                    self.decode_cases(expr, &cases, inside)?
                }
                _ => return Err(refused()),
            },
            Type::Option(t) => self.decode_cases(
                expr,
                &[
                    ("some".to_string(), vec![(**t).clone()]),
                    ("none".to_string(), Vec::new()),
                ],
                inside,
            )?,
            Type::Result(t, e) => self.decode_cases(
                expr,
                &[
                    ("ok".to_string(), vec![(**t).clone()]),
                    ("err".to_string(), vec![(**e).clone()]),
                ],
                inside,
            )?,
            _ => return Err(refused()),
        })
    }

    /// **A value of a type that contains itself, read from its nodes**
    /// (ADR-0205): from the last node to the first, each `$node` taking a
    /// later one, held once, so the nodes are a tree and nothing recurses
    /// on the value's depth. A graph that is not a tree traps.
    fn decode_graph(&self, expr: &str, def: DefId, args: &[Type]) -> Result<String, String> {
        let graph = (def, args.to_vec());
        held_back(self.program, def, args)?;
        // The node as the type's shape, its own type's values taken.
        let node = match self.shape(def, args) {
            Some(Shape::Record { fields }) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|(n, t)| {
                        Ok(format!(
                            "{}: {}",
                            json(n),
                            self.decode_in(&format!("o[{}]", json(n)), t, Some(&graph))?
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                format!("((o) => ({{ {} }}))(n)", fields.join(", "))
            }
            Some(Shape::Variant { cases }) => {
                let cases = cases.clone();
                self.decode_cases("n", &cases, Some(&graph))?
            }
            _ => {
                return Err(format!(
                    "a {:?}, whose shape is unknown",
                    Type::Nominal(def, args.to_vec())
                ));
            }
        };
        Ok(format!(
            "((w) => {{ const nodes = w === null || typeof w !== \"object\" ? undefined : w.$graph; \
             if (!Array.isArray(nodes) || nodes.length === 0) throw new Error(\"trap: a `$graph` with no node\"); \
             const built = new Array(nodes.length); const held = new Array(nodes.length).fill(false); \
             let at = nodes.length - 1; \
             const take = (r) => {{ const k = r === null || typeof r !== \"object\" ? undefined : r.$node; \
             if (!Number.isInteger(k) || k <= at || k >= nodes.length || held[k]) throw new Error(\"trap: a `$graph` that is not a tree\"); \
             held[k] = true; return built[k]; }}; \
             for (; at >= 0; at--) {{ const n = nodes[at]; built[at] = {node}; }} \
             for (let k = 1; k < nodes.length; k++) if (!held[k]) throw new Error(\"trap: a `$graph` that is not a tree\"); \
             return built[0]; }})({expr})"
        ))
    }

    /// A case from the wire, by its name, with its payload decoded. A name
    /// the type does not have traps rather than becoming a value of no case.
    fn decode_cases(
        &self,
        expr: &str,
        cases: &[(String, Vec<Type>)],
        inside: Option<&Graph>,
    ) -> Result<String, String> {
        let mut arms = Vec::new();
        for (name, fields) in cases {
            let wire = json(&crate::wit::ident(name));
            let built = match fields.as_slice() {
                [] => format!("{{ $case: {wire} }}"),
                [one] => format!(
                    "{{ $case: {wire}, value: {} }}",
                    self.decode_in("c.value", one, inside)?
                ),
                many => {
                    let each: Vec<String> = many
                        .iter()
                        .enumerate()
                        .map(|(i, t)| self.decode_in(&format!("c.value[{i}]"), t, inside))
                        .collect::<Result<_, String>>()?;
                    format!("{{ $case: {wire}, value: [{}] }}", each.join(", "))
                }
            };
            arms.push(format!("case {wire}: return {built};"));
        }
        Ok(format!(
            "((c) => {{ switch (c.$case) {{ {} default: throw new Error(\"trap: no such case \" + c.$case); }} }})({expr})",
            arms.join(" ")
        ))
    }

    /// **A value as JSON carries it**, whole (ADR-0130): what a signal
    /// holds in the browser. An `Int` as a number, exact within ±2^53 or a
    /// trap; a record as an object by field name; a case as its name and
    /// payload, as `decode` reads it. A value of a type that contains itself
    /// as its nodes (ADR-0205).
    fn wire_value(&mut self, v: &str, ty: &Type) -> Result<String, String> {
        self.wire_in(v, ty, None)
    }

    /// [`Emitter::wire_value`], inside the graph of the type `inside`
    /// names, where a value of that type is a node it holds (ADR-0205).
    fn wire_in(&mut self, v: &str, ty: &Type, inside: Option<&Graph>) -> Result<String, String> {
        // The value itself, its `Int`s checked, where its form is the wire's
        // (ADR-0433).
        if self.same_on_wire(ty) {
            return Ok(self.checked(v, ty));
        }
        Ok(match ty {
            Type::Int => format!("{}({v})", self.uses("exact")),
            Type::Float | Type::Str | Type::Bool => v.to_string(),
            Type::List(t) => format!("{v}.map((x) => {})", self.wire_in("x", t, inside)?),
            Type::Option(t) => {
                let t = (**t).clone();
                self.wire_cases(
                    v,
                    &[("some".into(), vec![t]), ("none".into(), Vec::new())],
                    inside,
                )?
            }
            Type::Result(t, e) => {
                let (t, e) = ((**t).clone(), (**e).clone());
                self.wire_cases(
                    v,
                    &[("ok".into(), vec![t]), ("err".into(), vec![e])],
                    inside,
                )?
            }
            Type::Nominal(def, args) if inside == Some(&(*def, args.clone())) => {
                format!("put({v})")
            }
            Type::Nominal(def, args) if contains_itself(self.program, *def, args) => {
                self.wire_graph(v, *def, args)?
            }
            Type::Nominal(def, args) => match self.shape(*def, args).cloned() {
                Some(Shape::Alias(of)) => self.wire_in(v, &of, inside)?,
                Some(Shape::Record { fields }) => {
                    let mut out = Vec::new();
                    for (n, t) in &fields {
                        out.push(format!(
                            "{}: {}",
                            json(n),
                            self.wire_in(&format!("o[{}]", json(n)), t, inside)?
                        ));
                    }
                    format!("((o) => ({{ {} }}))({v})", out.join(", "))
                }
                Some(Shape::Variant { cases }) => self.wire_cases(v, &cases, inside)?,
                None => return Err(format!("a value of type {ty:?}, whose shape is unknown")),
            },
            other => {
                return Err(format!(
                    "a value of type {other:?}, which JSON cannot carry"
                ));
            }
        })
    }

    /// **A value of a type that contains itself, as its nodes** (ADR-0205):
    /// in level order, each node's own values of the type each the next
    /// node, in the order its fields are declared, so a component's decoder
    /// reads the same nodes a host passes it (ADR-0194). A queue, not
    /// recursion: the value's depth is no stack's business.
    fn wire_graph(&mut self, v: &str, def: DefId, args: &[Type]) -> Result<String, String> {
        let graph = (def, args.to_vec());
        held_back(self.program, def, args)?;
        let node = match self.shape(def, args).cloned() {
            Some(Shape::Record { fields }) => {
                let mut out = Vec::new();
                for (n, t) in &fields {
                    out.push(format!(
                        "{}: {}",
                        json(n),
                        self.wire_in(&format!("o[{}]", json(n)), t, Some(&graph))?
                    ));
                }
                format!("((o) => ({{ {} }}))(n)", out.join(", "))
            }
            Some(Shape::Variant { cases }) => self.wire_cases("n", &cases, Some(&graph))?,
            _ => {
                return Err(format!(
                    "a value of type {:?}, whose shape is unknown",
                    Type::Nominal(def, args.to_vec())
                ));
            }
        };
        Ok(format!(
            "((v) => {{ const queue = [v]; const nodes = []; \
             const put = (x) => {{ queue.push(x); return {{ $node: queue.length - 1 }}; }}; \
             for (let at = 0; at < queue.length; at++) {{ const n = queue[at]; nodes.push({node}); }} \
             return {{ $graph: nodes }}; }})({v})"
        ))
    }

    fn wire_cases(
        &mut self,
        v: &str,
        cases: &[(String, Vec<Type>)],
        inside: Option<&Graph>,
    ) -> Result<String, String> {
        let mut arms = Vec::new();
        for (name, fields) in cases {
            let wire = json(&crate::wit::ident(name));
            let built = match fields.as_slice() {
                [] => format!("{{ $case: {wire} }}"),
                [one] => format!(
                    "{{ $case: {wire}, value: {} }}",
                    self.wire_in("c.value", one, inside)?
                ),
                many => {
                    let mut each = Vec::new();
                    for (i, t) in many.iter().enumerate() {
                        each.push(self.wire_in(&format!("c.value[{i}]"), t, inside)?);
                    }
                    format!("{{ $case: {wire}, value: [{}] }}", each.join(", "))
                }
            };
            arms.push(format!("case {wire}: return {built};"));
        }
        Ok(format!(
            "((c) => {{ switch (c.$case) {{ {} default: throw new Error(\"trap: no such case \" + c.$case); }} }})({v})",
            arms.join(" ")
        ))
    }

    /// **A value sent to a command, as JSON carries it** (ADR-0058): an `Int`
    /// as a number, exact within ±2^53 or a trap; an opaque type as its
    /// representation (ADR-0033 §4); a case as its WIT name and its payload,
    /// `{ $case, value }` (the integrator's, 2026-10-13; W9's finding in
    /// NEXT 6b), as a signal's value is written.
    fn wire(&mut self, v: &str, ty: &Type) -> Result<String, String> {
        match ty {
            Type::Int => Ok(format!("{}({v})", self.uses("exact"))),
            Type::Float | Type::Str | Type::Bool => Ok(v.to_string()),
            // A record or a list, as a signal's value is written (ADR-0172):
            // each field by its Pleris name, each `Int` exact or a trap.
            Type::List(_) => self.wire_value(v, ty),
            // `Option` and `Result`, whose cases are `some` and `none`, `ok`
            // and `err`.
            Type::Option(_) | Type::Result(..) => self.wire_value(v, ty),
            Type::Nominal(def, args) => match self.shape(*def, args).cloned() {
                Some(Shape::Alias(of)) => self.wire(v, &of),
                Some(Shape::Record { .. } | Shape::Variant { .. }) => self.wire_value(v, ty),
                _ => Err(format!(
                    "a command argument of type {ty:?}, which a browser cannot send"
                )),
            },
            other => Err(format!(
                "a command argument of type {other:?}, which a browser cannot send"
            )),
        }
    }

    fn function(&mut self, f: &Function) -> Result<(), String> {
        if !f.capabilities.is_empty() {
            return Err("it reaches the host, so it is not pure computation".into());
        }
        let [entry] = f.blocks.as_slice() else {
            return Err(format!("{} blocks; a pure query is one", f.blocks.len()));
        };
        let Terminator::Return(result) = &entry.terminator else {
            return Err("its body does not return a value".into());
        };
        for (v, t) in &f.params {
            self.types.insert(*v, t.clone());
        }
        self.reads = super::ir::reads(&entry.instrs, *result);
        self.instrs(&entry.instrs)?;
        self.line(&format!("return {};", val(*result)));
        self.unread_dropped();
        self.unmade_unread()
    }

    /// **Refused: a value left unmade that the body names** (ADR-0436).
    /// [`reads`](super::ir::reads) counted it read once, by the match that
    /// took it apart where it was made; a read it did not count would be a
    /// name nothing declares, a `ReferenceError` where the page runs.
    fn unmade_unread(&self) -> Result<(), String> {
        match self
            .unmade
            .iter()
            .find(|v| named(&self.body, &val(**v)) > 0)
        {
            Some(v) => Err(format!("{}, which is never made, is read", val(*v))),
            None => Ok(()),
        }
    }

    /// **A body's instructions, in order** (ADR-0436): an `Option` that
    /// `List.get`, `Map.get` or `List.find` makes, read only by the match
    /// that comes next, is taken apart there and never made: the match is
    /// an `if` on its test, `Some`'s arm reading its payload.
    fn instrs(&mut self, is: &[Instr]) -> Result<(), String> {
        let mut k = 0;
        while let Some(i) = is.get(k) {
            if let Some(
                m @ Instr::Match {
                    scrutinee, arms, ..
                },
            ) = is.get(k + 1)
                && *scrutinee == i.result()
                && self.reads.get(scrutinee) == Some(&1)
                && let Some((some, none)) = option_arms(arms)
                && let Some(o) = self.opened(i)?
            {
                self.taken_apart(i, o, m, some, none)?;
                k += 2;
                continue;
            }
            self.instr(i)?;
            k += 1;
        }
        Ok(())
    }

    /// **An `Option` made** (ADR-0436): its test, and `Some` of its payload
    /// where the test holds.
    fn made(&mut self, i: &Instr, o: Opened) {
        for l in &o.before {
            self.line(l);
        }
        let (some, none) = (self.uses("some"), self.uses("none"));
        self.line(&format!(
            "const {} = {} ? {some}({}) : {none};",
            val(i.result()),
            o.test,
            o.payload
        ));
    }

    /// **An `Option` taken apart where it is made** (ADR-0436): the match
    /// `m` as an `if` on `o`'s test, `Some`'s arm binding its payload, and
    /// the `Option` itself never made.
    fn taken_apart(
        &mut self,
        i: &Instr,
        o: Opened,
        m: &Instr,
        some: &MatchArm,
        none: &MatchArm,
    ) -> Result<(), String> {
        let Type::Option(held) = i.ty() else {
            return Err("an Option opened that is not one".into());
        };
        let r = val(m.result());
        self.types.insert(i.result(), i.ty().clone());
        self.types.insert(m.result(), m.ty().clone());
        for l in &o.before {
            self.line(l);
        }
        self.line(&format!("let {r};"));
        self.line(&format!("if ({}) {{", o.test));
        self.depth += 1;
        if let [Some(b)] = some.bindings.as_slice() {
            self.types.insert(*b, (**held).clone());
            self.line(&format!("const {} = {};", val(*b), o.payload));
        }
        self.region(&some.body, m.result())?;
        self.depth -= 1;
        self.line("} else {");
        self.depth += 1;
        self.region(&none.body, m.result())?;
        self.depth -= 1;
        self.line("}");
        self.unmade.insert(i.result());
        Ok(())
    }

    /// **What `List.get`, `Map.get` and `List.find` make, opened**
    /// (ADR-0436); `None` for any other instruction. `List.find`'s function
    /// is written here, so this is asked once of each instruction.
    fn opened(&mut self, i: &Instr) -> Result<Option<Opened>, String> {
        let r = val(i.result());
        let a: Vec<String> = match i {
            Instr::Intrinsic { args, .. } => args.iter().map(|v| val(*v)).collect(),
            _ => Vec::new(),
        };
        let arg = |k: usize| a.get(k).cloned().unwrap_or_default();
        Ok(Some(match i {
            // A place past 2^53 is a BigInt, and no array's.
            Instr::Intrinsic {
                op: Intrinsic::ListGet,
                ..
            } => {
                let (xs, i) = (arg(0), arg(1));
                Opened {
                    before: Vec::new(),
                    test: format!("{i} >= 0 && {i} < {xs}.length"),
                    payload: format!("{xs}[{i}]"),
                }
            }
            // Maps and sets (ADR-0057): arrays in ascending key order, a
            // map's entries `[key, value]`.
            Instr::Intrinsic {
                op: Intrinsic::MapGet,
                ..
            } => {
                let (m, k) = (arg(0), arg(1));
                let place = self.uses("place");
                Opened {
                    before: vec![format!("const {r}_at = {place}({m}, {k}, (e) => e[0]);")],
                    test: format!("{r}_at[1]"),
                    payload: format!("{m}[{r}_at[0]][1]"),
                }
            }
            Instr::Each {
                kind: EachKind::Find,
                list,
                params,
                body,
                ..
            } => {
                let element = match self.type_of(*list)? {
                    Type::List(t) => (**t).clone(),
                    t => return Err(format!("a list operation over a {t:?}")),
                };
                for p in params {
                    self.types.insert(*p, element.clone());
                }
                let f = self.lambda(params, body)?;
                let xs = val(*list);
                Opened {
                    before: vec![format!("const {r}_at = {xs}.findIndex({f});")],
                    test: format!("{r}_at >= 0"),
                    payload: format!("{xs}[{r}_at]"),
                }
            }
            _ => return Ok(None),
        }))
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.body.push_str("  ");
        }
        self.body.push_str(s);
        self.body.push('\n');
    }

    fn uses(&mut self, helper: &'static str) -> &'static str {
        self.helpers.insert(helper);
        helper
    }

    fn type_of(&self, v: ValueId) -> Result<&Type, String> {
        self.types
            .get(&v)
            .ok_or_else(|| format!("{} has no type", val(v)))
    }

    /// A region's instructions, then its value assigned to `target`.
    fn region(&mut self, r: &Region, target: ValueId) -> Result<(), String> {
        self.instrs(&r.instrs)?;
        self.line(&format!("{} = {};", val(target), val(r.value)));
        Ok(())
    }

    /// A region as an arrow function of `params`.
    fn lambda(&mut self, params: &[ValueId], r: &Region) -> Result<String, String> {
        let outer = std::mem::take(&mut self.body);
        let depth = self.depth;
        self.depth = 1;
        // A function value is not async: no command is awaited in it.
        let handler = std::mem::replace(&mut self.handler, false);
        if let Err(e) = self.instrs(&r.instrs) {
            self.handler = handler;
            return Err(e);
        }
        self.handler = handler;
        self.line(&format!("return {};", val(r.value)));
        let inner = std::mem::replace(&mut self.body, outer);
        self.depth = depth;
        let pad = "  ".repeat(depth);
        let inner: String = inner.lines().map(|l| format!("{pad}{l}\n")).collect();
        let names: Vec<String> = params.iter().map(|p| val(*p)).collect();
        Ok(format!("({}) => {{\n{inner}{pad}}}", names.join(", ")))
    }

    fn fields(
        &self,
        def: crate::resolve::DefId,
        args: &[Type],
    ) -> Result<&[(String, Type)], String> {
        match self.shape(def, args) {
            Some(Shape::Record { fields }) => Ok(fields),
            Some(Shape::Alias(_)) => Ok(&[]),
            Some(Shape::Variant { .. }) => Err("a sum type, which has cases, not fields".into()),
            None => Err("a nominal type with no definition".into()),
        }
    }

    /// A sum type's cases, each with its payload's fields (ADR-0059).
    fn cases(
        &self,
        def: crate::resolve::DefId,
        args: &[Type],
    ) -> Result<&[(String, Vec<Type>)], String> {
        match self.shape(def, args) {
            Some(Shape::Variant { cases }) => Ok(cases),
            _ => Err("a case of a type that is not a sum type".into()),
        }
    }

    /// A case's name as a module value holds it, and its payload's fields'
    /// types, in the scrutinee's type.
    fn case_of(&self, scrutinee: &Type, case: VariantCase) -> Result<(String, Vec<Type>), String> {
        Ok(match (scrutinee, case) {
            (_, VariantCase::Builtin(c)) => {
                let payload = match (scrutinee, c) {
                    (Type::Option(t), BuiltinCase::Some) => vec![(**t).clone()],
                    (Type::Result(t, _), BuiltinCase::Ok) => vec![(**t).clone()],
                    (Type::Result(_, e), BuiltinCase::Err) => vec![(**e).clone()],
                    (Type::Option(_), BuiltinCase::None) => Vec::new(),
                    _ => return Err(format!("`{c:?}` is not a case of a {scrutinee:?}")),
                };
                (case_name(c).to_string(), payload)
            }
            (Type::Nominal(def, args), VariantCase::Declared(i)) => {
                let (name, fields) = self
                    .cases(*def, args)?
                    .get(i as usize)
                    .ok_or("a case past the type's cases")?;
                (crate::wit::ident(name), fields.clone())
            }
            _ => return Err(format!("a declared case of a {scrutinee:?}")),
        })
    }

    fn instr(&mut self, i: &Instr) -> Result<(), String> {
        let r = val(i.result());
        self.types.insert(i.result(), i.ty().clone());
        if let Some(o) = self.opened(i)? {
            self.made(i, o);
            return Ok(());
        }
        match i {
            Instr::Const { value, .. } => {
                let lit = match value {
                    // A number within 2^53 of zero, a BigInt past it (ADR-0433).
                    Const::Int(n) if n.unsigned_abs() <= 1 << 53 => n.to_string(),
                    Const::Int(n) => format!("{n}n"),
                    Const::Float(f) => number(*f),
                    Const::Bool(b) => b.to_string(),
                    Const::Str(s) => serde_json::to_string(s).map_err(|e| e.to_string())?,
                    Const::Unit => "undefined".into(),
                };
                self.line(&format!("const {r} = {lit};"));
                self.literals.insert(i.result(), lit);
            }
            // A function value (ADR-0052): a JavaScript function that calls
            // its code with what it captured, then what it is given.
            Instr::Closure {
                index, captures, ..
            } => {
                let Some(c) = self.closures.get(*index as usize) else {
                    return Err("a function value nothing compiled".into());
                };
                let arity = c.function.params.len() - c.captures;
                let params: Vec<String> = (0..arity).map(|k| format!("a{k}")).collect();
                let mut given: Vec<String> = captures.iter().map(|v| val(*v)).collect();
                given.extend(params.iter().cloned());
                self.line(&format!(
                    "const {r} = ({}) => {}({});",
                    params.join(", "),
                    closure_name(*index as usize),
                    given.join(", ")
                ));
            }
            // A command a handler calls (ADR-0058): awaited through its
            // context, each argument as JSON can carry it, and its answer
            // decoded as its declared result (ADR-0157): `Ok`, or the
            // declared `Err` the handler can show.
            Instr::Command {
                command,
                args,
                ty,
                resend,
                ..
            } => {
                if !self.handler {
                    return Err(format!(
                        "a call to `{command}`, which only a handler's own body makes"
                    ));
                }
                let mut sent = Vec::new();
                for a in args {
                    let t = self.type_of(*a)?.clone();
                    sent.push(self.wire(&val(*a), &t)?);
                }
                // Sent first, on a line of its own: a decoder may not read
                // what it is given at all (`Unit`), and the command is sent
                // whatever its answer's type.
                // And how it is sent again where no answer came (ADR-0173),
                // as the command declares it: the runtime sends it, and only
                // it knows that no answer came.
                let how = match resend {
                    Some(r) => format!(", {{ retry: {{ max: {}, jitter: {} }} }}", r.max, r.jitter),
                    None => String::new(),
                };
                let answered = format!("{r}_answer");
                self.line(&format!(
                    "const {answered} = await context.command({}, [{}]{how});",
                    json(command),
                    sent.join(", ")
                ));
                // From outside the page: read case by case (ADR-0433).
                self.outside = true;
                let decoded = self.decode(&answered, ty);
                self.outside = false;
                let decoded = decoded.map_err(|why| {
                    format!("`{command}`'s answer cannot be read by the page: {why}")
                })?;
                self.line(&format!("const {r} = {decoded};"));
            }
            // **A handler goes to a page once its command commits**
            // (ADR-0280): through its context, the page's route and each
            // parameter's value as text, by name. The runtime fills the route
            // and goes; nothing after it runs (PW5043).
            Instr::Navigate { route, args, .. } => {
                if !self.handler {
                    return Err(format!(
                        "a navigation to `{route}`, which only a handler's own body makes"
                    ));
                }
                let mut given = Vec::new();
                for (name, a) in args {
                    let t = self.type_of(*a)?.clone();
                    // As a command's argument is sent: an opaque value as
                    // its representation, which is text here (PW0621).
                    given.push(format!("{}: {}", json(name), self.wire(&val(*a), &t)?));
                }
                let given = match given.is_empty() {
                    true => "{}".to_string(),
                    false => format!("{{ {} }}", given.join(", ")),
                };
                self.line(&format!(
                    "await context.navigate({}, {given});",
                    json(route)
                ));
                self.line(&format!("const {r} = undefined;"));
            }
            // A page's signal (ADR-0130), through the handler's context, as
            // JSON carries it.
            Instr::SignalGet { signal, ty, .. } => {
                if !self.handler {
                    return Err(format!(
                        "a read of the signal `{signal}`, which only a handler's own body makes"
                    ));
                }
                // A signal's value, which the build or a handler wrote, as
                // its nodes where its type contains itself (ADR-0205).
                self.graphs = true;
                let decoded = self.decode(&format!("context.get({})", json(signal)), ty);
                self.graphs = false;
                let decoded = decoded?;
                self.line(&format!("const {r} = {decoded};"));
            }
            Instr::SignalSet { signal, value, .. } => {
                if !self.handler {
                    return Err(format!(
                        "a change to the signal `{signal}`, which only a handler's own body makes"
                    ));
                }
                let t = self.type_of(*value)?.clone();
                let sent = self.wire_value(&val(*value), &t)?;
                self.line(&format!("context.set({}, {sent});", json(signal)));
                self.line(&format!("const {r} = undefined;"));
            }
            // An opaque type is its representation (ADR-0054).
            Instr::Retype { value, .. } => {
                self.line(&format!("const {r} = {};", val(*value)));
            }
            Instr::Apply { function, args, .. } => {
                let args: Vec<String> = args.iter().map(|a| val(*a)).collect();
                self.line(&format!(
                    "const {r} = {}({});",
                    val(*function),
                    args.join(", ")
                ));
            }
            // `return e` and `?`'s failure (ADR-0051). The placeholder is
            // declared, never read.
            Instr::Return { value, .. } => {
                self.line(&format!("return {};", val(*value)));
                self.line(&format!("let {r};"));
            }
            // A mutable binding: a JavaScript variable (ADR-0051).
            Instr::Local { init, .. } => {
                self.line(&format!("let {r} = {};", val(*init)));
            }
            Instr::Set { local, value, .. } => {
                self.line(&format!("{} = {};", val(*local), val(*value)));
                self.line(&format!("const {r} = undefined;"));
            }
            Instr::Get { local, .. } => {
                self.line(&format!("const {r} = {};", val(*local)));
            }
            // A function compiled beside the query (ADR-0050).
            Instr::Call {
                callee,
                instance,
                args,
                ..
            } => {
                let Some(n) = self.callees.get(&(*callee, instance.clone())) else {
                    return Err(format!("a call to {callee:?}, which nothing compiled"));
                };
                let args: Vec<String> = args.iter().map(|a| val(*a)).collect();
                self.line(&format!(
                    "const {r} = {}({});",
                    callee_name(*n),
                    args.join(", ")
                ));
            }
            Instr::ImportCall { import, .. } => {
                return Err(format!("a host call, `{}`", import.qualified()));
            }
            Instr::Construct { ctor, args, ty, .. } => {
                let instance: &[Type] = match ty {
                    Type::Nominal(_, a) => a,
                    _ => &[],
                };
                let fields = self.fields(*ctor, instance)?;
                let expr = if fields.is_empty() {
                    // An opaque type is its representation.
                    match args.as_slice() {
                        [a] => val(*a),
                        _ => return Err("an opaque construction of several values".into()),
                    }
                } else {
                    let parts: Result<Vec<String>, String> = fields
                        .iter()
                        .zip(args)
                        .map(|((name, _), a)| {
                            Ok(format!(
                                "{}: {}",
                                serde_json::to_string(name).map_err(|e| e.to_string())?,
                                val(*a)
                            ))
                        })
                        .collect();
                    format!("{{ {} }}", parts?.join(", "))
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::Project { of, field, .. } => {
                let Type::Nominal(def, instance) = self.type_of(*of)?.clone() else {
                    return Err("a field read from a value that is not a record".into());
                };
                let name = self
                    .fields(def, &instance)?
                    .get(*field as usize)
                    .map(|(n, _)| n.clone())
                    .ok_or("a field index past the record's fields")?;
                let key = serde_json::to_string(&name).map_err(|e| e.to_string())?;
                self.line(&format!("const {r} = {}[{key}];", val(*of)));
            }
            Instr::Variant { case, payload, .. } => {
                let expr = match (case, payload) {
                    (BuiltinCase::None, _) => self.uses("none").to_string(),
                    (BuiltinCase::Some, Some(p)) => format!("{}({})", self.uses("some"), val(*p)),
                    (c, Some(p)) => {
                        format!("{{ $case: \"{}\", value: {} }}", case_name(*c), val(*p))
                    }
                    (c, None) => format!("{{ $case: \"{}\" }}", case_name(*c)),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            // `Shape.Circle(r)` (ADR-0059): its WIT case's name, and its
            // fields, one as the value, several as an array of them.
            Instr::Case {
                case, fields, ty, ..
            } => {
                let (name, _) = self.case_of(ty, VariantCase::Declared(*case))?;
                let name = json(&name);
                let expr = match fields.as_slice() {
                    [] => format!("{{ $case: {name} }}"),
                    [one] => format!("{{ $case: {name}, value: {} }}", val(*one)),
                    many => format!(
                        "{{ $case: {name}, value: [{}] }}",
                        many.iter().map(|f| val(*f)).collect::<Vec<_>>().join(", ")
                    ),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::Match {
                scrutinee, arms, ..
            } => {
                self.line(&format!("let {r};"));
                self.line(&format!("switch ({}.$case) {{", val(*scrutinee)));
                self.depth += 1;
                let st = self.type_of(*scrutinee)?.clone();
                for arm in arms {
                    let labels: Vec<String> = arm
                        .cases
                        .iter()
                        .map(|c| self.case_of(&st, *c).map(|(n, _)| n))
                        .collect::<Result<_, _>>()?;
                    let Some((last, rest)) = labels.split_last() else {
                        return Err("a match arm that takes no case".into());
                    };
                    for l in rest {
                        self.line(&format!("case {}:", json(l)));
                    }
                    self.line(&format!("case {}: {{", json(last)));
                    self.depth += 1;
                    if let [case] = arm.cases.as_slice() {
                        let (_, types) = self.case_of(&st, *case)?;
                        let many = types.len() > 1;
                        for (k, (b, t)) in arm.bindings.iter().zip(types).enumerate() {
                            let Some(b) = b else { continue };
                            self.types.insert(*b, t);
                            let read = match many {
                                true => format!("{}.value[{k}]", val(*scrutinee)),
                                false => format!("{}.value", val(*scrutinee)),
                            };
                            self.line(&format!("const {} = {read};", val(*b)));
                        }
                    }
                    self.region(&arm.body, i.result())?;
                    self.line("break;");
                    self.depth -= 1;
                    self.line("}");
                }
                let trap = self.uses("trap");
                self.line(&format!("default: {trap}(\"no arm takes this case\");"));
                self.depth -= 1;
                self.line("}");
            }
            Instr::Binary { op, lhs, rhs, .. } => {
                let (a, b) = (val(*lhs), val(*rhs));
                let expr = match (self.type_of(*lhs)?.clone(), op) {
                    (Type::Int, BinaryOp::Add) => format!("{}({a}, {b})", self.uses("add")),
                    (Type::Int, BinaryOp::Sub) => format!("{}({a}, {b})", self.uses("sub")),
                    (Type::Int, BinaryOp::Mul) => format!("{}({a}, {b})", self.uses("mul")),
                    (Type::Int, BinaryOp::Div) => format!("{}({a}, {b})", self.uses("div")),
                    (Type::Int, BinaryOp::Rem) => format!("{}({a}, {b})", self.uses("rem")),
                    (Type::Float, BinaryOp::Rem) => {
                        return Err("`%` on a Float, whose semantics are not decided".into());
                    }
                    (Type::Str, o) if o.compares() && !matches!(o, BinaryOp::Eq | BinaryOp::Ne) => {
                        format!("{}({a}, {b}) {} 0", self.uses("compare"), symbol(*op))
                    }
                    (Type::Int | Type::Float | Type::Bool | Type::Str, o) => {
                        format!("{a} {} {b}", symbol(*o))
                    }
                    (t, o) => return Err(format!("`{o:?}` on a {t:?}")),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::Unary { op, operand, .. } => {
                let a = val(*operand);
                let expr = match (op, self.type_of(*operand)?) {
                    (UnaryOp::Neg, Type::Int) => format!("{}({a})", self.uses("neg")),
                    (UnaryOp::Neg, _) => format!("-{a}"),
                    (UnaryOp::Not, _) => format!("!{a}"),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::If {
                cond, then, els, ..
            } => {
                self.line(&format!("let {r};"));
                self.line(&format!("if ({}) {{", val(*cond)));
                self.depth += 1;
                self.region(then, i.result())?;
                self.depth -= 1;
                self.line("} else {");
                self.depth += 1;
                self.region(els, i.result())?;
                self.depth -= 1;
                self.line("}");
            }
            Instr::Concat { parts, .. } => {
                let ps: Vec<String> = parts.iter().map(|p| val(*p)).collect();
                self.line(&format!("const {r} = \"\" + {};", ps.join(" + ")));
            }
            Instr::Format { value, .. } => {
                let a = val(*value);
                let expr = match self.type_of(*value)? {
                    Type::Int => format!("{a}.toString()"),
                    Type::Bool => format!("String({a})"),
                    Type::Str => a,
                    t => return Err(format!("a {t:?} interpolated")),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::Each {
                kind,
                list,
                seed,
                params,
                body,
                ..
            } => {
                let element = match self.type_of(*list)? {
                    Type::List(t) => (**t).clone(),
                    t => return Err(format!("a list operation over a {t:?}")),
                };
                for (k, p) in params.iter().enumerate() {
                    // A fold's first parameter is its accumulator; every other
                    // parameter is an element.
                    let t = match (kind, k, seed) {
                        (EachKind::Fold, 0, Some(s)) => self.type_of(*s)?.clone(),
                        _ => element.clone(),
                    };
                    self.types.insert(*p, t);
                }
                // `for x in xs` (ADR-0051): a loop in the function's own body,
                // so a `return` in it leaves the function.
                if *kind == EachKind::For {
                    let xs = val(*list);
                    self.line(&format!("for (const {} of {xs}) {{", val(params[0])));
                    self.depth += 1;
                    self.instrs(&body.instrs)?;
                    self.depth -= 1;
                    self.line("}");
                    self.line(&format!("const {r} = undefined;"));
                    return Ok(());
                }
                let f = self.lambda(params, body)?;
                let xs = val(*list);
                let expr = match kind {
                    EachKind::Map => format!("{xs}.map({f})"),
                    EachKind::Filter => format!("{xs}.filter({f})"),
                    EachKind::Any => format!("{xs}.some({f})"),
                    EachKind::All => format!("{xs}.every({f})"),
                    EachKind::Fold => {
                        let s = seed.ok_or("a fold with no seed")?;
                        format!("{xs}.reduce({f}, {})", val(s))
                    }
                    EachKind::Find => unreachable!("an Option is opened above"),
                    // Stable, as the component's merge sort is: `compare(a, b)
                    // > 0` puts `b` first, and a tie keeps the list's order.
                    EachKind::SortBy => {
                        let sign = self.uses("sign");
                        format!("((f) => [...{xs}].sort((a, b) => {sign}(f(a, b))))({f})")
                    }
                    EachKind::GroupBy => format!("{}({xs}, {f})", self.uses("group_by")),
                    EachKind::For => unreachable!("a loop is emitted above"),
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::Intrinsic { op, args, .. } => {
                let a: Vec<String> = args.iter().map(|v| val(*v)).collect();
                let arg = |k: usize| a.get(k).cloned().unwrap_or_default();
                let expr = match op {
                    Intrinsic::ListLength => format!("{}.length", arg(0)),
                    Intrinsic::ListGet | Intrinsic::MapGet => {
                        unreachable!("an Option is opened above")
                    }
                    Intrinsic::ListTake => format!(
                        "{xs}.slice(0, {n} < 0 ? 0 : {n} > {xs}.length ? {xs}.length : {n})",
                        xs = arg(0),
                        n = arg(1)
                    ),
                    Intrinsic::ListDrop => format!(
                        "{}({xs}, {n}, {xs}.length)",
                        self.uses("slice"),
                        xs = arg(0),
                        n = arg(1)
                    ),
                    Intrinsic::ListSlice => {
                        format!("{}({}, {}, {})", self.uses("slice"), arg(0), arg(1), arg(2))
                    }
                    Intrinsic::ListReverse => format!("[...{}].reverse()", arg(0)),
                    Intrinsic::ListRange => {
                        format!("{}({}, {})", self.uses("range"), arg(0), arg(1))
                    }
                    Intrinsic::ListSwap => {
                        format!("{}({}, {}, {})", self.uses("swap"), arg(0), arg(1), arg(2))
                    }
                    Intrinsic::StrSlice => format!(
                        "{}(Array.from({}), {}, {}).join(\"\")",
                        self.uses("slice"),
                        arg(0),
                        arg(1),
                        arg(2)
                    ),
                    Intrinsic::ListConcat => format!("[...{}, ...{}]", arg(0), arg(1)),
                    Intrinsic::StrLength => format!("Array.from({}).length", arg(0)),
                    Intrinsic::StrCodepoints => format!("{}({})", self.uses("codepoints"), arg(0)),
                    Intrinsic::StrFromCodepoints => {
                        format!("{}({})", self.uses("from_codepoints"), arg(0))
                    }
                    Intrinsic::StrStartsWith => format!("{}.startsWith({})", arg(0), arg(1)),
                    Intrinsic::StrEndsWith => format!("{}.endsWith({})", arg(0), arg(1)),
                    Intrinsic::StrContains => format!("{}.includes({})", arg(0), arg(1)),
                    Intrinsic::StrJoin => format!("{}.join({})", arg(0), arg(1)),
                    Intrinsic::StrTrim => format!("{}({})", self.uses("trim"), arg(0)),
                    Intrinsic::StrToLowerAscii => {
                        format!("{}({})", self.uses("lower_ascii"), arg(0))
                    }
                    Intrinsic::StrToLower => format!(
                        "{}({}, {})",
                        self.uses("case_map"),
                        arg(0),
                        self.uses("case_lower")
                    ),
                    Intrinsic::StrToUpper => format!(
                        "{}({}, {})",
                        self.uses("case_map"),
                        arg(0),
                        self.uses("case_upper")
                    ),
                    // `Number(bigint)` is the nearest value, ties to even, as
                    // `f64.convert_i64_s` is.
                    Intrinsic::FloatFromInt => format!("Number({})", arg(0)),
                    // Maps and sets (ADR-0057): arrays in ascending key
                    // order, a map's entries `[key, value]`.
                    Intrinsic::MapEmpty | Intrinsic::SetEmpty => "[]".to_string(),
                    Intrinsic::MapSize | Intrinsic::SetSize => format!("{}.length", arg(0)),
                    Intrinsic::MapContains => {
                        format!(
                            "{}({}, {}, (e) => e[0])[1]",
                            self.uses("place"),
                            arg(0),
                            arg(1)
                        )
                    }
                    Intrinsic::SetContains => {
                        format!(
                            "{}({}, {}, (e) => e)[1]",
                            self.uses("place"),
                            arg(0),
                            arg(1)
                        )
                    }
                    Intrinsic::MapInsert => format!(
                        "((p) => [...{m}.slice(0, p[0]), [{k}, {v}], ...{m}.slice(p[0] + (p[1] ? 1 : 0))])({}({m}, {k}, (e) => e[0]))",
                        self.uses("place"),
                        m = arg(0),
                        k = arg(1),
                        v = arg(2)
                    ),
                    Intrinsic::SetInsert => format!(
                        "((p) => [...{s}.slice(0, p[0]), {x}, ...{s}.slice(p[0] + (p[1] ? 1 : 0))])({}({s}, {x}, (e) => e))",
                        self.uses("place"),
                        s = arg(0),
                        x = arg(1)
                    ),
                    Intrinsic::MapRemove | Intrinsic::SetRemove => format!(
                        "((p) => p[1] ? [...{xs}.slice(0, p[0]), ...{xs}.slice(p[0] + 1)] : {xs})({}({xs}, {k}, {of}))",
                        self.uses("place"),
                        xs = arg(0),
                        k = arg(1),
                        of = if matches!(op, Intrinsic::MapRemove) {
                            "(e) => e[0]"
                        } else {
                            "(e) => e"
                        }
                    ),
                    Intrinsic::MapKeys => format!("{}.map((e) => e[0])", arg(0)),
                    Intrinsic::MapValues => format!("{}.map((e) => e[1])", arg(0)),
                    Intrinsic::MapFromLists => format!(
                        "((ks, vs) => {{ if (ks.length !== vs.length) {}(\"lists of two lengths\"); return {}(ks.map((k, i) => [k, vs[i]]), (e) => e[0]); }})({}, {})",
                        self.uses("trap"),
                        self.uses("by_key"),
                        arg(0),
                        arg(1)
                    ),
                    Intrinsic::SetFromList => {
                        format!("{}({}, (e) => e)", self.uses("by_key"), arg(0))
                    }
                    Intrinsic::SetToList => arg(0),
                    Intrinsic::SetUnion | Intrinsic::SetIntersection | Intrinsic::SetDifference => {
                        let keep = match op {
                            Intrinsic::SetUnion => "true, true, true",
                            Intrinsic::SetIntersection => "false, false, true",
                            _ => "true, false, false",
                        };
                        format!("{}({}, {}, {keep})", self.uses("merged"), arg(0), arg(1))
                    }
                    Intrinsic::MapCheck => {
                        format!("{}({}, (e) => e[0])", self.uses("checked"), arg(0))
                    }
                    Intrinsic::SetCheck => {
                        format!("{}({}, (e) => e)", self.uses("checked"), arg(0))
                    }
                };
                self.line(&format!("const {r} = {expr};"));
            }
            Instr::MakeList { items, .. } => {
                // **A list of literals is a value of the module** (ADR-0427),
                // made once where a function made it at each call: a lookup
                // table in a row's function was made once a row. Named by
                // what it holds, so that two functions' are one.
                let literal: Option<Vec<&str>> = items
                    .iter()
                    .map(|v| self.literals.get(v).map(String::as_str))
                    .collect();
                match literal {
                    Some(xs) if !xs.is_empty() => {
                        let text = xs.join(", ");
                        let name = format!(
                            "list_{:016x}",
                            crate::build::fnv1a(crate::build::FNV_OFFSET, text.as_bytes())
                        );
                        self.constant(&name, &text)?;
                        self.hoisted.extend(items.iter().copied());
                        self.line(&format!("const {r} = {name};"));
                    }
                    _ => {
                        let xs: Vec<String> = items.iter().map(|v| val(*v)).collect();
                        self.line(&format!("const {r} = [{}];", xs.join(", ")));
                    }
                }
            }
        }
        Ok(())
    }
}

fn val(v: ValueId) -> String {
    format!("v{}", v.0)
}

/// `name`, holding `items`, among a module's lists of literals (ADR-0427):
/// two lists of one name are refused, as no module can hold both.
fn constant_into(
    constants: &mut BTreeMap<String, String>,
    name: &str,
    items: &str,
) -> Result<(), String> {
    match constants.get(name) {
        Some(had) if had != items => Err(format!("two lists of literals are both named {name}")),
        Some(_) => Ok(()),
        None => {
            constants.insert(name.to_string(), items.to_string());
            Ok(())
        }
    }
}

/// Every list of literals of `from` among `into`'s, as [`constant_into`].
fn constants_into(
    into: &mut BTreeMap<String, String>,
    from: &BTreeMap<String, String>,
) -> Result<(), String> {
    from.iter()
        .try_for_each(|(name, items)| constant_into(into, name, items))
}

/// A case table as a module constant: the numbers the component's data
/// segment holds, eight entries to a line.
fn case_table(name: &str, c: Case) -> String {
    let t = case::table(c);
    let lines = |words: Vec<[i64; 4]>| -> String {
        words
            .chunks(8)
            .map(|row| {
                let row: Vec<String> = row
                    .iter()
                    .map(|w| w.map(|x| x.to_string()).join(", "))
                    .collect();
                format!("    {},", row.join(", "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let ranges = lines(
        t.ranges
            .iter()
            .map(|r| [r.lo as i64, r.hi as i64, r.delta as i64, r.stride as i64])
            .collect(),
    );
    let multi = lines(
        t.multi
            .iter()
            .map(|m| {
                [
                    m.from as i64,
                    m.to[0] as i64,
                    m.to[1] as i64,
                    m.to[2] as i64,
                ]
            })
            .collect(),
    );
    let (major, minor, update) = char::UNICODE_VERSION;
    format!(
        "// Unicode {major}.{minor}.{update}'s {} case, per code point (ADR-0056).\n\
         const {name} = {{\n  ranges: [\n{ranges}\n  ],\n  multi: [\n{multi}\n  ],\n}};",
        match c {
            Case::Lower => "lower",
            Case::Upper => "upper",
        }
    )
}

/// A float as JavaScript reads it back exactly.
fn number(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        format!("{f:?}")
    }
}

fn symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Eq => "===",
        BinaryOp::Ne => "!==",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
    }
}

fn case_name(c: BuiltinCase) -> &'static str {
    match c {
        BuiltinCase::Some => "some",
        BuiltinCase::None => "none",
        BuiltinCase::Ok => "ok",
        BuiltinCase::Err => "err",
    }
}
