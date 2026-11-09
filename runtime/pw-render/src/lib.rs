//! E7 task 2 — the server renderer.
//!
//! > Given already-checked template IR and values, perfect-web can
//! > deterministically produce correct semantic HTML without Marko.
//!
//! Deliberately narrower than "build the renderer". No resumption, no patches,
//! no document parts, no lazy handlers — those layer on top of a boring,
//! trustworthy HTML serializer, and a serializer that is not boring is not
//! something to layer on.
//!
//! # It does not rediscover meaning
//!
//! The IR arrives with every decision already made: which context a value
//! occupies, which attributes are boolean, which regions are conditional, what
//! a list is keyed by. This crate looks nothing up by name, parses nothing, and
//! guesses nothing. Where the IR does not say, it refuses (see [`Blocked`]).
//!
//! ADR-0018's boundary again: the compiler emits data, the runtime reads it.
//! `pw-core` does not know this crate exists.
//!
//! # Determinism is a property, not an accident
//!
//! Identical IR plus identical values produces identical bytes. Later work
//! depends on it — content hashing, caching, materialization keys, document
//! schemas, resumption identity — so the ordering rules are chosen and written
//! down rather than inherited from a hash map's iteration order.
//!
//! # Nothing invalid disappears
//!
//! A `Blocked` part is an error return, never an empty string. The `Outcome`
//! lesson: a renderer that omits what it did not understand produces a page
//! that looks correct and is missing something, and no test of the page finds
//! it.

pub mod escape;
pub mod identity;
pub mod ir;

pub use identity::{
    Anchor, ElementId, IdentityDomain, InstanceFrame, InstancePath, InstanceToken, LocalPartId,
    PartAddress, Partition, TemplateSchemaId,
};
pub use ir::{
    Arm, Chunk, Context, Part, PartEntry, PartId, Segment, StreamArm, Template, TitlePiece,
};

use std::collections::BTreeMap;
use std::sync::Arc;

/// Why a render produced no bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// The IR carries a part an earlier stage could not represent.
    UnrepresentedConstruct { reason: String, at: String },
    /// A part names a value the environment does not have.
    ///
    /// An error rather than an empty string, for the same reason: a missing
    /// value renders as a page with a hole in it, and the hole looks like
    /// content that happened to be empty.
    MissingValue { path: String },
    /// A `RawHtml` part whose capability the caller did not present.
    UnauthorisedRawHtml { capability: String },
    /// An instance naming a template that is not in the set given.
    UnknownInstance { path: String },
    /// **An instance that would take the page past what a browser's parser
    /// nests** (ADR-0203). Blink's HTML parser nests no more than 512 open
    /// elements, and attaches a deeper one beside its parent, so the document
    /// a browser builds would not be the one rendered, and no part in it
    /// could be found where it was written.
    TooDeep { path: String, elements: u32 },
    /// Two different loop keys derived the same instance token.
    ///
    /// Vanishingly unlikely with a 96-bit keyed derivation, and refused rather
    /// than trusted anyway. Architect ruling, 2026-08-06: *"Do not make
    /// correctness depend purely on probability."* Emitting ambiguous markup
    /// would give two document instances one `PartAddress`, and a patch aimed
    /// at either would reach whichever the runtime indexed last.
    InstanceTokenCollision {
        token: String,
        first: String,
        second: String,
    },
    /// An item has no value for the key its loop declares.
    ///
    /// Its own case, because the repair differs: a duplicate key is two items
    /// claiming one identity, and a missing key is an item with none. Reported
    /// as an empty duplicate would have been the same message for two
    /// different defects.
    MissingLoopKey { each: PartId, field: String },
    /// One `{#each}` rendered two items with the same declared key.
    ///
    /// Diagnosed separately from a token collision, because it is a defect in
    /// the DATA rather than in the derivation and the repair is different: a
    /// key that does not identify is not a key.
    DuplicateLoopKey { each: PartId, key: String },
}

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Blocked::UnrepresentedConstruct { reason, at } => {
                write!(f, "unrepresented construct at `{at}`: {reason}")
            }
            Blocked::MissingValue { path } => write!(f, "no value for `{path}`"),
            Blocked::UnauthorisedRawHtml { capability } => {
                write!(f, "raw HTML requires the `{capability}` capability")
            }
            Blocked::UnknownInstance { path } => write!(f, "no template named `{path}`"),
            Blocked::TooDeep { path, elements } => write!(
                f,
                "an instance of `{path}` nests {elements} elements, past the \
                 {NESTED_ELEMENTS} a page may"
            ),
            Blocked::InstanceTokenCollision {
                token,
                first,
                second,
            } => write!(
                f,
                "two loop keys derived the instance token `{token}`: `{first}` and `{second}`"
            ),
            Blocked::MissingLoopKey { each, field } => write!(
                f,
                "loop part {each} declares the key `{field}` and an item has no value \
                 for it; an item with no identity cannot be addressed"
            ),
            Blocked::DuplicateLoopKey { each, key } => write!(
                f,
                "loop part {each} rendered two items with the key `{key}`; a key that \
                 does not identify is not a key"
            ),
        }
    }
}

/// A value a template can render.
///
/// **As deep as its data, and never on the stack** (ADR-0205): a value of a
/// type that contains itself, a reply thread, is cloned, compared and dropped
/// with a stack on the heap, so its depth is no thread's business. Printed
/// with `{:?}`, it is still read by recursion.
#[derive(Debug)]
pub enum Value {
    Text(String),
    Bool(bool),
    Int(i64),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    /// Bytes to emit verbatim, and the capability that authorises it.
    ///
    /// A separate variant, not a flag on `Text`. Architect ruling: raw HTML
    /// "should require a distinct trusted type/capability rather than being an
    /// option on normal strings". A boolean argument is something a caller can
    /// pass by accident; a different type is not.
    Raw {
        html: String,
        capability: String,
    },
    /// `Some(v)`, `None`, `Ok(v)` or `Err(e)`: a case and its payload, which
    /// `{#match}` takes apart (ADR-0042). A declared sum type's case is named
    /// as its WIT case is, and a payload of several fields is a list of them
    /// (ADR-0061). It has no text form, and it is not a condition: `{#if}` on
    /// one is refused.
    Variant {
        case: String,
        payload: Option<Box<Value>>,
    },
}

impl Value {
    /// **A value as JSON carries it** (ADR-0130): the form the browser's
    /// compiled modules read and write, and a page's signals are held in. A
    /// case is `{ "$case": name, "value": payload }`, its payload a list of
    /// its fields when it has several; a record is an object by field name.
    /// The server renders a signal's first value with this, and the browser's
    /// copy of this renderer each value after it, so the two agree by being
    /// one function.
    ///
    /// **A value of a type that contains itself is its nodes**
    /// (ADR-0205): `{ "$graph": [node, ...] }`, node 0 the value, each value
    /// of the type inside a node written `{ "$node": k }` for a later node
    /// `k`. Each node but the first is held by exactly one, so the value is
    /// a tree. It is built from its last node to its first, without
    /// recursion, so the JSON stays as shallow as the type and the value is
    /// as deep as its data. A graph that is not a tree is refused.
    pub fn from_wire(j: &serde_json::Value) -> Result<Value, String> {
        Value::read(j, &mut None)
    }

    /// One JSON value, inside the graph `nodes` is building, if any: a
    /// `$node` there takes the node it names. Read with a stack on the heap,
    /// children first, so nested JSON is no deeper on the call stack than a
    /// graph is.
    fn read(j: &serde_json::Value, nodes: &mut Option<Nodes>) -> Result<Value, String> {
        use serde_json::Value as J;
        enum Step<'j> {
            Into(&'j J),
            List(usize),
            Record(Vec<&'j String>),
            Case(&'j str, bool),
        }
        let mut steps = vec![Step::Into(j)];
        let mut done: Vec<Value> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Into(j) => match j {
                    J::Null => done.push(Value::Text(String::new())),
                    J::Bool(b) => done.push(Value::Bool(*b)),
                    J::Number(n) => done.push(match n.as_i64() {
                        Some(i) => Value::Int(i),
                        None => Value::Text(n.to_string()),
                    }),
                    J::String(s) => done.push(Value::Text(s.clone())),
                    J::Array(items) => {
                        steps.push(Step::List(items.len()));
                        steps.extend(items.iter().rev().map(Step::Into));
                    }
                    J::Object(o) if o.len() == 1 && o.contains_key("$graph") => {
                        done.push(Value::graph(&o["$graph"])?);
                    }
                    J::Object(o) if o.len() == 1 && o.contains_key("$node") => {
                        let at = o["$node"]
                            .as_u64()
                            .ok_or("a `$node` that is not an index")?;
                        done.push(
                            nodes
                                .as_mut()
                                .ok_or("a `$node` outside any `$graph`")?
                                .take(at)?,
                        );
                    }
                    J::Object(o) => match o.get("$case").and_then(J::as_str) {
                        Some(case) => {
                            let payload = o.get("value");
                            steps.push(Step::Case(case, payload.is_some()));
                            steps.extend(payload.map(Step::Into));
                        }
                        None => {
                            steps.push(Step::Record(o.keys().collect()));
                            steps.extend(o.values().rev().map(Step::Into));
                        }
                    },
                },
                Step::List(n) => {
                    let items = done.split_off(done.len() - n);
                    done.push(Value::List(items));
                }
                Step::Record(keys) => {
                    let values = done.split_off(done.len() - keys.len());
                    done.push(Value::Record(
                        keys.into_iter().cloned().zip(values).collect(),
                    ));
                }
                Step::Case(case, held) => {
                    let payload = match held {
                        true => Some(Box::new(done.pop().expect("its payload, read"))),
                        false => None,
                    };
                    done.push(Value::Variant {
                        case: case.to_string(),
                        payload,
                    });
                }
            }
        }
        Ok(done.pop().expect("the value, read"))
    }

