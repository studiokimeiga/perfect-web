// E7-R — the browser runtime.
//
// It reads the parts manifest the document carries, asks `decide` whether a
// handler may attach, attaches the ones it authorises, and updates **only the
// parts whose values changed**.
//
// # What it never does
//
// No component tree, no replay, no re-render. The document arrived complete
// from the server; this attaches behaviour to elements that already exist and
// replaces the contents of ranges that already exist. Charter §14 M7 gate 4 —
// "no full-tree hydration occurs" — is a property of there being no tree here
// to hydrate.
//
// # Identity comes from the manifest, never from the DOM
//
// A part is found by the id the compiler assigned: `data-pw="7"` for an
// element-anchored part, `<!--pw:s3-->…<!--pw:e3-->` for a range. Nothing is
// located by tag name, class or position, because those change when a designer
// edits the page and an identity that moves is not an identity.

const parts = JSON.parse(document.getElementById("pw-parts")?.textContent ?? "{}");

/**
 * **This document's life** (the soft navigation's ADR). Every listener the
 * runtime adds is added with its signal, and its subscription asks under it:
 * a navigation that shows the next page in this one's layout hands the
 * document to a runtime of the next document's, and aborting this ends all
 * of this one's at once, no late batch of it applied to the next.
 */
const life = new AbortController();

// --- addressing ----------------------------------------------------------
//
// Architect ruling, 2026-08-06:
//
//   `TemplateSchemaId + LocalPartId` identifies a part POSITION. A live
//   document needs `TemplateSchemaId + InstancePath + LocalPartId`.
//
// The distinction is not about loops. A template part DEFINITION is one thing
// and a document part INSTANCE is another, and the same gap appears with a
// component used twice, a conditional region recreated after toggling, and a
// streamed instance. So `InstancePath` is generic: a frame is a repeatable
// scope, and today that scope is a keyed `{#each}`.
//
// ONE traversal builds the index. After that an address is a map lookup, not a
// DOM walk — the comments are the structural truth and this is the cheap view
// of it.

/** `[[eachPartId, token], ...]` → a stable string key. */
function addressOf(path, part) {
  // Matches `pw_document::PartAddress::key` exactly. A second spelling would
  // be a second entry in the index for one location, and a patch aimed at the
  // other spelling would find nothing — silently, because "no such address" and
  // "nothing to do" look the same from here.
  const frames = path.map(([e, t]) => `${e}@${t}`);
  return `${parts.schema}/${frames.join("/")}|${part}`;
}

/** address → { element, template } or { start, end, template } */
const index = new Map();

/**
 * **Each template's parts, by its path** (ADR-0203): the page's, and each
 * one a view that contains itself renders an instance of. A part's id is
 * its template's, so an instance's parts are read in its own manifest, and
 * its button numbered as one of the page's is not taken for it.
 */
const PAGE = parts.template ?? "";
const manifests = new Map([
  [PAGE, parts.parts ?? []],
  ...Object.entries(parts.templates ?? {}).map(([path, t]) => [path, t.parts ?? []]),
]);
const entries = new Map(
  [...manifests].map(([path, list]) => [path, new Map(list.map((p) => [p.id, p]))]),
);

/** Each view's template, as JSON: what the browser's renderer renders an
 * instance in a block with (ADR-0203). Empty where the page has none. */
const TEMPLATES = Object.values(parts.templates ?? {}).map((t) => t.template);
const TEMPLATES_JSON = TEMPLATES.length ? JSON.stringify(TEMPLATES) : "";

/** A renderer's request, with each view's template where the page has one. */
function withTemplates(request) {
  return TEMPLATES_JSON ? `${request.slice(0, -1)},"templates":${TEMPLATES_JSON}}` : request;
}

function buildIndex() {
  index.clear();
  const path = [];
  // The template each frame's parts are in: a loop's row is its loop's, and
  // an instance is its view's (ADR-0203).
  const within = [PAGE];
  // A range being walked, by its path and its id: a view inside itself has
  // a part of the same id open around it (ADR-0203).
  const open = new Map();
  const at = () => path.map(([e, t]) => `${e}@${t}`).join("/");
  const walker = document.createTreeWalker(
    document.body,
    NodeFilter.SHOW_COMMENT | NodeFilter.SHOW_ELEMENT,
  );
  let node;
  while ((node = walker.nextNode())) {
    if (node.nodeType === Node.ELEMENT_NODE) {
      const owner = node.dataset?.pw;
      if (owner !== undefined) {
        // An element-anchored part is addressed by the path it sits in, which
        // is what distinguishes the three Add buttons that all carry
        // `data-pw="0"`.
        index.set(addressOf(path, `e${owner}`), { element: node, template: within.at(-1) });
      }
      continue;
    }
    const data = node.data ?? "";
    if (!data.startsWith("pw:")) continue;

    const m = /^pw:([se])(\d+)(?:@([A-Za-z0-9_-]+))?$/.exec(data);
    if (!m) continue;
    const [, side, id, token] = m;

    if (token) {
      // An INSTANCE boundary, a loop's row or a view's (ADR-0203): push a
      // frame, or pop it. A view's instance is a range too, of its part in
      // the template around it, which a change replaces whole.
      const template = within.at(-1);
      if (side === "s") {
        const part = entries.get(template)?.get(Number(id));
        const view = part?.kind === "instance";
        if (view) open.set(`${at()}|${id}@${token}`, { start: node, path: [...path], template });
        path.push([Number(id), token]);
        within.push(view ? part.value : template);
      } else {
        path.pop();
        within.pop();
        const key = `${at()}|${id}@${token}`;
        const started = open.get(key);
        if (started) {
          index.set(addressOf(started.path, id), {
            start: started.start,
            end: node,
            template: started.template,
          });
          open.delete(key);
        }
      }
      continue;
    }
    const key = `${at()}|${id}`;
    if (side === "s") {
      open.set(key, { start: node, path: [...path], template: within.at(-1) });
    } else {
      const started = open.get(key);
      if (started) {
        index.set(addressOf(started.path, id), {
          start: started.start,
          end: node,
          template: started.template,
        });
        open.delete(key);
      }
    }
  }
  // The page's title (ADR-0183): the document's, with no range in the body.
  for (const p of parts.parts ?? []) {
    if (p.kind === "title") index.set(addressOf([], p.id), { title: true });
  }
  return index.size;
}

/** Every address for a part id of `template`, the page's unless named,
 * across every instance of it. */
function addressesFor(part, template = PAGE) {
  const suffix = `|${part}`;
  return [...index]
    .filter(([k, v]) => k.endsWith(suffix) && (v.template ?? PAGE) === template)
    .map(([k]) => k);
}

/**
 * Replace a range's contents, leaving its anchors and everything else alone.
 *
 * The anchors are never touched, so the part keeps its identity across the
 * update — which is what lets it be updated again.
 *
 * A range that holds the text already is left as it is (ADR-0182). Text
 * written again, the same, is a change to a live region, and a screen reader
 * says the region again: one Add said "Items in cart: 1" up to four times,
 * written by its speculation, its answer and the cart's new value. Left as it
 * is, a selection in it stays too.
 */
function setRange(address, text) {
  const r = index.get(address);
  // The page's title is the document's (ADR-0183), set as it is shown.
  if (r?.title) {
    if (document.title !== text) document.title = text;
    return true;
  }
  if (!r?.start) return false;
  if (holdsText(r, text)) return true;
  let n = r.start.nextSibling;
  while (n && n !== r.end) {
    const next = n.nextSibling;
    n.remove();
    n = next;
  }
  r.end.parentNode.insertBefore(document.createTextNode(text), r.end);
  return true;
}

/** Whether a range holds `text`, and nothing but text. */
function holdsText(r, text) {
  let shown = "";
  for (let n = r.start.nextSibling; n && n !== r.end; n = n.nextSibling) {
    if (n.nodeType !== Node.TEXT_NODE) return false;
    shown += n.data;
  }
  return shown === text;
}

/**
 * The nodes one loop instance owns, its anchors included.
 *
 * `null` when the instance is not in the document — which a patch targeting a
 * nonexistent instance must be refused for, rather than silently doing nothing.
 */
function instanceRange(scope, token) {
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);
  let start = null;
  let node;
  while ((node = walker.nextNode())) {
    if (node.data === `pw:s${scope}@${token}`) start = node;
    else if (node.data === `pw:e${scope}@${token}` && start) {
      return { start, end: node };
    }
  }
  return null;
}

/** Every node from an instance's start anchor to its end anchor, inclusive. */
function instanceNodes(range) {
  const nodes = [range.start];
  let n = range.start.nextSibling;
  while (n && n !== range.end) {
    nodes.push(n);
    n = n.nextSibling;
  }
  nodes.push(range.end);
  return nodes;
}

/**
 * Move nodes without recreating them, preserving as much state as the engine
 * allows.
 *
 * `Element.moveBefore()` is an ATOMIC move: the node is never removed from the
 * document, so focus, `:active`, CSS animations and transitions, iframe load
 * state, fullscreen, popover and modal state all survive. `insertBefore()` on
 * an attached node is specified as a remove followed by an insert, and every
 * one of those is lost.
 *
 * Verified against MDN (`Element/moveBefore`, read 2026-08-07): the method is
 * on the PARENT, the moved node must be an Element or CharacterData — comment
 * anchors qualify — and it is explicitly not Baseline. So the fallback is not
 * optional, and `docs/ASSUMPTIONS.md` records what it cannot preserve.
 *
 * The fallback restores focus and the caret by hand. That is a smaller claim
 * than the atomic move makes, and stating it is the point: an implementation
 * that quietly did less would pass the same focus test.
 */
const ATOMIC_MOVE = typeof Element.prototype.moveBefore === "function";

function moveNodes(parent, nodes, anchor) {
  if (ATOMIC_MOVE) {
    for (const node of nodes) parent.moveBefore(node, anchor);
    return "atomic";
  }
  const active = document.activeElement;
  const caret =
    active && "selectionStart" in active
      ? [active.selectionStart, active.selectionEnd]
      : null;
  const scroll = active?.scrollTop;
  for (const node of nodes) parent.insertBefore(node, anchor);
  if (active && active.isConnected && document.activeElement !== active) {
    active.focus({ preventScroll: true });
    if (caret) active.setSelectionRange(caret[0], caret[1]);
    if (scroll !== undefined) active.scrollTop = scroll;
  }
  return "restored";
}

/** Parse instance markup into nodes, anchors included. */
function parseInstance(html) {
  const template = document.createElement("template");
  template.innerHTML = html;
  return [...template.content.childNodes];
}

/**
 * Apply a structural operation to a keyed collection.
 *
 * Returns false when the operation names an instance the document does not
 * have. A patch that addressed nothing must be REFUSED rather than ignored:
 * the two are indistinguishable from the outside, and one of them means the
 * page and the server disagree about what the document contains.
 */
function applyList(key, scope, op) {
  switch (op.op) {
    case "insert_before":
    case "insert_after": {
      const loop = index.get(key);
      if (!loop) return false;
      let parent;
      let anchor;
      if (op.instance == null) {
        // No anchor instance: the head for `insert_before`, the tail for
        // `insert_after`. The only way into an EMPTY collection, which has no
        // instance to sit beside.
        parent = loop.start.parentNode;
        anchor = op.op === "insert_before" ? loop.start.nextSibling : loop.end;
      } else {
        const at = instanceRange(scope, op.instance);
        if (!at) return false;
        parent = at.start.parentNode;
        anchor = op.op === "insert_before" ? at.start : at.end.nextSibling;
      }
      for (const node of parseInstance(op.html)) parent.insertBefore(node, anchor);
      return true;
    }

    case "remove_instance": {
      const at = instanceRange(scope, op.instance);
      if (!at) return false;
      for (const node of instanceNodes(at)) node.remove();
      return true;
    }

    case "move_instance": {
      const at = instanceRange(scope, op.instance);
      if (!at) return false;
      // The NODES are moved, not recreated. That is the whole reason a list is
      // keyed: a move that deleted and re-rendered would lose focus, scroll,
      // form state and any identity a test marked — and would look identical
      // in a screenshot.
      const nodes = instanceNodes(at);
      let anchor;
      if (op.after) {
        const after = instanceRange(scope, op.after);
        if (!after) return false;
        anchor = after.end.nextSibling;
      } else {
        // To the front: just inside the LOOP's own range, whose address is
        // the patch's target. Reconstructing that address from the scope id
        // would assume the loop is at the top level, which a nested loop is
        // not.
        const loop = index.get(key);
        if (!loop) return false;
        anchor = loop.start.nextSibling;
      }
      // Already where it is asked to go: its own first node is the anchor,
      // and there is nothing to move. Moved anyway, `moveBefore` put the
      // instance's nodes before its own start, and the list's anchors no
      // longer nested (found 2026-10-03, ADR-0168).
      if (nodes.includes(anchor)) return true;
      window.__pw.moveKind = moveNodes(at.start.parentNode, nodes, anchor);
      return true;
    }

    default:
      return false;
  }
}

// --- the compatibility decision -----------------------------------------
//
// The same wasm E7V compiled, and the same rule: the gate FAILS CLOSED. A
// handler that attached because the decision had not loaded would be the bypass
// this design exists to prevent.

let decide = null;

/** **The build that served this runtime** (ADR-0300), by its name, from its
 * handler table: empty until the table is read. */
let servedBuild = "";

async function bootDecision() {
  const response = await fetch("/pw-resume.wasm");
  const { instance } = await WebAssembly.instantiateStreaming(response, {});
  const { memory, alloc, decide_manifest, last_recovery, know, know_document } =
    instance.exports;

  const write = (s) => {
    const bytes = new TextEncoder().encode(s);
    const ptr = alloc(bytes.length);
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  };
  // What this build compiled (ADR-0132), from the server that served this
  // runtime and never from the document: a document cached from another
  // build names handlers this one may not have. If the table cannot be read,
  // nothing is known and every handler is refused, which is the closed side.
  const table = await fetch("/pw-handlers").then((r) => (r.ok ? r.text() : ""));
  const known = know(...write(table));
  log.push(`knows ${known} handler(s)`);
  // **What this build says of this page's documents** (ADR-0300): its name,
  // and this page's document schema and scope, from the same table and never
  // from the document; the decision holds each handler's document to them.
  const lines = table.split("\n");
  servedBuild = lines.find((l) => l.startsWith("#build|"))?.slice("#build|".length) ?? "";
  const page = lines.find((l) => l.startsWith(`#page|${PAGE}|`));
  if (page && know_document) {
    const [, , schema, scope] = page.split("|");
    know_document(...write(`${schema}|${scope}`));
    log.push(`knows ${PAGE}'s documents: ${schema} ${scope}`);
  }
  // `last_recovery` returns an INDEX into this list, not a pointer. A first
  // version read it as one and every decision threw `Start offset -1 is
  // outside the bounds of the buffer`, which the page reported as "boot
  // failed" — so no handler attached and the button was inert.
  //
  // Worth recording: fail-closed did its job. A bug in the code that READS the
  // decision produced a page that does nothing, not a page that attaches
  // anyway. That is the difference the design was for.
  //
  // In `pw-resume-wasm`'s order, from 0: `RefetchRegion` is 0 there. Until
  // 2026-10-03 this list began with a "none" the ABI does not have, so every
  // recovery was read one place off. A region to refetch read as "none", a
  // document to reload as "rerender-private-slot". Nothing acted on a
  // recovery then, so only the log was wrong (ADR-0155).
  const RECOVERY = [
    "refetch-region",
    "rerender-private-slot",
    "reload",
    "retry-interaction",
    "require-user-confirmation",
    "reject-irrecoverable",
  ];

  decide = (manifest) => {
    const [ptr, len] = write(manifest);
    // 0 resume, 1 migrate; anything else is a refusal.
    const code = decide_manifest(ptr, len);
    return {
      attach: code === 0 || code === 1,
      recovery: RECOVERY[last_recovery()] ?? "unknown",
      code,
    };
  };
}

// --- attaching -----------------------------------------------------------

/** What the page reports about itself, for the tests to read. */
const log = [];
window.__pw = {
  log,
  parts,
  /** The `PartAddress`es the last interaction updated. */
  updated: [],
  ready: false,
  address: (path, part) => addressOf(path, part),
  /** A page's address from its route and its parameters' values, as a
   * navigation goes to it (ADR-0280). */
  pageAddress: (route, args) => pageAddress(route, args),
  /** The build that served this runtime (ADR-0300). */
  build: () => servedBuild,
  indexSize: () => index.size,
  /** The bytes of command bodies kept alive and not yet answered, and what
   * they may come to (ADR-0268). */
  keepalive: () => ({ inFlight: keptAlive, budget: keepaliveBudget }),
  /** How long each send of a command waits for its answer (ADR-0381). */
  deadline: () => sendDeadline,
  /** How long a press's module may take to load (ADR-0382). */
  loadDeadline: () => loadDeadline,
  /** How many presses have not finished (ADR-0280's `pressing`). */
  pressing: () => pressing.size,
  /** Each streamed region that settled after boot, and who filled it:
   * `browser` where the platform applied its patch, `runtime` where this
   * did (ADR-0148). */
  settled: [],
};

// The manifest a handler presents, in the ABI `pw-resume-wasm` reads:
//
//   scheme|abi|build|handler|capture|document|scope|captures|construct
//
// Composed from what the SERVER put in the document, not from constants here.
// A test that wants an incompatible manifest changes the served data; it does
// not edit this file, which is what makes the refusal path testable rather
// than a branch nobody exercises.
const RESUME_FIELDS = [
  "scheme",
  "abi",
  "build",
  "handler",
  "capture",
  "document",
  "scope",
  "captures",
  "construct",
];

/**
 * The manifest for ONE handler.
 *
 * The document carries a base manifest and the part carries which handler it
 * is, so two handlers on one page present two manifests and `decide` answers
 * about the one being attached. A single page-wide manifest would authorise
 * every handler on the strength of whichever one it described.
 */
