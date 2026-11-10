//! E7 task 2 — the checked template IR.
//!
//! ```text
//! .pw → canonical syntax → HIR → types/labels/effects/placement
//!     → checked Template IR → E7 server renderer → HTML bytes
//! ```
//!
//! # The renderer must not rediscover program meaning
//!
//! Architect ruling, 2026-08-06. No textual name resolution, no "if this
//! expression happens to be called `query`", no re-parsing fragments, no
//! adapter-specific guesses. The renderer consumes identities and decisions
//! that are already represented here.
//!
//! That is why this is not "the HIR, serialized". The HIR is a faithful record
//! of what was written; a renderer walking it would have to ask, at every node,
//! what the node probably means. Every such question is answered once, here,
//! where the answer can be wrong in a way a test can see.
//!
//! # Static and dynamic are different things
//!
//! ```text
//! Template {
//!     Static("<main><h1>")
//!     Text(part 0)
//!     Static("</h1>")
//!     ...
//! }
//! ```
//!
//! The split is explicit rather than derived, and it is the representation the
//! reactive renderer reuses: the server renders `Static` + `Part`s, and the
//! browser runtime later updates only `Part`s. Deriving it at render time would
//! mean deriving it again, differently, in the runtime.
//!
//! # Escaping context is part of the IR
//!
//! A `Part` carries the [`Context`] its value occupies, decided here from where
//! the value appears in the markup. A renderer with one `escape_html` and a
//! guess at the call site is the classic injection surface: the same string is
//! inert in text, an attribute breakout in an attribute, and a script in a URL.
//!
//! # Nothing is silently dropped
//!
//! A construct the IR cannot represent becomes [`Part::Blocked`], carrying why.
//! The `Outcome` lesson from E2D, applied to rendering: a renderer that emitted
//! `""` for a region it did not understand would turn "an earlier analysis
//! failed" into apparently valid HTML, and the page would look fine.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::hir::{AttrValue, Body, DeclId, Expr, ExprId, Hir, Node, NodeId};
use crate::resolve::{DefId, Resolution, Workspace};
use crate::signatures::Signatures;

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
    /// `href` is a URL in every document ever written, however it is spelled:
    /// HTML lowercases an attribute's name, so `HREF` is `href`. Until
    /// 2026-09-26 the name was read as written, and `HREF={msg}` was escaped
    /// as an ordinary attribute, which refuses no scheme (ADR-0095).
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
        let bare = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
        let bare = bare.as_str();
        if URL_ATTRS.contains(&bare) {
            Context::Url
        } else if bare == "style" {
            Context::Style
        } else {
            Context::Attribute
        }
    }
}

/// A part's identity within its template.
///
/// **Template-scoped and ordinal**, namespaced by [`Template::schema`].
/// Architect ruling, 2026-08-06:
///
/// > semantic identity of entire template + cheap structural identity inside
/// > that template
///
/// not a content hash per part. Two `<span>{price}</span>` parts have identical
/// IR and are different places in the document, so content identity cannot say
/// which to patch; adding enough parent context to disambiguate reinvents
/// structural position at a higher price.
///
/// Positional identity is safe here **because E7V already refuses across
/// schemas**. Local part 3 is never interpreted as local part 3 of an
/// incompatible template, so a source edit may renumber freely: it also changes
/// the schema, and a cross-version patch is rejected or migrated.
///
/// Assigned by deterministic traversal of the IR, never from source offsets. A
/// comment or a reflow must not perturb an id unless it changed the structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PartId(pub u32);

impl std::fmt::Display for PartId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An element that owns at least one element-local part.
///
/// Separate from `PartId` because one element can own several parts — two
/// dynamic attributes and a handler — and giving each its own comment pair
/// would cost six nodes to say one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ElementId(pub u32);

impl std::fmt::Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How a part is anchored in the document.
///
/// Architect ruling: a `PartId` is a renderer concept and these are two wire
/// encodings of it, chosen per part KIND rather than one forced onto all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// Comment boundaries: `<!--pw:s3-->` … `<!--pw:e3-->`.
    ///
    /// A range can be zero nodes, one text node, twenty `<li>`s or a component
    /// subtree. An attribute on an element cannot represent any of those, which
    /// is why conditionals, loops, components and text ranges use comments even
    /// though they cost two nodes.
    Range,
    /// The owning element carries `data-pw`.
    ///
    /// For attributes, boolean attributes and handlers, where the thing being
    /// updated belongs to an element that already exists.
    Element,
    /// The document itself (ADR-0183): a page's title, which its host writes
    /// into `<head>` and the browser's runtime sets as `document.title`.
    Document,
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
        /// The handler's identity, as the resume manifest names it.
        ///
        /// **What authorises it.** Derived from the implementation, its
        /// dependencies, its capture schema and the platform ABI, so a changed
        /// handler is a different identity and the runtime refuses to resume
        /// into it.
        handler: String,
        /// The handler's NAME: which behaviour to load.
        ///
        /// A different question from the identity, and E7-L needs both. The
        /// identity says *may this handler attach here*; the name says *which
        /// code is it*. One value cannot answer both, because the identity
        /// changes when the implementation changes and the code to load is the
        /// same code either way — so a loader keyed on identity alone would
        /// have nothing to ask for.
        #[serde(default)]
        name: String,
        /// **What the handler reads of its captures**, as field paths:
        /// `item.id` in `resumable(captures = { item }) => add_to_cart(item.id,
        /// ..)`. The renderer serializes each path's value onto the element,
        /// nested by segment, and the compiled handler reads it there (E10,
        /// 2026-09-25). One derivation, `resume::capture_paths`, for both.
        /// Empty for a handler that reads nothing it captured.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        captures: Vec<String>,
        /// **Where a capture's value is, when the handler names it otherwise**
        /// (ADR-0136): a handler written in a view captures the view's
        /// parameter, `item`, and the view composed into a page reads it as
        /// the path it was given, `entry`. The renderer reads `item.id` at
        /// `entry.id` and writes it under `item.id`, which is what the
        /// handler's module reads. Empty where every name is the handler's.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        renames: BTreeMap<String, String>,
        /// **What the runtime does in the listener, before any code loads**
        /// (ADR-0131): `prevent` and `stop`, from `on:submit|prevent`.
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
    },
    /// `{#match value}{:Some(x)} .. {:None} .. {/match}` (ADR-0042): the arm
    /// whose case the value is, with the case's payload bound to the arm's
    /// name. A range part, as a conditional is.
    Match {
        id: PartId,
        value: String,
        arms: Vec<Arm>,
    },
    /// `<stream query={Q(args)}>` (ADR-0148): a region that shows its
    /// query's state. While a streamed query is pending it shows its
    /// placeholder, and then the arm the query settled to. A range part, as a
    /// block is.
    Stream {
        id: PartId,
        /// The query, as a component id: `store.page.Recommendations`.
        query: String,
        /// Each argument as a host computes it: a value's path, `id`, or an
        /// invocation-context call, `current_session()`.
        args: Vec<String>,
        /// `delivery streamed`: the document is sent before the query answers.
        streamed: bool,
        /// What shows while a streamed query is pending. Empty for one the
        /// page waits for.
        placeholder: Vec<Chunk>,
        /// `<ready as={items}>`: what the query answered.
        ready: StreamArm,
        /// `<failed as={why}>`: `Some(e)`, its declared error, or `None`, the
        /// host's failure.
        failed: StreamArm,
    },
    /// `href="/stores/{id}"` (ADR-0042): static text and values, each value
    /// escaped for the attribute's context. In a URL, each value is a URI
    /// component, so it cannot add a segment, a query, a fragment or a scheme.
    InterpolatedAttribute {
        id: PartId,
        owner: ElementId,
        name: String,
        segments: Vec<Segment>,
        context: Context,
    },
    /// **A use of a view that contains itself** (ADR-0203, ADR-0130's
    /// ruling 2): an instance of the view's own template, made at run time,
    /// each parameter given a value path read where the use is. Its parts
    /// are addressed inside a frame it opens.
    Instance {
        id: PartId,
        /// The view's template, by its path.
        path: String,
        args: Vec<(String, String)>,
        /// How many elements enclose the use, in its template.
        #[serde(default)]
        elements: u32,
        /// How many elements the deepest part of the view's template nests
        /// in, from its root: with `elements`, how deep each instance takes
        /// the page, which the renderer bounds.
        #[serde(default)]
        deepest: u32,
    },
    /// `<title>{store.name}</title>` at the top of a page's view (ADR-0183):
    /// the page's title. Its host writes it into the document's `<head>`,
    /// not where it is written, so it renders nothing in the body. Numbered
    /// after every other part of the page, so writing one moves no other.
    Title { id: PartId, pieces: Vec<TitlePiece> },
    /// `<meta name="description" content={store.description} />` at the top
    /// of a page's view (ADR-0186): what the page says of itself to what
    /// reads it without showing it. Its host writes it into the document's
    /// `<head>` as the page is served; it renders nothing in the body.
    /// Numbered after the title.
    Meta {
        id: PartId,
        /// `name` or `property`.
        attribute: String,
        /// `description`, `og:title`.
        key: String,
        content: Vec<TitlePiece>,
    },
    /// `<link rel="canonical" href={StorePage(id)} />` at the top of a
    /// page's view (ADR-0311): the address the page names its own. Its host
    /// writes it into the document's `<head>`, absolute with the
    /// deployment's origin, and Open Graph's `og:url` from it; it renders
    /// nothing in the body. Numbered after the metadata.
    Canonical {
        id: PartId,
        /// The route of the page it names, as declared: `/stores/{id}`.
        route: String,
        /// Each `{name}` of the route, and what fills it: text, or a value
        /// the page reads.
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

/// One arm of a [`Part::Match`]: its case, the names its payload is bound
/// to, and what it renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arm {
    /// The case by its WIT name, as every case is named here: `some`,
    /// `none`, `ok`, `err`, or a declared case's, `circle` (ADR-0061).
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

/// One settled arm of a [`Part::Stream`] (ADR-0148): the name its value is
/// bound to, and what it renders.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamArm {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    pub body: Vec<Chunk>,
}

/// A piece of a page's title (ADR-0183): text as written, or a value's path.
///
/// Not escaped: a title is text. Its host escapes it where it writes it into
/// `<title>`, and the browser's runtime sets it as `document.title`, which
/// takes text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "piece", content = "value")]
pub enum TitlePiece {
    Text(String),
    Value(String),
}

/// A piece of an interpolated attribute: text as written, or a value's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "segment", content = "value")]
pub enum Segment {
    /// Escaped at build time, as a static attribute is.
    Static(String),
    Value(String),
}

impl Part {
    /// The chunk lists nested in this part, in document order. Every walk
    /// over the IR descends through this, so a part with regions cannot be
    /// missed by one walk and seen by another.
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

    /// [`Part::nested`], to change.
    fn nested_mut(&mut self) -> Vec<&mut Vec<Chunk>> {
        match self {
            Part::Conditional {
                then, otherwise, ..
            } => vec![then, otherwise],
            Part::Each { body, .. } => vec![body],
            Part::Match { arms, .. } => arms.iter_mut().map(|a| &mut a.body).collect(),
            Part::Stream {
                placeholder,
                ready,
                failed,
                ..
            } => vec![placeholder, &mut ready.body, &mut failed.body],
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

/// Assigns every identity, once.
///
/// Architect ruling, 2026-08-06:
///
/// > One traversal assigns identity. Everybody else reads it.
///
/// The alternative — the server serializer numbering parts one way, the
/// manifest generator walking them another, the patch generator a third — is
/// the duplicated-traversal shape this project has found dangerous four times.
/// So identity is assigned during construction, in document order, and the
/// server renderer, the parts manifest, the patch generator and any devtools
/// read the same numbers.
#[derive(Debug, Default)]
struct Indexer {
    next_part: u32,
    next_element: u32,
    /// How many blocks (`{#each}`, `{#match}`, `{#if}`) enclose the node
    /// being lowered.
    depth: u32,
    /// Each text part, with the expression its hole holds (ADR-0122).
    holes: Vec<Hole>,
    /// Each value read outside a text part (ADR-0169).
    reads: Vec<Read>,
    /// How many names composition has renamed (ADR-0136), so each new name
    /// is one no other has.
    renamed: u32,
    /// How many `{#each}` enclose the node being lowered: a part inside one
    /// is an instance's, addressed with a frame (ADR-0142).
    frames: u32,
    /// Each view composed into the template, in the order first written.
    views: Vec<DefId>,
    /// The blocks around the node being lowered, outermost first (ADR-0144).
    blocks: Vec<PartId>,
    /// Each signal instance the template holds, in the order written
    /// (ADR-0144).
    instances: Vec<Instance>,
    /// How many elements enclose the node being lowered (ADR-0203).
    elements: u32,
    /// The most elements any node of the template nests in.
    deepest: u32,
    /// The template's path, which a computed part's own is named by
    /// (ADR-0226).
    template: String,
    /// The name each `{#each}` around the node binds its item to, outermost
    /// first: a value computed from the innermost's is the row's (ADR-0228).
    loops: Vec<String>,
    /// **The layout being lowered around a page** (ADR-0303): the file and
    /// the declaration whose `<slot />` is the page's place.
    layout: Option<(usize, DeclId)>,
    /// Where its `<slot />` was written: the page's markup goes there.
    slot: Option<Slot>,
    /// The tag of each element around the node being lowered, outermost
    /// first: what its text is taken to be ([`as_shown`]).
    parents: Vec<String>,
}

/// **Where a layout's `<slot />` is** (ADR-0303): the index in the
/// template's chunks the page's markup is placed at, and how many elements
/// enclose it, which the page's own nest inside (ADR-0203).
#[derive(Debug, Clone, Copy)]
struct Slot {
    at: usize,
    elements: u32,
}

/// **The layout a page is shown in** (ADR-0303), as its plan records it:
/// what a navigation compares, to keep the layout in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageLayout {
    /// `Module.Name`.
    pub path: String,
    /// Its own markup's schema, its slot empty: the same on every page that
    /// names it, and another where its markup changed.
    pub schema: String,
    /// How many parts it numbers, first: its parts are `0..parts` on every
    /// page that names it.
    pub parts: u32,
    /// And elements: `0..elements`.
    pub elements: u32,
}

impl Indexer {
    /// **The path a computed part is read by** (ADR-0226, ADR-0228): one the
    /// compiler names, which no source can write. A row's, computed from its
    /// item alone, is the item's, `p.#feed.app.Home~7`, so each row holds its
    /// own, as a member read of it does (ADR-0169); any other, the template's.
    fn computed_path(&self, part: PartId, inputs: &[(String, String)]) -> String {
        let named = format!("#{}~{}", self.template, part.0);
        match self.loops.last() {
            Some(item)
                if !inputs.is_empty()
                    && inputs
                        .iter()
                        .all(|(_, read)| read.split('.').next() == Some(item.as_str())) =>
            {
                format!("{item}.{named}")
            }
            _ => named,
        }
    }
}

/// **A signal the template holds** (ADR-0144): a page's own, a composed
/// view's own at each use, and each `provide`'s. The browser holds one value
/// for each, by its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// As the template reads it: `open` in the page's own markup, a fresh
    /// name no source can write at each use of a view, `open~2`.
    pub name: String,
    /// The file and the declaration whose body writes its first value.
    pub origin: (usize, DeclId),
    /// Its first value: a `signal`'s initializer, or a `provide`'s value.
    pub init: ExprId,
    /// Its type, as written.
    pub ty: InstanceType,
    /// The blocks it is inside, outermost first. Its value ends when one of
    /// them shows another arm (ADR-0130, R6).
    pub within: Vec<PartId>,
}

/// Where an instance's type is written (ADR-0144).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceType {
    /// A `signal x: T = e`'s `T`, in the origin's body.
    Written(crate::hir::TypeRefId),
    /// A module's `signal drawer: T`, which a `provide` gives.
    Declared(DefId),
}