    /// A `$graph`'s value, from its last node to its first.
    fn graph(j: &serde_json::Value) -> Result<Value, String> {
        let list = j
            .as_array()
            .ok_or("a `$graph` that is not a list of nodes")?;
        if list.is_empty() {
            return Err("a `$graph` with no node".into());
        }
        let mut nodes = Some(Nodes {
            built: (0..list.len()).map(|_| None).collect(),
            at: 0,
        });
        for (k, node) in list.iter().enumerate().rev() {
            if let Some(n) = nodes.as_mut() {
                n.at = k;
            }
            let v = Value::read(node, &mut nodes)?;
            nodes.as_mut().expect("building").built[k] = Some(v);
        }
        let mut built = nodes.expect("built").built;
        if built.iter().skip(1).any(Option::is_some) {
            return Err("a `$graph` whose node no other holds".into());
        }
        Ok(built[0].take().expect("the first node"))
    }

    /// Move this value's children onto `out`, leaving it none.
    fn give_children(&mut self, out: &mut Vec<Value>) {
        match self {
            Value::List(items) => out.append(items),
            Value::Record(fields) => out.extend(std::mem::take(fields).into_values()),
            Value::Variant { payload, .. } => {
                if let Some(p) = payload.take() {
                    out.push(*p);
                }
            }
            Value::Text(_) | Value::Bool(_) | Value::Int(_) | Value::Raw { .. } => {}
        }
    }

    /// How this value reads in text or attribute position.
    fn as_str(&self) -> Option<String> {
        Some(match self {
            Value::Text(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
            // A list or a record has no text form. `None` becomes
            // `MissingValue` rather than `"[object Object]"`.
            _ => return None,
        })
    }

    fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Text(s) => !s.is_empty(),
            Value::List(v) => !v.is_empty(),
            Value::Record(_) | Value::Raw { .. } | Value::Variant { .. } => true,
        }
    }

    /// The value as a condition. An `Option` or a `Result` is not one: which
    /// case it is decides a `{#match}`, and `Some(false)` must not read as
    /// true in an `{#if}`.
    fn condition(&self, path: &str) -> Result<bool, Blocked> {
        match self {
            Value::Variant { .. } => Err(Blocked::UnrepresentedConstruct {
                reason: "an Option or a Result is taken apart with `{#match}`, not tested"
                    .to_string(),
                at: path.to_string(),
            }),
            v => Ok(v.truthy()),
        }
    }
}

/// The nodes of a `$graph` being built (ADR-0205): each built and not yet
/// taken, and the one being read.
struct Nodes {
    built: Vec<Option<Value>>,
    at: usize,
}

impl Nodes {
    /// The node `k` names, taken: it must come after the one being read, and
    /// be held once.
    fn take(&mut self, k: u64) -> Result<Value, String> {
        let k = usize::try_from(k).map_err(|_| "a `$node` past the graph")?;
        if k <= self.at || k >= self.built.len() {
            return Err(format!(
                "node {} holds node {k}, which is not a later node of the graph",
                self.at
            ));
        }
        self.built[k]
            .take()
            .ok_or_else(|| format!("node {k} is held twice"))
    }
}

impl Drop for Value {
    /// Taken apart with a stack on the heap, not by recursion (ADR-0205).
    fn drop(&mut self) {
        let mut stack = Vec::new();
        self.give_children(&mut stack);
        while let Some(mut v) = stack.pop() {
            v.give_children(&mut stack);
        }
    }
}

impl Clone for Value {
    /// Copied children first, with a stack on the heap (ADR-0205).
    fn clone(&self) -> Value {
        enum Step<'a> {
            Into(&'a Value),
            Up(&'a Value),
        }
        let mut steps = vec![Step::Into(self)];
        let mut done: Vec<Value> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Into(v) => match v {
                    Value::List(items) => {
                        steps.push(Step::Up(v));
                        steps.extend(items.iter().rev().map(Step::Into));
                    }
                    Value::Record(fields) => {
                        steps.push(Step::Up(v));
                        steps.extend(fields.values().rev().map(Step::Into));
                    }
                    Value::Variant {
                        payload: Some(p), ..
                    } => {
                        steps.push(Step::Up(v));
                        steps.push(Step::Into(p));
                    }
                    Value::Variant {
                        case,
                        payload: None,
                    } => done.push(Value::Variant {
                        case: case.clone(),
                        payload: None,
                    }),
                    Value::Text(s) => done.push(Value::Text(s.clone())),
                    Value::Bool(b) => done.push(Value::Bool(*b)),
                    Value::Int(i) => done.push(Value::Int(*i)),
                    Value::Raw { html, capability } => done.push(Value::Raw {
                        html: html.clone(),
                        capability: capability.clone(),
                    }),
                },
                Step::Up(v) => match v {
                    Value::List(items) => {
                        let copied = done.split_off(done.len() - items.len());
                        done.push(Value::List(copied));
                    }
                    Value::Record(fields) => {
                        let copied = done.split_off(done.len() - fields.len());
                        done.push(Value::Record(fields.keys().cloned().zip(copied).collect()));
                    }
                    Value::Variant { case, .. } => {
                        let p = done.pop().expect("the payload, copied");
                        done.push(Value::Variant {
                            case: case.clone(),
                            payload: Some(Box::new(p)),
                        });
                    }
                    _ => unreachable!("only a value with children is gone up from"),
                },
            }
        }
        done.pop().expect("the value, copied")
    }
}

impl PartialEq for Value {
    /// Compared pair by pair, with a stack on the heap (ADR-0205).
    fn eq(&self, other: &Value) -> bool {
        let mut pairs = vec![(self, other)];
        while let Some(pair) = pairs.pop() {
            match pair {
                (Value::Text(a), Value::Text(b)) if a == b => {}
                (Value::Bool(a), Value::Bool(b)) if a == b => {}
                (Value::Int(a), Value::Int(b)) if a == b => {}
                (
                    Value::Raw {
                        html: a,
                        capability: x,
                    },
                    Value::Raw {
                        html: b,
                        capability: y,
                    },
                ) if a == b && x == y => {}
                (Value::List(a), Value::List(b)) if a.len() == b.len() => {
                    pairs.extend(a.iter().zip(b));
                }
                (Value::Record(a), Value::Record(b))
                    if a.len() == b.len() && a.keys().eq(b.keys()) =>
                {
                    pairs.extend(a.values().zip(b.values()));
                }
                (
                    Value::Variant {
                        case: a,
                        payload: x,
                    },
                    Value::Variant {
                        case: b,
                        payload: y,
                    },
                ) if a == b => match (x, y) {
                    (None, None) => {}
                    (Some(x), Some(y)) => pairs.push((x, y)),
                    _ => return false,
                },
                _ => return false,
            }
        }
        true
    }
}

/// **What a stream's query settled to** (ADR-0148): what it answered, or why
/// it did not, `Some(e)` its declared error and `None` the host's failure.
#[derive(Debug, Clone, PartialEq)]
pub enum Settled {
    Ready(Value),
    Failed(Option<Value>),
}

/// The values a render has, by path.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// **Each binding's value, shared** (ADR-0294): an item's scope is the
    /// page's bindings and the item's, and entering it copies the map of
    /// bindings, not their values. It copied them until ADR-0294, the page's
    /// whole query value with them, once for each item: a list rendered in
    /// its length times the page's size, and kiokun's さえこ, 128 names, took
    /// 218 ms where its values were read in 0.5.
    values: BTreeMap<String, Arc<Value>>,
    /// Capabilities the caller presented, for `RawHtml` parts.
    granted: Arc<Vec<String>>,
    /// Who shares an identity with whom.
    ///
    /// A TYPE, not a free-form salt. It used to be `document: String`, and a
    /// caller could pass a session identifier as the domain of a public
    /// fragment — which would give two readers of one shared cache entry
    /// different bytes. `IdentityDomain` makes that composition unrepresentable
    /// rather than documented; see [`identity`].
    domain: IdentityDomain,
    /// The loop instances enclosing what is being rendered, outermost first.
    ///
    /// `pw_document::InstancePath`, not a local tuple: the renderer builds the
    /// same path a patch will later address, and two representations of it
    /// would be two answers to "which instance".
    path: InstancePath,
    /// Parts whose markup is already materialized, and the domain each was
    /// materialized in.
    ///
    /// A MATERIALIZED FRAGMENT: bytes computed once and reused by every reader
    /// entitled to them. The renderer emits them rather than rendering the
    /// part, which is what makes "shared" mean shared — two readers get the
    /// same bytes because they are the same bytes, not because two renders
    /// happened to agree.
    materialized: Arc<BTreeMap<PartId, String>>,
    /// **What each stream's query settled to** (ADR-0148). A stream with no
    /// entry is pending: a streamed one shows its placeholder, and one the
    /// page waits for is refused, since nothing gave its answer.
    settled: Arc<BTreeMap<PartId, Settled>>,
    /// How many elements enclose the root of the template being rendered,
    /// through the instances around it (ADR-0203).
    elements: u32,
}

impl Env {
    pub fn new() -> Env {
        Env::default()
    }

    pub fn set(mut self, path: &str, value: Value) -> Env {
        self.values.insert(path.to_string(), Arc::new(value));
        self
    }

