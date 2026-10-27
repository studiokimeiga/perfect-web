#!/usr/bin/env python3
"""Mutation controls for ADR-0175: charter §15.5's one-shot network error and
forced reconnect, made by the server.

Each mutant undoes one piece:
- the drop: not taken, so every connection is dropped; `before` running the
  command anyway; `after` sending the answer anyway;
- the cut: an open stream, or an open long poll, not ended; a new
  subscription not refused inside the window; a cut that ends what opened
  after it too;
- the controls: `/bench/drop` arms nothing; `/bench/reconnect` cuts nothing.

A drop or cut mutant, and `/bench/reconnect`'s, must fail the development
server's tests that see its rule (measured, below); `/bench/drop`'s,
`e2e/connections.spec.mjs`
in Chromium. A page's held stream ends within two seconds of itself, so a
reconnect that cut nothing could pass in a browser by chance: the server's
test arms it through the control while a stream is held, and sees it end.

Run from the repository root; `just e14-connection-faults` records the
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
        "the drop is not taken",
        "server",
        SERVER,
        "            .and_then(|f| f.drop_command.take())\n",
        "            .and_then(|f| f.drop_command)\n",
    ),
    (
        # Re-anchored by ADR-0305: a command closed unrun is not yet the one
        # before what follows it.
        "`before` runs the command",
        "server",
        SERVER,
        "            if drop_at == Some(DropAt::Before) {\n"
        "                // Never run: what follows it waits for it to be sent again.\n"
        "                if let Some(arrival) = arrival {\n"
        "                    arrival.vanish();\n"
        "                }\n"
        "                return;\n"
        "            }\n",
        "",
    ),
    (
        "`after` sends the answer",
        "server",
        SERVER,
        "            if drop_at == Some(DropAt::After) {\n"
        "                return;\n"
        "            }\n",
        "",
    ),
    (
        "an open stream is not ended",
        "server",
        SERVER,
        "        // Cut off since it opened (ADR-0175): it ends, with no more bytes.\n"
        "        if server.cut_off(session, opened) {\n",
        "        // Cut off since it opened (ADR-0175): it ends, with no more bytes.\n"
        "        if false && server.cut_off(session, opened) {\n",
    ),
    (
        "an open long poll is not ended",
        "server",
        SERVER,
        "    for _ in 0..40 {\n"
        "        if server.cut_off(session, opened) {\n",
        "    for _ in 0..40 {\n"
        "        if false && server.cut_off(session, opened) {\n",
    ),
    (
        "a new subscription is not refused",
        "server",
        SERVER,
        "    let opened = std::time::Instant::now();\n"
        "    if server.cut_off(session, opened) {\n",
        "    let opened = std::time::Instant::now();\n"
        "    if false && server.cut_off(session, opened) {\n",
    ),
    (
        "the window refuses nothing",
        "server",
        SERVER,
        "                    || f.cut_until\n",
        "                    || false && f.cut_until\n",
    ),
    (
        "a cut ends what opened after it",
        "server",
        SERVER,
        "                f.cut_from.is_some_and(|from| from > opened)\n",
        "                f.cut_from.is_some()\n",
    ),
    (
        "`/bench/drop` arms nothing",
        "browser",
        SERVER,
        "                .or_default()\n"
        "                .drop_command = Some(at);\n"
        "            respond_json(&mut stream, 200, &session, fresh, \"{}\");\n",
        "                .or_default()\n"
        "                .drop_command = None;\n"
        "            let _ = at;\n"
        "            respond_json(&mut stream, 200, &session, fresh, \"{}\");\n",
    ),
    (
        "`/bench/reconnect` cuts nothing",
        "server",
        SERVER,
        "            mine.cut_from = Some(now);\n",
        "            mine.cut_from = None;\n"
        "            let _ = now;\n",
    ),
]

CARGO = {
    # The development server's tests that see the server mutants' rules
    # (W6's ruling of 2026-10-09), measured by `scripts/mutant_killers.py` on
    # 2026-10-12 against its whole suite: each of the nine failed by two of
    # these where two fail at all, seven of them by one test alone. Its whole
    # suite ran for every mutant, and the recipe took 9,614 s of a verify.
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_dropped_command_connection_is_answered_by_nothing",
        "tests::a_forced_reconnect_ends_a_subscription_and_refuses_new_ones_for_its_window",
        "tests::a_pages_command_after_one_closed_unrun_waits_for_it_sent_again",
    ],
}
BROWSER = ["e2e/connections.spec.mjs", "--project=chromium"]

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


SUITES = {"server": cargo_tests, "browser": browser_tests}


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
