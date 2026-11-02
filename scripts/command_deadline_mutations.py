#!/usr/bin/env python3
"""Mutation controls for ADR-0381: a command's send is answered within its
deadline, or it is no answer.

Each mutant undoes one piece:
- the deadline: a send given none; a send given up taken for an outcome; a
  send held for the one before it sent again as a retry, naming that one, or
  without waiting its turn;
- `?deadline=`: raising the deadline, taking what is no time above zero or
  less than half a second, not read at all, or read from `?load-deadline=`;
- a send held for the one before it: held only while that one is unanswered,
  or counted as a retry;
- the host's fault: `/bench/hold` arming nothing; `before` running the
  command; `after` sending the answer; `body` sending the whole body; a hold
  not taken; a hold outliving its connection.

A runtime mutant, and `/bench/hold`'s, must fail `e2e/command-deadline.spec.mjs`
in Chromium ("browser"); a fault mutant, the development server's test of the
fault ("cargo"). Only complete native results earn a kill: a build that
fails, a producer that times out, a receipt that is missing or forged, or an
inventory that differs from the baseline's is REFUSED, never a kill.

Run from the repository root; `just e14-command-deadline` records the output.
The source is restored after every mutant, whatever happens.
"""

import json
import os
import pathlib
import secrets
import signal
import subprocess
import sys
import tempfile

import browser_test_results
import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a send has no deadline",
        "browser",
        RUNTIME,
        "        signal: unheard.signal,\n",
        "",
    ),
    (
        "a send given up is an outcome, not no answer",
        "browser",
        RUNTIME,
        "      const why = unheard.signal.aborted ? ` within ${sendDeadline} ms` : ` (${error.message ?? error})`;\n",
        "      if (unheard.signal.aborted) throw new Unreachable(component, error);\n"
        "      const why = unheard.signal.aborted ? ` within ${sendDeadline} ms` : ` (${error.message ?? error})`;\n",
    ),
    (
        "a send held for the one before it is sent again as a retry",
        "browser",
        RUNTIME,
        "      if (unheard.signal.aborted && after !== undefined) {\n",
        "      if (false) {\n",
    ),
    (
        "a send is held for the one before it only while that one is unanswered",
        "browser",
        RUNTIME,
        "      if (unheard.signal.aborted && after !== undefined) {\n",
        "      if (unheard.signal.aborted && unanswered.has(after)) {\n",
    ),
    (
        "a send held for the one before it counts as a retry",
        "browser",
        RUNTIME,
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n"
        "        await Promise.all(before);\n"
        "        after = undefined;\n"
        "        attempt -= 1;\n",
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n"
        "        await Promise.all(before);\n"
        "        after = undefined;\n",
    ),
    (
        "a send held for the one before it is sent again naming that one",
        "browser",
        RUNTIME,
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n"
        "        await Promise.all(before);\n"
        "        after = undefined;\n",
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n"
        "        await Promise.all(before);\n",
    ),
    (
        "a send held for the one before it does not wait its turn",
        "browser",
        RUNTIME,
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n"
        "        await Promise.all(before);\n",
        "        log.push(`${component} sent again: no answer within ${sendDeadline} ms, the one before it unanswered`);\n",
    ),
    (
        "`?deadline=` raises the deadline",
        "browser",
        RUNTIME,
        "  return asked > 0 ? Math.min(10_000, Math.max(500, asked)) : 10_000;\n",
        "  return asked > 0 ? Math.max(10_000, Math.max(500, asked)) : 10_000;\n",
    ),
    (
        "`?deadline=` takes what is no time above zero",
        "browser",
        RUNTIME,
        "  return asked > 0 ? Math.min(10_000, Math.max(500, asked)) : 10_000;\n",
        "  return asked >= 0 ? Math.min(10_000, Math.max(500, asked)) : 10_000;\n",
    ),
    (
        "`?deadline=` is not read",
        "browser",
        RUNTIME,
        '  const asked = Number(new URLSearchParams(location.search).get(name));\n',
        "  const asked = 0;\n",
    ),
    (
        "`?deadline=` and `?load-deadline=` take a time below half a second",
        "browser",
        RUNTIME,
        "  return asked > 0 ? Math.min(10_000, Math.max(500, asked)) : 10_000;\n",
        "  return asked > 0 ? Math.min(10_000, asked) : 10_000;\n",
    ),
    (
        "a send's deadline is read from `?load-deadline=`",
        "browser",
        RUNTIME,
        'const sendDeadline = lowered("deadline");\n',
        'const sendDeadline = lowered("load-deadline");\n',
    ),
    (
        "`/bench/hold` arms nothing",
        "browser",
        SERVER,
        "                .hold_command = Some(at);\n"
        '            respond_json(&mut stream, 200, &session, fresh, "{}");\n',
        "                .hold_command = None;\n"
        "            let _ = at;\n"
        '            respond_json(&mut stream, 200, &session, fresh, "{}");\n',
    ),
    (
        "`before` runs the command",
        "cargo",
        SERVER,
        '                if hold_at == Some(HoldAt::Before) {\n                    // Never run, as a request that never came: what follows it\n                    // waits for it to be sent again.\n                    if let Some(arrival) = arrival {\n                        arrival.vanish();\n                    }\n                    hold_open(&mut stream);\n                    return;\n                }\n',
        '',
    ),
    (
        "`after` sends the answer",
        "cargo",
        SERVER,
        '                if hold_at == Some(HoldAt::After) {\n                    // Run, and what follows it may run; its answer never begun.\n                    drop(arrival);\n                    hold_open(&mut stream);\n                    return;\n                }\n',
        '',
    ),
    (
        "`body` sends the whole body",
        "cargo",
        SERVER,
        "    let _ = stream.write_all(&body.as_bytes()[..body.len() / 2]);\n",
        "    let _ = stream.write_all(body.as_bytes());\n",
    ),
    (
        "a hold is not taken",
        "cargo",
        SERVER,
        "            .and_then(|f| f.hold_command.take())\n",
        "            .and_then(|f| f.hold_command)\n",
    ),
    (
        "a hold outlives its connection",
        "cargo",
        SERVER,
        "            // Closed by its client.\n"
        "            Ok(0) => return,\n",
        "            // Closed by its client.\n"
        "            Ok(0) => {}\n",
    ),
]