function manifestFor(part) {
  const r = parts.resume;
  if (!r) return null;
  // By the handler's IDENTITY (ADR-0132), which names its code; a name does
  // not, and a handler that calls nothing has none.
  const per = (r.handlers ?? {})[part?.value] ?? {};
  return RESUME_FIELDS.map((f) => per[f] ?? r[f] ?? "").join("|");
}

// --- E7-L: handler code, loaded on interaction --------------------------
//
// Architect ruling, 2026-08-07: handler bytes absent before interaction; the
// exact handler fetched on first interaction; E7V check → Authorised → attach
// → run; unrelated handler code still absent.
//
// So the document carries no behaviour. An element with a handler gets a
// listener whose whole job is to LOAD the handler, check it, and run it — and
// the second press finds it already loaded.

/** handler identity → the module, or the in-flight promise for it. */
const loaded = new Map();

/**
 * Fetch a handler by IDENTITY.
 *
 * By identity rather than by name, because a changed implementation is a
 * different identity and therefore a different URL. A name-keyed URL would let
 * a cache serve yesterday's behaviour into today's document, and the resume
 * check would pass — it compares identities, and the document's identity would
 * be the new one.
 *
 * In flight is remembered, not just resolved: two fast presses must not become
 * two fetches, and a `has()` check that only saw finished loads would let the
 * second press start a second one.
 */
const attempts = new Map();

function loadHandler(identity) {
  if (loaded.has(identity)) return loaded.get(identity);
  // A retry asks for a different specifier.
  //
  // The module map caches FAILURES as well as successes, so re-importing the
  // same specifier after a failed load returns the same rejected promise
  // forever — the network is never touched again and the button is
  // permanently dead. The identity still names the module; the counter only
  // makes the failed attempt distinguishable from the next one.
  const attempt = attempts.get(identity) ?? 0;
  const url = attempt === 0 ? `/handler/${identity}.mjs` : `/handler/${identity}.mjs?attempt=${attempt}`;
  const pending = within(import(url), `the handler ${identity}`).then((module) => {
    loaded.set(identity, module);
    log.push(`loaded ${identity}`);
    return module;
  });
  loaded.set(identity, pending);
  window.__pw.fetched = (window.__pw.fetched ?? 0) + 1;
  return pending;
}

/**
 * **A module a press needs, loaded within the deadline** (ADR-0382): its
 * handler's, the page's speculation module, the renderer that renders its
 * regions. A load given up fails as a failed load does, and is no longer
 * waited for, so the press after it, which waits for its turn (ADR-0152),
 * is not held by a module that never comes. The import itself cannot be
 * stopped: it is not waited for, and whatever it brings late is not used.
 */
