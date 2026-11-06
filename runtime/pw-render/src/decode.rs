//! **The template IR, read without serde's derived visitors** (the first
//! press's ADR, "Next" 1): how the browser's renderer reads a request's part
//! and the templates of the views it renders.
//!
//! The browser's renderer is the server's (ADR-0130), built for
//! `wasm32-unknown-unknown`. Until 2026-11-06 it read the IR with the
//! `Deserialize` that serde derives for [`crate::ir`], whose generic
//! visitors, the internally tagged [`Part`]'s above all, were 54% of the
//! module's code: 199 KB of 367 KB, attributed by its name section. Every
//! page that renders in the browser downloaded them.
//!
//! This reads the same JSON into the same types as the derives do: the same
//! tags (`part`, adjacently `chunk`/`value`, `segment`/`value`,
//! `piece`/`value`), the same names, the same defaults (`#[serde(default)]`,
//! an `Option` absent or `null`), unknown fields ignored, and a missing
//! required field, an unknown tag or a value of another type refused. The
//! derives stay, for every native reader of the IR. `tests/decode.rs` holds
//! the two to one answer: over the compiler's own IR, over every part kind
//! with each optional field present and absent, and over malformed inputs,
//! which both refuse.
//!
//! What it does not copy is serde's wording: a refusal here says what was
//! refused in its own words, and a caller reports it as a request that is
//! not one (`pw-render-wasm`'s code 2), as it reported serde's. Nor two
//! leniencies: serde's derives also read a struct, or a tagged variant,
//! written as a list of its fields in order (`["text", 3, "greeting",
//! "text"]`), and an enum's unit variant written as a one-key map
//! (`{"text": null}`) as well as by its name. The compiler writes objects and
//! names (its IR is serialized by the same derives), and this refuses both
//! forms (`tests/decode.rs`, `serdes_two_leniencies_are_refused_here`). So
//! this reads nothing serde refuses, and reads everything the compiler
//! writes as serde does.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::ir::{
    Arm, Chunk, Context, ElementId, Part, PartId, Segment, StreamArm, Template, TitlePiece,
};

type Read<T> = Result<T, String>;
type Object = Map<String, Value>;

/// One part, as `serde_json::from_value::<Part>` reads it.
pub fn part(v: &Value) -> Read<Part> {
    let o = object(v, "a part")?;
    let tag = o
        .get("part")
        .ok_or_else(|| "a part names no `part`".to_string())?;
    let tag = tag
        .as_str()
        .ok_or_else(|| "a part's `part` is not a string".to_string())?;
    Ok(match tag {
        "text" => Part::Text {
            id: id(o)?,
            value: string(o, "value")?,
            context: context(o, "context")?,
        },
        "attribute" => Part::Attribute {
            id: id(o)?,
            owner: owner(o)?,
            name: string(o, "name")?,
            value: string(o, "value")?,
            context: context(o, "context")?,
        },
        "boolean_attribute" => Part::BooleanAttribute {
            id: id(o)?,
            owner: owner(o)?,
            name: string(o, "name")?,
            value: string(o, "value")?,
        },
        "event" => Part::Event {
            id: id(o)?,
            owner: owner(o)?,
            event: string(o, "event")?,
            handler: string(o, "handler")?,
            name: defaulted(o, "name", |v| text(v, "name"))?,
            captures: defaulted(o, "captures", |v| strings(v, "captures"))?,
            renames: defaulted(o, "renames", |v| string_map(v, "renames"))?,
            modifiers: defaulted(o, "modifiers", |v| strings(v, "modifiers"))?,
        },
        "conditional" => Part::Conditional {
            id: id(o)?,
            value: string(o, "value")?,
            then: chunks(field(o, "then")?)?,
            otherwise: chunks(field(o, "otherwise")?)?,
        },
        "each" => Part::Each {
            id: id(o)?,
            collection: string(o, "collection")?,
            binding: string(o, "binding")?,
            key: optional(o, "key", |v| text(v, "key"))?,
            body: chunks(field(o, "body")?)?,
        },
        "match" => Part::Match {
            id: id(o)?,
            value: string(o, "value")?,
            arms: list(field(o, "arms")?, "arms", arm)?,
        },
        "stream" => Part::Stream {
            id: id(o)?,
            query: string(o, "query")?,
            args: strings(field(o, "args")?, "args")?,
            streamed: boolean(field(o, "streamed")?, "streamed")?,
            placeholder: chunks(field(o, "placeholder")?)?,
            ready: stream_arm(field(o, "ready")?)?,
            failed: stream_arm(field(o, "failed")?)?,
        },
        "interpolated_attribute" => Part::InterpolatedAttribute {
            id: id(o)?,
            owner: owner(o)?,
            name: string(o, "name")?,
            segments: list(field(o, "segments")?, "segments", segment)?,
            context: context(o, "context")?,
        },
        "instance" => Part::Instance {
            id: id(o)?,
            path: string(o, "path")?,
            args: list(field(o, "args")?, "args", |v| {
                let (a, b) = pair(v, "args")?;
                Ok((text(a, "args")?, text(b, "args")?))
            })?,
            elements: defaulted(o, "elements", |v| number(v, "elements"))?,
            deepest: defaulted(o, "deepest", |v| number(v, "deepest"))?,
        },
        "title" => Part::Title {
            id: id(o)?,
            pieces: list(field(o, "pieces")?, "pieces", title_piece)?,
        },
        "meta" => Part::Meta {
            id: id(o)?,
            attribute: string(o, "attribute")?,
            key: string(o, "key")?,
            content: list(field(o, "content")?, "content", title_piece)?,
        },
        "canonical" => Part::Canonical {
            id: id(o)?,
            route: string(o, "route")?,
            args: list(field(o, "args")?, "args", |v| {
                let (a, b) = pair(v, "args")?;
                Ok((text(a, "args")?, title_piece(b)?))
            })?,
        },
        "raw_html" => Part::RawHtml {
            id: id(o)?,
            value: string(o, "value")?,
            capability: string(o, "capability")?,
        },
        "blocked" => Part::Blocked {
            reason: string(o, "reason")?,
            at: string(o, "at")?,
        },
        other => return Err(format!("no part is a `{other}`")),
    })
}

