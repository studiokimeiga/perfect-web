//! **A page no reader decides is one document** (ADR-0409): which
//! pages a host renders once and sends to everyone.
//!
//! Derived, and never declared, so that it cannot be wrong. A page is
//! rendered from its address's values, its queries' and streams' values,
//! what its reader answers to the predicates it asks, and its signals' first
//! values. Only the last are the build's. A page with none of the others
//! shows every reader the same document, and one on which no handler sends a
//! command never asks a server anything, so it has no use for a session.
//!
//! A value the device keeps changes neither: the document holds its declared
//! default whoever asks, hidden until the browser has read the device
//! (ADR-0366).

use std::collections::{BTreeMap, BTreeSet};

use crate::backend::js::Compiled;
use crate::backend::wasm::Encoding;
use crate::page_values::{PageValues, Planned};
use crate::template_ir::{Chunk, Part, Template};

/// **Each page no reader decides, marked in its plan.** Run once every
/// handler is compiled: what a handler sends is known only then.
pub(crate) fn mark(pages: &mut [Planned], templates: &[Template], handlers: &[Compiled]) {
    let commands = commands_by_handler(handlers);
    for page in pages {
        let Ok(plan) = &mut page.plan else { continue };
        plan.prerender = no_reader_decides(plan)
            && templates
                .iter()
                .find(|t| t.path == plan.page)
                .is_some_and(|t| Reach::new(templates, &commands).stays_in_the_browser(t));
    }
}

/// **Whether the plan's document is the same for every reader**: it has an
/// address of its own that gives it no value, no query or stream fills it, it
/// asks its reader nothing, and it is no one's. `predicates` is read here and
/// not left to PW5049, which reads what the page's own body asks: a layout or
/// a view it shows asks in the same document.
fn no_reader_decides(plan: &PageValues) -> bool {
    plan.route.is_some()
        && plan.params.is_empty()
        && plan.bindings.is_empty()
        && plan.streams.is_empty()
        && plan.predicates.is_empty()
        && plan.scope == "public"
}

/// What each compiled handler sends, by its identity. One that did not
/// compile is not here, and a build with one is refused.
fn commands_by_handler(handlers: &[Compiled]) -> BTreeMap<&str, &[String]> {
    handlers
        .iter()
        .filter_map(|h| match &h.module {
            Encoding::Encoded(m) => Some((m.identity.as_str(), m.commands.as_slice())),
            _ => None,
        })
        .collect()
}

/// The templates a page's markup reaches, each read once: a view may
/// contain itself (ADR-0203).
struct Reach<'a> {
    templates: &'a [Template],
    commands: &'a BTreeMap<&'a str, &'a [String]>,
    seen: BTreeSet<&'a str>,
}

impl<'a> Reach<'a> {
    fn new(templates: &'a [Template], commands: &'a BTreeMap<&'a str, &'a [String]>) -> Self {
        Reach {
            templates,
            commands,
            seen: BTreeSet::new(),
        }
    }

    /// **Whether no handler on the page sends a command**: in its markup, its
    /// layout's (which is in it), each block's arms and each view it reaches.
    fn stays_in_the_browser(&mut self, page: &'a Template) -> bool {
        self.chunks(&page.chunks)
    }

    fn chunks(&mut self, chunks: &'a [Chunk]) -> bool {
        chunks.iter().all(|chunk| match chunk {
            Chunk::Static(_) => true,
            Chunk::Dynamic(part) => self.part(part),
        })
    }

    fn part(&mut self, part: &'a Part) -> bool {
        let here = match part {
            // Unknown is not known to stay: only what compiled and sends
            // nothing does.
            Part::Event { handler, .. } => self
                .commands
                .get(handler.as_str())
                .is_some_and(|sent| sent.is_empty()),
            Part::Instance { path, .. } => self.view(path),
            _ => true,
        };
        here && part.nested().into_iter().all(|region| self.chunks(region))
    }

    fn view(&mut self, path: &'a str) -> bool {
        if !self.seen.insert(path) {
            return true;
        }
        self.templates
            .iter()
            .find(|t| t.path == path)
            .is_some_and(|t| self.chunks(&t.chunks))
    }
}
