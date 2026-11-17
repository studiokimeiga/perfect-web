"""Every `actions/setup-node` step can find the package manager it caches for.

setup-node v5 caches for one package manager, which it must find on the
PATH, or it fails the step. Which one is decided in its `src/main.ts` at
a0853c24 (the v5.0.0 every workflow pins):
- the step's `cache` input, where one is given;
- otherwise the manager that `package.json` names
  (`"packageManager": "pnpm@10.15.1"` here), unless the step passes
  `package-manager-cache: false`.

npm comes with Node itself. pnpm is only there after an earlier
`pnpm/action-setup` step in the same job, one with no `if:` that could skip
it.

Fallow's weekly job ran setup-node with no pnpm installed from bd8411c5
(2026-10-14, setup-node v4 to v5) on: "Unable to locate executable file:
pnpm", five Mondays running, 2026-10-19 to 2026-11-16, and nothing read a
scheduled run's failure. Its review (#125) found that turning the cache off
is not enough where an explicit `cache: pnpm` stays, as ci.yml's steps have
it. setup-node reads `cache` first, so this test models that too.
"""

from __future__ import annotations

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOWS = sorted(
    p for pattern in ("*.yml", "*.yaml") for p in (ROOT / ".github" / "workflows").glob(pattern)
)
NAMED = json.loads((ROOT / "package.json").read_text()).get("packageManager", "").split("@")[0] or None

# A setup-node step in a workflow's text, however its `uses:` is quoted or
# cased (GitHub reads an action's owner and name in any case), and wherever
# on its line (a flow-style `- { uses: ... }` too): what the parsed steps are
# counted against, so a step the parser cannot read fails the count.
SETUP_NODE = re.compile(r"(?i)\buses:[ \t]*['\"]?actions/setup-node@")


def scalar(text: str) -> str:
    """A YAML scalar's value, unquoted and without its comment."""
    text = text.strip()
    if text[:1] in ("'", '"'):
        end = text.find(text[0], 1)
        return text[1:end] if end > 0 else text[1:]
    return re.split(r"\s#", text, maxsplit=1)[0].strip()


