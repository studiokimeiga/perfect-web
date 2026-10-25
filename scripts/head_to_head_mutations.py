#!/usr/bin/env python3
"""Mutation and integrity controls for the head-to-head (ADR-XXXX).

Two kinds, each of which must be caught:

**The mutants** each take one rule or piece out of the instrument, its
judges or its counter: a run count of one accepted, a stack measured
unminified, a metric computed on the wrong build, a stale Pleris build,
shaping on one side only, a host that compresses nothing, an aborted request
counted as a response, a loss reported as none, a gap that passes accepted,
and the rest below. Each names its witness, a test that must fail by an
assertion in a complete run of its suite: the node tests
(`benchmarks/headtohead/test/*.test.mjs`) or the Python ones
(`scripts/tests/test_head_to_head.py`). A run past its bound, a test that
fails by an error, or a test missing from the baseline's inventory kills
nothing.

**The integrity controls** run the real instrument, briefly (one engine,
cold, five runs), on an input made wrong, and it must refuse the run with
the rule that names the wrong: a run count of one; React's build replaced by
an unminified print of itself; Svelte's script one comment longer than
upstream's; TodoMVC in Pleris's source changed after its build. Each input
is restored, whatever happens.

Run from the repository root, after the builds `just e14-head-to-head` makes
(the release host, TodoMVC in Pleris); it records the output.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

import mutation_source

ROOT = pathlib.Path(__file__).resolve().parent.parent
H2H = ROOT / "benchmarks/headtohead"
VALIDATE = H2H / "validate.mjs"
HOST = H2H / "static-host.mjs"
LIB = H2H / "lib.mjs"
REPORT = H2H / "report.mjs"
FEATURE = H2H / "feature-change.mjs"
MUSEUM = H2H / "museum-classes.mjs"
LOC = ROOT / "scripts/head_to_head_loc.py"
CYPRESS = ROOT / "scripts/head_to_head_cypress.py"
BEHAVIOUR = ROOT / "scripts/head_to_head_behaviour.py"

# (what is undone, file, anchor, replacement); each one's witness is in WITNESSES.
MUTANTS = [
    (
        "a cell of one run is accepted",
        VALIDATE,
        "    if (n < minRuns) problems.push(`runs: ${cell} has ${n} run(s), fewer than ${minRuns}`);\n",
        "",
    ),
    ("cells of different run counts are accepted", VALIDATE, "  if (counts.size > 1) {", "  if (false) {"),
    ("a stack measured unminified is accepted", VALIDATE, "    if (expect.minifies && !q.minified) {", "    if (false) {"),
    ("TodoMVC's common files are held to a stack's minifier", VALIDATE, "    if (isCommon(q.url, common)) continue;\n", ""),
    ("an uncompressed answer from a compressing host is accepted", VALIDATE, "    if (!q.encoding) problems.push(", "    if (false) problems.push("),
    ("a metric computed on the wrong build is accepted", VALIDATE, "      if (q.blob !== want.blob) problems.push(", "      if (false) problems.push("),
    (
        "a Pleris build of changed sources is accepted",
        VALIDATE,
        "        else if (sha256(readFile(f.path)) !== f.sha256) problems.push(",
        "        else if (false) problems.push(",
    ),
    ("a framework build older than its sources is accepted", VALIDATE, "        if (mtime(s) > built) problems.push(", "        if (false) problems.push("),
    (
        "shaping on one side only is accepted",
        VALIDATE,
        "    if (r.timings?.documentEnd == null || r.timings.documentEnd < network.latency) {",
        "    if (false) {",
    ),
    ("an interaction in a new document counts", VALIDATE, "    if (!r.sameDocument) problems.push(", "    if (false) problems.push("),
    (
        "a warm run that downloaded again is accepted",
        VALIDATE,
        "    if (again && again.status === 200 && (again.receivedBody ?? 0) >= 0.5 * (biggest.receivedBody ?? biggest.raw)) {",
        "    if (false) {",
    ),
    (
        "the static host compresses nothing",
        HOST,
        '    const encoding = file.encoded.br ? chooseEncoding(req.headers["accept-encoding"]) : null;',
        "    const encoding = null;",
    ),
    ("the static host answers no validator", HOST, '    if (req.headers["if-none-match"] === file.etag) {', "    if (false) {"),
    (
        "the static host keeps every file a year",
        HOST,
        '  return HASHED.test(path.basename(file)) ? "public, max-age=31536000, immutable" : "no-cache";',
        '  return "public, max-age=31536000, immutable";',
    ),
    (
        "the minified verdict passes an unminified file",
        LIB,
        "    return { minified: share < 0.1, rule: MINIFIED_RULE, share: Number(share.toFixed(3)), minifiedBytes: after };",
        "    return { minified: share < 0.9, rule: MINIFIED_RULE, share: Number(share.toFixed(3)), minifiedBytes: after };",
    ),
    (
        "a measurement an engine does not report is counted as zero",
        LIB,
        '  const xs = values.filter((v) => typeof v === "number" && Number.isFinite(v)).sort((a, b) => a - b);',
        "  const xs = values.map((v) => v ?? 0).sort((a, b) => a - b);",
    ),
    ("the stacks are visited in one order", LIB, "    const j = Math.floor(rand() * (i + 1));", "    const j = i;"),
    ("an overlap of spreads is a loss", REPORT, '  if (a.q1 > b.q3) return "loses";', '  if (a.median > b.median) return "loses";'),
    ("an aborted request is counted as a response", REPORT, "    if (q.failed) {\n      out.aborted++;\n      continue;\n    }\n", ""),
    ("a row Pleris loses is not reported a loss", REPORT, "    if (beaten.length) losses.push(", "    if (false) losses.push("),
    ("a hunk's content is read as headers", FEATURE, "    if (oldLeft > 0 || newLeft > 0) {", "    if (false) {"),
    ("the museum's verdict is not read", MUSEUM, "    if (verdict) current.verdict = verdict[1];\n", ""),
    ("a string's comment-like text is read as a comment", LOC, "        if c in quotes:\n            quote = c\n", ""),
    (
        "Rust's test modules are counted",
        LOC,
        '        if not skipping and line.strip() == "#[cfg(test)]" and i + 1 < len(lines) and lines[i + 1].startswith("mod "):',
        "        if False:",
    ),
    (
        "Pleris's markup comments are counted",
        LOC,
        "        stripped = strip_markup(strip_c_like(text, quotes='\"'))",
        "        stripped = strip_c_like(text, quotes='\"')",
    ),
    (
        "a gap the suite skips that passes is accepted",
        CYPRESS,
        '            if state != "pending":\n',
        "            if False:\n",
    ),
    (
        "a Cypress test that fails is accepted",
        CYPRESS,
        '        elif state != "pass":\n            problems.append(f"test {t.n} ({t.title!r}) is {state}: {error}")',
        "        elif False:\n            pass",
    ),
    ("a known gap that passes in an engine is accepted", BEHAVIOUR, '                    if status != "failed":', "                    if False:"),
    (
        "an engine that ran nothing is accepted",
        BEHAVIOUR,
        '            if got is None:\n                problems.append(f"{stack} in {engine}: no test ran")',
        "            if got is None:\n                got = {}",
    ),
]

NODE = "node"
PY = "python"
# The test each mutant must fail, by an assertion: (suite, test name).
WITNESSES = {
    "a cell of one run is accepted": (NODE, "a cell of one run is refused, and so is a cell of fewer runs than the rule asks"),
    "cells of different run counts are accepted": (NODE, "cells of different run counts are refused"),
    "a stack measured unminified is accepted": (NODE, "an unminified script from a stack whose build minifies is refused; Pleris's is reported, not refused"),
    "TodoMVC's common files are held to a stack's minifier": (NODE, "TodoMVC's common files are not held to a stack's minifier"),
    "an uncompressed answer from a compressing host is accepted": (NODE, "an uncompressed answer of 1 KiB or more from a host that compresses is refused"),
    "a metric computed on the wrong build is accepted": (NODE, "a served file that is not the vendored upstream build is refused (a metric computed on the wrong build)"),
    "a Pleris build of changed sources is accepted": (NODE, "a Pleris build whose sources changed since it was built is refused"),
    "a framework build older than its sources is accepted": (NODE, "a framework's build older than one of its sources is refused, and a page that names another build"),
    "shaping on one side only is accepted": (NODE, "a shaped run whose document came faster than the profile's round trip is refused (shaping on one side only)"),
    "an interaction in a new document counts": (NODE, "an interaction that loaded a new document, or did not complete, is refused"),
    "a warm run that downloaded again is accepted": (NODE, "a warm run that downloaded its main script again is refused where its host lets a cache keep it"),
    "the static host compresses nothing": (NODE, "the static host sends Brotli where it is accepted, gzip where only it is, and the bytes as they are otherwise"),
    "the static host answers no validator": (NODE, "the static host answers a matching validator 304, a hashed name immutable and any other no-cache"),
    "the static host keeps every file a year": (NODE, "the static host answers a matching validator 304, a hashed name immutable and any other no-cache"),
    "the minified verdict passes an unminified file": (NODE, "a script a minifier wrote is minified; one with its indentation and comments is not"),
    "a measurement an engine does not report is counted as zero": (NODE, "a summary's median and quartiles are by linear interpolation, and a missing value is counted, not zero"),
    "the stacks are visited in one order": (NODE, "a run's order is shuffled from its seed, the same seed the same order"),
    "an overlap of spreads is a loss": (NODE, "times differ only where their spreads do not overlap"),
    "an aborted request is counted as a response": (NODE, "a phase's bytes count its responses, an aborted request apart, and Chromium's bytes where they arrived"),
    "a row Pleris loses is not reported a loss": (NODE, "a row in which another stack does better is a loss, and one in which Pleris is best of all is a win"),
    "a hunk's content is read as headers": (NODE, "a patch's statistics read each hunk by its counts: a content line that begins like a header is content"),
    "the museum's verdict is not read": (NODE, "the museum's record is read per exhibit: where its bug was shown, its repair, and Pleris's verdict"),
    "a string's comment-like text is read as a comment": (PY, "test_comments_are_not_code_and_strings_are"),
    "Rust's test modules are counted": (PY, "test_rust_block_comments_nest_and_test_modules_are_left_out"),
    "Pleris's markup comments are counted": (PY, "test_pleris_reads_both_comment_forms"),
    "a gap the suite skips that passes is accepted": (PY, "test_a_gap_that_passes_is_a_problem"),
    "a Cypress test that fails is accepted": (PY, "test_a_test_that_fails_is_a_problem"),
    "a known gap that passes in an engine is accepted": (PY, "test_a_gap_that_passes_in_one_engine_is_a_problem"),
    "an engine that ran nothing is accepted": (PY, "test_an_engine_that_ran_nothing_is_a_problem"),
}

# How long one suite run may take before it is a hang: killed, and no kill.
BOUND = 600

TAP = re.compile(r"^(not ok|ok) \d+ - (.*)$")
UNITTEST = re.compile(r"^(test_\w+) \([\w.]+\) \.\.\. (ok|FAIL|ERROR|skipped.*)$")


def bounded(cmd, cwd):
    """(exit status, output), or (None, output) past the bound."""
    child = subprocess.Popen(cmd, cwd=cwd, start_new_session=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = child.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        out, _ = child.communicate()
        return None, out
    except BaseException:
        os.killpg(child.pid, signal.SIGKILL)
        child.communicate()
        raise
    return child.returncode, out


def run_node():
    """{test: None where it passed, else its failure's code}."""
    tests = sorted(str(p.relative_to(H2H)) for p in (H2H / "test").glob("*.test.mjs"))
    status, out = bounded(["node", "--test", "--test-reporter=tap", *tests], H2H)
    if status is None:
        return None, {}
    results, current = {}, None
    for line in out.splitlines():
        top = TAP.match(line)
        if top:
            current = top.group(2).replace("\\#", "#")
            results[current] = None if top.group(1) == "ok" else "failed"
            continue
        code = re.match(r"^  code: '([A-Z_]+)'$", line)
        if code and current is not None and results.get(current) == "failed":
            results[current] = code.group(1)
    return status, results


def run_python():
    """{test: None where it passed, else ERR_ASSERTION or ERROR}."""
    status, out = bounded([sys.executable, "-m", "unittest", "scripts/tests/test_head_to_head.py", "-v"], ROOT)
    if status is None:
        return None, {}
    results = {}
    for line in out.splitlines():
        m = UNITTEST.match(line)
        if m:
            results[m.group(1)] = None if m.group(2) == "ok" else ("ERR_ASSERTION" if m.group(2) == "FAIL" else "ERROR")
    return status, results


SUITES = {NODE: run_node, PY: run_python}


def code_mutants():
    import mutation_baseline  # keeps what each command says, for a red baseline

    baselines = {}
    for suite, run in SUITES.items():
        status, results = run()
        passed = sum(1 for r in results.values() if r is None)
        print(f"baseline ({suite}): {passed} passed, {len(results) - passed} failed", flush=True)
        if status != 0 or not results or any(r is not None for r in results.values()):
            print(f"FAIL: the unmutated {suite} baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return None
        baselines[suite] = results
    missing = sorted(name for suite, name in WITNESSES.values() if name not in baselines[suite])
    if missing:
        print("FAIL: a mutant names a test its suite does not have: " + "; ".join(missing))
        return None
    if set(WITNESSES) != {m[0] for m in MUTANTS}:
        print("FAIL: a mutant has no witness, or a witness no mutant")
        return None

    killed = refused = survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        suite, witness = WITNESSES[what]
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})", flush=True)
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            status, results = SUITES[suite]()
        if status is None or set(results) != set(baselines[suite]):
            verdict = "REFUSED (incomplete run; no kill)"
            refused += 1
        elif results.get(witness) == "ERR_ASSERTION":
            failing = sum(1 for r in results.values() if r is not None)
            verdict = f"KILLED (by an assertion of its test; {failing} of {len(results)} {suite} tests fail)"
            killed += 1
        else:
            print(f"  (its test {witness!r}: {results.get(witness) or 'passed'})", flush=True)
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived", flush=True)
    return refused + survivors


