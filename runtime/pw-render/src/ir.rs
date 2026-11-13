//! The compiler's template IR, read as data.
//!
//! ADR-0018 again: `pw-core` emits it, this crate deserializes it, and neither
//! links the other. The types mirror `pw_core::template_ir` **by field name**,
//! which is what makes it a boundary rather than a coupling.
//!
//! The cost is that a field could be renamed on one side. `tests/contract.rs`
//! reads IR the real compiler produced, so a rename fails a test rather than
//! becoming an empty template at run time.

use serde::{Deserialize, Serialize};

// The address vocabulary is `pw-document`'s. `PartId` is this crate's name for
// what the document layer calls a `LocalPartId`: the IR is where a part gets
// its number, and the document layer is where that number becomes an address.
pub use pw_document::{Anchor, ElementId, LocalPartId as PartId};

/// Where a value sits in the document, which decides how it is escaped.
///
/// Not a hint. Two values with the same bytes in different contexts need
/// different treatment, and a renderer that escapes "the usual way" is correct
/// for text and wrong for the other four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    /// Character data between tags. `<` and `&` are the dangerous ones.
    Text,
    /// Inside a double-quoted attribute value. `"` and `&` are the dangerous
    /// ones; `<` is not, and escaping it there is harmless but not the point.
    Attribute,
    /// An attribute the HTML spec treats as a URL — `href`, `src`, `action`,
    /// `formaction`, `poster`, `cite`. A `javascript:` scheme here is a script
    /// tag with extra steps, and no amount of character escaping prevents it.
    Url,
    /// Inside a `style` attribute or element.
    Style,
    /// Emitted verbatim. Reachable only from a value that carries the
    /// capability, never from an ordinary string — see [`Part::RawHtml`].
    RawHtml,
    /// **A form control's value, written as its text** (ADR-0221): a
    /// `<textarea>`'s `value`, which HTML reads from the element's content
    /// and from no attribute. Escaped as text, so no `</textarea>` in it ends
    /// the element, and a leading newline doubled, since the parser drops
    /// the one after the start tag. A patch carries it as an attribute's.
    Content,
}

impl Context {
    /// The context an attribute's value occupies, from its name.
    ///
    /// A table, because it is a fact about HTML rather than about this program.
    /// `href` is a URL in every document ever written.
    pub fn of_attribute(name: &str) -> Context {
        const URL_ATTRS: &[&str] = &[
            "href",
            "src",
            "action",
            "formaction",
            "poster",
            "cite",
            "data",
            "manifest",
            "ping",
            "srcset",
        ];
        let bare = name.rsplit(':').next().unwrap_or(name);
        if URL_ATTRS.contains(&bare) {
            Context::Url
        } else if bare == "style" {
            Context::Style
        } else {
            Context::Attribute
        }
    }
}