TEST = "tests::a_held_command_connection_is_answered_by_nothing_until_it_is_given_up"
CARGO = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
    "--", "--exact", TEST, "--format=pretty",
]
SPEC = ["e2e/command-deadline.spec.mjs"]
BOUND = 1200
INVENTORIES = {}


def bounded(cmd, **kw):
    """Shared native process ownership; timeout is invalid, never a test failure."""
    try:
        result = producer_process.run(cmd, timeout=BOUND, **kw)
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired as expired:
        return expired.output, None


def cargo_tests():
    out, code = bounded(CARGO, cwd=ROOT)
    result = cargo_assertion_results.validate(code, out, root=ROOT, sources=[SERVER], expected={TEST})
    if not result.valid or result.failed:
        print(out, flush=True)
    if not result.valid:
        print("REFUSED cargo producer: incomplete or unattributed result", flush=True)
        return False, 0, 0
    return True, result.passed, result.failed


def build():
    """Build the actual source; refusal cannot earn a mutation kill."""
    for command, env in (
        (["bash", "spikes/own-renderer/run.sh"], {"BUILD_ONLY": "1"}),
        (["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], {}),
    ):
        out, code = bounded(command, cwd=ROOT, env={**os.environ, **env})
        if code != 0:
            print(out, flush=True)
            return False
    return True


def browser_tests():
    if not build():
        return False, 0, 0
    with tempfile.TemporaryDirectory(prefix="pw-command-deadline-") as directory:
        receipt = pathlib.Path(directory) / "result.json"
        nonce = secrets.token_hex(32)
        out, code = bounded(
            ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",
             "pnpm", "exec", "playwright", "test", *SPEC, "--project=chromium", "--retries=0",
             "--reporter=line,./assertion-result-reporter.mjs"],
            cwd=ROOT / "spikes/own-renderer",
            env={**os.environ, "PW_ASSERTION_REPORT": str(receipt), "PW_ASSERTION_NONCE": nonce},
        )
        try:
            report = json.loads(receipt.read_text())
        except (OSError, ValueError):
            report = None
        result = browser_test_results.parse(code, report, nonce, INVENTORIES.get("browser"))
        if not result.valid or result.failed:
            print(out, flush=True)
        if not result.valid:
            print(f"REFUSED browser producer: {result.reason}", flush=True)
            return False, 0, 0
        INVENTORIES.setdefault("browser", result.inventory)
        return True, result.passed, result.failed


SUITES = {"cargo": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # retain native baseline output before starting producers
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
    INVENTORIES.clear()
    for suite in sorted({m[1] for m in MUTANTS}):
        valid, passed, failed = SUITES[suite]()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not valid or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    killed = refused = survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})")
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            valid, passed, failed = SUITES[suite]()
        if not valid or watch.stopped:
            verdict = "REFUSED (invalid or stopped producer; no kill)"
            refused += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            killed += 1
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
        if watch.stopped:
            print("REFUSED: memory guard stopped the campaign")
            return 1
    if not build() or watch.stopped:
        print("REFUSED: restored source did not build")
        refused += 1
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())