function within(loading, what) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what}: none within ${loadDeadline} ms`)), loadDeadline);
  });
  return Promise.race([loading, late]).finally(() => clearTimeout(timer));
}

/**
 * **A page's commands run in the order it sent them** (ADR-0305). Each names
 * the latest command this page sent and has not been answered (`pw-after`),
 * and the server runs it only once that one has run, whichever request the
 * network brings first. Until 2026-10-10 each went its own way, and under
 * load "Clear", pressed after "Place order", arrived first: the cart was
 * emptied, and the order refused as nothing to order.
 */
async function command(component, args, interaction, retry) {
  const before = [...unanswered.values()];
  const after = [...unanswered.keys()].at(-1);
  let answered;
  unanswered.set(interaction, new Promise((resolve) => (answered = resolve)));
  try {
    return await send(component, args, interaction, retry, after, before);
  } finally {
    unanswered.delete(interaction);
    answered();
  }
}

/** Each command this page has sent and not been answered, by interaction, in
 * the order sent, as a promise of its answer (ADR-0305). */
const unanswered = new Map();

/**
 * A command, as a compiled handler calls one: the command's component id and
 * the arguments the handler computed.
 *
 * The server types the arguments by the command component's own parameters
 * and refuses anything else. A refusal is thrown, so the press fails visibly.
 * A command that ran and did not commit is not a refusal: the page learns what
 * happened from the resource, as it does when a command commits.
 *
 * **Sent again where no answer came** (ADR-0173), as the command's `retry`
 * clause says, which the compiled handler passes: the request may not have
 * arrived, or its answer may have been lost after the server committed, and
 * nothing here can tell which. Sent again with the same interaction, a
 * command the server ran answers with its first outcome (ADR-0121). An
 * answer of any kind, a refusal included, is an outcome, and never sent
 * again.
 *
 * **Kept alive past the page** (ADR-0268): a press's speculation is shown
 * before its request leaves (ADR-0122), and a link followed at once ended the
 * page and the request with it, a press shown taken and never made. A request
 * marked `keepalive` outlives its document (the Fetch standard), within 64 KiB
 * of such bodies in flight; one past what remains is sent as before.
 *
 * **Answered early** (ADR-0305): the one before it, `after`, had not reached
 * the server in time, and this one ran nothing. That is no outcome: it is sent
 * again once each command sent `before` it is answered, naming none, and it
 * is no resend of the `retry` clause's.
 *
 * **Answered within its deadline, or not at all** (ADR-0381): each send has
 * `sendDeadline` for its whole answer, its body's last byte included. Past
 * it the request is aborted, and that is no answer, as a request the network
 * dropped is: until 2026-10-21 a connection held open was waited for without
 * end, its press pending, and every press the page's order put behind it.
 */
async function send(component, args, interaction, retry, after, before) {
  const sent = JSON.stringify(args);
  const bytes = new Blob([sent]).size;
  for (let attempt = 0; ; attempt += 1) {
    let response;
    let body;
    const keepalive = keptAlive + bytes <= keepaliveBudget;
    if (keepalive) keptAlive += bytes;
    // A controller and a timer, which every browser this runtime runs in has
    // (`AbortSignal.timeout` is Safari 16's). Left to fire: aborting a
    // request already answered does nothing.
    const unheard = new AbortController();
    setTimeout(() => unheard.abort(), sendDeadline);
    try {
      response = await fetch(`/command/${encodeURIComponent(component)}`, {
        method: "POST",
        // The interaction this request belongs to (ADR-0121). One per press
        // and per command the press calls, made here and never by the
        // handler, and carried unchanged by any retry of this request: a
        // command declared `idempotent_by InteractionId` runs once for it
        // however many times the request is sent.
        //
        // TRACK SEAM (store-accounts): and the document this page is (Q3),
        // so the host answers the command for the reader the page was shown
        // to, and refuses it, in words told as a refusal is, where the
        // reader signed in or out in another tab or the host no longer holds
        // the page. A page the host did not serve has none to name.
        headers: {
          "content-type": "application/json",
          "pw-interaction": interaction,
          ...(after && { "pw-after": after }),
          ...(documentCursor > 0 ? { "pw-document": String(documentCursor) } : {}),
        },
        body: sent,
        keepalive,
        signal: unheard.signal,
      });
      // An answer cut off in transit is no answer.
      if (response.ok) body = await response.text();
    } catch (error) {
      if (keepalive) keptAlive -= bytes;
      // **Held for the one before it, not lost** (ADR-0381): the host runs a
      // command once the one it names has run (ADR-0305), so a send given up
      // while it named one was waiting its turn, whether that one has since
      // been answered or failed. As an early answer is, it is no outcome and
      // no resend of the `retry` clause's.
      if (unheard.signal.aborted && after !== undefined) {
        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);
        await Promise.all(before);
        after = undefined;
        attempt -= 1;
        continue;
      }
      if (!retry || attempt >= retry.max) throw new Unreachable(component, error);
      const why = unheard.signal.aborted ? ` within ${sendDeadline} ms` : ` (${error.message ?? error})`;
      log.push(`resent ${component}: no answer${why}`);
      await new Promise((resolve) => setTimeout(resolve, resendDelay(retry, attempt)));
      continue;
    }
    if (keepalive) keptAlive -= bytes;
    if (response.status === 409) {
      const early = await response.json().catch(() => null);
      if (early?.early) {
        log.push(`${component} sent again: the one before it had not arrived`);
        await Promise.all(before);
        after = undefined;
        attempt -= 1;
        continue;
      }
    }
    if (!response.ok) {
      // **Refused by its `requires`** (ADR-0302): the predicate and the words
      // a reader is told, which the host that refused sends.
      if (response.status === 403) {
        const refusal = await response.json().catch(() => null);
        if (refusal?.refused) throw new Refused(component, refusal.refused, refusal.says ?? "");
      }
      throw new Error(`command ${component} refused: HTTP ${response.status}`);
    }
    return JSON.parse(body);
  }
}

/**
 * The bytes of command bodies kept alive and not yet answered (ADR-0268),
 * and what they may come to: the Fetch standard refuses a `keepalive` request
 * that would take the page's past 64 KiB. `?keepalive=` lowers it, so a test
 * reaches the bound with the store's few bytes, as `?transport=` pins an
 * adapter; it never raises it.
 */
let keptAlive = 0;
const keepaliveBudget = Math.min(
  64 * 1024,
  Number(new URLSearchParams(location.search).get("keepalive") ?? 64 * 1024) || 0,
);

/**
 * How long to wait before sending a command again (ADR-0173): a second,
 * doubling with each attempt, as TanStack Query waits between retries
 * (`Math.min(1000 * 2 ** n, 30000)`), and drawn at random below that where
 * the clause says `jitter`, so that pages that lost one connection do not all
 * send again at once. A delay that did not double, `fixed`, left the language
 * with ADR-0215.
 */
function resendDelay(retry, attempt) {
  const bound = Math.min(1000 * 2 ** attempt, 30000);
  return retry.jitter ? Math.random() * bound : bound;
}

/**
 * **How long a send waits for its whole answer, and a press's module for its
 * load** (ADR-0381, ADR-0382): ten seconds each, as ky gives an attempt and
 * .NET's standard resilience handler does. A press then settles within its
 * sends' deadlines and the waits between them, which its `retry` clause
 * bounds: ten seconds with no retry, about 33 for the store's two resends.
 * `?deadline=` and `?load-deadline=` lower them, so that a test reaches them
 * in a second or two, as `?keepalive=` lowers the keepalive budget: never
 * above ten seconds, never below half of one, so that a link cannot make a
 * page's presses fail at once; and what is no time above zero asks for
 * nothing.
 */
function lowered(name) {
  const asked = Number(new URLSearchParams(location.search).get(name));
  return asked > 0 ? Math.min(10_000, Math.max(500, asked)) : 10_000;
}
const sendDeadline = lowered("deadline");
const loadDeadline = lowered("load-deadline");

// --- ADR-0122: optimistic transitions -----------------------------------
//
// ADR-0025: an optimistic clause targets a resource ENTRY, binds its current
// value, and transforms it with a pure function. The runtime holds the value
// it displayed, so a rejected command is undone by restoring that value, not
// by an inverse anyone wrote.
//
//   held        the authoritative value and version: the document's, then
//               each `entry_value` frame's
//   pending     speculations not yet reconciled, oldest first
//   displayed   fold(pending, held): what the speculated parts show
//
// A speculation is dropped when its command fails (restored) or when the
// entry reaches the version its commit produced (reconciled). The parts are
// then re-rendered from what remains, so a later press's speculation survives
// an earlier one's resolution, and nothing is restored over a newer value.

/** binding → { entry, version, raw, value, pending } */
const speculated = new Map();
for (const [binding, e] of Object.entries(parts.entries ?? {})) {
  speculated.set(binding, {
    entry: e.entry,
    version: e.version,
    raw: e.value,
    value: undefined,
    pending: [],
  });
}

let speculationModule = null;
let speculationOwner = null;

// A known native execution failure is never the load-only fallback.
class RetiredRenderer extends Error {}

/** The page's speculation module, loaded with the first press that needs it,
 * and the renderer with it where a speculation renders a region (ADR-0172):
 * a region is rendered between a batch's frames, so both are ready first.
 *
 * **Within the deadline, and not kept failed** (ADR-0382): until 2026-10-21 a
 * failed load was kept, and every later press that speculates failed with it
 * until the page was reloaded. A load that fails, or is given up, is
 * forgotten, and the next asks under another specifier, as a handler's does
 * (`loadHandler`), so that no failure the browser keeps answers it. */
function speculations() {
  if (!parts.speculation) return Promise.resolve(null);
  if (speculationModule) return speculationModule;
  const url =
    speculationAttempts === 0 ? parts.speculation : `${parts.speculation}?attempt=${speculationAttempts}`;
  const rendering = parts.regions ? renderModule() : null;
  const owner = { renderer: null, retired: false };
  const trackedRenderer = rendering?.then((exports) => {
    owner.renderer = exports;
    owner.retired ||= rendererLoaded !== exports;
    return exports;
  }) ?? null;
  const loading = within(
    Promise.all([speculationLoaded ?? import(url), trackedRenderer]),
    "the speculation module",
  ).then(
    ([module, renderer]) => {
      if (!currentSpeculation(loading, owner, renderer)) {
        if (speculationModule === loading) {
          speculationModule = null;
          speculationOwner = null;
        }
        throw new RetiredRenderer("the renderer was retired while speculation loaded");
      }
      if (renderer) wasm = renderer;
      speculationLoaded = module;
      return module;
    },
    (error) => {
      // Retirement belongs to this load even if its import rejects later.
      // A newer renderer/load must not inherit an older owner's failure.
      if (owner.retired) error = new RetiredRenderer("the renderer was retired while speculation loaded");
      if (speculationModule !== loading) throw error;
      speculationModule = null;
      speculationOwner = null;
      speculationAttempts += 1;
      // A renderer still loading is forgotten with it: the next press asks
      // for it again rather than wait on the load this one gave up.
      forgetRenderer(rendering);
      throw error;
    },
  );
  speculationModule = loading;
  speculationOwner = owner;
  return loading;
}

/** Publication needs both the coordinator and its native owner still current. */
function currentSpeculation(loading, owner, renderer) {
  return speculationModule === loading && !owner.retired && (!renderer || rendererLoaded === renderer);
}

/** How many loads of the page's speculation module have failed (ADR-0382). */
let speculationAttempts = 0;

function current(module, binding) {
  const state = speculated.get(binding);
  if (state && state.value === undefined && state.raw !== undefined) {
    state.value = module.decode[binding](state.raw);
    state.raw = undefined;
  }
  return state;
}

/** What a template writes has a text form (ADR-0074). */
function textOf(v) {
  return typeof v === "string" ? v : String(v);
}

/** **Every name the page shows a binding's entry by** (ADR-0312): its own,
 * and each alias, its layout's, rendered from the one value. */
function namesOf(module, binding) {
  const aliases = Object.entries(module.aliases ?? {})
    .filter(([, owner]) => owner === binding)
    .map(([alias]) => alias);
  return [binding, ...aliases];
}

function render(module, binding) {
  const state = current(module, binding);
  if (!state || state.value === undefined) return;
  let value = state.value;
  for (const p of state.pending) value = p.transition(value, p.args);
  for (const name of namesOf(module, binding)) {
    for (const [part, read] of Object.entries(module.parts[name] ?? {})) {
      setRange(addressOf([], Number(part)), textOf(read(value)));
    }
    showRegions(module, name, value, state.value);
  }
}

let nextSpeculation = 0;

/** Before a command's request: show each of its transitions this page can,
 * each by the interaction its request carries (ADR-0172). */
async function speculate(component, args, interaction) {
  // **Sent unspeculated where its module cannot load** (ADR-0382): a
  // speculation only shows a press before its answer, and the page shows the
  // server's value when it comes. Only the load is caught: what fails after
  // it fails the press, as before.
  let module;
  try {
    module = await speculations();
  } catch (error) {
    if (error instanceof RetiredRenderer) throw error;
    log.push(`${component} sent unspeculated: ${error.message ?? error}`);
    return [];
  }
  const ids = [];
  try {
    for (const s of module?.commands?.[component] ?? []) {
      const state = current(module, s.binding);
      if (!state || state.value === undefined) continue;
      if (parts.regions && (!wasm || wasm !== rendererLoaded)) {
        throw new RetiredRenderer("the renderer was retired before speculation resumed");
      }
      const id = nextSpeculation++;
      state.pending.push({ id, transition: s.transition, args, until: undefined, interaction });
      ids.push([s.binding, id]);
      render(module, s.binding);
      log.push(`speculated ${component} on ${s.binding}`);
    }
  } catch (error) {
    // The request has not been sent, and these IDs never reach its caller.
    // Remove all of them even if the redraw cannot load or render either.
    await restoreSpeculation(ids);
    throw error;
  }
  return ids;
}

/** The command answered: reconciled when its version arrives, or restored. */
async function resolveSpeculation(ids, committed, basis) {
  if (ids.length === 0) return;
  const touched = new Set();
  for (const [binding, id] of ids) {
    const state = speculated.get(binding);
    const i = state?.pending.findIndex((p) => p.id === id) ?? -1;
    if (i < 0) continue;
    const version = (basis ?? []).find((b) => b.entry === state.entry)?.version;
    if (!committed) {
      state.pending.splice(i, 1);
      log.push(`restored ${binding}`);
    } else if (version === undefined || version <= state.version) {
      // The value the page holds already includes this commit.
      state.pending.splice(i, 1);
      log.push(`reconciled ${binding} at version ${state.version}`);
    } else {
      state.pending[i].until = version;
    }
    touched.add(binding);
  }
  // Ownership is settled before any fallible await or redraw. A frame that
  // arrives during the load remains authoritative when rendering resumes.
  const module = await speculations();
  for (const binding of touched) render(module, binding);
}

/** A redraw failure must not replace the press's original failure. */
async function restoreSpeculation(ids) {
  try {
    await resolveSpeculation(ids, false);
  } catch (error) {
    log.push(`speculation rollback failed: ${error.message ?? error}`);
  }
}

/** An `entry_value` frame: the authoritative value at a version. */
function entryValue(frame) {
  for (const [binding, state] of speculated) {
    if (state.entry !== frame.entry || frame.version <= state.version) continue;
    state.version = frame.version;
    state.raw = frame.value;
    state.value = undefined;
    const before = state.pending.length;
    // A speculation the value includes is dropped (ADR-0172): by the
    // interaction that committed it, which the value names whether or not
    // the command's answer has come; or by the version its answer said.
    const applied = new Set(frame.applied ?? []);
    state.pending = state.pending.filter(
      (p) => !applied.has(p.interaction) && (p.until === undefined || p.until > frame.version),
    );
    if (before !== state.pending.length) {
      log.push(`reconciled ${binding} at version ${frame.version}`);
    }
  }
}

/** After a batch: a patch writes the authoritative text, and a speculation
 * still pending is shown over it again. A region is shown as what the page
 * holds when none is (ADR-0172): its list was shown as held before the
 * batch, and the batch's patches made it what the page holds now. */
async function reapplySpeculations() {
  // The module as loaded, never a load under way: before it has loaded no
  // press has speculated, and a load that fails must not fail the batch
  // (ADR-0382).
  const module = speculationLoaded;
  if (!module) return;
  for (const [binding, state] of speculated) {
    const regions = namesOf(module, binding).some((name) => module.regions?.[name]?.length);
    if (state.pending.length > 0 || regions) render(module, binding);
  }
}

// --- ADR-0172: what a speculation renders again --------------------------
//
// A speculation reaches every part that reads its value. Text at the top of
// the page is set by the module's own functions (ADR-0122). An attribute, a
// block and a loop's rows are rendered by the browser's copy of the server's
// renderer, from the speculated value, so the two agree by being one.
//
// A row the server rendered keeps its nodes, and focus with them: its parts
// are set where they are, and one a speculation takes away is detached, not
// destroyed. A row a speculation adds is the browser's, at an address the
// browser's renderer derives, until the server's insert replaces it: the
// server's addresses are a keyed hash under the deployment's key, which a
// page does not hold.
//
// Before a batch of the server's patches applies, each list shows what the
// page held again, so each patch finds the document it was derived from;
// after, the speculation is shown again. A change's patches and its value
// come in one batch (the server queues them in one hold), so the list's rows
// are then the held value's, in its order.

/** The renderer, and the speculation module, once loaded. */
let wasm = null;
let speculationLoaded = null;

/** One call into the renderer: the request as JSON, the answer as text. */
function renderCall(name, request) {
  return callRenderer(wasm, name, request);
}

/** Retire the exact native instance that threw, keeping a newer owner. */
function callRenderer(exports, name, request, label = name) {
  const bytes = new TextEncoder().encode(withTemplates(request));
  let code, out;
  try {
    const ptr = exports.alloc(bytes.length);
    new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes);
    code = exports[name](ptr, bytes.length);
    out = new TextDecoder().decode(
      new Uint8Array(exports.memory.buffer, exports.out_ptr(), exports.out_len()),
    );
  } catch (error) {
    retireRenderer(exports);
    log.push(`renderer ${name} failed: ${error.message ?? error}`);
    throw error;
  }
  // A normal refusal is not a native execution failure.
  if (code !== 0) throw new Error(`${label} did not render: ${out}`);
  return out;
}

/** A value as the renderer reads it: JSON, each `Int` written exactly. A
 * `BigInt` has no JSON form of its own, and a number past 2^53 has no exact
 * one. */
function wire(v) {
  if (typeof v === "bigint") return v.toString();
  if (v === null || v === undefined) return "null";
  if (Array.isArray(v)) return `[${v.map(wire).join(",")}]`;
  if (typeof v === "object") {
    return `{${Object.entries(v)
      .map(([k, x]) => `${JSON.stringify(k)}:${wire(x)}`)
      .join(",")}}`;
  }
  return JSON.stringify(v);
}

/** The list at `collection`, `cart.lines`, in its binding's value. */
function listAt(value, collection) {
  let v = value;
  for (const field of collection.split(".").slice(1)) v = v?.[field];
  return Array.isArray(v) ? v : [];
}

/** A row's key, at its path from the row: `item_id`. */
function rowKey(item, path) {
  let v = item;
  for (const field of path.split(".").filter(Boolean)) v = v?.[field];
  return String(v);
}

/** A row as the renderer reads it: its item, and what the row reads of it
 * through a member, set whole by its path (ADR-0170). */
function rowOf(region, item) {
  const row = { ...item };
  for (const [path, read] of Object.entries(region.rows ?? {})) row[path] = read(item);
  return row;
}

/** What a region is rendered with: the value, what the module computes from
 * it, the page's parameters, and its signals. */
function regionValues(module, binding, value) {
  // And each value the region computes from it (ADR-0235): a block's
  // subject, and a value inside one, by the path the template reads.
  const computed = Object.entries(module.computed?.[binding] ?? {}).map(([path, f]) => [
    path,
    f(value),
  ]);
  // And the page's parameters (ADR-0236), which the document carries.
  const params = Object.entries(parts.params ?? {});
  // And what the page asked of its reader (ADR-0307), as the host answered
  // it for the document: `SignedIn~holds`, a boolean.
  const holds = Object.entries(parts.holds ?? {});
  return `{${[[binding, value], ...computed, ...params, ...holds, ...signals]
    .map(([k, v]) => `${JSON.stringify(k)}:${wire(v)}`)
    .join(",")}}`;
}

/** The addresses of a loop's instances, in the document's order. */
function instanceTokens(part) {
  const loop = index.get(addressOf([], part));
  const open = new RegExp(`^pw:s${part}@([A-Za-z0-9_-]+)$`);
  const out = [];
  for (let n = loop?.start?.nextSibling; n && n !== loop.end; n = n.nextSibling) {
    const m = n.nodeType === Node.COMMENT_NODE && open.exec(n.data);
    if (m) out.push(m[1]);
  }
  return out;
}

/**
 * loop part → its rows as the document shows them: each one's key, its
 * address, the item it shows, whether the server rendered it, and its nodes
 * while a speculation takes it away. Read from the document the first time a
 * speculation shows the list, and again after each batch.
 */
const shownLists = new Map();

function listRows(region, held) {
  let state = shownLists.get(region.part);
  if (!state) {
    const tokens = instanceTokens(region.part);
    state = {
      rows: listAt(held, region.collection).map((item, i) => ({
        key: rowKey(item, region.key),
        token: tokens[i],
        shown: item,
        own: true,
        detached: null,
      })),
    };
    shownLists.set(region.part, state);
  }
  return state;
}

/** A row's nodes, its anchors included. */
function rowNodes(part, token) {
  const range = instanceRange(part, token);
  return range ? instanceNodes(range) : [];
}

/**
 * **Where focus goes when a row that has it goes** (ADR-0172): to the same
 * control in the row after it, else in the row before it, else to what
 * labels the region the list is in (WCAG 2.4.3). Until 2026-10-03 it went to
 * the document's body, and a keyboard user had to find the cart again.
 */
function passFocus(nodes, part) {
  const active = document.activeElement;
  if (!active || !nodes.some((n) => n === active || n.contains?.(active))) return;
  const owner = active.closest("[data-pw]")?.dataset.pw;
  const open = new RegExp(`^pw:s${part}@`);
  const isOpen = (n) => n?.nodeType === Node.COMMENT_NODE && open.test(n.data);
  const control = (start) => {
    const close = `pw:e${start.data.slice(`pw:s`.length)}`;
    for (let n = start.nextSibling; n && !(n.nodeType === Node.COMMENT_NODE && n.data === close); n = n.nextSibling) {
      if (n.nodeType !== Node.ELEMENT_NODE) continue;
      const found = n.dataset?.pw === owner ? n : n.querySelector?.(`[data-pw="${owner}"]`);
      if (found) return found;
    }
    return null;
  };
  let after = nodes[nodes.length - 1]?.nextSibling;
  while (after && !isOpen(after)) after = after.nextSibling;
  let before = nodes[0]?.previousSibling;
  while (before && !isOpen(before)) before = before.previousSibling;
  const labelled = nodes[0]?.parentElement?.closest("[aria-labelledby]");
  const label = labelled && document.getElementById(labelled.getAttribute("aria-labelledby"));
  const target = (after && control(after)) || (before && control(before)) || label;
  target?.focus();
}

/** What changed in a row the document keeps, set where it is; the row
 * rendered again, at its own address, where more than its text and its
 * attributes changed. */
function setRow(region, row, item, values) {
  const template = JSON.stringify(parts.regions[region.part]);
  const changes = JSON.parse(
    renderCall(
      "instance_changes",
      `{"part":${template},"was":${wire(rowOf(region, row.shown))},"now":${wire(rowOf(region, item))},"values":${values}}`,
    ),
  );
  if (changes === null) {
    const html = renderCall(
      "render_instance",
      `{"part":${template},"item":${wire(rowOf(region, item))},"values":${values}}`,
    );
    const fresh = /^<!--pw:s\d+@([A-Za-z0-9_-]+)-->/.exec(html)?.[1];
    const nodes = rowNodes(region.part, row.token);
    const replaced = parseInstance(fresh ? html.replaceAll(`@${fresh}-->`, `@${row.token}-->`) : html);
    nodes[0].before(...replaced);
    for (const n of nodes) n.remove();
    buildIndex();
    return;
  }
  for (const [part, change] of changes) {
    const at = { template: parts.schema, instances: [{ scope: region.part, instance: row.token }], part };
    if ("text" in change) {
      setRange(addressOf([[region.part, row.token]], part), change.text);
    } else {
      setAttributeAt(
        at,
        change.value === null
          ? { op: "remove_attribute", name: change.attribute }
          : { op: "set_attribute", name: change.attribute, value: change.value },
      );
    }
  }
}

/** **A loop's rows, as `items` makes them** (ADR-0172): each row the
 * document keeps set where it is, each it does not taken away, and each new
 * one rendered by the browser, in the items' order. Focus in a row taken
 * away passes on, unless the page is only showing what it holds before a
 * batch, after which focus goes back to its row (`refocus`). */
function showList(region, items, values, held, passing = true) {
  const loop = index.get(addressOf([], region.part));
  if (!loop?.start) return;
  const state = listRows(region, held);
  const wanted = items.map((item) => ({ key: rowKey(item, region.key), item }));
  const keys = new Set(wanted.map((w) => w.key));
  for (const row of state.rows) {
    if (keys.has(row.key) || row.detached) continue;
    const nodes = rowNodes(region.part, row.token);
    if (passing) passFocus(nodes, region.part);
    for (const n of nodes) n.remove();
    if (row.own) row.detached = nodes;
    else row.gone = true;
  }
  state.rows = state.rows.filter((row) => !row.gone);
  buildIndex();
  let after = loop.start;
  const shown = [];
  for (const w of wanted) {
    let row = state.rows.find((r) => r.key === w.key);
    if (!row) {
      const html = renderCall(
        "render_instance",
        `{"part":${JSON.stringify(parts.regions[region.part])},"item":${wire(rowOf(region, w.item))},"values":${values}}`,
      );
      const nodes = parseInstance(html);
      after.after(...nodes);
      const token = /^pw:s\d+@([A-Za-z0-9_-]+)$/.exec(nodes[0]?.data ?? "")?.[1];
      row = { key: w.key, token, shown: w.item, own: false, detached: null };
      state.rows.push(row);
      buildIndex();
    } else {
      if (row.detached) {
        after.after(...row.detached);
        row.detached = null;
        buildIndex();
      }
      const nodes = rowNodes(region.part, row.token);
      if (nodes[0] && nodes[0] !== after.nextSibling) {
        moveNodes(after.parentNode, nodes, after.nextSibling);
        buildIndex();
      }
      if (wire(rowOf(region, row.shown)) !== wire(rowOf(region, w.item))) {
        setRow(region, row, w.item, values);
      }
      row.shown = w.item;
    }
    shown.push(row);
    const nodes = rowNodes(region.part, row.token);
    after = nodes[nodes.length - 1] ?? after;
  }
  state.rows = [...shown, ...state.rows.filter((r) => r.detached)];
  bindEvents();
}

/** The last rendering of each block a speculation renders, so one rendered
 * as it is is not replaced, and its nodes kept. */
const shownBlocks = new Map();

/** **Every region that reads `binding`, at `value`** (ADR-0172). */
function showRegions(module, binding, value, held) {
  const regions = module.regions?.[binding] ?? [];
  if (regions.length === 0 || !wasm) return;
  const values = regionValues(module, binding, value);
  for (const region of regions) {
    const template = JSON.stringify(parts.regions?.[region.part]);
    if (region.kind === "list") {
      showList(region, listAt(value, region.collection), values, held);
    } else if (region.kind === "block") {
      const html = renderCall("render_part", `{"part":${template},"values":${values}}`);
      if (shownBlocks.get(region.part) !== html) {
        shownBlocks.set(region.part, html);
        replaceBlock(region.part, html);
        buildIndex();
        bindEvents();
      }
    } else if (region.kind === "captures") {
      // What a handler at the top of the page captures (ADR-0217): its
      // element's captures, as the speculated value makes them, so a press
      // meanwhile sends what the page shows.
      const written = JSON.parse(
        renderCall("captures_value", `{"parts":${template},"values":${values}}`),
      );
      setAttributeAt(
        { template: parts.schema, instances: [], part: region.part },
        written === null
          ? { op: "remove_attribute", name: "data-pw-captures" }
          : { op: "set_attribute", name: "data-pw-captures", value: written },
      );
    } else if (region.kind === "attribute") {
      const [name, written] = JSON.parse(
        renderCall("attribute_value", `{"part":${template},"values":${values}}`),
      );
      setAttributeAt(
        { template: parts.schema, instances: [], part: region.part },
        written === null
          ? { op: "remove_attribute", name }
          : { op: "set_attribute", name, value: written },
      );
    }
  }
}

/** The row of a speculated list that has focus, and the control in it:
 * what focus goes back to once a batch has been applied (ADR-0172). */
function focusedRow() {
  const active = document.activeElement;
  if (!active || active === document.body) return null;
  for (const [part, state] of shownLists) {
    for (const row of state.rows) {
      if (row.detached) continue;
      if (rowNodes(part, row.token).some((n) => n === active || n.contains?.(active))) {
        return { part, key: row.key, owner: active.closest("[data-pw]")?.dataset.pw };
      }
    }
  }
  return null;
}

/** Focus back on its row's control, where the batch took it away; on what
 * labels the list's region where the row is gone. */
function refocus(focused) {
  if (!focused) return;
  const active = document.activeElement;
  if (active && active !== document.body && active.isConnected) return;
  const row = shownLists.get(focused.part)?.rows.find((r) => r.key === focused.key && !r.detached);
  if (!row) {
    const loop = index.get(addressOf([], focused.part));
    const labelled = loop?.start?.parentElement?.closest("[aria-labelledby]");
    document.getElementById(labelled?.getAttribute("aria-labelledby") ?? "")?.focus();
    return;
  }
  for (const n of rowNodes(focused.part, row.token)) {
    if (n.nodeType !== Node.ELEMENT_NODE) continue;
    const control =
      n.dataset?.pw === focused.owner ? n : n.querySelector?.(`[data-pw="${focused.owner}"]`);
    if (control) {
      control.focus();
      return;
    }
  }
}

/** **Each list shown as the page holds it, before a batch's patches apply**
 * (ADR-0172), and read from the document again after them. */
function revertLists() {
  if (!speculationLoaded || !wasm) return;
  for (const [binding] of speculated) {
    const state = current(speculationLoaded, binding);
    if (!state || state.value === undefined) continue;
    for (const name of namesOf(speculationLoaded, binding)) {
      const values = regionValues(speculationLoaded, name, state.value);
      for (const region of speculationLoaded.regions?.[name] ?? []) {
        if (region.kind === "list" && shownLists.has(region.part)) {
          showList(region, listAt(state.value, region.collection), values, state.value, false);
        }
      }
    }
  }
  shownLists.clear();
}

// --- ADR-0130: signals ---------------------------------------------------
//
// A page's UI state. The document carries each signal's first value, each
// part a signal decides, and the template of each block one decides. A
// handler changes a signal through its context; this holds the value and
// renders again exactly the parts that read it: a text part by setting its
// range, a block by rendering its template with the browser's build of the
// server's renderer (`pw-render-wasm`), so the two agree by being one
// implementation. Nothing here asks the server for anything.

/** name → its value, as JSON carries it (`js_pure`'s wire form). */
const signals = new Map(Object.entries(parts.signals ?? {}));

/** Each signal's first value, as the document gave it: what an instance
 * starts again at when its block shows another arm (ADR-0144). */
const firstValues = new Map(Object.entries(parts.signals ?? {}));

/** What `{#if}` reads as true, as the renderer reads it (`Value::truthy`). */
function truthy(v) {
  if (typeof v === "boolean") return v;
  if (typeof v === "number") return v !== 0;
  if (typeof v === "string") return v !== "";
  if (Array.isArray(v)) return v.length > 0;
  return v !== null && v !== undefined;
}

/** The arm a block a signal decides shows: whether an `{#if}`'s value
 * holds, or a `{#match}`'s case. */
function armOf(live) {
  const v = signalAt(live.path);
  if (live.kind === "conditional") return truthy(v);
  return v !== null && typeof v === "object" && "$case" in v ? v.$case : JSON.stringify(v);
}

/** The arm each block that holds signals last showed (ADR-0144). */
const arms = new Map();
for (const live of parts.live ?? []) {
  if (live.owns?.length) arms.set(live.part, armOf(live));
}
let renderer = null;
/** The renderer's exports, once loaded. */
let rendererLoaded = null;

/** The renderer, loaded with the first block a signal renders again. */
function renderModule() {
  if (rendererLoaded) return Promise.resolve(rendererLoaded);
  if (renderer) return renderer;
  // Not kept failed (ADR-0382): the next need asks the network again.
  const loading = fetch("/pw-render.wasm")
    .then((r) => WebAssembly.instantiateStreaming(r, {}))
    .then(
      ({ instance }) => {
        if (renderer === loading) rendererLoaded = instance.exports;
        return instance.exports;
      },
      (error) => {
        if (renderer === loading) renderer = null;
        log.push(`renderer not loaded: ${error.message ?? error}`);
        throw error;
      },
    );
  renderer = loading;
  return loading;
}

/** **A renderer load a press gave up on** (ADR-0382): forgotten unless it
 * has loaded, so the next need asks the network again. */
function forgetRenderer(loading = renderer) {
  if (!rendererLoaded && renderer === loading) renderer = null;
}

/** Native execution failed: only caches owned by these exports are retired. */
function retireRenderer(failed) {
  if (!failed) return;
  markSpeculationRetired(failed);
  if (rendererLoaded === failed) {
    rendererLoaded = null;
    renderer = null;
  }
  retireSpeculationRenderer(failed);
}

/** Retain this coordinator's failure before either cache is released. */
function markSpeculationRetired(failed) {
  if (speculationOwner?.renderer === failed) speculationOwner.retired = true;
}

/** The native owner may be shared; the speculation cache has its own owner. */
function retireSpeculationRenderer(failed) {
  if (wasm !== failed) return;
  wasm = null;
  speculationModule = null;
  speculationOwner = null;
}

/** A path read from the signals' values: `greeting`, or `panel.item`. */
function signalAt(path) {
  const [root, ...fields] = path.split(".");
  let v = signals.get(root);
  for (const f of fields) v = v?.[f];
  return v;
}

/** **What the page computes from its signals now** (ADR-0229): each value,
 * by the path the compiler names it, for a block the browser renders again,
 * whose subject may be one. None where the page computes none. */
async function computedNow() {
  const computed = (parts.live ?? []).filter((l) => l.derived);
  if (computed.length === 0) return [];
  const module = await computedValues();
  return computed.map((l) => [
    `#${parts.template}~${l.part}`,
    module.parts[l.part](...argsOf(l)),
  ]);
}

/** **What a computed part is given**: the signal's value at its path, or
 * each of several it reads, in order (track `device-store`, decision 7). */
function argsOf(live) {
  return (live.paths?.length ? live.paths : [live.path]).map(signalAt);
}

/** The signals a computed part is computed again for. */
function readsOf(live) {
  return live.reads?.length ? live.reads : [live.signal];
}

/** The block numbered `id`, rendered at the signals' values now, and what
 * the page computes from them. */
async function renderBlock(id) {
  const x = await renderModule();
  const values = [...signals, ...(await computedNow())]
    .map(([k, v]) => `${JSON.stringify(k)}:${wire(v)}`)
    .join(",");
  return callRenderer(
    x, "render_part", `{"part":${JSON.stringify(parts.blocks[id])},"values":{${values}}}`, `block ${id}`,
  );
}

/** The nodes from `start` to `end`, both included. */
function between(start, end) {
  const out = [];
  for (let n = start; n; n = n.nextSibling) {
    out.push(n);
    if (n === end) break;
  }
  return out;
}

/** Each `<dialog>` among `nodes` and inside them, open or shut as asked. */
function dialogsIn(nodes, open) {
  const out = [];
  for (const n of nodes) {
    if (n.nodeType !== Node.ELEMENT_NODE) continue;
    if (n.localName === "dialog" && n.open === open) out.push(n);
    for (const d of n.querySelectorAll("dialog")) if (d.open === open) out.push(d);
  }
  return out;
}

/**
 * **A `<dialog>` a signal's block renders is the browser's modal dialog**
 * (ADR-0141): shown with `showModal`, which focuses its `autofocus` element
 * and makes the page behind it inert. Escape closes it by itself, and the
 * dialog's `close` handler tells the signal. A `<dialog open>` is HTML's
 * dialog shown in place, and is left alone.
 */
function showDialogs(nodes) {
  for (const d of dialogsIn(nodes, false)) {
    if (!d.isConnected || d.hasAttribute("open")) continue;
    // What opened it: what has focus, or else the element whose handler ran
    // last. WebKit does not focus a button a pointer presses, and focus goes
    // back to what invoked a dialog (WAI-ARIA's dialog pattern).
    const focused = document.activeElement;
    openedFrom.set(d, focused && focused !== document.body ? focused : lastActed);
    d.addEventListener("close", () => giveFocusBack(d), { once: true, signal: life.signal });
    d.showModal();
  }
}

/** What had focus when each dialog was shown, or what invoked it. */
const openedFrom = new WeakMap();

/** The element whose handler ran last. */
let lastActed = null;

/**
 * **Focus goes back to what had it** when a modal dialog closes, as the HTML
 * standard says `close` does (ADR-0141). Some engines do not yet: WebKit
 * leaves focus in the closed dialog, or on the body. Done only where focus
 * was lost, so an engine that did it, or a handler that moved focus on
 * purpose, is left alone.
 */
function giveFocusBack(d) {
  const from = openedFrom.get(d);
  openedFrom.delete(d);
  const now = document.activeElement;
  const lost = !now || now === document.body || d.contains(now) || !now.isConnected;
  if (lost && from?.isConnected) from.focus();
}

// --- streamed regions (ADR-0148) ------------------------------------------
//
// A region whose query the page does not wait for is sent pending: its
// placeholder inside `<?start name="pw-N">` .. `<?end>`, and later in the
// same response its settled arm, as `<template for="pw-N">`. A browser with
// the platform's out-of-order streaming (Chrome 150+) applies it as it
// parses. One without reads the markers as comments and keeps the template,
// inert, at the end of the body; this applies it there. Either way, once a
// region has settled its parts are read again and their handlers bound.

const STREAM_NAME = /^pw-(\d+)$/;

/** The stream parts still showing their placeholder, as of boot and since. */
const pendingStreams = new Set();

/** **The stream parts pending when the page was first indexed** (ADR-0272):
 * the runtime boots across the network, and a browser that streams a region
 * itself can fill one meanwhile. Until ADR-0272 such a region was indexed
 * pending, found settled after boot, and never read again: its buttons were
 * bound to nothing, and a press on one did nothing. */
const pendingAtIndex = new Set();

/** Is the region of stream `id` still pending: does it hold its start
 * marker, a processing instruction or, where the parser has none, the
 * comment it reads one as? */
function streamPending(id) {
  const r = index.get(addressOf([], id));
  if (!r?.start) return false;
  for (let n = r.start.nextSibling; n && n !== r.end; n = n.nextSibling) {
    if (n.nodeType === Node.PROCESSING_INSTRUCTION_NODE && n.target === "start") return true;
    if (n.nodeType === Node.COMMENT_NODE && n.data.startsWith("?start ")) return true;
  }
  return false;
}

/** Apply each `<template for>` the browser left in the document, once it has
 * all arrived: the region's contents, between its anchors, become the
 * template's. Returns the parts it filled.
 *
 * A template is whole once the parser has put a node after it, the comment
 * the server writes after each one, or has read the whole document (ADR-0223).
 * Until 2026-10-05 one was applied as soon as it was seen, and a response that
 * arrived in parts gave the region the part of it parsed so far: the
 * recommendations showed one item of two, and the second was lost with the
 * template. The platform's own streaming applies one at its end tag. */
function applyStreamPatches() {
  const applied = new Set();
  for (const t of document.querySelectorAll("template[for]")) {
    if (!t.nextSibling && document.readyState === "loading") continue;
    const m = STREAM_NAME.exec(t.getAttribute("for") ?? "");
    const r = m && index.get(addressOf([], m[1]));
    if (!r?.start) continue;
    for (let n = r.start.nextSibling; n && n !== r.end; ) {
      const next = n.nextSibling;
      n.remove();
      n = next;
    }
    r.end.parentNode.insertBefore(document.importNode(t.content, true), r.end);
    t.remove();
    applied.add(m[1]);
  }
  return applied;
}

/** Each region that settled since this last looked: read again, its
 * handlers bound, and reported. */
function settleStreams() {
  const filled = applyStreamPatches();
  if (filled.size > 0) buildIndex();
  const settled = [...pendingStreams].filter((id) => !streamPending(id));
  if (settled.length === 0) return;
  for (const id of settled) {
    pendingStreams.delete(id);
    window.__pw.settled.push({ part: Number(id), by: filled.has(id) ? "runtime" : "browser" });
  }
  buildIndex();
  bindEvents();
}

/** Watch the document for regions settling, while the response that carries
 * them is still arriving. */
function watchStreams() {
  for (const p of parts.parts ?? []) {
    if (p.kind === "stream" && streamPending(String(p.id))) pendingStreams.add(String(p.id));
  }
  // A region that settled while the runtime booted, which only the browser
  // fills: read again and bound, as one settling later is.
  const whileBooting = [...pendingAtIndex].filter((id) => !pendingStreams.has(id));
  if (whileBooting.length > 0) {
    for (const id of whileBooting) window.__pw.settled.push({ part: Number(id), by: "browser" });
    buildIndex();
    bindEvents();
  }
  if (pendingStreams.size === 0) return;
  settleStreams();
  if (document.readyState === "complete" || pendingStreams.size === 0) return;
  const observer = new MutationObserver(settleStreams);
  observer.observe(document.body, { childList: true, subtree: true });
  // The response has ended: nothing more arrives for these regions.
  window.addEventListener(
    "load",
    () => {
      settleStreams();
      observer.disconnect();
    },
    { once: true, signal: life.signal },
  );
}

/** Replace a block's range, its anchors included, with what was rendered,
 * and say what was put in its place. */
function replaceBlock(id, html) {
  return replaceRangeAt(addressOf([], id), html) ?? [];
}

// --- a block rendered again keeps its nodes (track `device-store`, ADR-0366) --
//
// A block the browser renders again from its signals (ADR-0130) was replaced
// whole until 2026-10-20: every node in it new each time a signal it reads
// changed. A row of a list in it was another element after each press, so
// focus, a hover, what assistive technology held and a test's handle on the
// row were lost: TodoMVC's own tests hold a todo's `<li>` across its toggle,
// and once its list was the browser's to render (a value the device keeps),
// four of them failed for it. Now what was rendered is morphed into what is
// rendered: a node the new rendering has where the old one had one of the same
// kind is kept, its attributes and text set to the new ones; a row of a keyed
// list is matched by its key's anchor (`pw:s{part}@{token}`, the token the
// renderer derives from the key) and kept, moved or removed as its key says;
// only what is new is inserted, and only what is gone removed. A control's
// state, `checked` and `value`, follows the new rendering where it changed,
// as a part set in place does (ADR-0142).

/** The key anchor `node` opens or closes, `{ part, token, start }`, or none. */
function rowAnchor(node) {
  if (node.nodeType !== Node.COMMENT_NODE) return null;
  const m = /^pw:([se])(\d+)@(.+)$/.exec(node.data);
  return m ? { start: m[1] === "s", part: m[2], token: m[3] } : null;
}

/** `nodes` as the items a morph matches: each a node, or a row of a keyed
 * list, its anchors and what is between them. */
function rowsOf(nodes) {
  const items = [];
  for (let i = 0; i < nodes.length; i++) {
    const a = rowAnchor(nodes[i]);
    if (a?.start) {
      const end = nodes.findIndex(
        (n, j) => j > i && n.nodeType === Node.COMMENT_NODE && n.data === `pw:e${a.part}@${a.token}`,
      );
      if (end > i) {
        items.push({ key: `${a.part}@${a.token}`, nodes: nodes.slice(i, end + 1) });
        i = end;
        continue;
      }
    }
    items.push({ node: nodes[i] });
  }
  return items;
}

/** Whether `old` can be kept as `fresh`: a node of its kind, an element of
 * its tag and its identity, a comment of its text. */
function sameKind(old, fresh) {
  if (old.nodeType !== fresh.nodeType) return false;
  if (old.nodeType === Node.ELEMENT_NODE) {
    return (
      old.tagName === fresh.tagName &&
      old.getAttribute("data-pw") === fresh.getAttribute("data-pw") &&
      old.getAttribute("id") === fresh.getAttribute("id")
    );
  }
  if (old.nodeType === Node.COMMENT_NODE) return old.data === fresh.data;
  return true;
}

/** `old` made what `fresh` is, kept. */
function morphNode(old, fresh, inserted) {
  if (old.nodeType === Node.TEXT_NODE) {
    if (old.data !== fresh.data) old.data = fresh.data;
    return;
  }
  if (old.nodeType !== Node.ELEMENT_NODE) return;
  // A control's state follows the new rendering where it changed it.
  const wasChecked = old.hasAttribute("checked");
  const wasValue = old.getAttribute("value");
  for (const a of [...old.attributes]) {
    if (!fresh.hasAttribute(a.name)) old.removeAttribute(a.name);
  }
  for (const a of [...fresh.attributes]) {
    if (old.getAttribute(a.name) !== a.value) old.setAttribute(a.name, a.value);
  }
  if (typeof old.checked === "boolean" && (wasChecked !== fresh.hasAttribute("checked") || old.checked !== fresh.hasAttribute("checked"))) {
    old.checked = fresh.hasAttribute("checked");
  }
  if ("value" in old && old.tagName !== "TEXTAREA" && wasValue !== fresh.getAttribute("value")) {
    old.value = fresh.getAttribute("value") ?? "";
  }
  morphChildren(old, [...old.childNodes], [...fresh.childNodes], null, inserted);
}

/** **`olds`, children of `parent`, made `freshes`**, before `before`: each
 * kept where it can be, each row by its key, and in the new order. */
function morphChildren(parent, olds, freshes, before, inserted) {
  const old = rowsOf(olds);
  const fresh = rowsOf(freshes);
  const byKey = new Map(old.filter((o) => o.key).map((o) => [o.key, o]));
  const loose = old.filter((o) => o.node);
  const used = new Set();
  let cursor = 0;
  const placed = [];
  for (const f of fresh) {
    if (f.key) {
      const o = byKey.get(f.key);
      if (o && !used.has(o)) {
        used.add(o);
        const [start, ...rest] = o.nodes;
        const end = rest.pop();
        const freshInner = f.nodes.slice(1, -1);
        morphChildren(parent, rest, freshInner, end, inserted);
        placed.push(...between(start, end));
      } else {
        placed.push(...f.nodes);
        inserted.push(...f.nodes);
      }
      continue;
    }
    // A loose node: the next loose old one of its kind, in order.
    let match = null;
    while (cursor < loose.length) {
      const o = loose[cursor++];
      if (used.has(o)) continue;
      if (sameKind(o.node, f.node)) {
        match = o;
        break;
      }
    }
    if (match) {
      used.add(match);
      morphNode(match.node, f.node, inserted);
      placed.push(match.node);
    } else {
      placed.push(f.node);
      inserted.push(f.node);
    }
  }
  // What the new rendering has not, gone: a modal dialog among it closed
  // first, its focus given back (ADR-0141).
  for (const o of old) {
    if (used.has(o)) continue;
    for (const n of o.nodes ?? [o.node]) {
      if (n.nodeType === Node.ELEMENT_NODE) {
        for (const d of dialogsIn([n], true)) {
          d.close();
          giveFocusBack(d);
        }
      }
      n.remove();
    }
  }
  // In the new order, a kept node moved only where it is not already.
  let anchor = before;
  for (let i = placed.length - 1; i >= 0; i--) {
    const n = placed[i];
    if (n.parentNode !== parent || n.nextSibling !== anchor) {
      if (n.isConnected && n.parentNode === parent) moveNodes(parent, [n], anchor);
      else parent.insertBefore(n, anchor);
    }
    anchor = n;
  }
}

/** **A block the browser renders again, morphed**: what it rendered kept
 * where the new rendering has it, and the nodes it inserted. */
function morphBlock(id, html) {
  const r = index.get(addressOf([], id));
  if (!r?.start) return [];
  const parent = r.start.parentNode;
  const inserted = [];
  morphChildren(parent, between(r.start, r.end), parseInstance(html), r.end.nextSibling, inserted);
  return inserted;
}

/** The range at `key` replaced by `html`, or `null` where the document has
 * no such range (ADR-0146). */
function replaceRangeAt(key, html) {
  const r = index.get(key);
  if (!r?.start) return null;
  const parent = r.start.parentNode;
  const after = r.end.nextSibling;
  // A modal dialog the block showed is closed before it goes, so the browser
  // gives focus back to what had it before the dialog took it (ADR-0141).
  for (const d of dialogsIn(between(r.start, r.end), true)) {
    d.close();
    giveFocusBack(d);
  }
  for (let n = r.start; n; ) {
    const next = n.nextSibling;
    const last = n === r.end;
    n.remove();
    if (last) break;
    n = next;
  }
  const inserted = parseInstance(html);
  for (const node of inserted) parent.insertBefore(node, after);
  return inserted;
}

/**
 * **An attribute a signal decides, set in place** (ADR-0142), on the element
 * the part belongs to. A field's `value` is its property, and is not set from
 * the field's own typing: what it holds is what was typed, and a value its
 * handler set behind later keys would move the caret under the person typing
 * (ADR-0131's concern). Any other change, a handler clearing it, is shown. A
 * boolean attribute is its presence, and its property where the element has
 * one (`checked`, `disabled`).
 */
function setAttributePart(live, source, v = signalAt(live.path)) {
  const part = (parts.parts ?? []).find((p) => p.id === live.part);
  if (!part) return;
  for (const a of addressesFor(`e${part.owner}`)) {
    const el = index.get(a)?.element;
    if (!el) continue;
    if (live.kind === "boolean_attribute") {
      const on = v === true;
      el.toggleAttribute(live.attribute, on);
      if (typeof el[live.attribute] === "boolean") el[live.attribute] = on;
    } else if (live.attribute === "value" && "value" in el) {
      const typed = source?.el === el && source.event === "input";
      const text = textOf(v);
      if (!typed && el.value !== text) el.value = text;
    } else {
      el.setAttribute(live.attribute, textOf(v));
    }
  }
}

let dirty = new Set();
let flushing = null;

/** The parts the runtime sets in place: a text part's range and an
 * attribute (ADR-0142). A block is rendered again whole. */
const SET_IN_PLACE = new Set(["text", "attribute", "boolean_attribute"]);

let computedModule = null;

/** **What the page computes from its signals** (ADR-0227): the module the
 * compiler wrote, its functions the same ones the host computed the page's
 * first values with. Loaded when one of those signals first changes. */
function computedValues() {
  if (computedModule) return computedModule;
  // Not kept failed (ADR-0382): the next change asks again, under another
  // specifier, as a speculation's load does. No deadline: a press does not
  // wait for it, and a page's boot does, where a value the device keeps is
  // migrated by it; a slow load there is better waited for than failed.
  const url = computedAttempts === 0 ? parts.computed : `${parts.computed}?attempt=${computedAttempts}`;
  computedModule = import(url).then(
    (module) => {
      log.push(`loaded ${parts.computed}`);
      return module;
    },
    (error) => {
      computedModule = null;
      computedAttempts += 1;
      log.push(`computed values not loaded: ${error.message ?? error}`);
      throw error;
    },
  );
  return computedModule;
}

/** How many loads of the page's computed values have failed (ADR-0382). */
let computedAttempts = 0;

/** Where each signal's latest change came from: the element and the event
 * whose handler set it (ADR-0142). */
let sources = new Map();

/** A handler changed `name`: held now, and rendered once per press. A value
 * the signal already holds changes nothing, and renders nothing. */
function setSignal(name, value, source, kept = true) {
  if (JSON.stringify(signals.get(name)) === JSON.stringify(value)) return;
  signals.set(name, value);
  if (source) sources.set(name, source);
  else sources.delete(name);
  dirty.add(name);
  // A value the device keeps is written as it changes (track
  // `device-store`), unless the change is the device's own: read at start,
  // or another tab's.
  if (kept && deviceHeld.has(name)) keep(deviceHeld.get(name), value, source);
  flushing ??= Promise.resolve().then(flushSignals);
}

async function flushSignals() {
  const changed = dirty;
  const from = sources;
  dirty = new Set();
  sources = new Map();
  flushing = null;
  let rendered = false;
  const inserted = [];
  for (const live of parts.live ?? []) {
    // A value computed for a block's render alone (track `device-store`):
    // its block is rendered again, below, and computes it then.
    if (live.kind === "value") continue;
    if (live.derived && SET_IN_PLACE.has(live.kind)) {
      // A value computed from the signal (ADR-0227), by its part's function;
      // or from several (track `device-store`).
      if (!readsOf(live).some((s) => changed.has(s))) continue;
      const v = (await computedValues()).parts[live.part](...argsOf(live));
      if (live.kind === "text") setRange(addressOf([], live.part), textOf(v));
      else setAttributePart(live, from.get(live.signal), v);
      log.push(`signal ${live.signal} -> computed part ${live.part}`);
    } else if (live.kind === "text") {
      if (!changed.has(live.signal)) continue;
      setRange(addressOf([], live.part), textOf(signalAt(live.path)));
      log.push(`signal ${live.signal} -> part ${live.part}`);
    } else if (live.kind === "attribute" || live.kind === "boolean_attribute") {
      if (!changed.has(live.signal)) continue;
      setAttributePart(live, from.get(live.signal));
      log.push(`signal ${live.signal} -> attribute ${live.attribute} of part ${live.part}`);
    } else if ((live.reads ?? [live.signal]).some((s) => changed.has(s))) {
      // **A signal ends with its instance** (ADR-0130, R6; ADR-0144): when a
      // block shows another arm, what its arms held starts again, before the
      // block renders. Everything that reads them is inside it.
      if (live.owns?.length) {
        const arm = armOf(live);
        if (arm !== arms.get(live.part)) {
          for (const s of live.owns) signals.set(s, firstValues.get(s));
          arms.set(live.part, arm);
        }
      }
      // Morphed, so what it held is kept where it is rendered again (track
      // `device-store`): a row by its key.
      inserted.push(...morphBlock(live.part, await renderBlock(live.part)));
      rendered = true;
      log.push(`signal ${[...changed].join(",")} -> block ${live.part}`);
    }
  }
  if (rendered) {
    buildIndex();
    bindEvents();
    // Shown once what is in it can be pressed (ADR-0141).
    showDialogs(inserted);
  }
  readAgain(changed);
  window.__pw.signals = Object.fromEntries(signals);
}

// --- the address's fragment, bound to a signal (track `todomvc`) ------------
//
// A page that states `fragment filter "/" => "all", "/active" => "active"`
// binds the signal to its address's fragment, which no server sees: the
// document says which signal and each form the fragment takes with the value
// the signal takes then (`parts.fragment`). The signal follows the fragment:
// at start, and on each `hashchange`, a link's or the back button's. A
// fragment that is no form, or none, is the signal's first value, the page's
// declared default, which the server rendered. The address is the signal's
// only writer: the compiler refuses a handler that assigns it (PW6105), so
// the two cannot differ.

/** The value the signal takes at the address's fragment now. */
function fragmentValue(binding) {
  const form = location.hash.slice(1);
  return binding.forms.find((f) => f.form === form)?.value ?? firstValues.get(binding.signal);
}

/** The signal set to what the fragment says, held and rendered as any
 * signal's change is: the parts that read it, and a query it keys. */
function followFragment(binding) {
  const value = fragmentValue(binding);
  if (signals.get(binding.signal) === value) return;
  log.push(`fragment ${location.hash || "(none)"} -> ${binding.signal} ${JSON.stringify(value)}`);
  setSignal(binding.signal, value, null);
}

/** The signal follows the fragment, from now: once it is read at start. */
function bindFragment(binding) {
  followFragment(binding);
  addEventListener("hashchange", () => followFragment(binding), { signal: life.signal });
}

// --- `autofocus` on an element a block shows (track `todomvc`) --------------
//
// HTML runs `autofocus` when a page loads, and ignores it on an element put
// in later. A page shows fields after it has loaded, as TodoMVC shows its
// edit field when a person asks to edit a todo ("the edit field should be
// focused"): the field they asked for is the one they type in. So an element
// that arrives with `autofocus`, in a block a signal renders or in a patch
// from the server, takes focus as it arrives, each time it arrives. What the
// page was served with is left alone: the browser focused that. (A modal
// dialog's `autofocus` element is the one `showModal` focuses (ADR-0141): it
// is already focused by the time this looks.)

/** The element, among `nodes` and what they hold, that asks for focus. */
function autofocusIn(nodes) {
  for (const node of nodes) {
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const asking = node.matches("[autofocus]") ? [node] : [...node.querySelectorAll("[autofocus]")];
    const found = asking.find((el) => el.isConnected);
    if (found) return found;
  }
  return null;
}

/** Whatever is put into the page from now on, one change at a time. */
function followAutofocus() {
  const observer = new MutationObserver((records) => {
    const el = autofocusIn(records.flatMap((r) => [...r.addedNodes]));
    if (el && document.activeElement !== el) el.focus();
  });
  observer.observe(document.body, { childList: true, subtree: true });
  life.signal.addEventListener("abort", () => observer.disconnect(), { once: true });
}

// --- values the device keeps (track `device-store`, ADR-0366) --------------
//
// A module's signal declared `storage device "name"` and `privacy Device` is
// the device's: the document says which ones the page holds, the name each
// is kept under, its version and each version's form (`parts.device`). The
// server rendered each one's declared default, and the elements it decides
// are hidden while the root says `data-pw-device="reading"`. So, before any
// handler binds:
// - each is read from the browser's own storage, `localStorage`, under its
//   key (the name at version 1, `name@v` after), and decoded against its
//   form, never trusted (charter §7.10); an earlier version is decoded with
//   its own form, taken to this one by the steps the page's computed module
//   holds, written under the new key, and only then is the old one removed;
// - what each value decides is rendered, as any signal's change is;
// - and the root's mark is taken away, so the first frame that shows what
//   the device decides shows the device's value.
// From then on each change is written with `setItem`, whole; every other
// tab hears it as a `storage` event and shows it, writing nothing back
// (`conflict last_write_wins`: the whole value written last is every tab's).
// A value no decoder takes is never used and never lost: the default is
// shown, and the text is copied to `key!unreadable` before the first write
// replaces it. A page that finds a newer version than its own writes nothing
// more, and says so. A write the store refuses (`QuotaExceededError`) keeps
// what was stored, and is said where the change was made. Nothing here
// sends anything to any server.

/** What the device keeps that this page holds, by the signal it is held as. */
const deviceHeld = new Map((parts.device ?? []).map((h) => [h.signal, h]));

/** Where each stands, for the tests to read: `default`, `read`, `migrated`,
 * `unreadable`, `stale` or `unavailable`. */
const deviceState = {};
let deviceWrites = 0;

/** The platform's words for what the device did not keep. */
const DEVICE_SAYS = {
  full: "This device has no room to keep that.",
  stale: "This page is out of date. Reload to keep your changes.",
};

/** The key version `v` of `held` is kept under. */
function deviceKey(held, v) {
  return v === 1 ? held.name : `${held.name}@${v}`;
}

/** The browser's store, or none where it is refused: its getter throws in a
 * document whose storage the browser keeps from it. */
function deviceStore() {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** **Why `v` is not of the form `s`**, or `null` where it is: the wire form
 * the browser's modules read (`js_pure`), checked whole. */
function unlike(v, s, at) {
  const record = (x) => x !== null && typeof x === "object" && !Array.isArray(x);
  const ofCases = (x, cases) => {
    if (!record(x) || typeof x.$case !== "string") return `${at} is no case`;
    const found = cases.find(([name]) => name === x.$case);
    if (!found) return `${at}'s case \`${x.$case}\` is none its type has`;
    const [, fields] = found;
    const extra = Object.keys(x).filter((k) => k !== "$case" && k !== "value");
    if (extra.length) return `${at} holds \`${extra[0]}\`, which no case does`;
    if (fields.length === 0) return "value" in x ? `${at}'s case holds nothing` : null;
    if (fields.length === 1) return unlike(x.value, fields[0], `${at}.value`);
    if (!Array.isArray(x.value) || x.value.length !== fields.length) {
      return `${at}'s case holds ${fields.length} values`;
    }
    for (let i = 0; i < fields.length; i++) {
      const why = unlike(x.value[i], fields[i], `${at}.value[${i}]`);
      if (why) return why;
    }
    return null;
  };
  switch (s.t) {
    case "int":
      return Number.isSafeInteger(v) ? null : `${at} is no whole number`;
    case "float":
      return typeof v === "number" && Number.isFinite(v) ? null : `${at} is no number`;
    case "bool":
      return typeof v === "boolean" ? null : `${at} is no Bool`;
    case "string":
      return typeof v === "string" ? null : `${at} is no String`;
    case "unit":
      return v === null ? null : `${at} is no ()`;
    case "list": {
      if (!Array.isArray(v)) return `${at} is no list`;
      for (let i = 0; i < v.length; i++) {
        const why = unlike(v[i], s.of, `${at}[${i}]`);
        if (why) return why;
      }
      return null;
    }
    case "option":
      return ofCases(v, [["some", [s.of]], ["none", []]]);
    case "result":
      return ofCases(v, [["ok", [s.ok]], ["err", [s.err]]]);
    case "record": {
      if (!record(v)) return `${at} is no record`;
      const names = new Set(s.fields.map(([name]) => name));
      const extra = Object.keys(v).find((k) => !names.has(k));
      if (extra !== undefined) return `${at} holds \`${extra}\`, which its type does not`;
      for (const [name, f] of s.fields) {
        if (!(name in v)) return `${at} has no \`${name}\``;
        const why = unlike(v[name], f, `${at}.${name}`);
        if (why) return why;
      }
      return null;
    }
    case "cases":
      return ofCases(v, s.cases);
    default:
      return `${at}'s form \`${s.t}\` is none this runtime reads`;
  }
}

/** **What the store holds, decoded** against `schema`: `{ value }`, or `{
 * error }` naming what is wrong. */
function decodeKept(text, schema) {
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    return { error: "it is not JSON" };
  }
  const why = unlike(json, schema, "the value");
  return why ? { error: why } : { value: json };
}

/** The newest version of `held` the store holds after this page's, if any:
 * a newer build has taken it on, and this page writes it no more. */
function newerKept(store, held) {
  const prefix = `${held.name}@`;
  let newest = 0;
  for (let i = 0; i < store.length; i++) {
    const key = store.key(i);
    if (!key?.startsWith(prefix)) continue;
    const v = Number(key.slice(prefix.length));
    if (Number.isInteger(v) && v > held.version && v > newest) newest = v;
  }
  return newest;
}

/** **One value, read**: what the device holds of it at this page's version,
 * or taken to it from the newest earlier one, or the default. */
async function readKept(store, held) {
  if (!store) return { state: "unavailable" };
  if (newerKept(store, held)) return { state: "stale" };
  const schema = held.schemas[held.version - 1];
  const key = deviceKey(held, held.version);
  const text = store.getItem(key);
  if (text !== null) {
    const got = decodeKept(text, schema);
    if (got.error) return { state: "unreadable", why: got.error };
    return { state: "read", value: got.value };
  }
  for (let v = held.version - 1; v >= 1; v--) {
    const old = store.getItem(deviceKey(held, v));
    if (old === null) continue;
    const got = decodeKept(old, held.schemas[v - 1]);
    if (got.error) return { state: "unreadable", why: `version ${v}: ${got.error}` };
    let value = got.value;
    try {
      const steps = (await computedValues()).migrations?.[held.signal] ?? [];
      for (let at = v; at < held.version; at++) value = steps[at - 1](value);
    } catch (error) {
      return { state: "unreadable", why: `version ${v} did not migrate: ${error.message}` };
    }
    const why = unlike(value, schema, "the migrated value");
    if (why) return { state: "unreadable", why };
    // Written under its own key before the old one goes: a stop between the
    // two leaves both, and the next page reads the newer.
    try {
      store.setItem(key, wire(value));
      store.removeItem(deviceKey(held, v));
    } catch (error) {
      log.push(`device: ${held.name} migrated from ${v}, not kept: ${error.name}`);
    }
    return { state: "migrated", from: v, value };
  }
  return { state: "default" };
}

/** **A change kept**: the whole value, under its version's key. */
function keep(held, value, source) {
  if (deviceState[held.signal] === "stale") {
    if (source?.el) tell(source.el, DEVICE_SAYS.stale, "device-stale");
    else say(DEVICE_SAYS.stale);
    log.push(`device: ${held.name} not kept: a newer version holds it`);
    return;
  }
  const store = deviceStore();
  if (!store) return;
  const key = deviceKey(held, held.version);
  try {
    // An unreadable value is kept aside before the first write replaces it.
    if (deviceState[held.signal] === "unreadable") {
      const unread = store.getItem(key);
      if (unread !== null) store.setItem(`${key}!unreadable`, unread);
    }
    store.setItem(key, wire(value));
    deviceState[held.signal] = "read";
    deviceWrites++;
  } catch (error) {
    // The store refused it: what was kept stays (the HTML algorithm throws
    // before the map changes), and the page holds what the person did.
    if (source?.el) tell(source.el, DEVICE_SAYS.full, "device-full");
    else say(DEVICE_SAYS.full);
    log.push(`device: ${held.name} not kept: ${error.name}`);
  }
  window.__pw.device = { state: { ...deviceState }, writes: deviceWrites };
}

/** What another tab's write of `held` says, decoded as a read is. */
function heardValue(held, e) {
  if (e.newValue === null) return { state: "default" };
  const got = decodeKept(e.newValue, held.schemas[held.version - 1]);
  return got.error ? { state: "unreadable", why: got.error } : { state: "read", value: got.value };
}

/** **Another tab's write**: shown here, written nowhere. A `clear()`, whose
 * event has no key, is every value read again. */
async function heardKept(e) {
  const store = deviceStore();
  if (!store || e.storageArea !== store) return;
  for (const held of deviceHeld.values()) {
    if (e.key === null || e.key === deviceKey(held, held.version)) {
      const got = e.key === null ? await readKept(store, held) : heardValue(held, e);
      deviceState[held.signal] = got.state;
      if (got.state === "unreadable") log.push(`device: ${held.name} unreadable: ${got.why}`);
      log.push(`device: ${held.name} heard from another tab`);
      setSignal(held.signal, "value" in got ? got.value : firstValues.get(held.signal), null, false);
    } else if (e.key.startsWith(`${held.name}@`)) {
      const v = Number(e.key.slice(held.name.length + 1));
      if (Number.isInteger(v) && v > held.version && deviceState[held.signal] !== "stale") {
        deviceState[held.signal] = "stale";
        say(DEVICE_SAYS.stale);
        log.push(`device: ${held.name} is held at version ${v} by another page`);
      }
    }
  }
  window.__pw.device = { state: { ...deviceState }, writes: deviceWrites };
}

/** **The device read before what it decides is shown**: each value set,
 * what it decides rendered, and the root's mark taken away. The address's
 * fragment, which no server sees either, is followed first, so the device's
 * values are first shown as the address asks. */
async function readDevice() {
  if (deviceHeld.size === 0) return;
  const store = deviceStore();
  try {
    // Each value read first, then all of them set at once: one render shows
    // them, and the mark waits for that render. Set as each was read, a
    // value read later started a render of its own, and the mark waited for
    // that one alone, so an earlier value's part was shown at its default
    // while it was still being computed.
    const read = [];
    for (const held of deviceHeld.values()) {
      const got = await readKept(store, held);
      deviceState[held.signal] = got.state;
      if (got.state === "unreadable") log.push(`device: ${held.name} unreadable: ${got.why}`);
      log.push(`device: ${held.name} ${got.state}`);
      read.push([held, got]);
    }
    for (const [held, got] of read) {
      if ("value" in got) setSignal(held.signal, got.value, null, false);
    }
    if (parts.fragment) followFragment(parts.fragment);
    if (flushing) await flushing;
  } finally {
    document.documentElement.dataset.pwDevice = "read";
    window.__pw.device = { state: { ...deviceState }, writes: deviceWrites };
  }
  addEventListener("storage", (e) => void heardKept(e), { signal: life.signal });
}

// --- ADR-0152: a key a page changes ---------------------------------------
//
// A page's query given a signal is read again, for the new key, when the
// signal changes. The server answers in the session's frames, as one patch
// set, and applies a read only if it is the latest for this page. So the
// browser only says which key each binding asks for, and does what its stale
// work says:
//   cancel     the read in flight is aborted, and the new one sent;
//   supersede  the read in flight runs to its end, and the new one is sent;
//   keep       the new one is sent, and the server runs it after the one in
//              flight, dropping any it has not started that a newer replaced.
// In none is an old key's answer shown for the new key: the server applies
// only the latest read.

/** The document this page is: its first cursor, which each read names. */
const documentCursor = parts.cursor ?? 0;

/** Each binding a signal keys: its signals, its stale work, and its reads. */
const keyedReads = new Map(
  (parts.keyed ?? []).map((k) => [
    k.binding,
    { signals: k.signals, policy: k.on_key_change, seq: 0, current: null, inflight: null },
  ]),
);
for (const read of keyedReads.values()) read.current = keyOf(read);

/** A binding's key: each of its signals' values. */
function keyOf(read) {
  return JSON.stringify(Object.fromEntries(read.signals.map((s) => [s, signals.get(s)])));
}

/** The bindings whose keys `changed` changed, each asked for again. */
function readAgain(changed) {
  for (const [binding, read] of keyedReads) {
    if (!read.signals.some((s) => changed.has(s))) continue;
    const key = keyOf(read);
    // A key this page has, or is asking for, is not asked for again
    // (charter §15.6, test 6).
    if (key === read.current) continue;
    read.current = key;
    if (read.inflight && read.policy === "cancel") read.inflight.abort();
    ask(binding, read, key);
  }
}

/** One read of `binding` for `key`, numbered. */
function ask(binding, read, key) {
  const seq = ++read.seq;
  const controller = new AbortController();
  read.inflight = controller;
  busy(binding, true);
  window.__pw.reads = (window.__pw.reads ?? 0) + 1;
  const url =
    `/pw-read?binding=${encodeURIComponent(binding)}&seq=${seq}` +
    `&doc=${documentCursor}&key=${encodeURIComponent(key)}`;
  fetch(url, { signal: controller.signal })
    .then((r) => r.json())
    .then((answer) => log.push(`read ${binding} #${seq}: ${JSON.stringify(answer)}`))
    .catch((e) =>
      log.push(`read ${binding} #${seq} ${e?.name === "AbortError" ? "aborted" : "failed"}`),
    )
    .finally(() => {
      if (read.inflight !== controller) return;
      read.inflight = null;
      busy(binding, false);
    });
}

/** Mark what `binding` shows as waiting for a new key's answer. */
function busy(binding, waiting) {
  for (const p of parts.parts ?? []) {
    if (String(p.value ?? "").split(".")[0] !== binding) continue;
    const at = index.get(addressOf([], p.id))?.start?.parentElement;
    if (!at) continue;
    if (waiting) at.setAttribute("aria-busy", "true");
    else at.removeAttribute("aria-busy");
  }
}

// Leaving the page stops its reads (charter §15.6, test 8): the server, finding
// a read's request gone, lets go of a `cancel` read's flight.
addEventListener("pagehide", endReads, { signal: life.signal });

/** Each keyed read in flight let go of: the page is going, or handing over
 * to the next document's runtime. */
function endReads() {
  for (const read of keyedReads.values()) read.inflight?.abort();
}

/** Each event part's decision, asked once (E7-L): before anything binds. */
const verdicts = new Map();

// --- what a failed press is told -----------------------------------------
//
// **A refusal is told where the press was** (ADR-0302). Until 2026-10-09 a
// press a command's `requires` refused failed silently: the speculation was
// taken back, `data-pw-handler-error` set, a line logged, and the reader told
// nothing, the dead button this project exists to remove (the owner's
// finding, the feed's composer signed out). The words go beside the control,
// which its `aria-describedby` names, and to the page's announcer, a
// `role="status"` the host serves empty from the page's first byte, so a
// screen reader says them without the focus moving (WCAG 2.2 SC 4.1.3,
// ARIA22). Focus, the control and what was typed stay as they are.

/** A command its `requires` refused: the predicate, and its words. */
class Refused extends Error {
  constructor(component, predicate, says) {
    super(`command ${component} refused by ${predicate}`);
    this.predicate = predicate;
    this.says = says;
  }
}

/** A command no answer came for, its retries spent. */
class Unreachable extends Error {
  constructor(component, cause) {
    super(`command ${component} could not be sent: ${cause?.message ?? cause}`);
  }
}

/** The platform's words, where no predicate's apply. */
const PLATFORM_SAYS = {
  unreachable: "This could not be sent. Check the connection and try again.",
  stale: "This page is out of date. Reload it to go on.",
  failed: "This did not work. Try again.",
};

let announcer = null;

/** The page's announcer: the one the host served (`pw_render::ANNOUNCER`),
 * the same node from the first byte, or, from a host that served none, one
 * added once, before anything is said. */
function announcerOf() {
  if (announcer) return announcer;
  announcer = document.querySelector(".pw-announcer[role=status]");
  if (!announcer) {
    announcer = document.createElement("div");
    announcer.setAttribute("role", "status");
    announcer.className = "pw-announcer";
    document.body.append(announcer);
  }
  // Seen by no one, said to whoever reads the page by ear. Through the
  // CSSOM, which the page's policy allows where it allows no `style`
  // attribute: the host serves the announcer empty and unstyled.
  announcer.style.cssText =
    "position:absolute;width:1px;height:1px;margin:-1px;padding:0;border:0;" +
    "overflow:hidden;clip-path:inset(50%);white-space:nowrap";
  return announcer;
}

/** Each control's message, beside it. */
const messages = new WeakMap();
let told = 0;
/** The announcer's words waiting to be written. */
let saying = 0;

/** Tell `words` where `el` was pressed, as `kind`, and say them. */
function tell(el, words, kind) {
  let message = messages.get(el);
  if (!message?.isConnected) {
    message = document.createElement("span");
    message.className = "pw-refusal";
    message.id = `pw-said-${++told}`;
    el.after(message);
    messages.set(el, message);
  }
  message.textContent = words;
  const described = (el.getAttribute("aria-describedby") ?? "").split(" ").filter(Boolean);
  if (!described.includes(message.id)) {
    el.setAttribute("aria-describedby", [...described, message.id].join(" "));
  }
  el.dataset.pwHandlerError = kind;
  say(words);
  log.push(`told ${kind}: ${words}`);
}

/** **Said by the page's announcer**: emptied, then written, so the same
 * words told twice are said twice. Words told before the last were
 * written are not said at all: the last are. */
function say(words) {
  const status = announcerOf();
  status.textContent = "";
  clearTimeout(saying);
  saying = setTimeout(() => {
    status.textContent = words;
  }, 50);
}

/** What `el`'s last press was told, taken back: it is pressed again. */
function untell(el) {
  const message = messages.get(el);
  if (message) {
    message.textContent = "";
    const described = (el.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .filter((id) => id && id !== message.id);
    if (described.length) el.setAttribute("aria-describedby", described.join(" "));
    else el.removeAttribute("aria-describedby");
  }
  delete el.dataset.pwHandlerError;
}

/** The recoveries a press performs by reading the document again: each is
 * the region, the slot or the interaction as the build serving the page
 * renders it now. A reload never replays the press: a mutation the user did
 * not ask for again is how a version mismatch becomes a double charge. */
const RELOADING = new Set(["reload", "refetch-region", "rerender-private-slot", "retry-interaction"]);

/**
 * **A refused handler recovers when pressed** (charter §15.6 test 16). The
 * resume decision refused it, and said how to recover. A document from another
 * build is read again, once, and the press is not replayed. Until 2026-10-03 a
 * press on it did nothing and said nothing.
 *
 * A reload asked for again within ten seconds, for the same address, is not
 * made. Otherwise a server still sending the stale document would reload the
 * page for ever. The button then stays inert, and says so.
 */
function recoverOnPress(part, owners, verdict) {
  const listens = DOM_EVENTS[part.event || "press"];
  if (!listens) return;
  for (const el of owners) {
    const parts = bound.get(el) ?? new Set();
    if (parts.has(part.id)) continue;
    parts.add(part.id);
    bound.set(el, parts);
    el.addEventListener(listens, (e) => {
      const asks = verdict.recovery === "require-user-confirmation";
      if (!RELOADING.has(verdict.recovery) && !asks) {
        tell(el, PLATFORM_SAYS.stale, verdict.recovery);
        log.push(`press on refused ${part.id}: ${verdict.recovery}, nothing to do`);
        return;
      }
      e.preventDefault();
      if (asks && !confirm("This page is out of date. Load it again? What you typed is not kept.")) {
        return;
      }
      const key = `pw-recovered:${location.pathname}`;
      let last = 0;
      try {
        last = Number(sessionStorage.getItem(key) ?? 0);
      } catch {}
      if (Date.now() - last < 10_000) {
        tell(el, PLATFORM_SAYS.stale, "reload-loop");
        log.push(`press on refused ${part.id}: read again already; not again`);
        return;
      }
      try {
        sessionStorage.setItem(key, String(Date.now()));
      } catch {}
      log.push(`press on refused ${part.id}: ${verdict.recovery}, reading the page again`);
      window.__pw.reloading = true;
      location.reload();
    }, { signal: life.signal });
  }
}
/** When the latest press's handler started (ADR-0152): the next press's
 * starts after it, so presses run in the order they were made. */
let lastPressStarted = Promise.resolve();
/** Each press whose handler has not finished, as a promise of its finishing
 * (ADR-0280): a navigation waits for each made before it. */
const pressing = new Set();
/** Whether a handler has gone to another page (ADR-0280): from then the page
 * takes no press and no second navigation. A page the browser brings back
 * from its back-forward cache takes them again. */
let navigating = false;
// **A page shown again from the back-forward cache** takes presses again,
// and listens again: its subscription ended when it was hidden (`leaving`),
// and until 2026-10-09 it was never asked again, so the page was told
// nothing until reloaded (found mapping the runtime for the soft
// navigation's ADR). A server that has forgotten the document answers with
// a reload, as it does any document it does not know.
addEventListener(
  "pageshow",
  (e) => {
    if (!e.persisted) return;
    navigating = false;
    if (leaving && parts.listens !== false) {
      leaving = false;
      subscribe();
    }
  },
  { signal: life.signal },
);
/**
 * **A handler goes to a page once its command commits** (ADR-0280). Its
 * module calls this last in the `Ok` arm of a command's answer, and an `Ok`
 * is a commit: the command callback refuses one that is not. From now the
 * page takes no press. Each press made before this one finishes first, its
 * command answered and its arms run, so the page it goes to is read after
 * every commit this page was answered. Then the browser loads it: a document
 * the server serves after the session's turn, never from a cache
 * (`no-store`), so no parameter busts one and nothing is reloaded. A second
 * navigation, made while the page leaves, is not taken: the first decides.
 */
async function navigate(route, args, mine) {
  const url = pageAddress(route, args);
  // A page shown in a layout keeps it (the soft navigation's ADR), held to
  // the same two rules, by the same two functions.
  if (parts.layout) return softNavigate(url, "push", mine);
  if (!taken(url)) return;
  navigating = true;
  await pressedBefore(mine);
  log.push(`navigating to ${url}`);
  leaveFor(url);
}

/**
 * **Whether a navigation to `url` is taken** (ADR-0280): not while the page
 * leaves, the first navigation deciding, unless it is a reader's own choice
 * (a link, back or forward) replacing a soft navigation in flight (the soft
 * navigation's ADR). One rule for both ways a page leaves, so a test of
 * either holds it.
 */
function taken(url, chosen = false) {
  if (navigating && !(chosen && pending)) {
    log.push(`navigation to ${url} not taken: the page is already leaving`);
    return false;
  }
  return true;
}

/**
 * **Each press made before `mine`, answered first** (ADR-0280): the page a
 * navigation goes to is read after every commit this page was answered. One
 * wait for both ways a page leaves.
 */
function pressedBefore(mine) {
  return Promise.allSettled([...pressing].filter((p) => p !== mine));
}

/** Load `url` whole, the page leaving for it; `reload`: this address. */
function leaveFor(url, reload = false) {
  // A navigation stopped, by the user or `window.stop()`, leaves the page
  // where it was, and it takes presses again: where the browser says so.
  // The Navigation API's `navigate` event fires for this one too, and its
  // signal aborts when it is stopped. Where there is none, the page stays
  // leaving until it is loaded again.
  if (typeof navigation !== "undefined") {
    navigation.addEventListener(
      "navigate",
      (e) => e.signal.addEventListener("abort", () => (navigating = false), { once: true }),
      { once: true },
    );
  }
  if (reload) location.reload();
  else location.assign(url);
}

/**
 * **A page's address** (ADR-0280): each `{name}` segment of its route given
 * its value, encoded as the renderer encodes a link's (`url_component`):
 * every UTF-8 byte but RFC 3986's unreserved characters, so a value is one
 * segment, whatever it holds. A value no segment can carry, empty, `.` or
 * `..`, which the URL parser drops or climbs by, is refused, and the press
 * fails visibly.
 */
function pageAddress(route, args) {
  // The route's query part, RFC 6570's form-style expansion (ADR-0309):
  // `?name=value` for each name given as text, `&` between, nothing for one
  // not given; its values encoded as a segment's are.
  const at = route.indexOf("{?");
  const path = at < 0 ? route : route.slice(0, at);
  const names = at < 0 ? [] : route.slice(at + 2, -1).split(",").map((n) => n.trim());
  const query = names
    .filter((n) => typeof args[n] === "string")
    .map((n) => `${n}=${encodeUnreserved(args[n])}`);
  return query.length ? `${pathAddress(path, args)}?${query.join("&")}` : pathAddress(path, args);
}

/** Each byte of `value` outside RFC 3986's unreserved set, as `%XX`. */
function encodeUnreserved(value) {
  let out = "";
  for (const b of new TextEncoder().encode(value)) {
    const unreserved =
      (b >= 0x41 && b <= 0x5a) || (b >= 0x61 && b <= 0x7a) || (b >= 0x30 && b <= 0x39) ||
      b === 0x2d || b === 0x2e || b === 0x5f || b === 0x7e;
    out += unreserved ? String.fromCharCode(b) : "%" + b.toString(16).toUpperCase().padStart(2, "0");
  }
  return out;
}

/** A route's path, each `{name}` segment given its value (ADR-0280). */
function pathAddress(route, args) {
  return route.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_, name) => {
    const value = args[name];
    if (typeof value !== "string") {
      throw new Error(`no value for {${name}} in ${route}`);
    }
    if (value === "" || value === "." || value === "..") {
      throw new Error(`{${name}} in ${route} cannot be ${JSON.stringify(value)}: no segment carries it`);
    }
    return encodeUnreserved(value);
  });
}
/** Each element's parts a listener is bound for: an element may handle two
 * events, `on:input` and `on:keydown`, each its own listener. */
