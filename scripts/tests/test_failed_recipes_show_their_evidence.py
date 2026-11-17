"""**A recipe that fails still shows the evidence it wrote** (L5's second
review, 2026-11-17).

The justfile runs every recipe under `bash -euo pipefail` (`set shell`). A
recipe that writes its evidence with `{ ...; } > FILE 2>&1;` and only then
reads `status=$?` never gets there when the group fails: errexit stops the
shell at the group, so `cat FILE` never runs and the recipe says nothing but
just's own error line. The three upload recipes (`e14-generic-upload-http`,
`-leases`, `-claims`) did that. Their exit status was right, and CI's harness
prints a failed recipe's tail anyway, but a failing run on a developer's
machine showed nothing of why. Each now shows its evidence from an `EXIT`
trap, as the recipes beside them do. Not from the group's own `|| status=$?`
(the first repair): bash turns errexit off for everything left of `||`, so a
failure inside the group would be passed over while its last command
succeeded (#126's follow-up review, reproduced).

Each upload recipe is run here as `test_builds_before_suites.py` runs one: by
`just` under the justfile's own settings, in a tree of its own, each program
a stand-in. Its script's stand-in fails, and what the recipe says must hold
its evidence's header and what the script said.
"""

from __future__ import annotations

import pathlib
import re
import sys
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import ci_plan  # noqa: E402
from test_builds_before_suites import run_full  # noqa: E402

SAID = "the stand-in's script says why it failed"


def bare_statuses(body: str) -> list[str]:
    """The lines of a recipe that read `status=$?` as a statement of its own
    while errexit is on: a failed command before it stops the shell first.
    A trap's text runs as the shell exits, and `cmd || status=$?` is one
    statement errexit does not stop at; `set +e` turns errexit off until a
    `set -e`, statement by statement."""
    found = []
    errexit = True
    for line in body.splitlines():
        text = re.sub(r"trap '[^']*'", "trap", line)
        for statement in re.split(r";|&&|\bthen\b|\bdo\b", text):
            statement = statement.strip().lstrip("@").strip()
            if re.match(r"set\s+\+[a-z]*e", statement):
                errexit = False
            elif re.match(r"set\s+-[a-z]*e", statement):
                errexit = True
            elif errexit and re.match(r"status=\$\?", statement):
                found.append(line.strip())
    return found


# A group whose status a `||` takes: errexit is off inside it, so a failed
# command in it does not fail the recipe while its last one succeeds.
GROUP_TAKEN_BY_OR = re.compile(r"\}(?:\s*[0-9&]?>{1,2}\s*\S+)*\s*\|\|\s*status=\$\?")


def groups_taken_by_or(body: str) -> list[str]:
    """The groups of a recipe whose status `|| status=$?` takes."""
    joined = body.replace("\\\n", " ")
    return GROUP_TAKEN_BY_OR.findall(joined)


class FailedRecipesShowTheirEvidence(unittest.TestCase):
    def test_the_check_finds_a_status_errexit_skips_and_passes_the_ones_it_reaches(self):
        self.assertEqual(
            bare_statuses("{ a; } > f 2>&1; \\\n status=$?; cat f; exit $status"),
            ["status=$?; cat f; exit $status"],
        )
        self.assertEqual(bare_statuses("status=0; { a; } > f 2>&1 || status=$?; cat f; exit $status"), [])
        self.assertEqual(
            bare_statuses("trap 'status=$?; cat \"$f\" >&2; exit \"$status\"' EXIT; a"), []
        )
        self.assertEqual(bare_statuses("set +e; a; status=$?; set -e"), [])

    def test_no_recipe_reads_a_status_errexit_never_lets_it_reach(self):
        found = {name: bare_statuses(body) for name, body in ci_plan.bodies().items()}
        self.assertEqual({name: lines for name, lines in found.items() if lines}, {})

    def test_no_recipe_turns_errexit_off_for_a_group_by_taking_its_status(self):
        self.assertTrue(groups_taken_by_or("{ a; b; } > f 2>&1 || status=$?; cat f"))
        self.assertTrue(groups_taken_by_or("{ a; \\\n b; \\\n } > f 2>&1 || status=$?"))
        self.assertFalse(groups_taken_by_or("trap 'status=$?; cat f' EXIT; { a; } > f 2>&1"))
        self.assertFalse(groups_taken_by_or("cmd || status=$?"))
        found = {name: groups_taken_by_or(body) for name, body in ci_plan.bodies().items()}
        self.assertEqual({name: groups for name, groups in found.items() if groups}, {})

    def test_each_upload_recipe_shows_its_evidence_when_its_script_fails(self):
        for recipe, header in (
            ("e14-generic-upload-http", "generic-upload-http - "),
            ("e14-generic-upload-leases", "generic-upload-leases - "),
            ("e14-generic-upload-claims", "generic-upload-claims - "),
        ):
            with self.subTest(recipe):
                status, calls, said, _ = run_full(recipe)
                self.assertEqual(status, 0, said)
                at = [n for n, (name, _, _) in enumerate(calls, 1) if name == "python3"]
                self.assertEqual(len(at), 1, calls)
                status, _, said, evidence = run_full(recipe, fail_at=at[0], fail_says=SAID)
                self.assertNotEqual(status, 0, said)
                self.assertIn(header, said, "the recipe shows its evidence")
                self.assertIn(SAID, said, "and what its script said")
                written = [text for path, text in evidence.items() if path.endswith(".txt")]
                self.assertTrue(any(SAID in text for text in written), evidence.keys())


if __name__ == "__main__":
    unittest.main()
