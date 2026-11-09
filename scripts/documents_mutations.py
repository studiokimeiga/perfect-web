#!/usr/bin/env python3
"""Mutation controls for ADR-0161: each document is its own subscriber.

Each mutant puts back one piece of the session-wide subscriber:
- in the server: serving a document replaces the session's others, a change
  reaches only the session's latest document, a keyed read goes to the
  latest document, and a session is forgotten with any one of its documents;
- in the browser runtime: the page's long poll, or its stream, names no
  document.

A server mutant must fail the server's tests that see its rule (measured,
below). The
runtime's must fail the two-tab browser test, in Chromium.

Run from the repository root; `just e14-documents` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "serving a document replaces the session's others",
        "cargo",
        SERVER,
        "        let served = (|| {\n",
        # Bound first: a lock taken in a `for` loop's head is held through
        # its body, and the mutant would wait for itself.
        "        let others = documents_of(&self.pending.lock().expect(\"pending\"), session);\n"
        "        for d in others {\n"
        "            self.pending.lock().expect(\"pending\").remove(&d);\n"
        "        }\n"
        "        let served = (|| {\n",
    ),
    (
        "a change reaches only the session's latest document",
        "cargo",
        SERVER,
        # Re-anchored by ADR-0297: a telling takes the documents whose pages
        # ask; and by NEXT 4c's render merge (track `render-merge`), which
        # takes them (`to_tell`) before the render that reads them.
        "        let (live, passed): (Vec<Doc>, Vec<Doc>) = documents_of(&queue, session)\n"
        "            .into_iter()\n"
        "            .filter(|d| which(d) && queue.contains_key(d))\n",
        "        let (live, passed): (Vec<Doc>, Vec<Doc>) = documents_of(&queue, session)\n"
        "            .into_iter()\n"
        "            .rev()\n"
        "            .take(1)\n"
        "            .filter(|d| which(d) && queue.contains_key(d))\n",
    ),
    (
        "a keyed read goes to the session's latest document",
        "cargo",
        SERVER,
        "        let doc: Doc = (session.to_string(), document);\n"
        "        // A read for a document the server does not hold is no page's, and\n",
        "        let doc: Doc = documents_of(&self.keyed.lock().expect(\"keyed\"), session)\n"
        "            .pop()\n"
        "            .unwrap_or((session.to_string(), document));\n"
        "        // A read for a document the server does not hold is no page's, and\n",
    ),
    (
        "a session is forgotten with any one of its documents",
        "cargo",
        SERVER,
        "        .filter(|session| documents_of(queue, session).is_empty())\n",
        "        .filter(|_| true)\n",
    ),
    (
        "the page's long poll names no document",
        "browser",
        RUNTIME,
        "  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}${holdLong}`, {\n",
        "  const response = await fetch(`/stream?since=${cursor}${holdLong}`, {\n",
    ),
    (
        "the page's stream names no document",
        "browser",
        RUNTIME,
        "  const response = await fetch(`/stream?doc=${documentCursor}&since=${cursor}&mode=stream${holdLong}`, {\n",
        "  const response = await fetch(`/stream?since=${cursor}&mode=stream${holdLong}`, {\n",
    ),
]

# The server tests that fail under its mutants, measured against the whole
# suite by `scripts/mutant_killers.py` on 2026-10-12 (W6's ruling of
# 2026-10-09), two per mutant where two fail.
CARGO = [[
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
    "tests::a_change_reaches_each_page_that_reads_it_by_its_own_plan",
    "tests::a_change_reaches_every_document_of_its_session",
    "tests::a_read_older_than_the_latest_is_not_applied",
    "tests::a_session_is_forgotten_with_its_last_document",
]]
BROWSER = ["e2e/tabs.spec.mjs"]

# How long one command may run. A mutant can make a test wait for ever (a
# lock taken twice); past the bound it is killed with everything it started,
# and the run counts as failing.
BOUND = 900


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
    """(built, passed, failed) over every cargo test command."""
    built, passed, failed = True, 0, 0
    for cmd in CARGO:
        out, code = bounded(cmd, cwd=ROOT)
        if code is None:
            return True, passed, failed + 1
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            built = False
            continue
        for p, f in found:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def browser_tests():
    """(built, passed, failed) for the browser tests, after a build: the page,
    and the server the suite runs, which `run.sh` does not build."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    server = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--project=chromium",
         "--reporter=line"],
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
    for suite, run in SUITES.items():
        built, passed, failed = run()
        print(f"baseline ({suite}): {passed} passed, {failed} failed")
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
        print(f"{what}: {verdict}")
    # The page is built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