# --- the integrity controls: the real instrument, on an input made wrong ------

PORT = os.environ.get("PORT", "3141")
QUICK = ["node", "benchmarks/headtohead/head-to-head.mjs", "--apps", "todomvc", "--engines", "chromium",
         "--conditions", "cold", "--no-shaped", "--no-load"]
REACT_BUNDLE = H2H / "todomvc/react/dist/app.bundle.js"
SVELTE_SCRIPT = H2H / "todomvc/svelte/dist/assets/index-BxPr2-fK.js"
PLERIS_APP = ROOT / "examples/todomvc/app.pw"


def unminified(text):
    """The script as esbuild prints it unminified: indented, a statement a line."""
    out = subprocess.run(
        ["node", "-e", "const e=require('esbuild');let s='';process.stdin.on('data',d=>s+=d).on('end',()=>process.stdout.write(e.transformSync(s,{minify:false}).code))"],
        cwd=H2H, input=text, capture_output=True, text=True, check=True,
    )
    return out.stdout


CONTROLS = [
    ("a run count of one", None, None, ["--stacks", "react", "--runs", "1"], "fewer than 5"),
    ("React measured on an unminified print of its build", REACT_BUNDLE, unminified, ["--stacks", "react", "--runs", "5"], "is not minified"),
    ("Svelte measured on a build that is not upstream's", SVELTE_SCRIPT, lambda t: t + "/* another build */\n", ["--stacks", "svelte", "--runs", "5"], "is not upstream's"),
    ("Pleris measured on a build older than its source", PLERIS_APP, lambda t: t + "// changed after the build\n", ["--stacks", "pleris", "--runs", "5"], "changed since it was built"),
]


def integrity_controls():
    caught = missed = 0
    for what, path, change, args, refusal in CONTROLS:
        def run():
            return bounded([*QUICK, *args], ROOT)

        if path is None:
            status, out = run()
        else:
            original = path.read_text()
            with mutation_source.changed(path, original, change(original)):
                status, out = run()
        if status == 1 and "REFUSED" in out and refusal in out:
            caught += 1
            print(f"{what}: CAUGHT (refused: {refusal})", flush=True)
        else:
            missed += 1
            tail = "\n".join(out.splitlines()[-8:])
            print(f"{what}: MISSED (exit {status}; no refusal naming {refusal!r})\n{tail}", flush=True)
    print(f"{caught} of {len(CONTROLS)} integrity controls caught; {missed} missed", flush=True)
    return missed


def main():
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    os.environ.setdefault("PORT", PORT)
    bad = code_mutants()
    if bad is None:
        return 1
    missed = integrity_controls()
    return int(bool(bad or missed))


if __name__ == "__main__":
    sys.exit(main())
