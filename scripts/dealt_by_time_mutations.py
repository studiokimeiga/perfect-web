#!/usr/bin/env python3
"""Mutation controls for ADR-0290: a verification run is dealt by the
seconds its recipes last took.

Each mutant undoes one piece: the plan's costs read from the seconds kept,
a shard's setup counted where a recipe is dealt, the unmeasured estimated
at the upper quartile of its kind (the amendment of 2026-10-09: not the
median, not every recipe's, its scripts read for its kind, the host's
tests a kind of their own), the database recipes' shards chosen by when
the run ends (and the fewest of those that end it as soon), and the fetch
keeping only a recipe that ran to its end, over the others kept; and (the
amendment of 2026-10-10) a run dealt into more shards than it asks for where
its own would end a shard past the ceiling, a recipe longer than it alone
let be, and the ceiling under a shard's limit; and (ADR-0281's amendment
of 2026-10-12) the fetch refusing a file whose recipe reads what changed
since its run, against the tree here, staged or not, and one no recipe
wrote, and copying none of a recipe it skips; and (its amendment of
2026-10-13) a push to `master` planned from the newest commit on its
first-parent line that a push's or the nightly's run passed at, read by its
commit, and from the commit before the push on any error; and (its
amendment (6)) a push running no recipe whose committed evidence is
current: evidence not current where what the recipe reads changed since,
where it was written from a tree with changes, where the head does not
descend from its commit, or where the recipe's files were recorded at two
commits or one not at all; the plan dropping what is current only where it
is asked, and the workflow asking at each push; and (train-5's finding,
2026-10-14) a recipe planned by a program its own lines name, a glob read
segment by segment and a path naming what is under it; and (train-10's
finding, 2026-10-17) a recipe that skips without a database dealt where
there is none, and (2026-11-17) one refused without one; and (the
integrator's ruling 2 of 2026-10-17) a recipe
recorded past a job's limit planned, its shard's setup not counted against
it, a note in the seconds kept read as a recipe's, and a recipe run to its
end keeping its lower bound. At least one test of
`scripts/tests/test_ci_scripts.py` must then fail.

Run from the repository root; `just e14-dealt-by-time` records the output.
The sources are restored after every mutant, whatever happens.
"""