/// One dynamic hole in a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "part")]
pub enum Part {
    /// `{expr}` between tags.
    Text {
        id: PartId,
        /// The value's identity — a path the renderer looks up, never an
        /// expression it evaluates. Evaluation is the program's job and it has
        /// already happened by the time bytes are produced.
        value: String,
        context: Context,
    },
    /// `class={expr}` — the whole value.
    Attribute {
        id: PartId,
        owner: ElementId,
        name: String,
        value: String,
        context: Context,
    },
    /// `disabled={expr}` where the attribute's presence is the value.
    ///
    /// Separate from `Attribute` because the rendering rule is different in
    /// kind: a false boolean attribute is ABSENT, not empty. `disabled=""` is
    /// disabled.
    BooleanAttribute {
        id: PartId,
        owner: ElementId,
        name: String,
        value: String,
    },
    /// `on:press={handler}` — behaviour the browser runtime attaches.
    ///
    /// Represented rather than `Blocked`, which is what E7-2 did: a renderer
    /// with no runtime had nothing to emit and refusing was the honest answer.
    /// E7-R has one, so the part carries what it needs — which element, which
    /// event, and which handler identity to authorise and attach.
    Event {
        id: PartId,
        owner: ElementId,
        /// `press`, from `on:press`. The semantic event, not a DOM event name:
        /// translating it is the runtime's job and it differs per element.
        event: String,
        /// The handler's identity, as the resume manifest names it: what
        /// authorises this handler to attach here.
        handler: String,
        /// The handler's name: which code to load. A different question from
        /// the identity, and E7-L needs both — see `pw_core::template_ir`.
        #[serde(default)]
        name: String,
        /// What the handler reads of its captures, as paths (`item.id`); each
        /// value is serialized onto the element for the compiled handler to
        /// read. See `pw_core::resume::capture_paths`.
        #[serde(default)]
        captures: Vec<String>,
        /// Where a capture's value is when the handler names it otherwise: a
        /// view's parameter, composed into a page, read at the path it was
        /// given (ADR-0136). See `pw_core::template_ir::Part::Event`.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        renames: std::collections::BTreeMap<String, String>,
        /// What the runtime does in the listener, before any code loads:
        /// `prevent`, `stop` (ADR-0131). The renderer writes nothing for it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modifiers: Vec<String>,
    },
    /// A region rendered only when a condition holds.
    Conditional {
        id: PartId,
        value: String,
        then: Vec<Chunk>,
        /// Empty when there is no `else`.
        otherwise: Vec<Chunk>,
    },
    /// A region rendered once per element of a collection.
    Each {
        id: PartId,
        collection: String,
        /// The name each element is bound to inside `body`.
        binding: String,
        /// What identifies an element, as the path from the element to it:
        /// `id` from `(item.id)`, `r.id` from `(item.r.id)`, and empty from
        /// `(item)` (ADR-0073). `None` means the list declared no key — which
        /// markup rules reject for a mutable collection and permit for a
        /// static one.
        key: Option<String>,
        body: Vec<Chunk>,
        /// **Where each row's identity is written** (ADR-0438): between two
        /// comments, as every keyed list's was, or on the row's one element,
        /// where its build read the row as one element a parser keeps whole
        /// and nothing addresses inside it (`own-rows.mjs`'s `rowElement`,
        /// which the packaging writes here). The compiler writes none.
        #[serde(default, skip_serializing_if = "RowAnchor::is_comments")]
        row_anchor: RowAnchor,
    },
    /// `{#match value}{:Some(x)} .. {:None} .. {/match}`: the arm whose case
    /// the value is, with the case's payload bound to the arm's name
    /// (ADR-0042).
    Match {
        id: PartId,
        value: String,
        arms: Vec<Arm>,
    },
    /// `<stream query={Q(args)}>` (ADR-0148): a region that shows its
    /// query's state. Pending, a streamed one shows its placeholder; then the
    /// arm the query settled to, which the host gives as [`crate::Settled`].
    Stream {
        id: PartId,
        /// The query, as a component id.
        query: String,
        /// Each argument as a host computes it.
        args: Vec<String>,
        /// `delivery streamed`: the document is sent before the query answers.
        streamed: bool,
        placeholder: Vec<Chunk>,
        ready: StreamArm,
        failed: StreamArm,
    },
    /// `href="/stores/{id}"`: static text and values, each value escaped for
    /// the attribute's context (ADR-0042).
    InterpolatedAttribute {
        id: PartId,
        owner: ElementId,
        name: String,
        segments: Vec<Segment>,
        context: Context,
    },
    /// **A use of a view that contains itself** (ADR-0203): an instance of
    /// the view's own template, rendered in a frame of its own, each
    /// parameter given the value its path reads where the use is.
    Instance {
        id: PartId,
        /// The view's template, by its path.
        path: String,
        args: Vec<(String, String)>,
        /// How many elements enclose the use, in its template.
        #[serde(default)]
        elements: u32,
        /// How many elements the view's template nests, from its root.
        #[serde(default)]
        deepest: u32,
    },
    /// `<title>{store.name}</title>` at the top of a page's view (ADR-0183):
    /// the page's title. Its host writes it into the document's `<head>`
    /// ([`crate::title_text`]), so it renders nothing in the body.
    Title { id: PartId, pieces: Vec<TitlePiece> },
    /// `<meta name="description" content={store.description} />` at the top
    /// of a page's view (ADR-0186). Its host writes it into the document's
    /// `<head>` ([`crate::head_metadata`]), so it renders nothing in the
    /// body.
    Meta {
        id: PartId,
        /// `name` or `property`.
        attribute: String,
        key: String,
        content: Vec<TitlePiece>,
    },
    /// `<link rel="canonical" href={StorePage(id)} />` at the top of a
    /// page's view (ADR-0311): the route of the page it names, and what fills
    /// each `{name}` of it. Its host writes it into the document's `<head>`,
    /// absolute with the deployment's origin ([`crate::head_canonical`]), so
    /// it renders nothing in the body.
    Canonical {
        id: PartId,
        route: String,
        args: Vec<(String, TitlePiece)>,
    },
    /// Verbatim bytes.
    ///
    /// Reachable only from a value whose type carries the raw-HTML capability.
    /// The `capability` field records which, so the artifact says on its face
    /// what authorised the bypass — an "it was fine when I wrote it" is not
    /// checkable and this is.
    RawHtml {
        id: PartId,
        value: String,
        capability: String,
    },
    /// A construct this IR does not represent, or one an earlier analysis
    /// rejected.
    ///
    /// Rendering it is an error, not an empty string. `Outcome` again: recovery
    /// is not proof, and a renderer that silently omits what it did not
    /// understand produces a page that looks correct and is missing something.
    Blocked { reason: String, at: String },
}

