#!/usr/bin/env python3
"""Mutation controls for E14's agent telemetry (ADR-0391): usage an adapter
reports is kept as it was reported, or refused, and never changes a grade; a
timeout is the agent's failure, told apart; only infrastructure is ungraded.

Each mutant undoes one rule. `benchmarks/harness/test/agent-telemetry.test.mjs`
must fail each, by an assertion of its witness test in a complete run: a run
past its bound, a test that fails by an error, or a test missing from the
baseline's inventory kills nothing.

Run from the repository root; `just e14-agent-telemetry` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

import mutation_source

ROOT = pathlib.Path(__file__).resolve().parent.parent
HARNESS = ROOT / "benchmarks/harness"
BENCH = HARNESS / "bench.mjs"
TELEMETRY = HARNESS / "agent-telemetry.mjs"
TEST = "test/agent-telemetry.test.mjs"

KEPT = "provider usage is kept without converting missing counters to zeros"
OPTIONAL = "usage is optional when an adapter has not supplied a measurement"
INVALID = "invalid measurements are refused instead of becoming benchmark evidence"
LINKS = "usage symlinks are refused even if they point to a valid record"
OUTSIDE = "a real stub adapter writes usage outside the graded app"
NEVER_A_GRADE = "a refused sidecar is a refused measurement, never a changed grade"
KINDS = "an ordinary failure and a timeout score 0, and only infrastructure is ungraded"
TIMEOUT = "an agent that outlives its budget is scored 0 as a timeout"
STAGES = "successful agent work needs real, passing grading stages"
NAMED = {KEPT, OPTIONAL, INVALID, LINKS, OUTSIDE, NEVER_A_GRADE, KINDS, TIMEOUT, STAGES}

# (what is undone, file, anchor, replacement); each one's witness test is in
# WITNESSES.
MUTANTS = [
    (
        "a refused sidecar makes the run ungraded",
        BENCH,
        "    usageProblem = error.message;\n",
        '    return { ok: false, seconds, out: tail(result.out), usage: null,'
        ' usage_problem: error.message, failure_kind: "infra" };\n',
    ),
    (
        "a timeout is ungraded infrastructure",
        TELEMETRY,
        '  if (error_code === "ETIMEDOUT") return "timeout";\n',
        "",
    ),
    (
        "a timeout is told as the agent's ordinary failure",
        TELEMETRY,
        '  if (agentRun.failure_kind === "timeout") return { score: 0, failedAt: "timeout" };\n',
        "",
    ),
    (
        "an adapter's temporary failure is scored as the agent's",
        TELEMETRY,
        '  if (status === null || [75, 126, 127].includes(status)) return "infra";\n',
        '  if (status === null || [126, 127].includes(status)) return "infra";\n',
    ),
    (
        "an unwritten sidecar is a fabricated zero",
        TELEMETRY,
        "  if (fd === null) return null;\n",
        "  if (fd === null) return { input_tokens: 0, output_tokens: 0 };\n",
    ),
    (
        "a linked sidecar is followed",
        TELEMETRY,
        "    return fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);\n",
        "    return fs.openSync(file, fs.constants.O_RDONLY);\n",
    ),
    (
        "a sidecar without its token counts is taken",
        TELEMETRY,
        '  if (!Object.hasOwn(out, "input_tokens") || !Object.hasOwn(out, "output_tokens")) {\n',
        "  if (false) {\n",
    ),
    (
        "the sidecar is written inside the graded app",
        BENCH,
        '  const usageFile = path.join(box.root, "AGENT_USAGE.json");\n',
        '  const usageFile = path.join(box.app, "AGENT_USAGE.json");\n',
    ),
    (
        "a run with no grading stages is scored",
        TELEMETRY,
        '  if (stages.length === 0) return { score: null, failedAt: "infra" };\n',
        "",
    ),
]

# The test each mutant must fail, by an assertion.
WITNESSES = {
    "a refused sidecar makes the run ungraded": NEVER_A_GRADE,
    "a timeout is ungraded infrastructure": TIMEOUT,
    "a timeout is told as the agent's ordinary failure": TIMEOUT,
    "an adapter's temporary failure is scored as the agent's": KINDS,
    "an unwritten sidecar is a fabricated zero": OPTIONAL,
    "a linked sidecar is followed": LINKS,
    "a sidecar without its token counts is taken": INVALID,
    "the sidecar is written inside the graded app": OUTSIDE,
    "a run with no grading stages is scored": STAGES,
}

# How long one run may take before it is a hang: killed, and no kill.
BOUND = 300

TOP = re.compile(r"^(not ok|ok) \d+ - (.*)$")


def run_suite():
    """(exit status, {test: None where it passed, else its failure's code}),
    or (None, {}) past the bound."""
    child = subprocess.Popen(
        ["node", "--test", "--test-reporter=tap", TEST],
        cwd=HARNESS, start_new_session=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        out, _ = child.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.communicate()
        return None, {}
    except BaseException:
        os.killpg(child.pid, signal.SIGKILL)
        child.communicate()
        raise
    results, current = {}, None
    for line in out.splitlines():
        top = TOP.match(line)
        if top:
            current = top.group(2).replace("\\#", "#")
            results[current] = None if top.group(1) == "ok" else "failed"
            continue
        code = re.match(r"^  code: '([A-Z_]+)'$", line)
        if code and current is not None and results.get(current) == "failed":
            results[current] = code.group(1)
    if any(r is not None for r in results.values()):
        print(out, flush=True)
    return child.returncode, results


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    status, baseline = run_suite()
    passed = sum(1 for r in baseline.values() if r is None)
    print(f"baseline: {passed} passed, {len(baseline) - passed} failed", flush=True)
    if status != 0 or not baseline or any(r is not None for r in baseline.values()):
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    missing = sorted(NAMED - set(baseline))
    if missing:
        print("FAIL: a mutant names a test the suite does not have: " + "; ".join(missing))
        return 1

    killed = refused = survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        witness = WITNESSES[what]
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})", flush=True)
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            status, results = run_suite()
        if status is None or set(results) != set(baseline):
            verdict = "REFUSED (incomplete run; no kill)"
            refused += 1
        elif results.get(witness) == "ERR_ASSERTION":
            failing = sum(1 for r in results.values() if r is not None)
            verdict = f"KILLED (by an assertion of its test; {failing} of {len(results)} tests fail)"
            killed += 1
        else:
            print(f"  (its test {witness!r}: {results.get(witness) or 'passed'})", flush=True)
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())
