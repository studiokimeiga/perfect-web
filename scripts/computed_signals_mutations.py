#!/usr/bin/env python3
"""Mutation controls for ADR-0227: a value computed from a signal is the
browser's, and its first value the host's.

Each mutant undoes one piece: the signal read as one, the component each
computed part names, the path a view's field is read at; the page's module,
each function by its part and given the signal's value decoded, written and
refused; the host's first value, read through a field, the module named and
served only where a plan names it; and the runtime computing each part again,
with the signal's value now, an attribute's included.

The compiler's and the server's tests must fail for a mutant of the compiler
or the server ("cargo"). A mutant of the runtime must fail
`e2e/feed.spec.mjs` in three engines, against a build of the mutated source
("browser"): the store's and the feed's pages, and the server.

Run from the repository root; `just e14-computed-signals` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VALUES = ROOT / "compiler/pw-core/src/page_values.rs"
MODULE = ROOT / "compiler/pw-core/src/backend/computed.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a signal's value is a query's to compute",
        "cargo",
        VALUES,
        # Re-anchored by ADR-0228, which branches on it at once, and by
        # ADR-0229, which refuses one inside a block first.
        "    if signals.iter().any(|s| s == root) {\n        if nested {\n",
        "    if false && signals.iter().any(|s| s == root) {\n        if nested {\n",
    ),
    (
        "a text part computed from a signal names no component",
        "cargo",
        VALUES,
        # Re-anchored by track `device-store`: a computed part's entry is
        # `computed_live`'s, given the component at each call.
        '                    if hole.nested { "value" } else { "text" },\n'
        "                    String::new(),\n"
        "                    inputs,\n"
        "                    derived.component_id.clone(),\n",
        '                    if hole.nested { "value" } else { "text" },\n'
        "                    String::new(),\n"
        "                    inputs,\n"
        "                    String::new(),\n",
    ),
    (
        "an attribute computed from a signal names no component",
        "cargo",
        VALUES,
        "                        attribute,\n"
        "                        inputs,\n"
        "                        derived.component_id.clone(),\n",
        "                        attribute,\n"
        "                        inputs,\n"
        "                        String::new(),\n",
    ),
    (
        "a view's field of a signal is read at the signal",
        "cargo",
        VALUES,
        "                inputs: vec![(root.to_string(), read.clone())],\n",
        "                inputs: vec![(root.to_string(), root.to_string())],\n",
    ),
    (
        "a page that computes from a signal has no module",
        "cargo",
        MODULE,
        "    let derived = &checked.pages().derived;\n",
        "    return Ok(Vec::new());\n    let derived = &checked.pages().derived;\n",
    ),
    (
        "the prepared executable plan has no computed module",
        "cargo",
        MODULE,
        "    let analysis = checked.pages();\n",
        "    return Ok(Vec::new());\n    let analysis = checked.pages();\n",
    ),
    (
        "the module names each function by its order",
        "cargo",
        MODULE,
        # Re-anchored by track `device-store`: a part's function given each
        # value it reads; and by the row's computed holes' ADR, a part's key
        # beside a row's.
        '            None => format!("\\"{}\\"", entry.part),\n',
        '            None => format!("\\"{}\\"", n),\n',
    ),
    (
        "the module does not decode the signal's value",
        "cargo",
        MODULE,
        '            decoded.join(", ")\n        );\n',
        '            takes.join(", ")\n        );\n',
    ),
    (
        "a module that did not compile is no refusal",
        "cargo",
        BUILD,
        "        for c in &self.computed {\n            if let Err(why) = &c.module {\n",
        "        for c in self.computed.iter().filter(|_| false) {\n            if let Err(why) = &c.module {\n",
    ),
    (
        "the build writes no module",
        "cargo",
        BUILD,
        '                write(&format!("computed/{}.mjs", c.page), source.as_bytes())?;\n',
        "",
    ),
    (
        "the host renders no first value from a signal",
        "cargo",
        SERVER,
        "        let env = self\n"
        "            .with_computed_signals(env, plan, self.template_of(page))\n"
        '            .unwrap_or_else(|e| panic!("`{page}`\'s computed values: {e}"));\n',
        "",
    ),
    (
        "the host reads no field of a signal's first value",
        "cargo",
        SERVER,
        "                first = first[field].clone();\n",
        "                let _ = field;\n",
    ),
    (
        "the document names no module",
        "cargo",
        SERVER,
        '        .any(|l| l["derived"].is_string())\n',
        '        .any(|l| l["derived"].is_string() && false)\n',
    ),
    (
        "a module no plan names is read",
        "cargo",
        SERVER,
        "                    .any(|p| computed_module(p).as_deref() == Some(r))\n",
        '                    .any(|_| r.starts_with("/computed/"))\n',
    ),
    (
        "the browser does not compute a value from a signal",
        "browser",
        RUNTIME,
        # Re-anchored by ADR-0229, which renders a block whose subject it is.
        "    if (live.derived && SET_IN_PLACE.has(live.kind)) {\n",
        "    if (false) {\n",
    ),
    (
        "the browser computes each value from the signal's first",
        "browser",
        RUNTIME,
        "      const v = (await computedValues()).parts[live.part](...argsOf(live));\n",
        "      const v = (await computedValues()).parts[live.part](firstValues.get(live.signal));\n",
    ),
    (
        "a computed attribute is set from the signal itself",
        "browser",
        RUNTIME,
        "      else setAttributePart(live, from.get(live.signal), v);\n",
        "      else setAttributePart(live, from.get(live.signal));\n",
    ),
]

CARGO = [
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core",
        "--test", "computed_signals", "--test", "computed_holes",
    ],
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "from_a_signal", "signals_alone",
    ],
]

SPEC = ["e2e/feed.spec.mjs"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


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


def cargo_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in CARGO:
        out, code = bounded(cmd, cwd=ROOT)
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out or "")
        if code is None:
            failed += 1
            continue
        if not results:
            built = False
            continue
        for p, f in results:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def build():
    """The store's and the feed's pages, with the runtime they load, and the
    server, as the source says."""
    _, page = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                      env={**os.environ, "BUILD_ONLY": "1"})
    _, feed = bounded(["bash", "spikes/own-renderer/feed.sh"], cwd=ROOT)
    _, server = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    return page == 0 and feed == 0 and server == 0


def browser_tests():
    """(built, passed, failed) for the spec in three engines, after a build.
    A spec skipped, as one whose build is stale is, ran nothing: not built."""
    if not build():
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"cargo": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite in sorted({m[1] for m in MUTANTS}):
        built, passed, failed = SUITES[suite]()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    try:
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
    finally:
        # The pages and the server as the source says, whatever a mutant
        # left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
