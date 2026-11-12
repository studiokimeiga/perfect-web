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
  it cuts where none is, a server's runtime too.

A mutant of what is written must fail `renderer-agreement.test.mjs`, which
holds the page's own code to the general code row by row; of the walk's
reading or the packaging, `optimize-assets.test.mjs`; of the runtime's
comparison, `e2e/rows.spec.mjs` in Chromium on production builds of the
large list and its fixture of presses, as served and built to report.

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
     '    if (names.has(path)) {\n      reaches.page = true;\n      uses.add("pageAt");\n      return `pageAt(e, reads, ${q(path)})`;\n    }\n', ""),
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
     '      if (key === "") throw new Blocked("MissingLoopKey", p.key);\n      if (now.has(key))', "      if (now.has(key))"),
    ("a list's own keeping takes a repeated key", "agreement", OWN,
     '      if (now.has(key)) throw new Blocked("DuplicateLoopKey", key);\n', ""),
    ("a list's own keeping keeps no row", "agreement", OWN,
     "const row = old && own.still(old, item, e, p) ? old : own.make(p, e, others, item, key);", "const row = own.make(p, e, others, item, key);"),
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
    ("a page its server renders says its rows are their own anchors", "packaging", PACKAGING,
     "    if (!onTheDevice(plan)) continue;\n    const page = name.slice(0, -\".json\".length);\n    const template = templates.find((t) => t.path === page);\n    if (!template) return [];\n",
     "    const page = name.slice(0, -\".json\".length);\n    const template = templates.find((t) => t.path === page);\n    if (!template) return [];\n"),
    ("a list whose rows no plan makes is said to be its rows' own anchor", "packaging", PACKAGING,
     "      if (!planned(list)) continue;\n", ""),
    ("a row its own anchor is not said in its build's IR", "packaging", PACKAGING,
     '      list.row_anchor = "element";\n', ""),
    ("what is said in a build's IR is not written", "packaging", PACKAGING,
     "  if (anchored.length > 0) await writeFile(file, `${JSON.stringify(templates)}\\n`);\n", ""),
    ("a list that keeps its rows' comments is not said", "packaging", PACKAGING,
     "        process.stderr.write(`pw packaging: list ${list.id} of ${page} keeps its rows' comments: ${why}\\n`);\n", ""),
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


SUITES = {"agreement": agreement, "packaging": packaging, "rows": rows}


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
