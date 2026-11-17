"""The gates a pull request is refused by run on every branch CI builds.

Ruff's new-only gate (`quality.yml`, `scripts/quality_delta.py`) and
Fallow's audit (`fallow.yml`) ran only on a pull request. L5 lived on its
branch for days with no pull request, every push of it built and tested by
`ci.yml`, and its first pull request (#126) was refused for 122 new Ruff
findings that none of its pushes had been shown: a whole CI round lost at
the end (2026-11-17). Each gate now also runs on a push of every branch CI
builds, against where the branch left master, so a branch is shown what its
pull request would be refused for before it has one (ADR-0281, amended).

Each step's own `if:`, and its job's, is read from the workflow (the step
reader of `test_workflow_setup_node.py`) and evaluated for every event the
workflows take, so a condition that can never be true, or a step in the
wrong job, fails here: #128's review found the first version of this test,
which matched strings, passed on five such workflows.
"""

from __future__ import annotations

import pathlib
import re
import sys
import types
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from test_workflow_setup_node import jobs, steps  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"

# Every event the two workflows take, as (name, event, ref).
EVENTS = [
    ("pull request", "pull_request", "refs/pull/1/merge"),
    ("push to master", "push", "refs/heads/master"),
    ("push to wip", "push", "refs/heads/wip/a-branch"),
    ("push to track", "push", "refs/heads/track/a-track"),
    ("schedule", "schedule", "refs/heads/master"),
    ("dispatch on master", "workflow_dispatch", "refs/heads/master"),
    ("dispatch on a branch", "workflow_dispatch", "refs/heads/wip/a-branch"),
]


def push_branches(workflow: str) -> list[str]:
    """The branches a workflow's `on.push` names, in order."""
    found = re.search(r"(?m)^  push:\n(?:[ \t]*#.*\n)*[ \t]*branches:[ \t]*\[([^\]]*)\]", workflow)
    if not found:
        return []
    return [b.strip().strip("'\"") for b in found.group(1).split(",") if b.strip()]


def triggered(workflow: str, event: str, ref: str) -> bool:
    """Whether the workflow runs at all for this event and ref."""
    on = workflow.split("\non:\n", 1)[1].split("\n\n", 1)[0]
    if not re.search(rf"(?m)^  {event}:", on):
        return False
    if event != "push":
        return True
    branch = ref.removeprefix("refs/heads/")
    return any(re.fullmatch(b.replace("**", ".*"), branch) for b in push_branches(workflow))


def holds(condition: str | None, event: str, ref: str) -> bool:
    """A GitHub `if:` of the forms these workflows use (`==`, `!=`, `&&`, `||`,
    `!`, parentheses over `github.event_name` and `github.ref`, and the status
    functions `always()`, `success()`, `failure()`, `cancelled()`), evaluated."""
    if not condition:
        return True
    python = re.sub(r"!(?!=)", " not ", condition.replace("&&", " and ").replace("||", " or "))
    assert re.fullmatch(r"[\w\s.'=!()/*-]+", python), condition
    # A step's status functions, as on a run where every earlier step passed.
    status = {"always": lambda: True, "success": lambda: True, "failure": lambda: False, "cancelled": lambda: False}
    scope = {"github": types.SimpleNamespace(event_name=event, ref=ref), **status}
    return bool(eval(python, {"__builtins__": {}}, scope))


def job_if(job: str) -> str | None:
    found = re.search(r"(?m)^    if:[ \t]*(.+)$", job.split("\n    steps:", 1)[0])
    return found.group(1).strip() if found else None


def running(workflow: str, event: str, ref: str) -> list[tuple[str, int, str]]:
    """Each step that runs for the event: (job, its index, its name or action)."""
    if not triggered(workflow, event, ref):
        return []
    out = []
    for name, job in jobs(workflow).items():
        if not holds(job_if(job), event, ref):
            continue
        for n, step in enumerate(steps(job)):
            if holds(step.get("if"), event, ref):
                out.append((name, n, step.get("name") or step.get("uses", "")))
    return out