const bound = new WeakMap();

/**
 * **What the browser calls each event the platform declares** (ADR-0138).
 * Until 2026-10-02 every handler listened for a click, whatever its event.
 */
const DOM_EVENTS = {
  press: "click",
  // Track `todomvc` (ADR-0367): a double press, and a control's losing focus.
  dblclick: "dblclick",
  blur: "blur",
  input: "input",
  change: "change",
  keydown: "keydown",
  submit: "submit",
  close: "close",
};

/**
 * **An event's record** (ADR-0131), the plain data its handler is given, as
 * `packages/pw-platform-web/events.pw` declares it. Read here, in the
 * listener: the DOM event is spent by the time a handler's module has loaded.
 */
function eventRecord(name, e) {
  switch (name) {
    case "press":
    case "dblclick":
      return { x: Math.round(e.clientX ?? 0), y: Math.round(e.clientY ?? 0) };
    case "input":
    case "change":
    case "blur":
      return { value: String(e.target?.value ?? "") };
    case "keydown":
      return { key: String(e.key ?? "") };
    case "submit":
      return { prevented: e.defaultPrevented === true };
    case "close":
      return { value: String(e.target?.returnValue ?? "") };
    default:
      return {};
  }
}
let booted = false;

/**
 * Bind each authorised handler to each of its elements not bound yet.
 *
 * At boot, every element the document has. After a block a signal decides
 * is rendered again (ADR-0130), what it made: its old elements are gone with
 * their listeners, and the elements still standing keep theirs.
 */