/// **A text part and the expression it shows** (ADR-0122): what an optimistic
/// speculation recomputes in the browser. Recorded by the same traversal that
/// numbers the parts, so the two cannot disagree about which part is which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hole {
    pub part: PartId,
    /// The path the part reads, as the template names it: through each view
    /// composed around it, a view's parameter read as what it was given
    /// (ADR-0136).
    pub path: String,
    pub expr: ExprId,
    /// The file and the declaration whose body `expr` is in: the template's
    /// own, or a view composed into it (ADR-0136).
    pub origin: (usize, DeclId),
    /// Inside a block.
    pub nested: bool,
    /// Inside an `{#each}`: an instance's, whose address carries a frame the
    /// browser does not compute (ADR-0142).
    pub framed: bool,
    /// **A computed hole's inputs** (ADR-0226): each name its expression
    /// reads that is bound outside it, and the path the template reads it by.
    /// `None` for a hole that is a path.
    pub inputs: Option<Vec<(String, String)>>,
}

/// **A value the template reads outside a text part** (ADR-0169): an
/// attribute's, what a block decides by, or the list a loop iterates. Each is
/// read by path, as a text part's is, so what it reads through a member
/// function a host computes, or the page's plan refuses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// The part that reads it: the attribute's, or the block's.
    pub part: PartId,
    /// As the template reads it, through each view around it (ADR-0136).
    pub path: String,
    pub kind: ReadKind,
    /// Where it is written: an expression, or for a loop's list its block,
    /// whose directive writes the list as a path.
    pub at: ReadAt,
    /// The file and the declaration whose body it is written in.
    pub origin: (usize, DeclId),
    /// Inside a block.
    pub nested: bool,
    /// A computed attribute's inputs (ADR-0226), as a [`Hole`]'s.
    pub inputs: Option<Vec<(String, String)>>,
}

/// What reads a [`Read`]'s value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKind {
    /// An attribute's value, or a value written in one.
    Attribute,
    /// What an `{#if}`, an `{:else if}` or a `{#match}` decides by.
    Subject,
    /// The list an `{#each}` iterates.
    List,
    /// A value a page's `<title>` reads (ADR-0183).
    Title,
    /// A value a page's `<meta>` reads (ADR-0186).
    Meta,
    /// **What a handler captures** (ADR-0217): written on its element for the
    /// handler to read when it runs, and set again when the value changes.
    Captures,
}

/// Where a [`Read`] is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadAt {
    Expr(ExprId),
    /// An `{#each}` block.
    List(NodeId),
}

impl Indexer {
    /// A value read outside a text part, where it is being lowered.
    fn read(&mut self, part: PartId, path: &str, kind: ReadKind, at: ReadAt, ctx: &Lowering<'_>) {
        self.reads.push(Read {
            part,
            path: path.to_string(),
            kind,
            at,
            origin: (ctx.unit, ctx.decl),
            nested: self.depth > 0,
            inputs: None,
        });
    }
}

impl Indexer {
    fn part(&mut self) -> PartId {
        let id = PartId(self.next_part);
        self.next_part += 1;
        id
    }

    fn element(&mut self) -> ElementId {
        let id = ElementId(self.next_element);
        self.next_element += 1;
        id
    }

    /// A name for `name` that nothing written can be: `~` is in no
    /// identifier (ADR-0136).
    fn rename(&mut self, name: &str) -> String {
        self.renamed += 1;
        format!("{name}~{}", self.renamed)
    }
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

/// One row of the parts manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartEntry {
    pub id: PartId,
    pub kind: &'static str,
    pub anchor: Anchor,
    /// Set for the element-anchored kinds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ElementId>,
    /// What the part reads, for a text or attribute part; the collection for a
    /// loop; the handler IDENTITY for an event.
    pub value: String,
    /// The handler's name, for an event part. Which code to load, as opposed
    /// to which identity to authorise — see [`Part::Event`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The event, for an event part: what the runtime listens for (ADR-0138).
    /// Until 2026-10-02 the manifest did not say, and the runtime listened
    /// for a click whatever the event.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event: String,
    /// What the runtime does in the listener, for an event part (ADR-0131).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
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
                    kind: p.kind(),
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
                        // The query whose state the region shows.
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
///
/// A fact about HTML, listed rather than guessed. Emitting `</img>` produces a
/// parse error the browser repairs by ignoring it — plausible bytes, different
/// tree.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Attributes whose PRESENCE is their value.
pub const BOOLEAN_ATTRIBUTES: &[&str] = &[
    "allowfullscreen",
    "async",
    "autofocus",
    "autoplay",
    "checked",
    "controls",
    "default",
    "defer",
    "disabled",
    "formnovalidate",
    "hidden",
    "inert",
    "ismap",
    "itemscope",
    "loop",
    "multiple",
    "muted",
    "nomodule",
    "novalidate",
    "open",
    "playsinline",
    "readonly",
    "required",
    "reversed",
    "selected",
];

/// Handler identities by where the lambda is, from `resume_artifacts::located`.
///
/// Keyed by the file too (ADR-0135): a declaration's and an expression's
/// index are each counted within one file, so two pages of one shape in two
/// files had one key, and one page's button ran the other's handler.
pub type Handlers = std::collections::BTreeMap<
    (usize, crate::hir::DeclId, crate::hir::ExprId),
    (String, Vec<String>),
>;

/// Build the template IR for every renderable declaration in a program.
///
/// Without handler identities: an `Event` part gets an empty handler, which is
/// honest for a caller that has no signatures to derive one from.
pub fn build(hirs: &[&Hir]) -> Vec<Template> {
    let ws = Workspace::build(hirs);
    build_with(hirs, &Signatures::build(&ws, hirs), &Handlers::new())
}

/// Build with the handler identities the resume artifacts derived.
///
/// The identity is derived ONCE, in `resume_artifacts`, and read here. A second
/// derivation could disagree with the manifest the runtime compares against,
/// and the disagreement would be silent: `decide` would refuse a handler that
/// is in fact the right one, and the page would simply not respond.
pub fn build_with(hirs: &[&Hir], sigs: &Signatures, handlers: &Handlers) -> Vec<Template> {
    // The views a template uses are found by name, from the file it is
    // written in (ADR-0136).
    let ws = Workspace::build(hirs);
    let mut out = Vec::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            use crate::hir::DeclKind::*;
            if !matches!(decl.kind, View | Component | Page) {
                continue;
            }
            if let Some(l) = lowered(hirs, &ws, sigs, handlers, unit, id) {
                out.push((l.template, l.deepest));
            }
        }
    }
    // Each instance, how deep its view's template nests (ADR-0203).
    let deepest: BTreeMap<String, u32> = out.iter().map(|(t, d)| (t.path.clone(), *d)).collect();
    let mut out: Vec<Template> = out.into_iter().map(|(t, _)| t).collect();
    for t in &mut out {
        fill_deepest(&mut t.chunks, &deepest);
    }
    close_schemas(&mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// **Each layout, lowered alone** (ADR-0303): its markup, its slot empty.
/// Not a template the build writes (a layout is rendered in each page that
/// names it), but one the build refuses as it refuses a page's: a part the
/// renderer cannot render in a layout no page names yet is still a defect.
pub fn layouts(hirs: &[&Hir], sigs: &Signatures, handlers: &Handlers) -> Vec<Template> {
    let ws = Workspace::build(hirs);
    let mut out = Vec::new();
    for (unit, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != crate::hir::DeclKind::Layout {
                continue;
            }
            if let Some(l) = lowered(hirs, &ws, sigs, handlers, unit, id) {
                out.push(l.template);
            }
        }
    }
    out
}

/// Each instance's template, by path, in `chunks` at any depth.
fn instance_paths(chunks: &[Chunk], out: &mut BTreeSet<String>) {
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        if let Part::Instance { path, .. } = p {
            out.insert(path.clone());
        }
        for nested in p.nested() {
            instance_paths(nested, out);
        }
    }
}