    /// **The value set at `path`**, as it was set: what a host reads back
    /// of a document it rendered, such as a predicate's answer (ADR-0307).
    pub fn value(&self, path: &str) -> Option<&Value> {
        self.values.get(path).map(|v| &**v)
    }

    /// Present a capability. Without it, a `RawHtml` part is refused.
    pub fn grant(mut self, capability: &str) -> Env {
        Arc::make_mut(&mut self.granted).push(capability.to_string());
        self
    }

    /// Set the identity domain: who shares an identity with whom.
    pub fn in_domain(mut self, domain: IdentityDomain) -> Env {
        self.domain = domain;
        self
    }

    /// Supply a part's markup instead of rendering it.
    ///
    /// The bytes must include the part's own anchors — they are what a full
    /// render would have produced for that part — because a fragment that
    /// omitted them would be unaddressable, and a fragment that had different
    /// ones would be addressable at a name nothing else uses.
    pub fn materialized(mut self, part: PartId, html: &str) -> Env {
        Arc::make_mut(&mut self.materialized).insert(part, html.to_string());
        self
    }

    /// Give a stream what its query settled to (ADR-0148).
    pub fn settle(mut self, part: PartId, outcome: Settled) -> Env {
        Arc::make_mut(&mut self.settled).insert(part, outcome);
        self
    }

    /// The value at `path`: a whole path set directly, or the longest name
    /// bound and each field after it. `item.price.display` is `item`'s
    /// `price`'s `display` (ADR-0169). Until 2026-10-03 one field was read
    /// after the name, and a path two fields deep named nothing.
    ///
    /// At each record, the rest of the path is read whole first: a value a
    /// host computed for a row is set in it by its path from the row,
    /// `quantity.count`, where `quantity` is a number with no fields
    /// (ADR-0170). No field's name holds a `.`, so the two cannot meet.
    fn get(&self, path: &str) -> Option<&Value> {
        if let Some(v) = self.values.get(path) {
            return Some(&**v);
        }
        let segments: Vec<&str> = path.split('.').collect();
        for cut in (1..segments.len()).rev() {
            let Some(mut value) = self.values.get(&segments[..cut].join(".")).map(|v| &**v) else {
                continue;
            };
            let mut rest = &segments[cut..];
            while let [field, after @ ..] = rest {
                let Value::Record(fields) = value else {
                    return None;
                };
                if let Some(whole) = fields.get(&rest.join(".")) {
                    return Some(whole);
                }
                value = fields.get(*field)?;
                rest = after;
            }
            return Some(value);
        }
        None
    }

    fn with(&self, name: &str, value: Value) -> Env {
        let mut next = self.clone();
        next.values.insert(name.to_string(), Arc::new(value));
        next
    }

    /// Enter a repeatable scope.
    fn within(&self, scope: PartId, instance: InstanceToken) -> Env {
        let mut next = self.clone();
        next.path.push(InstanceFrame { scope, instance });
        next
    }

    /// **What an instance of a view renders in** (ADR-0203): its frame on
    /// the path, `elements` deep in the page, with the capabilities and
    /// identity domain of the page around it, and none of its values: it
    /// reads its parameters alone.
    fn instance(&self, scope: PartId, instance: InstanceToken, elements: u32) -> Env {
        let mut path = self.path.clone();
        path.push(InstanceFrame { scope, instance });
        Env {
            values: BTreeMap::new(),
            granted: self.granted.clone(),
            domain: self.domain.clone(),
            path,
            materialized: Arc::default(),
            settled: Arc::default(),
            elements,
        }
    }
}

/// **The most elements a page may nest** (ADR-0203): Blink's HTML parser
/// nests no more than 512 open elements (`kMaximumHTMLParserDOMTreeDepth`,
/// `html_construction_site.h`), and the document's own `<html>` and `<body>`
/// are two of them. Ten more are left for a host's shell around the page.
pub const NESTED_ELEMENTS: u32 = 500;

/// **The page's announcer** (ADR-0302): where the runtime says what a failed
/// press is told, a `role="status"` read by ear and seen by no one. Written
/// into every document that ships the runtime, so it is the same node from
/// the page's first byte (ADR-0182): a live region added with its words is
/// not reliably said. Empty as it is served, so seen by no one; the runtime
/// hides it through the CSSOM before it says anything. A `style` attribute
/// would be one the page's policy allows only by `'unsafe-hashes'` (CSP
/// Level 3, §6.7.3.3), and a policy whose host writes none needs none.
pub const ANNOUNCER: &str = "<div role=\"status\" class=\"pw-announcer\"></div>";

/// **Markup being written, and each instance inside it, written later**
/// (ADR-0203). A view that contains itself goes as deep as its data, and a
/// renderer that went down with each instance would overflow its thread's
/// stack before the page reached what a browser nests: a 2 MiB thread, a
/// server's worker's, held about 250 in a debug build. So an instance is a
/// place in `html`, filled by [`rendered`].
#[derive(Default)]
struct Out<'o> {
    html: String,
    /// Each instance, by where its markup goes in `html`, in order.
    later: Vec<Later<'o>>,
}

/// An instance to render, and where.
struct Later<'o> {
    /// Its offset in the markup around it, between its frame's markers.
    at: usize,
    chunks: &'o [Chunk],
    env: Env,
}

impl std::ops::Deref for Out<'_> {
    type Target = String;
    fn deref(&self) -> &String {
        &self.html
    }
}

impl std::ops::DerefMut for Out<'_> {
    fn deref_mut(&mut self) -> &mut String {
        &mut self.html
    }
}

/// Markup whose instances are not written yet, and how far it is written.
struct Open<'o> {
    html: String,
    later: std::vec::IntoIter<Later<'o>>,
    written: usize,
    /// Where it stopped, if it did: after the instances it holds so far.
    failed: Option<Blocked>,
}

impl<'o> Open<'o> {
    fn new(out: Out<'o>, failed: Option<Blocked>) -> Open<'o> {
        Open {
            html: out.html,
            later: out.later.into_iter(),
            written: 0,
            failed,
        }
    }
}

/// **The markup `first` writes, each instance in it written where it is**
/// (ADR-0203), depth first, so the document is written in order and an
/// error is the first in it. What is open is a stack on the heap, not the
/// call stack, so a page as deep as its data takes the renderer no deeper.
fn rendered<'o>(
    others: &'o [Template],
    first: impl FnOnce(&mut Out<'o>) -> Result<(), Blocked>,
) -> Result<String, Blocked> {
    let mut out = Out::default();
    let failed = first(&mut out).err();
    let mut open = vec![Open::new(out, failed)];
    let mut doc = String::new();
    while let Some(top) = open.last_mut() {
        if let Some(next) = top.later.next() {
            doc.push_str(&top.html[top.written..next.at]);
            top.written = next.at;
            let mut out = Out::default();
            let failed = emit(next.chunks, &next.env, others, &mut out).err();
            open.push(Open::new(out, failed));
            continue;
        }
        let done = open.pop().expect("one is open");
        if let Some(e) = done.failed {
            return Err(e);
        }
        doc.push_str(&done.html[done.written..]);
    }
    Ok(doc)
}

/// **The templates a template's instances reach** (ADR-0203): each view
/// that contains itself it renders an instance of, and each one those
/// reach, once, in the order met. A host sends each one's manifest with the
/// page, so the browser reads an instance's parts in its own template. One
/// not in `others` is left out, and refused where it is rendered.
pub fn instances_reached<'o>(t: &Template, others: &'o [Template]) -> Vec<&'o Template> {
    fn walk<'c>(chunks: &'c [Chunk], out: &mut Vec<&'c str>) {
        for c in chunks {
            let Chunk::Dynamic(p) = c else { continue };
            if let Part::Instance { path, .. } = p {
                out.push(path);
            }
            for inner in p.nested() {
                walk(inner, out);
            }
        }
    }
    let mut reached: Vec<&'o Template> = Vec::new();
    let mut paths = Vec::new();
    walk(&t.chunks, &mut paths);
    let mut i = 0;
    while i < paths.len() {
        if let Some(found) = others.iter().find(|o| o.path == paths[i])
            && !reached.iter().any(|r| r.path == found.path)
        {
            reached.push(found);
            walk(&found.chunks, &mut paths);
        }
        i += 1;
    }
    reached
}

/// Render one template to HTML bytes.
///
/// `others` is every template that may be reached by a `Component` part. A
/// component naming a template not in the set is `Blocked`, not skipped.
/// Render ONE part, anchors included, in this env's domain.
///
/// What a materialized fragment is made of. Rendering it separately from the
/// page is the whole point: the fragment's identity domain is its OWN — a
/// public fragment is public — and a page that rendered it inline would give
/// it the page's domain, so two sessions would get two sets of instance
/// tokens for one shared cache entry.
pub fn render_part(
    t: &Template,
    part: PartId,
    env: &Env,
    others: &[Template],
) -> Result<String, Blocked> {
    let Some(p) = find_part(&t.chunks, part) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("no part {part} in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    rendered(others, |out| emit_part(p, env, others, out))
}

/// **The name a streamed region's range has**, which the patch that fills it
/// names (ADR-0148): `pw-7`.
pub fn stream_name(part: PartId) -> String {
    format!("pw-{part}")
}

/// The arm a stream's query settled to, rendered with its value bound.
fn emit_settled<'o>(
    p: &Part,
    outcome: &Settled,
    env: &Env,
    others: &'o [Template],
    out: &mut Out<'o>,
) -> Result<(), Blocked> {
    let Part::Stream { ready, failed, .. } = p else {
        return Ok(());
    };
    let (arm, value) = match outcome {
        Settled::Ready(v) => (ready, v.clone()),
        Settled::Failed(why) => (
            failed,
            Value::Variant {
                case: if why.is_some() { "some" } else { "none" }.to_string(),
                payload: why.clone().map(Box::new),
            },
        ),
    };
    let scoped = match &arm.binding {
        Some(name) => env.with(name, value),
        None => env.clone(),
    };
    emit(&arm.body, &scoped, others, out)
}

