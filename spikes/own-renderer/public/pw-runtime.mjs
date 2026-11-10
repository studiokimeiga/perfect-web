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

/** **Whether this runtime talks to a server** (ADR-0413). The
 * packaging builds a second runtime with this false, for a page no reader
 * decides and with no layout, and what only a server's page uses falls away
 * with it: the subscription, commands, speculation, streams, a navigation
 * that keeps the layout. */
const PW_SERVER = true;

/** **What a build's pages that talk to no server use** (the device runtime's
 * ADR), which the packaging reads from their templates, and what none of them
 * uses falls away with these: a value in a URL or a style, a `{#match}`, a view
 * used inside another, a `<dialog>`, a value the device keeps, a signal bound to
 * the address's fragment, an `autofocus`, a list whose rows a plan can make. */
const PW_LINKS = true;
const PW_MATCH = true;
const PW_VIEWS = true;
const PW_DIALOGS = true;
const PW_KEPT = true;
const PW_FRAGMENT = true;
const PW_AUTOFOCUS = true;
const PW_PLANS = true;

/**
 * **This document's life** (ADR-0306). Every listener the
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

/** Each view's template: what the browser's renderer renders an instance in
 * a block with (ADR-0203). Empty where the page has none. */
const TEMPLATES = Object.values(parts.templates ?? {}).map((t) => t.template);

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
        if (DELEGATED && !view) {
          // Nothing asks for what is inside a row here, and a press finds
          // its element at the document.
          const close = `pw:e${id}@${token}`;
          let end = node.nextSibling;
          while (end && !(end.nodeType === Node.COMMENT_NODE && end.data === close)) end = end.nextSibling;
          if (end) {
            walker.currentNode = end;
            continue;
          }
        }
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
// E7V's decision, and its rule: the gate FAILS CLOSED. A handler that attached
// because the decision had not been made would be the bypass this design
// exists to prevent. `pw-resume`, the Rust crate, is its specification, and
// `decision-agreement.test.mjs` holds these lines to that crate's own build.

let decide = null;

/** **The build that served this runtime** (ADR-0300), by its name, from its
 * handler table: empty until the table is read. */
let servedBuild = "";

/** Whether its build warms (ADR-0399): the table's `#warm|1`. */
let buildWarms = false;

/** **Where this page's code is one module** (ADR-0412), `null`
 * where each piece of it is a module of its own. */
let pageBundleAt = null;
let pageBundleLoading = null;

/** Whether this page's code comes from its one module: only where the
 * decision authorised every handler on it, as a refused handler is never
 * fetched (E7-L's order). Asked after the decision, and answered once. */
let bundleUsable = null;
function usesBundle() {
  if (bundleUsable === null) {
    bundleUsable =
      pageBundleAt !== null &&
      [...manifests].every(([template, list]) =>
        list.every((p) => p.kind !== "event" || !p.value || verdictOf(nameOf(template, p), p).attach),
      );
  }
  return bundleUsable && pageBundleAt !== null;
}

/** This page's one module, loaded with the first need of anything in it. A
 * load that fails is not asked for again: each piece is asked for alone, as
 * in a build with no bundle. */
function pageBundle() {
  pageBundleLoading ??= import(delivered(pageBundleAt)).catch((error) => {
    pageBundleAt = null;
    log.push(`the page's module was not loaded: ${error.message ?? error}`);
    throw error;
  });
  return pageBundleLoading;
}

/** A piece of this page's code: from its one module where there is one and
 * this is the piece's first try; else, or where that module fails, alone. */
function pageCode(piece, url, attempt) {
  if (attempt > 0 || !usesBundle()) return import(url);
  return pageBundle().then(
    // A page's bundle is of the build that answered. A document from another
    // build can be authorised a handler this page's bundle does not hold.
    (all) => all[piece] ?? import(url),
    () => import(url),
  );
}

/** The delivery this runtime was asked for at (`?b=`, ADR-0399);
 * none for a development build. */
const DELIVERY = new URL(import.meta.url).searchParams.get("b");

/** A file of the build at this delivery, as the head asked for it. */
function delivered(path) {
  return DELIVERY ? `${path}?b=${encodeURIComponent(DELIVERY)}` : path;
}

/** A module at this runtime's delivery, and under the specifier of its
 * `attempt` after a failed one (ADR-0382): the first is its address alone. */
function attempted(path, attempt) {
  const at = delivered(path);
  return attempt === 0 ? at : `${at}${at.includes("?") ? "&" : "?"}attempt=${attempt}`;
}

/** What this build's table says of its build and this page, which either
 * decision reads: its lines, for the decision to read the rest of. */
function heardTable(table) {
  const lines = table.split("\n");
  servedBuild = lines.find((l) => l.startsWith("#build|"))?.slice("#build|".length) ?? "";
  // Whether the build warms: the table's word, never the document's.
  buildWarms = lines.includes("#warm|1");
  // Where this page's code is one module: the table's word, as a build's is.
  pageBundleAt = lines.find((l) => l.startsWith(`#bundle|${PAGE}|`))?.split("|")[2] ?? null;
  return lines;
}

/** **Whether a handler a document names may attach** (E7V; ADR-0414):
 * `pw-resume`'s answers, from the build's table and the document's manifest.
 * What that crate hashes, a handler's name and what it captures, is compared
 * here as the text it was hashed from, so a handler is known exactly when the
 * crate knows it. Fails closed as that does: a table that cannot be read knows
 * no handler, and each is refused. */
const decider = (lines, page) => {
  const known = new Set(lines.filter((l) => l && l[0] !== "#").map((l) => (l.includes("|") ? l : `${l}|`)));
  // Told nothing of this page, a document's schema and scope are not compared,
  // as no build has spoken for them.
  const told = lines.find((l) => l.startsWith(`#page|${page}|`))?.split("|");
  return function decided(manifest) {
    const [scheme, abi, , handler, capture = "", schema = "", scope = "", captures, construct] = manifest.split("|");
    const code = !captures && capture
      ? 9
      : !/^\+?0*2$/.test(scheme)
        ? 2
        : !/^\+?0*1$/.test(abi)
          ? 3
          : !known.has(`${handler}|${capture}`)
            ? 4
            : told && scope !== "public" && scope !== told[3]
              ? 5
              : told && schema !== told[2]
                ? 6
                : 0;
    const recovery = construct === "private"
      ? "rerender-private-slot"
      : construct === "command"
        ? "retry-interaction"
        : construct === "resource"
          ? "reject-irrecoverable"
          : scope === "public"
            ? "refetch-region"
            : "rerender-private-slot";
    return { attach: code === 0, recovery, code };
  };
};
// --- end of the decision

async function bootDecision() {
  // What this build compiled (ADR-0132), from the server that served this
  // runtime and never from the document: a document cached from another build
  // names handlers this one may not have. Answered by the head's preload
  // (ADR-0399). If the table cannot be read, nothing is known and every
  // handler is refused, which is the closed side.
  const lines = heardTable(await fetch(delivered("/pw-handlers")).then((r) => (r.ok ? r.text() : "")));
  decide = decider(lines, PAGE);
  if (!PW_SERVER) return;
  log.push(`knows ${lines.filter((l) => l && l[0] !== "#").length} handler(s)`);
  // What this build says of this page's documents (ADR-0300), from the same
  // table and never from the document.
  const [, , schema, scope] = lines.find((l) => l.startsWith(`#page|${PAGE}|`))?.split("|") ?? [];
  if (schema !== undefined) log.push(`knows ${PAGE}'s documents: ${schema} ${scope}`);
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
  pageAddress: !PW_SERVER ? null : (route, args) => pageAddress(route, args),
  /** The build that served this runtime (ADR-0300). */
  build: () => servedBuild,
  indexSize: () => index.size,
  /** The bytes of command bodies kept alive and not yet answered, and what
   * they may come to (ADR-0268). A page with no server sends none. */
  keepalive: !PW_SERVER ? null : () => ({ inFlight: keptAlive, budget: keepaliveBudget }),
  /** How long each send of a command waits for its answer (ADR-0381). */
  deadline: !PW_SERVER ? null : () => sendDeadline,
  /** How long a press's module may take to load (ADR-0382). */
  loadDeadline: () => loadDeadline,
  /** How many presses have not finished (ADR-0280's `pressing`). */
  pressing: () => pressing.size,
  /** Each streamed region that settled after boot, and who filled it:
   * `browser` where the platform applied its patch, `runtime` where this
   * did (ADR-0148). */
  settled: [],
};

// The manifest a handler presents, in the fields `pw-resume` reads:
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
  const url = attempted(`/handler/${identity}.mjs`, attempt);
  // No deadline: the load is shared, and each press waits within its own
  // (ADR-0382; the first press's review).
  const pending = pageCode(`h_${identity}`, url, attempt).then((module) => {
    // What a load given up brings late is not used (ADR-0382).
    if (loaded.get(identity) === pending) {
      loaded.set(identity, module);
      log.push(`loaded ${identity}`);
    }
    return module;
  });
  loaded.set(identity, pending);
  window.__pw.fetched = (window.__pw.fetched ?? 0) + 1;
  return pending;
}

/**
 * **A module a press needs, loaded within the deadline** (ADR-0382): its
 * handler's, or the page's speculation module. A load given up fails as a
 * failed load does, and is no longer
 * waited for, so the press after it, which waits for its turn (ADR-0152),
 * is not held by a module that never comes. The import itself cannot be
 * stopped: it is not waited for, and whatever it brings late is not used.
 */
function within(loading, what) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(
      // Marked: a wait given up, not a failed load.
      () => reject(Object.assign(new Error(`${what}: none within ${loadDeadline} ms`), { givenUp: true })),
      loadDeadline,
    );
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
// A page with no server sends no command.
const keepaliveBudget = !PW_SERVER
  ? 0
  : Math.min(64 * 1024, Number(new URLSearchParams(location.search).get("keepalive") ?? 64 * 1024) || 0);

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
const sendDeadline = !PW_SERVER ? 0 : lowered("deadline");
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

/** The page's speculation module, loaded with the first press that needs it.
 *
 * **Within the deadline, and not kept failed** (ADR-0382): until 2026-10-21 a
 * failed load was kept, and every later press that speculates failed with it
 * until the page was reloaded. A load that fails, or is given up, is
 * forgotten, and the next asks under another specifier, as a handler's does
 * (`loadHandler`), so that no failure the browser keeps answers it. */
function speculations() {
  if (!parts.speculation) return Promise.resolve(null);
  if (speculationModule) return speculationModule;
  const url = attempted(parts.speculation, speculationAttempts);
  // No deadline: each press waits within its own (`speculationsWithin`).
  const loading = (speculationLoaded ? Promise.resolve(speculationLoaded) : pageCode("speculation", url, speculationAttempts)).then(
    (module) => {
      // What a load given up brings late is not used (ADR-0382).
      if (speculationModule === loading) speculationLoaded = module;
      return module;
    },
    (error) => {
      forgetSpeculation(loading);
      throw error;
    },
  );
  speculationModule = loading;
  return loading;
}

/** A press's wait for the speculation module, within its own deadline
 * (ADR-0382): given up, it forgets the load. */
async function speculationsWithin() {
  const loading = speculations();
  try {
    return await within(loading, "the speculation module");
  } catch (error) {
    if (error?.givenUp) forgetSpeculation(loading);
    throw error;
  }
}