/// A list of parts, as `serde_json::from_value::<Vec<Part>>` reads it.
pub fn parts(v: &Value) -> Read<Vec<Part>> {
    list(v, "parts", part)
}

/// One template, as `serde_json::from_value::<Template>` reads it.
pub fn template(v: &Value) -> Read<Template> {
    let o = object(v, "a template")?;
    Ok(Template {
        path: string(o, "path")?,
        name: string(o, "name")?,
        params: strings(field(o, "params")?, "params")?,
        schema: string(o, "schema")?,
        chunks: chunks(field(o, "chunks")?)?,
    })
}

/// A list of templates, as `serde_json::from_value::<Vec<Template>>` reads it.
pub fn templates(v: &Value) -> Read<Vec<Template>> {
    list(v, "templates", template)
}

/// A list of chunks: each `{"chunk": "static", "value": "<b>"}` or
/// `{"chunk": "dynamic", "value": <a part>}`.
pub fn chunks(v: &Value) -> Read<Vec<Chunk>> {
    list(v, "chunks", chunk)
}

fn chunk(v: &Value) -> Read<Chunk> {
    let (tag, value) = adjacent(v, "chunk")?;
    match tag {
        "static" => Ok(Chunk::Static(text(value, "a static chunk")?)),
        "dynamic" => Ok(Chunk::Dynamic(part(value)?)),
        other => Err(format!("no chunk is a `{other}`")),
    }
}

fn segment(v: &Value) -> Read<Segment> {
    let (tag, value) = adjacent(v, "segment")?;
    let value = text(value, "a segment")?;
    match tag {
        "static" => Ok(Segment::Static(value)),
        "value" => Ok(Segment::Value(value)),
        other => Err(format!("no segment is a `{other}`")),
    }
}

fn title_piece(v: &Value) -> Read<TitlePiece> {
    let (tag, value) = adjacent(v, "piece")?;
    let value = text(value, "a piece")?;
    match tag {
        "text" => Ok(TitlePiece::Text(value)),
        "value" => Ok(TitlePiece::Value(value)),
        other => Err(format!("no piece is a `{other}`")),
    }
}

fn arm(v: &Value) -> Read<Arm> {
    let o = object(v, "an arm")?;
    Ok(Arm {
        case: string(o, "case")?,
        binding: optional(o, "binding", |v| text(v, "binding"))?,
        fields: defaulted(o, "fields", |v| strings(v, "fields"))?,
        body: chunks(field(o, "body")?)?,
    })
}

