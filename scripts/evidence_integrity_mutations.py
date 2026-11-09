#!/usr/bin/env python3
"""Mutation controls for track evidence-integrity: six ways evidence could
be wrong, or a gate fail for the wrong reason, without anything noticing.

Each mutant undoes one piece:
1. a mutation script's sources run as they are (`mutation_baseline.py`): its
   children write bytecode again, or its own process does, or bytecode an
   earlier run left for a source its mutants change is read;
2. stale evidence is planned and counts are checked (`ci_plan.py`,
   `verify.yml`, `evidence_counts.py`): a train's push plans no stale
   evidence, or the planner adds none where asked; evidence that names no
   commit is judged stale, or evidence the head does not hold is not; an
   evidence file's count is not compared with its script's mutants, nor its
   killed with its verdicts; a verdict that names no mutant, or one named
   before, is let be; a script the recipe runs may be counted nowhere; a
   suite's tag is taken as part of a mutant's name, or a description inside
   a longer one names it; a disagreement is planned again though its recipe
   is recorded on the development machine, or its evidence is current, or
   its staleness is read at the head and not in the tree here; and (the
   integrator's ruling 1 of 2026-10-17, train-10) a part's count held to its
   whole script's, a script's parts leaving a mutant to none or holding one
   twice, or a part no recipe runs let be; and (2b, train-11, 2026-10-20) a
   binary file's change is read as none: the diff's binaries not read, their
   readers' lines not changes, a directory named whole, a placeholder, a
   directory inside or ending in the binary's, or a record misread;
3. a rejected fixture's `@rule` is its code as written
   (`checking_source.rs`): an alias, a retired code, a code its check does
   not report, or another code's invariant passes; R-004 names its alias
   again;
4. `check_latency` bounds the checking thread's CPU time per thousand lines
   checked: checking is pathological (the program checked ten times over,
   as the ceiling is two and a half times CI's slowest runner's, some six
   and a half times the PC's), or the thread's clock reads nothing;
5. a browser suite runs on what its recipe built (`host-metadata.mjs`,
   the justfile): a `dist` that records no sources is served; `e14-signals`
   runs its suite on what was built before; `e14-slots`'s build failing
   leaves its suite to run;
6. `master` holds no unnumbered ADR (`unnumbered_adrs.py`): a mention in a
   code span is read too, or no mention is; an unnumbered ADR file is let
   be; a track's branch is held, or a train is not; a decision's path that
   evidence recorded before its numbering is read as a mention, or a path
   is let be outside evidence too, or a line of evidence with one is let be
   whole (amended 2026-10-17, train-10).

Each runs the suite that holds it: `scripts/tests/`'s, by `unittest`, or
`pw-core`'s, by `cargo test`. At least one test must fail.

Run from the repository root; `just e14-evidence-integrity` records the
output. The sources are restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts/mutation_baseline.py"
PLAN = ROOT / "scripts/ci_plan.py"
VERIFY = ROOT / ".github/workflows/verify.yml"
COUNTS = ROOT / "scripts/evidence_counts.py"
CORPUS = ROOT / "compiler/pw-core/tests/checking_source.rs"
R004 = ROOT / "examples/rejected/R-004-private-cart-in-shared-cache.pw"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
LATENCY = ROOT / "compiler/pw-core/tests/check_latency.rs"
METADATA = ROOT / "spikes/own-renderer/host-metadata.mjs"
JUSTFILE = ROOT / "justfile"
ADRS = ROOT / "scripts/unnumbered_adrs.py"
RECIPE = ROOT / "just/evidence-integrity.just"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        'a report is visible while its own gate is reading it',
        "atomic",
        RECIPE,
        '     } > "$report"; \\\n',
        '     } > docs/evidence/E14/evidence-integrity.txt; \\\n',
    ),
    (
        'a failed recording hides its diagnostic output',
        "atomic",
        RECIPE,
        'then cat "$report" >&2; fi;',
        'then :; fi;',
    ),
    (
        "a published report disappears from evidence planning",
        "atomic",
        PLAN,
        "    return sorted(set(WRITES.findall(body) + PUBLISHES.findall(body)))\n",
        "    return sorted(set(WRITES.findall(body)))\n",
    ),
    # 1. Bytecode.
    (
        "a mutation script's children write bytecode",
        "baseline",
        BASELINE,
        'os.environ["PYTHONDONTWRITEBYTECODE"] = "1"\n',
        "pass\n",
    ),
    (
        "a mutation script's own process writes bytecode",
        "baseline",
        BASELINE,
        "sys.dont_write_bytecode = True\n",
        "pass\n",
    ),
    (
        "bytecode left before for a source a mutant changes is read",
        "baseline",
        BASELINE,
        'forget_bytecode(getattr(sys.modules.get("__main__"), "MUTANTS", None) or [])\n',
        "pass\n",
    ),
    # 2. Stale evidence planned; counts checked.
    (
        "a train's push plans no stale evidence",
        "plan",
        VERIFY,
        'case "$GITHUB_REF_NAME" in track/train-*) stale="--stale" ;; esac\n',
        'case "$GITHUB_REF_NAME" in track/train-*) stale="" ;; esac\n',
    ),
    (
        "the planner adds no stale evidence where asked",
        "plan",
        PLAN,
        "        if args.stale:\n",
        "        if False:\n",
    ),
    (
        "evidence that names no commit is judged stale",
        "plan",
        PLAN,
        "        if files and all(t is None or NAMES_A_COMMIT.search(t) for t in texts):\n",
        "        if files:\n",
    ),
    (
        "evidence the head does not hold is not stale",
        "plan",
        PLAN,
        "        if files and all(t is None or NAMES_A_COMMIT.search(t) for t in texts):\n",
        "        if files and all(t is not None and NAMES_A_COMMIT.search(t) for t in texts):\n",
    ),
    (
        "an evidence file's count is not compared with its script's mutants",
        "plan",
        COUNTS,
        "    if n != len(mutants):\n",
        "    if False:\n",
    ),
    (
        "an evidence file's killed is not compared with its verdicts",
        "plan",
        COUNTS,
        "    if k != killed:\n",
        "    if False:\n",
    ),
    # The integrator's ruling 1 of 2026-10-17 (train-10): a script's
    # mutants split across recipes, each a part, together every mutant once.
    (
        "a part's count is held to its whole script's",
        "plan",
        COUNTS,
        "                    scripts[s] = loaded[s][start:end]\n",
        "                    scripts[s] = loaded[s]\n",
    ),
    (
        "a script's parts may leave a mutant to none",
        "plan",
        COUNTS,
        "    if none:\n",
        "    if False:\n",
    ),
    (
        "a script's parts may hold a mutant twice",
        "plan",
        COUNTS,
        "    if twice:\n",
        "    if False:\n",
    ),
    (
        "a part no recipe runs is let be",
        "plan",
        COUNTS,
        "        if not run.get(i):\n",
        "        if False:\n",
    ),
    (
        "a verdict that names no mutant of its script is let be",
        "plan",
        COUNTS,
        "        if found is None:\n",
        "        if False:\n",
    ),
    (
        "a verdict that names a mutant named before is let be",
        "plan",
        COUNTS,
        "if i not in taken and names(said, m)), None\n",
        "if names(said, m)), None\n",
    ),
    (
        "a script the recipe runs may be counted in no block",
        "plan",
        COUNTS,
        "            for s in run:\n                if s not in counted:\n",
        "            for s in []:\n                if s not in counted:\n",
    ),
    (
        "a verdict that says only KILLED is read as none",
        "plan",
        COUNTS,
        r'    r"^(.*): (KILLED(?: by [^()]+ \(.*\)| \(.*\))?|SURVIVED' + "|",
        r'    r"^(.*): (KILLED(?: by [^()]+ \(.*\)| \(.*\))|SURVIVED' + "|",
    ),
    (
        "a verdict that says what killed its mutant is read as none",
        "plan",
        COUNTS,
        r'    r"^(.*): (KILLED(?: by [^()]+ \(.*\)| \(.*\))?|SURVIVED' + "|",
        r'    r"^(.*): (KILLED(?: \(.*\))?|SURVIVED' + "|",
    ),
    (
        "a suite's tag is taken as part of a mutant's name",
        "plan",
        COUNTS,
        '    return TAG.sub(" ", rest).strip(" :") == ""\n',
        '    return rest.strip(" :") == ""\n',
    ),
    (
        "a description inside a longer one names its mutant",
        "plan",
        COUNTS,
        '    return TAG.sub(" ", rest).strip(" :") == ""\n',
        "    return True\n",
    ),
    (
        "a disagreement is planned again though its recipe is the development machine's",
        "plan",
        COUNTS,
        "    names = [r for r in recipes if r not in plan.LOCAL_ONLY]\n",
        "    names = list(recipes)\n",
    ),
    (
        "a disagreement is planned again though its evidence is current",
        "plan",
        COUNTS,
        "            reach=lambda sha, head: set(plan.recipes_for(plan.hunks(sha, None), plan.NEAR, names)),\n",
        "            reach=lambda sha, head: set(names),\n",
    ),
    (
        "a disagreement's staleness is read at the head, not in the tree here",
        "plan",
        COUNTS,
        "            reach=lambda sha, head: set(plan.recipes_for(plan.hunks(sha, None), plan.NEAR, names)),\n",
        "            reach=lambda sha, head: set(plan.recipes_for(plan.hunks(sha, head), plan.NEAR, names)),\n",
    ),
    # 2b. A binary's change planned by what reads it (train-11, 2026-10-20:
    # train-10's regenerated components planned nothing).
    (
        "a binary file's change is read as no change",
        "plan",
        PLAN,
        "    return with_readers(parsed(diff), binaries(diff), head, root)\n",
        "    return parsed(diff)\n",
    ),
    (
        "the diff's binary files are not read",
        "plan",
        PLAN,
        "    return sorted({m.group(2) or m.group(1) for m in BINARY.finditer(text)})\n",
        "    return []\n",
    ),
    (
        "a changed binary's readers are not changes",
        "plan",
        PLAN,
        "        for path, lines in read_by(binary, head, root).items():\n",
        "        for path, lines in {}.items():\n",
    ),
    (
        "a binary's directory named whole does not name it",
        "plan",
        PLAN,
        "        if not rest:\n            return True\n",
        "        if not rest:\n            continue\n",
    ),
    (
        "a placeholder names no file",
        "plan",
        PLAN,
        r'        pattern = re.sub(r"\$\{[^}]*\}|\$\w+|\{[^}]*\}|%[sd]", "*", rest)' + "\n",
        "        pattern = rest\n",
    ),
    (
        "a directory in the binary's names it",
        "plan",
        PLAN,
        '        if "/" in rest:\n            continue\n',
        '        if "/" in rest:\n            return True\n',
    ),
    (
        "another directory ending in the binary's names it",
        "plan",
        PLAN,
        r'    for m in re.finditer(r"(?<![\w.-])" + re.escape(directory) + r"(?![\w.-])(/[-\w.*?{}$%\[\]/]*)?", text):'
        + "\n",
        r'    for m in re.finditer(re.escape(directory) + r"(?![\w.-])(/[-\w.*?{}$%\[\]/]*)?", text):' + "\n",
    ),
    (
        "a record reads a binary",
        "plan",
        PLAN,
        "        if not number.isdigit() or path == binary or path.startswith(RECORDS):\n",
        "        if not number.isdigit() or path == binary:\n",
    ),
    # 3. The corpus's headers.
    (
        "a header's alias passes",
        "corpus",
        CORPUS,
        "    if let Some((_, code)) = DEPRECATED_ALIASES.iter().find(|(alias, _)| *alias == rule) {\n",
        "    if let Some((_, code)) = DEPRECATED_ALIASES.iter().find(|_| false) {\n",
    ),
    (
        "a header's retired code passes",
        "corpus",
        CORPUS,
        "    if RETIRED.iter().any(|(id, _)| *id == rule) {\n",
        "    if false {\n",
    ),
    (
        "a header's code its check does not report passes",
        "corpus",
        CORPUS,
        "    if !reported.contains(&rule) {\n",
        "    if false {\n",
    ),
    (
        "a header's invariant of another code passes",
        "corpus",
        CORPUS,
        "    if invariant != Some(code.symbol) {\n",
        "    if false {\n",
    ),
    (
        "R-004 names its alias again",
        "corpus",
        R004,
        "// @rule: PW5001\n",
        "// @rule: PW0100\n",
    ),
    # 4. The checker's latency.
    (
        "checking is pathological: the program is checked ten times over",
        "latency",
        CHECK,
        "    crate::program::validate_sources_with(files, released).diagnostics\n",
        "    for _ in 0..9 {\n"
        "        let _ = crate::program::validate_sources_with(files, released);\n"
        "    }\n"
        "    crate::program::validate_sources_with(files, released).diagnostics\n",
    ),
    (
        "the checking thread's clock reads nothing",
        "latency",
        LATENCY,
        "    Some(Duration::new(now.tv_sec as u64, now.tv_nsec as u32))\n",
        "    Some(Duration::ZERO)\n",
    ),
    # 5. A suite on what its recipe built.
    (
        "a dist that records no sources is served",
        "builds",
        METADATA,
        '  if (!existsSync(resolve(directory, "sources.json"))) return problem(`${build.dist} is no whole build (sources.json is missing)`);\n',
        '  if (!existsSync(resolve(directory, "sources.json"))) return null;\n',
    ),
    (
        "e14-signals runs its suite on what was built before",
        "builds",
        JUSTFILE,
        "e14-signals:\n"
        "    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null\n"
        "    @cargo build --quiet --locked -p pw-dev-server\n",
        "e14-signals:\n",
    ),
    (
        "e14-slots's build failing leaves its suite to run",
        "builds",
        JUSTFILE,
        "e14-slots:\n    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null\n",
        "e14-slots:\n    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null || true\n",
    ),
    # 6. No unnumbered ADR on master.
    (
        "a mention in a code span is read too",
        "adrs",
        ADRS,
        '            text = CODE_SPAN.sub("", line)\n',
        "            text = line\n",
    ),
    (
        "no mention is read",
        "adrs",
        ADRS,
        "            if MENTION.search(text):\n",
        "            if False:\n",
    ),
    # Amended 2026-10-17 (train-10's merge of train-9): a decision's file,
    # by the path evidence recorded it under before it was numbered.
    (
        "a path evidence recorded is read as a mention",
        "adrs",
        ADRS,
        '            if rel.startswith("docs/evidence/"):\n',
        "            if False:\n",
    ),
    (
        "a decision's path is let be outside evidence too",
        "adrs",
        ADRS,
        '            if rel.startswith("docs/evidence/"):\n',
        "            if True:\n",
    ),
    (
        "a line of evidence with a recorded path is let be whole",
        "adrs",
        ADRS,
        '                text = RECORDED_PATH.sub("", text)\n',
        '                text = "" if RECORDED_PATH.search(text) else text\n',
    ),
    (
        "an unnumbered ADR file is let be",
        "adrs",
        ADRS,
        '        if rel.startswith("docs/DECISIONS/") and pathlib.PurePosixPath(rel).name.startswith(PLACEHOLDER):\n',
        "        if False:\n",
    ),
    (
        "a track's branch is held",
        "adrs",
        ADRS,
        '    return ref == "refs/heads/master" or (ref or "").startswith("refs/heads/track/train-")\n',
        "    return True\n",
    ),
    (
        "a train is not held",
        "adrs",
        ADRS,
        '    return ref == "refs/heads/master" or (ref or "").startswith("refs/heads/track/train-")\n',
        '    return ref == "refs/heads/master"\n',
    ),
]

UNITTEST = [sys.executable, "-m", "unittest"]
SUITES = {
    "atomic": UNITTEST + ["scripts/tests/test_atomic_evidence.py"],
    "baseline": UNITTEST + ["scripts/tests/test_mutation_baseline.py"],
    "plan": UNITTEST + ["scripts/tests/test_ci_scripts.py", "scripts/tests/test_evidence_counts.py"],
    "corpus": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "checking_source", "--",
        "a_fixtures_header_is_held", "every_rejected_fixtures_rule",
    ],
    "latency": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "check_latency"],
    "builds": UNITTEST + ["scripts/tests/test_builds_before_suites.py"],
    "adrs": UNITTEST + ["scripts/tests/test_unnumbered_adrs.py"],
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1800


def counted(out):
    """(built, passed, failed) as a suite's output says: `unittest`'s `Ran`
    and `OK (...)` or `FAILED (...)` lines (a skipped test neither passed nor
    failed), or cargo's `test result:` lines."""
    ran = [int(n) for n in re.findall(r"^Ran (\d+) tests? in", out, re.M)]
    if ran:
        ended = re.findall(r"^(?:OK|FAILED) \((.*)\)$", out, re.M)
        failed = sum(int(n) for line in ended for n in re.findall(r"(?:failures|errors)=(\d+)", line))
        skipped = sum(int(n) for line in ended for n in re.findall(r"skipped=(\d+)", line))
        return True, sum(ran) - failed - skipped, failed
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def run_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    p = subprocess.Popen(SUITES[suite], cwd=ROOT, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    return counted(out)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the sources are still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite in SUITES:
        built, passed, failed = run_tests(suite)
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
            built, passed, failed = run_tests(suite)
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
