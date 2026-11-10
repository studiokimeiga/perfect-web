#!/usr/bin/env python3
"""Mutation controls for ADR-0176: a regeneration that fails sends nothing,
and is tried again.

Each mutant undoes one piece:
- the drain: a failed regeneration's frames sent at the version that did
  not move; a stale entry not tried again; a regeneration tried again that
  sends nothing;
- the answer: the commit names the version that did not move;
- the subscription: a page's request does not drain its session, seen by a
  test of its own, since a hold tries it again too;
- the control: `/bench/materializer` arms nothing.

A drain, answer or control mutant must fail the development server's test
that sees its rule (measured, below); the subscription's,
`e2e/materializer.spec.mjs` in Chromium.

Run from the repository root; `just e14-materializer-failure` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a failed regeneration's frames are sent",
        "server",
        SERVER,
        "        if regenerated == pw_materialize::Regenerated::Failed {\n",
        "        if false && regenerated == pw_materialize::Regenerated::Failed {\n",
    ),
    (
        "a stale entry is not tried again",
        "server",
        SERVER,
        "        if existing && !stale && invalidated.is_empty() {\n",
        "        if existing && invalidated.is_empty() {\n",
    ),
    (
        "a regeneration tried again sends nothing",
        "server",
        SERVER,
        # Re-anchored by NEXT 4c's render merge (track `render-merge`): the
        # documents told are taken where the entry exists, before the
        # drain's render, and told after it.
        "        let Some(told) = told else {\n",
        "        let Some(told) = told.filter(|_| !invalidated.is_empty()) else {\n",
    ),
    (
        "the answer names the version that did not move",
        "server",
        SERVER,
        "        let version = if stale {\n",
        "        let version = if false && stale {\n",
    ),
    (
        "a page's request does not drain its session",
        "request",
        SERVER,
        "    server.drain(session);\n",
        "",
    ),
    (
        "`/bench/materializer` arms nothing",
        "server",
        SERVER,
        "                .insert(session.clone());\n"
        "            respond_json(&mut stream, 200, &session, fresh, \"{}\");\n",
        "                .remove(&session);\n"
        "            respond_json(&mut stream, 200, &session, fresh, \"{}\");\n",
    ),
]

CARGO = {
    # The server test that fails under its mutants, measured against the
    # whole suite by `scripts/mutant_killers.py` on 2026-10-12 (W6's ruling
    # of 2026-10-09): it alone fails under each of the five.
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_regeneration_that_fails_sends_nothing_and_is_tried_again",
    ],
    # A hold tries a failed regeneration too (ADR-0402), so what a
    # request does alone is seen where no hold tries it.
    "request": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::longer_holds::a_pages_next_request_tries_a_failed_regeneration_again",
    ],
}
BROWSER = ["e2e/materializer.spec.mjs", "--project=chromium"]

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


def cargo_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    out, code = bounded(CARGO[suite], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def browser_tests(_suite="browser"):
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
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"server": cargo_tests, "request": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run(suite)
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
            built, passed, failed = SUITES[suite](suite)
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
    # The page and the server are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