/// **Where a keyed list's row writes its identity** (ADR-0438).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowAnchor {
    /// `<!--pw:s3@T-->` the row `<!--pw:e3@T-->`: whatever the row is.
    #[default]
    Comments,
    /// `<li data-pw-row="3@T">`: the row's one element is its own anchor,
    /// two nodes fewer a row, where nothing addresses what is inside a row
    /// (a page whose presses are heard at its document, ADR-0413).
    Element,
}

impl RowAnchor {
    pub fn is_comments(&self) -> bool {
        *self == RowAnchor::Comments
    }
}

/// One arm of a [`Part::Match`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arm {
    /// `Some`, `None`, `Ok` or `Err`; a declared sum type's case by its WIT
    /// name, `circle` (ADR-0061).
    pub case: String,
    /// The payload of a case of one field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    /// Each field of a case of several, whose payload is a list of them
    /// (ADR-0061).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    pub body: Vec<Chunk>,
}

/// One settled arm of a [`Part::Stream`]: the name its value is bound to,
/// and what it renders.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamArm {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    pub body: Vec<Chunk>,
}

/// A piece of a page's title (ADR-0183): text as written, or a value's path.
/// Not escaped: a title is text, escaped where it is written into `<title>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "piece", content = "value")]
pub enum TitlePiece {
    Text(String),
    Value(String),
}

/// A piece of an interpolated attribute: text escaped at build time, or a
/// value's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "segment", content = "value")]
pub enum Segment {
    Static(String),
    Value(String),
}

impl Part {
    /// The chunk lists nested in this part, in document order. Every walk
    /// descends through this, so no walk can miss a part's regions.
    pub fn nested(&self) -> Vec<&[Chunk]> {
        match self {
            Part::Conditional {
                then, otherwise, ..
            } => vec![then, otherwise],
            Part::Each { body, .. } => vec![body],
            Part::Match { arms, .. } => arms.iter().map(|a| a.body.as_slice()).collect(),
            Part::Stream {
                placeholder,
                ready,
                failed,
                ..
            } => vec![placeholder, &ready.body, &failed.body],
            _ => Vec::new(),
        }
    }