/// **A streamed region's settled arm, as the patch that fills it**
/// (ADR-0148): `<template for="pw-7">..</template><!--/pw-7-->`. A browser
/// with the platform's out-of-order streaming applies it as it parses; the
/// runtime applies it in one without, once the comment after it has arrived
/// (ADR-0223). The parser puts the comment after the template's end tag, so a
/// template followed by it is whole: until then a response that arrives in
/// parts can show a template only part parsed, and its region only part
/// filled. `env` gives the stream what its query settled to, and the
/// document's identity domain.
pub fn settled_patch(
    t: &Template,
    part: PartId,
    env: &Env,
    others: &[Template],
) -> Result<String, Blocked> {
    let Some(p @ Part::Stream { .. }) = find_part(&t.chunks, part) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("part {part} is not a stream in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    let outcome = env.settled.get(&part).ok_or(Blocked::MissingValue {
        path: format!("the answer of stream {part}"),
    })?;
    let inner = rendered(others, |out| emit_settled(p, outcome, env, others, out))?;
    let name = stream_name(part);
    Ok(format!(
        "<template for=\"{name}\">{inner}</template><!--/{name}-->"
    ))
}

fn find_part(chunks: &[Chunk], want: PartId) -> Option<&Part> {
    match chunk_of(chunks, want)? {
        Chunk::Dynamic(p) => Some(p),
        Chunk::Static(_) => None,
    }
}

/// The chunk that is the part numbered `want`, wherever it is in `chunks`.
fn chunk_of(chunks: &[Chunk], want: PartId) -> Option<&Chunk> {
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        if p.id() == Some(want) {
            return Some(c);
        }
        if let Some(found) = p
            .nested()
            .into_iter()
            .find_map(|inner| chunk_of(inner, want))
        {
            return Some(found);
        }
    }
    None
}

/// **The arm a `{#match}` on `value` shows at `env`, and the env its body is
/// rendered in** (ADR-0042): the arm whose case the value is, the case's
/// payload bound to its name, or each field of a case of several to each of
/// its names (ADR-0061). One reading for a render and for what changed
/// inside it.
fn arm_shown<'a>(arms: &'a [Arm], value: &str, env: &Env) -> Result<(&'a Arm, Env), Blocked> {
    let v = env.get(value).ok_or(Blocked::MissingValue {
        path: value.to_string(),
    })?;
    let Value::Variant { case, payload } = v else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: "a `{#match}` takes an Option or a Result apart".to_string(),
            at: value.to_string(),
        });
    };
    let arm =
        arms.iter()
            .find(|a| a.case == *case)
            .ok_or_else(|| Blocked::UnrepresentedConstruct {
                reason: format!("no arm renders `{case}`"),
                at: value.to_string(),
            })?;
    let mut scoped = match (&arm.binding, payload) {
        (Some(name), Some(p)) => env.with(name, (**p).clone()),
        (Some(_), None) => {
            return Err(Blocked::UnrepresentedConstruct {
                reason: format!("`{case}` has no payload to bind"),
                at: value.to_string(),
            });
        }
        (None, _) => env.clone(),
    };
    // A case of several fields: its payload is a list of them, one per name
    // (ADR-0061).
    if !arm.fields.is_empty() {
        let Some(Value::List(parts)) = payload.as_deref() else {
            return Err(Blocked::UnrepresentedConstruct {
                reason: format!("`{case}` has no fields to bind"),
                at: value.to_string(),
            });
        };
        if parts.len() != arm.fields.len() {
            return Err(Blocked::UnrepresentedConstruct {
                reason: format!(
                    "`{case}` has {} field(s), and the arm binds {}",
                    parts.len(),
                    arm.fields.len()
                ),
                at: value.to_string(),
            });
        }
        for (name, part) in arm.fields.iter().zip(parts) {
            scoped = scoped.with(name, part.clone());
        }
    }
    Ok((arm, scoped))
}

/// Render ONE instance of a keyed loop, anchors included.
///
/// What a patch carries when it inserts an item: the markup the server would
/// have produced for that instance in a full render, byte for byte, including
/// its instance boundaries. Producing it here rather than in a patch generator
/// is what keeps one renderer — a second one would drift, and the drift would
/// show up as an inserted item that looks right and cannot be addressed.
///
/// The env must carry the same [`IdentityDomain`] the document was rendered
/// with, or the instance's token will not match the addresses already in the
/// browser's index.
pub fn render_instance(
    t: &Template,
    each: PartId,
    item: &Value,
    env: &Env,
    others: &[Template],
) -> Result<String, Blocked> {
    let Some(part @ Part::Each { .. }) = find_part(&t.chunks, each) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("part {each} is not a keyed loop in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    each_instance(part, item, env, others, &t.path)
}

/// One instance of the keyed loop `part`, rendered in `env`, where the loop
/// is: inside another loop's instance, `env` is scoped to it (ADR-0181).
fn each_instance(
    part: &Part,
    item: &Value,
    env: &Env,
    others: &[Template],
    at: &str,
) -> Result<String, Blocked> {
    let Part::Each {
        id,
        binding,
        key,
        body,
        ..
    } = part
    else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: "only a loop has instances".into(),
            at: at.to_string(),
        });
    };
    let Some(field) = key else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: "an unkeyed loop has no addressable instance to render".into(),
            at: at.to_string(),
        });
    };

    let raw = key_of(item, field);
    if raw.is_empty() {
        return Err(Blocked::MissingLoopKey {
            each: *id,
            field: field.clone(),
        });
    }

    let token = env.domain.instance_token(&env.path, *id, &raw);
    let scoped = env.with(binding, item.clone()).within(*id, token.clone());
    rendered(others, |out| {
        out.push_str(&format!("<!--pw:s{id}@{token}-->"));
        emit(body, &scoped, others, out)?;
        out.push_str(&format!("<!--pw:e{id}@{token}-->"));
        Ok(())
    })
}

/// The instance token an item would get in this template's loop.
///
/// The server needs it to say WHICH instance a patch removes or moves, and
/// deriving it here rather than in the patch generator keeps one derivation.
/// **What changed in one instance of a keyed loop** (ADR-0168).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceChange {
    /// A text part's new text, as `ReplaceText` sets it.
    Text(String),
    /// An attribute's new value as the document writes it, or `None` for a
    /// boolean attribute now absent.
    Attribute { name: String, value: Option<String> },
    /// A keyed loop inside the instance, changed: its own operations, each
    /// inside the instance (ADR-0181). Until 2026-10-04 a list inside a row
    /// rendered the row again.
    List(Vec<ListChange>),
}

/// **One change to a keyed list, as an operation on the document**
/// (ADR-0145, ADR-0181). An instance whose key stayed keeps its nodes: a
/// focus, a scroll, and anything a test marked on it stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListChange {
    /// The instance whose key left the list.
    Remove(InstanceToken),
    /// An item new to the list, rendered, after the instance named.
    InsertAfter { after: InstanceToken, html: String },
    /// An item new to the list with none before it: before the instance
    /// named, the first, or into the list where it is empty.
    InsertBefore {
        before: Option<InstanceToken>,
        html: String,
    },
    /// An item out of place: after the instance named, or to the front.
    Move {
        instance: InstanceToken,
        after: Option<InstanceToken>,
    },
    /// An item whose value changed, each part set where it is.
    Set {
        instance: InstanceToken,
        changes: Vec<(PartId, InstanceChange)>,
    },
}