/// **Each template's schema, over every template its instances reach**
/// (ADR-0203). The parts inside an instance are numbered by its view's own
/// template, so a change there changes what the page's addresses mean, as a
/// change to the page's own markup does.
fn close_schemas(templates: &mut [Template]) {
    let own: BTreeMap<String, String> = templates
        .iter()
        .map(|t| (t.path.clone(), t.schema.clone()))
        .collect();
    let reaches: BTreeMap<String, BTreeSet<String>> = templates
        .iter()
        .map(|t| {
            let mut paths = BTreeSet::new();
            instance_paths(&t.chunks, &mut paths);
            (t.path.clone(), paths)
        })
        .collect();
    let feed = |h: &mut u64, bytes: &[u8]| {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for t in templates.iter_mut() {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = reaches[&t.path].iter().collect();
        while let Some(p) = stack.pop() {
            if seen.insert(p) {
                stack.extend(reaches.get(p).into_iter().flatten());
            }
        }
        if seen.is_empty() {
            continue;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        feed(&mut h, own[&t.path].as_bytes());
        for p in seen {
            feed(&mut h, p.as_bytes());
            feed(&mut h, own.get(p).map(String::as_bytes).unwrap_or_default());
        }
        t.schema = format!("{h:016x}");
    }
}

/// Each instance in `chunks`, at any depth, told how deep its view's
/// template nests (ADR-0203).
fn fill_deepest(chunks: &mut [Chunk], deepest: &BTreeMap<String, u32>) {
    for c in chunks {
        let Chunk::Dynamic(p) = c else { continue };
        if let Part::Instance {
            path, deepest: d, ..
        } = p
        {
            *d = deepest.get(path.as_str()).copied().unwrap_or_default();
        }
        for nested in p.nested_mut() {
            fill_deepest(nested, deepest);
        }
    }
}

/// One declaration's template, as one traversal lowered it.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub template: Template,
    /// Each text part and the expression it shows (ADR-0122).
    pub holes: Vec<Hole>,
    /// **Each value it reads outside a text part** (ADR-0169).
    pub reads: Vec<Read>,
    /// **Each view composed into it**, through the views it uses (ADR-0136):
    /// whose handlers are in its document.
    pub views: Vec<DefId>,
    /// **Each signal it holds** (ADR-0144).
    pub instances: Vec<Instance>,
    /// The most elements any of its parts nests in (ADR-0203).
    pub deepest: u32,
    /// **The layout it is shown in** (ADR-0303), for a page that names one.
    pub layout: Option<PageLayout>,
}

/// **One renderable declaration's template, and its text holes** (ADR-0122),
/// numbered by one traversal: the one [`build_with`] numbers them by, with
/// each view it uses composed in place (ADR-0136).
pub fn lowered(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    handlers: &Handlers,
    unit: usize,
    id: DeclId,
) -> Option<Lowered> {
    let hir = hirs.get(unit)?;
    let decl = hir.decl(id);
    let body = hir.body(decl.body?);
    let mut chunks = Vec::new();
    let mut ix = Indexer {
        template: crate::contract::component_id(hir, id),
        ..Indexer::default()
    };
    // Its own signals, and the signals it provides, by the names it writes:
    // the outermost body's names are its own (ADR-0144).
    let mut provided = BTreeMap::new();
    let mut signals = BTreeSet::new();
    for (name, init, declared) in crate::page_values::signals_of(body) {
        if let Expr::Let { ty: Some(t), .. } = body.expr(declared) {
            ix.instances.push(Instance {
                name: name.clone(),
                origin: (unit, id),
                init,
                ty: InstanceType::Written(*t),
                within: Vec::new(),
            });
        }
        signals.insert(name);
    }
    for (name, signal, value) in provides_of(hirs, ws, unit, body) {
        ix.instances.push(Instance {
            name: name.clone(),
            origin: (unit, id),
            init: value,
            ty: InstanceType::Declared(signal),
            within: Vec::new(),
        });
        provided.insert(signal, name.clone());
        signals.insert(name);
    }
    // **And each signal the device keeps that it, its layout or a view it
    // composes names** (track `device-store`, ADR-0366): one value for the
    // page, under the name it is declared with, its first value the
    // declaration's default, which the server renders and the browser
    // replaces with the device's.
    if decl.kind == crate::hir::DeclKind::Page {
        for (name, signal) in crate::device_store::held_by(hirs, ws, sigs, unit, id) {
            if signals.contains(&name) {
                continue;
            }
            let Some(init) = crate::resolve::declaration(hirs, signal)
                .and_then(|d| d.body)
                .map(|b| hirs[signal.unit].body(b).root)
            else {
                continue;
            };
            ix.instances.push(Instance {
                name: name.clone(),
                origin: (signal.unit, DeclId(signal.decl)),
                init,
                ty: InstanceType::Declared(signal),
                within: Vec::new(),
            });
            provided.insert(signal, name.clone());
            signals.insert(name);
        }
    }
    // The names its markup can read outside any block (ADR-0310): its
    // parameters, its query bindings and its signals.
    let around: BTreeSet<String> = decl
        .params
        .iter()
        .map(|p| p.name.clone())
        .chain(
            crate::page_values::query_bindings(ws, unit, body)
                .into_iter()
                .map(|(name, ..)| name),
        )
        .chain(signals.iter().cloned())
        .collect();
    let ctx = Lowering {
        handlers,
        hirs,
        ws,
        types: std::rc::Rc::new(crate::infer::Types::of_decl(sigs, hir, id, body)),
        sigs,
        unit,
        decl: id,
        names: asked_predicates(hirs, ws, sigs, unit),
        around,
        in_layout: false,
        provided,
        signals,
    };
    // A layout alone, as the build checks one no page names: its markup,
    // its slot empty, its values named as a page names them (ADR-0303).
    if decl.kind == crate::hir::DeclKind::Layout {
        let def = DefId { unit, decl: id.0 };
        ix.instances.clear();
        let layout = lower_layout(hirs, ws, sigs, handlers, def, &mut ix, &mut chunks);
        let chunks = coalesce(chunks);
        return Some(Lowered {
            template: Template {
                path: ix.template.clone(),
                name: decl.name.clone(),
                schema: schema_of(&[], &chunks),
                params: Vec::new(),
                chunks,
            },
            holes: ix.holes,
            reads: ix.reads,
            views: ix.views,
            instances: ix.instances,
            deepest: ix.deepest,
            layout,
        });
    }
    // A page's title is lowered after the rest of its view, wherever it is
    // written, so writing one moves no other part's number (ADR-0183).
    // Its metadata after its title, for the same reason (ADR-0186).
    // And its canonical address after its metadata (ADR-0311).
    let page = decl.kind == crate::hir::DeclKind::Page;
    let canonical = crate::canonical::page_link(decl, body);
    let (head, rest): (Vec<NodeId>, Vec<NodeId>) = roots_of(body)
        .into_iter()
        .partition(|r| page && (is_title(body, *r) || is_meta(body, *r) || canonical == Some(*r)));
    let (titles, metas): (Vec<NodeId>, Vec<NodeId>) = head
        .into_iter()
        .filter(|r| canonical != Some(*r))
        .partition(|r| is_title(body, *r));
    // **The layout the page is shown in** (ADR-0303): its markup lowered
    // first, so its parts and elements are numbered the same on every page
    // that names it, and the page's own after them, placed in its slot.
    let layout = page
        .then(|| crate::layouts::of(hirs, ws, unit, decl))
        .flatten()
        .and_then(|def| lower_layout(hirs, ws, sigs, handlers, def, &mut ix, &mut chunks));
    let slot = ix.slot.take();
    let mut own = Vec::new();
    if let Some(s) = slot {
        ix.elements = s.elements;
    }
    for root in rest {
        lower_node(body, root, &ctx, &mut ix, &mut own);
    }
    ix.elements = 0;
    match (slot, &layout) {
        (Some(s), _) => {
            chunks.splice(s.at..s.at, own);
        }
        // Refused at check (PW5045): a layout that shows no page.
        (None, Some(l)) => chunks.push(Chunk::Dynamic(Part::Blocked {
            reason: format!(
                "the layout `{}` has no `<slot />` to show the page in",
                l.path
            ),
            at: "layout".to_string(),
        })),
        (None, None) => chunks.extend(own),
    }
    for title in titles {
        chunks.push(Chunk::Dynamic(lower_title(body, title, &ctx, &mut ix)));
    }
    for meta in metas {
        chunks.push(Chunk::Dynamic(lower_meta(body, meta, &ctx, &mut ix)));
    }
    if let Some(link) = canonical {
        chunks.push(Chunk::Dynamic(lower_canonical(
            hirs, ws, unit, body, link, &ctx, &mut ix,
        )));
    }
    let chunks = coalesce(chunks);
    let params: Vec<String> = decl.params.iter().map(|p| p.name.clone()).collect();
    let template = Template {
        path: crate::contract::component_id(hir, id),
        name: decl.name.clone(),
        schema: schema_of(&params, &chunks),
        params,
        chunks,
    };
    Some(Lowered {
        template,
        holes: ix.holes,
        reads: ix.reads,
        views: ix.views,
        instances: ix.instances,
        deepest: ix.deepest,
        layout,
    })
}

/// **The predicates a body in `unit` asks of its reader** (ADR-0307), each
/// by the name the template writes and the one it is read by, `SignedIn` as
/// `SignedIn~holds`, which no source can write: a host answers each for the
/// document, as it answers `requires`. A predicate with parameters is asked of
/// nothing yet (PW0629).
fn asked_predicates(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    unit: usize,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (u, hir) in hirs.iter().enumerate() {
        for (id, decl) in hir.all_decls() {
            if decl.kind != crate::hir::DeclKind::Predicate {
                continue;
            }
            let def = DefId {
                unit: u,
                decl: id.0,
            };
            let seen = matches!(
                ws.resolve_in(unit, crate::resolve::Namespace::Predicate, &decl.name),
                Resolution::Local(d) | Resolution::Imported { def: d, .. } if d == def
            );
            if seen && sigs.by_def(def).is_some_and(|s| s.params.is_empty()) {
                out.insert(decl.name.clone(), asked_name(&decl.name));
            }
        }
    }
    out
}

/// The name a template reads a predicate's answer by.
pub fn asked_name(predicate: &str) -> String {
    format!("{predicate}~holds")
}

/// **A page's layout, lowered into its template first** (ADR-0303), as a
/// view used in it is (ADR-0136): its bindings and its signals the page's,
/// each under the name [`crate::layouts::bound`] gives it, which no source
/// writes; its handlers the page's; its computed parts named by the layout,
/// not the page. Its `<slot />` is recorded in `ix`, for the page's markup.
fn lower_layout(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    handlers: &Handlers,
    def: DefId,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) -> Option<PageLayout> {
    let hir = hirs.get(def.unit)?;
    let id = DeclId(def.decl);
    let decl = hir.decl(id);
    let body = hir.body(decl.body?);
    let path = crate::contract::component_id(hir, id);
    let mut names = asked_predicates(hirs, ws, sigs, def.unit);
    for (name, _, _) in crate::page_values::query_bindings(ws, def.unit, body) {
        names.insert(name.clone(), crate::layouts::bound(&decl.name, &name));
    }
    let mut provided = BTreeMap::new();
    let mut signals = BTreeSet::new();
    for (name, init, declared) in crate::page_values::signals_of(body) {
        let fresh = crate::layouts::bound(&decl.name, &name);
        if let Expr::Let { ty: Some(t), .. } = body.expr(declared) {
            ix.instances.push(Instance {
                name: fresh.clone(),
                origin: (def.unit, id),
                init,
                ty: InstanceType::Written(*t),
                within: Vec::new(),
            });
        }
        names.insert(name.clone(), fresh);
        signals.insert(name);
    }
    for (name, signal, value) in provides_of(hirs, ws, def.unit, body) {
        let fresh = crate::layouts::bound(&decl.name, &name);
        ix.instances.push(Instance {
            name: fresh.clone(),
            origin: (def.unit, id),
            init: value,
            ty: InstanceType::Declared(signal),
            within: Vec::new(),
        });
        names.insert(name.clone(), fresh.clone());
        provided.insert(signal, fresh);
        signals.insert(name);
    }
    // Its handlers are in the page's document (ADR-0136's rule for a view).
    if !ix.views.contains(&def) {
        ix.views.push(def);
    }
    let ctx = Lowering {
        handlers,
        hirs,
        ws,
        types: std::rc::Rc::new(crate::infer::Types::of_decl(sigs, hir, id, body)),
        sigs,
        unit: def.unit,
        decl: id,
        names,
        around: BTreeSet::new(),
        in_layout: true,
        provided,
        signals,
    };
    let page = std::mem::replace(&mut ix.template, path.clone());
    ix.layout = Some((def.unit, id));
    let start = out.len();
    for root in roots_of(body) {
        lower_node(body, root, &ctx, ix, out);
    }
    ix.layout = None;
    ix.template = page;
    Some(PageLayout {
        path,
        schema: schema_of(&[], &coalesce(out[start..].to_vec())),
        parts: ix.next_part,
        elements: ix.next_element,
    })
}

/// **Each text part of a renderable declaration, with its hole's expression**
/// (ADR-0122), numbered by the traversal [`build_with`] numbers them by.
pub fn holes(
    hirs: &[&Hir],
    ws: &Workspace,
    sigs: &Signatures,
    unit: usize,
    decl: DeclId,
) -> Vec<Hole> {
    lowered(hirs, ws, sigs, &Handlers::new(), unit, decl)
        .map(|l| l.holes)
        .unwrap_or_default()
}

/// What lowering needs beyond the body: which declaration it is in, the
/// handler identities derived for it, and the program its views are in.
#[derive(Clone)]
struct Lowering<'a> {
    handlers: &'a Handlers,
    hirs: &'a [&'a Hir],
    ws: &'a Workspace,
    sigs: &'a Signatures,
    /// **What the checker types in this body** (ADR-0232): an opaque value's
    /// `.value` is read by the opaque value's own path. Shared by the copies a
    /// scope makes of this context.
    types: std::rc::Rc<crate::infer::Types<'a>>,
    /// Which file, by its index in the program, and which declaration in it:
    /// the body the nodes being lowered are written in.
    unit: usize,
    decl: DeclId,
    /// **How a name this body writes is read where the template renders it**
    /// (ADR-0136): a composed view's parameter as the path it was given, and
    /// a name a block binds renamed where the template could already read
    /// one of its spelling (see `around`). Empty at the top of a
    /// declaration's own markup, whose names are its own.
    names: BTreeMap<String, String>,
    /// **The names the template can read here** (ADR-0310), as it writes
    /// them: the page's parameters, its query bindings and its signals, and
    /// the binding of each block around the node. A block that binds one of
    /// these is given a name no source writes, as a composed view's is
    /// (ADR-0136), so each name the template reads is one binding's: until
    /// 2026-10-10 the page plan read a loop's `line` as the page's binding
    /// `line` (ADR-0277's limitation).
    around: BTreeSet<String>,
    /// **In a layout's markup** (ADR-0310), every block's binding is renamed:
    /// the markup is the same on every page that names the layout, whatever
    /// the page binds, and a soft navigation compares it by its schema.
    in_layout: bool,
    /// **The instance each provided signal is, here** (ADR-0144): the
    /// nearest `provide` around this body, by the signal's declaration.
    provided: BTreeMap<DefId, String>,
    /// The names this body writes that are signals: its own, the ones it
    /// provides, and the ones provided around it. A handler's module names
    /// each as the body writes it, and the element says which instance it is.
    signals: BTreeSet<String>,
}

impl<'a> Lowering<'a> {
    /// **The path the template reads `e` by** (ADR-0073), an opaque value's
    /// `.value` by the opaque value's own (ADR-0232). `None` for anything
    /// computed.
    fn path(&self, body: &Body, e: ExprId) -> Option<String> {
        let at = (self.hirs[self.unit], self.unit, self.decl);
        path_where(body, e, &|x| {
            is_opaque((self.sigs, self.ws), at, &self.types, body, x)
        })
    }

    /// `path` as the template reads it: its root as this scope names it.
    fn read(&self, path: String) -> String {
        let (root, rest) = match path.split_once('.') {
            Some((root, rest)) => (root, Some(rest)),
            None => (path.as_str(), None),
        };
        match (self.names.get(root), rest) {
            (Some(to), Some(rest)) => format!("{to}.{rest}"),
            (Some(to), None) => to.clone(),
            (None, _) => path,
        }
    }

    /// **The scope inside a block that binds `bound`**, and each name as
    /// the template binds it (ADR-0136). A bound name hides a parameter, or
    /// a signal, of its own name. One the template could already read here
    /// would hide that, where the page plan reads the template by name: a
    /// loop's `line` beside the page's binding `line`, or, in a composed
    /// view, `{#each item.tags as entry}` given `item={entry}`, which would
    /// read the tag where it meant the page's `entry`. Such a name is
    /// renamed, to one no source can write (ADR-0310), as is every one a
    /// layout binds.
    fn binding(&self, bound: &[String], ix: &mut Indexer) -> (Lowering<'a>, Vec<String>) {
        let mut inner = self.clone();
        for b in bound {
            inner.names.remove(b);
            inner.signals.remove(b);
        }
        let mut written = Vec::new();
        for b in bound {
            let hides = self.in_layout
                || self.around.contains(b)
                || inner
                    .names
                    .values()
                    .any(|p| p.split('.').next() == Some(b.as_str()));
            let name = if hides {
                let fresh = ix.rename(b);
                inner.names.insert(b.clone(), fresh.clone());
                fresh
            } else {
                b.clone()
            };
            inner.around.insert(name.clone());
            written.push(name);
        }
        (inner, written)
    }

    /// The view a capitalised tag names, from the file it is written in.
    fn view_named(&self, tag: &str) -> Option<DefId> {
        if !tag.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        match self
            .ws
            .resolve_in(self.unit, crate::resolve::Namespace::Ui, tag)
        {
            Resolution::Local(def) | Resolution::Imported { def, .. } => {
                crate::resolve::declaration(self.hirs, def)
                    .is_some_and(|d| d.kind == crate::hir::DeclKind::View)
                    .then_some(def)
            }
            _ => None,
        }
    }
}

