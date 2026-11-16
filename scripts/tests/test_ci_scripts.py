"""The verification run's scripts (ADR-0245), each on what it decides.

- `ci_plan.py` deals recipes into shards, the longest first, each where it
  ends soonest, its setup counted, by the seconds each last took, the same
  way every time; leaves out the measurements of a machine; gives the
  recipes run against a database shards and a PostgreSQL of their own, as
  many as end the run soonest; takes more shards than it is asked for where
  its number would end a shard past the ceiling under a shard's limit;
  reaches from a change to the mutation scripts it touches; and, as a push
  plans, runs no recipe whose committed evidence is current;
- `ci_recipes.py` takes what a recipe wrote to be the files whose bytes
  changed while it ran, and frees what its builds leave;
- `ci_summary.py` fails a run where a recipe failed, a mutant survived, or a
  shard reported nothing;
- `evidence_fetch.py` names the run after a file's `commit:` line, and
  refuses a file of another commit, a file whose recipe reads what changed
  since the run, against the tree here, and a run that failed anywhere but
  in the jobs it is told are known; and keeps the seconds of each recipe
  that ran to its end;
- `verified_base.py` plans a push to `master` from the newest commit on its
  first-parent line that a push's or the nightly's verify run passed at,
  and from the commit before the push where none did, or on any error.
"""

import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest import mock

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


plan = load("ci_plan")
recipes = load("ci_recipes")
fetch = load("evidence_fetch")
vbase = load("verified_base")