function forgetSpeculation(loading) {
  if (speculationModule !== loading) return;
  speculationModule = null;
  speculationAttempts += 1;
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
    module = await speculationsWithin();
  } catch (error) {
    log.push(`${component} sent unspeculated: ${error.message ?? error}`);
    return [];
  }
  const ids = [];
  try {
    for (const s of module?.commands?.[component] ?? []) {
      const state = current(module, s.binding);
      if (!state || state.value === undefined) continue;
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
  const module = await speculationsWithin();
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

// --- the browser's renderer ------------------------------------------------
//
// A block a signal decides, a loop's row, an attribute and what a handler
// captures are rendered here, from the template IR the page carries, with
// the server's escaping in each context, its anchors and its refusals. It is
// `runtime/pw-render`, for what a browser asks of it. As WebAssembly it was
// 77 KB as sent, most of it a heap, JSON and a formatter around these few
// hundred lines, and every request went through JSON twice to reach it. Two
// renderers can disagree, so the two are held to each other, byte for byte,
// in every run of the suite (`renderer-agreement.test.mjs`).
//
// A value here is as a page's signals hold it: JSON's forms, an `Int` a
// number or a `BigInt`, a case `{ $case, value }`, and a type that contains
// itself the `{ $graph }` of its nodes (ADR-0194, ADR-0205). `null` is empty
// text, as it is to the server's reader.
const renderer = (() => {
  /** Why a render produced no bytes, named as the server's `Blocked` is. */
  class Blocked extends Error {
    constructor(kind, at) {
      super(`${kind}: ${at}`);
      this.kind = kind;
    }
  }
  const missing = (path) => new Blocked("MissingValue", path);
  const unrepresented = (why) => new Blocked("UnrepresentedConstruct", why);
  const malformed = (why) => new Blocked("Malformed", why);

  /** What a lookup finds where there is nothing, which `null` is not. */
  const NONE = {};

  /** **Each value a row computes**, by the name its item holds it under (the
   * row's computed holes' ADR): given by `rowsComputed` once the page's
   * computed module is loaded. */
  const rowComputes = new Map();

  const isList = Array.isArray;
  const isObject = (v) => typeof v === "object" && v !== null && !isList(v);
  const isVariant = (v) => isObject(v) && typeof v.$case === "string";
  const isRecord = (v) => isObject(v) && typeof v.$case !== "string";
  const isInt = (v) => typeof v === "bigint" || (Number.isInteger(v) && Math.abs(v) < 2 ** 63);

  /** Each `$graph` as the tree it says, read once. */
  const graphs = new WeakMap();

  /** A value as it is read: a `$graph` is its tree. A `$node` outside one
   * is no value. */
  function opened(v) {
    if (!isObject(v)) return v;
    if ("$graph" in v && Object.keys(v).length === 1) {
      if (!graphs.has(v)) graphs.set(v, built(v.$graph));
      return graphs.get(v);
    }
    if ("$node" in v && Object.keys(v).length === 1) throw malformed("a $node outside any $graph");
    return v;
  }

  /** A `$graph`'s tree, from its last node to its first: a `$node` takes a
   * later node, once, and every node is taken but the first. */
  function built(list) {
    if (!isList(list) || list.length === 0) throw malformed("a $graph that is not a list of nodes");
    const nodes = new Array(list.length).fill(NONE);
    const take = (k) => {
      if (!(typeof k === "bigint" || Number.isInteger(k)) || k < 0) throw malformed("a $node that is not an index");
      const i = Number(k);
      if (i >= nodes.length || nodes[i] === NONE) throw malformed("a $node that is no later node");
      const node = nodes[i];
      nodes[i] = NONE;
      return node;
    };
    const read = (j) => {
      if (isList(j)) return j.map(read);
      if (!isObject(j)) return j;
      const keys = Object.keys(j);
      if (keys.length === 1 && keys[0] === "$graph") return built(j.$graph);
      if (keys.length === 1 && keys[0] === "$node") return take(j.$node);
      return Object.fromEntries(keys.map((k) => [k, read(j[k])]));
    };
    for (let at = list.length - 1; at >= 0; at--) nodes[at] = read(list[at]);
    if (nodes.slice(1).some((n) => n !== NONE)) throw malformed("a node no other holds");
    return nodes[0];
  }

  const field = (record, name) => (Object.hasOwn(record, name) ? opened(record[name]) : NONE);

  /** How a value reads in text or attribute position; `null` for a list, a
   * record or a case, which have no text form. */
  function shown(v) {
    switch (typeof v) {
      case "string":
        return v;
      case "boolean":
        return v ? "true" : "false";
      case "bigint":
        return v.toString();
      case "number":
        return Number.isFinite(v) ? String(v) : "";
      default:
        return v === null || v === undefined ? "" : null;
    }
  }

  /** The value as a condition. An `Option` or a `Result` is not one: which
   * case it is decides a `{#match}`. */
  function holds(v) {
    if (isVariant(v)) throw unrepresented("an Option or a Result is taken apart with {#match}");
    switch (typeof v) {
      case "boolean":
        return v;
      case "bigint":
        return v !== 0n;
      case "number":
        return Number.isFinite(v) && v !== 0;
      case "string":
        return v !== "";
      default:
        return isList(v) ? v.length > 0 : v !== null && v !== undefined;
    }
  }

  // --- what a render has -----------------------------------------------------

  /** The values a render has, by path. A name bound inside it goes in front
   * of them, so a row's scope costs one cell. */
  const env = (entries) => ({ page: new Map(entries), names: null, frames: [], elements: 0 });
  const named = (e, name, value) => ({ ...e, names: { name, value, up: e.names } });
  const within = (e, scope, instance) => ({ ...e, frames: [...e.frames, { scope, instance }] });

  function bound(e, key) {
    for (let n = e.names; n; n = n.up) if (n.name === key) return opened(n.value);
    if (!e.page.has(key)) return NONE;
    // What a row read of the page, to say whether its text still holds.
    if (!PW_SERVER && e.reads) e.reads.set(key, e.page.get(key));
    return opened(e.page.get(key));
  }

  /** The value at `path`: a whole path set directly, or the longest name
   * bound and each field after it; at each record, the rest of the path is
   * read whole first, where a host set a value by its path (ADR-0170). */
  function read(e, path) {
    const whole = bound(e, path);
    if (whole !== NONE) return whole;
    const segments = path.split(".");
    for (let cut = segments.length - 1; cut >= 1; cut--) {
      let value = bound(e, segments.slice(0, cut).join("."));
      if (value === NONE) continue;
      for (let rest = cut; rest < segments.length; rest++) {
        if (!isRecord(value)) return NONE;
        const whole = segments.slice(rest).join(".");
        const all = field(value, whole);
        if (all !== NONE) return all;
        // **A value its row computes** (the row's computed holes' ADR), by
        // the name its item holds it under, which a host's first render set:
        // the browser computes each after, in the row's scope, so what it
        // reads of the page is the row's own.
        const computes = rowComputes.get(whole);
        if (computes) return computes(e);
        value = field(value, segments[rest]);
        if (value === NONE) return NONE;
      }
      return value;
    }
    return NONE;
  }

  function need(e, path) {
    const v = read(e, path);
    if (v === NONE) throw missing(path);
    return v;
  }

  function needText(e, path) {
    const s = shown(need(e, path));
    if (s === null) throw missing(path);
    return s;
  }

  /** The value at a capture path: the longest prefix bound, then fields. */
  function readCaptured(e, path) {
    const segments = path.split(".");
    for (let split = segments.length; split >= 1; split--) {
      let v = bound(e, segments.slice(0, split).join("."));
      if (v === NONE) continue;
      for (const name of segments.slice(split)) {
        if (!isRecord(v)) return NONE;
        v = field(v, name);
        if (v === NONE) return NONE;
      }
      return v;
    }
    return NONE;
  }

  /** An element's key: the text at `path` from it, empty where there is none. */
  function keyOf(item, path) {
    let v = item;
    for (const name of path.split(".")) {
      if (name === "") continue;
      if (!isRecord(v)) return "";
      v = field(v, name);
      if (v === NONE) return "";
    }
    return shown(v) ?? "";
  }

  /** **An instance's address in this page.** One per instance and key, for
   * as long as the page is open, and so never the same for two; the server's
   * are a keyed hash, under a key a page does not hold. A row of a list with
   * no list around it is addressed by its list and key, in the characters an
   * anchor may hold, so nothing is kept for a row the page has let go. */
  const tokens = new Map();
  function tokenOf(e, id, key) {
    if (e.frames.length === 0) return `k${id}_${key.replace(/[^A-Za-z0-9-]/g, (c) => `_${c.charCodeAt(0).toString(16)}_`)}`;
    const said = `${e.frames.map((f) => `${f.scope}@${f.instance}`).join("/")}|${id}|${key}`;
    let token = tokens.get(said);
    if (token === undefined) tokens.set(said, (token = `b${tokens.size}`));
    return token;
  }

  // --- each context's escaping (`runtime/pw-render/src/escape.rs`) ---------

  const ENTITY = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;", "`": "&#96;" };
  const entity = (c) => ENTITY[c];
  const escText = (v) => v.replace(/[&<>]/g, entity);
  const escAttribute = (v) => v.replace(/[&"'<>`]/g, entity);
  const escContent = (v) => (/^[\n\r]/.test(v) ? "\n" : "") + escText(v);
  // The escaping of a value in a URL or a style: only a page with one needs it.
  const { escUrlComponent, escUrl, escStyle } = !PW_LINKS ? {} : (() => {
    const lowerAscii = (s) => s.replace(/[A-Z]+/g, (m) => m.toLowerCase());
    const UNSEEN = /[\p{White_Space}\p{Cc}]/gu;
    const DANGEROUS = ["javascript:", "vbscript:", "data:"];
    const utf8 = new TextEncoder();

    /** A value inside a URL whose text the author wrote around it: one
     * component, every UTF-8 byte but RFC 3986's unreserved encoded. */
    function escUrlComponent(v) {
      let out = "";
      for (const b of utf8.encode(v)) {
        const plain = (b > 47 && b < 58) || (b > 64 && b < 91) || (b > 96 && b < 123) || b === 45 || b === 46 || b === 95 || b === 126;
        out += plain ? String.fromCharCode(b) : `%${b.toString(16).toUpperCase().padStart(2, "0")}`;
      }
      return out;
    }

    function digits(text, radix, shape) {
      if (!shape.test(text)) return null;
      let n = 0;
      for (const d of text.replace("+", "")) {
        n = n * radix + parseInt(d, radix);
        if (n > 0xffffffff) return null;
      }
      return n;
    }

    /** What a reference's body says, `""` for what a parser drops, `null` for
     * what this does not read. The text is lowercased already. */
    function referenced(body) {
      let code;
      if (body.startsWith("#x")) code = digits(body.slice(2), 16, /^\+?[0-9a-f]+$/);
      else if (body[0] === "#") code = digits(body.slice(1), 10, /^\+?[0-9]+$/);
      else return body === "colon" ? ":" : body === "amp" ? "&" : null;
      if (code === null || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) return null;
      const c = String.fromCodePoint(code);
      return c.replace(UNSEEN, "");
    }

    function decodeEntities(s) {
      let out = "";
      let rest = s;
      for (let i = rest.indexOf("&"); i >= 0; i = rest.indexOf("&")) {
        out += rest.slice(0, i);
        rest = rest.slice(i);
        // Within twelve characters, as the server's is within twelve bytes:
        // a body holding a wide character is no number, and leaves an `&` at
        // the front of what is compared, wherever its window ends.
        const end = rest.slice(0, 12).indexOf(";");
        if (end < 0) {
          out += "&";
          rest = rest.slice(1);
          continue;
        }
        out += referenced(rest.slice(1, end)) ?? rest.slice(0, end + 1);
        rest = rest.slice(end + 1);
      }
      return out + rest;
    }

    /** A URL-valued attribute: a scheme that executes is replaced, whatever
     * spelling reaches the parser, and then the attribute's rules. */
    function escUrl(v) {
      const decoded = decodeEntities(lowerAscii(v.replace(UNSEEN, "")));
      return DANGEROUS.some((s) => decoded.startsWith(s)) ? "about:blank" : escAttribute(v);
    }

    function escStyle(v) {
      const lower = lowerAscii(v);
      const suspicious =
        lower.includes("expression(") || DANGEROUS.some((s) => lower.includes(`url(${s}`) || lower.includes(`url('${s}`));
      return suspicious ? "" : escAttribute(v);
    }
    return { escUrlComponent, escUrl, escStyle };
  })();

  /** The one place a context becomes an escaping function. */
  function escaped(v, context) {
    switch (context) {
      case "text":
        return escText(v);
      case "attribute":
      case "content":
        return escAttribute(v);
      case "url":
        return escUrl(v);
      case "style":
        return escStyle(v);
      case "raw_html":
        return v;
      default:
        throw unrepresented(`a context the IR does not have: ${context}`);
    }
  }

  // --- writing -------------------------------------------------------------

  /** Markup being written, and each instance inside it, written later: a
   * view that contains itself goes as deep as its data, and this stays no
   * deeper on the stack (ADR-0203). */
  function rendered(others, first) {
    return written_(others, first, false).join("");
  }

  /** The same, its pieces: text, and where `lists`, each keyed list at the top
   * of the block apart from it. */
  function written_(others, first, lists) {
    const opening = (write, whole) => {
      const out = { html: [], later: [], lists: whole && lists };
      let failed = null;
      try {
        write(out);
      } catch (error) {
        if (!(error instanceof Blocked)) throw error;
        failed = error;
      }
      return { out, failed, next: 0, written: 0 };
    };
    const open = [opening(first, true)];
    const doc = [];
    const copy = (top, until) => {
      for (; top.written < until; top.written++) doc.push(top.out.html[top.written]);
    };
    while (open.length > 0) {
      const top = open[open.length - 1];
      const later = top.out.later[top.next++];
      if (later) {
        copy(top, later.at);
        open.push(opening((out) => emit(later.chunks, later.env, others, out)));
        continue;
      }
      open.pop();
      if (top.failed) throw top.failed;
      copy(top, top.out.html.length);
    }
    return doc;
  }

  function emit(chunks, e, others, out) {
    let i = 0;
    while (i < chunks.length) {
      const c = chunks[i];
      if (c.chunk === "static") {
        out.html.push(c.value);
      } else if (c.value.part === "event") {
        // An element's handlers, adjacent in the IR, write one attribute
        // between them (ADR-0138).
        const run = [c.value];
        for (;;) {
          const q = chunks[i + run.length];
          if (q?.chunk !== "dynamic" || q.value.part !== "event" || q.value.owner !== c.value.owner) break;
          run.push(q.value);
        }
        i += run.length;
        const text = capturedText(run, e);
        out.html.push(capturesAttribute(run, e, text));
        // A row's plan sets what the handlers capture as it was before it was
        // escaped: nothing is read back.
        if (!PW_SERVER && out.holes) {
          out.holes.push(out.html[out.html.length - 1]);
          out.raws[out.holes.length - 1] = text;
        }
        continue;
      } else {
        const mark = out.html.length;
        emitPart(c.value, e, others, out);
        if (!PW_SERVER && out.holes) out.holes.push(out.html.slice(mark).join(""));
      }
      i += 1;
    }
  }

  /** The separator the IR wrote for an attribute that turned out absent. */
  function dropSpace(html) {
    let i = html.length - 1;
    while (i >= 0 && html[i] === "") i -= 1;
    if (i >= 0 && html[i].endsWith(" ")) html[i] = html[i].slice(0, -1);
  }

  function emitPart(p, e, others, out) {
    const html = out.html;
    const start = `<!--pw:s${p.id}-->`;
    const end = `<!--pw:e${p.id}-->`;
    switch (p.part) {
      case "blocked":
        throw unrepresented(p.reason);
      // Written into the document's head by its host.
      case "title":
      case "meta":
      case "canonical":
        return;
      case "text":
        // Nothing addresses a text inside a kept row: it is morphed whole.
        if (out.bare) {
          const text = needText(e, p.value);
          if (!PW_SERVER && out.raws) out.raws[out.holes.length] = text;
          html.push(escaped(text, p.context));
        } else html.push(start, escaped(needText(e, p.value), p.context), end);
        return;
      case "attribute":
        if (p.context === "content") {
          html.push(escContent(needText(e, p.value)));
          return;
        }
      // falls through: the attribute's value is `attributeOf`'s.
      case "interpolated_attribute": {
        const [name, value] = attributeOf(p, e);
        // A planned row's attribute, as a plan sets it: its name and its
        // value as written, or none.
        if (!PW_SERVER && out.raws) out.raws[out.holes.length] = value === null ? null : [name, value];
        if (value !== null) html.push(`${name}="${value}"`);
        return;
      }
      case "boolean_attribute": {
        const present = attributeOf(p, e)[1] !== null;
        if (!PW_SERVER && out.raws) out.raws[out.holes.length] = present ? [p.name, ""] : null;
        if (present) html.push(p.name);
        else dropSpace(html);
        return;
      }
      case "event":
        html.push(capturesAttribute([p], e));
        return;
      case "stream":
        // No query's answer is here: a streamed region shows its placeholder.
        if (!p.streamed) throw missing(`the answer of stream ${p.id}`);
        html.push(start, `<?start name="pw-${p.id}">`);
        emit(p.placeholder, e, others, out);
        html.push("<?end>", end);
        return;
      case "conditional": {
        const branch = holds(need(e, p.value)) ? p.then : p.otherwise;
        html.push(start);
        emit(branch, e, others, out);
        html.push(end);
        return;
      }
      case "each":
        emitEach(p, e, others, out, start, end);
        return;
      case "match": {
        const [arm, scoped] = armShown(p.arms, p.value, e);
        html.push(start);
        emit(arm.body, scoped, others, out);
        html.push(end);
        return;
      }
      case "instance":
        emitInstance(p, e, others, out);
        return;
      case "raw_html":
        // No capability is presented to a browser's render.
        throw new Blocked("UnauthorisedRawHtml", p.capability);
      default:
        throw unrepresented(`a part the IR does not have: ${p.part}`);
    }
  }

  function emitEach(p, e, others, out, start, end) {
    const html = out.html;
    const items = need(e, p.collection);
    if (!isList(items)) throw missing(p.collection);
    html.push(start);
    if (!PW_SERVER && out.lists && e.frames.length === 0 && p.key !== null && p.key !== undefined) {
      html.push(rowsKept(p, e, others, items), end);
      return;
    }
    const seen = new Set();
    for (const raw of items) {
      const item = opened(raw);
      const scoped = named(e, p.binding, item);
      if (p.key === null || p.key === undefined) {
        emit(p.body, scoped, others, out);
        continue;
      }
      const key = rowKey(item, p, seen);
      const token = tokenOf(e, p.id, key);
      html.push(`<!--pw:s${p.id}@${token}-->`);
      emit(p.body, within(scoped, p.id, token), others, out);
      html.push(`<!--pw:e${p.id}@${token}-->`);
    }
    html.push(end);
  }

  /** An item's key in its list, and no other item's. */
  function rowKey(item, p, seen) {
    const key = keyOf(item, p.key);
    if (key === "") throw new Blocked("MissingLoopKey", p.key);
    if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);
    seen.add(key);
    return key;
  }

  /** Whether nothing asks for what is inside a row, as `DELEGATED` says. */
  const DELEGATED_ROWS = !PW_SERVER && !PW_VIEWS;

  /** How many rows' text the last block's render wrote. */
  let rowsWritten = 0;

  /** What each keyed list at the top of a block last wrote, by its key: each
   * row's value, what it read of the page, and its text. */
  const kept = new Map();

  /** **The rows of list `p`**, each the one kept where nothing it reads has
   * changed, so a press writes the text of the rows it changed. */
  function rowsKept(p, e, others, items) {
    const was = kept.get(p.id);
    const now = new Map();
    const seen = new Set();
    const rows = items.map((raw) => {
      const item = opened(raw);
      const key = rowKey(item, p, seen);
      const old = was?.get(key);
      const row = old && holdsStill(old, item, e, p) ? old : rowOf(p, e, others, item, key);
      now.set(key, row);
      return row;
    });
    kept.set(p.id, now);
    return { part: p.id, rows, of: PW_PLANS && planned(p) ? p : null };
  }

  function holdsStill(row, item, e, p) {
    if (!same(row.item, item)) return false;
    // What a `{#match}` around the list bound, which a row can read. The
    // names are the template's, the same at every render of the list, so
    // what they hold is compared and not they.
    for (let a = row.names, b = e.names; a || b; a = a.up, b = b.up) {
      if (!a || !b || !same(a.value, b.value)) return false;
    }
    for (const [name, value] of row.reads) if (e.page.get(name) !== value) return false;
    return computedHold(row, item, e, p);
  }

  /** **Whether what a row computes still comes out as it did** (the row's
   * computed holes' ADR): where nothing it read of the page moved, so; else
   * each is computed again, for the row's item, and the row is kept where
   * each is what it showed, what they read noted as it is now. The large
   * list's class reads the selection in every row, and only the rows whose
   * class it changes are written again. */
  function computedHold(row, item, e, p) {
    const computing = row.computing;
    if (!computing || computing.computedValues.length === 0) return true;
    let moved = false;
    for (const [name, value] of computing.computedReads) {
      if (e.page.get(name) !== value) {
        moved = true;
        break;
      }
    }
    if (!moved) return true;
    const reads = new Map();
    const again = { ...e, reads, computedReads: reads, computedValues: null, names: { name: p.binding, value: item, up: e.names } };
    for (const [name, value] of computing.computedValues) {
      if (!same(rowComputes.get(name)(again), value)) return false;
    }
    computing.computedReads = reads;
    return true;
  }

  function rowOf(p, e, others, item, key) {
    rowsWritten += 1;
    const reads = new Map();
    const token = tokenOf(e, p.id, key);
    const at = `${p.id}@${token}`;
    // What its computed values read, and what they came to (the row's
    // computed holes' ADR), where the page's rows compute any.
    const computing = rowComputes.size ? { computedReads: new Map(), computedValues: [] } : null;
    if (PW_PLANS && planned(p)) return plannedRowOf(p, e, others, item, at, token, reads, computing);
    const text = rendered(others, (out) => {
      out.bare = !PW_SERVER && DELEGATED_ROWS;
      out.html.push(`<!--pw:s${at}-->`);
      emit(p.body, within(named({ ...e, reads, ...computing }, p.binding, item), p.id, token), others, out);
      out.html.push(`<!--pw:e${at}-->`);
    });
    return { key: at, text, item, reads, names: e.names, computing };
  }

  /** **A row a plan makes** (`planOf`): its holes, what each sets as it was
   * before it was escaped (`raws`), and its text only when something reads
   * it, a morph or a parse. Nothing in such a row is written later (no view
   * is in it), so it needs none of the writer that waits for one, and its
   * scope is made once. */
  const plannedRowOf = !PW_PLANS ? null : function plannedRowOf(p, e, others, item, at, token, reads, computing) {
    const env = { ...e, reads, ...computing, names: { name: p.binding, value: item, up: e.names }, frames: [...e.frames, { scope: p.id, instance: token }] };
    const out = { html: [`<!--pw:s${at}-->`], later: [], lists: false, bare: true, holes: [], raws: [] };
    emit(p.body, env, others, out);
    out.html.push(`<!--pw:e${at}-->`);
    const html = out.html;
    let text = null;
    return {
      key: at,
      item,
      reads,
      names: e.names,
      computing,
      holes: out.holes,
      raws: out.raws,
      get text() {
        return (text ??= html.join(""));
      },
    };
  };

  /** Each list part whose rows a plan can make, as `planned` found it. */
  const plannable = !PW_PLANS ? null : new WeakMap();

  /** **Whether a list's rows can be made from one parsed row** (`planOf`):
   * nothing inside a row changes its shape, and no anchor inside it is read.
   * A build with no such list holds none of this (an expression, so that
   * the build drops it). */
  const planned = !PW_PLANS ? null : function planned(p) {
    if (!plannable.has(p)) {
      // The packaging holds this too (`HOLES`), to know a build needs plans.
      // fallow-ignore-next-line code-duplication
      const hole = (v) =>
        (v.part === "text" && v.context === "text") ||
        (v.part === "attribute" && v.context !== "content") ||
        v.part === "interpolated_attribute" ||
        v.part === "boolean_attribute" ||
        v.part === "event";
      plannable.set(p, DELEGATED_ROWS && p.body.every((c) => c.chunk === "static" || hole(c.value)));
    }
    return plannable.get(p);
  };

  /** The most elements a page may nest (`NESTED_ELEMENTS`, ADR-0203). */
  const NESTED_ELEMENTS = 500;

  const emitInstance = !PW_VIEWS ? null : function emitInstance(p, e, others, out) {
    const t = others.find((o) => o.path === p.path);
    if (!t) throw new Blocked("UnknownInstance", p.path);
    const root = e.elements + (p.elements ?? 0);
    if (root + (p.deepest ?? 0) > NESTED_ELEMENTS) throw new Blocked("TooDeep", p.path);
    const token = tokenOf(e, p.id, p.path);
    let child = { page: new Map(), names: null, frames: [...e.frames, { scope: p.id, instance: token }], elements: root };
    for (const [param, source] of p.args) child = named(child, param, need(e, source));
    out.html.push(`<!--pw:s${p.id}@${token}-->`);
    out.later.push({ at: out.html.length, chunks: t.chunks, env: child });
    out.html.push(`<!--pw:e${p.id}@${token}-->`);
  };

  /** The arm a `{#match}` shows, and what its body is rendered in. */
  const armShown = !PW_MATCH ? null : function armShown(arms, path, e) {
    const v = need(e, path);
    if (!isVariant(v)) throw unrepresented("a {#match} takes an Option or a Result apart");
    const arm = arms.find((a) => a.case === v.$case);
    if (!arm) throw unrepresented(`no arm renders ${v.$case}`);
    const held = Object.hasOwn(v, "value");
    let scoped = e;
    if (arm.binding !== undefined && arm.binding !== null) {
      if (!held) throw unrepresented(`${v.$case} has no payload to bind`);
      scoped = named(scoped, arm.binding, v.value);
    }
    const fields = arm.fields ?? [];
    if (fields.length > 0) {
      const parts = held ? opened(v.value) : null;
      if (!isList(parts)) throw unrepresented(`${v.$case} has no fields to bind`);
      if (parts.length !== fields.length) throw unrepresented(`${v.$case} has another number of fields`);
      fields.forEach((name, i) => (scoped = named(scoped, name, parts[i])));
    }
    return [arm, scoped];
  };

  /** An attribute part's name and its value as the document writes it,
   * `null` for a boolean attribute that is absent; `null` for no attribute. */
  function attributeOf(p, e) {
    switch (p.part) {
      case "attribute":
        return [p.name, escaped(needText(e, p.value), p.context)];
      case "boolean_attribute":
        return [p.name, holds(need(e, p.value)) ? "" : null];
      case "interpolated_attribute": {
        let value = "";
        for (const s of p.segments) {
          if (s.segment === "static") value += s.value;
          else {
            const v = read(e, s.value);
            const text = v === NONE ? null : shown(v);
            if (text === null) throw missing(s.value);
            if (p.context === "url") value += escUrlComponent(text);
            else if (p.context === "attribute") value += escAttribute(text);
            else throw unrepresented(`a value interpolated into a ${p.context} attribute`);
          }
        }
        return [p.name, value];
      }
      default:
        return null;
    }
  }

  // --- what a handler captures --------------------------------------------

  /** Keys in the order the server's map has them: by code point. */
  function byPoint(a, b) {
    const place = (c) => (c >= 0xd800 && c < 0xe000 ? c + 0x2000 : c >= 0xe000 ? c - 0x800 : c);
    for (let i = 0; i < Math.min(a.length, b.length); i++) {
      const x = a.charCodeAt(i);
      const y = b.charCodeAt(i);
      if (x !== y) return place(x) - place(y);
    }
    return a.length - b.length;
  }

  /** A captured value, a `Map` for an object and each scalar written. */
  function captured(raw) {
    const v = opened(raw);
    if (isList(v)) return v.map(captured);
    if (isVariant(v)) throw unrepresented(`${v.$case} cannot be captured by a handler`);
    if (isObject(v)) {
      // Its fields. What a host computed for a row is the page's to show.
      return new Map(Object.keys(v).filter((k) => !k.includes(".")).map((k) => [k, captured(v[k])]));
    }
    if (typeof v === "boolean") return String(v);
    if (isInt(v)) {
      if (typeof v === "bigint" ? v > 2n ** 53n || v < -(2n ** 53n) : Math.abs(v) > 2 ** 53) {
        throw unrepresented(`${v} is outside 2^53, so a handler would read it as a different number`);
      }
      return v.toString();
    }
    return JSON.stringify(shown(v));
  }

  function written(v) {
    if (isList(v)) return `[${v.map(written).join(",")}]`;
    if (typeof v === "string") return v;
    return `{${[...v.keys()].sort(byPoint).map((k) => `${JSON.stringify(k)}:${written(v.get(k))}`).join(",")}}`;
  }

  /** `{"item": {"id": ..}}` for the path `item.id`. */
  function insertAt(object, path, value) {
    const names = path.split(".");
    const leaf = names.pop();
    let at = object;
    for (const name of names) {
      if (!at.has(name)) at.set(name, new Map());
      at = at.get(name);
      if (!(at instanceof Map)) throw unrepresented("one capture path runs through another's value");
    }
    if (at.has(leaf)) throw unrepresented("one capture path runs through another's value");
    at.set(leaf, value);
  }

  /** What an element's handlers capture, as the document writes it, or
   * `null` where nothing is captured. */
  function captures(parts, e) {
    return escapedCaptures(capturedText(parts, e));
  }

  /** The one place what a handler captures is escaped. */
  const escapedCaptures = (text) => (text === null ? null : escAttribute(text));

  /** What `parts` capture, as text before it is escaped; `null` for nothing. */
  function capturedText(parts, e) {
    const paths = new Map();
    for (const p of parts) {
      if (p.part !== "event") continue;
      const renames = p.renames ?? {};
      for (const path of p.captures ?? []) {
        // `item.id` of a view given `entry` is `entry.id` (ADR-0136).
        const dot = path.indexOf(".");
        const root = dot < 0 ? path : path.slice(0, dot);
        let at = path;
        if (Object.hasOwn(renames, root)) at = dot < 0 ? renames[root] : `${renames[root]}${path.slice(dot)}`;
        if (paths.has(path) && paths.get(path) !== at) {
          throw unrepresented("two handlers on one element capture one name from two places");
        }
        paths.set(path, at);
      }
    }
    if (paths.size === 0) return null;
    const object = new Map();
    for (const path of [...paths.keys()].sort(byPoint)) {
      const at = paths.get(path);
      const v = readCaptured(e, at);
      if (v === NONE) throw missing(at);
      insertAt(object, path, captured(v));
    }
    return written(object);
  }

  function capturesAttribute(run, e, text = capturedText(run, e)) {
    const value = escapedCaptures(text);
    return value === null ? "" : ` data-pw-captures="${value}"`;
  }

  const scalar = (v) => (typeof v === "boolean" ? "b" : isInt(v) ? "i" : "t") + shown(v);

  /** Whether two values are one value, as the server's `==` has it. What two
   * values hold as one is one value, and is not looked into: a list of a
   * thousand rows, each a new record of the same fields, is compared field by
   * field without a pair made for each. */
  function same(a, b) {
    const pairs = [[a, b]];
    while (pairs.length) {
      const [p, q] = pairs.pop();
      const x = opened(p);
      const y = opened(q);
      if (x === y) continue;
      if (isList(x) || isList(y)) {
        if (!isList(x) || !isList(y) || x.length !== y.length) return false;
        for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) pairs.push([x[i], y[i]]);
      } else if (!isObject(x) || !isObject(y)) {
        if (isObject(x) || isObject(y) || scalar(x) !== scalar(y)) return false;
      } else if (isVariant(x) || isVariant(y)) {
        if (x.$case !== y.$case || Object.hasOwn(x, "value") !== Object.hasOwn(y, "value")) return false;
        if (Object.hasOwn(x, "value") && x.value !== y.value) pairs.push([x.value, y.value]);
      } else {
        const keys = Object.keys(x);
        if (keys.length !== Object.keys(y).length) return false;
        for (const k of keys) {
          if (!Object.hasOwn(y, k)) return false;
          if (x[k] !== y[k]) pairs.push([x[k], y[k]]);
        }
      }
    }
    return true;
  }

  // --- what the page asks ---------------------------------------------------

  const asked = {
    env,
    /** **What each row of a list the browser renders computes** (the row's
     * computed holes' ADR): `functions`, the page's computed module's `rows`;
     * `entries`, the manifest's, each the name its item holds the value under
     * and what its function reads, in order, by its path in the row. */
    rowsComputed: (functions, entries) => {
      for (const { name, inputs } of entries) {
        const f = functions[name];
        if (!f) continue;
        // What it reads of the page is its row's computed reads, apart from
        // what the row's markup reads, and what it computes is kept with the
        // row: a change it reads keeps the row where each comes out the same.
        rowComputes.set(name, (e) => {
          const at = e.computedReads ? { ...e, reads: e.computedReads } : e;
          const value = f(...inputs.map((path) => need(at, path)));
          e.computedValues?.push([name, value]);
          return value;
        });
      }
    },
    letGo: () => tokens.clear(),
    /** One part, its anchors included. */
    part: (p, e, others) => rendered(others, (out) => emit([{ chunk: "dynamic", value: p }], e, others, out)),
    /** A block as a page that talks to no server keeps it: what is around
     * each of its keyed lists (`strs`, one more than `lists`), and the rows. */
    block: PW_SERVER
      ? null
      : (p, e, others) => {
          const strs = [""];
          const lists = [];
          rowsWritten = 0;
          const write = (out) => emit([{ chunk: "dynamic", value: p }], e, others, out);
          for (const piece of written_(others, write, true)) {
            if (typeof piece === "string") strs[strs.length - 1] += piece;
            else {
              lists.push(piece);
              strs.push("");
            }
          }
          return { strs, lists, written: rowsWritten };
        },
    same,
    captures,
  };

  // Only a page with a server is sent a list's changes to compare with what
  // it holds, so the runtime of a page that talks to none has none of this.
  if (PW_SERVER) {
    // --- what changed --------------------------------------------------------

    function nested(p) {
      switch (p.part) {
        case "conditional":
          return [p.then, p.otherwise];
        case "each":
          return [p.body];
        case "match":
          return p.arms.map((a) => a.body);
        case "stream":
          return [p.placeholder, p.ready.body, p.failed.body];
        default:
          return [];
      }
    }

    function handlersWithin(chunks, out) {
      for (const c of chunks) {
        if (c.chunk !== "dynamic") continue;
        const p = c.value;
        if (p.part === "event") {
          if (!out.has(p.owner)) out.set(p.owner, []);
          out.get(p.owner).push(p);
        }
        for (const inner of nested(p)) handlersWithin(inner, out);
      }
    }

    /** What changed in one row, part by part; `null` where it is rendered again. */
    function rowChanges(p, was, now, e, others) {
      const scoped = (item) => within(named(e, p.binding, item), p.id, tokenOf(e, p.id, keyOf(item, p.key)));
      const before = scoped(was);
      const after = scoped(now);
      const handlers = new Map();
      handlersWithin(p.body, handlers);
      const out = [];
      return changesWithin(p.body, before, after, handlers, others, out) ? out : null;
    }

    function changesWithin(chunks, before, after, handlers, others, out) {
      for (const c of chunks) {
        if (c.chunk !== "dynamic") continue;
        const p = c.value;
        const differs = () => {
          const one = [c];
          return rendered(others, (o) => emit(one, before, others, o)) !== rendered(others, (o) => emit(one, after, others, o));
        };
        switch (p.part) {
          case "text": {
            const x = shown(need(before, p.value));
            const y = shown(need(after, p.value));
            if (x === null || y === null) {
              if (differs()) return false;
            } else if (x !== y) {
              out.push([p.id, { text: y }]);
            }
            break;
          }
          case "attribute":
          case "boolean_attribute":
          case "interpolated_attribute": {
            const x = attributeOf(p, before);
            const y = attributeOf(p, after);
            if (x[0] !== y[0] || x[1] !== y[1]) out.push([p.id, { attribute: y[0], value: y[1] }]);
            break;
          }
          case "event": {
            const run = handlers.get(p.owner) ?? [];
            if (run[0]?.id !== p.id) break;
            const x = captures(run, before);
            const y = captures(run, after);
            if (x !== y) out.push([p.id, { attribute: "data-pw-captures", value: y }]);
            break;
          }
          case "conditional": {
            const x = holds(need(before, p.value));
            const y = holds(need(after, p.value));
            if (x !== y || !changesWithin(x ? p.then : p.otherwise, before, after, handlers, others, out)) return false;
            break;
          }
          case "match": {
            const [x, was] = armShown(p.arms, p.value, before);
            const [y, now] = armShown(p.arms, p.value, after);
            if (x.case !== y.case || !changesWithin(y.body, was, now, handlers, others, out)) return false;
            break;
          }
          case "each": {
            const x = p.key === null || p.key === undefined ? NONE : read(before, p.collection);
            const y = p.key === null || p.key === undefined ? NONE : read(after, p.collection);
            if (isList(x) && isList(y)) {
              // A list inside a row: its row is rendered again in the browser,
              // and whether it changed is what is asked here.
              if (!same(x, y) && listChanged(p, x, y, after, others)) out.push([p.id, { list: true }]);
            } else if (differs()) {
              return false;
            }
            break;
          }
          default:
            if (differs()) return false;
        }
      }
      return true;
    }

    /** Whether the keyed loop `p` goes from `old` to `now` by any operation,
     * refusing as the server's derivation of them does (`each_changes`). */
    function listChanged(p, old, now, e, others) {
      const seen = new Set();
      for (const raw of now) {
        const key = keyOf(opened(raw), p.key);
        if (key === "") throw new Blocked("MissingLoopKey", p.key);
        if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);
        seen.add(key);
      }
      const token = (v) => tokenOf(e, p.id, keyOf(opened(v), p.key));
      const wanted = now.map(token);
      const current = [];
      let changed = false;
      for (const v of old) {
        const t = token(v);
        if (wanted.includes(t)) current.push([t, v]);
        else changed = true;
      }
      now.forEach((raw, i) => {
        const item = opened(raw);
        const found = current.findIndex(([t]) => t === wanted[i]);
        if (found < 0) {
          eachInstance(p, item, e, others);
          current.splice(i, 0, [wanted[i], item]);
          changed = true;
          return;
        }
        if (found !== i) {
          current.splice(i, 0, current.splice(found, 1)[0]);
          changed = true;
        }
        if (!same(current[i][1], item)) {
          if (rowChanges(p, opened(current[i][1]), item, e, others) === null) eachInstance(p, item, e, others);
          current[i][1] = item;
          changed = true;
        }
      });
      return changed;
    }

    function eachInstance(p, item, e, others) {
      if (p.key === null || p.key === undefined) throw unrepresented("an unkeyed loop has no addressable instance");
      const key = keyOf(item, p.key);
      if (key === "") throw new Blocked("MissingLoopKey", p.key);
      const token = tokenOf(e, p.id, key);
      const scoped = within(named(e, p.binding, item), p.id, token);
      return rendered(others, (out) => {
        out.html.push(`<!--pw:s${p.id}@${token}-->`);
        emit(p.body, scoped, others, out);
        out.html.push(`<!--pw:e${p.id}@${token}-->`);
      });
    }

    /** One instance of a keyed loop, for its item. */
    asked.instance = (p, item, e, others) => {
      if (p.part === "blocked") throw malformed("the part is no loop");
      if (p.part !== "each") throw unrepresented("the part is not a keyed loop");
      return eachInstance(p, opened(item), e, others);
    };
    /** What changed in one instance, `null` where it is rendered again. */
    asked.changes = (p, was, now, e, others) => {
      if (p.part === "blocked") throw malformed("the part is no loop");
      if (p.part !== "each" || p.key === null || p.key === undefined) throw unrepresented("the part is not a keyed loop");
      const found = rowChanges(p, opened(was), opened(now), e, others);
      return found && found.some(([, change]) => change.list) ? null : found;
    };
    /** An attribute's name, and its value as the document writes it. */
    asked.attribute = (p, e) => {
      const found = attributeOf(p, e);
      if (found === null) throw malformed("the part is no attribute");
      return found;
    };
  }

  return asked;
})();
// --- end of the browser's renderer