/// **The views a view's markup uses** (ADR-0203), each with whether a
/// block, `{#if}`, `{#match}` or `{#each}`, encloses the use, resolved as
/// [`Lowering::view_named`] resolves a tag.
fn view_uses(ws: &Workspace, hirs: &[&Hir], def: DefId) -> Vec<(DefId, bool)> {
    let Some(body) = crate::resolve::declaration(hirs, def)
        .and_then(|d| d.body)
        .map(|b| hirs[def.unit].body(b))
    else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, bool)> = roots.iter().rev().map(|r| (*r, false)).collect();
    while let Some((n, in_block)) = stack.pop() {
        let kids: Vec<(NodeId, bool)> = match body.node(n) {
            Node::Element { tag, children, .. } => {
                if tag.starts_with(|c: char| c.is_ascii_uppercase())
                    && let Resolution::Local(d) | Resolution::Imported { def: d, .. } =
                        ws.resolve_in(def.unit, crate::resolve::Namespace::Ui, tag)
                    && crate::resolve::declaration(hirs, d)
                        .is_some_and(|x| x.kind == crate::hir::DeclKind::View)
                {
                    out.push((d, in_block));
                }
                children.iter().map(|c| (*c, in_block)).collect()
            }
            Node::Block { children, .. } => children.iter().map(|c| (*c, true)).collect(),
            _ => Vec::new(),
        };
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// Does `from` reach `target` through the views it uses, every one of them,
/// or, with `blocks` false, only those no block encloses?
fn view_reaches(
    ws: &Workspace,
    hirs: &[&Hir],
    from: DefId,
    target: DefId,
    blocks: bool,
    seen: &mut BTreeSet<DefId>,
) -> bool {
    if !seen.insert(from) {
        return false;
    }
    view_uses(ws, hirs, from)
        .into_iter()
        .filter(|(_, in_block)| blocks || !in_block)
        .any(|(d, _)| d == target || view_reaches(ws, hirs, d, target, blocks, seen))
}

/// **Does a body's markup show a query's state**, in a `<stream>`
/// (ADR-0148)? A host runs the streams its page's plan names, so a view that
/// contains itself shows none yet (ADR-0203).
pub(crate) fn shows_a_stream(body: &crate::hir::Body) -> bool {
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    body.walk_markup(&roots)
        .into_iter()
        .any(|n| matches!(body.node(n), Node::Element { tag, .. } if tag == "stream"))
}

/// **The values a body's markup computes** (ADR-0226): each text hole, each
/// attribute's value and each hole in an attribute's text, each condition,
/// each `{#match}` subject and each prop a view is given, that is no path.
/// Not what the template does not write: an event's handler and any other
/// directive, a stream's query and the name its part binds, and a mounted
/// resource's arguments, as the value relations read them (ADR-0074).
pub(crate) fn computed_values(body: &Body) -> Vec<ExprId> {
    let mut out = template_values(body);
    out.retain(|e| value_path(body, *e).is_none());
    out
}

/// **Each value a template's markup reads**: a text part's, a block's
/// subject, a branch's condition, and each attribute's that is no directive,
/// each part of an interpolated one.
fn template_values(body: &Body) -> Vec<ExprId> {
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    let mut out = Vec::new();
    for n in body.walk_markup(&roots) {
        match body.node(n) {
            Node::Interpolation(e)
            | Node::Block {
                subject: Some(e), ..
            }
            | Node::Branch {
                condition: Some(e), ..
            } => out.push(*e),
            Node::Element { tag, attrs, .. } if !attrs.iter().any(mounts) => {
                for a in attrs {
                    let AttrValue::Expr(e) = &a.value else {
                        continue;
                    };
                    let directive = a
                        .name
                        .split_once(':')
                        .is_some_and(|(prefix, _)| !matches!(prefix, "xml" | "xlink" | "xmlns"));
                    if directive
                        || (tag == "stream" && a.name == "query")
                        || (matches!(tag.as_str(), "ready" | "failed") && a.name == "as")
                    {
                        continue;
                    }
                    match body.expr(*e) {
                        Expr::Interpolated { parts, .. } => out.extend(parts.iter().copied()),
                        _ => out.push(*e),
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// **The first value a view's markup reads through a member function**, by
/// its path (ADR-0169): `None` where it reads each by field.
fn member_read(ctx: &Lowering<'_>, def: DefId, body: &Body) -> Option<String> {
    let mut typed = BTreeMap::new();
    template_values(body).into_iter().find_map(|e| {
        let path = value_path(body, e)?;
        crate::page_values::calls_a_member(
            ctx.hirs,
            ctx.ws,
            ctx.sigs,
            &mut typed,
            (def.unit, DeclId(def.decl)),
            ReadAt::Expr(e),
        )
        .then_some(path)
    })
}

/// **Does a view contain itself** (ADR-0203), through the views it uses?
/// Each use of one is an instance made at run time (ADR-0130, ruling 2).
pub(crate) fn contains_itself(ws: &Workspace, hirs: &[&Hir], def: DefId) -> bool {
    view_reaches(ws, hirs, def, def, true, &mut BTreeSet::new())
}

/// **Does a view contain itself with no block between** (ADR-0203): a
/// cycle of uses no `{#if}`, `{#match}` or `{#each}` encloses, which no data
/// ends?
pub(crate) fn contains_itself_endlessly(ws: &Workspace, hirs: &[&Hir], def: DefId) -> bool {
    view_reaches(ws, hirs, def, def, false, &mut BTreeSet::new())
}

/// **Is a view's body what composition writes in place?** Its markup
/// (ADR-0136), and the signals it holds and provides, of which each use holds
/// its own (ADR-0144). A view that binds other values of its own would need
/// them where it is used, and they are not there.
pub(crate) fn composable(body: &Body) -> bool {
    match body.expr(body.root) {
        Expr::Block { stmts } => stmts.iter().all(|s| {
            matches!(body.expr(*s), Expr::Template { .. })
                || body.signals.contains(s)
                || body.provides.contains(s)
        }),
        Expr::Template { .. } => true,
        _ => false,
    }
}

/// **What a body provides** (ADR-0144): each `provide`'s name as written,
/// the module signal it names, and its value. A `provide` of anything else
/// is PW5306's to refuse, and is left out.
pub(crate) fn provides_of(
    hirs: &[&Hir],
    ws: &Workspace,
    unit: usize,
    body: &Body,
) -> Vec<(String, DefId, ExprId)> {
    let mut out = Vec::new();
    for id in &body.provides {
        let Expr::Binary { lhs, rhs, .. } = body.expr(*id) else {
            continue;
        };
        let Expr::Name(name) = body.expr(*lhs) else {
            continue;
        };
        if let Some(signal) = module_signal(hirs, ws, unit, name) {
            out.push((name.clone(), signal, *rhs));
        }
    }
    out
}

/// **The module signals a body names**, each once, by the name it writes
/// (ADR-0144).
pub(crate) fn signals_named(
    hirs: &[&Hir],
    ws: &Workspace,
    unit: usize,
    body: &Body,
) -> Vec<(String, DefId)> {
    let mut out: Vec<(String, DefId)> = Vec::new();
    for (_, e, _) in body.exprs() {
        let Expr::Name(name) = e else { continue };
        if out.iter().any(|(n, _)| n == name) {
            continue;
        }
        if let Some(signal) = module_signal(hirs, ws, unit, name) {
            out.push((name.clone(), signal));
        }
    }
    out
}

/// The signal a module declares that `name` resolves to, where it is written.
pub(crate) fn module_signal(
    hirs: &[&Hir],
    ws: &Workspace,
    unit: usize,
    name: &str,
) -> Option<DefId> {
    match ws.resolve(unit, name) {
        Resolution::Local(def) | Resolution::Imported { def, .. } => {
            crate::resolve::declaration(hirs, def)
                .is_some_and(|d| d.kind == crate::hir::DeclKind::Signal)
                .then_some(def)
        }
        _ => None,
    }
}

/// The name of the thing a handler lambda calls.
///
/// `on:press={resumable(..) => add_to_cart(..)}` is `add_to_cart`, and so is
/// a handler that matches on what the call answered, `=> match add_to_cart(..)
/// { .. }` (ADR-0157), or discards it by name, `=> { let _added =
/// add_to_cart(..) }` (ADR-0159): what it does with the answer is how it
/// reacts, and the behaviour is still the call's. In a block, the first
/// statement of one of those forms. Empty when the handler is not a lambda
/// that calls a named thing — in which case there is no separately loadable
/// behaviour to name, and the runtime says so rather than guessing.
pub(crate) fn called_name(body: &Body, expr: crate::hir::ExprId) -> String {
    match body.expr(expr) {
        Expr::Lambda { body: inner, .. } => named_call(body, *inner).unwrap_or_default(),
        // A declaration named as the handler, `on:submit={save}`, is what it
        // calls, as `(e) => save(e)` is; through its module, `forms.save`, it
        // is named as `(e) => forms.save(e)` is, by nothing (ADR-0199).
        Expr::Name(n) => n.clone(),
        _ => String::new(),
    }
}

/// The name a call is made by, where `e` is the call, a `match` on it, a
/// binding of it, or a block with one of those as a statement: the first.
fn named_call(body: &Body, e: crate::hir::ExprId) -> Option<String> {
    match body.expr(e) {
        Expr::Call { callee, .. } => match body.expr(*callee) {
            Expr::Name(n) => Some(n.clone()),
            _ => None,
        },
        Expr::Match { scrutinee, .. } => named_call(body, *scrutinee),
        Expr::Let { init: Some(i), .. } => named_call(body, *i),
        Expr::Block { stmts } => stmts.iter().find_map(|s| named_call(body, *s)),
        _ => None,
    }
}

pub(crate) fn roots_of(body: &Body) -> Vec<NodeId> {
    let mut roots = Vec::new();
    for e in body.walk() {
        if let Expr::Template { roots: r, .. } = body.expr(e) {
            roots.extend(r.iter().copied());
        }
    }
    roots
}

/// Whether a node is a `<title>` element: exactly, as `<Title>` is a view
/// (ADR-0072).
fn is_title(body: &Body, n: NodeId) -> bool {
    matches!(body.node(n), Node::Element { tag, .. } if tag == "title")
}

/// Whether a node is the page's metadata: a `<meta>` that is not an item's
/// property (microdata), which is written where it is (ADR-0186).
fn is_meta(body: &Body, n: NodeId) -> bool {
    matches!(body.node(n), Node::Element { tag, attrs, .. }
        if tag == "meta" && !attrs.iter().any(|a| a.name == "itemprop"))
}

/// **A page's metadata** (ADR-0186): what it names, `name` or `property`, as
/// written, and its content as text and the values it reads. Anything else
/// is refused at check (PW5034), and blocked here.
fn lower_meta(body: &Body, n: NodeId, ctx: &Lowering<'_>, ix: &mut Indexer) -> Part {
    let blocked = |reason: &str| Part::Blocked {
        reason: reason.to_string(),
        at: "<meta>".to_string(),
    };
    let Node::Element { attrs, .. } = body.node(n) else {
        return blocked("metadata is an element");
    };
    let attr = |name: &str| attrs.iter().find(|a| a.name == name);
    // Its name and its content, and nothing the head would not carry.
    if attrs
        .iter()
        .any(|a| !["name", "property", "content"].contains(&a.name.as_str()))
    {
        return blocked("metadata is its name and its content (PW5034)");
    }
    let named: Vec<(&str, &crate::hir::Attr)> = ["name", "property"]
        .into_iter()
        .filter_map(|k| attr(k).map(|a| (k, a)))
        .collect();
    let [(attribute, a)] = named.as_slice() else {
        return blocked("metadata names what it describes once (PW5034)");
    };
    let AttrValue::Static(v) = &a.value else {
        return blocked("metadata names what it describes as text (PW5034)");
    };
    let key = unquote(v).to_string();
    let mut content = Vec::new();
    let mut values = Vec::new();
    match attr("content").map(|a| &a.value) {
        Some(AttrValue::Static(v)) => content.push(TitlePiece::Text(unquote(v).to_string())),
        Some(AttrValue::Expr(e)) => match body.expr(*e) {
            Expr::Interpolated { text, parts } => {
                let mut holes = parts.iter();
                let mut rest = unquote(text);
                while let Some(open) = rest.find('{') {
                    let after = &rest[open + 1..];
                    let Some(close) = after.find('}') else { break };
                    if open > 0 {
                        content.push(TitlePiece::Text(rest[..open].to_string()));
                    }
                    let Some(path) = holes
                        .next()
                        .and_then(|h| ctx.path(body, *h).map(|p| (p, *h)))
                    else {
                        return blocked("a value in metadata is read by its path");
                    };
                    let read = ctx.read(path.0);
                    values.push((read.clone(), path.1));
                    content.push(TitlePiece::Value(read));
                    rest = &after[close + 1..];
                }
                if !rest.is_empty() {
                    content.push(TitlePiece::Text(rest.to_string()));
                }
            }
            _ => {
                let Some(path) = ctx.path(body, *e) else {
                    return blocked("a value in metadata is read by its path");
                };
                let read = ctx.read(path);
                values.push((read.clone(), *e));
                content.push(TitlePiece::Value(read));
            }
        },
        _ => return blocked("metadata has content (PW5034)"),
    }
    let id = ix.part();
    for (path, e) in values {
        ix.read(id, &path, ReadKind::Meta, ReadAt::Expr(e), ctx);
    }
    Part::Meta {
        id,
        attribute: attribute.to_string(),
        key,
        content,
    }
}

/// **A page's canonical address** (ADR-0311): the route of the page its
/// `href` names, each `{name}` of it given the argument its parameter takes,
/// text or a value the page reads, by its path. Anything else is refused at
/// check (PW5035, PW5051, PW0604), and blocked here.
fn lower_canonical(
    hirs: &[&Hir],
    ws: &Workspace,
    unit: usize,
    body: &Body,
    n: NodeId,
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
) -> Part {
    let blocked = |reason: &str| Part::Blocked {
        reason: reason.to_string(),
        at: "<link rel=\"canonical\">".to_string(),
    };
    let Some(e) = crate::canonical::address(body, n) else {
        return blocked("a canonical address is a page's (PW5051)");
    };
    let Expr::Address { page: name, args } = body.expr(e) else {
        return blocked("a canonical address is a page's (PW5051)");
    };
    let Some((def, page)) = crate::canonical::page_named(ws, unit, name).and_then(|d| {
        crate::resolve::declaration(hirs, d)
            .filter(|p| p.kind == crate::hir::DeclKind::Page)
            .map(|p| (d, p))
    }) else {
        return blocked("a canonical address names a page (PW5051)");
    };
    let Some(route) = crate::routes::declared_route(hirs[def.unit], page) else {
        return blocked("a canonical address names a page served at a route (PW5051)");
    };
    if route.contains("{?") {
        return blocked("a canonical address with a query is not claimed (PW5051)");
    }
    let params: Vec<&str> = page.params.iter().map(|p| p.name.as_str()).collect();
    if args.len() != params.len() {
        return blocked("a page's address gives each of its parameters (PW0604)");
    }
    // Each argument by the parameter it gives: its name where one is
    // written, its place otherwise.
    let mut given: BTreeMap<String, (TitlePiece, Option<(String, ExprId)>)> = BTreeMap::new();
    for (i, a) in args.iter().enumerate() {
        let param = a.name.clone().unwrap_or_else(|| params[i].to_string());
        let piece = match body.expr(a.value) {
            Expr::Literal(l @ crate::hir::Literal::Str(_)) => match l.string_value() {
                Some(t) => (TitlePiece::Text(t), None),
                None => return blocked("an argument is text, or a value the page reads"),
            },
            _ => {
                let Some(path) = ctx.path(body, a.value) else {
                    return blocked("a value in a canonical address is read by its path (PW5051)");
                };
                let read = ctx.read(path);
                (TitlePiece::Value(read.clone()), Some((read, a.value)))
            }
        };
        given.insert(param, piece);
    }
    // In the route's order, each `{name}` given.
    let mut filled = Vec::new();
    let mut values = Vec::new();
    for hole in crate::routes::route_holes(&route) {
        let Some((piece, value)) = given.remove(hole.as_str()) else {
            return blocked("a page's address gives each of its route's parameters (PW0604)");
        };
        values.extend(value);
        filled.push((hole, piece));
    }
    let id = ix.part();
    for (path, e) in values {
        ix.read(id, &path, ReadKind::Meta, ReadAt::Expr(e), ctx);
    }
    Part::Canonical {
        id,
        route,
        args: filled,
    }
}

/// **A page's title** (ADR-0183): its text as written, and each value it
/// reads by its path. Anything else in it is refused at check (PW5030), and
/// blocked here.
fn lower_title(body: &Body, n: NodeId, ctx: &Lowering<'_>, ix: &mut Indexer) -> Part {
    let blocked = |reason: &str| Part::Blocked {
        reason: reason.to_string(),
        at: "<title>".to_string(),
    };
    let Node::Element { children, .. } = body.node(n) else {
        return blocked("a title is an element");
    };
    let mut pieces = Vec::new();
    let mut values = Vec::new();
    for c in children {
        match body.node(*c) {
            Node::Text(t) => pieces.push(TitlePiece::Text(t.clone())),
            Node::Interpolation(e) => {
                let Some(path) = ctx.path(body, *e) else {
                    return blocked("a value in a title is read by its path");
                };
                let path = ctx.read(path);
                values.push((path.clone(), *e));
                pieces.push(TitlePiece::Value(path));
            }
            _ => return blocked("a title holds text and values (PW5030)"),
        }
    }
    let id = ix.part();
    for (path, e) in values {
        ix.read(id, &path, ReadKind::Title, ReadAt::Expr(e), ctx);
    }
    Part::Title { id, pieces }
}

/// Adjacent `Static` chunks become one.
///
/// Not an optimisation: it is what makes the output deterministic to compare.
/// `["<p>", ">"]` and `["<p>>"]` are the same document and would otherwise be
/// different IR depending on how the source happened to be split.
fn coalesce(chunks: Vec<Chunk>) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for c in chunks {
        match (out.last_mut(), c) {
            (Some(Chunk::Static(prev)), Chunk::Static(next)) => prev.push_str(&next),
            (_, c) => out.push(c),
        }
    }
    out
}

/// **The path a template reads a value by**: a name, or fields read from one.
///
/// `None` for anything computed. The renderer looks each value up by its path
/// among the values it is given, and a computation has none. Until 2026-09-26
/// a text hole or an attribute that was not a path lowered with an empty path,
/// and `f(x).name` with the path `.name`, so every render failed; a program
/// that wrote one now fails to build instead (ADR-0073).
fn value_path(body: &Body, e: ExprId) -> Option<String> {
    path_where(body, e, &|_| false)
}

/// [`value_path`], where `representation` says which `.value` reads an
/// opaque value's representation (ADR-0232): the same value at run time, so
/// read by the opaque value's own path. Until 2026-10-06 `{p.id.value}` was
/// read as a field of the text, and every request for its page was answered
/// 503. A record's field named `value` is a field.
fn path_where(body: &Body, e: ExprId, representation: &dyn Fn(ExprId) -> bool) -> Option<String> {
    match body.expr(e) {
        Expr::Name(n) => Some(n.clone()),
        Expr::Field { base, name } if name == "value" && representation(*base) => {
            path_where(body, *base, representation)
        }
        Expr::Field { base, name } => {
            path_where(body, *base, representation).map(|b| format!("{b}.{name}"))
        }
        _ => None,
    }
}

/// **Whether `e` is a value of an opaque type**, whose `.value` is its
/// representation (ADR-0048, ADR-0232), in `decl`'s body in unit `unit`. As
/// `types` types it; where they do not, as the value relations do, which
/// solve a call: the binding of `{:Some(r)}` under `{#match List.get(xs,
/// 0)}`.
fn is_opaque(
    (sigs, ws): (&Signatures, &Workspace),
    (hir, unit, decl): (&Hir, usize, DeclId),
    types: &crate::infer::Types<'_>,
    body: &Body,
    e: ExprId,
) -> bool {
    let def = types.of(body, e).and_then(|t| t.def_id()).or_else(|| {
        let module = hir.module_of(decl);
        match crate::values::type_of(sigs, ws, unit, module, hir.decl(decl), body, e).0 {
            crate::values::Ty::Nominal(d, _) => Some(d),
            _ => None,
        }
    });
    def.and_then(|d| sigs.type_decl(d))
        .is_some_and(|t| t.representation.is_some())
}

/// [`value_path`], for a checker: the path a template reads a value by, or
/// `None` for anything computed (ADR-0073, ADR-0136).
pub fn value_path_of(body: &Body, e: ExprId) -> Option<String> {
    value_path(body, e)
}

/// **The path a template reads a value by, as the template does** (ADR-0232):
/// an opaque value's `.value` by the opaque value's own path, as the checker
/// types `body`, the body of `decl` in unit `unit`.
pub fn value_path_typed(
    (sigs, ws): (&Signatures, &Workspace),
    (hirs, unit, decl): (&[&Hir], usize, DeclId),
    body: &Body,
    e: ExprId,
) -> Option<String> {
    let hir = hirs[unit];
    let types = crate::infer::Types::of_decl(sigs, hir, decl, body);
    path_where(body, e, &|x| {
        is_opaque((sigs, ws), (hir, unit, decl), &types, body, x)
    })
}

/// **What a computed hole reads** (ADR-0226): each name its expression
/// writes that is bound outside it, once, in the order first written, with
/// the path the template reads it by. A query's binding, a signal, a loop's
/// item, a view's parameter. A name bound inside it, a function's parameter,
/// a block's `let` or a match arm's, is its own; one no local binds is a
/// declaration's, a function it calls, unless it is a module's signal.
fn inputs_of(body: &Body, e: ExprId, ctx: &Lowering<'_>) -> Vec<(String, String)> {
    let Some(lexical) = crate::lexical::Lexical::build_in(ctx.hirs[ctx.unit], ctx.decl) else {
        return Vec::new();
    };
    // Every pattern bound inside it, and each pattern within those.
    let mut inner = BTreeSet::new();
    let mut stack = Vec::new();
    for x in body.walk_from(e) {
        match body.expr(x) {
            Expr::Lambda { params, .. } => stack.extend(params.iter().copied()),
            Expr::Let { pat: Some(p), .. } | Expr::For { pat: Some(p), .. } => stack.push(*p),
            Expr::Match { arms, .. } => stack.extend(arms.iter().map(|a| a.pat)),
            _ => {}
        }
    }
    while let Some(p) = stack.pop() {
        if inner.insert(p) {
            match body.pat(p) {
                crate::hir::Pattern::Ctor { args, .. } => stack.extend(args.iter().copied()),
                crate::hir::Pattern::Or(alts) => stack.extend(alts.iter().copied()),
                _ => {}
            }
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for x in body.walk_from(e) {
        let Expr::Name(n) = body.expr(x) else {
            continue;
        };
        let outside = match lexical.binder(x) {
            Some(crate::lexical::Binder::Pattern(p)) => !inner.contains(&p),
            Some(_) => true,
            // A signal, or a predicate the page asks of its reader, read by
            // the name its scope gives it (ADR-0307).
            None => {
                ctx.signals.contains(n) || ctx.names.get(n).is_some_and(|to| to.ends_with("~holds"))
            }
        };
        if outside && !out.iter().any(|(m, _)| m == n) {
            out.push((n.clone(), ctx.read(n.clone())));
        }
    }
    out
}

/// **A template's text as a page holds it**: the way its source is laid out is
/// not part of the page. A run of white space that holds a line break is one
/// space, as a browser shows it; and where nothing between two tags can ever
/// be shown, in a table's structure, a `<select>` or the head, it is nothing.
/// Inside `<pre>`, `<textarea>`, `<script>` and `<style>` every character is
/// content, and is left as it is written.
fn as_shown<'t>(text: &'t str, parents: &[String]) -> std::borrow::Cow<'t, str> {
    use std::borrow::Cow;
    const IS_CONTENT: [&str; 4] = ["pre", "textarea", "script", "style"];
    const SHOWS_NO_TEXT: [&str; 11] = [
        "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "datalist",
        "head", "html",
    ];
    let space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}');
    if parents.iter().any(|p| IS_CONTENT.contains(&p.as_str())) || !text.contains('\n') {
        return Cow::Borrowed(text);
    }
    if text.chars().all(space)
        && parents
            .last()
            .is_some_and(|p| SHOWS_NO_TEXT.contains(&p.as_str()))
    {
        return Cow::Borrowed("");
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(space) {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after.find(|c: char| !space(c)).unwrap_or(after.len());
        let run = &after[..end];
        if run.contains('\n') {
            out.push(' ');
        } else {
            out.push_str(run);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn lower_node(body: &Body, id: NodeId, ctx: &Lowering<'_>, ix: &mut Indexer, out: &mut Vec<Chunk>) {
    match body.node(id) {
        Node::Text(t) => {
            let text = as_shown(t, &ix.parents);
            if !text.is_empty() {
                out.push(Chunk::Static(escape_static_text(&text)));
            }
        }
        Node::Interpolation(e) => out.push(Chunk::Dynamic(match ctx.path(body, *e) {
            Some(value) => {
                let value = ctx.read(value);
                let id = ix.part();
                ix.holes.push(Hole {
                    part: id,
                    path: value.clone(),
                    expr: *e,
                    origin: (ctx.unit, ctx.decl),
                    nested: ix.depth > 0,
                    framed: ix.frames > 0,
                    inputs: None,
                });
                Part::Text {
                    id,
                    value,
                    context: Context::Text,
                }
            }
            // **A computed hole** (ADR-0226, ruling 0073-a): read by a path
            // the compiler names, `#{template}~{part}`, which no source can
            // write, and given its value by what computes it. Until ADR-0226
            // it was refused here: "a template hole is read by path".
            None => {
                let id = ix.part();
                let inputs = inputs_of(body, *e, ctx);
                let value = ix.computed_path(id, &inputs);
                ix.holes.push(Hole {
                    part: id,
                    path: value.clone(),
                    expr: *e,
                    origin: (ctx.unit, ctx.decl),
                    nested: ix.depth > 0,
                    framed: ix.frames > 0,
                    inputs: Some(inputs),
                });
                Part::Text {
                    id,
                    value,
                    context: Context::Text,
                }
            }
        })),
        Node::Element {
            tag,
            attrs,
            children,
            self_closing,
        } => lower_element(
            body,
            Element {
                tag,
                attrs,
                children,
                self_closing: *self_closing,
            },
            ctx,
            ix,
            out,
        ),
        Node::Block {
            directive,
            children,
            subject,
            ..
        } => lower_block(body, id, directive, children, *subject, ctx, ix, out),
        // A marker outside any block. The markup rules refuse it (PW5019).
        Node::Branch { marker, .. } => out.push(Chunk::Dynamic(Part::Blocked {
            reason: format!("`{marker}` stands outside any block"),
            at: marker.clone(),
        })),
    }
}

/// **Does this attribute mount a resource?** `resource={StoreMap}` does
/// (A-007): its element hosts the resource, and the element's other
/// attributes are the resource's arguments. RDFa's `resource="/x"` is an
/// ordinary attribute (ADR-0075).
pub(crate) fn mounts(a: &crate::hir::Attr) -> bool {
    a.name == "resource" && matches!(a.value, AttrValue::Expr(_))
}

/// The element being lowered, as one value.
///
/// Four of these travelled as separate parameters and clippy was right to
/// object: they are one thing — the element — and passing them apart invites a
/// call site that pairs the wrong tag with the wrong attributes.
struct Element<'a> {
    tag: &'a str,
    attrs: &'a [crate::hir::Attr],
    children: &'a [NodeId],
    self_closing: bool,
}

/// **Whether `e` reads a signal the device keeps** (track `device-store`):
/// a name of one among the signals this body reads.
fn reads_the_device(body: &Body, e: ExprId, ctx: &Lowering<'_>) -> bool {
    body.walk_from(e).into_iter().any(|x| match body.expr(x) {
        Expr::Name(n) => {
            ctx.signals.contains(n)
                && matches!(
                    ctx.ws.resolve(ctx.unit, n),
                    crate::resolve::Resolution::Local(d)
                        | crate::resolve::Resolution::Imported { def: d, .. }
                        if ctx.sigs.kept_by_the_device(d)
                )
        }
        _ => false,
    })
}

/// **Whether an element holds a part the device decides** (track
/// `device-store`): one of its attributes, a handler's aside, reads a signal
/// the device keeps, or a text or block whose nearest element it is does,
/// through the blocks between, which are no element.
fn decided_by_the_device(
    body: &Body,
    attrs: &[crate::hir::Attr],
    children: &[NodeId],
    ctx: &Lowering<'_>,
) -> bool {
    fn own(body: &Body, n: NodeId, ctx: &Lowering<'_>) -> bool {
        match body.node(n) {
            Node::Interpolation(e) => reads_the_device(body, *e, ctx),
            Node::Block {
                subject,
                each,
                children,
                ..
            } => {
                subject.is_some_and(|e| reads_the_device(body, e, ctx))
                    || each
                        .as_ref()
                        .is_some_and(|h| reads_the_device(body, h.list, ctx))
                    || children.iter().any(|c| own(body, *c, ctx))
            }
            Node::Branch { condition, .. } => {
                condition.is_some_and(|e| reads_the_device(body, e, ctx))
            }
            _ => false,
        }
    }
    attrs.iter().any(|a| {
        a.event().is_none()
            && matches!(a.value, AttrValue::Expr(e) if reads_the_device(body, e, ctx))
    }) || children.iter().any(|c| own(body, *c, ctx))
}

fn lower_element(
    body: &Body,
    el: Element<'_>,
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) {
    let Element {
        tag,
        attrs,
        children,
        self_closing,
    } = el;
    // **A layout's `<slot />`** (ADR-0303): where the page that names the
    // layout is shown. Only the layout's own, at the top of its view, once,
    // and empty (PW5044): the page's markup is lowered after the layout's,
    // and placed here, between two markers a navigation finds.
    if tag == "slot" {
        let own = ix.layout == Some((ctx.unit, ctx.decl));
        if own
            && ix.depth == 0
            && ix.frames == 0
            && ix.slot.is_none()
            && attrs.is_empty()
            && children.is_empty()
        {
            out.push(Chunk::Static(crate::layouts::SLOT_START.to_string()));
            ix.slot = Some(Slot {
                at: out.len(),
                elements: ix.elements,
            });
            out.push(Chunk::Static(crate::layouts::SLOT_END.to_string()));
        } else {
            out.push(Chunk::Dynamic(Part::Blocked {
                reason: "a `<slot />` is its layout's, once, at the top of its view, and \
                         empty (PW5044)"
                    .to_string(),
                at: "<slot>".to_string(),
            }));
        }
        return;
    }
    // A region showing its query's state (ADR-0148). Until 2026-10-03 it
    // was refused here, and only the Marko adapter rendered one (ADR-0075).
    if tag == "stream" {
        lower_stream(body, attrs, children, ctx, ix, out);
        return;
    }
    // A title anywhere but at the top of a page's view (ADR-0183): refused
    // at check (PW5030), and written nowhere.
    if tag == "title" {
        out.push(Chunk::Dynamic(Part::Blocked {
            reason: "a title is the page's, at the top of its view (PW5030)".to_string(),
            at: "<title>".to_string(),
        }));
        return;
    }
    // And metadata (ADR-0186, PW5034). An item's property is written where
    // it is.
    if tag == "meta" && !attrs.iter().any(|a| a.name == "itemprop") {
        out.push(Chunk::Dynamic(Part::Blocked {
            reason: "metadata is the page's, at the top of its view (PW5034)".to_string(),
            at: "<meta>".to_string(),
        }));
        return;
    }
    // Not compiled by this renderer (ADR-0075): an element that mounts a
    // resource (A-007). Until 2026-09-26 it lowered as a literal element, a
    // `<map-container>` whose resource was never mounted.
    if attrs.iter().any(mounts) {
        out.push(Chunk::Dynamic(Part::Blocked {
            reason: "an element that mounts a resource is not compiled".to_string(),
            at: format!("<{tag} resource={{..}}>"),
        }));
        return;
    }
    // A view used in another is written where it is used (ADR-0136). A
    // capitalised tag that names no view is not an element either: until
    // 2026-09-26 it was written out as one (ADR-0072).
    if tag.starts_with(|c: char| c.is_ascii_uppercase()) {
        match ctx.view_named(tag) {
            Some(def) => compose(body, def, tag, attrs, ctx, ix, out),
            None => out.push(Chunk::Dynamic(Part::Blocked {
                reason: format!("`<{tag}>` names no view this build can see"),
                at: format!("<{tag}>"),
            })),
        }
        return;
    }
    // An element gets an identity only if it OWNS something dynamic. Charter
    // §14 M7 task 3: stable IDs for dynamic parts "without making static HTML
    // noisy", and the invariant that gives it meaning is that a static region
    // receives no identity markup at all.
    let dynamic_owner = attrs
        .iter()
        .any(|a| a.name.starts_with("on:") || matches!(a.value, AttrValue::Expr(_)));
    let owner = dynamic_owner.then(|| ix.element());

    out.push(Chunk::Static(format!("<{tag}")));
    if let Some(o) = owner {
        out.push(Chunk::Static(format!(" data-pw=\"{o}\"")));
    }
    // **What the device decides is not shown until the device is read**
    // (track `device-store`, ADR-0366, decision 3): an element one of whose
    // attributes, or a text or block whose nearest element it is, reads a
    // signal the device keeps. The server renders the declared default; the
    // document's head keeps a marked element hidden while the document is
    // reading the device, and the runtime shows it once it has.
    if decided_by_the_device(body, attrs, children, ctx) {
        out.push(Chunk::Static(" data-pw-device".to_string()));
    }
    // **Which instance each signal its handlers name is** (ADR-0144). A
    // handler's module is compiled once and names a signal as its body
    // writes it; where this use of a view holds the signal under another
    // name, the element says so, and the browser reads and sets that one.
    // Written here, at compile time, so no handler reaches another instance.
    let mut instances: BTreeMap<&str, &str> = BTreeMap::new();
    for a in attrs.iter().filter(|a| a.event().is_some()) {
        let AttrValue::Expr(handler) = &a.value else {
            continue;
        };
        for e in body.walk_from(*handler) {
            if let Expr::Name(n) = body.expr(e)
                && ctx.signals.contains(n)
                && let Some(to) = ctx.names.get(n)
                && to != n
            {
                instances.insert(n, to);
            }
        }
    }
    if !instances.is_empty() {
        let json = serde_json::to_string(&instances).expect("names serialize");
        out.push(Chunk::Static(format!(
            " data-pw-signals=\"{}\"",
            escape_static_attribute(&json)
        )));
    }

    // Attribute order is the SOURCE order, and it is preserved rather than
    // sorted. HTML gives attribute order no meaning, so either would be
    // correct — but the choice has to be made once and written down, or two
    // runs of the same compiler produce different bytes and content addressing
    // stops working. Source order is chosen because it is the one a reader can
    // predict from the file.
    //
    // Handlers come last, in their source order (ADR-0138): they write no
    // markup but what they capture, and an element's handlers carry it in
    // one `data-pw-captures` attribute, which the renderer writes once for
    // the run of them. Two would be one attribute written twice, and a
    // browser keeps the first.
    let (handlers, markup): (Vec<&crate::hir::Attr>, Vec<&crate::hir::Attr>) =
        attrs.iter().partition(|a| a.event().is_some());
    // **A `<textarea>`'s value is its text** (ADR-0221): written after its
    // start tag, where HTML reads it, numbered where it is written. Until
    // ADR-0221 it was an attribute a textarea does not have, and a first
    // value showed nothing until the runtime set it. A `<select>`'s is the
    // option it marks `selected`, which PW5036 asks for.
    let control = tag.to_ascii_lowercase();
    let is_value = |a: &crate::hir::Attr| a.name.eq_ignore_ascii_case("value");
    let mut content: Option<Chunk> = None;
    for a in markup.into_iter().chain(handlers) {
        if control == "select" && is_value(a) {
            out.push(Chunk::Dynamic(Part::Blocked {
                reason: "a `<select>`'s value is the option it marks `selected` (PW5036)"
                    .to_string(),
                at: "<select value>".to_string(),
            }));
            continue;
        }
        if control == "textarea" && is_value(a) {
            content = Some(match &a.value {
                AttrValue::None => Chunk::Static(String::new()),
                AttrValue::Static(v) => Chunk::Static(escape_static_content(unquote(v))),
                AttrValue::Expr(e) => match ctx.path(body, *e).map(|v| ctx.read(v)) {
                    Some(value) if !matches!(body.expr(*e), Expr::Interpolated { .. }) => {
                        let owner =
                            owner.expect("an element with a dynamic attribute owns an identity");
                        let id = ix.part();
                        ix.read(id, &value, ReadKind::Attribute, ReadAt::Expr(*e), ctx);
                        Chunk::Dynamic(Part::Attribute {
                            id,
                            owner,
                            name: a.name.clone(),
                            value,
                            context: Context::Content,
                        })
                    }
                    _ => Chunk::Dynamic(Part::Blocked {
                        reason: "a `<textarea>`'s value is a signal or text (PW5036)".to_string(),
                        at: "<textarea value={..}>".to_string(),
                    }),
                },
            });
            continue;
        }
        // An event handler is not markup. It is a behaviour the browser runtime
        // attaches, and E7-R owns it; emitting anything for it here would be
        // inventing an encoding the runtime does not yet have.
        // An event handler is behaviour, not markup: nothing is written into
        // the element for it. The part records which element, which event and
        // which handler, and the browser runtime attaches after `decide`.
        if let Some((event, modifiers)) = a.event() {
            // The identity `resume_artifacts` derived for this exact lambda.
            // Empty when the handler is not resumable — an ordinary handler has
            // no resume manifest and nothing to compare against.
            // And what the document carries for it (ADR-0134): derived with
            // the identity, once, where names are resolved.
            let (handler, captures) = match &a.value {
                AttrValue::Expr(e) => ctx
                    .handlers
                    .get(&(ctx.unit, ctx.decl, *e))
                    .cloned()
                    .unwrap_or_default(),
                _ => (String::new(), Vec::new()),
            };
            // A capture the scope names otherwise is read where it is
            // (ADR-0136): in a composed view, a parameter or a renamed name.
            let renames: BTreeMap<String, String> = captures
                .iter()
                .filter_map(|c| {
                    let root = c.split('.').next()?;
                    let to = ctx.names.get(root)?;
                    (to != root).then(|| (root.to_string(), to.clone()))
                })
                .collect();
            let name = match &a.value {
                AttrValue::Expr(e) => called_name(body, *e),
                _ => String::new(),
            };
            let id = ix.part();
            // **What it captures is read where its element is** (ADR-0217):
            // a host sets the captures again when a value they read changes,
            // as it sets an attribute's. Until then nothing did, and a press
            // sent the value the page was first rendered with.
            if let AttrValue::Expr(e) = &a.value {
                for c in &captures {
                    ix.read(
                        id,
                        &ctx.read(c.clone()),
                        ReadKind::Captures,
                        ReadAt::Expr(*e),
                        ctx,
                    );
                }
            }
            out.push(Chunk::Dynamic(Part::Event {
                id,
                owner: owner.expect("an element with a handler owns an identity"),
                event: event.to_string(),
                handler,
                name,
                captures,
                renames,
                modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
            }));
            continue;
        }
        // `style:width={w}` is a directive, and `on:` is the only one compiled.
        // Written out, it is an attribute named `style:width`, which a browser
        // ignores: until 2026-09-26 the style was silently not applied
        // (ADR-0073). An XML namespace (`xlink:href`) is an attribute's name.
        if let Some((prefix, _)) = a.name.split_once(':')
            && !matches!(prefix, "xml" | "xlink" | "xmlns")
        {
            out.push(Chunk::Dynamic(Part::Blocked {
                reason: format!(
                    "`{}` is a directive, and only `on:` directives are compiled",
                    a.name
                ),
                at: a.name.clone(),
            }));
            continue;
        }
        match &a.value {
            AttrValue::None => {
                out.push(Chunk::Static(format!(" {}", a.name)));
            }
            // A `{` that opens no expression: `title="{}"`. A string with
            // holes lowers as one (`AttrValue::Expr`, below); what is left
            // here is a brace the author may have meant as a hole, and writing
            // it out would publish `/stores/{id}` as an address, as happened
            // before ADR-0042.
            AttrValue::Static(v) if unquote(v).contains('{') => {
                out.push(Chunk::Dynamic(Part::Blocked {
                    reason: format!("`{}` has a `{{` that opens no expression", a.name),
                    at: format!("{}={}", a.name, v),
                }));
            }
            AttrValue::Static(v) => {
                // The HIR keeps the value AS WRITTEN, quotes included, because
                // it is a faithful record of the source. The IR is a
                // description of a document, so the quotes are the syntax that
                // delimited the value and not part of it — leaving them in
                // renders `aria-label="&quot;Menu&quot;"`, which a screen
                // reader announces with the quote marks.
                out.push(Chunk::Static(format!(
                    " {}=\"{}\"",
                    a.name,
                    escape_static_attribute(unquote(v))
                )));
            }
            // `href="/stores/{id}"` (ADR-0042).
            AttrValue::Expr(e) if matches!(body.expr(*e), Expr::Interpolated { .. }) => {
                out.push(Chunk::Static(" ".to_string()));
                let owner = owner.expect("an element with a dynamic attribute owns an identity");
                if let Expr::Interpolated { text, parts } = body.expr(*e) {
                    out.push(Chunk::Dynamic(interpolated_attribute(
                        body, &a.name, text, parts, owner, ctx, ix,
                    )));
                }
            }
            AttrValue::Expr(e) => {
                out.push(Chunk::Static(" ".to_string()));
                let owner = owner.expect("an element with a dynamic attribute owns an identity");
                let id = ix.part();
                // A path, or a computed value read by the path the compiler
                // names it (ADR-0226), as a computed hole is.
                let (value, inputs) = match ctx.path(body, *e).map(|v| ctx.read(v)) {
                    Some(value) => (value, None),
                    None => {
                        let inputs = inputs_of(body, *e, ctx);
                        (ix.computed_path(id, &inputs), Some(inputs))
                    }
                };
                ix.read(id, &value, ReadKind::Attribute, ReadAt::Expr(*e), ctx);
                if let Some(read) = ix.reads.last_mut() {
                    read.inputs = inputs;
                }
                if BOOLEAN_ATTRIBUTES.contains(&a.name.as_str()) {
                    out.push(Chunk::Dynamic(Part::BooleanAttribute {
                        id,
                        owner,
                        name: a.name.clone(),
                        value,
                    }));
                } else {
                    out.push(Chunk::Dynamic(Part::Attribute {
                        id,
                        owner,
                        name: a.name.clone(),
                        value,
                        context: Context::of_attribute(&a.name),
                    }));
                }
            }
        }
    }

    // How deep the template nests (ADR-0203): this element, inside those
    // around it.
    ix.deepest = ix.deepest.max(ix.elements + 1);
    if VOID_ELEMENTS.contains(&tag) {
        // `<br>`, not `<br/>` and not `<br></br>`. The slash is ignored by the
        // HTML parser and the closing tag is a parse error it repairs — both
        // produce plausible bytes and, for the second, a different tree.
        out.push(Chunk::Static(">".to_string()));
        return;
    }

    out.push(Chunk::Static(">".to_string()));
    if control == "textarea" {
        // Its text is its value, written once, as text (PW5036): a value in
        // it would be written between markers a textarea shows as text.
        let written = children
            .iter()
            .all(|c| matches!(body.node(*c), Node::Text(_)));
        match content {
            Some(_) if !children.is_empty() => out.push(Chunk::Dynamic(Part::Blocked {
                reason: "a `<textarea>` has a value and text (PW5036)".to_string(),
                at: "<textarea value>".to_string(),
            })),
            Some(value) => out.push(value),
            None if !written => out.push(Chunk::Dynamic(Part::Blocked {
                reason: "a `<textarea>`'s text holds a value (PW5036)".to_string(),
                at: "<textarea>".to_string(),
            })),
            None => {}
        }
    }
    ix.elements += 1;
    ix.parents.push(tag.to_string());
    for c in children {
        lower_node(body, *c, ctx, ix, out);
    }
    ix.parents.pop();
    ix.elements -= 1;
    // A closing tag for everything that is not void, whether or not the source
    // wrote `/>`. A first version branched on `self_closing` and produced the
    // same bytes on both sides, which is the shape of a decision that was never
    // made: HTML has no self-closing non-void element, so `<div/>` and `<div>`
    // open the same element and both need `</div>`.
    let _ = self_closing;
    out.push(Chunk::Static(format!("</{tag}>")));
}

/// **A view used in another, written where it is used** (ADR-0136): its
/// markup lowered in place, in the template's one numbering, each parameter
/// read as the path the prop gives it. What cannot be composed is a
/// `Blocked` part, as everything the IR cannot represent is; `pw check`
/// refuses each case first (PW5020, PW0619).
fn compose(
    body: &Body,
    def: DefId,
    tag: &str,
    attrs: &[crate::hir::Attr],
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) {
    let blocked = |reason: String| {
        Chunk::Dynamic(Part::Blocked {
            reason,
            at: format!("<{tag}>"),
        })
    };
    let Some(decl) = crate::resolve::declaration(ctx.hirs, def) else {
        out.push(blocked(format!("`<{tag}>` names no declaration")));
        return;
    };
    let hir = ctx.hirs[def.unit];
    let Some(view) = decl.body.map(|b| hir.body(b)) else {
        out.push(blocked(format!("`{tag}` has no body")));
        return;
    };
    if !composable(view) {
        out.push(blocked(format!(
            "`{tag}` declares values of its own, and a composed view holds its markup and its \
             signals alone"
        )));
        return;
    }
    if let Some(a) = attrs
        .iter()
        .find(|a| !decl.params.iter().any(|p| p.name == a.name))
    {
        out.push(blocked(format!("`{tag}` takes no prop `{}`", a.name)));
        return;
    }
    // A predicate it asks of its reader is read by the name a host answers,
    // as a page's and a layout's is (ADR-0307): as written, no document held
    // a value for it, and its page's plan named nothing it asked.
    let mut names = asked_predicates(ctx.hirs, ctx.ws, ctx.sigs, def.unit);
    for p in &decl.params {
        let given = attrs
            .iter()
            .find(|a| a.name == p.name)
            .and_then(|a| match &a.value {
                AttrValue::Expr(e) => ctx.path(body, *e).map(|v| ctx.read(v)),
                _ => None,
            });
        let Some(path) = given else {
            out.push(blocked(format!(
                "`{tag}`'s prop `{}` is given no value path",
                p.name
            )));
            return;
        };
        names.insert(p.name.clone(), path);
    }
    // Its own signals, and the signals it provides: each use holds its own,
    // named so no source can write it (ADR-0144). Each row of a loop would
    // need one of its own, which the browser does not hold yet (PW5307).
    let own = crate::page_values::signals_of(view);
    let gives = provides_of(ctx.hirs, ctx.ws, def.unit, view);
    // A view that contains itself is an instance made at run time (ADR-0203,
    // ADR-0130's ruling 2): its own template, compiled once, its parameters
    // given the value paths its props are, read here. Composed in place, it
    // would never end. An instance is a run-time scope, as a loop's row is,
    // and holds no signal yet.
    if contains_itself(ctx.ws, ctx.hirs, def) {
        if !(own.is_empty() && gives.is_empty()) {
            out.push(blocked(format!(
                "`<{tag}>` contains itself and holds a signal, and a view that contains \
                 itself holds none yet"
            )));
            return;
        }
        if shows_a_stream(view) {
            out.push(blocked(format!(
                "`<{tag}>` contains itself and shows a query's state in a `<stream>`, and a \
                 view that contains itself shows none yet"
            )));
            return;
        }
        if !computed_values(view).is_empty() {
            out.push(blocked(format!(
                "`<{tag}>` contains itself and computes a value in its template, and a view \
                 that contains itself computes none yet (ruling 0073-a)"
            )));
            return;
        }
        // Nor what it reads through a member, which a host computes from a
        // query's value and a row's item, and never from an instance's
        // (ADR-0323). Until 2026-10-12 one built, and nothing computed it.
        if let Some(read) = member_read(ctx, def, view) {
            out.push(blocked(format!(
                "`<{tag}>` contains itself and reads `{read}` through a member function, and a \
                 view that contains itself computes none yet (ruling 0073-a)"
            )));
            return;
        }
        if !ix.views.contains(&def) {
            ix.views.push(def);
        }
        let id = DeclId(def.decl);
        let path = crate::contract::component_id(hir, id);
        out.push(Chunk::Dynamic(Part::Instance {
            id: ix.part(),
            path,
            args: decl
                .params
                .iter()
                .filter_map(|p| Some((p.name.clone(), names.get(&p.name)?.clone())))
                .collect(),
            elements: ix.elements,
            deepest: 0,
        }));
        return;
    }
    if ix.frames > 0 && !(own.is_empty() && gives.is_empty()) {
        out.push(blocked(format!(
            "`<{tag}>` holds a signal, and a view used in a loop's row holds none yet"
        )));
        return;
    }
    let origin = (def.unit, DeclId(def.decl));
    let mut provided = ctx.provided.clone();
    let mut signals = BTreeSet::new();
    for (name, init, declared) in own {
        let Expr::Let { ty: Some(t), .. } = view.expr(declared) else {
            continue;
        };
        let fresh = ix.rename(&name);
        ix.instances.push(Instance {
            name: fresh.clone(),
            origin,
            init,
            ty: InstanceType::Written(*t),
            within: ix.blocks.clone(),
        });
        names.insert(name.clone(), fresh);
        signals.insert(name);
    }
    for (name, signal, value) in gives {
        let fresh = ix.rename(&name);
        ix.instances.push(Instance {
            name: fresh.clone(),
            origin,
            init: value,
            ty: InstanceType::Declared(signal),
            within: ix.blocks.clone(),
        });
        names.insert(name.clone(), fresh.clone());
        provided.insert(signal, fresh);
        signals.insert(name);
    }
    // A signal provided around it is the nearest `provide`'s instance. With
    // none, as in a view's own template, it is read by its name: a page that
    // provides nothing a view it uses needs is refused (PW5305).
    for (name, signal) in signals_named(ctx.hirs, ctx.ws, def.unit, view) {
        if signals.contains(&name) {
            continue;
        }
        let instance = ctx.provided.get(&signal).cloned().unwrap_or(name.clone());
        names.insert(name.clone(), instance);
        signals.insert(name);
    }
    if !ix.views.contains(&def) {
        ix.views.push(def);
    }
    let inner = Lowering {
        handlers: ctx.handlers,
        hirs: ctx.hirs,
        ws: ctx.ws,
        types: std::rc::Rc::new(crate::infer::Types::of_decl(
            ctx.sigs,
            ctx.hirs[def.unit],
            DeclId(def.decl),
            view,
        )),
        sigs: ctx.sigs,
        unit: def.unit,
        decl: DeclId(def.decl),
        names,
        around: ctx.around.clone(),
        in_layout: ctx.in_layout,
        provided,
        signals,
    };
    for root in roots_of(view) {
        lower_node(view, root, &inner, ix, out);
    }
}

/// **A `<stream>`** (ADR-0148): its query and arguments, and each of its
/// parts, lowered as a block's arms are. At the top of the template, or in a
/// view composed there: one inside a block or a loop's row is not rendered
/// yet, since nothing would render it again when the block shows another arm.
fn lower_stream(
    body: &Body,
    attrs: &[crate::hir::Attr],
    children: &[NodeId],
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) {
    let blocked = |reason: &str| {
        Chunk::Dynamic(Part::Blocked {
            reason: reason.to_string(),
            at: "<stream>".to_string(),
        })
    };
    if ix.depth > 0 {
        out.push(blocked(
            "a `<stream>` inside a block or a loop's row is not rendered yet (ADR-0148)",
        ));
        return;
    }
    let Some(call) = crate::streams::query_attr(attrs) else {
        out.push(blocked("a `<stream>` names its query: `query={Q(..)}`"));
        return;
    };
    let Some((def, decl)) = crate::streams::stream_query(ctx.ws, ctx.hirs, ctx.unit, body, call)
    else {
        out.push(blocked("a `<stream>`'s `query` is a call of a query"));
        return;
    };
    let Expr::Call { args: given, .. } = body.expr(call) else {
        return;
    };
    let mut args = Vec::new();
    for a in given.iter().map(|a| a.value) {
        match (ctx.path(body, a), body.expr(a)) {
            (Some(path), _) => args.push(ctx.read(path)),
            // An invocation-context call: `current_session()`.
            (None, Expr::Call { callee, args: none }) if none.is_empty() => {
                args.push(format!("{}()", crate::infer::path_of(body, *callee)));
            }
            _ => {
                out.push(blocked(
                    "a stream's query is given values by path, or an invocation-context call",
                ));
                return;
            }
        }
    }
    let Some(query) = ctx.hirs.get(def.unit).and_then(|hir| {
        hir.all_decls()
            .find(|(id, _)| id.0 == def.decl)
            .map(|(id, _)| crate::contract::component_id(hir, id))
    }) else {
        out.push(blocked("a stream's query has no component"));
        return;
    };
    let id = ix.part();
    // A signal instance inside it belongs to it (ADR-0144).
    ix.blocks.push(id);
    let mut placeholder = Vec::new();
    let mut ready = StreamArm::default();
    let mut failed = StreamArm::default();
    for c in children {
        let Node::Element {
            tag,
            attrs,
            children,
            ..
        } = body.node(*c)
        else {
            continue;
        };
        let bound: Vec<String> = attrs
            .iter()
            .filter(|a| a.name == "as")
            .filter_map(|a| match &a.value {
                AttrValue::Expr(e) => match body.expr(*e) {
                    Expr::Name(n) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let (scope, mut written) = ctx.binding(&bound, ix);
        let lowered = lower_run(body, children, &scope, ix);
        match tag.as_str() {
            "placeholder" => placeholder = lowered,
            "ready" => {
                ready = StreamArm {
                    binding: written.pop(),
                    body: lowered,
                }
            }
            "failed" => {
                failed = StreamArm {
                    binding: written.pop(),
                    body: lowered,
                }
            }
            _ => {}
        }
    }
    ix.blocks.pop();
    out.push(Chunk::Dynamic(Part::Stream {
        id,
        query,
        args,
        streamed: crate::streams::streamed(decl),
        placeholder,
        ready,
        failed,
    }));
}

#[allow(clippy::too_many_arguments)]
fn lower_block(
    body: &Body,
    node: NodeId,
    directive: &str,
    children: &[NodeId],
    subject: Option<ExprId>,
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) {
    // The block's own id comes BEFORE its children's, so document order and id
    // order agree — a patch stream that arrives in id order arrives in a order
    // the runtime can apply without buffering.
    let id = ix.part();
    // What a signal instance inside it belongs to (ADR-0144).
    ix.blocks.push(id);
    lower_block_at(body, node, id, directive, children, subject, ctx, ix, out);
    ix.blocks.pop();
}

#[allow(clippy::too_many_arguments)]
fn lower_block_at(
    body: &Body,
    node: NodeId,
    id: PartId,
    directive: &str,
    children: &[NodeId],
    subject: Option<ExprId>,
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
    out: &mut Vec<Chunk>,
) {
    let d = directive.trim();
    let (lead, branches) = split_branches(body, children);
    let blocked = |reason: String| {
        Chunk::Dynamic(Part::Blocked {
            reason,
            at: d.to_string(),
        })
    };
    // Its head, as the grammar read it (ADR-0242).
    let each = match body.node(node) {
        Node::Block { each, .. } => each.as_ref(),
        _ => None,
    };
    if let Some(each) = each {
        let binding = each.binder.clone();
        // The list's path, read from the expression it is (ADR-0345): a
        // name, or fields read from one. Or **a list the browser computes**
        // from the values it holds (track `device-store`, ADR-0366, decision
        // 7), read by the path the compiler names, as a computed subject is
        // (ADR-0229); the plan says who computes it, and refuses one no one
        // does.
        let computed = value_path(body, each.list).is_none();
        let collection = value_path(body, each.list).unwrap_or_default();
        let browsers = inputs_of(body, each.list, ctx);
        if computed
            && (browsers.is_empty() || !browsers.iter().all(|(name, _)| ctx.signals.contains(name)))
        {
            out.push(blocked(format!(
                "`{{#each}}` reads its list by path, and `{}` is computed",
                each.written.list
            )));
            return;
        }
        // The key as the path from the element to it: `id` from `(item.id)`,
        // `r.id` from `(item.r.id)`, and empty from `(item)`. Until 2026-09-26
        // it was the key's last segment, so `(item.r.id)` keyed on `item.id`
        // (ADR-0073). A key whose head is another name is PW5021.
        let key = match each.key {
            None => None,
            Some(k) => {
                let path = value_path(body, k).unwrap_or_default();
                let mut segments = path.split('.');
                if segments.next() != Some(binding.as_str()) {
                    out.push(blocked(format!(
                        "a loop's key is `{binding}` or a field read from it, and this is `{}`",
                        each.written.key.clone().unwrap_or_default()
                    )));
                    return;
                }
                Some(segments.collect::<Vec<_>>().join("."))
            }
        };
        if !branches.is_empty() {
            out.push(blocked(
                "`{#each}` takes no branch marker; `{#if xs}` around the list says \
                 `{:else}`"
                    .to_string(),
            ));
            return;
        }
        let collection = match computed {
            true => {
                let inputs = browsers;
                let value = ix.computed_path(id, &inputs);
                ix.read(id, &value, ReadKind::List, ReadAt::Expr(each.list), ctx);
                if let Some(read) = ix.reads.last_mut() {
                    read.inputs = Some(inputs);
                }
                value
            }
            false => {
                let collection = ctx.read(collection);
                ix.read(id, &collection, ReadKind::List, ReadAt::List(node), ctx);
                collection
            }
        };
        let (scope, mut written) = ctx.binding(std::slice::from_ref(&binding), ix);
        ix.frames += 1;
        ix.loops
            .push(written.last().cloned().unwrap_or_else(|| binding.clone()));
        let inner = lower_run(body, &lead, &scope, ix);
        ix.loops.pop();
        ix.frames -= 1;
        out.push(Chunk::Dynamic(Part::Each {
            id,
            collection,
            binding: written.pop().unwrap_or(binding),
            key,
            body: inner,
        }));
        return;
    }
    if opens(d, "{#if") {
        let Some(e) = subject else {
            out.push(blocked("an `{#if}` has a condition".to_string()));
            return;
        };
        let value = subject_of(body, id, e, ctx, ix);
        out.push(conditional(body, id, value, &lead, &branches, ctx, ix));
        return;
    }
    if opens(d, "{#match") {
        let Some(e) = subject else {
            out.push(blocked("a `{#match}` has a subject".to_string()));
            return;
        };
        let value = subject_of(body, id, e, ctx, ix);
        let stray = lead.iter().any(|n| match body.node(*n) {
            Node::Text(t) => !t.trim().is_empty(),
            _ => true,
        });
        if stray {
            out.push(blocked(
                "a `{#match}` holds only its arms: nothing may come before the first".to_string(),
            ));
            return;
        }
        let mut arms = Vec::new();
        for (marker, run) in &branches {
            let Some(arm) = marker.arm else {
                out.push(blocked(
                    "a `{#match}` arm is `{:Some(x)}` or `{:None}`".to_string(),
                ));
                return;
            };
            // A case as its WIT case is named, `circle` for `Circle`
            // (ADR-0061), and the language's own four too, `some` for
            // `Some`: one name, the browser's wire's, whoever wrote the
            // value (ADR-0206).
            let case = crate::wit::ident(arm.short());
            let (scope, written) = ctx.binding(&arm.bindings, ix);
            let (binding, fields) = match written.as_slice() {
                [one] => (Some(one.clone()), Vec::new()),
                many => (None, many.to_vec()),
            };
            arms.push(Arm {
                case,
                binding,
                fields,
                body: lower_run(body, run, &scope, ix),
            });
        }
        out.push(Chunk::Dynamic(Part::Match { id, value, arms }));
        return;
    }
    // Not represented. Blocked rather than dropped: a region the IR does not
    // understand must not become an empty string, or the page renders without
    // it and looks correct.
    out.push(Chunk::Dynamic(Part::Blocked {
        reason: format!("the block directive `{d}` has no template-IR representation"),
        at: d.to_string(),
    }));
}

/// `href="/stores/{id}"`: static text and value paths, each value escaped for
/// the attribute's context when rendered (ADR-0042).
fn interpolated_attribute(
    body: &Body,
    name: &str,
    text: &str,
    parts: &[ExprId],
    owner: ElementId,
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
) -> Part {
    let blocked = |reason: &str| Part::Blocked {
        reason: reason.to_string(),
        at: format!("{name}={text}"),
    };
    let context = Context::of_attribute(name);
    if BOOLEAN_ATTRIBUTES.contains(&name) {
        return blocked("a boolean attribute's value is its presence, not text");
    }
    if context == Context::Style {
        return blocked("escaping inside a CSS declaration is not decided (ADR-0042)");
    }
    let mut segments = Vec::new();
    let mut values = Vec::new();
    let mut holes = parts.iter();
    let mut rest = unquote(text);
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        if open > 0 {
            segments.push(Segment::Static(escape_static_attribute(&rest[..open])));
        }
        if after[..close].trim().is_empty() {
            return blocked("`{}` holds no expression");
        }
        let Some(hole) = holes.next() else {
            return blocked("a hole in the attribute did not parse");
        };
        // A computed one is the attribute's whole value (ADR-0226), which a
        // host computes as it does a hole between tags.
        let Some(path) = ctx.path(body, *hole) else {
            return blocked(
                "a hole in an attribute's text is a value path, and a computed value is \
                 written as the attribute's whole value, `name={..}` (ADR-0226)",
            );
        };
        let path = ctx.read(path);
        values.push((path.clone(), *hole));
        segments.push(Segment::Value(path));
        rest = &after[close + 1..];
    }
    if !rest.is_empty() {
        segments.push(Segment::Static(escape_static_attribute(rest)));
    }
    // The author's text fixes a URL's scheme and where its path starts; a
    // value only fills in components.
    if context == Context::Url && !matches!(segments.first(), Some(Segment::Static(_))) {
        return blocked(
            "a URL with holes must begin with text, which fixes its scheme; a URL \
             that is wholly a value is written `name={value}`",
        );
    }
    let id = ix.part();
    for (path, e) in values {
        ix.read(id, &path, ReadKind::Attribute, ReadAt::Expr(e), ctx);
    }
    Part::InterpolatedAttribute {
        id,
        owner,
        name: name.to_string(),
        segments,
        context,
    }
}

/// Does the directive open a block named `head`: `{#if ..}`, not `{#iffy}`?
fn opens(d: &str, head: &str) -> bool {
    d.strip_prefix(head)
        .is_some_and(|r| r.starts_with(char::is_whitespace) || r.starts_with('}'))
}

/// A branch marker's fields: as written, its condition, and the arm it names.
struct Marker<'b> {
    written: &'b str,
    condition: Option<ExprId>,
    arm: &'b Option<crate::hir::TemplateArm>,
}

/// A block's children split at its branch markers: the run before the first
/// marker, then each marker with the run after it (ADR-0042).
fn split_branches<'b>(
    body: &'b Body,
    children: &[NodeId],
) -> (Vec<NodeId>, Vec<(Marker<'b>, Vec<NodeId>)>) {
    let mut lead = Vec::new();
    let mut branches: Vec<(Marker<'b>, Vec<NodeId>)> = Vec::new();
    for c in children {
        match (body.node(*c), branches.last_mut()) {
            (
                Node::Branch {
                    marker,
                    condition,
                    arm,
                },
                _,
            ) => branches.push((
                Marker {
                    written: marker,
                    condition: *condition,
                    arm,
                },
                Vec::new(),
            )),
            (_, Some((_, run))) => run.push(*c),
            (_, None) => lead.push(*c),
        }
    }
    (lead, branches)
}

/// Some nodes, lowered in order and coalesced.
fn lower_run(body: &Body, nodes: &[NodeId], ctx: &Lowering<'_>, ix: &mut Indexer) -> Vec<Chunk> {
    let mut out = Vec::new();
    ix.depth += 1;
    for n in nodes {
        lower_node(body, *n, ctx, ix, &mut out);
    }
    ix.depth -= 1;
    coalesce(out)
}

/// `{#if a} A {:else if b} B {:else} C {/if}`: a conditional on `a` whose
/// `otherwise` is a conditional on `b`, whose `otherwise` is C. Ids follow the
/// document: the nested conditional's comes after A's parts.
/// **A block's subject, as the renderer reads it** (ADR-0073, ADR-0229): a
/// value's path, or a computed one's, which the compiler names, read with the
/// values it is computed from. Until ADR-0229 a computed one was refused: "an
/// `{#if}` condition must be a value path".
fn subject_of(body: &Body, id: PartId, e: ExprId, ctx: &Lowering<'_>, ix: &mut Indexer) -> String {
    if let Some(value) = ctx.path(body, e).map(|v| ctx.read(v)) {
        ix.read(id, &value, ReadKind::Subject, ReadAt::Expr(e), ctx);
        return value;
    }
    let inputs = inputs_of(body, e, ctx);
    let value = ix.computed_path(id, &inputs);
    ix.read(id, &value, ReadKind::Subject, ReadAt::Expr(e), ctx);
    if let Some(read) = ix.reads.last_mut() {
        read.inputs = Some(inputs);
    }
    value
}

fn conditional(
    body: &Body,
    id: PartId,
    value: String,
    then: &[NodeId],
    rest: &[(Marker<'_>, Vec<NodeId>)],
    ctx: &Lowering<'_>,
    ix: &mut Indexer,
) -> Chunk {
    let then = lower_run(body, then, ctx, ix);
    let blocked = |reason: &str, at: &str| {
        vec![Chunk::Dynamic(Part::Blocked {
            reason: reason.to_string(),
            at: at.to_string(),
        })]
    };
    let otherwise = match rest.split_first() {
        None => Vec::new(),
        Some(((marker, run), more)) => match marker.condition {
            Some(c) => {
                let nested = ix.part();
                let v = subject_of(body, nested, c, ctx, ix);
                // A block of its own: what its arms hold ends when it shows
                // another (ADR-0144).
                ix.blocks.push(nested);
                let chunk = conditional(body, nested, v, run, more, ctx, ix);
                ix.blocks.pop();
                vec![chunk]
            }
            None if marker.written.trim() == "{:else}"
                && marker.arm.is_none()
                && more.is_empty() =>
            {
                lower_run(body, run, ctx, ix)
            }
            None => blocked(
                "an `{#if}` takes `{:else if c}` markers and one `{:else}`, last",
                marker.written,
            ),
        },
    };
    Chunk::Dynamic(Part::Conditional {
        id,
        value,
        then,
        otherwise,
    })
}

/// The template's semantic identity.
///
/// Over the IR's SHAPE — part kinds, names, contexts, static bytes, nesting and
/// order — because that is what makes a local part id mean something. Not over
/// source bytes: a comment or a reflow must not change it, and E7V's scheme-2
/// reasoning is the same argument one layer up.
///
/// FNV-1a. Not cryptographic: this distinguishes templates within a build, and
/// what refuses across builds is E7V's decision, which has its own hash.
fn schema_of(params: &[String], chunks: &[Chunk]) -> String {
    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn walk(h: &mut u64, chunks: &[Chunk]) {
        for c in chunks {
            match c {
                Chunk::Static(s) => {
                    feed(h, b"S");
                    feed(h, s.as_bytes());
                }
                Chunk::Dynamic(p) => {
                    feed(h, p.kind().as_bytes());
                    match p {
                        Part::Text { value, context, .. } => {
                            feed(h, value.as_bytes());
                            feed(h, format!("{context:?}").as_bytes());
                        }
                        Part::Attribute {
                            name,
                            value,
                            context,
                            ..
                        } => {
                            feed(h, name.as_bytes());
                            feed(h, value.as_bytes());
                            feed(h, format!("{context:?}").as_bytes());
                        }
                        Part::BooleanAttribute { name, value, .. } => {
                            feed(h, name.as_bytes());
                            feed(h, value.as_bytes());
                        }
                        Part::Event {
                            event,
                            handler,
                            renames,
                            modifiers,
                            ..
                        } => {
                            feed(h, event.as_bytes());
                            feed(h, handler.as_bytes());
                            for (from, to) in renames {
                                feed(h, from.as_bytes());
                                feed(h, to.as_bytes());
                            }
                            for m in modifiers {
                                feed(h, b"|");
                                feed(h, m.as_bytes());
                            }
                        }
                        Part::Conditional {
                            value,
                            then,
                            otherwise,
                            ..
                        } => {
                            feed(h, value.as_bytes());
                            walk(h, then);
                            feed(h, b"|");
                            walk(h, otherwise);
                        }
                        Part::Each {
                            collection,
                            binding,
                            key,
                            body,
                            ..
                        } => {
                            feed(h, collection.as_bytes());
                            feed(h, binding.as_bytes());
                            feed(h, key.as_deref().unwrap_or("-").as_bytes());
                            walk(h, body);
                        }
                        Part::Match { value, arms, .. } => {
                            feed(h, value.as_bytes());
                            for a in arms {
                                feed(h, b"|");
                                feed(h, a.case.as_bytes());
                                feed(h, a.binding.as_deref().unwrap_or("-").as_bytes());
                                walk(h, &a.body);
                            }
                        }
                        Part::Stream {
                            query,
                            args,
                            streamed,
                            placeholder,
                            ready,
                            failed,
                            ..
                        } => {
                            feed(h, query.as_bytes());
                            for a in args {
                                feed(h, a.as_bytes());
                            }
                            feed(h, if *streamed { b"streamed" } else { b"at-once" });
                            walk(h, placeholder);
                            for arm in [ready, failed] {
                                feed(h, b"|");
                                feed(h, arm.binding.as_deref().unwrap_or("-").as_bytes());
                                walk(h, &arm.body);
                            }
                        }
                        Part::InterpolatedAttribute {
                            name,
                            segments,
                            context,
                            ..
                        } => {
                            feed(h, name.as_bytes());
                            for s in segments {
                                match s {
                                    Segment::Static(t) => {
                                        feed(h, b"S");
                                        feed(h, t.as_bytes());
                                    }
                                    Segment::Value(v) => {
                                        feed(h, b"V");
                                        feed(h, v.as_bytes());
                                    }
                                }
                            }
                            feed(h, format!("{context:?}").as_bytes());
                        }
                        Part::Instance { path, args, .. } => {
                            feed(h, path.as_bytes());
                            for (a, b) in args {
                                feed(h, a.as_bytes());
                                feed(h, b.as_bytes());
                            }
                        }
                        Part::RawHtml {
                            value, capability, ..
                        } => {
                            feed(h, value.as_bytes());
                            feed(h, capability.as_bytes());
                        }
                        Part::Title {
                            pieces: content, ..
                        }
                        | Part::Meta { content, .. } => {
                            if let Part::Meta { attribute, key, .. } = p {
                                feed(h, attribute.as_bytes());
                                feed(h, key.as_bytes());
                            }
                            for p in content {
                                match p {
                                    TitlePiece::Text(t) => {
                                        feed(h, b"T");
                                        feed(h, t.as_bytes());
                                    }
                                    TitlePiece::Value(v) => {
                                        feed(h, b"V");
                                        feed(h, v.as_bytes());
                                    }
                                }
                            }
                        }
                        Part::Canonical { route, args, .. } => {
                            feed(h, route.as_bytes());
                            for (name, p) in args {
                                feed(h, name.as_bytes());
                                match p {
                                    TitlePiece::Text(t) => {
                                        feed(h, b"T");
                                        feed(h, t.as_bytes());
                                    }
                                    TitlePiece::Value(v) => {
                                        feed(h, b"V");
                                        feed(h, v.as_bytes());
                                    }
                                }
                            }
                        }
                        Part::Blocked { reason, at } => {
                            feed(h, reason.as_bytes());
                            feed(h, at.as_bytes());
                        }
                    }
                }
            }
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in params {
        feed(&mut h, p.as_bytes());
    }
    walk(&mut h, chunks);
    format!("{h:016x}")
}

/// A source attribute value without its delimiters.
///
/// Only a matched pair is removed. `"a"b"` keeps everything, because the value
/// is then not what the outer quotes suggest and guessing would change it.
fn unquote(v: &str) -> &str {
    let bytes = v.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[0] == bytes[bytes.len() - 1]
    {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

/// Source text between tags, escaped once at build time.
fn escape_static_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// **A `<textarea>`'s value written in the source, as its text** (ADR-0221),
/// escaped as the renderer escapes one a value gives
/// (`pw_render::escape::content`). An attribute written in the source holds
/// no newline, so none is doubled.
fn escape_static_content(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_static_attribute(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
    out
}