function bindEvents() {
  for (const [template, list] of manifests) {
    for (const part of list) bindEvent(template, part);
  }
}

/** One event part of `template`, bound to every element it has: the
 * page's, or a view's in each of its instances (ADR-0203). */
function bindEvent(template, part) {
  if (part.kind !== "event") return;
  // A part of a view's instance is named by its template, and the page's
  // as it always was.
  const named = template === PAGE ? `${part.id}` : `${template}:${part.id}`;
  // Every INSTANCE of this template position. A template-scoped id names a
  // position in the template; the document has one instance per loop item,
  // and all three Add buttons carry `data-pw="0"`.
  const addresses = addressesFor(`e${part.owner}`, template);
  const owners = addresses.map((a) => index.get(a).element).filter(Boolean);
  if (owners.length === 0) {
    // A part in a block not rendered now (ADR-0130) is bound when it is.
    if (!booted) log.push(`no element ${part.owner} for part ${named}`);
    return;
  }

  // The decision, before anything is bound. `decide` returning anything but
  // "attach" leaves the element without a listener — the button is inert
  // rather than wrong.
  //
  // Asked NOW rather than on interaction, deliberately. A refused handler
  // must never load its code: fetching first and checking after would make
  // the refusal a formality the network had already ignored.
  let verdict = verdicts.get(named);
  if (!verdict) {
    const manifest = manifestFor(part);
    verdict =
      decide && manifest ? decide(manifest) : { attach: false, recovery: "no-decision" };
    verdicts.set(named, verdict);
    if (!verdict.attach) {
      // The CODE as well as the recovery. "refused 5: none" says a handler
      // was refused and nothing about why; the code is the one field that
      // distinguishes an unknown handler from a widened privacy scope from
      // a stale document schema.
      log.push(`refused ${named}: code ${verdict.code} recovery ${verdict.recovery}`);
    }
  }
  if (!verdict.attach) {
    recoverOnPress(part, owners, verdict);
    return;
  }

  // The event the part handles, and what to do before any code loads.
  const event = part.event || "press";
  const listens = DOM_EVENTS[event];
  if (!listens) {
    log.push(`no listener for event ${event} of part ${named}`);
    return;
  }
  const modifiers = part.modifiers ?? [];

  let fresh = 0;
  for (const el of owners) {
    // Bound once: a block a signal renders again binds what it made, and
    // leaves what was bound before alone (ADR-0130).
    const parts = bound.get(el) ?? new Set();
    if (parts.has(part.id)) continue;
    parts.add(part.id);
    bound.set(el, parts);
    fresh++;
    el.addEventListener(listens, async (e) => {
      // Declared, so done now, synchronously (ADR-0131).
      if (modifiers.includes("prevent")) e.preventDefault();
      if (modifiers.includes("stop")) e.stopPropagation();
      // **A page that is leaving takes no press** (ADR-0280): a handler has
      // gone to another page once its command committed, and that page is
      // loading. A press now would send what no one here would read.
      if (navigating) {
        log.push(`press on ${part.value} not taken: the page is leaving`);
        return;
      }
      lastActed = el;
      // What its last press was told, taken back (ADR-0302).
      untell(el);
      const record = eventRecord(event, e);
      // Which instance each signal the handler names is, for this use of
      // its view (ADR-0144). Read at the press: the element is the one the
      // block rendered last.
      const instances = JSON.parse(el.dataset.pwSignals ?? "{}");
      const instance = (name) => instances[name] ?? name;
      // **Presses run in the order they were made** (ADR-0152). Each
      // handler's module loads on its own, so a second press's could arrive
      // first; its handler waits for the press before it to start. Until
      // 2026-10-03 it did not wait: Hot then Cold, pressed quickly, could
      // leave Hot chosen.
      const myTurn = lastPressStarted;
      let started;
      lastPressStarted = new Promise((resolve) => (started = resolve));
      // This press until its handler finishes: a navigation waits for each
      // press made before it (ADR-0280).
      let finished;
      const done = new Promise((resolve) => (finished = resolve));
      pressing.add(done);
      try {
        // Authorised above; loaded here. The order is the point of E7-L:
        // the bytes for this handler do not exist in this page until
        // somebody presses this button.
        let module;
        try {
          module = await loadHandler(part.value);
        } finally {
          // Its turn, loaded or not: a press whose code failed to load
          // must not hold up every press after it.
          await myTurn;
          started();
        }
        // The command COMMITS and returns nothing about the cart. The
        // browser learns the new value from the RESOURCE, because that is
        // what the program declares the page depends on:
        //
        //   command add_to_cart(..) invalidates Cart(current_session())
        //
        // A response carrying the value would make the UI change because an
        // endpoint said so, which is the thing E6 exists to replace.
        //
        // The module is the handler's compiled BODY (E10, 2026-09-25). It
        // reads what it captured from this element, where the renderer
        // serialized exactly the paths it reads, and calls its command with
        // the arguments it computed.
        // `getRandomValues`, not `randomUUID`: the second exists only in a
        // secure context, and a press must have an interaction everywhere.
        const press = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) =>
          b.toString(16).padStart(2, "0"),
        ).join("");
        let calls = 0;
        await module.run({
          captures: JSON.parse(el.dataset.pwCaptures ?? "{}"),
          // What the event was, read in the listener (ADR-0138).
          event: record,
          // The page's signals (ADR-0130), as JSON carries them, and where
          // a change came from (ADR-0142).
          // Through the instance this element's use of a view holds each
          // signal as, where the compiler wrote one (ADR-0144).
          get: (name) => signals.get(instance(name)),
          set: (name, value) => setSignal(instance(name), value, { el, event }),
          // Each command speculates before its request (ADR-0122), and its
          // speculation is resolved by the answer, whatever it is.
          command: async (component, args, how = {}) => {
            const interaction = `${press}-${calls++}`;
            const ids = await speculate(component, args, interaction);
            let answer;
            try {
              answer = await command(component, args, interaction, how.retry);
            } catch (error) {
              await restoreSpeculation(ids);
              throw error;
            }
            await resolveSpeculation(ids, answer.committed === true, answer.basis);
            // What the command answered, as its handler's code reads it
            // (ADR-0157): `Ok`, or a declared `Err` the handler can show.
            // A command that trapped or was refused answered no value, and
            // the press fails visibly.
            if (!("result" in answer)) {
              throw new Error(`${component} answered no value`);
            }
            // **An answer that is not a declared error is a commit**
            // (ADR-0280): an `Ok` arm runs only after one, and a navigation
            // in it goes only after one. One that did not commit has failed
            // the press, whatever it says.
            if (answer.committed !== true && answer.result?.$case !== "err") {
              throw new Error(`${component} answered without committing`);
            }
            return answer.result;
          },
          // Goes to a page once the command it follows commits (ADR-0280).
          navigate: (route, args) => navigate(route, args, done),
        }, { signal: life.signal });
      } catch (error) {
        // A load or a refused command is VISIBLE and leaves the button
        // usable. A silent failure here is the worst outcome available: the
        // press did nothing, the page looks fine, and the next press is the
        // user's only way to find out.
        loaded.delete(part.value);
        attempts.set(part.value, (attempts.get(part.value) ?? 0) + 1);
        // Told where the press was (ADR-0302): a refusal in its predicate's
        // words, anything else in the platform's.
        if (error instanceof Refused) {
          tell(el, error.says || PLATFORM_SAYS.failed, `refused:${error.predicate}`);
        } else if (error instanceof Unreachable) {
          tell(el, PLATFORM_SAYS.unreachable, "unreachable");
        } else {
          tell(el, PLATFORM_SAYS.failed, "failed");
        }
        log.push(`handler ${part.value} failed: ${error.message ?? error}`);
        window.__pw.handlerErrors = (window.__pw.handlerErrors ?? 0) + 1;
      } finally {
        pressing.delete(done);
        finished();
      }
    });
  }
  if (fresh > 0) {
    log.push(
      `attached ${named} to ${owners.length} instance(s): ${addresses.join(" ")}`,
    );
  }
}