def jobs(workflow: str) -> dict[str, str]:
    """Each job's text by its name, from a workflow's `jobs:` block."""
    body = workflow.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([A-Za-z0-9_-]+):[ \t]*(?:#.*)?\n", body)
    return dict(zip(parts[1::2], parts[2::2]))


def fields(item: str, own: int) -> dict[str, str]:
    """A step's own keys, each one's value: a scalar's text, or the lines of
    its block. `item` is the step's text after its dash; its other keys are
    the lines at `own` spaces."""
    out: dict[str, str] = {}
    key = None
    for n, line in enumerate(item.splitlines()):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        depth = own if n == 0 else len(line) - len(line.lstrip(" "))
        found = re.match(r"([A-Za-z0-9_-]+):(.*)$", stripped) if depth == own else None
        if found:
            key = found.group(1)
            out[key] = scalar(found.group(2))
        elif key is not None and depth > own:
            out[key] = f"{out[key]}\n{line}" if out[key] else line
    return out


def steps(job: str) -> list[dict[str, str]]:
    """Each step of a job, in order, as its own keys."""
    found = re.search(r"(?m)^    steps:[ \t]*(?:#.*)?\n", job)
    if not found:
        return []
    block = []
    for line in job[found.end():].splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#") and len(line) - len(line.lstrip()) <= 4:
            break
        block.append(line)
    text = "\n".join(block)
    first = re.search(r"(?m)^( *)- ", text)
    if not first:
        return []
    own = len(first.group(1)) + 2
    return [fields(item, own) for item in re.split(rf"(?m)^{first.group(1)}- ", text)[1:]]


def inputs(step: dict[str, str]) -> dict[str, str]:
    """A step's `with:` inputs, each unquoted: the block's first level only."""
    lines = [line for line in step.get("with", "").splitlines() if line.strip()]
    depth = min((len(line) - len(line.lstrip(" ")) for line in lines), default=0)
    out = {}
    for line in lines:
        if len(line) - len(line.lstrip(" ")) != depth or line.strip().startswith("#"):
            continue
        found = re.match(r"\s*([A-Za-z0-9_-]+):(.*)$", line)
        if found:
            out[found.group(1)] = scalar(found.group(2))
    return out


def caches_for(step: dict[str, str], named: str | None = NAMED) -> str | None:
    """The package manager a setup-node step caches for, as setup-node v5's
    main.ts decides it. Only a literal `false` turns the automatic cache off
    here, so a value read at run time (`${{ ... }}`) is taken to leave it on."""
    given = inputs(step)
    if given.get("cache"):
        return given["cache"]
    if given.get("package-manager-cache", "true").lower() == "false":
        return None
    return named


def unready(job: str, named: str | None = NAMED) -> list[int]:
    """The setup-node steps of a job (by their index) whose package manager
    no earlier step installs for certain."""
    installed, refused = {"npm"}, []
    for n, step in enumerate(steps(job)):
        action = step.get("uses", "").lower()
        if action.startswith("pnpm/action-setup@") and "if" not in step:
            installed.add("pnpm")
        if action.startswith("actions/setup-node@"):
            manager = caches_for(step, named)
            if manager is not None and manager not in installed:
                refused.append(n)
    return refused


WEEKLY_AS_IT_WAS = """\
  weekly:
    if: github.event_name != 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/setup-node@a0853c24544627f65ddf259abe73b1d18a591444 # v5
        with:
          node-version: 24
      - name: Generate full Fallow report
        run: |
          npx -y fallow@3.23.0 --format json --quiet > fallow-report.json
"""
PNPM = "      - uses: pnpm/action-setup@fc06bc1257f339d1d5d8b3a19a8cae5388b55320 # v5\n"
SETUP = "      - uses: actions/setup-node@"


def given(*lines: str) -> str:
    """The weekly job as it was, with `lines` among its setup-node inputs."""
    return WEEKLY_AS_IT_WAS.replace(
        "node-version: 24\n", "node-version: 24\n" + "".join(f"          {line}\n" for line in lines)
    )


class SetupNodeFindsItsPackageManager(unittest.TestCase):
    def test_each_setup_node_step_can_find_its_package_manager(self):
        refused = []
        for path in WORKFLOWS:
            for name, job in jobs(path.read_text()).items():
                refused += [f"{path.name} {name} step {n + 1}" for n in unready(job)]
        self.assertEqual(refused, [], "setup-node caching for a package manager its job has not installed")

    def test_every_setup_node_step_is_one_the_check_reads(self):
        for path in WORKFLOWS:
            text = path.read_text()
            read = sum(
                step.get("uses", "").lower().startswith("actions/setup-node@")
                for job in jobs(text).values()
                for step in steps(job)
            )
            self.assertEqual(read, len(SETUP_NODE.findall(text)), path.name)
        weekly = jobs((ROOT / ".github/workflows/fallow.yml").read_text())["weekly"]
        self.assertTrue(any(s.get("uses", "").lower().startswith("actions/setup-node@") for s in steps(weekly)))
        self.assertEqual(NAMED, "pnpm", "the manager package.json names, which this file's cases assume")

    def test_the_weekly_job_as_it_was_is_refused_and_each_repair_passes(self):
        self.assertEqual(unready(WEEKLY_AS_IT_WAS), [1])
        self.assertEqual(unready(given("package-manager-cache: false")), [])
        self.assertEqual(unready(given("package-manager-cache: 'false'")), [])
        self.assertEqual(unready(WEEKLY_AS_IT_WAS.replace(SETUP, PNPM + SETUP)), [])
        self.assertEqual(unready(given("package-manager-cache: true")), [1])
        late = WEEKLY_AS_IT_WAS.replace("      - name: Generate", PNPM + "      - name: Generate")
        self.assertEqual(unready(late), [1], "pnpm set up after setup-node is too late")

    def test_an_explicit_cache_is_the_one_its_step_needs(self):
        # setup-node reads `cache` before `package-manager-cache` (main.ts):
        # ci.yml's `cache: pnpm`, copied with the cache turned off, still
        # needs pnpm (#125's review).
        self.assertEqual(unready(given("cache: pnpm", "package-manager-cache: false")), [1])
        self.assertEqual(unready(given("cache: pnpm").replace(SETUP, PNPM + SETUP)), [])
        self.assertEqual(unready(given("cache: npm")), [])
        self.assertEqual(unready(given("cache: yarn")), [1], "a manager this file cannot see installed")
        self.assertEqual(unready(given("cache: ${{ matrix.cache }}")), [1])

    def test_a_step_that_may_not_run_or_an_input_out_of_place_counts_for_nothing(self):
        guarded = PNPM + "        if: matrix.runner == 'ubuntu-24.04-arm'\n"
        self.assertEqual(unready(WEEKLY_AS_IT_WAS.replace(SETUP, guarded + SETUP)), [2], "setup-node, after it")
        under_env = WEEKLY_AS_IT_WAS.replace(
            "        with:\n", "        env:\n          package-manager-cache: false\n        with:\n"
        )
        self.assertEqual(unready(under_env), [1])
        quoted = WEEKLY_AS_IT_WAS.replace("- uses: actions/setup-node@", '- uses: "actions/setup-node@')
        quoted = quoted.replace("a0853c24544627f65ddf259abe73b1d18a591444 # v5\n        with", 'a0853c24544627f65ddf259abe73b1d18a591444" # v5\n        with')
        self.assertEqual(unready(quoted), [1])
        self.assertEqual(len(SETUP_NODE.findall(quoted)), 1)
        flow = "name: x\non: push\njobs:\n  weekly:\n    steps:\n      - { uses: actions/setup-node@v5 }\n"
        self.assertEqual(len(SETUP_NODE.findall(flow)), 1, "counted, though no step is read from it")
        self.assertEqual(sum(len(steps(job)) for job in jobs(flow).values()), 1)
        self.assertEqual([s.get("uses") for job in jobs(flow).values() for s in steps(job)], [None], "so the count fails")
        cased = WEEKLY_AS_IT_WAS.replace("- uses: actions/setup-node@", "- uses: Actions/Setup-Node@")
        self.assertEqual(unready(cased), [1])
        self.assertEqual(len(SETUP_NODE.findall(cased)), 1)
        self.assertEqual(unready(given("package-manager-cache: false"), named=None), [])
        self.assertEqual(unready(WEEKLY_AS_IT_WAS, named=None), [], "no manager named, no cache")


if __name__ == "__main__":
    unittest.main()
