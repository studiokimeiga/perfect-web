"""The gates a pull request is refused by run on every branch CI builds.

Ruff's new-only gate (`quality.yml`, `scripts/quality_delta.py`) and
Fallow's audit (`fallow.yml`) ran only on a pull request. L5 lived on its
branch for days with no pull request, every push of it built and tested by
`ci.yml`, and its first pull request (#126) was refused for 122 new Ruff
findings that none of its pushes had been shown: a whole CI round lost at
the end (2026-11-17). Each gate now also runs on a push of every branch CI
builds, against where the branch left master, so a branch is shown what its
pull request would be refused for before it has one (ADR-0281, amended).
"""

from __future__ import annotations

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"


def push_branches(workflow: str) -> list[str]:
    """The branches a workflow's `on.push` names, in order."""
    found = re.search(r"(?m)^  push:\n(?:[ \t]*#.*\n)*[ \t]*branches:[ \t]*\[([^\]]*)\]", workflow)
    if not found:
        return []
    return [b.strip().strip("'\"") for b in found.group(1).split(",") if b.strip()]


def step(workflow: str, *needles: str) -> str:
    """The one step of a workflow whose text holds every needle."""
    steps = re.split(r"(?m)^      - ", workflow)
    found = [s for s in steps if all(n in s for n in needles)]
    assert len(found) == 1, (needles, len(found))
    return found[0]


class BranchPushesMeetThePullRequestsGates(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = (WORKFLOWS / "ci.yml").read_text()
        cls.quality = (WORKFLOWS / "quality.yml").read_text()
        cls.fallow = (WORKFLOWS / "fallow.yml").read_text()

    def test_every_branch_ci_builds_runs_both_gates(self):
        built = [b for b in push_branches(self.ci) if b != "master"]
        self.assertTrue(built, "ci.yml builds branches beside master")
        self.assertEqual(sorted(set(built) - set(push_branches(self.quality))), [])
        self.assertEqual(sorted(set(built) - set(push_branches(self.fallow))), [])

    def test_ruffs_gate_reads_a_branch_against_where_it_left_master(self):
        gate = step(self.quality, "quality_delta.py", "merge-base")
        self.assertIn("if: github.event_name == 'push' && github.ref != 'refs/heads/master'", gate)
        self.assertIn('--base "$(git merge-base origin/master HEAD)"', gate)
        # master's own pushes keep the report of what is there already.
        report = step(self.quality, "Report existing Ruff baseline")
        self.assertIn("github.ref == 'refs/heads/master'", report)
        # A merge base needs the history: the job's checkout fetches it all.
        python = self.quality.split("\n  python:\n", 1)[1].split("\n  workflow-syntax:\n", 1)[0]
        self.assertIn("fetch-depth: 0", python)

    def test_fallows_audit_reads_a_branch_against_where_it_left_master(self):
        audit = self.fallow.split("\n  audit:\n", 1)[1].split("\n  weekly:\n", 1)[0]
        self.assertIn("if: github.event_name == 'pull_request' || github.event_name == 'push'", audit)
        self.assertIn("fetch-depth: 0", audit)
        base = step(self.fallow, "id: base")
        self.assertIn('sha=$(git merge-base origin/master HEAD)', base)
        pushed = step(self.fallow, "changed-since: ${{ steps.base.outputs.sha }}")
        self.assertIn("if: github.event_name == 'push'", pushed)
        self.assertIn("gate: new-only", pushed)
        self.assertIn("fail-on-issues: true", pushed)

    def test_the_weekly_report_runs_on_its_schedule_alone(self):
        weekly = self.fallow.split("\n  weekly:\n", 1)[1]
        self.assertIn(
            "if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'", weekly
        )

    def test_the_branch_list_is_read_as_written(self):
        self.assertEqual(
            push_branches('on:\n  push:\n    # a comment\n    branches: [master, "track/**", \'wip/**\']\n'),
            ["master", "track/**", "wip/**"],
        )
        self.assertEqual(push_branches("on:\n  pull_request:\n"), [])


if __name__ == "__main__":
    unittest.main()