async function attach() {
  // Marked, so activation cost is a MEASUREMENT rather than a wall-clock guess
  // taken from outside. `performance.measure` attributes the work to this
  // runtime; a stopwatch around navigation would also count the network, the
  // parser and the paint.
  performance.mark("pw:activate:start");

  // One traversal, before anything else. Every later lookup is a map hit.
  const indexed = buildIndex();
  // The announcer a failed press is said by, there before anything is
  // (ADR-0302): a live region added with its words is not reliably said.
  announcerOf();
  log.push(`indexed ${indexed} address(es)`);
  for (const p of parts.parts ?? []) {
    if (p.kind === "stream" && streamPending(String(p.id))) pendingAtIndex.add(String(p.id));
  }

  await bootDecision();

  // What the device keeps, read, and what it decides shown, before any
  // handler binds (track `device-store`).
  await readDevice();

  bindEvents();
  booted = true;
  // A dialog the server rendered at a signal's first value is shown as one
  // the browser renders is (ADR-0141).
  for (const live of parts.live ?? []) {
    if (live.kind !== "conditional" && live.kind !== "match") continue;
    const r = index.get(addressOf([], live.part));
    if (r?.start) showDialogs(between(r.start, r.end));
  }

  performance.mark("pw:activate:end");
  performance.measure("pw:activate", "pw:activate:start", "pw:activate:end");

  // The regions still pending, filled as the response goes on (ADR-0148).
  watchStreams();

  // A page whose values are its signals alone listens for nothing
  // (ADR-0130): no resource of its changes on the server.
  if (parts.listens !== false) subscribe();
  // The signal a page binds to its address's fragment follows it from here.
  if (parts.fragment) bindFragment(parts.fragment);
  // What a block shows from here on, with `autofocus`, takes focus.
  followAutofocus();
  window.__pw.signals = Object.fromEntries(signals);
  window.__pw.ready = true;
  document.documentElement.dataset.pwReady = "1";
}