class Plan(unittest.TestCase):
    def test_the_costliest_go_first_each_to_where_it_ends_soonest(self) -> None:
        costs = {"a": 10, "b": 6, "c": 5, "d": 1}
        self.assertEqual([r for r, _, _ in plan.deal(list(costs), costs, 2)], [["a", "d"], ["b", "c"]])
        self.assertEqual(
            plan.plan(list(costs), costs, 2),
            [
                {"index": 0, "recipes": ["a", "d"], "browsers": False, "build": False, "database": False},
                {"index": 1, "recipes": ["b", "c"], "browsers": False, "build": False, "database": False},
            ],
        )

    def test_a_recipe_needing_less_may_run_where_more_is_set_up(self) -> None:
        # ADR-0249: a shard installs browsers, or builds, only for a recipe
        # dealt it that needs them. ADR-0290: a recipe that needs neither
        # runs in such a shard where it ends soonest there.
        costs = {"x": 4000, "p": 1000, "y": 500}
        need = {"p": (True, True)}
        self.assertEqual(
            [(s["recipes"], s["browsers"], s["build"]) for s in plan.plan(list(costs), costs, 2, need)],
            [(["x"], False, False), (["p", "y"], True, True)],
        )

    def test_what_a_recipe_adds_to_a_shards_setup_counts(self) -> None:
        # `q` would end sooner after `x`, but not with the browsers that
        # shard would install for it.
        costs = {"x": 1200, "b": 1000, "q": 100}
        need = {"b": (True, True), "q": (True, True)}
        dealt = plan.deal(list(costs), costs, 2, need)
        self.assertEqual([(r, k) for r, k, _ in dealt], [(["x"], (False, False)), (["b", "q"], (True, True))])
        setup = plan.SETUP
        self.assertEqual([end for _, _, end in dealt], [setup[(False, False)] + 1200, setup[(True, True)] + 1100])
        # Setting up a build, or browsers and a build, takes longer than a
        # plain shard's setup.
        self.assertLess(setup[(False, False)], setup[(False, True)])
        self.assertLessEqual(setup[(False, True)], setup[(True, True)])

    def test_what_a_recipe_needs_is_read_from_its_lines(self) -> None:
        self.assertEqual(plan.needs("    pnpm exec playwright test e2e/feed.spec.mjs\n"), (True, True))
        self.assertEqual(plan.needs("    ./target/debug/pw check a.pw\n"), (False, True))
        self.assertEqual(plan.needs("    BUILD_ONLY=1 bash spikes/own-renderer/run.sh\n"), (False, True))
        self.assertEqual(plan.needs("    cargo test -p pw-core --test names\n"), (False, False))

    def test_a_plan_is_the_same_every_time(self) -> None:
        costs = {f"e14-r{i}": i % 4 + 1 for i in range(40)}
        names = list(costs)
        first = plan.plan(names, costs, 16)
        self.assertEqual(first, plan.plan(list(reversed(names)), costs, 16))
        self.assertLessEqual(len(first), 16)
        dealt = sorted(r for s in first for r in s["recipes"])
        self.assertEqual(dealt, sorted(names))

    def test_no_shard_is_planned_empty(self) -> None:
        self.assertEqual(len(plan.plan(["a", "b"], {}, 16)), 2)
        # And nothing to run plans no shard: a change that touches no recipe
        # (run 37914111273 failed its plan instead, ADR-0290's first week).
        for database in (frozenset(), {"d1"}):
            with self.subTest(database=database):
                try:
                    empty = plan.plan([], {}, 9, database=database)
                except ValueError as error:
                    self.fail(f"An empty recipe selection must return no shards: {error}")
                self.assertEqual(empty, [])

    def test_a_recipe_recorded_past_a_jobs_limit_is_refused_before_any_shard(self) -> None:
        # The integrator's ruling 2 (train-10): verify 39054436657 stopped
        # `e14-refusal`, alone in its shard, at the 345-minute limit after
        # 20,507 s. A plan that knows a recipe's seconds refuses it then, not
        # a run hours in. Its shard's setup counts, and a note in the seconds
        # kept (`_lower_bounds`) is no recipe.
        import contextlib
        import io

        self.assertEqual(plan.past_the_limit(["a"], {"a": plan.LIMIT - 100}, {"a": (True, True)}), [("a", plan.LIMIT - 100)])
        self.assertEqual(plan.past_the_limit(["a"], {"a": plan.LIMIT - 100}, {"a": (False, False)}), [])
        self.assertEqual(plan.past_the_limit(["a"], {}, {"a": (True, True)}), [])
        names = ["e10-affine-bindings", "e10-attribute-case"]
        real, argv = plan.SECONDS, sys.argv
        try:
            with tempfile.TemporaryDirectory() as d:
                plan.SECONDS = pathlib.Path(d) / "seconds.json"
                sys.argv = ["ci_plan.py", *names]
                for recorded, refused in ((plan.LIMIT, True), (plan.LIMIT - 1_000, False)):
                    plan.SECONDS.write_text(json.dumps({
                        "_lower_bounds": {names[0]: "stopped at the limit"},
                        names[0]: recorded,
                        names[1]: 100,
                    }))
                    out, err = io.StringIO(), io.StringIO()
                    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                        try:
                            code = plan.main()
                        except TypeError as error:
                            self.fail(f"a note in the seconds kept is read as a recipe's: {error}")
                    if refused:
                        self.assertEqual(code, 1, err.getvalue())
                        self.assertIn(f"{names[0]} is recorded at {recorded} s", err.getvalue())
                        self.assertNotIn("shards=", out.getvalue())
                    else:
                        self.assertEqual(code, 0, err.getvalue())
                        self.assertIn("shards=", out.getvalue())
        finally:
            plan.SECONDS, sys.argv = real, argv

    def test_a_recipe_that_skips_without_a_database_is_dealt_beside_one(self) -> None:
        # Found at train-10's verify (39054436657): `e14-scheduled-orders`
        # says it skipped and writes no evidence where no database is named,
        # NEEDS_DATABASE did not hold it, and its shard had none: the run
        # recorded nothing of ADR-0347 and passed. Every recipe of that kind,
        # read from the justfile's every part, is dealt beside a database.
        skips = [
            name
            for name, body in plan.bodies().items()
            if "skipped, PW_STORE_DATABASE_URL is not set" in body
            or "skipped, PW_FEED_DATABASE_URL is not set" in body
        ]
        self.assertIn("e14-scheduled-orders", skips)
        self.assertGreaterEqual(len(skips), 10, skips)
        self.assertEqual(sorted(set(skips) - set(plan.NEEDS_DATABASE)), [])

    def test_a_part_of_a_scripts_mutants_costs_its_mutants_alone(self) -> None:
        # The second review of L5 (2026-11-17): `--part i/n` ran one part,
        # and the plan costed it as the script's every mutant, so both of
        # d3's parts cost 136; a measured part's unit then looked cheaper
        # than it was, and every estimate of its kind with it.
        import d3_postgres_mutations as d3
        import refusal_mutations as refusal

        body = plan.bodies()
        self.assertEqual(plan.cost(body["e14-d3-postgres"]), 1 + len(d3.DECLARED))
        self.assertEqual(plan.cost(body["e14-d3-postgres-2"]), 1 + len(d3.UPLOADS))
        first, second = refusal.PARTS
        self.assertEqual(plan.cost(body["e14-refusal-2"]), 1 + second[1] - second[0])
        # A part a script does not define costs the whole, as before.
        self.assertEqual(
            plan.cost("python3 scripts/d3_postgres_mutations.py --part 3/2"), 1 + len(d3.MUTANTS)
        )

    def test_a_recipe_that_reads_a_database_is_dealt_beside_one(self) -> None:
        # Found before L5's full verify (2026-11-17): `e14-generic-upload-claims`
        # is refused without a database ("refused, it needs
        # PW_TABLES_DATABASE_URL"), words the test above does not read, and
        # NEEDS_DATABASE did not hold it: the run would have dealt it where
        # there is none, and failed it. And `e14-every-entry` and
        # `e14-follows`, which only said whether one is named, ran tests that
        # said "skipped" and passed without one. A recipe that names a
        # database's URL at all is dealt beside one.
        named = ("PW_TABLES_DATABASE_URL", "PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL")
        reads = sorted(name for name, body in plan.bodies().items() if any(v in body for v in named))
        for name in ("e14-generic-upload-claims", "e14-d3-postgres-2", "e14-every-entry", "e14-follows"):
            self.assertIn(name, reads)
        self.assertEqual(sorted(set(reads) - set(plan.NEEDS_DATABASE)), [])

    def test_a_measurement_of_the_machine_is_not_planned(self) -> None:
        self.assertIn("e10-close-bench", plan.LOCAL_ONLY)
        self.assertTrue(plan.EVIDENCE_RECIPE.match("e14-feed"))
        self.assertFalse(plan.EVIDENCE_RECIPE.match("ci"))

    def test_a_change_reaches_the_scripts_it_touches(self) -> None:
        # The script itself.
        self.assertIn(
            "statements_separated_mutations.py",
            plan.touched_scripts({"scripts/statements_separated_mutations.py": [(1, 1)]}, 30),
        )
        # A test file a script runs.
        self.assertIn(
            "each_heads_mutations.py",
            plan.touched_scripts({"compiler/pw-core/tests/each_heads.rs": [(1, 1)]}, 30),
        )
        # A line near a mutant's anchor, and not one far from every anchor.
        grammar = "compiler/pw-syntax/src/grammar.rs"
        source = (plan.ROOT / grammar).read_text()
        line = source[: source.find("let mut separated = true;")].count("\n") + 1
        self.assertIn(
            "statements_separated_mutations.py",
            plan.touched_scripts({grammar: [(line + 20, line + 20)]}, 30),
        )
        self.assertNotIn(
            "statements_separated_mutations.py",
            plan.touched_scripts({grammar: [(1, 1)]}, 30),
        )

    def test_a_changed_server_test_module_reaches_what_runs_it_by_its_filter(self) -> None:
        # Train-6's finding, 2026-10-14: the server's test modules run by a
        # filter, `tests::store_scheduled::`, never by `--test`, and a change
        # to one planned nothing: the push of a fix to two of its tests would
        # have run no recipe of them, and the fetch would have taken their
        # old evidence as current.
        module = "spikes/own-renderer/server/src/tests/store_scheduled.rs"
        self.assertIn("scheduled_mutations.py", plan.touched_scripts({module: [(1, 1)]}, 30))
        planned = plan.recipes_for({module: [(1, 1)]}, 30, plan.recipes())
        self.assertIn("e14-scheduled", planned)
        # And a recipe that runs it by its filter with no script of its own:
        # today every such recipe runs a script that names the filter too.
        saved = plan.bodies
        try:
            plan.bodies = lambda: {"e14-direct": "cargo test -- tests::store_scheduled::"}
            self.assertEqual(plan.recipes_for({module: [(1, 1)]}, 30, ["e14-direct"]), ["e14-direct"])
        finally:
            plan.bodies = saved
        # The control: a module nothing runs plans nothing by it.
        nothing = "spikes/own-renderer/server/src/tests/nothing_runs_this.rs"
        self.assertNotIn("scheduled_mutations.py", plan.touched_scripts({nothing: [(1, 1)]}, 30))
        self.assertNotIn("e14-scheduled", plan.recipes_for({nothing: [(1, 1)]}, 30, plan.recipes()))

    def test_a_change_to_a_sources_own_tests_reaches_what_runs_them(self) -> None:
        # Train-6's finding, 2026-10-14: the test that kills its survivor sits
        # in `csp.rs`'s own `mod tests`, far from every mutant, and is no test
        # file: its change planned no `e14-page-policy`, whose script runs
        # `csp::`.
        csp = "spikes/own-renderer/server/src/csp.rs"
        lines = (plan.ROOT / csp).read_text().splitlines()
        # Its last test's lines, far from every mutant, which the near rule
        # would plan by itself.
        inside = len(lines) - 2
        changed = {csp: [(inside, inside)]}
        self.assertEqual(plan.test_modules(changed), {"csp"})
        self.assertIn("page_policy_mutations.py", plan.touched_scripts(changed, 30))
        saved = plan.bodies
        try:
            plan.bodies = lambda: {"e14-direct": "cargo test -- csp::tests"}
            self.assertEqual(plan.recipes_for(changed, 30, ["e14-direct"]), ["e14-direct"])
        finally:
            plan.bodies = saved
        # The control: the same file's first line is no test's, and a path
        # that ends in the module's name is another module.
        self.assertEqual(plan.test_modules({csp: [(1, 1)]}), set())
        self.assertFalse(plan.names_module("pw_core::csp::Needs", "csp"))
        self.assertTrue(plan.names_module('"csp::", "tests::page_policy::"', "csp"))

    def test_root_test_changes_reach_the_package_test_consumers(self) -> None:
        # Crate roots do not contribute `main::` or `lib::` to a test name.
        # Select the package conservatively, including bare-name filters.
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "sample/src").mkdir(parents=True)
            (root / "scripts").mkdir()
            (root / "sample/Cargo.toml").write_text('[package]\nname = "sample-crate"\n')
            source = 'fn production() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn witness() {}\n}\n'
            (root / "sample/src/main.rs").write_text(source)
            (root / "scripts/root_mutations.py").write_text(
                'COMMAND = ["cargo", "test", "-p", "sample-crate", "witness"]\nMUTANTS = []\n'
            )
            (root / "scripts/other_mutations.py").write_text(
                'COMMAND = ["cargo", "test", "-p", "other-crate"]\nMUTANTS = []\n'
            )
            bodies = {
                "direct": "cargo test --package=sample-crate witness",
                "workspace": "cargo test --workspace",
                "script": "python3 scripts/root_mutations.py",
                "other": "cargo test -p other-crate witness",
                "prefix": "cargo test -p sample-crate-extra witness",
            }
            with mock.patch.object(plan, "ROOT", root), mock.patch.object(plan, "bodies", return_value=bodies), mock.patch.object(plan, "justfile_parts", return_value=[]):
                for name in ("main.rs", "lib.rs"):
                    path = f"sample/src/{name}"
                    (root / path).write_text(source)
                    changed = {path: [(5, 5)]}
                    with self.subTest(source=name):
                        self.assertEqual(plan.touched_scripts(changed, 0), {"root_mutations.py"})
                        self.assertEqual(plan.recipes_for(changed, 0, list(bodies)), ["direct", "script", "workspace"])
                        self.assertEqual(plan.touched_scripts({path: [(1, 1)]}, 0), set())

    def test_an_out_of_line_test_module_reaches_what_runs_it(self) -> None:
        # The nightly of 2026-11-15's finding: `pw-table-memory`'s
        # `failure_tests.rs` is a whole test module, declared out of line
        # (`#[cfg(test)] mod failure_tests;` in `lib.rs`), neither under a
        # `tests/` directory nor an inline `mod tests {`, and a change to it
        # planned nothing: the fix to its one test that the nightly found
        # INVALID would have run no `e14-d2-writes`, whose script runs it.
        failure = "runtime/pw-table-memory/src/failure_tests.rs"
        self.assertIn("d2_writes_mutations.py", plan.touched_scripts({failure: [(1, 1)]}, 30))
        self.assertIn("e14-d2-writes", plan.recipes_for({failure: [(1, 1)]}, 30, plan.recipes()))
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "sample/src/inner").mkdir(parents=True)
            (root / "scripts").mkdir()
            (root / "sample/Cargo.toml").write_text('[package]\nname = "sample-crate"\n')
            (root / "sample/src/lib.rs").write_text(
                "pub mod inner;\nmod helpers;\n#[cfg(test)]\nmod probe_tests;\n"
                "#[cfg(test)]\npub(crate) mod tests;\n"
            )
            (root / "sample/src/inner.rs").write_text("#[cfg(test)]\n#[allow(dead_code)]\nmod inner_tests;\n")
            for module in ("probe_tests", "tests", "helpers", "inner/inner_tests"):
                (root / f"sample/src/{module}.rs").write_text("#[test]\nfn witness() {}\n")
            (root / "scripts/root_mutations.py").write_text(
                'COMMAND = ["cargo", "test", "-p", "sample-crate", "--lib"]\nMUTANTS = []\n'
            )
            bodies = {
                "package": "cargo test --locked -p sample-crate",
                "probe": "cargo test -- probe_tests::witness",
                "inner": "cargo test -- inner::",
                "inner-tests": "cargo test -- inner_tests::witness",
                "script": "python3 scripts/root_mutations.py",
                "other": "cargo test -p other-crate -- tests::elsewhere::",
            }
            with mock.patch.object(plan, "ROOT", root), mock.patch.object(plan, "bodies", return_value=bodies), mock.patch.object(plan, "justfile_parts", return_value=[]):
                for module, modules, packages, scripts, planned in (
                    # Declared by the crate root: its package's tests, and its
                    # own filter.
                    ("probe_tests", {"probe_tests"}, {"sample-crate"}, {"root_mutations.py"},
                     ["package", "probe", "script"]),
                    # `tests` as a filter would be every crate's: its package.
                    ("tests", set(), {"sample-crate"}, {"root_mutations.py"}, ["package", "script"]),
                    # Declared by another module: its filter and its parent's,
                    # as an inline `mod tests` is its file's.
                    ("inner/inner_tests", {"inner_tests", "inner"}, set(), set(), ["inner", "inner-tests"]),
                    # The control: a module no `#[cfg(test)]` declares is no
                    # test module, whatever it holds.
                    ("helpers", set(), set(), set(), []),
                ):
                    changed = {f"sample/src/{module}.rs": [(1, 1)]}
                    with self.subTest(module=module):
                        self.assertEqual(plan.test_modules(changed), modules)
                        self.assertEqual(plan.root_test_packages(changed), packages)
                        self.assertEqual(plan.touched_scripts(changed, 0), scripts)
                        self.assertEqual(plan.recipes_for(changed, 0, list(bodies)), planned)

    def test_python_test_changes_reach_scripts_and_direct_recipes(self) -> None:
        path = "scripts/tests/test_ci_scripts.py"
        changed = {path: [(1, 1)]}
        self.assertIn("evidence_integrity_mutations.py", plan.touched_scripts(changed, 0))
        self.assertIn("e14-evidence-integrity", plan.recipes_for(changed, 0, plan.recipes()))
        bodies = {
            "path": f"python3 -m unittest {path}",
            "module": "python3 -m unittest scripts.tests.test_ci_scripts.Plan",
            "discovery": "python3 -m unittest discover -s scripts/tests -p test_ci_scripts.py",
            "other": "python3 -m unittest scripts/tests/test_ci_scripts_extra.py",
            "other-module": "python3 -m unittest scripts.tests.test_ci_scripts_extra",
        }
        with mock.patch.object(plan, "bodies", return_value=bodies):
            self.assertEqual(plan.recipes_for(changed, 0, list(bodies)), ["discovery", "module", "path"])
        unknown = {"scripts/tests/test_nobody_runs_this.py": [(1, 1)]}
        self.assertEqual(plan.touched_scripts(unknown, 0), set())

    def test_bare_python_module_inventories_reach_their_campaigns(self) -> None:
        changed = {"scripts/tests/test_ci_scripts.py": [(1, 1)]}
        self.assertIn("mutation_bound_mutations.py", plan.touched_scripts(changed, 0))
        self.assertIn("e14-mutation-bound", plan.recipes_for(changed, 0, plan.recipes()))
        bodies = {
            "bare": 'SEARCH = "scripts/tests"; SELECTORS = ("test_ci_scripts",)',
            "class": 'SEARCH = "scripts/tests"; SELECTORS = ("test_ci_scripts.Plan",)',
            "other": 'SEARCH = "scripts/tests"; SELECTORS = ("test_ci_scripts_extra",)',
            "other-directory": 'SEARCH = "elsewhere/tests"; SELECTORS = ("test_ci_scripts",)',
        }
        with mock.patch.object(plan, "bodies", return_value=bodies):
            self.assertEqual(plan.recipes_for(changed, 0, list(bodies)), ["bare", "class"])

    def test_a_changed_browser_spec_reaches_the_scripts_and_recipes_that_run_it(self) -> None:
        # ADR-0281: a spec a script runs is its test, as a Rust test file is;
        # a change to `accessibility.spec.mjs` planned nothing before.
        spec = "spikes/own-renderer/e2e/accessibility.spec.mjs"
        self.assertIn("accessibility_mutations.py", plan.touched_scripts({spec: [(1, 1)]}, 30))
        self.assertEqual(plan.changed_specs({spec: [(1, 1)]}), {"accessibility.spec.mjs"})
        # And each recipe that runs it: the one that names it in its own
        # lines, and the one that runs a script that runs it.
        planned = plan.recipes_for({spec: [(1, 1)]}, 30, plan.recipes())
        self.assertIn("e14-accessibility", planned)
        self.assertIn("e14-titles", planned)
        self.assertNotIn(
            "accessibility_mutations.py",
            plan.touched_scripts({"spikes/own-renderer/e2e/nothing.spec.mjs": [(1, 1)]}, 30),
        )

    def test_a_change_to_the_suites_configuration_plans_every_recipe_that_runs_a_suite_under_it(self) -> None:
        # The integrator's ruling 7 (2026-10-15): every spec runs under
        # `playwright.config.mjs`, its hosts and its checks of the builds; a
        # change to it planned only a recipe whose mutant's anchor sat near.
        names = plan.recipes()
        body = plan.bodies()
        planned = plan.recipes_for({plan.PLAYWRIGHT_CONFIG: [(100, 100)]}, 30, names)
        under = sorted(n for n in names if plan.runs_browser_suite(body.get(n, "")))
        self.assertGreater(len(under), 80)
        self.assertEqual(sorted(set(under) - set(planned)), [])
        for name in (
            "e14-feed",  # its own lines, by the helper
            "e14-keepalive",  # two suites in its lines
            "e14-resume-gate",  # a suite in a loop
            "e10-browser",  # every spec, three runs
            "e14-speculated-values",  # only by its mutation script
        ):
            self.assertIn(name, planned, name)
        # The benchmark harness's suite runs under its own configuration,
        # and a recipe that runs no browser is not planned.
        self.assertNotIn("e14-contract", planned)
        self.assertNotIn("e8-wit", planned)
        self.assertFalse(plan.runs_browser_suite(body["e14-contract"]))
        self.assertTrue(plan.runs_browser_suite(body["e14-contract"], "benchmarks/harness"))
        # The control: another file of the spike plans none of them by this.
        other = plan.recipes_for({"spikes/own-renderer/README.md": [(1, 1)]}, 30, names)
        self.assertNotIn("e14-feed", other)

    def test_the_modules_the_configuration_reads_are_its_own_and_plan_as_it_does(self) -> None:
        # The host map (ADR-0356): the configuration plans each run's hosts
        # with modules of its directory and the registry they read, found by
        # following its imports; a change to any of them is a change to every
        # suite run there, as a change to the configuration is.
        found = plan.suite_configuration()
        self.assertEqual(
            sorted(found),
            [
                f"{plan.SUITE_DIRECTORY}/{name}"
                for name in ("host-metadata.mjs", "host-plan-reporter.mjs", "host-plan.mjs", "host-registry.json", "playwright.config.mjs")
            ],
        )
        names = plan.recipes()
        body = plan.bodies()
        under = sorted(n for n in names if plan.runs_browser_suite(body.get(n, "")))
        for module in sorted(found - {plan.PLAYWRIGHT_CONFIG}):
            planned = plan.recipes_for({module: [(1, 1)]}, 30, names)
            self.assertEqual(sorted(set(under) - set(planned)), [], module)
        # Followed through what each module imports or names beside itself,
        # and not into what it only reads by a path of the tree.
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            suite = root / plan.SUITE_DIRECTORY
            suite.mkdir(parents=True)
            (suite / "playwright.config.mjs").write_text('import { a } from "./a.mjs";\nimport { defineConfig } from "@playwright/test";\n')
            (suite / "a.mjs").write_text('const b = await import("./b.mjs");\nconst data = new URL("./c.json", import.meta.url);\nreadFileSync("public/d.mjs");\n')
            (suite / "b.mjs").write_text('export { a } from "./a.mjs";\n')
            self.assertEqual(
                sorted(plan.suite_configuration(root)),
                [f"{plan.SUITE_DIRECTORY}/{name}" for name in ("a.mjs", "b.mjs", "c.json", "playwright.config.mjs")],
            )

    def test_a_changed_spec_or_a_module_its_specs_import_plans_what_runs_them(self) -> None:
        # Track evidence-integrity-2: `e2e/accessibility-rules.mjs`, which
        # three specs import, planned nothing; and `e10-browser`, which runs
        # every spec, was planned by none.
        names = plan.recipes()
        rules = {"spikes/own-renderer/e2e/accessibility-rules.mjs": [(1, 1)]}
        self.assertEqual(
            plan.changed_specs(rules),
            {"accessibility.spec.mjs", "payments.spec.mjs", "store-search.spec.mjs", "scheduled-orders.spec.mjs"},
        )
        planned = plan.recipes_for(rules, 30, names)
        for name in ("e14-accessibility", "e14-titles", "e14-payments", "e14-store-search", "e14-scheduled-orders", "e10-browser"):
            self.assertIn(name, planned, name)
        self.assertNotIn("e14-feed", planned)
        feed = plan.recipes_for({"spikes/own-renderer/e2e/feed.spec.mjs": [(1, 1)]}, 30, names)
        self.assertIn("e10-browser", feed)
        self.assertTrue(plan.runs_every_spec(plan.bodies()["e10-browser"]))
        self.assertFalse(plan.runs_every_spec(plan.bodies()["e14-feed"]))
        # The control: a module no spec imports reaches no spec.
        self.assertEqual(plan.changed_specs({"spikes/own-renderer/e2e/nothing.mjs": [(1, 1)]}), set())

    def test_a_recipe_is_planned_where_its_script_reads_what_changed(self) -> None:
        # W8's finding, 2026-10-12: holds' merge changed the benchmark's
        # baseline store, and no verify planned `e14-diffs`, whose script
        # applies each task's patches to it. A change to the baseline, to a
        # task's patch, or to the script itself plans it now.
        names = plan.recipes()
        for changed in [
            "benchmarks/baselines/pleris/store/app.pw",
            "benchmarks/tasks/T11-confirm-clear/reference/pleris.patch",
            "scripts/semantic_diffs.py",
        ]:
            self.assertIn("e14-diffs", plan.recipes_for({changed: [(1, 1)]}, 30, names), changed)
        self.assertIn(
            "benchmarks/baselines/pleris",
            plan.data_inputs(plan.ROOT / "scripts/semantic_diffs.py"),
        )
        # The control: what it does not read plans nothing of it.
        self.assertNotIn(
            "e14-diffs",
            plan.recipes_for({"benchmarks/baselines/sveltekit/README.md": [(1, 1)]}, 30, names),
        )

    def test_a_recipe_is_planned_where_its_own_lines_name_a_program_that_changed(self) -> None:
        # Train-5's finding (2026-10-14): `e10-build` builds
        # `examples/store/*.pw` in its own lines, and no push planned it
        # since 2026-10-12; its evidence named an import ADR-0332 removed.
        names = plan.recipes()
        planned = plan.recipes_for({"examples/store/app.pw": [(1, 1)]}, 30, names)
        self.assertIn("e10-build", planned)
        self.assertIn("e8-wit", planned)
        self.assertIn("examples/store/*.pw", plan.body_inputs(plan.bodies()["e10-build"]))
        # A glob's star stays in its segment, as a shell's does; a path
        # names what is under it.
        self.assertTrue(plan.names_path("examples/*.pw", "examples/domain.pw"))
        self.assertFalse(plan.names_path("examples/*.pw", "examples/store/app.pw"))
        self.assertTrue(plan.names_path("examples/store/*.pw", "examples/store/app.pw"))
        self.assertTrue(
            plan.names_path("examples/kiokun/data/han-1char-3", "examples/kiokun/data/han-1char-3/a.json")
        )
        self.assertFalse(plan.names_path("examples/kiokun/data/han", "examples/kiokun/data/han-1char-3"))
        # The control: a program no recipe's lines name plans none of them.
        self.assertNotIn(
            "e10-build",
            plan.recipes_for({"examples/feed/app.pw": [(1, 1)]}, 30, names),
        )

    def test_a_recipe_is_planned_where_a_shell_script_it_runs_changed(self) -> None:
        # Train-3's run (2026-10-13) planned no `e8-host` for a change to
        # `audit.sh`, the script the recipe is: only a `scripts/*.py` was read.
        names = plan.recipes()
        self.assertIn(
            "e8-host",
            plan.recipes_for({"spikes/wasmtime-component/audit.sh": [(1, 1)]}, 30, names),
        )
        # A script many recipes run plans each of them.
        built = plan.recipes_for({"spikes/own-renderer/run.sh": [(1, 1)]}, 30, names)
        self.assertIn("e14-page-parameters", built)
        # The control: a script no recipe runs plans nothing.
        self.assertEqual(
            plan.recipes_for({"spikes/wasmtime-component/elsewhere.sh": [(1, 1)]}, 30, names),
            [],
        )

    def test_a_changed_binary_is_named_where_a_line_names_it(self) -> None:
        # Train-10's landing (2026-10-20): git diffs a binary with no `+++`
        # line and no hunk, so 8c5618c8, which regenerated three of the
        # store's components, planned no recipe and staled no evidence.
        diff = (
            b"diff --git a/d/x.wasm b/d/x.wasm\nindex 1..2 100644\n"
            b"Binary files a/d/x.wasm and b/d/x.wasm differ\n"
            b"diff --git a/n.bin b/n.bin\nnew file mode 100644\n"
            b"Binary files /dev/null and b/n.bin differ\n"
            b"diff --git a/g/o.bin b/g/o.bin\ndeleted file mode 100644\n"
            b"Binary files a/g/o.bin and /dev/null differ\n"
        )
        self.assertEqual(plan.binaries(diff), ["d/x.wasm", "g/o.bin", "n.bin"])
        self.assertEqual(plan.parsed(diff), {})
        for line, named in [
            ('std::fs::read(root().join("d/x.wasm"))', True),
            ('let problems = components_against(&built, &root.join("d"));', True),
            ('rm -f d/*.wasm; cp "$out"/components/*.wasm d/; \\', True),
            ('let path = root().join(format!("d/{id}.wasm"));', True),
            ('cmp "$out/$id.wasm" "d/$id.wasm"', True),
            ("include_bytes!(\"../../d/${ID}.wasm\")", True),
            # The controls: another file, a directory in it or beside it,
            # another kind of file, another directory ending in its name.
            ("d/y.wasm", False),
            ('let dir = root.join("d/handlers");', False),
            ("d/kiokun/x.wasm", False),
            ("d/*/x.wasm", False),
            ("d0/x.wasm", False),
            ("dd/x.wasm", False),
            ("d/*.json", False),
        ]:
            self.assertEqual(plan.names_binary(line, "d", "x.wasm"), named, line)

    def test_a_changed_component_plans_the_recipes_whose_tests_read_it(self) -> None:
        # On this tree: a committed component is read by `evidence_is_current`
        # (whose scripts' recipes, e14-committed-components and others, run
        # it), by pw-host's tests, by the server, and by e10-component's own
        # lines, which write it.
        names = plan.recipes()
        component = "docs/evidence/E10/store.hours.CartHours.wasm"
        readers = plan.read_by(component, None)
        self.assertIn("compiler/pw-core/tests/evidence_is_current.rs", readers)
        self.assertIn("runtime/pw-host/tests/display.rs", readers)
        self.assertEqual([p for p in readers if p.startswith(plan.RECORDS)], [], "a record reads nothing")
        planned = plan.recipes_for(plan.with_readers({}, [component], None), 30, names)
        for recipe in ("e14-committed-components", "e10-component", "e14-last-known-good"):
            self.assertIn(recipe, planned)
        # The control: the component alone, as git's diff gave it, plans
        # nothing (train-10's 72 recipes chosen by hand).
        self.assertEqual(plan.recipes_for({component: []}, 30, names), [])

    def test_a_binary_changed_in_a_range_or_the_tree_here_is_changed_where_it_is_read(self) -> None:
        # `hunks`, end to end, on a git of its own: the range's and the
        # tree's own changes, as the planner and the fetch's stale check ask.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)

            def git(*args: str) -> str:
                return subprocess.run(
                    ["git", *args], cwd=root, check=True, capture_output=True, text=True
                ).stdout.strip()

            git("init", "-q")
            git("config", "user.email", "toy@example.com")
            git("config", "user.name", "toy")
            (root / "d").mkdir()
            (root / "d" / "x.bin").write_bytes(b"\x00one")
            (root / "reader.rs").write_text('fn f() {}\nlet b = include_bytes!("d/x.bin");\n')
            (root / "other.rs").write_text('let h = "d/handlers";\n')
            git("add", "-A")
            git("commit", "-q", "-m", "one")
            first = git("rev-parse", "HEAD")
            (root / "d" / "x.bin").write_bytes(b"\x00two")
            expected = {"d/x.bin": [], "reader.rs": [(2, 2)]}
            self.assertEqual(plan.hunks(first, None, root), expected)
            git("commit", "-q", "-am", "two")
            self.assertEqual(plan.hunks(first, "HEAD", root), expected)
            # A text file's change is as it was: its own lines alone.
            (root / "other.rs").write_text('let h = "d/handlers";\nlet i = 1;\n')
            self.assertEqual(plan.hunks("HEAD", None, root), {"other.rs": [(2, 2)]})

    def test_a_recipe_in_an_imported_file_is_read_and_planned(self) -> None:
        # ADR-0253: a track's recipes live in a file the justfile imports.
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "just").mkdir()
            (root / "justfile").write_text(
                "set positional-arguments\n\nimport 'just/t.just'\nimport? 'just/none.just'\n\n"
                "e14-root:\n    cargo test -p pw-core --test names\n"
            )
            (root / "just/t.just").write_text(
                "# A track's.\ne14-track:\n    python3 scripts/track_mutations.py\n"
            )
            self.assertEqual(plan.justfile_parts(root), ["justfile", "just/t.just"])
            body = plan.bodies(root)
            self.assertIn("scripts/track_mutations.py", body["e14-track"])
            self.assertIn("--test names", body["e14-root"])
            # A changed line of the imported file is its recipe's.
            text = (root / "just/t.just").read_text().splitlines()
            self.assertEqual(plan.recipes_at(text, [(3, 3)], ["e14-track"]), {"e14-track"})
            self.assertEqual(plan.recipes_at(text, [(1, 1)], ["e14-track"]), set())

    def test_a_recipe_whose_parameters_have_defaults_is_read(self) -> None:
        # Track evidence-integrity-2: a header's default stopped the header's
        # pattern until 2026-10-15, and `e10-browser`, `e10-close-bench` and
        # `e14-harness` had no body: nothing they run planned them, and a
        # shard dealt one set up neither browsers nor the build.
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "justfile").write_text(
                'set shell := ["bash", "-euo", "pipefail", "-c"]\nexport PATH := "x:" + env_var(\'PATH\')\n'
                "toolchain_bin := justfile_directory()\n\n"
                'e10-browser engines="chromium firefox" out="docs/evidence/E10/a:b.txt":\n'
                "    bash scripts/run_suite.sh -- pnpm exec playwright test e2e/a.spec.mjs\n\n"
                'e14-harness TASK="T08" *rest:\n    node bench.mjs controls --task {{TASK}}\n\n'
                "evidence-fetch run *flags: e14-harness\n    python3 scripts/evidence_fetch.py {{run}}\n"
            )
            body = plan.bodies(root)
            self.assertEqual(sorted(body), ["e10-browser", "e14-harness", "evidence-fetch"])
            self.assertIn("e2e/a.spec.mjs", body["e10-browser"])
            self.assertEqual(plan.needs(body["e10-browser"]), (True, True))
            # The benchmark's controls grade each stack in a browser.
            self.assertEqual(plan.needs(body["e14-harness"]), (True, True))
            text = (root / "justfile").read_text().splitlines()
            self.assertEqual(plan.recipes_at(text, [(6, 6)], ["e10-browser"]), {"e10-browser"})
        # And the repository's: every recipe `just` knows has its body.
        known = plan.recipes()
        self.assertEqual([n for n in known if n not in plan.bodies()], [])
        self.assertIn("e2e/performance.spec.mjs", plan.bodies()["e10-close-bench"])

    def test_a_todomvc_plan_writes_only_the_workflow_output(self) -> None:
        done = subprocess.run(
            [sys.executable, str(SCRIPTS / "ci_plan.py"), "--shards", "1", "e14-todomvc"],
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        lines = done.stdout.splitlines()
        self.assertEqual(len(lines), 1, "planning metadata must not start a mutation runtime: " + done.stdout)
        self.assertTrue(lines[0].startswith("shards="), done.stdout)
        shards = json.loads(lines[0].split("=", 1)[1])
        self.assertEqual([recipe for shard in shards for recipe in shard["recipes"]], ["e14-todomvc"])

    def test_a_recipe_run_against_a_database_is_a_shard_of_its_own(self) -> None:
        out = subprocess.run(
            [sys.executable, str(SCRIPTS / "ci_plan.py"), "--shards", "2", "e14-feed-postgres", "e14-feed"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.splitlines()
        # Its shard has a PostgreSQL beside it (ADR-0278), and sets up
        # nothing more than its recipe needs (ADR-0258): `e14-feed-postgres`
        # drives no browser and reads no page built.
        self.assertEqual(
            out,
            [
                'shards=[{"index": 0, "recipes": ["e14-feed"], "browsers": true, "build": true, '
                '"database": false}, {"index": 1, "recipes": ["e14-feed-postgres"], "browsers": false, '
                '"build": false, "database": true}]'
            ],
        )

    def test_a_shard_with_a_database_sets_up_what_its_recipe_needs(self) -> None:
        # `e14-identity` drives three browsers against its PostgreSQL tests'
        # database (ADR-0258): its shard installs them, as any would.
        out = subprocess.run(
            [sys.executable, str(SCRIPTS / "ci_plan.py"), "--shards", "2", "e14-identity"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.splitlines()
        self.assertEqual(
            out,
            ['shards=[{"index": 0, "recipes": ["e14-identity"], "browsers": true, "build": true, "database": true}]'],
        )

    def test_recipes_run_against_a_database_take_the_shards_that_end_the_run_soonest(self) -> None:
        # ADR-0278: each shard of theirs has a PostgreSQL of its own, and
        # no other recipe runs there. ADR-0290: they take as many shards as
        # end the run soonest. Long, each its own.
        db = {"d1", "d2", "d3"}
        costs = {"a": 3000, "b": 3000, "d1": 3000, "d2": 3000, "d3": 3000}
        shards = plan.plan(list(costs), costs, 5, {"d1": (True, True)}, db)
        self.assertEqual(
            [(s["recipes"], s["browsers"], s["database"]) for s in shards],
            [
                (["a"], False, False),
                (["b"], False, False),
                (["d1"], True, True),
                (["d2"], False, True),
                (["d3"], False, True),
            ],
        )
        self.assertEqual([s["index"] for s in shards], [0, 1, 2, 3, 4])

    def test_short_recipes_run_against_a_database_share_one(self) -> None:
        db = {"d1", "d2", "d3"}
        costs = {"a": 3000, "b": 3000, "d1": 300, "d2": 200, "d3": 100}
        shards = plan.plan(list(costs), costs, 3, {}, db)
        self.assertEqual(
            [(s["recipes"], s["database"]) for s in shards],
            [(["a"], False), (["b"], False), (["d1", "d2", "d3"], True)],
        )
        # The fewest where more would end the run no sooner.
        costs = {"d1": 3000, "d2": 100, "d3": 100}
        self.assertEqual([s["recipes"] for s in plan.plan(list(costs), costs, 3, {}, db)], [["d1"], ["d2", "d3"]])
        # With no others, every shard may be theirs.
        costs = {"d1": 3000, "d2": 3000, "d3": 3000}
        self.assertEqual([s["recipes"] for s in plan.plan(list(costs), costs, 3, {}, db)], [["d1"], ["d2"], ["d3"]])
        # One shard asked for, and others to run: one for each.
        self.assertEqual(len(plan.plan(["a", "d1"], costs, 1, {}, db)), 2)

    def test_a_recipes_cost_counts_the_mutants_it_plants(self) -> None:
        body = "    python3 scripts/statements_separated_mutations.py\n"
        self.assertGreater(plan.cost(body), 20)
        self.assertEqual(plan.cost("    cargo test\n"), 1)

    def test_a_recipe_costs_the_seconds_it_last_took(self) -> None:
        # ADR-0290: mutants foretold a recipe's time poorly (0.43, over the
        # nightly of 2026-10-08), and two shards outlasted their job.
        body = {n: "    cargo test\n" for n in ("a", "b", "c", "new")}
        body["m"] = "    python3 scripts/statements_separated_mutations.py\n"
        seconds = {"a": 10.4, "b": 30, "c": 20}
        costs = plan.estimated(["a", "new", "m"], body, seconds)
        self.assertEqual(costs["a"], 10)
        # One not yet measured: itself and its mutants, each at the upper
        # quartile of what a measured recipe of its kind took one.
        self.assertEqual(costs["new"], 30)
        self.assertEqual(costs["m"], 30 * plan.cost(body["m"]))
        # Where none is measured, at `RATE`.
        self.assertEqual(plan.estimated(["new"], body, {}), {"new": plan.RATE})

    def test_a_recipe_not_yet_measured_costs_what_its_kind_does(self) -> None:
        # `e14-refusal`'s first run was planned at the median of every
        # recipe, 24 s a mutant, took about 420 s a mutant, and its shard ran
        # out of time: what a browser's mutant costs is not what a type
        # check's does.
        browser = "    cd spikes/own-renderer && pnpm exec playwright test e2e/a.spec.mjs\n"
        host = "    cargo test --locked -p pw-dev-server -- a\n"
        core = "    cargo test --locked -p pw-core --test a\n"
        self.assertEqual(
            [plan.kind(b) for b in (browser, host, core)], ["browser", "host", "core"]
        )
        # A script's kind is what it runs.
        self.assertEqual(plan.kind("    python3 scripts/navigate_mutations.py\n"), "browser")
        self.assertEqual(plan.kind("    python3 scripts/redirects_mutations.py\n"), "host")
        body = {"b1": browser, "b2": browser, "b3": browser, "b4": browser, "c1": core}
        body.update({"new-browser": browser, "new-core": core, "new-host": host})
        seconds = {"b1": 100, "b2": 200, "b3": 400, "b4": 800, "c1": 10}
        costs = plan.estimated(["new-browser", "new-core", "new-host"], body, seconds)
        # The browser's upper quartile, not the median of every recipe.
        self.assertEqual(costs["new-browser"], 800)
        self.assertEqual(costs["new-core"], 10)
        # A kind none of whose recipes is measured: every recipe's upper
        # quartile, 400 of 10, 100, 200, 400 and 800.
        self.assertEqual(costs["new-host"], 400)

    def test_a_run_is_planned_by_the_seconds_kept(self) -> None:
        # Three recipes that set up nothing, in two shards: whichever took
        # longest last is alone.
        import contextlib
        import io

        names = ["e10-affine-bindings", "e10-attribute-case", "e10-built-pages"]
        real, argv = plan.SECONDS, sys.argv
        try:
            with tempfile.TemporaryDirectory() as d:
                plan.SECONDS = pathlib.Path(d) / "seconds.json"
                sys.argv = ["ci_plan.py", "--shards", "2", *names]
                for longest in names:
                    plan.SECONDS.write_text(json.dumps({n: 5000 if n == longest else 1000 for n in names}))
                    out = io.StringIO()
                    with contextlib.redirect_stdout(out):
                        self.assertEqual(plan.main(), 0)
                    shards = json.loads(out.getvalue().strip()[len("shards="):])
                    self.assertEqual(shards[0]["recipes"], [longest])
        finally:
            plan.SECONDS, sys.argv = real, argv

    def test_a_run_takes_more_shards_than_it_asks_where_its_would_end_past_the_ceiling(self) -> None:
        # ADR-0290, amended 2026-10-10: track/command-order's verify dealt
        # 13,959 s into a shard that its limit then stopped. Four recipes of
        # 8,000 s each, asked for in two shards, would end at 16,042 s
        # each: the run takes four.
        costs = {n: 8000 for n in "abcd"}
        self.assertEqual(plan.ends(plan.plan(list(costs), costs, 2), costs), 16042)
        dealt = plan.planned(list(costs), costs, 2)
        self.assertEqual(len(dealt), 4)
        self.assertLessEqual(plan.ends(dealt, costs), plan.CEILING)
        # The control: where the asked number ends within it, it is kept.
        small = {n: 1000 for n in "abcd"}
        self.assertEqual(len(plan.planned(list(small), small, 2)), 2)

    def test_a_recipe_longer_than_the_ceiling_alone_is_not_paid_for_in_shards(self) -> None:
        # One recipe longer than the ceiling ends its shard past it however
        # many shards there are: it is dealt a shard of its own, and the
        # others within the ceiling, not a shard each.
        costs = {"long": plan.CEILING + 2000, "a": 5000, "b": 5000, "c": 5000}
        dealt = plan.planned(list(costs), costs, 2)
        self.assertEqual(len(dealt), 3)
        self.assertIn({"index": 0, "recipes": ["long"], "browsers": False, "build": False, "database": False}, dealt)
        for shard in dealt:
            if shard["recipes"] != ["long"]:
                self.assertLessEqual(plan.ends([shard], costs), plan.CEILING)

    def test_a_long_singleton_does_not_hide_a_database_shard_past_the_ceiling(self) -> None:
        # More database shards cannot shorten the long singleton, but can
        # prevent a different, splittable shard exceeding ADR-0290's ceiling.
        costs = {"long": 100000, "d1": 6000, "d2": 6000}
        db = {"d1", "d2"}
        for asked in (2, 3, 4, plan.MOST):
            with self.subTest(asked=asked):
                dealt = plan.planned(list(costs), costs, asked, database=db)
                self.assertEqual(len(dealt), 3)
                self.assertEqual(sorted(s["recipes"] for s in dealt), [["d1"], ["d2"], ["long"]])

    def test_a_long_database_singleton_does_not_hide_its_other_database_shard(self) -> None:
        costs = {"ordinary": 6000, "long-db": 100000, "d1": 6000, "d2": 6000}
        dealt = plan.planned(list(costs), costs, 3, database={"long-db", "d1", "d2"})
        self.assertEqual(len(dealt), 4)
        for shard in dealt:
            if shard["recipes"] != ["long-db"]:
                self.assertLessEqual(plan.ends([shard], costs), plan.CEILING)

    def test_the_ceiling_tie_counts_database_setup(self) -> None:
        # The recipes total 10,000: plain setup fits, browser setup does not.
        costs = {"long": 100000, "d1": 5000, "d2": 5000}
        db = {"d1", "d2"}
        self.assertEqual(len(plan.planned(list(costs), costs, 3, database=db)), 2)
        browsers = {"d1": (True, True)}
        dealt = plan.planned(list(costs), costs, 3, browsers, db)
        self.assertEqual(len(dealt), 3)
        self.assertEqual([s["recipes"] for s in dealt if s["database"]], [["d1"], ["d2"]])

    def test_a_safe_database_bundle_keeps_the_fewest_database_shards(self) -> None:
        costs = {"long": 100000, "d1": 1200, "d2": 1200}
        dealt = plan.planned(list(costs), costs, 3, database={"d1", "d2"})
        self.assertEqual([s["recipes"] for s in dealt], [["long"], ["d1", "d2"]])

    def test_main_deals_a_run_within_the_ceiling(self) -> None:
        # The workflow's own call: four recipes of 8,000 s asked for in two
        # shards are dealt into four.
        import contextlib
        import io

        names = ["e10-affine-bindings", "e10-attribute-case", "e10-built-pages", "e10-clause-keys"]
        real, argv = plan.SECONDS, sys.argv
        try:
            with tempfile.TemporaryDirectory() as d:
                plan.SECONDS = pathlib.Path(d) / "seconds.json"
                plan.SECONDS.write_text(json.dumps({n: 8000 for n in names}))
                sys.argv = ["ci_plan.py", "--shards", "2", *names]
                out = io.StringIO()
                with contextlib.redirect_stdout(out):
                    self.assertEqual(plan.main(), 0)
                shards = json.loads(out.getvalue().strip()[len("shards="):])
                self.assertEqual(len(shards), 4)
        finally:
            plan.SECONDS, sys.argv = real, argv

    def test_the_ceiling_is_within_the_shards_limit(self) -> None:
        # A shard is stopped at verify.yml's `timeout-minutes`.
        workflow = (SCRIPTS.parent / ".github/workflows/verify.yml").read_text()
        limits = [int(m) for m in __import__("re").findall(r"timeout-minutes: (\d+)", workflow)]
        self.assertLess(plan.CEILING, max(limits) * 60)

    def test_a_diff_that_is_no_utf8_is_still_read(self) -> None:
        # A raw DEFLATE file git diffs as text, its bytes no UTF-8, and a
        # path in kana as `core.quotePath=false` writes it.
        diff = (
            "diff --git a/examples/kiokun/data/02/\u5410\u304f.json.deflate "
            "b/examples/kiokun/data/02/\u5410\u304f.json.deflate\n"
            "--- /dev/null\n"
            "+++ b/examples/kiokun/data/02/\u5410\u304f.json.deflate\n"
            "@@ -0,0 +1,2 @@\n"
        ).encode("utf-8") + b"+\xcd\xff\x80 raw\n+\xfe\n" + (
            "diff --git a/compiler/pw-core/src/check.rs b/compiler/pw-core/src/check.rs\n"
            "--- a/compiler/pw-core/src/check.rs\n"
            "+++ b/compiler/pw-core/src/check.rs\n"
            "@@ -10,0 +11,3 @@\n"
        ).encode("utf-8")
        self.assertEqual(
            plan.parsed(diff),
            {
                "examples/kiokun/data/02/\u5410\u304f.json.deflate": [(1, 2)],
                "compiler/pw-core/src/check.rs": [(11, 13)],
            },
        )

    def test_the_seconds_are_read_where_the_fetch_keeps_them(self) -> None:
        self.assertEqual(plan.SECONDS, fetch.SECONDS)
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "seconds.json"
            self.assertEqual(plan.measured(path), {})
            path.write_text('{"e14-a": 61}\n')
            self.assertEqual(plan.measured(path), {"e14-a": 61.0})
            path.write_text("not json")
            self.assertEqual(plan.measured(path), {})


class Recipes(unittest.TestCase):
    def test_what_a_recipe_wrote_is_what_changed(self) -> None:
        before = {"docs/evidence/a.txt": "1", "docs/evidence/b.txt": "2"}
        after = {"docs/evidence/a.txt": "1", "docs/evidence/b.txt": "3", "docs/evidence/c.txt": "4"}
        self.assertEqual(
            recipes.written(before, after),
            ["docs/evidence/b.txt", "docs/evidence/c.txt"],
        )


    def test_a_failed_recipe_says_its_last_lines_in_the_jobs_log(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = pathlib.Path(tmp)
            (out / "logs").mkdir()
            (out / "logs" / "e14-x.log").write_text("".join(f"line {i}\n" for i in range(100)))
            (out / "evidence" / "docs").mkdir(parents=True)
            (out / "evidence" / "docs" / "x.txt").write_text("built\nError: no such module\n")
            said = recipes.failed_tail("e14-x", out, ["docs/x.txt"], lines=5)
        self.assertIn("| line 99", said)
        self.assertNotIn("| line 94", said)
        self.assertIn("| Error: no such module", said)

    def test_a_heartbeat_says_what_a_recipe_is_doing_until_it_ends(self) -> None:
        # A runner that dies mid-recipe keeps what reached the job's log.
        import contextlib, io, threading
        stop = threading.Event()
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            beat = threading.Thread(target=recipes.heartbeat, args=("e14-x", stop, 0.05))
            beat.start()
            # Until the first beat is heard, however long a loaded machine
            # takes to run the thread (a sleep of 0.3 s was a silence that
            # machine made too), and no longer than thirty seconds.
            deadline = time.monotonic() + 30
            while not said.getvalue() and time.monotonic() < deadline:
                time.sleep(0.01)
            stop.set()
            beat.join()
        lines = [l for l in said.getvalue().splitlines() if l.startswith("  [e14-x, ")]
        self.assertTrue(lines, said.getvalue())
        self.assertIn("evidence", lines[-1])

class Prune(unittest.TestCase):
    def test_what_builds_leave_is_freed_and_the_newest_binary_kept(self) -> None:
        import os
        import time

        with tempfile.TemporaryDirectory() as tmp:
            target = pathlib.Path(tmp)
            deps = target / "debug" / "deps"
            deps.mkdir(parents=True)
            (target / "debug" / "incremental" / "x").mkdir(parents=True)
            old, new = deps / "feed-0123456789abcdef", deps / "feed-fedcba9876543210"
            for i, binary in enumerate((old, new)):
                binary.write_text("binary")
                binary.chmod(0o755)
                os.utime(binary, (time.time() + i, time.time() + i))
            library = deps / "libpw_core-0123456789abcdef.rlib"
            library.write_text("library")
            (deps / "feed.a1.rcgu.o").write_text("object")
            recipes.prune(target)
            left = sorted(p.name for p in deps.iterdir())
            self.assertEqual(left, sorted([library.name, new.name]))
            self.assertFalse((target / "debug" / "incremental").exists())


class Fetch(unittest.TestCase):
    SHA = "a" * 40
    URL = "https://github.com/o/r/actions/runs/1"

    def test_the_run_is_named_after_the_commit(self) -> None:
        text = f"title\n\nproduced by: just e14-x\ncommit: {self.SHA}\nrust: 1\n"
        stamped = fetch.stamp(text, self.SHA, self.URL, "Ubuntu 24.04, x86_64")
        self.assertEqual(
            stamped,
            f"title\n\nproduced by: just e14-x\ncommit: {self.SHA}\n"
            f"ci: {self.URL} (Ubuntu 24.04, x86_64)\nrust: 1\n",
        )
        # Fetched again, the line is replaced, not repeated.
        again = fetch.stamp(stamped, self.SHA, self.URL + "2", "r")
        self.assertEqual(again.count("ci: "), 1)
        self.assertIn("runs/12 (r)", again)

    def test_a_file_of_another_commit_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            fetch.stamp(f"commit: {'b' * 40}\n", self.SHA, self.URL, "r")

    def test_a_file_with_no_commit_is_copied_as_it_is(self) -> None:
        self.assertEqual(fetch.stamp("raw output\n", self.SHA, self.URL, "r"), "raw output\n")


    def test_main_reaches_the_refusal_by_its_own_name(self) -> None:
        # `main` keeps a list of refused files; a function of the same name
        # was shadowed there, and `main` failed before it fetched anything.
        import ast
        tree = ast.parse((SCRIPTS / "evidence_fetch.py").read_text())
        main = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "main")
        assigned = {t.id for n in ast.walk(main) if isinstance(n, ast.Assign) for t in n.targets if isinstance(t, ast.Name)}
        self.assertNotIn("refusal", assigned)

    def test_a_run_that_failed_is_fetched_only_where_its_failures_are_known(self) -> None:
        # ADR-0281: a merge names each failure open in NEXT, and its
        # evidence is fetched beside it.
        run = {"status": "completed", "conclusion": "failure"}
        jobs = [
            {"name": "recipes 0", "conclusion": "success"},
            {"name": "browser webkit", "conclusion": "failure"},
            {"name": "summary", "conclusion": "failure"},
        ]
        self.assertIsNone(fetch.refusal(run, jobs, ["browser webkit", "summary"]))
        self.assertIn("summary", fetch.refusal(run, jobs, ["browser webkit"]))
        self.assertIn("browser webkit", fetch.refusal(run, jobs, []))
        # A recipe shard is never known: its evidence is what is copied.
        shard = jobs + [{"name": "recipes 3", "conclusion": "failure"}]
        self.assertIn("recipes 3", fetch.refusal(run, shard, ["browser webkit", "summary", "recipes 3"]))
        # A run that has not finished, or one that failed nowhere, is not.
        self.assertIn("in_progress", fetch.refusal({"status": "in_progress", "conclusion": None}, jobs, []))
        self.assertIsNone(fetch.refusal({"status": "completed", "conclusion": "success"}, jobs, []))
        self.assertIn("no job", fetch.refusal(run, [{"name": "recipes 0", "conclusion": "success"}], ["x"]))

    def test_what_a_failed_recipe_wrote_is_not_copied(self) -> None:
        # 2026-10-12: W8's merge, its run's failure known, took e14-diffs'
        # output with its evidence, stopped at a patch that did not apply.
        results = [
            {"recipe": "e14-diffs", "status": 1, "seconds": 9, "wrote": ["docs/evidence/E14/semantic-diffs.txt"]},
            {"recipe": "e14-feed", "status": 0, "seconds": 9, "wrote": ["docs/evidence/E14/feed.txt"]},
            {"recipe": "e14-old", "status": 0, "seconds": 9},
        ]
        self.assertEqual(fetch.failed_files(results), {"docs/evidence/E14/semantic-diffs.txt"})
        self.assertEqual(fetch.failed_files(results[1:]), set())

    def test_a_file_whose_recipe_reads_what_changed_since_its_run_is_refused(self) -> None:
        # ADR-0281, amended 2026-10-12: narrowing-2's run at 853674f was of
        # scripts that run the server's tests by name, and master had
        # changed that server since; its files were evidence of code master
        # would not hold.
        results = [
            {"recipe": "e14-diffs", "status": 0, "seconds": 9, "wrote": ["docs/evidence/E14/semantic-diffs.txt"]},
            {"recipe": "e14-feed", "status": 0, "seconds": 9, "wrote": ["docs/evidence/E14/feed.txt"]},
        ]
        by = fetch.recorded(results)
        self.assertEqual(by, {
            "docs/evidence/E14/semantic-diffs.txt": "e14-diffs",
            "docs/evidence/E14/feed.txt": "e14-feed",
        })
        moved = {"e14-diffs"}
        reason = fetch.verdict("e14-diffs", moved, [])
        self.assertIsInstance(reason, str, "A moved recipe must return a refusal reason")
        self.assertIn("reads what changed", reason)
        self.assertIsNone(fetch.verdict("e14-feed", moved, []))
        # Skipped, where another run records it: neither copied nor refused.
        self.assertEqual(fetch.verdict("e14-diffs", moved, ["e14-diffs"]), "skipped")
        self.assertEqual(fetch.verdict("e14-feed", set(), ["e14-feed"]), "skipped")
        # A file no recipe of the run says it wrote is no one's evidence.
        reason = fetch.verdict(None, set(), [])
        self.assertIsInstance(reason, str, "An unowned file must return a refusal reason")
        self.assertIn("no recipe", reason)

    def test_what_changed_since_a_run_is_the_planners_against_the_tree_here(self) -> None:
        # A push plans by `ci_plan.py`'s rule; a fetch reads the same, from
        # the run's commit to the tree here, a merge not yet committed.
        seen = []

        def hunks(base, head):
            seen.append((base, head))
            return {"benchmarks/baselines/pleris/store/app.pw": [(1, 1)]}

        planner = types.SimpleNamespace(hunks=hunks, recipes_for=plan.recipes_for, NEAR=plan.NEAR)
        with mock.patch.dict(sys.modules, {"ci_plan": planner}):
            moved = fetch.moved_on(self.SHA, ["e14-diffs", "e14-feed"])
        self.assertEqual(moved, {"e14-diffs"})
        self.assertEqual(seen, [(self.SHA, None)])

    def test_the_tree_here_is_what_is_staged_and_what_is_not(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)

            def git(*args: str) -> None:
                subprocess.run(
                    ["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", *args],
                    cwd=root, check=True, capture_output=True,
                )

            git("init", "-q")
            (root / "a.txt").write_text("1\n2\n")
            git("add", "a.txt")
            git("commit", "-q", "-m", "a")
            (root / "a.txt").write_text("1\ntwo\n")
            (root / "b.txt").write_text("b\n")
            git("add", "b.txt")
            with mock.patch.object(plan, "ROOT", root):
                self.assertEqual(plan.hunks("HEAD", None), {"a.txt": [(2, 2)], "b.txt": [(1, 1)]})
                # Between commits, what is not committed is not read.
                self.assertEqual(plan.hunks("HEAD", "HEAD"), {})

    def test_a_recipe_that_ran_to_its_end_has_its_seconds_kept(self) -> None:
        # ADR-0290: the plan deals the next runs by them.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for i, results in enumerate(
                [
                    [{"recipe": "e14-a", "status": 0, "seconds": 61.6}],
                    # A recipe that failed took what it took to fail: not kept.
                    [{"recipe": "e14-b", "status": 2, "seconds": 5.0}, {"recipe": "e14-d", "status": 0, "seconds": 7}],
                ]
            ):
                (root / f"evidence-{i}").mkdir()
                (root / f"evidence-{i}" / "results.json").write_text(json.dumps(results))
            # A shard that stopped before its first recipe ended wrote none.
            (root / "evidence-2").mkdir()
            timed = fetch.timed(fetch.results_of(fetch.shard_dirs(root)))
            self.assertEqual(timed, {"e14-a": 62, "e14-d": 7})
            path = root / "seconds.json"
            path.write_text('{"e14-a": 10, "e14-c": 30}\n')
            self.assertEqual(fetch.remember(timed, path), 2)
            # Each kept over its last; every other recipe's as it was.
            self.assertEqual(json.loads(path.read_text()), {"e14-a": 62, "e14-c": 30, "e14-d": 7})
            # A run of one shard, extracted into the download's own directory.
            lone = root / "lone"
            (lone / "evidence").mkdir(parents=True)
            (lone / "results.json").write_text(json.dumps([{"recipe": "e14-x", "status": 0, "seconds": 1}]))
            self.assertEqual(fetch.timed(fetch.results_of(fetch.shard_dirs(lone))), {"e14-x": 1})

    def test_the_seconds_kept_are_each_a_recipes(self) -> None:
        kept = json.loads(fetch.SECONDS.read_text())
        notes = {k: v for k, v in kept.items() if k.startswith("_")}
        seconds = {k: v for k, v in kept.items() if not k.startswith("_")}
        self.assertGreater(len(seconds), 100)
        self.assertTrue(all(plan.EVIDENCE_RECIPE.match(r) and isinstance(s, int) and s >= 0 for r, s in seconds.items()))
        self.assertEqual(list(kept), sorted(kept))
        # The one note: the recipes whose seconds are lower bounds, each kept
        # with its seconds, and why (the integrator's ruling 2, train-10).
        self.assertLessEqual(set(notes), {"_lower_bounds"})
        for recipe, why in notes.get("_lower_bounds", {}).items():
            self.assertIn(recipe, seconds)
            self.assertIsInstance(why, str)

    def test_a_recipe_run_to_its_end_leaves_the_lower_bounds(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "seconds.json"
            path.write_text(json.dumps({
                "_lower_bounds": {"e14-a": "stopped at the limit", "e14-b": "stopped at the limit"},
                "e14-a": 100,
                "e14-b": 200,
            }))
            fetch.remember({"e14-a": 150}, path)
            kept = json.loads(path.read_text())
            self.assertEqual(kept["e14-a"], 150)
            self.assertEqual(kept["_lower_bounds"], {"e14-b": "stopped at the limit"})
            fetch.remember({"e14-b": 250}, path)
            self.assertNotIn("_lower_bounds", json.loads(path.read_text()))

class Summary(unittest.TestCase):
    def run_summary(self, shards: list[list[dict]], files: dict[str, str], planned: int) -> int:
        return self.summary_of(shards, files, planned).returncode

    def summary_of(self, shards: list[list[dict]], files: dict[str, str], planned: int):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for i, results in enumerate(shards):
                shard = root / f"evidence-{i}"
                (shard / "evidence").mkdir(parents=True)
                (shard / "results.json").write_text(json.dumps(results))
                for path, text in files.items():
                    target = shard / "evidence" / path
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(text)
            return subprocess.run(
                [sys.executable, str(SCRIPTS / "ci_summary.py"), tmp, "--shards", str(planned)],
                capture_output=True,
                text=True,
            )

    def result(self, status: int = 0, wrote: list[str] | None = None) -> dict:
        return {"recipe": "e14-x", "status": status, "seconds": 1.0, "wrote": wrote or []}

    def test_a_run_whose_recipes_passed_passes(self) -> None:
        self.assertEqual(self.run_summary([[self.result(wrote=["a.txt"])]], {"a.txt": "3 of 3 mutants killed\n"}, 1), 0)

    def test_a_kill_perhaps_the_memory_bounds_alone_fails_the_recipe(self) -> None:
        # A stopped producer is invalid, even if its legacy runner says killed.
        stopped = (
            "  memory bound: stopped `graphs`, which held more than 4 GiB\n"
            "the renderer takes a node twice: KILLED (1 of 25 tests fail)\n"
            "19 of 19 mutants killed\n"
            "memory bound: a process was stopped in 1 of 19 mutants' runs, "
            "each perhaps killed by the bound alone: the renderer takes a node twice\n"
        )
        done = self.summary_of([[self.result(wrote=["a.txt"])]], {"a.txt": stopped}, 1)
        self.assertEqual(done.returncode, 1)
        self.assertIn("Invalid producer: memory guard", done.stdout)
        self.assertIn("| `e14-x` | FAILED |", done.stdout)
        self.assertIn(
            "- e14-x: memory bound: a process was stopped in 1 of 19 mutants' runs, each perhaps killed "
            "by the bound alone: the renderer takes a node twice\n",
            done.stdout,
        )
        quiet = self.summary_of(
            [[self.result(wrote=["a.txt"])]], {"a.txt": "3 of 3 mutants killed\nmemory bound: no process was stopped\n"}, 1
        )
        self.assertNotIn("memory bound", quiet.stdout)

    def test_a_failed_recipe_fails_the_run(self) -> None:
        self.assertEqual(self.run_summary([[self.result(status=1)]], {}, 1), 1)

    def test_a_surviving_mutant_fails_the_run(self) -> None:
        self.assertEqual(self.run_summary([[self.result(wrote=["a.txt"])]], {"a.txt": "x: SURVIVED\n"}, 1), 1)

    def test_a_shard_that_reported_nothing_fails_the_run(self) -> None:
        self.assertEqual(self.run_summary([[self.result()]], {}, 2), 1)

    def test_a_run_of_one_shard_is_read_where_it_was_extracted(self) -> None:
        # `download-artifact` puts a lone match in the path itself: W6's 1b
        # ran one shard, passed it, and the run reported none.
        def one(results: list[dict], files: dict[str, str]) -> int:
            with tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                (root / "evidence").mkdir()
                (root / "results.json").write_text(json.dumps(results))
                for path, text in files.items():
                    (root / "evidence" / path).write_text(text)
                return subprocess.run(
                    [sys.executable, str(SCRIPTS / "ci_summary.py"), tmp, "--shards", "1"],
                    capture_output=True,
                    text=True,
                ).returncode

        self.assertEqual(one([self.result(wrote=["a.txt"])], {"a.txt": "3 of 3 mutants killed\n"}), 0)
        self.assertEqual(one([self.result(wrote=["a.txt"])], {"a.txt": "x: SURVIVED\n"}), 1)


class CurrentEvidence(unittest.TestCase):
    """ADR-0281, amended 2026-10-13 (6): a push does not run a recipe whose
    committed evidence is current at its head."""

    HEAD, OLD, ELSE = "h" * 40, "a" * 40, "e" * 40
    BODY = {
        "e14-one": "@{ echo; } > docs/evidence/E14/one.txt\n",
        "e14-two": "@{ echo; } > docs/evidence/E14/two.txt\n    @{ echo; } >docs/evidence/E14/two-more.txt\n",
        "e14-none": "@cargo test --locked\n",
    }

    def current(self, files: dict, reached: set = frozenset(), descends: bool = True) -> dict:
        return plan.current(
            sorted(self.BODY),
            self.HEAD,
            body=self.BODY,
            read=lambda head, path: files.get(path),
            descends_from=lambda sha, head: descends,
            reach=lambda sha, head: set(reached),
        )

    def test_a_recipe_whose_evidence_is_current_is_not_run_again(self) -> None:
        files = {"docs/evidence/E14/one.txt": self.OLD}
        self.assertEqual(self.current(files), {"e14-one": self.OLD})
        # The control: what it reads changed since its evidence was recorded.
        self.assertEqual(self.current(files, reached={"e14-one"}), {})

    def test_evidence_of_a_commit_the_head_does_not_descend_from_is_not_current(self) -> None:
        files = {"docs/evidence/E14/one.txt": self.OLD}
        self.assertEqual(self.current(files, descends=False), {})

    def test_a_recipe_is_current_only_where_every_file_it_writes_is_at_one_commit(self) -> None:
        both = {"docs/evidence/E14/two.txt": self.OLD, "docs/evidence/E14/two-more.txt": self.OLD}
        self.assertEqual(self.current(both), {"e14-two": self.OLD})
        apart = {"docs/evidence/E14/two.txt": self.OLD, "docs/evidence/E14/two-more.txt": self.ELSE}
        self.assertEqual(self.current(apart), {})
        one = {"docs/evidence/E14/two.txt": self.OLD}
        self.assertEqual(self.current(one), {})

    def test_a_recipe_that_writes_no_evidence_is_never_current(self) -> None:
        self.assertNotIn("e14-none", self.current({"docs/evidence/E14/one.txt": self.OLD}))
        self.assertEqual(plan.evidence_files(self.BODY["e14-two"]), [
            "docs/evidence/E14/two-more.txt", "docs/evidence/E14/two.txt",
        ])

    def test_any_error_leaves_a_recipe_planned(self) -> None:
        def broken(head: str, path: str) -> str:
            raise OSError("no git")

        found = plan.current(
            ["e14-one"], self.HEAD, body=self.BODY, read=broken,
            descends_from=lambda sha, head: True, reach=lambda sha, head: set(),
        )
        self.assertEqual(found, {})

    def test_a_tree_with_changes_records_no_commit(self) -> None:
        sha = "0123456789abcdef0123456789abcdef01234567"
        self.assertEqual(plan.RECORDED_AT.search(f"x\ncommit: {sha}\nrust: 1\n").group(1), sha)
        self.assertIsNone(plan.RECORDED_AT.search(f"commit: {sha} + uncommitted changes\n"))
        self.assertIsNone(plan.RECORDED_AT.search("commit: 0123456\n"))

    def test_the_evidence_is_read_as_the_head_holds_it(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)

            def git(*args: str) -> str:
                return subprocess.run(
                    ["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", *args],
                    cwd=root, check=True, capture_output=True, text=True,
                ).stdout.strip()

            git("init", "-q")
            (root / "a.txt").write_text("a\n")
            git("add", "a.txt")
            git("commit", "-q", "-m", "a")
            run = git("rev-parse", "HEAD")
            evidence = root / "docs/evidence/E14/one.txt"
            evidence.parent.mkdir(parents=True)
            evidence.write_text(f"one\n\ncommit: {run}\n")
            git("add", "docs")
            git("commit", "-q", "-m", "evidence")
            head = git("rev-parse", "HEAD")
            # Changed in the tree since, and not committed: not what the head holds.
            evidence.write_text("one\n\ncommit: " + "f" * 40 + "\n")
            self.assertEqual(plan.recorded_at(head, "docs/evidence/E14/one.txt", root), run)
            self.assertIsNone(plan.recorded_at(head, "docs/evidence/E14/none.txt", root))
            self.assertTrue(plan.descends(run, head, root))
            self.assertFalse(plan.descends(head, run, root))

    def test_main_runs_no_current_recipe_only_where_asked(self) -> None:
        import contextlib
        import io

        def planned(argv: list[str]) -> list[str]:
            out = io.StringIO()
            with mock.patch.object(sys, "argv", ["ci_plan.py", *argv]), contextlib.redirect_stdout(out):
                self.assertEqual(plan.main(), 0)
            shards = json.loads(out.getvalue().strip()[len("shards="):])
            return sorted(r for s in shards for r in s["recipes"])

        names = ["e10-affine-bindings", "e10-attribute-case"]
        with mock.patch.object(plan, "changed_recipes", lambda *a: list(names)), \
                mock.patch.object(plan, "current", lambda n, head, near: {"e10-attribute-case": self.OLD}):
            self.assertEqual(planned(["--changed", "b", "h"]), names)
            self.assertEqual(planned(["--changed", "b", "h", "--current"]), ["e10-affine-bindings"])

    def test_the_workflow_plans_each_push_and_pr_with_the_evidence(self) -> None:
        workflow = (SCRIPTS.parent / ".github/workflows/verify.yml").read_text()

        def changed_plans(section: str) -> list[str]:
            return [
                line for line in section.splitlines()
                if line.lstrip().startswith("python3 scripts/ci_plan.py")
                and "--changed" in line
            ]

        # A PR adds its own planning arm. Do not count it as a third push,
        # and do not allow any arm to reuse stale committed evidence.
        push = workflow.split("            push)\n", 1)[1].split(
            "            pull_request)\n", 1
        )[0]
        pr = workflow.split("            pull_request)\n", 1)[1].split(
            "            schedule)\n", 1
        )[0]
        push_plans = changed_plans(push)
        pr_plans = changed_plans(pr)
        self.assertEqual(len(push_plans), 2, push_plans)
        self.assertEqual(len(pr_plans), 1, pr_plans)
        self.assertTrue(
            all("--current" in line for line in [*push_plans, *pr_plans]),
            [*push_plans, *pr_plans],
        )


class StaleEvidence(unittest.TestCase):
    """Track evidence-integrity, finding 2: a train's push plans each recipe
    whose committed evidence is stale at its head, whatever the train
    changed. A push planned what its range reached, less what was current,
    so evidence gone stale before the range was planned by none, and the
    nightly keeps none: `docs/evidence/E10/privacy-by-resolution.txt` said
    "4 of 4 mutants killed" for a week after its script's fourth mutant was
    retired."""

    HEAD, OLD = "h" * 40, "a" * 40
    BODY = {
        "e14-one": "@{ echo; } > docs/evidence/E14/one.txt\n",
        "e14-two": "@{ echo; } > docs/evidence/E14/two.txt\n",
        "e14-none": "@cargo test --locked\n",
    }

    def stale(self, texts: dict, reached: set = frozenset()) -> list:
        def read(head: str, path: str) -> str | None:
            m = plan.RECORDED_AT.search(texts.get(path) or "")
            return m.group(1) if m else None

        return plan.stale(
            sorted(self.BODY),
            self.HEAD,
            body=self.BODY,
            show=lambda head, path: texts.get(path),
            read=read,
            descends_from=lambda sha, head: True,
            reach=lambda sha, head: set(reached),
        )

    def test_evidence_whose_reads_changed_since_its_commit_is_stale(self) -> None:
        texts = {
            "docs/evidence/E14/one.txt": f"commit: {self.OLD}\n",
            "docs/evidence/E14/two.txt": f"commit: {self.OLD}\n",
        }
        self.assertEqual(self.stale(texts, reached={"e14-one"}), ["e14-one"])
        # The control: nothing either reads changed since; current, both.
        self.assertEqual(self.stale(texts), [])

    def test_evidence_from_a_tree_with_changes_or_none_at_all_is_stale(self) -> None:
        texts = {
            "docs/evidence/E14/one.txt": f"commit: {self.OLD} + uncommitted changes\n",
            "docs/evidence/E14/two.txt": f"commit: {self.OLD}\n",
        }
        self.assertEqual(self.stale(texts), ["e14-one"])
        # The head holds none of its evidence: it records nothing.
        del texts["docs/evidence/E14/one.txt"]
        self.assertEqual(self.stale(texts), ["e14-one"])

    def test_evidence_that_names_no_commit_is_not_judged(self) -> None:
        # An artifact, or an older recipe's text: planned as before, by what
        # changed, and never as stale; nor a recipe that writes none.
        texts = {
            "docs/evidence/E14/one.txt": "an artifact\n",
            "docs/evidence/E14/two.txt": f"commit: {self.OLD}\n",
        }
        self.assertEqual(self.stale(texts, reached={"e14-one", "e14-two", "e14-none"}), ["e14-two"])

    def test_main_plans_stale_evidence_only_where_asked(self) -> None:
        import contextlib
        import io

        def planned(argv: list[str]) -> list[str]:
            out = io.StringIO()
            with mock.patch.object(sys, "argv", ["ci_plan.py", *argv]), contextlib.redirect_stdout(out):
                self.assertEqual(plan.main(), 0)
            shards = json.loads(out.getvalue().strip()[len("shards="):])
            return sorted(r for s in shards for r in s["recipes"])

        with mock.patch.object(plan, "changed_recipes", lambda *a: ["e10-affine-bindings"]), \
                mock.patch.object(plan, "current", lambda n, head, near: {}), \
                mock.patch.object(plan, "stale", lambda n, head, near: ["e10-attribute-case"]):
            self.assertEqual(planned(["--changed", "b", "h", "--current"]), ["e10-affine-bindings"])
            self.assertEqual(
                planned(["--changed", "b", "h", "--current", "--stale"]),
                ["e10-affine-bindings", "e10-attribute-case"],
            )

    def test_a_trains_push_plans_the_stale_evidence_and_another_tracks_does_not(self) -> None:
        # The track's branch of the plan step, run as the workflow runs it,
        # with `git` and `python3` answering for themselves.
        workflow = (SCRIPTS.parent / ".github/workflows/verify.yml").read_text().splitlines()
        start = next(i for i, l in enumerate(workflow) if 'if [ "${GITHUB_REF_NAME#track/}"' in l)
        end = next(i for i in range(start, len(workflow)) if workflow[i].lstrip().startswith("elif "))
        branch = "\n".join(l.strip() for l in workflow[start + 1 : end])

        def plans(ref: str) -> str:
            with tempfile.TemporaryDirectory() as d:
                output = pathlib.Path(d) / "output"
                script = (
                    'git() { echo base; }\npython3() { echo "python3 $*"; }\n'
                    f"GITHUB_REF_NAME={ref}\nGITHUB_SHA=head\nGITHUB_OUTPUT={output}\n{branch}\n"
                )
                subprocess.run(["bash", "-c", script], check=True, capture_output=True)
                return output.read_text()

        self.assertIn("--current --stale", plans("track/train-9"))
        self.assertNotIn("--stale", plans("track/evidence-integrity"))
        self.assertIn("--changed base head --current", plans("track/evidence-integrity"))


class VerifiedBase(unittest.TestCase):
    """ADR-0281, amended 2026-10-13: a train verified at its head is not
    verified again when `master` moves to it."""

    PASSED = [{"event": "push", "conclusion": "success"}]

    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = pathlib.Path(directory.name)
        self.env = {"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1", "PATH": "/usr/bin:/bin"}
        self.git("init", "-q", "-b", "master")
        self.sha = {"before": self.commit("before")}
        # A train: its merges on one line, its head, then its evidence.
        self.git("checkout", "-q", "-b", "train")
        for name in ("merge", "head", "evidence"):
            self.sha[name] = self.commit(name)
        # A track merged on `master` itself, not fast-forwarded to.
        self.git("checkout", "-q", "-b", "side", self.sha["before"])
        self.sha["side"] = self.commit("side")
        self.git("checkout", "-q", "train")
        self.git("merge", "-q", "--no-ff", "-m", "side", "side")
        self.sha["merged"] = self.git("rev-parse", "HEAD")

    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", *args],
            cwd=self.root, env=self.env, check=True, capture_output=True, text=True,
        ).stdout.strip()

    def commit(self, name: str) -> str:
        (self.root / name).write_text(name)
        self.git("add", name)
        self.git("commit", "-q", "-m", name)
        return self.git("rev-parse", "HEAD")

    def base(self, head: str, runs: dict[str, list[dict]]) -> str:
        return vbase.base(self.sha["before"], self.sha[head], lambda sha: runs.get(sha, []), self.root)

    def test_master_plans_from_the_newest_commit_a_run_passed_at(self) -> None:
        runs = {self.sha["merge"]: self.PASSED, self.sha["head"]: self.PASSED}
        self.assertEqual(self.base("evidence", runs), self.sha["head"])

    def test_where_no_run_passed_master_plans_from_the_commit_before(self) -> None:
        self.assertEqual(self.base("evidence", {}), self.sha["before"])

    def test_a_pushed_commit_a_run_passed_at_plans_nothing_again(self) -> None:
        runs = {self.sha["evidence"]: self.PASSED}
        self.assertEqual(self.base("evidence", runs), self.sha["evidence"])

    def test_a_dispatch_or_a_run_that_did_not_pass_does_not_count(self) -> None:
        runs = {self.sha["head"]: [
            {"event": "workflow_dispatch", "conclusion": "success"},
            {"event": "push", "conclusion": "failure"},
            {"event": "push", "conclusion": "cancelled"},
        ]}
        self.assertEqual(self.base("evidence", runs), self.sha["before"])
        # The nightly's run counts: it ran every recipe.
        runs = {self.sha["head"]: [{"event": "schedule", "conclusion": "success"}]}
        self.assertEqual(self.base("evidence", runs), self.sha["head"])

    def test_a_merges_second_parent_does_not_count(self) -> None:
        self.assertEqual(self.base("merged", {self.sha["side"]: self.PASSED}), self.sha["before"])
        # Its first-parent line does: what the merge brought is planned since.
        self.assertEqual(self.base("merged", {self.sha["head"]: self.PASSED}), self.sha["head"])

    def test_an_error_plans_from_the_commit_before(self) -> None:
        def broken(_sha: str) -> list[dict]:
            raise RuntimeError("the API answered 403")
        with mock.patch("sys.stderr", new_callable=lambda: __import__("io").StringIO()) as err:
            try:
                got = vbase.base(self.sha["before"], self.sha["evidence"], broken, self.root)
            except RuntimeError as error:
                self.fail(f"A failing run provider must fall back to the previous commit: {error}")
        self.assertEqual(got, self.sha["before"])
        self.assertIn("the API answered 403", err.getvalue())
        # And a commit git does not know.
        with mock.patch("sys.stderr", new_callable=lambda: __import__("io").StringIO()):
            got = vbase.base(self.sha["before"], "0" * 40, lambda _s: self.PASSED, self.root)
        self.assertEqual(got, self.sha["before"])

    def test_the_browser_suites_run_again_only_where_more_than_records_changed(self) -> None:
        # Master's verify after train-3 ran the train's browser suites again,
        # on the same code (amended 2026-10-13).
        (self.root / "docs").mkdir()
        (self.root / "docs/STATUS.md").write_text("status")
        (self.root / "scripts").mkdir()
        (self.root / "scripts/ci_seconds.json").write_text("{}")
        self.git("checkout", "-q", "train")
        self.git("add", "docs", "scripts")
        self.git("commit", "-q", "-m", "records")
        records = self.git("rev-parse", "HEAD")
        self.assertFalse(vbase.browsers(self.sha["merged"], records, self.root))
        # Code since: run them.
        self.assertTrue(vbase.browsers(self.sha["before"], records, self.root))
        # An error runs them.
        with mock.patch("sys.stderr", new_callable=lambda: __import__("io").StringIO()):
            self.assertTrue(vbase.browsers("0" * 40, records, self.root))

    def test_the_runs_are_read_through_gh_by_their_commit(self) -> None:
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        log = self.root / "gh.log"
        gh = bin_dir / "gh"
        gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            f"open({str(log)!r}, 'a').write(' '.join(sys.argv[1:]) + '\\n')\n"
            "passed = 'head_sha=' + os.environ['PASSED_AT'] in sys.argv[2]\n"
            "print(json.dumps([{'event': 'push', 'conclusion': 'success'}] if passed else []))\n"
        )
        gh.chmod(0o755)
        env = {"PATH": f"{bin_dir}:/usr/bin:/bin", "GITHUB_REPOSITORY": "o/r", "PASSED_AT": self.sha["head"]}
        with mock.patch.dict("os.environ", env):
            got = vbase.base(self.sha["before"], self.sha["evidence"], root=self.root)
        self.assertEqual(got, self.sha["head"])
        calls = log.read_text().splitlines()
        self.assertEqual(len(calls), 2, "the evidence's runs, then the head's")
        self.assertIn(
            f"api repos/o/r/actions/workflows/verify.yml/runs?head_sha={self.sha['head']}&status=success",
            calls[1],
        )



class BoundEvidence(unittest.TestCase):
    STOP = "  memory bound: stopped `native-test`, which held more than 4 GiB\n"
    FINAL = "memory bound: a process was stopped in 1 of 1 mutants' runs\n"
    WATCH = "  the memory bound stopped watching: PermissionError('no /proc')\n"
    CLEAN = "1 of 1 mutants killed\nmemory bound: no process was stopped\n"

    def artifact(self, root, text, log="", status=0):
        paths = ["docs/evidence/E14/stopped.txt", "docs/evidence/E14/stopped.wasm"]
        good = "docs/evidence/E14/good.txt"
        row = {"recipe": "e14-stopped", "status": status, "seconds": 17, "wrote": paths}
        neighbor = {"recipe": "e14-good", "status": 0, "seconds": 23, "wrote": [good]}
        for path, data in [(paths[0], text.encode()), (paths[1], b"\0asm"), (good, self.CLEAN.encode())]:
            target = root / "evidence" / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        (root / "logs").mkdir(exist_ok=True)
        (root / "logs/e14-stopped.log").write_text(log)
        (root / "results.json").write_text(json.dumps([row, neighbor]))
        return row, neighbor

    def summary(self, root):
        return subprocess.run([sys.executable, str(SCRIPTS / "ci_summary.py"), str(root), "--shards", "1"],
                              capture_output=True, text=True)

    def test_summary_refuses_each_stop_and_failed_watcher(self):
        for text in [self.STOP, self.FINAL, self.WATCH, self.STOP + self.CLEAN, self.WATCH + self.CLEAN]:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                self.artifact(root, text)
                done = self.summary(root)
                self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
                self.assertIn("**2 recipes, 1 failed**", done.stdout)
                self.assertIn("| `e14-stopped` | FAILED |", done.stdout)
                self.assertIn("| `e14-good` | ok |", done.stdout)

    def test_recipe_log_alone_invalidates_summary_fetch_and_preverify(self):
        import preverify
        for text in [self.STOP, self.WATCH]:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                row, _ = self.artifact(root, self.CLEAN, log=text)
                self.assertEqual(self.summary(root).returncode, 1)
                rows = fetch.results_of([root])
                self.assertEqual(rows[0]["status"], 0)
                self.assertEqual(fetch.failed_files(rows), set(row["wrote"]))
                self.assertEqual(fetch.timed(rows), {"e14-good": 23})
                local = preverify.result_of("e14-stopped", 0, root, "done", [])
                self.assertEqual(local.verdict, "FAILED")
                self.assertIn(text.strip(), local.reason)
                self.assertEqual(preverify.result_of("e14-good", 0, root, "done", []).verdict, "ok")

    def test_fetch_excludes_all_outputs_and_completed_timing_of_stopped_recipe(self):
        for text in [self.STOP, self.FINAL, self.WATCH]:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                row, _ = self.artifact(root, text)
                rows = fetch.results_of([root])
                self.assertEqual(fetch.failed_files(rows), set(row["wrote"]))
                self.assertEqual(fetch.timed(rows), {"e14-good": 23})
                self.assertEqual(rows[0]["status"], 0, "retain the producer's native status")

    def test_bound_controls_require_a_complete_actual_intended_assertion(self):
        import mutation_bound_mutations as controls
        def result(**changes):
            fields = dict(valid=True, body_errors=[], executed=1, assertion_failed=["intended"])
            fields.update(changes)
            return types.SimpleNamespace(result=types.SimpleNamespace(**fields))
        self.assertEqual(controls.credit([result()], [["intended"]], False, "intended"), "KILLED")
        for changes in [dict(valid=False), dict(body_errors=["setup error"]), dict(executed=0)]:
            with self.subTest(changes=changes):
                self.assertEqual(controls.credit([result(**changes)], [["intended"]], False, "intended"), "REFUSED")
        self.assertEqual(controls.credit([result()], [["intended"]], True, "intended"), "REFUSED")
        self.assertEqual(controls.credit([], [["intended"]], False, "intended"), "REFUSED")
        self.assertEqual(controls.credit([result(assertion_failed=["another"])], [["intended"]], False, "intended"), "SURVIVED")
        self.assertEqual(controls.credit([result(assertion_failed=[])], [["intended"]], False, "intended"), "SURVIVED")

    def test_exact_no_stop_and_absent_guard_remain_valid(self):
        import preverify
        for text in [self.CLEAN, "ordinary successful recipe\n"]:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                self.artifact(root, text, log=text)
                self.assertEqual(self.summary(root).returncode, 0)
                rows = fetch.results_of([root])
                self.assertEqual(fetch.failed_files(rows), set())
                self.assertEqual(fetch.timed(rows), {"e14-stopped": 17, "e14-good": 23})
                self.assertEqual(preverify.result_of("e14-stopped", 0, root, "done", []).verdict, "ok")

    def test_known_summary_cannot_copy_stopped_recipe_from_historical_green_run(self):
        import shutil
        for conclusion in ["success", "failure"]:
            with self.subTest(conclusion=conclusion), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                artifact = root / "artifact"
                artifact.mkdir()
                row, good = self.artifact(artifact, self.STOP)
                destination = root / "checkout"
                (destination / "scripts").mkdir(parents=True)
                seconds = destination / "scripts/ci_seconds.json"
                old = destination / row["wrote"][0]
                old.parent.mkdir(parents=True)
                old.write_text("prior evidence must remain")
                run = {"headSha": "a" * 40, "url": "https://example.invalid/run/1", "status": "completed",
                       "conclusion": conclusion, "jobs": [{"name": "summary", "conclusion": conclusion}]}
                def gh(*args):
                    if args[:2] == ("run", "view"):
                        return json.dumps(run)
                    self.assertEqual(args[:2], ("run", "download"))
                    shutil.copytree(artifact, pathlib.Path(args[args.index("--dir") + 1]), dirs_exist_ok=True)
                    return ""
                with mock.patch.object(fetch, "ROOT", destination), mock.patch.object(fetch, "SECONDS", seconds), \
                     mock.patch.object(fetch, "gh", side_effect=gh), mock.patch.object(fetch, "moved_on", return_value=set()), \
                     mock.patch.object(fetch.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout=run["headSha"])), \
                     mock.patch.object(sys, "argv", ["evidence_fetch.py", "1", "--known", "summary"]):
                    self.assertEqual(fetch.main(), 0)
                self.assertEqual(old.read_text(), "prior evidence must remain")
                self.assertFalse((destination / row["wrote"][1]).exists())
                accepted = destination / good["wrote"][0]
                self.assertTrue(accepted.is_file(), "the valid no-stop neighbor must be copied")
                self.assertEqual(accepted.read_text(), self.CLEAN)
                self.assertTrue(seconds.is_file(), "valid completed timing must be recorded")
                self.assertEqual(json.loads(seconds.read_text()), {"e14-good": 23})

if __name__ == "__main__":
    unittest.main()