    /// This part's identity, or `None` for a `Blocked` one — which has no
    /// identity because it is never rendered.
    pub fn id(&self) -> Option<PartId> {
        Some(match self {
            Part::Text { id, .. }
            | Part::Attribute { id, .. }
            | Part::BooleanAttribute { id, .. }
            | Part::Event { id, .. }
            | Part::Conditional { id, .. }
            | Part::Each { id, .. }
            | Part::Match { id, .. }
            | Part::Stream { id, .. }
            | Part::InterpolatedAttribute { id, .. }
            | Part::Instance { id, .. }
            | Part::RawHtml { id, .. }
            | Part::Title { id, .. }
            | Part::Meta { id, .. }
            | Part::Canonical { id, .. } => *id,
            Part::Blocked { .. } => return None,
        })
    }

    /// The element this part belongs to, for the element-anchored kinds.
    pub fn owner(&self) -> Option<ElementId> {
        match self {
            Part::Attribute { owner, .. }
            | Part::BooleanAttribute { owner, .. }
            | Part::Event { owner, .. }
            | Part::InterpolatedAttribute { owner, .. } => Some(*owner),
            _ => None,
        }
    }

    /// How this kind of part is anchored.
    pub fn anchor(&self) -> Option<Anchor> {
        Some(match self {
            Part::Attribute { .. }
            | Part::BooleanAttribute { .. }
            | Part::Event { .. }
            | Part::InterpolatedAttribute { .. } => Anchor::Element,
            Part::Text { .. }
            | Part::Conditional { .. }
            | Part::Each { .. }
            | Part::Match { .. }
            | Part::Stream { .. }
            | Part::Instance { .. }
            | Part::RawHtml { .. } => Anchor::Range,
            Part::Title { .. } | Part::Meta { .. } | Part::Canonical { .. } => Anchor::Document,
            Part::Blocked { .. } => return None,
        })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Part::Text { .. } => "text",
            Part::Attribute { .. } => "attribute",
            Part::BooleanAttribute { .. } => "boolean_attribute",
            Part::Event { .. } => "event",
            Part::Conditional { .. } => "conditional",
            Part::Each { .. } => "each",
            Part::Match { .. } => "match",
            Part::Stream { .. } => "stream",
            Part::InterpolatedAttribute { .. } => "interpolated_attribute",
            Part::Instance { .. } => "instance",
            Part::RawHtml { .. } => "raw_html",
            Part::Title { .. } => "title",
            Part::Meta { .. } => "meta",
            Part::Canonical { .. } => "canonical",
            Part::Blocked { .. } => "blocked",
        }
    }
}

/// A piece of a template: literal bytes, or a hole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Adjacently tagged, not internally: an internally-tagged representation
// cannot encode a newtype variant holding a string, and `Static` is one. The
// alternative was to make it a struct variant for serde's benefit, which would
// let a serialization detail choose the shape of the IR.
#[serde(rename_all = "snake_case", tag = "chunk", content = "value")]
pub enum Chunk {
    /// Bytes emitted exactly, already escaped where they came from source text.
    Static(String),
    Dynamic(Part),
}

/// One row of the parts manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartEntry {
    pub id: PartId,
    pub kind: String,
    pub anchor: Anchor,
    /// Set for the element-anchored kinds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ElementId>,
    /// What the part reads, for a text or attribute part; the collection for a
    /// loop; the handler IDENTITY for an event.
    pub value: String,
    /// The handler's name, for an event part.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The event, for an event part: what the runtime listens for (ADR-0138).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event: String,
    /// What the runtime does in the listener, for an event part (ADR-0131).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// One renderable declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    /// `Module.Name`.
    pub path: String,
    pub name: String,
    /// Parameters, in declaration order — the renderer's inputs.
    pub params: Vec<String>,
    /// The template's **semantic** identity: what makes local part 3 mean
    /// something.
    ///
    /// Over the IR's structure — kinds, names, contexts, nesting, order — and
    /// not over source bytes, so a comment or a reflow does not change it and
    /// a reordered attribute does. E7V's scheme-2 reasoning, one layer up: a
    /// digest cannot say what made it, so the parts it namespaces are only
    /// comparable within one schema, and E7V's decision already refuses across
    /// schemas.
    pub schema: String,
    pub chunks: Vec<Chunk>,
}

