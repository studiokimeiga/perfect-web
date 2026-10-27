#!/usr/bin/env python3
"""Mutation controls for ADR-0305: a page's commands run in the order it sent
them.

Each mutant undoes one piece:
- the server's: a command run without waiting for the one it names; one run
  whose named one never came, or was answered early, or is still running;
  the wait not waited, or never ended; a command closed unrun, or answered
  early, taken for one that ran; another session's command taken for the
  one named; one in flight let go past the bound; a session forgotten that
  keeps its commands' order; an early answer that is no refusal;
- the runtime's: a command that names none before it, or the first and not
  the latest; an early answer taken for a failure; one sent again at once,
  or naming the one before it again; a command kept unanswered after its
  answer; one whose answer never settles what waits for it.

The server's must fail its tests (`order::`, and the command route's over
HTTP); the runtime's, `e2e/command-order.spec.mjs` in Chromium. Run from the
repository root; `just e14-command-order` records the output. The source is
restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
ORDER = ROOT / "spikes/own-renderer/server/src/order.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a command is run without waiting for the one it names",
        "server",
        SERVER,
        # Re-anchored by track store-accounts: the wait follows the stale
        # check, for a command no kept answer answered (Q3).
        '                && let Some(after) = header("pw-after")\n',
        '                && let Some(after) = header("pw-after").filter(|_| false)\n',
    ),
    (
        "one whose named one never came runs, past the wait",
        "server",
        ORDER,
        "                    if now >= deadline {\n"
        "                        return Turn::Early;\n",
        "                    if now >= deadline {\n"
        "                        return Turn::Run;\n",
    ),
    (
        "one whose named one was answered early runs",
        "server",
        ORDER,
        "                Some(Fate::Early) => return Turn::Early,\n",
        "                Some(Fate::Early) => return Turn::Run,\n",
    ),
    (
        "one whose named one is still running runs",
        "server",
        ORDER,
        # Re-anchored by track `flake-hunt`: the wait counts itself for a test
        # (`Order::waits`), in a block.
        "                Some(Fate::Here) => {\n"
        "                    #[cfg(test)]\n"
        "                    self.waits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);\n"
        '                    fates = self.changed.wait(fates).expect("order")\n'
        "                }\n",
        "                Some(Fate::Here) => return Turn::Run,\n",
    ),
    (
        "the wait for the named one is not waited",
        "server",
        ORDER,
        "        let deadline = Instant::now() + self.wait;\n",
        "        let deadline = Instant::now();\n",
    ),
    (
        "the wait for the named one has no end",
        "server",
        ORDER,
        "                    if now >= deadline {\n",
        "                    if now >= deadline + Duration::from_secs(30) {\n",
    ),
    (
        "a command answered early is taken for one that ran",
        "server",
        SERVER,
        '                    if let Some(arrival) = arrival {\n                        arrival.early();\n                    }\n',
        '                    drop(arrival);\n',
    ),
    (
        # Re-anchored by ADR-0381: a command held before it runs vanishes by
        # the same lines, so the drop's are named by its comment.
        "a command closed unrun is taken for one that ran",
        "server",
        SERVER,
        '                    // Never run: what follows it waits for it to be sent again.\n                    if let Some(arrival) = arrival {\n                        arrival.vanish();\n                    }\n',
        '                    // Never run: what follows it waits for it to be sent again.\n                    drop(arrival);\n',
    ),
    (
        "another session's command is taken for the one named",
        "server",
        ORDER,
        "                .get(session)\n"
        "                .and_then(|kept| kept.iter().find(|(i, _)| i == after))\n",
        "                .values()\n"
        "                .flat_map(|kept| kept.iter())\n"
        "                .find(|(i, _)| i == after)\n",
    ),
    (
        "a command in flight is let go past the bound",
        "server",
        ORDER,
        "                match kept.iter().position(|(_, fate)| *fate != Fate::Here) {\n",
        "                match kept.iter().position(|_| true) {\n",
    ),
    (
        "a session forgotten keeps its commands' fates",
        "server",
        ORDER,
        '        self.fates.lock().expect("order").remove(session);\n',
        "        let _ = session;\n",
    ),
    (
        "a session forgotten for idleness keeps its commands' order",
        "server",
        SERVER,
        "            self.order.forget(&session);\n",
        "",
    ),
    (
        "an early answer is answered as a commit's",
        "browser",
        SERVER,
        "                respond_json(&mut stream, 409, &session, fresh, &early.to_string());\n",
        "                respond_json(&mut stream, 202, &session, fresh, &early.to_string());\n",
    ),
    (
        "a command names none before it",
        "browser",
        RUNTIME,
        '          ...(after && { "pw-after": after }),\n',
        "",
    ),
    (
        "a command names the first unanswered, not the latest",
        "browser",
        RUNTIME,
        "  const after = [...unanswered.keys()].at(-1);\n",
        "  const after = [...unanswered.keys()].at(0);\n",
    ),
    (
        "an early answer is taken for a failure",
        "browser",
        RUNTIME,
        "    if (response.status === 409) {\n",
        "    if (false) {\n",
    ),
    (
        # Re-anchored by ADR-0381: a send given up while the one before it is
        # unanswered waits its turn by the same lines, so the early answer's
        # are named by the line before them.
        "an early answer is sent again at once",
        "browser",
        RUNTIME,
        "        log.push(`${component} sent again: the one before it had not arrived`);\n"
        "        await Promise.all(before);\n",
        "        log.push(`${component} sent again: the one before it had not arrived`);\n",
    ),
    (
        # Re-anchored by ADR-0381, as above.
        "an early answer is sent again naming the one before it",
        "browser",
        RUNTIME,
        "        log.push(`${component} sent again: the one before it had not arrived`);\n"
        "        await Promise.all(before);\n"
        "        after = undefined;\n",
        "        log.push(`${component} sent again: the one before it had not arrived`);\n"
        "        await Promise.all(before);\n",
    ),
    (
        "a command is kept unanswered after its answer",
        "browser",
        RUNTIME,
        "    unanswered.delete(interaction);\n",
        "",
    ),
    (
        "an answer settles nothing that waits for it",
        "browser",
        RUNTIME,
        "    answered();\n",
        "",
    ),
]

CARGO = {
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "order::", "a_pages_command", "a_session_forgotten_takes_its_commands_order",
    ],
}
BROWSER = ["e2e/command-order.spec.mjs", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
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