/// **The changes that take the keyed loop `each` from `old` to `new`**
/// (ADR-0145, ADR-0181), in `env`, where the loop is rendered:
/// - an item whose key left is removed;
/// - a new one is inserted where it now is;
/// - one out of place is moved;
/// - one whose value changed has each part set where it is, a list inside
///   it diffed in turn, or is rendered again where it is when anything else
///   changed.
///
/// One derivation, for a session's list, the shared menu, and a list inside
/// a row of either. It was the development server's, for the top of a page
/// alone, until 2026-10-04.
pub fn list_changes(
    t: &Template,
    each: PartId,
    old: &[Value],
    new: &[Value],
    env: &Env,
    others: &[Template],
) -> Result<Vec<ListChange>, Blocked> {
    let Some(part @ Part::Each { .. }) = find_part(&t.chunks, each) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("part {each} is not a keyed loop in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    each_changes(part, old, new, env, others, &t.path)
}

fn each_changes(
    part: &Part,
    old: &[Value],
    new: &[Value],
    env: &Env,
    others: &[Template],
    at: &str,
) -> Result<Vec<ListChange>, Blocked> {
    let Part::Each {
        id, key: Some(key), ..
    } = part
    else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: "an unkeyed loop has no addressable instance to change".into(),
            at: at.to_string(),
        });
    };
    let token = |v: &Value| env.domain.instance_token(&env.path, *id, &key_of(v, key));
    let render = |v: &Value| each_instance(part, v, env, others, at);
    let wanted: Vec<InstanceToken> = new.iter().map(token).collect();
    let mut out = Vec::new();
    // What the document holds, as each operation leaves it.
    let mut current: Vec<(InstanceToken, Value)> = Vec::new();
    for v in old {
        let t = token(v);
        if wanted.contains(&t) {
            current.push((t, v.clone()));
        } else {
            out.push(ListChange::Remove(t));
        }
    }
    let mut prev: Option<InstanceToken> = None;
    for (i, item) in new.iter().enumerate() {
        let t = wanted[i].clone();
        match current.iter().position(|(c, _)| *c == t) {
            None => {
                let html = render(item)?;
                out.push(match (&prev, current.first()) {
                    (Some(after), _) => ListChange::InsertAfter {
                        after: after.clone(),
                        html,
                    },
                    (None, first) => ListChange::InsertBefore {
                        before: first.map(|(f, _)| f.clone()),
                        html,
                    },
                });
                current.insert(i, (t.clone(), item.clone()));
            }
            Some(found) => {
                if found != i {
                    out.push(ListChange::Move {
                        instance: t.clone(),
                        after: prev.clone(),
                    });
                    let moved = current.remove(found);
                    current.insert(i, moved);
                }
                if current[i].1 != *item {
                    let was = current[i].1.clone();
                    match row_changes(part, &was, item, env, others, at)? {
                        Some(changes) => out.push(ListChange::Set {
                            instance: t.clone(),
                            changes,
                        }),
                        // More than its parts: rendered again, where it is.
                        None => {
                            out.push(ListChange::Remove(t.clone()));
                            let html = render(item)?;
                            out.push(match &prev {
                                Some(after) => ListChange::InsertAfter {
                                    after: after.clone(),
                                    html,
                                },
                                None => ListChange::InsertBefore { before: None, html },
                            });
                        }
                    }
                    current[i].1 = item.clone();
                }
            }
        }
        prev = Some(t);
    }
    Ok(out)
}

/// **The environment inside one instance of the keyed loop `each`**
/// (ADR-0181): the item bound to the loop's name, and the instance's frame
/// on the path, as a render of it has. What a loop inside it is rendered and
/// changed in.
pub fn instance_env(t: &Template, each: PartId, item: &Value, env: &Env) -> Result<Env, Blocked> {
    let Some(Part::Each {
        id,
        binding,
        key: Some(field),
        ..
    }) = find_part(&t.chunks, each)
    else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("part {each} is not a keyed loop in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    let token = env
        .domain
        .instance_token(&env.path, *id, &key_of(item, field));
    Ok(env.with(binding, item.clone()).within(*id, token))
}

/// **What changed in one instance of the keyed loop `each`, from `was` to
/// `now`, part by part** (ADR-0168): each text part's new text, each
/// attribute's new value, what each element's handlers capture, and each
/// keyed list's changes, in the order the template writes them, inside a
/// block that shows the branch it showed too. `None` when anything changed
/// that is not set in place (a block that shows another branch, a view's
/// instance, a raw value, an unkeyed list): the instance is rendered again.
///
/// Until 2026-10-03 a host patched an instance's text parts and nothing
/// else, so an attribute that read a changed field kept its old value: an
/// Add button named "Add Espresso" stayed so after the item was renamed.
pub fn instance_changes(
    t: &Template,
    each: PartId,
    was: &Value,
    now: &Value,
    env: &Env,
    others: &[Template],
) -> Result<Option<Vec<(PartId, InstanceChange)>>, Blocked> {
    let Some(part @ Part::Each { key: Some(_), .. }) = find_part(&t.chunks, each) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("part {each} is not a keyed loop in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    row_changes(part, was, now, env, others, &t.path)
}

/// [`instance_changes`], for the loop `part` itself.
fn row_changes(
    part: &Part,
    was: &Value,
    now: &Value,
    env: &Env,
    others: &[Template],
    at: &str,
) -> Result<Option<Vec<(PartId, InstanceChange)>>, Blocked> {
    let Part::Each {
        id,
        binding,
        key: Some(field),
        body,
        ..
    } = part
    else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: "only a keyed loop's instance changes".into(),
            at: at.to_string(),
        });
    };
    let scoped = |item: &Value| {
        let token = env
            .domain
            .instance_token(&env.path, *id, &key_of(item, field));
        env.with(binding, item.clone()).within(*id, token)
    };
    let (before, after) = (scoped(was), scoped(now));
    // Each element's handlers in the row, whose captures are one attribute
    // of it (ADR-0172), wherever in the row the element is.
    let mut handlers: BTreeMap<ElementId, Vec<&Part>> = BTreeMap::new();
    handlers_within(body, &mut handlers);
    let mut out = Vec::new();
    if !changes_within(body, &before, &after, &handlers, others, at, &mut out)? {
        return Ok(None);
    }
    Ok(Some(out))
}

/// **What changed in the block `block`, from `before` to `after`, part by
/// part** (ADR-0320): while it shows the arm it showed, each part inside it
/// set where it is, as a row's are (ADR-0168, ADR-0178). `None` where it
/// shows another arm, or where anything changed inside it that is not set in
/// place (a view's instance, a raw value, an unkeyed list): the block is
/// rendered again, whole.
pub fn block_changes(
    t: &Template,
    block: PartId,
    before: &Env,
    after: &Env,
    others: &[Template],
) -> Result<Option<Vec<(PartId, InstanceChange)>>, Blocked> {
    let Some(chunk) = chunk_of(&t.chunks, block) else {
        return Err(Blocked::UnrepresentedConstruct {
            reason: format!("no part {block} in `{}`", t.path),
            at: t.path.clone(),
        });
    };
    let chunk = std::slice::from_ref(chunk);
    // Each element's handlers in the block, whose captures are one
    // attribute of it (ADR-0172).
    let mut handlers: BTreeMap<ElementId, Vec<&Part>> = BTreeMap::new();
    handlers_within(chunk, &mut handlers);
    let mut out = Vec::new();
    if !changes_within(chunk, before, after, &handlers, others, &t.path, &mut out)? {
        return Ok(None);
    }
    Ok(Some(out))
}

/// Every event part in `chunks`, and in the blocks inside them, by its
/// element.
fn handlers_within<'t>(chunks: &'t [Chunk], out: &mut BTreeMap<ElementId, Vec<&'t Part>>) {
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        if let Part::Event { owner, .. } = p {
            out.entry(*owner).or_default().push(p);
        }
        for inner in p.nested() {
            handlers_within(inner, out);
        }
    }
}