// --- the protocol ---------------------------------------------------------
//
// The browser knows five things: `ResourceEntryId`, `Version`, `PartAddress`,
// `StreamFrame` and `Patch`. It does not know `EntryKey`, SQLite, outbox rows,
// compiler graph nodes or anything else the server owns.
//
// Held versions are per ENTRY, not per page: `Store(47)` at 8 and
// `Cart(session_A)` at 14 are two independent facts, and one page version
// would make either one lie.

/**
 * ResourceEntryId → the version the DOCUMENT reflects.
 *
 * Advanced only when a patch is applied. Hearing that newer state exists is a
 * different fact and lives in `known`.
 */
const held = new Map();

/** ResourceEntryId → the newest version the server has told us about. */
const known = new Map();

/** The addresses the last frame batch updated, for the tests to read. */
window.__pw.updated = [];

/**
 * Would applying this basis move any dependency backward?
 *
 * Asked over EVERY entry, because a patch derived from a fresh cart and a
 * stale promotion would show a total computed from state the page has already
 * moved past. An empty basis has no causal claim and is refused: "no basis" is
 * not "any basis".
 */
function isNewer(basis) {
  const resources = basis?.resources ?? [];
  if (resources.length === 0) return false;
  let advances = false;
  for (const r of resources) {
    const current = held.get(r.entry);
    if (current === undefined) {
      advances = true;
    } else if (r.version < current) {
      return false;
    } else if (r.version > current) {
      advances = true;
    }
  }
  return advances;
}

/** The index key for a `PartAddress`, matching `pw_document::PartAddress::key`. */
function addressKey(address) {
  const path = (address.instances ?? []).map((f) => `${f.scope}@${f.instance}`);
  return `${address.template}/${path.join("/")}|${address.part}`;
}

/**
 * **An attribute a change reaches** (ADR-0168): the element the part
 * belongs to, by its owner in the parts manifest, in the instance the
 * address names. The value is as the document's HTML writes it, and the
 * platform's own parser reads it, so a patched attribute means what a
 * rendered one does. Until 2026-10-03 no patch set an attribute: an Add button
 * kept the name of an item renamed since.
 *
 * **A checkbox's state follows the attribute** (track `todomvc`, ADR-0367).
 * `checked` is an attribute only until a person has pressed the box: from
 * then on the property is the box's own, and the attribute no longer moves it.
 * A patch that unchecks the "toggle all" box, or checks it, is shown only if
 * the property is set as well, as a signal-set attribute already does
 * (`setAttributePart`). Until then a box a person had pressed stayed as they
 * left it, whatever the page said it was. (An option's `selected` is the same
 * and is not set here: no page has needed it yet, and none tests it.)
 */
function setAttributeAt(address, op) {
  const part = (parts.parts ?? []).find((p) => p.id === address.part);
  if (part?.owner == null) return false;
  const path = (address.instances ?? []).map((f) => `${f.scope}@${f.instance}`);
  const element = index.get(`${address.template}/${path.join("/")}|e${part.owner}`)?.element;
  if (!element) return false;
  if (op.op === "remove_attribute") {
    element.removeAttribute(op.name);
    if (op.name === "checked" && "checked" in element) element.checked = false;
    return true;
  }
  const parsed = document.createElement("template");
  parsed.innerHTML = `<i ${op.name}="${op.value}"></i>`;
  const value = parsed.content.firstChild.getAttribute(op.name) ?? "";
  // A value set again, the same, is a change all the same, and a control's
  // name changed is said again where it has focus (ADR-0182).
  if (element.getAttribute(op.name) !== value) element.setAttribute(op.name, value);
  if (op.name === "checked" && "checked" in element) element.checked = true;
  return true;
}

const isAttributeOp = (op) => op.op === "set_attribute" || op.op === "remove_attribute";

function applyFrame(frame) {
  if (frame.protocol !== undefined && frame.protocol !== 1) {
    // Incomparable, not different. A frame from another protocol version is
    // refused before anything it contains is interpreted.
    log.push(`refused frame: protocol ${frame.protocol}`);
    return;
  }
  switch (frame.frame) {
    case "resource_changed":
      // A NOTICE, not an application.
      //
      // `held` is what the DOCUMENT reflects, and hearing that newer state
      // exists does not make the document reflect it. A first version advanced
      // `held` here, and then the patch that realized the same version
      // advanced nothing and was refused as stale — the page stayed at 0 while
      // both frames arrived and both were "handled".
      //
      // What a notice is for: a subscriber with no patch coming may refetch,
      // and one that has already applied a patch learns nothing new.
      known.set(frame.entry, frame.version);
      log.push(`resource ${frame.entry.slice(0, 8)} at ${frame.version}`);
      return;

    case "patch": {
      if (!isNewer(frame.basis)) {
        window.__pw.ignored = (window.__pw.ignored ?? 0) + 1;
        log.push(`ignored a patch that advances nothing`);
        return;
      }
      for (const r of frame.basis.resources) held.set(r.entry, r.version);
      const key = addressKey(frame.target);
      const op = frame.operation;
      const at = frame.basis.resources.map((r) => r.version).join(",");

      if (op.op === "replace_text") {
        if (setRange(key, op.text)) {
          window.__pw.updated.push(key);
          log.push(`updated ${key} at version ${at}`);
        }
        return;
      }
      if (isAttributeOp(op)) {
        if (setAttributeAt(frame.target, op)) {
          window.__pw.updated.push(`${key}:${op.name}`);
          log.push(`${op.op} ${op.name} on ${key} at version ${at}`);
        }
        return;
      }

      // Structural operations on a keyed collection. The target names the LOOP;
      // `op.instance` names which of its instances.
      const scope = frame.target.part;
      if (applyList(key, scope, op)) {
        // The index maps addresses to nodes, and a structural change moves
        // nodes. Rebuilt rather than patched incrementally: an index that
        // tracked its own edits is a second model of the document, and the
        // document is right here to be read.
        buildIndex();
        window.__pw.updated.push(`${key}:${op.op}`);
        log.push(`${op.op} on ${key} at version ${at}`);
      } else {
        log.push(`refused ${op.op}: no instance ${op.instance}`);
        window.__pw.refused = (window.__pw.refused ?? 0) + 1;
      }
      return;
    }

    case "patch_set": {
      // **One change, every place it reaches** (ADR-0145): applied in order,
      // and the version held once all of them applied.
      if (!isNewer(frame.basis)) {
        window.__pw.ignored = (window.__pw.ignored ?? 0) + 1;
        log.push(`ignored a patch set that advances nothing`);
        return;
      }
      const at = frame.basis.resources.map((r) => r.version).join(",");
      for (const { target, operation: op } of frame.patches) {
        const key = addressKey(target);
        const applied =
          op.op === "replace_text"
            ? setRange(key, op.text)
            : op.op === "replace_range"
              ? replaceRangeAt(key, op.html) !== null
              : isAttributeOp(op)
                ? setAttributeAt(target, op)
                : applyList(key, target.part, op);
        if (!applied) {
          // The document is not what the server derived the change from, and
          // every later change would be derived from it too. Never "try
          // anyway": the version is not held, and a fresh document comes with
          // a fresh queue, as a `reload` recovery does.
          window.__pw.refused = (window.__pw.refused ?? 0) + 1;
          log.push(`refused ${op.op} on ${key}: the document disagrees; reloading`);
          if (!window.__pw.reloading) {
            window.__pw.reloading = true;
            showAgain();
          }
          return;
        }
        // A block a speculation renders too (ADR-0172): what the server
        // rendered is what it shows now.
        if (op.op === "replace_range" && !target.instances?.length) {
          shownBlocks.set(target.part, op.html);
        }
        if (op.op === "replace_text") {
          window.__pw.updated.push(key);
          log.push(`updated ${key} at version ${at}`);
        } else if (isAttributeOp(op)) {
          window.__pw.updated.push(`${key}:${op.name}`);
          log.push(`${op.op} ${op.name} on ${key} at version ${at}`);
        } else {
          // A structural change moves nodes: the index is read again, so the
          // next patch in the set finds what this one put in place.
          buildIndex();
          window.__pw.updated.push(`${key}:${op.op}`);
          log.push(`${op.op} on ${key} at version ${at}`);
        }
      }
      for (const r of frame.basis.resources) held.set(r.entry, r.version);
      // What a change inserted may be pressed.
      bindEvents();
      return;
    }

    case "entry_value":
      entryValue(frame);
      return;

    case "recovery":
      // Never "try anyway".
      log.push(`recovery: ${JSON.stringify(frame.recovery)}`);
      // A reload is the one recovery the page performs by itself. The server
      // sends it when it can no longer say what this document missed: the
      // subscriber fell too far behind, or was forgotten while idle (E10 gate
      // item 3). Applying later frames to this document would be exactly
      // "try anyway"; a fresh document comes with a fresh queue. Once, so a
      // second reload frame in the same batch does not queue a second one.
      if (frame.recovery?.recovery === "reload" && !window.__pw.reloading) {
        window.__pw.reloading = true;
        showAgain();
      }
      return;

    default:
      log.push(`unknown frame`);
  }
}

/**
 * Where this page is in its subscriber's frame sequence.
 *
 * Carried by the document rather than starting at zero. A reloaded page's
 * document already contains every change committed before it was rendered, so
 * asking from zero would replay them — and replaying a structural patch is not
 * a no-op: an item would be inserted twice.
 */
let cursor = parts.cursor ?? 0;

/**
 * Apply one delivered batch.
 *
 * Every adapter ends here, and this function has no idea which one called it.
 * That is the whole design: `pw-protocol` names no transport, so a transport
 * cannot smuggle meaning into a frame — and a second adapter is how that stops
 * being a claim and becomes something a test can fail.
 */
