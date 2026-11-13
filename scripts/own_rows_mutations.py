#!/usr/bin/env python3
"""Mutation controls for ADR-0437: a planned list's rows are kept and made by
its page's own code.

The packaging writes, for each list a device's page keeps whose rows a plan
makes, the list's own key, hold and making, and a walk to its holes
(`spikes/own-renderer/own-rows.mjs`), and splices them into the runtime where
it holds a place for them (`optimize-assets.mjs`'s `withOwnRows`). Each
mutant undoes one piece:

- what is written: each path read as `read` reads it, each context's
  escaping, each hole as `plannedRowOf` writes it, the row's key, scope and
  captures, what it computes and notes, and the keeping of its rows;
- what is refused, and said: a list whose row reads a name a block binds;
- the walk: the build's reading of a row's markup, the steps written, and
  the runtime's comparison of them with the browser's parse;
- the packaging: what it drops where every list is the page's own, and what
  it cuts where none is, a server's runtime too;
- the rows found where they were (ADR-0439): each row whose item is the very
  one at its place, or as far from the list's end, found by it, kept where
  nothing its rows read of the page moved, every other by its key; what it
  says it found; and the runtime's placing of the rows by it, as few moved
  as leave the rest in order.

A mutant of what is written must fail `renderer-agreement.test.mjs`, which
holds the page's own code to the general code row by row; of the walk's
reading or the packaging, `optimize-assets.test.mjs`; of the runtime's
comparison, `e2e/rows.spec.mjs` in Chromium on production builds of the
large list and its fixture of presses, as served and built to report; of
which rows its placing leaves where they are, `renderer-contract.test.mjs`.

Run from the repository root; `just e14-device-runtime` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OWN = ROOT / "spikes/own-renderer/own-rows.mjs"
PACKAGING = ROOT / "spikes/own-renderer/optimize-assets.mjs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

# (what, suite, file, anchor, replacement)
MUTANTS = [
    # -- each path, read as `read` reads it
    ("a path the page holds whole is read from no page", "agreement", OWN,
     '    if (names.has(path)) {\n      reaches.page = true;\n      reaches.names.add(path);\n      uses.add("pageAt");\n      return `pageAt(e, reads, ${q(path)})`;\n    }\n', ""),
    ("a value the row computes is asked before the row's own", "agreement", OWN,
     "(v = field(item, ${q(name)})) !== NONE ? v : (f = ownFunctions.get(", "(f = ownFunctions.get("),
    ("a value the row computes, no function given, is none whatever its item holds", "agreement", OWN,
     '        if (segments.length > 2) {\n          uses.add("fieldsOf");\n          fields = `fieldsOf(item, cutsOf(${q(path)}), 1)`;\n        }\n', ""),
    ("what a row computes is not noted with it", "agreement", OWN,
     ", computing.computedValues.push([${q(name)}, v]), v)", ", v)"),
    ("what a row's computed value reads is noted as its markup's", "agreement", OWN,
     "needed(pageAt(e, computing.computedReads, ${q(input)}), ${q(input)})", "needed(pageAt(e, reads, ${q(input)}), ${q(input)})"),
    ("a value of the page a row reads is not noted", "agreement", OWN,
     "    reads.set(name, v);\n    return opened(v);", "    return opened(v);"),
    ("a path's rest is not read whole first", "agreement", OWN,
     "      const all = field(value, tails[rest]);\n      if (all !== NONE) return all;\n", ""),
    # -- each context's escaping, each hole as `plannedRowOf` writes it
    ("an attribute is escaped as text", "agreement", OWN,
     'const ESCAPE = { text: "escText", attribute: "escAttribute",', 'const ESCAPE = { text: "escText", attribute: "escText",'),
    ("an address is escaped as an attribute", "agreement", OWN, 'url: "escUrl",', 'url: "escAttribute",'),
    ("an address interpolated is escaped as an attribute", "agreement", OWN,
     '{ url: "escUrlComponent", attribute: "escAttribute" }', '{ url: "escAttribute", attribute: "escAttribute" }'),
    ("an absent boolean attribute is there", "agreement", OWN,
     'holes.push(`h${h} ? ${q(hole.name)} : ""`);', "holes.push(q(hole.name));"),
    ("a hole with no text is not refused", "agreement", OWN,
     "    if (s === null) throw missing(path);\n    return s;", '    return s ?? "";'),
    ("a hole with no value is not refused", "agreement", OWN,
     "    if (v === NONE) throw missing(path);\n    return v;", "    return v;"),
    ("a handler's hole sets something", "agreement", OWN,
     "      holes.push('\"\"');\n      raws.push(\"undefined\");", "      holes.push('\"\"');\n      raws.push('\"\"');"),
    ("whether a row's handlers capture is not said", "agreement", OWN,
     "capturing: ${capturing}, others", "capturing: false, others"),
    # -- the row's key, scope, captures and what it reads
    ("a row's key is its key alone", "agreement", OWN,
     "return { key: \\`${list.id}@\\${token}\\`, text: null,", "return { key: token, text: null,"),
    ("a key a row does not hold is a key", "agreement", OWN,
     '(isRecord(item) ? (shown(field(item, ${q(key[0])})) ?? "") : "")', '(isRecord(item) ? (shown(field(item, ${q(key[0])})) ?? "none") : "")'),
    ("a row that reads the page shares the map of one that reads nothing", "agreement", OWN,
     '        const reads = ${unread ? "NO_READS" : "new Map()"};', "        const reads = NO_READS;"),
    ("a row's scope does not hold its item", "agreement", OWN,
     "names: { name: row.of.binding, value: row.item, up: row.names }, frames", "names: row.names, frames"),
    ("a row's text is written in no scope of its own", "agreement", OWN,
     '"rowText(row.of, { ...scopeOf(row), computedValues: null }, row.others, row.key)"',
     '"rowText(row.of, { ...row.env, computedValues: null }, row.others, row.key)"'),
    ("a press captures in no scope of the row's", "agreement", OWN,
     '"return capturedText(parts[h], scopeOf(row));"', '"return capturedText(parts[h], row.env);"'),
    ("what a row computes with is not given to the page's own code", "agreement", RUNTIME,
     "        if (PW_OWN) ownFunctions.set(name, f);\n", ""),
    # -- the keeping of its rows
    ("a row that read nothing of the page is kept by its item alone", "agreement", OWN,
     "(old.item === item ? computedHold(old, item, e, p) : holdsStill(old, item, e, p))", "(old.item === item || holdsStill(old, item, e, p))"),
    ("a list's own keeping takes a missing key", "agreement", OWN,
     '        if (key === "") throw new Blocked("MissingLoopKey", p.key);\n', ""),
    ("a list's own keeping takes a key a row it made holds", "agreement", OWN,
     '        if (made !== null && made.has(key)) throw new Blocked("DuplicateLoopKey", key);\n', ""),
    ("a list's own keeping takes a key its last render's row holds twice", "agreement", OWN,
     '          if (taken[j] === 1) throw new Blocked("DuplicateLoopKey", key);\n', ""),
    ("a list's own keeping keeps no row", "agreement", OWN,
     "      if ((clean && o.item === item) || own.still(o, item, e, p)) {\n", "      if (false) {\n"),
    # -- the rows found where they were (ADR-0439): the keeping, what it says
    # it found, what the rows read of the page, and the runtime's placing
    ("a row whose item is the very one is kept though what it reads moved", "agreement", OWN,
     "      if ((clean && o.item === item) || own.still(o, item, e, p)) {\n", "      if (o.item === item || own.still(o, item, e, p)) {\n"),
    ("what a list's rows read of the page is not looked at", "agreement", OWN,
     "    if (clean) for (const name of own.reads) if (!unmoved(e.page.get(name), was.page.get(name))) clean = false;\n", ""),
    ("a row as far from the list's end is not found by its item", "agreement", OWN,
     "        j = i + m - n;\n        o = j >= 0 && j < m && old[j].item === item ? old[j] : undefined;\n", "        o = undefined;\n"),
    ("a row at its place is not found by its item", "agreement", OWN,
     "      let o = i < m ? old[i] : undefined;\n", "      let o;\n"),
    ("a row found by its key is at the place of the one it replaces", "agreement", OWN,
     "          j = old[o.at] === o ? o.at : old.indexOf(o);\n", ""),
    ("a row's place is read as written though a render refused since wrote over it", "agreement", OWN,
     "          j = old[o.at] === o ? o.at : old.indexOf(o);\n", "          j = o.at;\n"),
    ("a key gone is still looked up", "agreement", OWN,
     "          byKey.delete(own.key(old[j].item));\n", ""),
    ("a row gone is not said", "agreement", OWN,
     "          gone.push(j);\n          byKey.delete(own.key(old[j].item));\n", "          byKey.delete(own.key(old[j].item));\n"),
    ("a row made is not looked up by its key after", "agreement", OWN,
     "      if (made !== null) for (const [key, row] of made) byKey.set(key, row);\n", ""),
    ("a list none of whose rows stays keeps the keys of the last", "agreement", OWN,
     "    if (matched === 0) byKey = made ?? new Map();\n", "    if (matched === 0) byKey = was?.byKey ?? made ?? new Map();\n"),
    ("whether the rows that stay are in order is not said", "agreement", OWN,
     "      if (j < last) sorted = false;\n      else last = j;\n", "      last = j;\n"),
    ("a row made again is not said", "agreement", OWN, "      remade.push(i);\n", ""),
    ("the rows looked for by their keys are not counted", "agreement", OWN, "        keyed += 1;\n", ""),
    ("the rows a list found its rows among are its own", "agreement", OWN, "from: old, places,", "from: rows, places,"),
    ("a row found is not counted", "agreement", OWN,
     "      taken[j] = 1;\n      matched += 1;\n", "      taken[j] = 1;\n"),
    ("a row found is not taken", "agreement", OWN,
     "      taken[j] = 1;\n      matched += 1;\n", "      matched += 1;\n"),
    ("what a value its row computes reads of the page is not said", "agreement", OWN,
     "            reaches.names.add(input);\n", ""),
    ("a value of the page a hole reads whole is not said", "agreement", OWN,
     "      reaches.names.add(path);\n", ""),
    ("a value under a name of the page a hole reads is not said", "agreement", OWN,
     "    reaches.names.add(head[0]);\n", ""),
    ("a list says its rows read nothing of the page", "agreement", OWN,
     "      reads: ${pageReads},\n", "      reads: [],\n"),
    ("a list whose handlers capture the page says what its rows read", "packaging", OWN,
     '  const pageReads = reachesPage ? "null" : q([...reaches.names].sort());\n', "  const pageReads = q([...reaches.names].sort());\n"),
    ("a list whose handlers capture the page keeps its rows by their items alone", "agreement", OWN,
     '  const pageReads = reachesPage ? "null" : q([...reaches.names].sort());\n', "  const pageReads = q([...reaches.names].sort());\n"),
    ("a list its own code found is placed by the general code", "rows", RUNTIME,
     "  if (PW_OWN && (!PW_UNOWNED || now.from !== undefined)) return rowsFound(a, b, now.from === a ? now : foundByKeys(a, b), shown, plan, range, inserted);\n", ""),
    ("rows found among others than the document shows are placed as found among them", "rows", RUNTIME,
     "now.from === a ? now : foundByKeys(a, b)", "now"),
    ("a list found by its keys finds none", "contract", RUNTIME,
     "  if (a.length > 0) {\n    const at = new Map();", "  if (false) {\n    const at = new Map();"),
    ("a row found by its key is new", "contract", RUNTIME,
     "      if (j === undefined) continue;\n      places[i] = j;\n", "      if (j === undefined) continue;\n"),
    ("a row found by its key is not taken", "contract", RUNTIME,
     "      places[i] = j;\n      taken[j] = 1;\n", "      places[i] = j;\n"),
    ("a list found by its keys says none gone", "contract", RUNTIME,
     "    if (matched > 0 && matched < a.length) for (let j = 0; j < a.length; j++) if (taken[j] === 0) gone.push(j);\n", ""),
    ("rows found by their keys are in order whatever their places", "contract", RUNTIME,
     "      if (j < last) sorted = false;\n      else last = j;\n      if (b[i] !== a[j]) remade.push(i);\n",
     "      last = j;\n      if (b[i] !== a[j]) remade.push(i);\n"),
    ("a row found by its key is never made again", "contract", RUNTIME,
     "      if (b[i] !== a[j]) remade.push(i);\n", ""),
    ("a build whose every list is its own keeps the general placing", "packaging", PACKAGING,
     '    if (!own.unowned) out = out.replace(GENERAL_LINES, "");\n', ""),
    ("a build with a list of the general code's holds no general placing", "packaging", PACKAGING,
     '    if (!own.unowned) out = out.replace(GENERAL_LINES, "");\n', '    out = out.replace(GENERAL_LINES, "");\n'),
    ("rows gone are left where some stay", "rows", RUNTIME,
     "      rowGone(shown.get(a[j].key));\n      shown.delete(a[j].key);\n", ""),
    ("new rows among rows in order are put in after the first of them", "rows", RUNTIME,
     "    lo = matched === n ? n : places.indexOf(-1);\n", "    lo = matched === n ? n : places.indexOf(-1) + 1;\n"),
    ("new rows among rows in order are put in before the last of them", "rows", RUNTIME,
     "    hi = matched === n ? n : places.lastIndexOf(-1) + 1;\n", "    hi = matched === n ? n : places.lastIndexOf(-1);\n"),
    ("rows gone are left where none stays", "rows", RUNTIME,
     "    if (!PW_DIALOGS) rowsAllGone(range);\n", ""),
    ("a row made again is left as it was", "rows", RUNTIME,
     "    if (rowDiffers(old, b[i])) changed.push([shown.get(b[i].key), b[i], old]);\n", ""),
    ("rows out of their order are left there", "rows", RUNTIME,
     "  const stays = sorted ? null : stayingIn(places, lo, hi);\n", "  const stays = null;\n"),
    ("rows out of their order are all moved", "rows", RUNTIME,
     "    if (places[i] >= 0 && (stays === null || stays[i - lo] === 1)) {\n", "    if (places[i] >= 0 && stays === null) {\n"),
    ("a row moved goes after the row it goes before", "rows", RUNTIME,
     "      rowMoved(parent, between(nodes[0], nodes[1]), anchor);\n      REPORT:", "      rowMoved(parent, between(nodes[0], nodes[1]), anchor.nextSibling);\n      REPORT:"),
    ("a row moved that holds what has focus loses it", "focus", RUNTIME,
     "  if (ATOMIC_MOVE || (active !== null && nodes.some((n) => n.contains(active)))) {\n", "  if (ATOMIC_MOVE) {\n"),
    ("a row moved is put where it goes before the one after it", "focus", RUNTIME,
     "  for (const node of nodes) parent.insertBefore(node, anchor);\n}\n\n/** **What a list's own code would have found",
     "  for (const node of nodes) parent.insertBefore(node, anchor.nextSibling);\n}\n\n/** **What a list's own code would have found"),
    ("new rows go in at the list's end", "rows", RUNTIME,
     "    parent.insertBefore(rowsNew(b.slice(first, i + 1), plan, shown, inserted), anchor);\n",
     "    parent.insertBefore(rowsNew(b.slice(first, i + 1), plan, shown, inserted), range.end);\n"),
    ("a new row at the end is one in its place", "rows", RUNTIME,
     "  while (hi > lo && places[hi - 1] >= lo && places[hi - 1] === hi - 1 + m - n) hi--;\n",
     "  while (hi > lo && places[hi - 1] === hi - 1 + m - n) hi--;\n"),
    ("a new row's place counts among those that stay", "contract", RUNTIME, "    if (value < 0) continue;\n", ""),
    ("a place past the run's last starts it again", "contract", RUNTIME, "    let low = length;\n", "    let low = 0;\n"),
    ("the run kept is its first", "contract", RUNTIME,
     "  for (let i = length > 0 ? tails[length - 1] : -1; i >= 0; i = before[i - lo]) stays[i - lo] = 1;\n",
     "  for (let i = length > 0 ? tails[0] : -1; i >= 0; i = before[i - lo]) stays[i - lo] = 1;\n"),
    ("a row before one that stays is not said to", "contract", RUNTIME,
     "    before[i - lo] = low > 0 ? tails[low - 1] : -1;\n", "    before[i - lo] = -1;\n"),
    # -- what is refused
    ("a list inside a block that binds a name is written", "agreement", OWN,
     "  if (around.length > 0) refuse(`it is inside a block that binds ${around.join(\", \")}`);\n", ""),
    ("a list the packaging does not write is not said", "packaging", PACKAGING,
     "    process.stderr.write(`pw packaging: list ${part} of ${template} is kept by the general code: ${why}\\n`);\n", ""),
    # -- the walk
    ("the walk steps to a sibling past the one it reads", "packaging", OWN,
     'expression = sibling + ".nextSibling".repeat(index - i);', 'expression = sibling + ".nextSibling".repeat(index - i + 1);'),
    ("a void element holds what follows it", "packaging", OWN,
     "      if (!VOID.has(name)) stack.push(node);", "      stack.push(node);"),
    ("what a parser reads as text is read as elements", "packaging", OWN,
     "      if (UNREAD.has(name)) return null;\n", ""),
    ("no list is given a walk", "packaging", OWN,
     "    const paths = rowPaths(list.body);\n", "    const paths = null;\n"),
    ("a walk is taken where the browser's parse is not its reading", "rows", RUNTIME,
     '    plan.walk = own?.paths === plan.paths.join("|") ? own.walk : null;', "    plan.walk = own ? own.walk : null;"),
    ("no plan takes the walk the packaging wrote", "rows", RUNTIME,
     '    plan.walk = own?.paths === plan.paths.join("|") ? own.walk : null;', "    plan.walk = null;"),
    ("a list the page's own code keeps is the general code's", "rows", RUNTIME,
     "    if (PW_OWN && (!PW_UNOWNED || OWN_ROWS?.[p.id])) return ownKept(OWN_ROWS?.[p.id], p, e, others, items);\n", ""),
    # -- the packaging
    ("a build whose every list is its own keeps the general code's making", "packaging", PACKAGING,
     '.replaceAll("PW_UNOWNED", String(!written || own.unowned))', '.replaceAll("PW_UNOWNED", "true")'),
    ("a build that writes no list keeps the lines it has for one", "packaging", PACKAGING,
     '    out = out.replace(OWN_LINES, "");\n', ""),
    ("a server's runtime keeps the lines a page's own code has", "packaging", PACKAGING,
     '  const server = alone(withOwnRows(source, { rows: null }).replace(declared, "")', '  const server = alone(source.replace(declared, "")'),
    # -- a planned row that is one element is its own anchor (ADR-0438): the
    # build's reading of its row, and what the packaging says and cuts
    ("a row with a comment is one element", "packaging", OWN,
     '    if (html.startsWith("<!", at) || html.startsWith("<?", at)) return "it holds a comment";\n', ""),
    ("a stray `</` is text", "packaging", OWN,
     '      if (html.startsWith("</", at)) return "it holds a `</` that ends no element";\n', ""),
    ("text beside a row's element is none", "packaging", OWN,
     '      if (stack.length === 0) return "it holds text beside its element";\n', ""),
    ("a table's row holding text keeps it", "packaging", OWN,
     '      if (root === "tr" && stack.length === 1) return "its `<tr>` holds text, which a parser moves out of it";\n', ""),
    ("an end tag ends what it does not name", "packaging", OWN,
     "      if (stack.at(-1) !== name) return `its \\`</${name}>\\` ends no element it opened`;\n", ""),
    ("what a parser does not keep is kept as written", "packaging", OWN,
     "    if (NOT_KEPT.has(name)) return `it holds a \\`<${name}>\\`, which a parser does not keep as written`;\n", ""),
    ("two elements are one", "packaging", OWN,
     '      if (root !== null) return "it is more than one element";\n', ""),
    ("an element of any name is one a row is its own anchor as", "packaging", OWN,
     "      if (!ENDED_BY.has(name)) return `its element is a \\`<${name}>\\`, which is not one a row is its own anchor as`;\n", ""),
    ("what would end a row's element inside it does not", "packaging", OWN,
     "    } else if (ENDED_BY.get(root).has(name)) {\n", "    } else if (false) {\n"),
    ("a list's item holding one is its own anchor", "packaging", OWN, '  ["li", ["li"]],\n', '  ["li", []],\n'),
    ("a paragraph holding a list's item is its own anchor", "packaging", OWN, '"hr", "li", "listing"', '"hr", "listing"'),
    ("a table's row holds what a parser moves out of it", "packaging", OWN,
     '    } else if (root === "tr" && stack.length === 1 && name !== "td" && name !== "th") {\n', "    } else if (false) {\n"),
    ("an element not ended is one", "packaging", OWN,
     "  if (stack.length > 0) return `its \\`<${stack.at(-1)}>\\` is not ended`;\n", ""),
    ("a row its own anchor is walked from its fragment", "packaging", OWN,
     '    const from = list.row_anchor === "element" ? paths.map((p) => p.slice(1)) : paths;\n', "    const from = paths;\n"),
    ("a walk of no steps declares one", "packaging", OWN,
     "  if (lines.length === 0) return `(c) => [${holes.join(\", \")}]`;\n", ""),
    ("a build whose presses are not heard at its documents says its rows are their own anchors", "packaging", PACKAGING,
     "  if (!uses.PW_PLANS || uses.PW_VIEWS) return [];\n", "  if (!uses.PW_PLANS) return [];\n"),
    ("a page its server renders is read as the device's", "packaging", PACKAGING,
     "    if (!onTheDevice(plan)) continue;\n    const page = name.slice(0, -\".json\".length);\n    const template = templates.find((t) => t.path === page);\n    if (!template) return null;\n",
     "    const page = name.slice(0, -\".json\".length);\n    const template = templates.find((t) => t.path === page);\n    if (!template) return null;\n"),
    ("a list whose rows no plan makes is said to be its rows' own anchor", "packaging", PACKAGING,
     "    if (!planned(list)) continue;\n", ""),
    ("a row its own anchor is not said in its build's IR", "packaging", PACKAGING,
     '    list.row_anchor = "element";\n', ""),
    ("what is said in a build's IR is not written", "packaging", PACKAGING,
     "  if (anchored.length > 0) await writeFile(file, `${JSON.stringify(templates)}\\n`);\n", ""),
    ("a list that keeps its rows' comments is not said", "packaging", PACKAGING,
     "      process.stderr.write(`pw packaging: list ${list.id} of ${page} keeps its rows' comments: ${why}\\n`);\n", ""),
    ("a build none of whose rows is its own anchor keeps their lines", "packaging", PACKAGING,
     '  if (anchored.length === 0) device = device.replace(ELEMENT_LINES, "");\n', ""),
    ("a server's runtime keeps the lines a row its own anchor has", "packaging", PACKAGING,
     '  const rows = server.replace(ELEMENT_LINES, "");\n', "  const rows = server;\n"),
]


def bounded(cmd, **kw):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def node_tests(*files):
    out, code = bounded(["node", "--test", "--test-reporter=spec", *files], cwd=ROOT)
    if code is None:
        return True, 0, 1
    passed = sum(int(n) for n in re.findall(r"(?m)^ℹ pass (\d+)$", out))
    failed = sum(int(n) for n in re.findall(r"(?m)^ℹ fail (\d+)$", out))
    return passed + failed > 0, passed, failed


def agreement():
    """The page's own code held to the general code, and the general code to
    the server's, which the oracle answers for: built from the source."""
    _, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-render-oracle", "--bin", "oracle"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    return node_tests("spikes/own-renderer/renderer-agreement.test.mjs")


