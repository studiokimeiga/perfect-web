"""A recipe that skips without a database skips, and passes.

A recipe that runs against PostgreSQL says "skipped" and exits 0 where no
database is named, so a developer's `just ci`-adjacent run without one does
not fail. But `just` runs each recipe line in a shell of its own, and that
`exit 0` ends only the line it is in: a later line that summarises the
evidence (`@grep ... docs/evidence/E14/<recipe>.txt`) still ran, and failed
where no file was written, or printed an old run's summary as this one's.
#126's follow-up review found `e14-d3-postgres-3` failing so (its evidence
file new); the same check found `e14-d3-postgres`, `-2` and `e14-scheduled`
alike in a tree with no evidence (2026-11-17). Each summary now runs in the
line that writes its file.

Each recipe that says it skips is run here by `just` under the justfile's
own settings in a tree of its own (`test_builds_before_suites.py`), its
programs stand-ins, with no database named: it must pass, and call no
program but the ones its skip needs.
"""

from __future__ import annotations

import pathlib
import sys
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import ci_plan  # noqa: E402
from test_builds_before_suites import run_full  # noqa: E402

URLS = ("PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL", "PW_TABLES_DATABASE_URL")
SKIPS = ("skipped, PW_STORE_DATABASE_URL is not set", "skipped, PW_FEED_DATABASE_URL is not set")


def skippers() -> list[str]:
    return sorted(n for n, b in ci_plan.bodies().items() if any(s in b for s in SKIPS))


class RecipesSkipCleanly(unittest.TestCase):
    def test_each_recipe_that_skips_without_a_database_passes(self):
        names = skippers()
        self.assertGreaterEqual(len(names), 10, names)
        self.assertIn("e14-d3-postgres-3", names)
        for name in names:
            with self.subTest(name):
                status, _, said, evidence = run_full(name, unset=URLS)
                self.assertEqual(status, 0, said[-2000:])
                self.assertIn("skipped", said)
                self.assertEqual(sorted(evidence), [], "a skip writes no evidence")


if __name__ == "__main__":
    unittest.main()