function applyBatch(batch) {
  // A batch of a document this runtime has handed over is not the page's,
  // and one that arrives while the page leaves, to be shown in its layout or
  // loaded whole, is applied to nothing: a reload it asked for, or a patch
  // that missed, would load this page again, and the navigation with it.
  if (life.signal.aborted || navigating) return;
  window.__pw.updated = [];
  // The page's patches are derived from what it holds (ADR-0172), and focus
  // in a row stays with its row through them.
  const focused = focusedRow();
  revertLists();
  for (const frame of batch.frames ?? []) applyFrame(frame);
  reapplySpeculations().then(() => refocus(focused));
  // Advanced only AFTER applying. Advancing on receipt would acknowledge
  // frames a mid-batch failure never applied, and the server would drop them.
  cursor = batch.cursor ?? cursor;
  window.__pw.cursor = cursor;
}

/**
 * The long poll: one batch per request.
 *
 * The cursor is BOTH the request and the acknowledgement — asking for what
 * follows N tells the server that N arrived and was applied. A response that
 * never reaches this page costs nothing, because the frames are still there
 * for the next ask.
 */
async function pollOnce() {
  // Which document this page is (ADR-0161): each is its own subscriber.
  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}`, {
    signal: life.signal,
  });
  applyBatch(await response.json());
}

/**
 * The streaming adapter: one connection, many batches.
 *
 * Newline-delimited JSON, read incrementally. A partial line is held rather
 * than parsed — a batch split across two network chunks is the normal case,
 * not an error, and parsing half of one would apply half a change.
 */
async function streamOnce() {
  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}&mode=stream`, {
    signal: life.signal,
  });
  if (!response.body) throw new Error("no streaming body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let held = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) return;
    held += decoder.decode(value, { stream: true });
    const lines = held.split("\n");
    held = lines.pop() ?? "";
    for (const line of lines) {
      if (line) applyBatch(JSON.parse(line));
    }
  }
}

/**
 * Which adapter to use.
 *
 * Streaming where the engine supports a readable response body, long poll
 * otherwise — and long poll again if streaming fails once, because a transport
 * that cannot connect is not a reason to stop subscribing. `?transport=` lets
 * a test pin one, so "both adapters deliver the same frames" is measured
 * rather than assumed.
 */
function chosenTransport() {
  const pinned = new URLSearchParams(location.search).get("transport");
  if (pinned === "poll") return "poll";
  if (pinned === "stream") return "stream";
  return typeof ReadableStream === "function" ? "stream" : "poll";
}

/** The page is going away: its requests fail because it is, and nothing is
 * asked again. */
let leaving = false;
addEventListener(
  "pagehide",
  () => {
    leaving = true;
  },
  { signal: life.signal },
);

/**
 * **The subscription, kept** (charter §15.5's forced reconnect). A failed
 * request is a dropped connection, a server restarting or a network that
 * blinked, and it is asked again, from the cursor this page holds, after a
 * pause that grows to five seconds. Until 2026-10-03 a failure ended the
 * subscription for good: its fallback to the long poll waited on
 * `chosenTransport()` answering differently, which it never does, and the page
 * stopped hearing changes without a word.
 */
async function subscribe() {
  let transport = chosenTransport();
  const pinned = new URLSearchParams(location.search).has("transport");
  window.__pw.transport = transport;
  let failures = 0;
  for (;;) {
    try {
      if (transport === "stream") await streamOnce();
      else await pollOnce();
      failures = 0;
    } catch {
      if (leaving || life.signal.aborted) return;
      failures += 1;
      // A stream that fails twice in a row falls back to the long poll, and
      // says so: a silent fallback would make the streaming adapter
      // untestable. A test that pins the adapter keeps it.
      if (transport === "stream" && failures >= 2 && !pinned) {
        transport = "poll";
        window.__pw.transport = "poll (fell back)";
        log.push("transport fell back to long poll");
      }
      const pause = Math.min(250 * 2 ** (failures - 1), 5000);
      window.__pw.reconnects = (window.__pw.reconnects ?? 0) + 1;
      log.push(`subscription failed; asking again in ${pause} ms`);
      await new Promise((resolve) => setTimeout(resolve, pause));
      if (leaving || life.signal.aborted) return;
    }
  }
}

// Exposed so a test can deliver a frame out of order. A transport cannot be
// made to do that on demand, and the property is about the page's guard rather
// than about the transport.
window.__pwTestApply = applyFrame;
// And a batch, as a transport delivers one: what a page leaving does with
// it is the batch's guard, not a frame's (the soft navigation's ADR).
window.__pwTestBatch = applyBatch;
window.__pwHeld = () => Object.fromEntries(held);
window.__pwKnown = () => Object.fromEntries(known);

/** Update every part whose value the resource carries. */
function apply(values) {
  window.__pw.updated = [];
  for (const p of parts.parts ?? []) {
    if (p.kind !== "text") continue;
    if (!(p.value in values)) continue;
    for (const address of addressesFor(String(p.id))) {
      if (setRange(address, String(values[p.value]))) {
        window.__pw.updated.push(address);
      }
    }
  }
  if (window.__pw.updated.length) {
    log.push(`updated ${window.__pw.updated.join(",")} at version ${values.version}`);
  }
}

// --- a navigation that keeps the layout (the soft navigation's ADR) ---------
//
// The owner's Next.js bug, its second half: save, go to a page, and see the
// header mounted again. A page shown in a layout (the layouts' ADR) is shown
// in it, and a navigation to another page of the same layout and the same
// build keeps it: the next page's document is fetched as a load fetches it,
// after the commit before it (ADR-0280), and only its slot shown in place of
// this one's. The layout's markup stays; each region of it the next document
// renders differently is taken from it, so what the host derives its next
// patches from is what the page shows. Then this runtime hands the page to
// a runtime of the next document's, and ends.

/** The slot's markers, the page's markup between them. */
const SLOT = ["pw-slot", "/pw-slot"];

/** The markers in `doc`'s body, or `null`. */
function slotIn(doc) {
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_COMMENT);
  let start = null;
  while (walker.nextNode()) {
    const c = walker.currentNode;
    if (c.data === SLOT[0]) start = c;
    else if (c.data === SLOT[1] && start) return { start, end: c };
  }
  return null;
}

/** Whether `node` is between the slot's markers. */
function inSlot(node, slot) {
  return (
    !!(slot.start.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) &&
    !!(slot.end.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_PRECEDING)
  );
}

/** The layout's top-level regions in `doc`, outside the slot, by part id:
 * each start marker, `<!--pw:sN-->`, with its end. A region inside another
 * is the other's. */
function layoutRegions(doc, slot, layout) {
  const out = new Map();
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_COMMENT);
  let inside = null;
  while (walker.nextNode()) {
    const c = walker.currentNode;
    if (inside) {
      if (c.data === `pw:e${inside.id}`) {
        inside.end = c;
        out.set(inside.id, inside);
        inside = null;
      }
      continue;
    }
    const m = /^pw:s(\d+)$/.exec(c.data);
    if (!m || Number(m[1]) >= layout.parts || inSlot(c, slot)) continue;
    inside = { id: m[1], start: c, end: null };
  }
  return out;
}

/** The nodes between two markers, as markup. */
function markupBetween(start, end) {
  let html = "";
  for (let n = start.nextSibling; n && n !== end; n = n.nextSibling) {
    html += n.nodeType === 1 ? n.outerHTML : n.nodeType === 3 ? n.data : `<!--${n.data}-->`;
  }
  return html;
}

/** Show `next`, a fetched document of the same layout, in this one: its
 * slot's markup in place of this one's, each of the layout's regions it
 * renders differently taken from it, and each of the layout's elements'
 * attributes, its head's title, what it states of itself and its style, its
 * parts manifest, and the regions its response filled after its body. */
function showNext(next, nextParts, here, there) {
  const layout = parts.layout;
  // The page: this one's markup out, the next one's in.
  for (let n = here.start.nextSibling; n && n !== here.end; ) {
    const after = n.nextSibling;
    n.remove();
    n = after;
  }
  const page = document.createDocumentFragment();
  for (let n = there.start.nextSibling; n && n !== there.end; n = n.nextSibling) {
    page.append(document.importNode(n, true));
  }
  here.end.before(page);
  // The layout's regions.
  const mine = layoutRegions(document, here, layout);
  for (const [id, theirs] of layoutRegions(next, there, layout)) {
    const own = mine.get(id);
    if (!own?.end || !theirs.end) continue;
    if (markupBetween(own.start, own.end) === markupBetween(theirs.start, theirs.end)) continue;
    for (let n = own.start.nextSibling; n && n !== own.end; ) {
      const after = n.nextSibling;
      n.remove();
      n = after;
    }
    const fresh = document.createDocumentFragment();
    for (let n = theirs.start.nextSibling; n && n !== theirs.end; n = n.nextSibling) {
      fresh.append(document.importNode(n, true));
    }
    own.end.before(fresh);
  }
  // The layout's elements: each attribute as the next document has it,
  // captures included, and none it does not.
  for (const theirs of next.body.querySelectorAll("[data-pw]")) {
    if (Number(theirs.dataset.pw) >= layout.elements || inSlot(theirs, there)) continue;
    const own = [...document.body.querySelectorAll(`[data-pw="${theirs.dataset.pw}"]`)].find(
      (el) => !inSlot(el, here),
    );
    if (!own) continue;
    for (const a of [...own.attributes]) {
      if (!theirs.hasAttribute(a.name)) own.removeAttribute(a.name);
    }
    for (const a of theirs.attributes) {
      if (own.getAttribute(a.name) !== a.value) own.setAttribute(a.name, a.value);
    }
  }
  // Its head: what the page states of itself, every `<meta>` but the host's
  // charset and viewport (ADR-0186), its canonical address and Open Graph's
  // from it (ADR-0311), and its style. Until ADR-0311 only a description
  // was taken, and the last page's other metadata stayed.
  document.title = next.title;
  const stated = 'meta[name]:not([name="viewport" i]), meta[property], link[rel="canonical"], style';
  for (const el of document.head.querySelectorAll(stated)) el.remove();
  for (const el of next.head.querySelectorAll(stated)) document.head.append(document.importNode(el, true));
  // Its manifest, which the next runtime reads.
  document.getElementById("pw-parts").textContent = JSON.stringify(nextParts);
  // What its response filled after its body (ADR-0148), for the next
  // runtime's streams to settle.
  for (const t of next.body.querySelectorAll("template[for]")) {
    document.body.append(document.importNode(t, true));
  }
}

/** **Where the page's content begins**, given focus: its first heading,
 * else its `main`, held focusable without a tab stop. A node that is gone
 * holds no focus, and a reader by ear is told where they are. */
function focusPage(slot) {
  for (let n = slot.start.nextSibling; n && n !== slot.end; n = n.nextSibling) {
    if (n.nodeType !== 1) continue;
    const target = n.matches("h1") ? n : n.querySelector("h1") ?? (n.matches("main") ? n : n.querySelector("main"));
    if (!target) continue;
    if (!target.hasAttribute("tabindex")) target.setAttribute("tabindex", "-1");
    target.focus({ preventScroll: true });
    return true;
  }
  return false;
}

/** The page the address names, loaded whole, as the browser would have;
 * nothing, where the browser is already loading another. */
function loadWhole(url, how) {
  log.push(`navigation to ${url} loaded whole`);
  delete document.documentElement.dataset.pwNavigating;
  if (!window.__pwUnloading) leaveFor(url, how === "pop");
}

/** The page shown again from what the server says now, unless the browser
 * is already loading another: a reload then would stop that load. */
function showAgain() {
  if (!window.__pwUnloading) location.reload();
}

/** The soft navigation in flight, which a reader's next choice abandons. */
let pending = null;

/**
 * **Go to `url`, keeping the layout** where the next page is of this
 * build and this layout; otherwise load it whole. `how`: `push` (a link, or
 * a handler after its commit), `pop` (back or forward to an entry this
 * runtime made). `mine`: the press that asked for it, which does not wait
 * for itself. `chosen`: a reader's own choice, a link or back and forward,
 * which abandons a navigation in flight, as a link followed while a page
 * loads does (Turbo, SvelteKit and React Router alike); a handler's after
 * its commit never does, and is not taken while another is (ADR-0280).
 */
async function softNavigate(url, how, mine, chosen = false) {
  if (!taken(url, chosen)) return;
  pending?.abort();
  const going = new AbortController();
  pending = going;
  navigating = true;
  document.documentElement.dataset.pwNavigating = "1";
  // Not ready from now until the next document's runtime is: what waits for
  // a ready page waits for the page it went to.
  document.documentElement.dataset.pwReady = "0";
  // Each press made before this answered first, so the page is read after
  // its commit (ADR-0280).
  await pressedBefore(mine);
  if (going.signal.aborted) return;
  log.push(`navigating softly to ${url}`);
  let response, text;
  try {
    response = await fetch(url, {
      headers: { accept: "text/html" },
      cache: "no-store",
      credentials: "same-origin",
      signal: going.signal,
    });
    text = await response.text();
  } catch {
    if (going.signal.aborted) return;
    return loadWhole(url, how);
  }
  if (going.signal.aborted || window.__pwUnloading) return;
  pending = null;
  const at = response.url || url;
  // The same build, by the name it says of itself (the build id's ADR).
  if (!response.ok || !servedBuild || response.headers.get("pw-build") !== servedBuild) {
    return loadWhole(at, how);
  }
  const next = new DOMParser().parseFromString(text, "text/html");
  let nextParts = null;
  try {
    nextParts = JSON.parse(next.getElementById("pw-parts")?.textContent ?? "null");
  } catch {}
  const mineLayout = parts.layout;
  const theirs = nextParts?.layout;
  const here = slotIn(document);
  const there = slotIn(next);
  if (
    !theirs ||
    theirs.path !== mineLayout.path ||
    theirs.schema !== mineLayout.schema ||
    !here ||
    !there
  ) {
    return loadWhole(at, how);
  }
  // From here the page is the next one's: a failure shows it whole.
  try {
    if (how === "push") {
      history.replaceState({ ...history.state, pw: true, scroll: scrollY }, "");
      if (at === location.href) history.replaceState({ pw: true, scroll: 0 }, "", at);
      else history.pushState({ pw: true, scroll: 0 }, "", at);
    }
    showNext(next, nextParts, here, there);
    // The reader's place: the top, or the fragment, or where they were on
    // this entry before.
    const hash = new URL(at).hash;
    const target = hash && document.getElementById(decodeURIComponent(hash.slice(1)));
    if (how === "pop") scrollTo(0, history.state?.scroll ?? 0);
    else if (target) target.scrollIntoView();
    else scrollTo(0, 0);
    if (!focusPage(here)) say(document.title);
  } catch (e) {
    log.push(`navigation to ${at} failed in place: ${e}`);
    showAgain();
    return;
  }
  handOver();
}

/** This runtime ends, and one of the next document's begins. */
function handOver() {
  endReads();
  window.__pwSoftNavigations = (window.__pwSoftNavigations ?? 0) + 1;
  delete document.documentElement.dataset.pwNavigating;
  document.documentElement.dataset.pwReady = "0";
  life.abort();
  import(`/pw-runtime.mjs?document=${window.__pwSoftNavigations}`).catch((e) => {
    log.push(`the next runtime did not load: ${e}`);
    showAgain();
  });
}

// The page's scroll is the navigation's to keep, for each entry it makes.
if (parts.layout && "scrollRestoration" in history) history.scrollRestoration = "manual";

// **The browser's own navigation wins.** Once it begins one, an address
// typed, a link it follows itself, a test's `goto`, a soft navigation in
// flight is abandoned, and nothing this page does loads another: a
// `pushState` or a reload then stops the browser's load (Firefox's
// NS_BINDING_ABORTED, found by the accessibility spec's `goto` after a soft
// navigation). The window keeps the mark, whichever runtime is current; a
// page shown again from the back-forward cache clears it.
if (!window.__pwUnloadWatched) {
  window.__pwUnloadWatched = true;
  addEventListener("beforeunload", () => (window.__pwUnloading = true));
  addEventListener("pageshow", () => (window.__pwUnloading = false));
}
addEventListener("beforeunload", () => pending?.abort(), { signal: life.signal });

// **A link to a page of this layout**, followed in place: the primary
// button, no key held, nothing having claimed the click, an `<a href>` to
// this origin, not a fragment of this page, not to be downloaded, opened
// elsewhere, marked external or marked `data-pw-reload` (the consensus of
// Turbo, Astro and SvelteKit, read 2026-10-09). A page with no layout keeps
// nothing, and leaves every link to the browser.
document.addEventListener(
  "click",
  (e) => {
    if (!parts.layout || e.defaultPrevented || e.button !== 0) return;
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (!a || !(a instanceof HTMLAnchorElement)) return;
    if (a.hasAttribute("download") || a.hasAttribute("data-pw-reload")) return;
    if (a.target && a.target !== "_self") return;
    if (a.relList.contains("external")) return;
    const url = new URL(a.href, location.href);
    if (url.origin !== location.origin) return;
    if (url.pathname === location.pathname && url.search === location.search && url.hash) return;
    e.preventDefault();
    softNavigate(url.href, "push", undefined, true);
  },
  { signal: life.signal },
);

// **Back and forward**, between entries a navigation made: read again, as
// any navigation is, never a page kept from before (ADR-0280's rule).
addEventListener(
  "popstate",
  (e) => {
    if (!e.state?.pw || !parts.layout) return;
    softNavigate(location.href, "pop", undefined, true);
  },
  { signal: life.signal },
);

attach().catch((e) => {
  log.push(`boot failed: ${e}`);
  // Deliberately NOT re-thrown into a state where handlers attach anyway. A
  // runtime that failed to boot leaves an inert page, which is a page that
  // works for reading and does nothing surprising.
  document.documentElement.dataset.pwReady = "failed";
});