def packaging():
    return node_tests("spikes/own-renderer/optimize-assets.test.mjs")


def contract():
    """Which rows the runtime's placing leaves where they are (ADR-0439)."""
    return node_tests("spikes/own-renderer/renderer-contract.test.mjs")


def rows():
    """The large list and its fixture of presses, on production builds: as
    they are served, less the tests that read what the page reports, and
    built to report for those (`@report`, ADR-0430)."""
    # The host the suite serves them with, built where this runs: a slot's
    # target is its own (`mutation_slots.py`).
    _, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    passed = failed = 0
    for report, select in (("0", "--grep-invert"), ("1", "--grep")):
        env = {**os.environ, "PW_PRODUCTION_ASSETS": "1", "PW_REPORT": report}
        for script in ("spikes/own-renderer/rows.sh", "spikes/own-renderer/row-events.sh"):
            _, code = bounded(["bash", script], cwd=ROOT, env=env)
            if code != 0:
                return False, 0, 0
        out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/rows.spec.mjs", select, "@report",
                             "--project=chromium", "--reporter=line"],
                            cwd=ROOT / "spikes/own-renderer", env=env)
        if code is None:
            return True, passed, failed + 1
        out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
        passed += sum(int(n) for n in re.findall(r"(\d+) passed", out))
        failed += sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


def focus():
    """A row moved that holds what has focus, where the engine moves no node
    whole (ADR-0439): the rows suite's test of it in WebKit, which has no
    `moveBefore` (Chromium and Firefox have), on the production builds as
    they are served."""
    _, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1", "PW_REPORT": "0"}
    for script in ("spikes/own-renderer/rows.sh", "spikes/own-renderer/row-events.sh"):
        _, code = bounded(["bash", script], cwd=ROOT, env=env)
        if code != 0:
            return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/rows.spec.mjs", "--grep", "keeps what has focus inside it",
                         "--project=webkit", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer", env=env)
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"agreement": agreement, "packaging": packaging, "contract": contract, "rows": rows, "focus": focus}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = SUITES[suite]()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
    # What was built from a mutant is built again from the restored source.
    rows()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