// --- ADR-0172: what a speculation renders again --------------------------
//
// A speculation reaches every part that reads its value. Text at the top of
// the page is set by the module's own functions (ADR-0122). An attribute, a
// block and a loop's rows are rendered by the browser's renderer, from the
// speculated value.
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

/** The speculation module, once loaded. */
let speculationLoaded = null;

/** A value as the device keeps it: JSON, each `Int` written exactly. A
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
  return renderer.env([[binding, value], ...computed, ...params, ...holds, ...signals]);
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
  const template = parts.regions[region.part];
  const changes = renderer.changes(template, rowOf(region, row.shown), rowOf(region, item), values, TEMPLATES);
  if (changes === null) {
    const html = renderer.instance(template, rowOf(region, item), values, TEMPLATES);
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
      const html = renderer.instance(parts.regions[region.part], rowOf(region, w.item), values, TEMPLATES);
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
      if (!renderer.same(rowOf(region, row.shown), rowOf(region, w.item))) {
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
  if (regions.length === 0) return;
  const values = regionValues(module, binding, value);
  for (const region of regions) {
    const template = parts.regions?.[region.part];
    if (region.kind === "list") {
      showList(region, listAt(value, region.collection), values, held);
    } else if (region.kind === "block") {
      const html = renderer.part(template, values, TEMPLATES);
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
      const written = renderer.captures(template, values);
      setAttributeAt(
        { template: parts.schema, instances: [], part: region.part },
        written === null
          ? { op: "remove_attribute", name: "data-pw-captures" }
          : { op: "set_attribute", name: "data-pw-captures", value: written },
      );
    } else if (region.kind === "attribute") {
      const [name, written] = renderer.attribute(template, values);
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
  if (!speculationLoaded || shownLists.size === 0) return;
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
// range, a block by rendering its template with the browser's renderer.
// Nothing here asks the server for anything.

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
  if (computed.length === 0 && !parts.rowsComputed) return [];
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
  const computed = await computedNow();
  const values = renderer.env([...signals, ...computed]);
  if (PW_SERVER) return renderer.part(parts.blocks[id], values, TEMPLATES);
  const block = renderer.block(parts.blocks[id], values, TEMPLATES);
  block.boxes = boxesIn(id);
  return block;
}

/** Whether block `id` can show a box: an `<input>` its markup writes, or a
 * view it shows may. Read once a block, as its template does not change. */
