"""Every `actions/setup-node` step can find the package manager it caches for.

setup-node v5 turns caching on for the package manager that `package.json`
names (`"packageManager": "pnpm@10.15.1"` here) unless the step passes
`package-manager-cache: false` (its action.yml and README at a0853c24, the
v5.0.0 every workflow pins). It looks the manager up on the PATH, so a job
that has not installed pnpm stops at setup-node itself.

Fallow's weekly job did exactly that from bd8411c5 (2026-10-14, setup-node v4
to v5) on: "Unable to locate executable file: pnpm", five Mondays running,
2026-10-19 to 2026-11-16, and nothing read a scheduled run's failure. So a
job's setup-node step comes after `pnpm/action-setup`, or turns the cache
off, as failure-census.yml's does.
"""

from __future__ import annotations

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOWS = sorted((ROOT / ".github" / "workflows").glob("*.yml"))


def jobs(workflow: str) -> dict[str, str]:
    """Each job's text by its name, from a workflow's `jobs:` block."""
    body = workflow.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([A-Za-z0-9_-]+):[ \t]*\n", body)
    return dict(zip(parts[1::2], parts[2::2]))


def steps(job: str) -> list[str]:
    """Each step's text, in order: the items of the job's `steps:` list."""
    found = re.search(r"(?m)^    steps:[ \t]*\n", job)
    if not found:
        return []
    block = []
    for line in job[found.end():].splitlines():
        if line.strip() and len(line) - len(line.lstrip()) <= 4:
            break
        block.append(line)
    text = "\n".join(block)
    first = re.search(r"(?m)^( *)- ", text)
    if not first:
        return []
    return re.split(rf"(?m)^{first.group(1)}- ", text)[1:]


def uses(step: str) -> str:
    found = re.search(r"(?m)^\s*uses:\s*([^\s#]+)", step)
    return found.group(1) if found else ""


def unready(job: str) -> list[int]:
    """The setup-node steps (by their index) that would cache for a package
    manager no earlier step of the job installs."""
    pnpm, refused = False, []
    for n, step in enumerate(steps(job)):
        action = uses(step)
        if action.startswith("pnpm/action-setup@"):
            pnpm = True
        off = re.search(r"(?m)^\s*package-manager-cache:\s*false\s*(?:#.*)?$", step)
        if action.startswith("actions/setup-node@") and not pnpm and not off:
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


class SetupNodeFindsItsPackageManager(unittest.TestCase):
    def test_each_setup_node_step_can_find_its_package_manager(self):
        refused = []
        for path in WORKFLOWS:
            for name, job in jobs(path.read_text()).items():
                refused += [f"{path.name} {name} step {n + 1}" for n in unready(job)]
        self.assertEqual(refused, [], "setup-node with pnpm uninstalled and its cache on")

    def test_every_setup_node_step_is_one_the_check_reads(self):
        for path in WORKFLOWS:
            text = path.read_text()
            read = sum(
                uses(step).startswith("actions/setup-node@")
                for job in jobs(text).values()
                for step in steps(job)
            )
            self.assertEqual(read, text.count("uses: actions/setup-node@"), path.name)
        weekly = jobs((ROOT / ".github/workflows/fallow.yml").read_text())["weekly"]
        self.assertTrue(any(uses(s).startswith("actions/setup-node@") for s in steps(weekly)))

    def test_the_weekly_job_as_it_was_is_refused_and_each_repair_passes(self):
        self.assertEqual(unready(WEEKLY_AS_IT_WAS), [1])
        off = WEEKLY_AS_IT_WAS.replace(
            "node-version: 24\n", "node-version: 24\n          package-manager-cache: false\n"
        )
        self.assertEqual(unready(off), [])
        pnpm = WEEKLY_AS_IT_WAS.replace(
            "      - uses: actions/setup-node@",
            "      - uses: pnpm/action-setup@fc06bc1257f339d1d5d8b3a19a8cae5388b55320 # v5\n"
            "      - uses: actions/setup-node@",
        )
        self.assertEqual(unready(pnpm), [])
        late = WEEKLY_AS_IT_WAS.replace(
            "      - name: Generate full Fallow report",
            "      - uses: pnpm/action-setup@fc06bc1257f339d1d5d8b3a19a8cae5388b55320 # v5\n"
            "      - name: Generate full Fallow report",
        )
        self.assertEqual(unready(late), [1], "pnpm set up after setup-node is too late")
        on = WEEKLY_AS_IT_WAS.replace(
            "node-version: 24\n", "node-version: 24\n          package-manager-cache: true\n"
        )
        self.assertEqual(unready(on), [1])


if __name__ == "__main__":
    unittest.main()