/// **What changed in `chunks`, each part set where it is** (ADR-0168), into
/// `out`; `false` where the instance must be rendered again. A conditional
/// that decides as it did is looked into, and its branch's parts are set
/// where they are (ADR-0178): until 2026-10-04 any change inside a block
/// rendered the row again, and a renamed item whose Add sat in one lost its
/// button's node. A `{#match}` that shows the arm it showed is too
/// (ADR-0320).
#[allow(clippy::too_many_arguments)]
fn changes_within(
    chunks: &[Chunk],
    before: &Env,
    after: &Env,
    handlers: &BTreeMap<ElementId, Vec<&Part>>,
    others: &[Template],
    at: &str,
    out: &mut Vec<(PartId, InstanceChange)>,
) -> Result<bool, Blocked> {
    let rendered = |c: &Chunk, e: &Env| -> Result<String, Blocked> {
        rendered(others, |out| emit(std::slice::from_ref(c), e, others, out))
    };
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        let text = |e: &Env, value: &String| match e.get(value) {
            Some(Value::Raw { .. }) => Ok(None),
            Some(v) => Ok(v.as_str()),
            None => Err(Blocked::MissingValue {
                path: value.clone(),
            }),
        };
        match p {
            Part::Text { id, value, .. } => match (text(before, value)?, text(after, value)?) {
                (Some(x), Some(y)) => {
                    if x != y {
                        out.push((*id, InstanceChange::Text(y)));
                    }
                }
                _ => {
                    if rendered(c, before)? != rendered(c, after)? {
                        return Ok(false);
                    }
                }
            },
            Part::Attribute { id, .. }
            | Part::BooleanAttribute { id, .. }
            | Part::InterpolatedAttribute { id, .. } => {
                let x = attribute_value(p, before)?;
                let y = attribute_value(p, after)?;
                if x != y
                    && let Some((name, value)) = y
                {
                    out.push((*id, InstanceChange::Attribute { name, value }));
                }
            }
            // **What a handler captures is an attribute of its element**,
            // which the runtime reads when the handler runs (ADR-0172): set
            // where it is, as any attribute is, once for the element's
            // handlers. Until 2026-10-03 it rendered the row again, and a
            // renamed item's Add button lost its nodes.
            Part::Event { id, owner, .. } => {
                let run = handlers.get(owner).map(Vec::as_slice).unwrap_or_default();
                if run.first().and_then(|p| p.id()) != Some(*id) {
                    continue;
                }
                let x = captures_value(run, before)?;
                let y = captures_value(run, after)?;
                if x != y {
                    out.push((
                        *id,
                        InstanceChange::Attribute {
                            name: "data-pw-captures".into(),
                            value: y,
                        },
                    ));
                }
            }
            Part::Conditional {
                value,
                then,
                otherwise,
                ..
            } => {
                let decides = |e: &Env| -> Result<bool, Blocked> {
                    e.get(value)
                        .ok_or(Blocked::MissingValue {
                            path: value.clone(),
                        })?
                        .condition(value)
                };
                let (x, y) = (decides(before)?, decides(after)?);
                if x != y {
                    return Ok(false);
                }
                let branch = if x { then } else { otherwise };
                if !changes_within(branch, before, after, handlers, others, at, out)? {
                    return Ok(false);
                }
            }
            // **A `{#match}` that shows the arm it showed is looked into**, as
            // a conditional is, its names bound to what each side gives them
            // (ADR-0320). Until 2026-10-12 one rendered its region again for
            // any change inside it.
            Part::Match { value, arms, .. } => {
                let (x, was) = arm_shown(arms, value, before)?;
                let (y, now) = arm_shown(arms, value, after)?;
                if x.case != y.case {
                    return Ok(false);
                }
                if !changes_within(&y.body, &was, &now, handlers, others, at, out)? {
                    return Ok(false);
                }
            }
            // A keyed loop inside the row, over a list in its item: diffed
            // where it is, each of its instances kept (ADR-0181).
            Part::Each {
                id,
                collection,
                key: Some(_),
                ..
            } => {
                let list = |e: &Env| match e.get(collection) {
                    Some(Value::List(items)) => Some(items.clone()),
                    _ => None,
                };
                match (list(before), list(after)) {
                    (Some(x), Some(y)) => {
                        if x != y {
                            let changes = each_changes(p, &x, &y, after, others, at)?;
                            if !changes.is_empty() {
                                out.push((*id, InstanceChange::List(changes)));
                            }
                        }
                    }
                    _ => {
                        if rendered(c, before)? != rendered(c, after)? {
                            return Ok(false);
                        }
                    }
                }
            }
            _ => {
                if rendered(c, before)? != rendered(c, after)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

pub fn instance_token_of(each: PartId, item: &Value, key_field: &str, env: &Env) -> InstanceToken {
    env.domain
        .instance_token(&env.path, each, &key_of(item, key_field))
}

/// **An element's key**: the text at `path` from it, empty where there is
/// none. `id` reads the element's `id`, `r.id` its `r`'s `id`, and the empty
/// path the element itself. One derivation for the render, an inserted
/// instance and a patch's token. Until 2026-09-26 each read one field, so a
/// nested key could not be followed (ADR-0073).
fn key_of(item: &Value, path: &str) -> String {
    path.split('.')
        .filter(|s| !s.is_empty())
        .try_fold(item, |v, segment| match v {
            Value::Record(fields) => fields.get(segment),
            _ => None,
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// A JavaScript number holds every integer up to 2^53 exactly. The compiled
/// handler reads its captures with `JSON.parse`, so an integer past that would
/// reach it as a different number.
const EXACT_INTEGER: u64 = 1 << 53;

/// A captured value as the document carries it. Raw HTML is not a value a
/// handler can capture: it is bytes with an authority attached, and neither
/// survives serialization into an attribute.
fn capture_json(v: &Value, name: &str) -> Result<serde_json::Value, Blocked> {
    Ok(match v {
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) if i.unsigned_abs() <= EXACT_INTEGER => serde_json::Value::from(*i),
        Value::Int(i) => {
            return Err(Blocked::UnrepresentedConstruct {
                reason: format!(
                    "{i} is outside ±2^53, so a handler would read it as a different number"
                ),
                at: name.to_string(),
            });
        }
        Value::List(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|i| capture_json(i, name))
                .collect::<Result<_, _>>()?,
        ),
        // Its fields. What a host computed for a row is set in it by a
        // dotted path (ADR-0170), and is the page's to show, not the item's:
        // a handler is given the item.
        Value::Record(fields) => serde_json::Value::Object(
            fields
                .iter()
                .filter(|(k, _)| !k.contains('.'))
                .map(|(k, v)| Ok((k.clone(), capture_json(v, name)?)))
                .collect::<Result<_, Blocked>>()?,
        ),
        Value::Raw { .. } => {
            return Err(Blocked::UnrepresentedConstruct {
                reason: "raw HTML cannot be captured by a handler".into(),
                at: name.to_string(),
            });
        }
        // A compiled handler takes no Option or Result apart (ADR-0033), so
        // there is no encoding it would read.
        Value::Variant { case, .. } => {
            return Err(Blocked::UnrepresentedConstruct {
                reason: format!("`{case}` cannot be captured by a handler"),
                at: name.to_string(),
            });
        }
    })
}

/// The value at a capture path, `item.id`: the longest prefix the environment
/// holds directly, then record fields one segment at a time.
fn value_at<'e>(env: &'e Env, path: &str) -> Option<&'e Value> {
    let segments: Vec<&str> = path.split('.').collect();
    for split in (1..=segments.len()).rev() {
        let Some(mut v) = env.values.get(&segments[..split].join(".")).map(|v| &**v) else {
            continue;
        };
        for field in &segments[split..] {
            match v {
                Value::Record(fields) => v = fields.get(*field)?,
                _ => return None,
            }
        }
        return Some(v);
    }
    None
}

/// `{"item": {"id": ..}}` for the path `item.id`: the nesting the compiled
/// handler reads, `context.captures["item"]["id"]`.
fn insert_at(
    object: &mut serde_json::Map<String, serde_json::Value>,
    path: &str,
    value: serde_json::Value,
) -> Result<(), Blocked> {
    let conflict = || Blocked::UnrepresentedConstruct {
        reason: "one capture path runs through another's value".into(),
        at: path.to_string(),
    };
    let (parents, leaf) = match path.rsplit_once('.') {
        Some((parents, leaf)) => (parents.split('.').collect::<Vec<_>>(), leaf),
        None => (Vec::new(), path),
    };
    let mut at = object;
    for segment in parents {
        at = at
            .entry(segment.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .ok_or_else(conflict)?;
    }
    if at.insert(leaf.to_string(), value).is_some() {
        return Err(conflict());
    }
    Ok(())
}

/// **An attribute part's name, and its value as the document writes it**
/// (ADR-0168): what [`render`] puts between the attribute's quotes, escaped by
/// its context. `None` for a boolean attribute that is absent, and `Ok(None)`
/// for a part that is no attribute. A patch to the attribute carries this
/// value, and the browser reads it with its own parser, so a patched
/// attribute means what a rendered one does.
pub fn attribute_value(p: &Part, env: &Env) -> Result<Option<(String, Option<String>)>, Blocked> {
    let read = |path: &String| {
        env.get(path)
            .ok_or(Blocked::MissingValue { path: path.clone() })
    };
    Ok(Some(match p {
        Part::Attribute {
            name,
            value,
            context,
            ..
        } => {
            let s = read(value)?.as_str().ok_or(Blocked::MissingValue {
                path: value.clone(),
            })?;
            (name.clone(), Some(escaped(&s, *context)))
        }
        Part::BooleanAttribute { name, value, .. } => (
            name.clone(),
            read(value)?.condition(value)?.then(String::new),
        ),
        Part::InterpolatedAttribute {
            name,
            segments,
            context,
            ..
        } => {
            let mut value = String::new();
            for s in segments {
                match s {
                    Segment::Static(t) => value.push_str(t),
                    Segment::Value(path) => {
                        let v = env
                            .get(path)
                            .and_then(Value::as_str)
                            .ok_or(Blocked::MissingValue { path: path.clone() })?;
                        value.push_str(&match context {
                            Context::Url => escape::url_component(&v),
                            Context::Attribute => escape::attribute(&v),
                            other => {
                                return Err(Blocked::UnrepresentedConstruct {
                                    reason: format!(
                                        "a value interpolated into a {other:?} attribute"
                                    ),
                                    at: name.clone(),
                                });
                            }
                        });
                    }
                }
            }
            (name.clone(), Some(value))
        }
        _ => return Ok(None),
    }))
}

pub fn render(t: &Template, env: &Env, others: &[Template]) -> Result<String, Blocked> {
    rendered(others, |out| emit(&t.chunks, env, others, out))
}

/// **A page's metadata, as its head writes it** (ADR-0186): each `<meta>` at
/// the top of the template, in order, its name and its content escaped for
/// an attribute. Empty for a template that states none.
pub fn head_metadata(t: &Template, env: &Env) -> Result<String, Blocked> {
    let mut out = String::new();
    for c in &t.chunks {
        let Chunk::Dynamic(Part::Meta {
            attribute,
            key,
            content,
            ..
        }) = c
        else {
            continue;
        };
        let mut text = String::new();
        for piece in content {
            match piece {
                TitlePiece::Text(t) => text.push_str(t),
                TitlePiece::Value(path) => {
                    let v = env
                        .get(path)
                        .ok_or(Blocked::MissingValue { path: path.clone() })?;
                    if let Value::Raw { .. } = v {
                        return Err(Blocked::UnrepresentedConstruct {
                            reason: "metadata is text, and this value is HTML".to_string(),
                            at: format!("<meta {attribute}=\"{key}\">"),
                        });
                    }
                    let s = v
                        .as_str()
                        .ok_or(Blocked::MissingValue { path: path.clone() })?;
                    text.push_str(&s);
                }
            }
        }
        out.push_str(&format!(
            "<meta {attribute}=\"{}\" content=\"{}\">\n",
            escape::attribute(key),
            escape::attribute(&text)
        ));
    }
    Ok(out)
}

/// **A page's canonical address, as its head writes it** (ADR-0311): the
/// route of the page it names, each `{name}` filled with its value encoded as
/// a link's hole is (`url_component`), absolute with `origin`, the
/// deployment's public origin, never a request's `Host`; and Open Graph's
/// `og:url` from it. Empty for a template that states none. Refused where a
/// template states one and its host has no origin, and for a value no segment
/// can carry (empty, `.` or `..`), as `navigate`'s address refuses one
/// (ADR-0280).
pub fn head_canonical(t: &Template, env: &Env, origin: Option<&str>) -> Result<String, Blocked> {
    let at = || "<link rel=\"canonical\">".to_string();
    let mut out = String::new();
    for c in &t.chunks {
        let Chunk::Dynamic(Part::Canonical { route, args, .. }) = c else {
            continue;
        };
        let Some(origin) = origin else {
            return Err(Blocked::UnrepresentedConstruct {
                reason: "a page that states its canonical address is served with the \
                         deployment's public origin, and this host has none"
                    .to_string(),
                at: at(),
            });
        };
        let mut address = route.clone();
        for (name, piece) in args {
            let text = match piece {
                TitlePiece::Text(t) => t.clone(),
                TitlePiece::Value(path) => {
                    let v = env
                        .get(path)
                        .ok_or(Blocked::MissingValue { path: path.clone() })?;
                    if let Value::Raw { .. } = v {
                        return Err(Blocked::UnrepresentedConstruct {
                            reason: "an address is text, and this value is HTML".to_string(),
                            at: at(),
                        });
                    }
                    v.as_str()
                        .ok_or(Blocked::MissingValue { path: path.clone() })?
                        .to_string()
                }
            };
            if text.is_empty() || text == "." || text == ".." {
                return Err(Blocked::UnrepresentedConstruct {
                    reason: format!("`{text}` is no segment of an address"),
                    at: at(),
                });
            }
            address = address.replace(&format!("{{{name}}}"), &escape::url_component(&text));
        }
        let absolute = format!("{}{address}", origin.trim_end_matches('/'));
        out.push_str(&format!(
            "<link rel=\"canonical\" href=\"{}\">\n<meta property=\"og:url\" content=\"{}\">\n",
            escape::attribute(&absolute),
            escape::attribute(&absolute)
        ));
    }
    Ok(out)
}

/// **A page's title, as text** (ADR-0183): its text, and each value by its
/// path, with its whitespace collapsed and trimmed as a browser's
/// `document.title` reads it. Its host writes it into `<title>`, escaped,
/// and the browser's runtime sets it as `document.title`, so what a page is
/// served with and what a change sets are one text. `None` for a template
/// that states no title.
pub fn title_text(t: &Template, env: &Env) -> Result<Option<String>, Blocked> {
    let Some(pieces) = t.chunks.iter().find_map(|c| match c {
        Chunk::Dynamic(Part::Title { pieces, .. }) => Some(pieces),
        _ => None,
    }) else {
        return Ok(None);
    };
    let mut text = String::new();
    for piece in pieces {
        match piece {
            TitlePiece::Text(t) => text.push_str(t),
            TitlePiece::Value(path) => {
                let v = env
                    .get(path)
                    .ok_or(Blocked::MissingValue { path: path.clone() })?;
                if let Value::Raw { .. } = v {
                    return Err(Blocked::UnrepresentedConstruct {
                        reason: "a title is text, and this value is HTML".to_string(),
                        at: format!("<title>{{{path}}}</title>"),
                    });
                }
                let s = v
                    .as_str()
                    .ok_or(Blocked::MissingValue { path: path.clone() })?;
                text.push_str(&s);
            }
        }
    }
    Ok(Some(
        text.split_ascii_whitespace().collect::<Vec<_>>().join(" "),
    ))
}

fn emit<'o>(
    chunks: &[Chunk],
    env: &Env,
    others: &'o [Template],
    out: &mut Out<'o>,
) -> Result<(), Blocked> {
    let mut i = 0;
    while i < chunks.len() {
        match &chunks[i] {
            Chunk::Static(s) => out.push_str(s),
            // An element's handlers, adjacent in the IR, write one attribute
            // between them (ADR-0138).
            Chunk::Dynamic(p @ Part::Event { owner, .. }) => {
                let mut run = vec![p];
                while let Some(Chunk::Dynamic(q @ Part::Event { owner: o, .. })) =
                    chunks.get(i + run.len())
                    && o == owner
                {
                    run.push(q);
                }
                i += run.len();
                out.push_str(&captures_attribute(&run, env)?);
                continue;
            }
            Chunk::Dynamic(p) => emit_part(p, env, others, out)?,
        }
        i += 1;
    }
    Ok(())
}

/// **What an element's handlers capture, as the one attribute it carries**
/// (ADR-0136, ADR-0138). Each path is read where the template holds it and
/// written where its handler reads it. In one element's scope a name is one
/// value, so the handlers' captures are one object; the same name read from
/// two places is refused, not guessed between. Empty where nothing is
/// captured.
fn captures_attribute(parts: &[&Part], env: &Env) -> Result<String, Blocked> {
    Ok(match captures_value(parts, env)? {
        Some(value) => format!(" data-pw-captures=\"{value}\""),
        None => String::new(),
    })
}

/// **What the handlers of `part`'s element capture, as the document writes
/// it** (ADR-0217), or none where they capture nothing: what a host sets
/// again on an element at the top of the page when a value they read
/// changes, and what a speculation sets in the browser.
pub fn captures_at(t: &Template, part: PartId, env: &Env) -> Result<Option<String>, Blocked> {
    let mut handlers: BTreeMap<ElementId, Vec<&Part>> = BTreeMap::new();
    handlers_within(&t.chunks, &mut handlers);
    let run = handlers
        .values()
        .find(|run| run.iter().any(|p| p.id() == Some(part)))
        .ok_or(Blocked::UnrepresentedConstruct {
            reason: "the part is no handler's".into(),
            at: format!("part {}", part.0),
        })?;
    captures_value(run, env)
}

/// [`captures_at`], given the element's handler parts themselves: what the
/// browser's copy of the renderer is given for a speculation (ADR-0217).
pub fn element_captures(parts: &[Part], env: &Env) -> Result<Option<String>, Blocked> {
    let run: Vec<&Part> = parts.iter().collect();
    captures_value(&run, env)
}

/// [`captures_attribute`]'s value, as the document writes it, or none where
/// nothing is captured.
fn captures_value(parts: &[&Part], env: &Env) -> Result<Option<String>, Blocked> {
    let mut paths: BTreeMap<String, String> = BTreeMap::new();
    for p in parts {
        let Part::Event {
            captures, renames, ..
        } = p
        else {
            continue;
        };
        for path in captures {
            // `item.id` of a view given `entry` is `entry.id` (ADR-0136).
            let at = match path.split_once('.') {
                Some((root, rest)) => renames.get(root).map(|to| format!("{to}.{rest}")),
                None => renames.get(path.as_str()).cloned(),
            }
            .unwrap_or_else(|| path.clone());
            if paths.get(path).is_some_and(|seen| *seen != at) {
                return Err(Blocked::UnrepresentedConstruct {
                    reason: "two handlers on one element capture one name from two places".into(),
                    at: path.clone(),
                });
            }
            paths.insert(path.clone(), at);
        }
    }
    if paths.is_empty() {
        return Ok(None);
    }
    let mut object = serde_json::Map::new();
    for (path, at) in &paths {
        let v = value_at(env, at).ok_or(Blocked::MissingValue { path: at.clone() })?;
        insert_at(&mut object, path, capture_json(v, path)?)?;
    }
    let json = serde_json::Value::Object(object).to_string();
    Ok(Some(escape::attribute(&json)))
}

fn emit_part<'o>(
    p: &Part,
    env: &Env,
    others: &'o [Template],
    out: &mut Out<'o>,
) -> Result<(), Blocked> {
    // A materialized part is EMITTED, not rendered. The bytes were produced by
    // this same renderer in the fragment's own identity domain, and using them
    // verbatim is the difference between a shared fragment and two renders that
    // agree today.
    if let Some(id) = p.id()
        && let Some(html) = env.materialized.get(&id)
    {
        out.push_str(html);
        return Ok(());
    }
    match p {
        Part::Blocked { reason, at } => Err(Blocked::UnrepresentedConstruct {
            reason: reason.clone(),
            at: at.clone(),
        }),

        // Written into the document's head by its host (`title_text`), not
        // where the page writes it (ADR-0183).
        Part::Title { .. } => Ok(()),
        // And its metadata (`head_metadata`, ADR-0186).
        Part::Meta { .. } => Ok(()),
        // And its canonical address (`head_canonical`, ADR-0311).
        Part::Canonical { .. } => Ok(()),

        Part::Text { id, value, context } => {
            let v = env.get(value).ok_or(Blocked::MissingValue {
                path: value.clone(),
            })?;
            // A raw value in text position is still raw — its capability is
            // what authorises it, and reaching it through a `Text` part does
            // not change that.
            if let Value::Raw { html, capability } = v {
                if !env.granted.iter().any(|g| g == capability) {
                    return Err(Blocked::UnauthorisedRawHtml {
                        capability: capability.clone(),
                    });
                }
                out.push_str(html);
                return Ok(());
            }
            let s = v.as_str().ok_or(Blocked::MissingValue {
                path: value.clone(),
            })?;
            // A range anchor, because a text part can be empty and an empty
            // range still needs a place to reappear. An element attribute
            // cannot express "nothing, here".
            out.push_str(&format!("<!--pw:s{id}-->"));
            out.push_str(&escaped(&s, *context));
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }

        // A form control's value is its text (ADR-0221): written between
        // its tags, where HTML reads it, and not as an attribute it does not
        // have.
        Part::Attribute {
            value,
            context: Context::Content,
            ..
        } => {
            let s = env
                .get(value)
                .and_then(Value::as_str)
                .ok_or(Blocked::MissingValue {
                    path: value.clone(),
                })?;
            out.push_str(&escape::content(&s));
            Ok(())
        }
        // Each attribute's value is [`attribute_value`]'s, which a patch to
        // it carries too (ADR-0168).
        Part::Attribute { .. } | Part::InterpolatedAttribute { .. } => {
            if let Some((name, Some(value))) = attribute_value(p, env)? {
                out.push_str(&format!("{name}=\"{value}\""));
            }
            Ok(())
        }

        // False means ABSENT, not empty: `disabled=""` is disabled.
        Part::BooleanAttribute { name, .. } => {
            if let Some((_, Some(_))) = attribute_value(p, env)? {
                out.push_str(name);
            } else if out.ends_with(' ') {
                // Remove the separator the IR emitted for an attribute that
                // turned out not to exist, so the bytes do not carry a trailing
                // space whose presence depends on a value.
                out.pop();
            }
            Ok(())
        }

        // Behaviour, not markup. The browser runtime attaches it after
        // `decide`; the server emits nothing, because the element already
        // carries the `data-pw` that says which element it is.
        // An event handler writes no markup of its own (the runtime attaches
        // it after `decide`) except what it READS of its captures: the paths
        // `pw_core::resume::capture_paths` derived, serialized onto the element
        // so the compiled handler reads them from the document rather than
        // asking the server what the button meant.
        Part::Event { .. } => {
            out.push_str(&captures_attribute(&[p], env)?);
            Ok(())
        }

        // A region showing its query's state (ADR-0148). Pending, a streamed
        // one shows its placeholder inside the range the platform's
        // out-of-order patch replaces, `<?start name>` .. `<?end>`, which a
        // browser without it reads as two comments.
        Part::Stream {
            id,
            streamed,
            placeholder,
            ..
        } => {
            out.push_str(&format!("<!--pw:s{id}-->"));
            match env.settled.get(id) {
                Some(outcome) => emit_settled(p, outcome, env, others, out)?,
                None if *streamed => {
                    out.push_str(&format!("<?start name=\"{}\">", stream_name(*id)));
                    emit(placeholder, env, others, out)?;
                    out.push_str("<?end>");
                }
                None => {
                    return Err(Blocked::MissingValue {
                        path: format!("the answer of stream {id}"),
                    });
                }
            }
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }

        Part::Conditional {
            id,
            value,
            then,
            otherwise,
        } => {
            let v = env.get(value).ok_or(Blocked::MissingValue {
                path: value.clone(),
            })?;
            let branch = if v.condition(value)? { then } else { otherwise };
            out.push_str(&format!("<!--pw:s{id}-->"));
            emit(branch, env, others, out)?;
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }

        Part::Each {
            id,
            collection,
            binding,
            key,
            body,
        } => {
            let v = env.get(collection).ok_or(Blocked::MissingValue {
                path: collection.clone(),
            })?;
            let Value::List(items) = v else {
                return Err(Blocked::MissingValue {
                    path: collection.clone(),
                });
            };
            // Per-loop, per-render: two different keys deriving one token, and
            // two items declaring one key, are both refused here.
            let mut tokens: BTreeMap<String, String> = BTreeMap::new();
            let mut seen: BTreeMap<String, String> = BTreeMap::new();

            // The loop's own range, so the whole list is addressable — an
            // unkeyed list is replaced wholesale, and that is where.
            out.push_str(&format!("<!--pw:s{id}-->"));
            for (i, item) in items.iter().enumerate() {
                let scoped = env.with(binding, item.clone());
                match key {
                    // A KEYED list: each instance gets its own boundaries and
                    // an opaque token, so one instance can be addressed,
                    // reordered or removed without touching its neighbours.
                    Some(field) => {
                        let raw = key_of(item, field);
                        // A declared key that does not identify is not a key.
                        // Both defects are in the DATA rather than in the
                        // derivation, and they are separate because their
                        // repairs are.
                        if raw.is_empty() {
                            return Err(Blocked::MissingLoopKey {
                                each: *id,
                                field: field.clone(),
                            });
                        }
                        if seen.insert(raw.clone(), raw.clone()).is_some() {
                            return Err(Blocked::DuplicateLoopKey {
                                each: *id,
                                key: raw,
                            });
                        }
                        let token = env.domain.instance_token(&env.path, *id, &raw);
                        // Correctness does not rest on probability. A 96-bit
                        // keyed derivation makes this unreachable; refusing
                        // rather than trusting is what makes that a fact about
                        // the renderer instead of about the odds.
                        if let Some(first) = tokens.insert(token.to_string(), raw.clone())
                            && first != raw
                        {
                            return Err(Blocked::InstanceTokenCollision {
                                token: token.to_string(),
                                first,
                                second: raw,
                            });
                        }
                        out.push_str(&format!("<!--pw:s{id}@{token}-->"));
                        emit(body, &scoped.within(*id, token.clone()), others, out)?;
                        out.push_str(&format!("<!--pw:e{id}@{token}-->"));
                    }
                    // An UNKEYED list renders and promises nothing about
                    // per-item identity. No instance boundaries, because an
                    // identity that is really a position is worse than none:
                    // it looks addressable and moves when the list does.
                    None => {
                        let _ = i;
                        emit(body, &scoped, others, out)?;
                    }
                }
            }
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }

        // The arm whose case the value is, its payload bound (ADR-0042). A
        // range, as a conditional is: the runtime replaces it whole.
        Part::Match { id, value, arms } => {
            let (arm, scoped) = arm_shown(arms, value, env)?;
            out.push_str(&format!("<!--pw:s{id}-->"));
            emit(&arm.body, &scoped, others, out)?;
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }

        // Text as written, and each value escaped for the attribute's context:
        // a URI component in a URL, attribute-escaped elsewhere (ADR-0042).
        // A use of a view that contains itself (ADR-0203, ADR-0130's ruling
        // 2): an instance of its template, in a frame of its own, as a loop's
        // row is, so its parts are addressed apart from every other
        // instance's. It reads its parameters alone, from the values its
        // arguments read here. Each takes the page deeper, and none may take
        // it past what a browser's parser nests.
        Part::Instance {
            id,
            path,
            args,
            elements,
            deepest,
        } => {
            let t = others
                .iter()
                .find(|t| t.path == *path)
                .ok_or(Blocked::UnknownInstance { path: path.clone() })?;
            let root = env.elements + elements;
            if root + deepest > NESTED_ELEMENTS {
                return Err(Blocked::TooDeep {
                    path: path.clone(),
                    elements: root + deepest,
                });
            }
            let token = env.domain.instance_token(&env.path, *id, path);
            let mut child = env.instance(*id, token.clone(), root);
            for (param, source) in args {
                let v = env.get(source).ok_or(Blocked::MissingValue {
                    path: source.clone(),
                })?;
                child = child.set(param, v.clone());
            }
            // Its markup is written here by `rendered`, after this
            // template's, so no instance is on the stack of the one around
            // it.
            out.push_str(&format!("<!--pw:s{id}@{token}-->"));
            let at = out.len();
            out.later.push(Later {
                at,
                chunks: &t.chunks,
                env: child,
            });
            out.push_str(&format!("<!--pw:e{id}@{token}-->"));
            Ok(())
        }

        Part::RawHtml {
            id,
            value,
            capability,
        } => {
            if !env.granted.iter().any(|g| g == capability) {
                return Err(Blocked::UnauthorisedRawHtml {
                    capability: capability.clone(),
                });
            }
            let v = env.get(value).ok_or(Blocked::MissingValue {
                path: value.clone(),
            })?;
            out.push_str(&format!("<!--pw:s{id}-->"));
            match v {
                Value::Raw { html, .. } => out.push_str(html),
                other => {
                    let s = other.as_str().ok_or(Blocked::MissingValue {
                        path: value.clone(),
                    })?;
                    out.push_str(&s);
                }
            }
            out.push_str(&format!("<!--pw:e{id}-->"));
            Ok(())
        }
    }
}

/// The one place a context becomes an escaping function.
///
/// A single `match`, so "which escaping did this value get" has one answer that
/// a reader can check, and adding a context without deciding is a compile
/// error rather than a default.
fn escaped(value: &str, context: Context) -> String {
    match context {
        Context::Text => escape::text(value),
        Context::Attribute => escape::attribute(value),
        Context::Url => escape::url(value),
        Context::Style => escape::style(value),
        Context::RawHtml => value.to_string(),
        // As an attribute's, which a patch to it carries (ADR-0221): the
        // document writes it as text, in `emit`.
        Context::Content => escape::attribute(value),
    }
}

#[cfg(test)]
mod scopes {
    use super::*;

    /// **An item's scope shares the page's values** (ADR-0294): entering it
    /// copies the map of bindings, and every value but the item's is the one
    /// the page holds, by pointer, as are the page's fragments and streams.
    #[test]
    fn an_items_scope_shares_the_pages_values() {
        let names = Value::List((0..128).map(|i| Value::Text(format!("n{i}"))).collect());
        let page = Env::new()
            .set("names", names)
            .materialized(PartId(9), "<p>shared</p>")
            .settle(PartId(8), Settled::Ready(Value::Int(1)));
        let item = page.with("name", Value::Text("n0".into()));
        assert!(Arc::ptr_eq(&page.values["names"], &item.values["names"]));
        assert!(Arc::ptr_eq(&page.materialized, &item.materialized));
        assert!(Arc::ptr_eq(&page.settled, &item.settled));
        assert!(Arc::ptr_eq(&page.granted, &item.granted));
        // The item's own value is its own, and the page does not see it.
        assert_eq!(item.get("name"), Some(&Value::Text("n0".into())));
        assert_eq!(page.get("name"), None);
    }
}