fn stream_arm(v: &Value) -> Read<StreamArm> {
    let o = object(v, "a stream's arm")?;
    Ok(StreamArm {
        binding: optional(o, "binding", |v| text(v, "binding"))?,
        body: chunks(field(o, "body")?)?,
    })
}

/// An adjacently tagged value's tag and its content, both required, as
/// serde's `#[serde(tag = "..", content = "value")]` reads them.
fn adjacent<'v>(v: &'v Value, tag: &str) -> Read<(&'v str, &'v Value)> {
    let o = object(v, tag)?;
    let name = o
        .get(tag)
        .ok_or_else(|| format!("a {tag} names no `{tag}`"))?
        .as_str()
        .ok_or_else(|| format!("a {tag}'s `{tag}` is not a string"))?;
    let value = o
        .get("value")
        .ok_or_else(|| format!("a {tag} has no `value`"))?;
    Ok((name, value))
}

fn context(o: &Object, name: &str) -> Read<Context> {
    Ok(match field(o, name)?.as_str() {
        Some("text") => Context::Text,
        Some("attribute") => Context::Attribute,
        Some("url") => Context::Url,
        Some("style") => Context::Style,
        Some("raw_html") => Context::RawHtml,
        Some("content") => Context::Content,
        Some(other) => return Err(format!("no context is a `{other}`")),
        None => return Err(format!("`{name}` is not a string")),
    })
}

fn id(o: &Object) -> Read<PartId> {
    Ok(PartId(number(field(o, "id")?, "id")?))
}

fn owner(o: &Object) -> Read<ElementId> {
    Ok(ElementId(number(field(o, "owner")?, "owner")?))
}

/// A `u32`: a JSON integer from 0 to 2^32 - 1, no float, no string.
fn number(v: &Value, name: &str) -> Read<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("`{name}` is not a u32"))
}

fn boolean(v: &Value, name: &str) -> Read<bool> {
    v.as_bool()
        .ok_or_else(|| format!("`{name}` is not a boolean"))
}

fn object<'v>(v: &'v Value, what: &str) -> Read<&'v Object> {
    v.as_object()
        .ok_or_else(|| format!("{what} is not an object"))
}

fn field<'o>(o: &'o Object, name: &str) -> Read<&'o Value> {
    o.get(name).ok_or_else(|| format!("missing field `{name}`"))
}

/// A required string field.
fn string(o: &Object, name: &str) -> Read<String> {
    text(field(o, name)?, name)
}

fn text(v: &Value, name: &str) -> Read<String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("`{name}` is not a string"))
}

fn strings(v: &Value, name: &str) -> Read<Vec<String>> {
    list(v, name, |v| text(v, name))
}

fn string_map(v: &Value, name: &str) -> Read<BTreeMap<String, String>> {
    object(v, name)?
        .iter()
        .map(|(k, v)| Ok((k.clone(), text(v, name)?)))
        .collect()
}

/// A two-element array, as serde reads a pair: no fewer, no more.
fn pair<'v>(v: &'v Value, name: &str) -> Read<(&'v Value, &'v Value)> {
    match v.as_array().map(Vec::as_slice) {
        Some([a, b]) => Ok((a, b)),
        _ => Err(format!("an entry of `{name}` is not a pair")),
    }
}

fn list<T>(v: &Value, name: &str, each: impl Fn(&Value) -> Read<T>) -> Read<Vec<T>> {
    v.as_array()
        .ok_or_else(|| format!("`{name}` is not a list"))?
        .iter()
        .map(each)
        .collect()
}

/// A `#[serde(default)]` field: absent, its type's default; present, read
/// (so `null` is refused where its type has no null, as serde refuses it).
fn defaulted<T: Default>(o: &Object, name: &str, read: impl Fn(&Value) -> Read<T>) -> Read<T> {
    o.get(name).map_or_else(|| Ok(T::default()), read)
}

/// An `Option` field: absent or `null`, `None`, as serde reads an `Option`.
fn optional<T>(o: &Object, name: &str, read: impl Fn(&Value) -> Read<T>) -> Read<Option<T>> {
    match o.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => read(v).map(Some),
    }
}