const boxed = new Map();
function boxesIn(id) {
  if (!boxed.has(id)) boxed.set(id, PW_VIEWS || /<input[\s>/]/i.test(JSON.stringify(parts.blocks[id])));
  return boxed.get(id);
}

/** A block as the text of the document, the rows of each list of `without`
 * left out. */
function textOfBlock(block, without = []) {
  if (PW_SERVER) return block;
  let text = block.strs[0];
  block.lists.forEach((list, i) => {
    text += (without.includes(list) ? "" : list.rows.map((row) => row.text).join("")) + block.strs[i + 1];
  });
  return text;
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
      const close = `pw:e${a.part}@${a.token}`;
      let end = i + 1;
      while (end < nodes.length && !(nodes[end].nodeType === Node.COMMENT_NODE && nodes[end].data === close)) end++;
      if (end < nodes.length) {
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
  if (!PW_DIALOGS) {
    runsGone(old, used);
  } else for (const o of old) {
    if (used.has(o)) continue;
    for (const n of o.nodes ?? [o.node]) {
      if (PW_DIALOGS && n.nodeType === Node.ELEMENT_NODE) {
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
    if (!n.isConnected) {
      // New nodes side by side go in as one.
      let first = i;
      while (first > 0 && !placed[first - 1].isConnected) first--;
      const run = document.createDocumentFragment();
      for (let k = first; k <= i; k++) run.appendChild(placed[k]);
      parent.insertBefore(run, anchor);
      anchor = placed[first];
      i = first;
      continue;
    }
    if (n.parentNode !== parent || n.nextSibling !== anchor) {
      if (n.isConnected && n.parentNode === parent) moveNodes(parent, [n], anchor);
      else parent.insertBefore(n, anchor);
    }
    anchor = n;
  }
}

/** The siblings from `first` to `last` gone. Where only comments and text are
 * left beside them the parent's children are replaced in one step, which every
 * engine does at once; a `Range` over ten thousand rows took WebKit three
 * seconds. */
function spanGone(first, last) {
  const parent = first.parentNode;
  const rest = [];
  for (let n = parent.firstChild; n; n = n.nextSibling) {
    if (n === first) {
      n = last;
      continue;
    }
    if (n.nodeType === Node.ELEMENT_NODE || rest.length === 64) {
      for (let gone = first; ; ) {
        const next = gone.nextSibling;
        gone.remove();
        if (gone === last) return;
        gone = next;
      }
    }
    rest.push(n);
  }
  parent.replaceChildren(...rest);
}

/** Each run of items side by side that a morph does not keep, gone at once:
 * a thousand rows are one step for the browser, not a thousand. */
function runsGone(old, used) {
  for (let i = 0; i < old.length; i++) {
    if (used.has(old[i])) continue;
    let last = i;
    while (last + 1 < old.length && !used.has(old[last + 1])) last++;
    spanGone(old[i].nodes?.[0] ?? old[i].node, old[last].nodes?.at(-1) ?? old[last].node);
    i = last;
  }
}

/** **A block the browser renders again, morphed**: what it rendered kept
 * where the new rendering has it, and the nodes it inserted. */
function morphBlock(id, html) {
  const r = index.get(addressOf([], id));
  // Not shown, or taken away by a block around it earlier in this same pass.
  if (!r?.start?.isConnected) return [];
  const parent = r.start.parentNode;
  const inserted = [];
  if (!PW_SERVER) {
    const before = lastRendered.get(r.start);
    lastRendered.set(r.start, html);
    if (before && rowsMade(r, before, html, inserted)) return inserted;
    // A list that is its own block, rendered for the first time: shown with
    // no rows, its rows are put in by key as any change's are. A document
    // that shows some is morphed whole, once.
    if (!before && html.strs.length === 2 && html.lists.length === 1 && html.lists[0].part === id) {
      if (rowsMade(r, { strs: html.strs, lists: [{ part: id, rows: [] }] }, html, inserted)) return inserted;
    }
    rewritten = true;
    // A list the block did not hold before is put in by its plan, once the
    // block around it is: no row of it is parsed.
    const later = !PW_PLANS ? [] : html.lists.filter((list) => list.part !== id && list.rows.length > 0 && list.of && planOf(list.of) && !before?.lists.some((b) => b.part === list.part));
    morphChildren(parent, between(r.start, r.end), parseInstance(textOfBlock(html, later)), r.end.nextSibling, inserted);
    for (const list of later) {
      const range = rangeOf(r, list.part);
      if (!range || !listMade({ part: list.part, rows: [], nodes: new Map() }, list, inserted, range)) {
        morphChildren(parent, between(r.start, r.end), parseInstance(textOfBlock(html)), r.end.nextSibling, inserted);
        return inserted;
      }
    }
    return inserted;
  }
  morphChildren(parent, between(r.start, r.end), parseInstance(textOfBlock(html)), r.end.nextSibling, inserted);
  return inserted;
}

/** The anchors of list `part` inside block `r`, as the document holds them. */
function rangeOf(r, part) {
  const walker = document.createTreeWalker(r.start.parentNode, NodeFilter.SHOW_COMMENT);
  walker.currentNode = r.start;
  let start = null;
  for (let n = walker.nextNode(); n && n !== r.end; n = walker.nextNode()) {
    if (n.data === `pw:s${part}`) start = n;
    else if (start && n.data === `pw:e${part}` && n.parentNode === start.parentNode) return { start, end: n };
  }
  return null;
}

/** What each block last rendered, on a page that talks to no server, by the
 * anchor that opens it. While that anchor stands, only the block's own renders
 * change what it shows: a block around it renders it the same. One that block
 * took away and showed again has a new anchor, so nothing is kept for it,
 * whatever its signals did while it was gone. */
const lastRendered = new WeakMap();

/** Whether a pass of the signals rewrote a whole block: what the page indexed
 * inside it is gone. */
let rewritten = false;

// --- which rows changed ---

/** For each index of `b`, the index in `a` of the row with its key, or -1. */
function placesOf(a, b) {
  const at = new Map(a.map((row, i) => [row.key, i]));
  return b.map((row) => at.get(row.key) ?? -1);
}

/** The indexes of `places` that need not move: a longest run of them whose
 * values increase. */
function staying(places) {
  const tails = [];
  const before = new Int32Array(places.length);
  places.forEach((value, i) => {
    if (value < 0) return;
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (places[tails[middle]] < value) low = middle + 1;
      else high = middle;
    }
    before[i] = low > 0 ? tails[low - 1] : -1;
    tails[low] = i;
  });
  const out = new Set();
  for (let i = tails.length > 0 ? tails[tails.length - 1] : -1; i >= 0; i = before[i]) out.add(i);
  return out;
}
// --- end of which rows changed

/** Each row of list `part` the document holds, by its key: its two anchors.
 * `null` where one is not closed. */
function rowsShown(part, range) {
  const shown = new Map();
  const opens = `pw:s${part}@`;
  for (let n = range.start.nextSibling; n && n !== range.end; n = n.nextSibling) {
    if (n.nodeType !== Node.COMMENT_NODE || !n.data.startsWith(opens)) continue;
    const key = n.data.slice("pw:s".length);
    const close = `pw:e${key}`;
    let end = n.nextSibling;
    while (end && !(end.nodeType === Node.COMMENT_NODE && end.data === close)) end = end.nextSibling;
    if (!end) return null;
    shown.set(key, [n, end]);
    n = end;
  }
  return shown;
}

/** One row gone. */
function rowGone([start, end]) {
  if (PW_DIALOGS) {
    for (const d of dialogsIn(between(start, end), true)) {
      d.close();
      giveFocusBack(d);
    }
  }
  for (let n = start; ; ) {
    const next = n.nextSibling;
    n.remove();
    if (n === end) return;
    n = next;
  }
}

/** Every row between a list's anchors gone at once. */
function rowsAllGone(range) {
  if (range.start.nextSibling !== range.end) spanGone(range.start.nextSibling, range.end.previousSibling);
}

/** Rows their text says changed, each `[its anchors, its row]`, morphed where
 * they are, parsed as one. */
function rowsMorphed(changed, inserted) {
  if (changed.length === 0) return;
  const parsed = parseInstance(changed.map(([, row]) => row.text).join(""));
  let at = 0;
  for (const [[start, end], row] of changed) {
    const close = `pw:e${row.key}`;
    const from = at;
    while (!(parsed[at].nodeType === Node.COMMENT_NODE && parsed[at].data === close)) at++;
    at++;
    morphChildren(start.parentNode, between(start, end), parsed.slice(from, at), end.nextSibling, inserted);
  }
}

/** The new rows of a list, parsed as one, each noted in `shown`. Their nodes
 * are the fragment's until they are placed. */
function rowsParsed(rows, shown, inserted) {
  const template = document.createElement("template");
  template.innerHTML = rows.map((row) => row.text).join("");
  const content = template.content;
  let n = content.firstChild;
  for (const row of rows) {
    const close = `pw:e${row.key}`;
    const start = n;
    while (!(n.nodeType === Node.COMMENT_NODE && n.data === close)) n = n.nextSibling;
    shown.set(row.key, [start, n]);
    n = n.nextSibling;
  }
  if (PW_DIALOGS) inserted.push(...content.childNodes);
  return content;
}

/** The nodes of one row put before `anchor`, from wherever they are. */
function rowPut(parent, [start, end], anchor) {
  for (let n = start; ; ) {
    const next = n.nextSibling;
    parent.insertBefore(n, anchor);
    if (n === end) return;
    n = next;
  }
}

// --- rows a plan makes ---

/** What each list part's plan is, once made; `null` where none holds. */
const plans = new WeakMap();

/** **One row's plan**: its markup parsed once, an empty text where each text
 * hole is and a mark on the element each attribute hole is on, so a new row is
 * a clone given its values, and a changed one is given the values that
 * changed: no row is parsed. `null` where the parse does not hold each hole
 * once (a hole inside a `<textarea>`'s text is text, and no comment). */
function planOf(p) {
  if (plans.has(p)) return plans.get(p);
  let html = "";
  const kinds = [];
  for (let i = 0; i < p.body.length; i++) {
    const c = p.body[i];
    if (c.chunk === "static") {
      html += c.value;
      continue;
    }
    // An element's handlers write one attribute, as `emit` writes them.
    while (c.value.part === "event" && p.body[i + 1]?.value?.part === "event" && p.body[i + 1].value.owner === c.value.owner) i++;
    html += c.value.part === "text" ? `<!--pwh${kinds.length}-->` : ` data-pwh${kinds.length}=""`;
    kinds.push(c.value.part === "text" ? "text" : c.value.part === "event" ? "event" : "attribute");
  }
  const template = document.createElement("template");
  template.innerHTML = html;
  const found = new Array(kinds.length).fill(null);
  const walker = document.createTreeWalker(template.content, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_COMMENT);
  const comments = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.nodeType === Node.COMMENT_NODE) {
      const m = /^pwh(\d+)$/.exec(n.data);
      if (m) comments.push([Number(m[1]), n]);
      continue;
    }
    for (const name of n.getAttributeNames()) {
      const m = /^data-pwh(\d+)$/.exec(name);
      if (!m) continue;
      n.removeAttribute(name);
      found[Number(m[1])] = found[Number(m[1])] ? false : n;
    }
  }
  for (const [at, comment] of comments) {
    const text = document.createTextNode("");
    comment.replaceWith(text);
    found[at] = found[at] ? false : text;
  }
  const pathOf = (node) => {
    const path = [];
    for (let n = node; n.parentNode; n = n.parentNode) path.unshift([...n.parentNode.childNodes].indexOf(n));
    return path;
  };
  const plan = found.every(Boolean) ? { proto: template.content, kinds, paths: found.map(pathOf) } : null;
  plans.set(p, plan);
  return plan;
}