class BranchPushesMeetThePullRequestsGates(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = (WORKFLOWS / "ci.yml").read_text()
        cls.quality = (WORKFLOWS / "quality.yml").read_text()
        cls.fallow = (WORKFLOWS / "fallow.yml").read_text()

    def ran(self, workflow, label):
        return {(job, what) for job, _, what in running(workflow, *dict((e[0], e[1:]) for e in EVENTS)[label])}

    def test_every_branch_ci_builds_runs_both_gates(self):
        built = [b for b in push_branches(self.ci) if b != "master"]
        self.assertTrue(built, "ci.yml builds branches beside master")
        self.assertEqual(sorted(set(built) - set(push_branches(self.quality))), [])
        self.assertEqual(sorted(set(built) - set(push_branches(self.fallow))), [])

    def test_ruffs_gates_run_where_each_should(self):
        pr_gate = ("python", "Refuse newly introduced Ruff errors")
        branch_gate = ("python", "Refuse Ruff errors a branch introduces since master")
        report = ("python", "Report existing Ruff baseline")
        expected = {
            "pull request": {pr_gate},
            "push to master": {report},
            "push to wip": {branch_gate},
            "push to track": {branch_gate},
            "schedule": {report},
            "dispatch on master": {report},
            "dispatch on a branch": {report},
        }
        for label, want in expected.items():
            with self.subTest(label):
                ran = self.ran(self.quality, label) & {pr_gate, branch_gate, report}
                self.assertEqual(ran, want)
        gate = next(s for j in jobs(self.quality).values() for s in steps(j) if s.get("name") == branch_gate[1])
        self.assertIn('base="$(git merge-base origin/master HEAD)"', gate["run"])
        self.assertIn('--base "$base"', gate["run"])

    def test_fallows_audit_runs_on_a_branch_push_against_its_merge_base(self):
        audit = jobs(self.fallow)["audit"]
        listed = steps(audit)
        index = {s.get("id") or s.get("name") or s.get("uses"): n for n, s in enumerate(listed)}
        pushed = [n for n, s in enumerate(listed) if "changed-since" in s.get("with", "")]
        self.assertEqual(len(pushed), 1)
        self.assertLess(index["base"], pushed[0], "the base is found before the audit reads it")
        self.assertIn("${{ steps.base.outputs.sha }}", listed[pushed[0]]["with"])
        self.assertIn('sha="$(git merge-base origin/master HEAD)"', listed[index["base"]]["run"])
        for label, event, ref in EVENTS:
            with self.subTest(label):
                ran = {n for job, n, _ in running(self.fallow, event, ref) if job == "audit"}
                if event == "push" and ref != "refs/heads/master":
                    self.assertTrue({index["base"], pushed[0]} <= ran)
                if event == "pull_request":
                    self.assertNotIn(pushed[0], ran)
                    self.assertTrue(any("fallow-rs/fallow" in listed[n].get("uses", "") for n in ran))
        for n in (index["base"], pushed[0]):
            self.assertIn("gate: new-only", listed[pushed[0]]["with"])

    def test_the_weekly_report_runs_on_its_schedule_and_a_dispatch_alone(self):
        for label, event, ref in EVENTS:
            with self.subTest(label):
                weekly = [step for job, _, step in running(self.fallow, event, ref) if job == "weekly"]
                self.assertEqual(bool(weekly), event in ("schedule", "workflow_dispatch"))

    def test_a_dispatch_and_a_push_are_never_one_concurrency_group(self):
        group = re.search(r"(?m)^  group:[ \t]*(.+)$", self.quality).group(1)
        self.assertIn("github.event_name", group)
        self.assertIn("github.ref", group)

    def test_the_readings_tell_a_condition_never_true_from_one_that_holds(self):
        self.assertTrue(holds("github.event_name == 'push' && github.ref != 'refs/heads/master'", "push", "refs/heads/wip/x"))
        self.assertFalse(holds("github.event_name == 'push' && github.ref == 'refs/heads/master' && github.ref != 'refs/heads/master'", "push", "refs/heads/master"))
        self.assertTrue(holds("github.event_name != 'pull_request' && (github.event_name != 'push' || github.ref == 'refs/heads/master')", "schedule", "refs/heads/master"))
        self.assertFalse(holds("!(github.event_name == 'push')", "push", "refs/heads/wip/x"))
        self.assertEqual(
            push_branches('on:\n  push:\n    # a comment\n    branches: [master, "track/**", \'wip/**\']\n'),
            ["master", "track/**", "wip/**"],
        )
        broken = self.quality.replace(
            "if: github.event_name == 'push' && github.ref != 'refs/heads/master'",
            "if: github.event_name == 'push' && github.ref == 'refs/heads/master' && github.ref != 'refs/heads/master'",
        )
        self.assertNotIn(("python", "Refuse Ruff errors a branch introduces since master"),
                         {(j, w) for j, _, w in running(broken, "push", "refs/heads/wip/x")})


if __name__ == "__main__":
    unittest.main()