import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLAN = ROOT / "scripts/ci_plan.py"
FETCH = ROOT / "scripts/evidence_fetch.py"
VBASE = ROOT / "scripts/verified_base.py"
VERIFY = ROOT / ".github/workflows/verify.yml"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    # And a source's own test module, by its module's filter (train-6's
    # survivor's test, in `csp.rs`).
    (
        "a source's own test module plans no script that runs it",
        PLAN,
        "        if any(names_module(text, m) for m in modules):\n",
        "        if False:\n",
    ),
    (
        "a source's own test module plans no recipe that runs it",
        PLAN,
        "        or any(names_module(body.get(name, \"\"), m) for m in modules)\n",
        "        or False\n",
    ),
    (
        "a line before a source's test module is taken as its test's",
        PLAN,
        "        if start is not None and any(last >= start for _, last in hunks):\n",
        "        if start is not None:\n",
    ),
    # Train-6's finding: a server test module is run by its filter, and a
    # change to one plans what runs it.
    (
        "a server test module plans no script that runs it by its filter",
        PLAN,
        ' or f"tests::{t}::" in text\n            for t in tests\n',
        "\n            for t in tests\n",
    ),
    (
        "a server test module plans no recipe that runs it by its filter",
        PLAN,
        '        or any(f"tests::{t}::" in body.get(name, "") for t in tests)\n',
        "        or False\n",
    ),
    # ADR-0281, amended 2026-10-13 (6): a push does not run a recipe whose
    # committed evidence is current at its head.
    (
        "a recipe's evidence is current though what it reads changed since",
        PLAN,
        "            if name not in reached[sha]:\n",
        "            if True:\n",
    ),
    (
        "evidence written from a tree with changes is current",
        PLAN,
        'RECORDED_AT = re.compile(r"^commit: ([0-9a-f]{40})[ \\t]*$", re.M)\n',
        'RECORDED_AT = re.compile(r"^commit: ([0-9a-f]{40})", re.M)\n',
    ),
    (
        "evidence of a commit the head does not descend from is current",
        PLAN,
        "            if not descends_from(sha, head):\n                continue\n",
        "            if False:\n                continue\n",
    ),
    (
        "a recipe whose files were recorded at two commits is current",
        PLAN,
        "            sha = next(iter(shas)) if len(shas) == 1 else None\n",
        "            sha = next(iter(shas)) if len(shas) >= 1 else None\n",
    ),
    (
        "a recipe a file of which was not recorded is current",
        PLAN,
        "            if sha is None:\n                continue\n",
        "            if False:\n                continue\n",
    ),
    (
        "a plan drops what is current unasked",
        PLAN,
        "        if args.current:\n",
        "        if True:\n",
    ),
    (
        "a push plans with no regard to the evidence",
        VERIFY,
        ' --changed "$base" "$GITHUB_SHA" --current $stale >> "$GITHUB_OUTPUT"\n'
        "              # The chain's choice,",
        ' --changed "$base" "$GITHUB_SHA" $stale >> "$GITHUB_OUTPUT"\n'
        "              # The chain's choice,",
    ),
    # ADR-0281, amended 2026-10-13: a shell script a recipe runs plans it.
    (
        "a shell script a recipe runs is not read",
        PLAN,
        # Re-anchored by train-5's finding: the programs a recipe's own lines
        # name are read after the shell scripts it runs.
        "        if rel in changed:\n            return True\n    if any(names_path(",
        "        if False:\n            return True\n    if any(names_path(",
    ),
    # ADR-0281, amended 2026-10-13: what a run passed at is not run again.
    (
        "a dispatch's run counts as covering what it pushed",
        VBASE,
        'COVERING = ("push", "schedule")\n',
        'COVERING = ("push", "schedule", "workflow_dispatch")\n',
    ),
    (
        "the nightly's run does not count",
        VBASE,
        'COVERING = ("push", "schedule")\n',
        'COVERING = ("push",)\n',
    ),
    (
        "a merge's second parent counts",
        VBASE,
        '["git", "rev-list", "--first-parent", f"{before}..{head}"]',
        '["git", "rev-list", f"{before}..{head}"]',
    ),
    (
        "a run that did not pass counts",
        VBASE,
        'r.get("conclusion") == "success" and ',
        "",
    ),
    (
        "the oldest commit a run passed at is the base",
        VBASE,
        "        for sha in first_parents(before, head, root):\n",
        "        for sha in reversed(first_parents(before, head, root)):\n",
    ),
    (
        "the base is always the commit before the push",
        VBASE,
        "            if passed(runs(sha)):\n                return sha\n",
        "            if passed(runs(sha)):\n                return before\n",
    ),
    (
        "an error ends the plan",
        VBASE,
        "    except Exception as error:  # noqa: BLE001 - any failure plans more, never less\n",
        "    except ZeroDivisionError as error:\n",
    ),
    (
        "the browser suites run again where only records changed",
        VBASE,
        "    return not all(n.startswith(RECORDS) for n in names)\n",
        "    return True\n",
    ),
    (
        "the browser suites are skipped whatever changed",
        VBASE,
        "    return not all(n.startswith(RECORDS) for n in names)\n",
        "    return False\n",
    ),
    (
        "the runs are read by no commit",
        VBASE,
        'f"?head_sha={sha}&status=success&per_page=100",',
        'f"?status=success&per_page=100",',
    ),
    (
        "a file whose recipe reads what changed since its run is copied",
        FETCH,
        "    if recipe in moved:\n",
        "    if False:\n",
    ),
    (
        "what changed since a run is read between commits, not against the tree here",
        FETCH,
        "ci_plan.hunks(sha, None)",
        "ci_plan.hunks(sha, \"HEAD\")",
    ),
    (
        "the tree here is read as its last commit",
        PLAN,
        "*([head] if head else [])",
        "*([head] if head else [\"HEAD\"])",
    ),
    (
        "a recipe the fetch skips has its files judged",
        FETCH,
        "    if recipe in skip:\n        return \"skipped\"\n",
        "",
    ),
    (
        "a file no recipe wrote is copied",
        FETCH,
        "    if recipe is None:\n        return \"no recipe of the run wrote it\"\n",
        "",
    ),
    (
        "a recipe costs its mutants again",
        PLAN,
        "    costs = estimated(names, body, seconds)\n",
        '    costs = {n: cost(body.get(n, "")) for n in names}\n',
    ),
    (
        "a shard's setup is not counted",
        PLAN,
        "            return ends[k] + costs.get(name, 1) + SETUP[joined(kinds[k], wants)] - SETUP[kinds[k]]\n",
        "            return ends[k] + costs.get(name, 1)\n",
    ),
    (
        "a recipe not yet measured is estimated at the rate where none is",
        PLAN,
        "        return rs[(3 * len(rs)) // 4] if rs else RATE\n",
        "        return RATE\n",
    ),
    # Its amendment of 2026-10-09: the upper quartile of its kind.
    (
        "a recipe not yet measured is estimated at the median",
        PLAN,
        "        return rs[(3 * len(rs)) // 4] if rs else RATE\n",
        "        return rs[len(rs) // 2] if rs else RATE\n",
    ),
    (
        "a recipe not yet measured is estimated at every recipe's rate",
        PLAN,
        "        rs = sorted(rates.get(k) or every)\n",
        "        rs = sorted(every)\n",
    ),
    (
        "a script a recipe runs is not read for its kind",
        PLAN,
        "            texts.append((ROOT / \"scripts\" / script).read_text())\n",
        "            pass\n",
    ),
    (
        "the host's tests are taken for a type check's",
        PLAN,
        '    if "pw-dev-server" in text:\n        return "host"\n',
        "",
    ),
    (
        "the recipes run against a database take a shard each",
        PLAN,
        "    for beside in range(1, most + 1) if on_database else [0]:\n",
        "    for beside in [most] if on_database else [0]:\n",
    ),
    (
        "the recipes run against a database share one shard",
        PLAN,
        "    for beside in range(1, most + 1) if on_database else [0]:\n",
        "    for beside in [1] if on_database else [0]:\n",
    ),
    (
        "more shards are taken where fewer end the run as soon",
        PLAN,
        "        if best is None or score < best[0]:\n",
        "        if best is None or score <= best[0]:\n",
    ),
    # ADR-0290, 2026-10-16: an irreducible singleton must not hide an
    # avoidable ceiling excess in another shard at the same last finish.
    (
        "a long singleton hides a multi-recipe shard past the ceiling",
        PLAN,
        "        score = (ends, overflow)\n",
        "        score = (ends, 0)\n",
    ),
    # The integrator's ruling 2 (train-10): a recipe recorded past a job's
    # limit is refused before any shard runs; its shard's setup counts; a
    # note in the seconds kept is no recipe; a run to its end ends a lower
    # bound.
    (
        "a recipe recorded past a job's limit is planned",
        PLAN,
        "    if too_long:\n",
        "    if False:\n",
    ),
    (
        "a shard's setup is not counted against a job's limit",
        PLAN,
        "        if n in seconds and seconds[n] + SETUP[need.get(n, (False, False))] > LIMIT\n",
        "        if n in seconds and seconds[n] > LIMIT\n",
    ),
    (
        "a note in the seconds kept is read as a recipe's",
        PLAN,
        '            if not k.startswith("_")\n',
        "            if True\n",
    ),
    (
        "a recipe run to its end keeps its lower bound",
        FETCH,
        "            bounds.pop(name, None)\n",
        "            pass\n",
    ),
    # Train-10's verify (39054436657): a recipe that skips without a
    # database, dealt where there is none, records nothing and passes.
    (
        "a recipe that skips without a database is dealt where there is none",
        PLAN,
        '    "e14-scheduled-orders",\n',
        "",
    ),
    # Found before L5's full verify (2026-11-17): a recipe refused without
    # a database, in words the skip's test does not read, dealt where there
    # is none, fails its run.
    (
        "a recipe refused without a database is dealt where there is none",
        PLAN,
        '    "e14-generic-upload-claims",\n',
        "",
    ),
    (
        "a plan of nothing takes the longest of no shards",
        PLAN,
        "    if not names:\n        return []\n",
        "",
    ),
    (
        "a recipe that failed has its seconds kept",
        FETCH,
        '    return {r["recipe"]: round(r["seconds"]) for r in results if r.get("status") == 0 and not r.get("invalid")}\n',
        '    return {r["recipe"]: round(r["seconds"]) for r in results if not r.get("invalid")}\n',
    ),
    (
        "the seconds kept replace every other recipe's",
        FETCH,
        "    kept.update(seconds)\n",
        "    kept = dict(seconds)\n",
    ),
    (
        "a run is dealt into the shards it asks for, past the ceiling",
        PLAN,
        "    for more in range(shards + 1, max(shards, MOST) + 1):\n",
        "    for more in range(0):\n",
    ),
    (
        "a recipe longer than the ceiling alone is paid for in shards",
        PLAN,
        '        return all(ends([s], costs) <= CEILING or len(s["recipes"]) == 1 for s in dealt)\n',
        '        return all(ends([s], costs) <= CEILING for s in dealt)\n',
    ),
    (
        "a run's plan is its asked shards' again",
        PLAN,
        '    print("shards=" + json.dumps(planned(names, costs, args.shards, need, NEEDS_DATABASE)))\n',
        '    print("shards=" + json.dumps(plan(names, costs, args.shards, need, NEEDS_DATABASE)))\n',
    ),
    (
        "the ceiling is a shard's limit",
        PLAN,
        "CEILING = 10_350\n",
        "CEILING = 20_700\n",
    ),
    (
        "a program a recipe's own lines name plans nothing",
        PLAN,
        "    if any(names_path(p, c) for p in body_inputs(body) for c in changed):\n        return True\n",
        "",
    ),
    (
        "a glob's star crosses a directory",
        PLAN,
        "    return len(parts) == len(names) and all(\n"
        "        fnmatch.fnmatchcase(n, p) for p, n in zip(parts, names)\n"
        "    )\n",
        "    return fnmatch.fnmatchcase(path, pattern)\n",
    ),
    (
        "a path a recipe names is only itself",
        PLAN,
        '        return path == pattern or path.startswith(pattern + "/")\n',
        "        return path == pattern\n",
    ),
]

TESTS = [[sys.executable, "-m", "unittest", "scripts/tests/test_ci_scripts.py"]]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        ran = re.search(r"^Ran (\d+) tests?", out, re.M)
        if ran is None:
            built = False
            continue
        bad = re.search(r"^FAILED \((.*)\)", out, re.M)
        n_bad = sum(int(n) for n in re.findall(r"=(\d+)", bad.group(1))) if bad else 0
        passed += int(ran.group(1)) - n_bad
        failed += n_bad
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the sources are still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run_tests()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not run)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