const ESCAPED = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'", "&#96;": "`" };

/** **What the renderer escaped, as the parser reads it back**: its escapes
 * undone, line breaks as the parser makes them, and a NUL as `nul`. `null`
 * for any escape the renderer does not write. */
function unescaped(text, nul) {
  // Most text holds nothing the renderer escaped and no character the parser
  // changes: it is read back as it is.
  if (!/[&\r\0]/.test(text)) return text;
  if (/&(?!(?:amp|lt|gt|quot|#39|#96);)/.test(text)) return null;
  return text.replace(/&(?:amp|lt|gt|quot|#39|#96);/g, (m) => ESCAPED[m]).replace(/\r\n?/g, "\n").replaceAll("\0", nul);
}

/** An attribute the renderer wrote, `name="value"` or `name`: `[name, value]`,
 * `null` for none, `undefined` for what it does not write. */
function attributeOfPiece(piece) {
  const written = piece.trim();
  if (written === "") return null;
  const m = /^([^\s="'<>`/]+)(?:="([^"]*)")?$/.exec(written);
  if (!m) return undefined;
  const value = m[2] === undefined ? "" : unescaped(m[2], "\ufffd");
  return value === null ? undefined : [m[1], value];
}

/** One hole given what the renderer wrote for it, where it was `was` (`null`
 * for a new row), and for a handler's, what it captures before it was
 * escaped (`raw`). False, with the hole perhaps half set, for what a plan does
 * not take: the row is then morphed. */
function holeSet(kind, node, piece, was, raw, wasRaw) {
  if (kind === "text") {
    // As the parser reads the text the renderer escaped: a return a line, a
    // NUL nothing.
    if (raw !== undefined) {
      node.data = /[\r\0]/.test(raw) ? raw.replace(/\r\n?/g, "\n").replaceAll("\0", "") : raw;
      return true;
    }
    const text = unescaped(piece, "");
    if (text === null) return false;
    node.data = text;
    return true;
  }
  // What a row's handlers capture, as the renderer wrote it before escaping
  // it (`capturesAttribute`). It is JSON's text, so no character in it is one
  // the parser changes in an attribute: no return, no NUL.
  if (kind === "event" && raw !== undefined) {
    if (raw === null) node.removeAttribute("data-pw-captures");
    else node.setAttribute("data-pw-captures", raw);
    return true;
  }
  // An attribute as the renderer wrote it, `[name, value]` or none: its value
  // read back as the parser would, `undefined` for what it does not write.
  const held = (r) => (r === null ? null : ((v) => (v === null ? undefined : [r[0], v]))(unescaped(r[1], "\ufffd")));
  const now = raw !== undefined ? held(raw) : attributeOfPiece(piece);
  const then = was === null ? null : wasRaw !== undefined ? held(wasRaw) : attributeOfPiece(was);
  if (now === undefined || then === undefined) return false;
  if (then && then[0] !== now?.[0]) node.removeAttribute(then[0]);
  if (now) node.setAttribute(now[0], now[1]);
  if (was === null) return true;
  // A field's value follows the rendering where it changed it, as a morph's
  // does. A box's is the block's own pass over its boxes (`rowsMade`).
  if ((then?.[0] === "value" || now?.[0] === "value") && "value" in node && node.tagName !== "TEXTAREA") {
    node.value = now?.[0] === "value" ? now[1] : "";
  }
  return true;
}

/** New rows, each a clone of its plan given its values, noted in `shown` with
 * the node of each hole. `null`, with nothing put in, where one row's values
 * are not a plan's. */
function rowsPlanned(rows, plan, shown, inserted) {
  const content = document.createDocumentFragment();
  for (const row of rows) {
    const clone = plan.proto.cloneNode(true);
    const holes = new Array(plan.paths.length);
    for (let h = 0; h < holes.length; h++) {
      let n = clone;
      for (const i of plan.paths[h]) n = n.childNodes[i];
      holes[h] = n;
      if (!holeSet(plan.kinds[h], n, row.holes[h], null, row.raws?.[h])) return null;
    }
    const start = document.createComment(`pw:s${row.key}`);
    const end = document.createComment(`pw:e${row.key}`);
    content.append(start, clone, end);
    shown.set(row.key, [start, end, holes]);
  }
  if (PW_DIALOGS) inserted.push(...content.childNodes);
  return content;
}

/** Rows changed in place, each `[its nodes, its row, its row before]`: by the
 * plan where the row was made by one and the plan takes what changed, and
 * morphed otherwise, which leaves it no holes to be set by. */
function rowsChanged(changed, plan, inserted) {
  const morphed = changed.filter(([nodes, row, old]) => {
    if (!plan || !nodes[2] || !row.holes || !old.holes) return true;
    for (let h = 0; h < row.holes.length; h++) {
      if (row.holes[h] !== old.holes[h] && !holeSet(plan.kinds[h], nodes[2][h], row.holes[h], old.holes[h], row.raws?.[h], old.raws?.[h])) return true;
    }
    return false;
  });
  for (const [nodes] of morphed) nodes.length = 2;
  rowsMorphed(morphed, inserted);
}

/** **Whether a row's markup differs from the one before it**: by its holes
 * where both were written by them, the rest of a row being its template's,
 * which equal holes make equal; by its text otherwise. A plan's row's text is
 * written only when something asks for it. */
function rowDiffers(old, row) {
  if (!old.holes || !row.holes) return old.text !== row.text;
  for (let h = 0; h < row.holes.length; h++) if (old.holes[h] !== row.holes[h]) return true;
  return false;
}

/** **The rows of one list made what `now` says**, from what `was` shows: only
 * those it changed parsed, only those out of place moved. False, with nothing
 * touched, where the document does not hold what `was` says. */
function listMade(was, now, inserted, range = index.get(addressOf([], now.part))) {
  if (!range?.end?.isConnected) return false;
  const plan = PW_PLANS && now.of ? planOf(now.of) : null;
  let shown = was.nodes;
  if (!shown) {
    shown = rowsShown(now.part, range);
    if (!shown || shown.size !== was.rows.length || was.rows.some((row) => !shown.has(row.key))) return false;
  }
  now.nodes = shown;
  const a = was.rows;
  const b = now.rows;
  let inPlace = a.length === b.length;
  for (let i = 0; inPlace && i < a.length; i++) inPlace = a[i].key === b[i].key;
  if (inPlace) {
    const changed = [];
    for (let i = 0; i < b.length; i++) {
      if (a[i] !== b[i] && rowDiffers(a[i], b[i])) changed.push([shown.get(b[i].key), b[i], a[i]]);
    }
    if (PW_PLANS) rowsChanged(changed, plan, inserted);
    else rowsMorphed(changed, inserted);
    return true;
  }
  const places = placesOf(a, b);
  const stay = new Set(places.filter((p) => p >= 0));
  if (stay.size === 0 && !PW_DIALOGS) {
    rowsAllGone(range);
    shown.clear();
  } else {
    a.forEach((row, i) => {
      if (stay.has(i)) return;
      rowGone(shown.get(row.key));
      shown.delete(row.key);
    });
  }
  const changed = [];
  b.forEach((row, i) => {
    const old = places[i] < 0 ? null : a[places[i]];
    if (old && old !== row && rowDiffers(old, row)) changed.push([shown.get(row.key), row, old]);
  });
  if (PW_PLANS) rowsChanged(changed, plan, inserted);
  else rowsMorphed(changed, inserted);
  const parent = range.end.parentNode;
  const fresh = b.filter((_, i) => places[i] < 0);
  const content =
    fresh.length === 0
      ? null
      : (PW_PLANS && plan && fresh.every((row) => row.holes) && rowsPlanned(fresh, plan, shown, inserted)) || rowsParsed(fresh, shown, inserted);
  // The new rows side by side go in together, as one parsed fragment.
  const first = places.indexOf(-1);
  const together = fresh.length > 0 && places.lastIndexOf(-1) - first + 1 === fresh.length;
  const still = staying(places);
  let anchor = range.end;
  for (let i = b.length - 1; i >= 0; i--) {
    const nodes = shown.get(b[i].key);
    if (places[i] >= 0) {
      if (!still.has(i)) moveNodes(parent, between(nodes[0], nodes[1]), anchor);
    } else if (together) {
      const opens = shown.get(b[first].key)[0];
      parent.insertBefore(content, anchor);
      anchor = opens;
      i = first;
      continue;
    } else {
      rowPut(parent, nodes, anchor);
    }
    anchor = nodes[0];
  }
  return true;
}

/** **A block's rows made what its render says**, where nothing around them
 * changed. False where something else did: the whole block is morphed. */
function rowsMade(block, was, now, inserted) {
  if (was.strs.length !== now.strs.length || was.strs.some((text, i) => text !== now.strs[i])) return false;
  if (was.lists.some((list, i) => list.part !== now.lists[i].part)) return false;
  for (let i = 0; i < now.lists.length; i++) {
    if (!listMade(was.lists[i], now.lists[i], inserted)) return false;
  }
  // A box in a row left alone shows what its rendering says, as morphing the
  // whole block makes it. Only a box that differs from its attribute matches,
  // and only a block that can show one is looked over: a thousand rows of
  // buttons are not.
  if (!now.boxes) return true;
  for (const box of block.start.parentNode.querySelectorAll("input:checked:not(:default),input:not(:checked):default")) {
    const after = block.start.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING;
    const before = block.end.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_PRECEDING;
    if (after && before && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;
  }
  return true;
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
  const url = attempted(parts.computed, computedAttempts);
  computedModule = pageCode("computed", url, computedAttempts).then(
    (module) => {
      log.push(`loaded ${parts.computed}`);
      // What each row of a list the browser renders computes (the row's
      // computed holes' ADR), given to the renderer as the module arrives.
      if (parts.rowsComputed) renderer.rowsComputed(module.rows ?? {}, parts.rowsComputed);
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
  if (renderer.same(signals.get(name), value)) return;
  signals.set(name, value);
  if (source) sources.set(name, source);
  else sources.delete(name);
  dirty.add(name);
  // A value the device keeps is written as it changes (track
  // `device-store`), unless the change is the device's own: read at start,
  // or another tab's.
  if (PW_KEPT && kept && deviceHeld.has(name)) keep(deviceHeld.get(name), value, source);
  flushing ??= Promise.resolve().then(flushSignals);
}

async function flushSignals() {
  const changed = dirty;
  const from = sources;
  dirty = new Set();
  sources = new Map();
  flushing = null;
  let rendered = false;
  rewritten = false;
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
      const block = await renderBlock(live.part);
      inserted.push(...morphBlock(live.part, block));
      rendered = true;
      log.push(`signal ${[...changed].join(",")} -> block ${live.part}`);
      if (!PW_SERVER) log.push(`block ${live.part} wrote ${block.written} of ${block.lists.reduce((n, l) => n + l.rows.length, 0)} rows`);
    }
  }
  if (rendered) {
    if (!DELEGATED || rewritten) {
      buildIndex();
      bindEvents();
    }
    // Shown once what is in it can be pressed (ADR-0141).
    if (PW_DIALOGS) showDialogs(inserted);
  }
  if (PW_SERVER) readAgain(changed);
  window.__pw.signals = Object.fromEntries(signals);
  if (!PW_SERVER) log.push(`${index.size} indexed`);
  // What the tests and a developer read, as far back as is any use: a page
  // left open keeps nothing without end.
  if (log.length > 4000) log.splice(0, 2000);
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
  followFragmentLinks(binding);
}

const MODIFIER_KEYS = ["metaKey", "ctrlKey", "shiftKey", "altKey"];

/** The `<a href>` a primary, unmodified, unclaimed click is on. */
function linkOf(e) {
  const a = plainClick(e) && e.target instanceof Element ? e.target.closest("a[href]") : null;
  return a instanceof HTMLAnchorElement ? a : null;
}

function plainClick(e) {
  return !e.defaultPrevented && e.button === 0 && !MODIFIER_KEYS.some((key) => e[key]);
}

/** **A link to one of the fragment's forms, followed in its press**
 * (ADR-0399), not a task later in `hashchange`: a primary,
 * unmodified, unclaimed click to this page at a form. Back and forward still
 * come as `hashchange`. */
function followFragmentLinks(binding) {
  const forms = new Set(binding.forms.map((f) => f.form));
  document.addEventListener(
    "click",
    (e) => {
      const a = linkOf(e);
      if (!a || a.hasAttribute("download") || (a.target && a.target !== "_self")) return;
      const url = new URL(a.href, location.href);
      if (url.origin !== location.origin || url.pathname !== location.pathname) return;
      if (url.search !== location.search || !url.hash || !forms.has(url.hash.slice(1))) return;
      e.preventDefault();
      if (url.href !== location.href) history.pushState(null, "", url.href);
      followFragment(binding);
    },
    { signal: life.signal },
  );
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
function heardValue(held, text) {
  if (text === null) return { state: "default" };
  const got = decodeKept(text, held.schemas[held.version - 1]);
  return got.error ? { state: "unreadable", why: got.error } : { state: "read", value: got.value };
}

/** **Another tab's write**: shown here, written nowhere. A `clear()`, whose
 * event has no key, is every value read again. */
async function heardKept(e) {
  const store = deviceStore();
  if (!store || e.storageArea !== store) return;
  for (const held of deviceHeld.values()) {
    if (e.key === null || e.key === deviceKey(held, held.version)) {
      // What the device holds now, which may be later than what the event
      // said: it waited for this tab to be idle.
      const got = e.key === null ? await readKept(store, held) : heardValue(held, store.getItem(e.key));
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
    if (PW_FRAGMENT && parts.fragment) followFragment(parts.fragment);
    if (flushing) await flushing;
  } finally {
    document.documentElement.dataset.pwDevice = "read";
    window.__pw.device = { state: { ...deviceState }, writes: deviceWrites };
  }
  addEventListener("storage", heardLater, { signal: life.signal });
}

/** Each key another tab has written since this tab last had a moment. */
const heardWaiting = new Map();

/** How long another tab's write waits before this tab shows it, in ms: a
 * frame or three, too little to see in a tab no one is pressing in. */
const HEARD_AFTER = 50;

/** **Another tab's write waits its turn.** A press in the tab someone is using
 * has a frame to make, and with many tabs of the app open each of them drawing
 * what it heard at once delayed that frame by a third. A short wait lets that
 * frame go first, and shows a burst of writes once. */
function heardLater(e) {
  if (heardWaiting.size === 0) {
    setTimeout(() => {
      const events = [...heardWaiting.values()];
      heardWaiting.clear();
      for (const event of events) void heardKept(event);
    }, HEARD_AFTER);
  }
  heardWaiting.set(e.key, e);
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
  const { listens } = eventOf(part);
  if (!listens) return;
  for (const el of owners) {
    if (claim(el, part.id)) el.addEventListener(listens, (e) => refusedPress(part, el, verdict, e), { signal: life.signal });
  }
}

/** A press of an element whose handler the decision refused. */
function refusedPress(part, el, verdict, e) {
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
    // A frozen page hears no `storage` event, so another tab's writes while
    // it was kept went unheard: everything the device keeps is read again,
    // as after a `clear()` (ADR-0409).
    if (PW_KEPT) void heardKept({ key: null, storageArea: deviceStore() });
    if (PW_SERVER && leaving && parts.listens !== false) {
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
  // A page shown in a layout keeps it (ADR-0306), held to
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

/** **Whether a press is heard at the document**: on a page that talks to no
 * server and shows no view inside itself, an element's `data-pw` says which of
 * the page's handlers it has, so ten thousand rows need no listener each, and
 * nothing need be bound or indexed inside a row. */
const DELEGATED = !PW_SERVER && !PW_VIEWS;

/** The events that reach no parent: heard on their way down, for their target. */
const STAYS_ON_TARGET = new Set(["blur", "close"]);

/** Each of the page's event parts, by the element that has it. */
const handlers = new Map();

function heardAtTheDocument() {
  if (handlers.size > 0) return;
  const heard = new Set();
  for (const part of manifests.get(PAGE)) {
    if (part.kind !== "event") continue;
    const { event, listens } = eventOf(part);
    if (!listens) {
      log.push(`no listener for event ${event} of part ${part.id}`);
      continue;
    }
    // Asked now: a refused handler's code is never fetched.
    verdictOf(nameOf(PAGE, part), part);
    const owner = String(part.owner);
    handlers.set(owner, [...(handlers.get(owner) ?? []), part]);
    if (heard.has(listens)) continue;
    heard.add(listens);
    document.addEventListener(listens, pressHeard, { capture: STAYS_ON_TARGET.has(listens), signal: life.signal });
  }
}

/** Each element on the event's way that has a handler for it, innermost
 * first, up to one that stops it. */
function pressHeard(e) {
  for (const el of STAYS_ON_TARGET.has(e.type) ? [e.target] : e.composedPath()) {
    const owner = el.dataset?.pw;
    if (owner === undefined) continue;
    let stop = false;
    for (const part of handlers.get(owner) ?? []) {
      const { event, listens } = eventOf(part);
      if (listens !== e.type) continue;
      const verdict = verdictOf(nameOf(PAGE, part), part);
      const modifiers = part.modifiers ?? [];
      if (verdict.attach) acted(part, event, modifiers, el, e);
      else refusedPress(part, el, verdict, e);
      stop ||= modifiers.includes("stop");
    }
    if (stop) return;
  }
}

/**
 * Bind each authorised handler to each of its elements not bound yet.
 *
 * At boot, every element the document has. After a block a signal decides
 * is rendered again (ADR-0130), what it made: its old elements are gone with
 * their listeners, and the elements still standing keep theirs.
 */
const bindEvents = DELEGATED
  ? () => heardAtTheDocument()
  : () => {
      for (const [template, list] of manifests) {
        for (const part of list) bindEvent(template, part);
      }
    };

/**
 * **The decision for one handler part**, asked once and kept, by the name
 * its template gives it: whether its handler may attach (and so be fetched),
 * and, refused, why. One function for binding and for warming (the first
 * press's ADR), so the two cannot disagree about what is authorised.
 */
function verdictOf(named, part) {
  if (!verdicts.has(named)) verdicts.set(named, decided(named, part));
  return verdicts.get(named);
}

function decided(named, part) {
  const manifest = manifestFor(part);
  const verdict =
    decide && manifest ? decide(manifest) : { attach: false, recovery: "no-decision" };
  if (!verdict.attach) {
    // With its CODE: only the code tells an unknown handler from a widened
    // privacy scope from a stale document schema. The console says it to a
    // developer, and where it is explained.
    log.push(`refused ${named}: code ${verdict.code} recovery ${verdict.recovery}`);
    console.warn(`Pleris: handler ${named} not attached: ${explained(verdict.code)}, recovery ${verdict.recovery}`);
  }
  return verdict;
}

/** A refusal's code, `PWR04`, and its page at `pleris.dev`, as the compiler's
 * `PW0606` has one (ADR-0400). The number is the decision's. */
function explained(code) {
  const name = `PWR${String(code).padStart(2, "0")}`;
  return code ? `${name}, https://pleris.dev/e/${name}` : "no decision";
}

/** A part of a view's instance is named by its template, and the page's
 * as it always was. */
function nameOf(template, part) {
  return template === PAGE ? `${part.id}` : `${template}:${part.id}`;
}

function eventOf(part) {
  const event = part.event || "press";
  return { event, listens: DOM_EVENTS[event] };
}

/** Bound once: a block a signal renders again binds what it made, and
 * leaves what was bound before alone (ADR-0130). */
function claim(el, id) {
  const parts = bound.get(el) ?? new Set();
  if (parts.has(id)) return false;
  parts.add(id);
  bound.set(el, parts);
  return true;
}

function bindingOf(template, part) {
  if (part.kind !== "event") return null;
  const named = nameOf(template, part);
  // Every INSTANCE of this template position. A template-scoped id names a
  // position in the template; the document has one instance per loop item,
  // and all three Add buttons carry `data-pw="0"`.
  const addresses = addressesFor(`e${part.owner}`, template);
  const owners = addresses.map((a) => index.get(a).element).filter(Boolean);
  if (owners.length > 0) return listenerFor(part, named, addresses, owners);
  // A part in a block not rendered now (ADR-0130) is bound when it is.
  if (!booted) log.push(`no element ${part.owner} for part ${named}`);
  return null;
}

/**
 * The decision, before anything is bound: refused, the element loads no
 * code, and recovers when pressed (`recoverOnPress`).
 *
 * Asked NOW rather than on interaction, deliberately. A refused handler
 * must never load its code: fetching first and checking after would make
 * the refusal a formality the network had already ignored.
 */
function listenerFor(part, named, addresses, owners) {
  const verdict = verdictOf(named, part);
  if (!verdict.attach) {
    recoverOnPress(part, owners, verdict);
    return null;
  }
  // The event the part handles, and what to do before any code loads.
  const { event, listens } = eventOf(part);
  if (!listens) {
    log.push(`no listener for event ${event} of part ${named}`);
    return null;
  }
  return { named, addresses, owners, event, listens, modifiers: part.modifiers ?? [] };
}

/** One event part of `template`, bound to every element it has: the
 * page's, or a view's in each of its instances (ADR-0203). */
function bindEvent(template, part) {
  const binding = bindingOf(template, part);
  if (!binding) return;
  const { event, listens, modifiers } = binding;
  const fresh = binding.owners.filter((el) => claim(el, part.id));
  for (const el of fresh) el.addEventListener(listens, (e) => acted(part, event, modifiers, el, e));
  if (fresh.length > 0) {
    const { named, owners, addresses } = binding;
    log.push(`attached ${named} to ${owners.length} instance(s): ${addresses.join(" ")}`);
  }
}

/** **One press of one element**, from the event to the end of its handler. */
async function acted(part, event, modifiers, el, e) {
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
      // Within this press's own deadline (ADR-0382).
      module = await within(loadHandler(part.value), `the handler ${part.value}`);
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
      command: !PW_SERVER ? null : async (component, args, how = {}) => {
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
      // After a command's answer alone, and such a page sends none.
      navigate: !PW_SERVER ? null : (route, args) => navigate(route, args, done),
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
}

// --- warming: what a press needs, there before it is pressed --------------
//
// ADR-0399, amending E7-L where a build says `#warm|1`: once
// interactive, loaded and idle, by the press's own loaders. A handler the
// decision refused is never fetched. Development builds warm none.

function rendersBlock(live) {
  return live.kind !== "value" && !SET_IN_PLACE.has(live.kind);
}

/** Whether a signal or a speculation renders anything here with the
 * renderer: a block, a row, a region (`flushSignals`, `showRegions`). */
function rendersInBrowser() {
  if (parts.regions && Object.keys(parts.regions).length > 0) return true;
  return (parts.live ?? []).some(rendersBlock);
}

function quietly(loading, what) {
  loading?.catch((error) => log.push(`warming ${what} failed: ${error.message ?? error}`));
}

/** **Warm what this page's presses need**, in the order they need it: the
 * computed and speculation modules, the handlers shown, then the rest. */
function warm() {
  warmComputed();
  warmSpeculation();
  const identities = warmable();
  for (const identity of identities) warmHandler(identity);
  log.push(`warming ${identities.size} handler(s)`);
}

// Each module forgets a failed load itself (ADR-0382), as for a press.
function warmComputed() {
  if (parts.computed && !computedModule) quietly(computedValues(), parts.computed);
}

function warmSpeculation() {
  if (PW_SERVER && parts.speculation && !speculationModule && !speculationLoaded) {
    quietly(speculations(), parts.speculation);
  }
}

/** The handlers the decision authorised, those shown first (a Set keeps
 * the order). */
function warmable() {
  const shown = new Set();
  const later = new Set();
  for (const [template, list] of manifests) {
    for (const part of list.filter((p) => authorised(template, p))) {
      const isShown = DELEGATED ? document.querySelector(`[data-pw="${part.owner}"]`) : addressesFor(`e${part.owner}`, template).length > 0;
      (isShown ? shown : later).add(part.value);
    }
  }
  return new Set([...shown, ...later]);
}

function authorised(template, part) {
  return part.kind === "event" && part.value && verdictOf(nameOf(template, part), part).attach;
}

/** One handler no press has asked for yet, under any specifier. */
function warmHandler(identity) {
  if (loaded.has(identity) || (attempts.get(identity) ?? 0) > 0) return;
  const loading = loadHandler(identity);
  loading.catch((error) => {
    // Forgotten as a press's failed load is, unless a press forgot it.
    if (loaded.get(identity) === loading) {
      loaded.delete(identity);
      attempts.set(identity, (attempts.get(identity) ?? 0) + 1);
    }
    log.push(`warming ${identity} failed: ${error.message ?? error}`);
  });
}

/** **A press's own paths, run once on nothing** (ADR-0399): the morph's on
 * a copy of a block in an inert document. A block holding a `<dialog>`, or
 * larger than `WARM_ELEMENTS`, is passed over. Each step waits for idle; none
 * runs once the runtime has ended. */
async function warmPaths() {
  if (!rendersInBrowser()) return;
  if (!(await idleAndLive())) return;
  const block = warmableBlock();
  if (block) await warmBlock(block);
}

async function idleAndLive() {
  await idleness();
  return !life.signal.aborted;
}

function warmableBlock() {
  for (const live of (parts.live ?? []).filter(rendersBlock)) {
    const block = blockOf(live);
    if (warmsOn(block)) return block;
  }
  return null;
}

function blockOf(live) {
  const at = index.get(addressOf([], live.part));
  return { part: live.part, nodes: at?.start ? between(at.start, at.end) : null };
}

function warmsOn({ nodes }) {
  if (!nodes || (PW_DIALOGS && (dialogsIn(nodes, true).length || dialogsIn(nodes, false).length))) return false;
  const elements = nodes.reduce((n, node) => n + (node.nodeType === 1 ? 1 + node.getElementsByTagName("*").length : 0), 0);
  return elements <= WARM_ELEMENTS;
}

async function warmBlock({ part, nodes }) {
  const html = textOfBlock(await renderBlock(part));
  if (!(await idleAndLive())) return;
  const inert = document.implementation.createHTMLDocument("");
  const copy = inert.createElement("div");
  for (const n of nodes) copy.append(inert.importNode(n, true));
  morphChildren(copy, [...copy.childNodes], parseInstance(html), null, []);
  log.push(`warmed the paths of block ${part}`);
}

/** The most elements a block's paths are warmed on: any block serves. */
const WARM_ELEMENTS = 400;

/** The browser's next idle period (in WebKit, a moment's pause). */
function idleness() {
  return new Promise((resolve) => {
    const later = window.requestIdleCallback ?? ((f) => setTimeout(f, 1));
    later(() => resolve(), { timeout: 1000 });
  });
}

/** Warming, once the page is loaded and the browser idle, where the
 * build that served it asks for it, and while the runtime is live. */
function warmWhenIdle() {
  if (!buildWarms) return;
  const idle = async () => {
    if (!(await idleAndLive())) return;
    warm();
    // Once what they run is loaded: `renderBlock` waits for it.
    warmPaths().catch((e) => log.push(`warming the paths failed: ${e.message ?? e}`));
  };
  if (document.readyState === "complete") idle();
  else addEventListener("load", idle, { once: true, signal: life.signal });
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
  if (PW_SERVER) {
    for (const p of parts.parts ?? []) {
      if (p.kind === "stream" && streamPending(String(p.id))) pendingAtIndex.add(String(p.id));
    }
  }

  await bootDecision();

  // What the device keeps, read, and what it decides shown, before any
  // handler binds (track `device-store`).
  if (PW_KEPT) await readDevice();

  bindEvents();
  booted = true;
  // A dialog the server rendered at a signal's first value is shown as one
  // the browser renders is (ADR-0141).
  for (const live of parts.live ?? []) {
    if (live.kind !== "conditional" && live.kind !== "match") continue;
    const r = index.get(addressOf([], live.part));
    if (PW_DIALOGS && r?.start) showDialogs(between(r.start, r.end));
  }

  performance.mark("pw:activate:end");
  performance.measure("pw:activate", "pw:activate:start", "pw:activate:end");

  // The regions still pending, filled as the response goes on (ADR-0148).
  if (PW_SERVER) watchStreams();

  // A page whose values are its signals alone listens for nothing
  // (ADR-0130): no resource of its changes on the server.
  if (PW_SERVER && parts.listens !== false) subscribe();
  // The signal a page binds to its address's fragment follows it from here.
  if (PW_FRAGMENT && parts.fragment) bindFragment(parts.fragment);
  // What a block shows from here on, with `autofocus`, takes focus.
  if (PW_AUTOFOCUS) followAutofocus();
  window.__pw.signals = Object.fromEntries(signals);
  window.__pw.ready = true;
  document.documentElement.dataset.pwReady = "1";
  // What its presses will need, where its build warms (ADR-0399):
  // after it is interactive, never before.
  warmWhenIdle();
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
  // As the parser reads the attribute the renderer wrote: its escapes undone,
  // a return a line, a NUL U+FFFD. A value with an escape the renderer does
  // not write is none it wrote, and the change is refused.
  const value = unescaped(op.value, "\ufffd");
  if (value === null) return false;
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
  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}${holdLong}`, {
    signal: life.signal,
    priority: SUBSCRIPTION_PRIORITY,
  });
  applyBatch(await response.json());
}

/** **A subscription asks at low priority** (ADR-0399): a
 * command takes the next of HTTP/1.1's six connections ahead of other tabs'
 * held subscriptions. */
const SUBSCRIPTION_PRIORITY = "low";

/**
 * The streaming adapter: one connection, many batches.
 *
 * Newline-delimited JSON, read incrementally. A partial line is held rather
 * than parsed — a batch split across two network chunks is the normal case,
 * not an error, and parsing half of one would apply half a change.
 */
async function streamOnce() {
  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}&mode=stream${holdLong}`, {
    signal: life.signal,
    priority: SUBSCRIPTION_PRIORITY,
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
/** `&hold=long`, where this page's subscription may be held until something
 * happens (ADR-0402); empty otherwise. */
let holdLong = "";

/** Lets the long hold go, where this page holds it or is waiting for it. */
let letHoldGo = null;

/** **Whether a page's subscription may be held long.** A browser opens six
 * connections to an HTTP/1.1 origin, so a page that held one until something
 * happened would, with six tabs open, leave a press none. On HTTP/2 or later
 * every request shares one connection, and each page may; on HTTP/1.1 one tab
 * of the origin may, and the next takes over when it closes or leaves. A page
 * kept in the back-forward cache holds nothing while it is gone, and asks again
 * when it is shown. */
function askToHoldLong() {
  const protocol = performance.getEntriesByType("navigation")[0]?.nextHopProtocol ?? "";
  if (/^h[23]/.test(protocol)) {
    holdLong = "&hold=long";
    return;
  }
  if (letHoldGo || !navigator.locks) return;
  const asked = new AbortController();
  letHoldGo = () => {
    asked.abort();
    letHoldGo = null;
    holdLong = "";
  };
  addEventListener("pagehide", letHoldGo, { once: true, signal: life.signal });
  life.signal.addEventListener("abort", letHoldGo, { once: true });
  navigator.locks
    .request("pw-subscription", { signal: asked.signal }, () => {
      holdLong = "&hold=long";
      return new Promise((resolve) => asked.signal.addEventListener("abort", resolve, { once: true }));
    })
    .catch(() => {});
}

async function subscribe() {
  askToHoldLong();
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
if (PW_SERVER) window.__pwTestApply = applyFrame;
// And a batch, as a transport delivers one: what a page leaving does with
// it is the batch's guard, not a frame's (ADR-0306).
if (PW_SERVER) window.__pwTestBatch = applyBatch;
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

// --- a navigation that keeps the layout (ADR-0306) ---------
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
  // The same build, by the name it says of itself (ADR-0300).
  if (!response.ok || !servedBuild || response.headers.get("pw-build") !== servedBuild) {
    return loadWhole(at, how);
  }
  // And the same delivery, or the next runtime would meet this one's kept
  // Wasm (the first press's review).
  if (response.headers.get("pw-delivery") !== DELIVERY) return loadWhole(at, how);
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
  // What its renderer holds let go: this module stays in the module map
  // (the first press's review), and nothing here renders again.
  renderer.letGo();
  // At this runtime's delivery, where it has one (ADR-0399).
  const next = delivered("/pw-runtime.mjs");
  import(`${next}${next.includes("?") ? "&" : "?"}document=${window.__pwSoftNavigations}`).catch((e) => {
    log.push(`the next runtime did not load: ${e}`);
    showAgain();
  });
}

// The page's scroll is the navigation's to keep, for each entry it makes.
if (PW_SERVER && parts.layout && "scrollRestoration" in history) history.scrollRestoration = "manual";

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
if (PW_SERVER) document.addEventListener(
  "click",
  (e) => {
    const a = parts.layout ? linkOf(e) : null;
    if (!a || a.hasAttribute("download") || a.hasAttribute("data-pw-reload")) return;
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
if (PW_SERVER) addEventListener(
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