impl Template {
    /// The **parts manifest**: dynamic regions only (charter §14 M7 task 4).
    ///
    /// What the browser runtime needs in order to find and update a part, and
    /// nothing else. Static regions do not appear, because there is nothing for
    /// the runtime to do with them — which is the same invariant that keeps
    /// them free of identity markup.
    ///
    /// Read from the same numbering the server renderer emitted, because there
    /// is one indexing pass and everybody reads it. A manifest generator that
    /// walked the IR again would be the second traversal.
    pub fn manifest(&self) -> Vec<PartEntry> {
        fn walk(chunks: &[Chunk], out: &mut Vec<PartEntry>) {
            for c in chunks {
                let Chunk::Dynamic(p) = c else { continue };
                let (Some(id), Some(anchor)) = (p.id(), p.anchor()) else {
                    continue;
                };
                out.push(PartEntry {
                    id,
                    kind: p.kind().to_string(),
                    anchor,
                    owner: p.owner(),
                    value: match p {
                        Part::Text { value, .. }
                        | Part::Attribute { value, .. }
                        | Part::BooleanAttribute { value, .. }
                        | Part::RawHtml { value, .. }
                        | Part::Conditional { value, .. }
                        | Part::Match { value, .. } => value.clone(),
                        // Every value the attribute reads, in order.
                        Part::InterpolatedAttribute { segments, .. } => segments
                            .iter()
                            .filter_map(|s| match s {
                                Segment::Value(v) => Some(v.as_str()),
                                Segment::Static(_) => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                        Part::Each { collection, .. } => collection.clone(),
                        Part::Event { handler, .. } => handler.clone(),
                        Part::Instance { path, .. } => path.clone(),
                        Part::Stream { query, .. } => query.clone(),
                        // Every value the title or the metadata reads, in
                        // order.
                        Part::Title {
                            pieces: content, ..
                        }
                        | Part::Meta { content, .. } => content
                            .iter()
                            .filter_map(|p| match p {
                                TitlePiece::Value(v) => Some(v.as_str()),
                                TitlePiece::Text(_) => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                        // Every value its address reads, in the route's order.
                        Part::Canonical { args, .. } => args
                            .iter()
                            .filter_map(|(_, p)| match p {
                                TitlePiece::Value(v) => Some(v.as_str()),
                                TitlePiece::Text(_) => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                        Part::Blocked { .. } => String::new(),
                    },
                    name: match p {
                        Part::Event { name, .. } => name.clone(),
                        _ => String::new(),
                    },
                    event: match p {
                        Part::Event { event, .. } => event.clone(),
                        _ => String::new(),
                    },
                    modifiers: match p {
                        Part::Event { modifiers, .. } => modifiers.clone(),
                        _ => Vec::new(),
                    },
                });
                for inner in p.nested() {
                    walk(inner, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.chunks, &mut out);
        out.sort_by_key(|e| e.id);
        out
    }

    /// Every `Blocked` part, anywhere in the tree.
    ///
    /// The renderer refuses a template with any, so this is what a caller asks
    /// before rendering rather than a diagnostic it recovers from.
    pub fn blocked(&self) -> Vec<&Part> {
        fn walk<'a>(chunks: &'a [Chunk], out: &mut Vec<&'a Part>) {
            for c in chunks {
                let Chunk::Dynamic(p) = c else { continue };
                if let Part::Blocked { .. } = p {
                    out.push(p);
                }
                for inner in p.nested() {
                    walk(inner, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.chunks, &mut out);
        out
    }
}

/// Elements HTML forbids a closing tag on.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
