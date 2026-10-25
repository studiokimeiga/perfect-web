"""The head-to-head's judges and counter (ADR-XXXX), each held to real or
exact inputs with its control:

- `head_to_head_loc.py`: a line counts when it holds anything but white
  space and comments, by each language's own comments, strings read as
  strings, Rust's test modules left out;
- `head_to_head_cypress.py`: TodoMVC's own React and Svelte apps pass every
  official test but the one the suite's own table skips for them (their
  gap); anything else fails. Held to Cypress's real output for each
  (`fixtures/head-to-head/cypress-*.txt`, the run of 2026-10-25);
- `head_to_head_behaviour.py`: each stack passes every specification test in
  every engine but its written gaps, which must fail in all three.

Each test is a witness of a mutation control in
`scripts/head_to_head_mutations.py`.
"""

import importlib.util
import pathlib
import sys
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent
FIXTURES = pathlib.Path(__file__).resolve().parent / "fixtures" / "head-to-head"
sys.path.insert(0, str(SCRIPTS))


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


loc = load("head_to_head_loc")
cypress = load("head_to_head_cypress")
behaviour = load("head_to_head_behaviour")


class Lines(unittest.TestCase):
    def count(self, name, text):
        return loc.code_lines(pathlib.Path(name), text)

    def test_comments_are_not_code_and_strings_are(self):
        js = "// a comment\nconst url = 'http://x'; // after code\n/* a\n block */\n\nlet b = `// in a template`;\n"
        self.assertEqual(self.count("a.js", js), 2)
        # A template literal's line that begins `//` is the string's, not a comment.
        css = "const sheet = `\n// a line of the string\n`;\n"
        self.assertEqual(self.count("a.js", css), 3)
        ts = "/** doc */\nexport const a: number = 1;\n"
        self.assertEqual(self.count("a.ts", ts), 1)

    def test_pleris_reads_both_comment_forms(self):
        pw = '// the page\nmodule app\n<!-- a view\n comment -->\nlet s = "// kept"\n'
        self.assertEqual(self.count("a.pw", pw), 2)

    def test_rust_block_comments_nest_and_test_modules_are_left_out(self):
        rs = "/* outer /* inner */ still outer */\nfn main() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn after() {}\n"
        self.assertEqual(self.count("a.rs", rs), 2)

    def test_css_has_block_comments_only(self):
        css = "/* note */\n.a { background: url(//cdn/x.png); }\n"
        self.assertEqual(self.count("a.css", css), 1)

    def test_a_svelte_component_is_read_by_each_part(self):
        svelte = (
            "<script>\n  // state\n  let a = 1;\n</script>\n\n<!-- markup note -->\n<p>{a}</p>\n"
            "<style>\n  /* look */\n  p { color: red; }\n</style>\n"
        )
        self.assertEqual(self.count("A.svelte", svelte), 7)

    def test_shell_comments(self):
        self.assertEqual(self.count("a.sh", "#!/usr/bin/env bash\n# note\nset -e\n"), 1)


class Cypress(unittest.TestCase):
    def judge(self, framework, text, status=0):
        spec_text = cypress.cy.SPEC.read_text(encoding="utf-8")
        tests = cypress.cy.spec_tests(spec_text)
        run = cypress.cy.parse_run(text)
        return cypress.judge(framework, tests, run, status, spec_text)

    def test_each_apps_real_run_is_as_expected_its_gap_reported(self):
        for framework in ("react", "svelte"):
            text = (FIXTURES / f"cypress-{framework}.txt").read_text(encoding="utf-8")
            st, problems = self.judge(framework, text)
            self.assertEqual(problems, [], framework)
            pending = [t.title for t in cypress.cy.spec_tests(cypress.cy.SPEC.read_text()) if st[t.n].state == "pending"]
            self.assertEqual(pending, [cypress.cy.PERSISTENCE])

    def test_a_gap_that_passes_is_a_problem(self):
        text = (FIXTURES / "cypress-react.txt").read_text(encoding="utf-8")
        passed = text.replace("      - should persist its data", "      ✓ should persist its data (10ms)")
        passed = passed.replace("Passing:      28", "Passing:      29").replace("Pending:      1 ", "Pending:      0 ")
        _, problems = self.judge("react", passed)
        self.assertTrue(any("skips it for react" in p for p in problems), problems)

    def test_a_test_that_fails_is_a_problem(self):
        text = (FIXTURES / "cypress-react.txt").read_text(encoding="utf-8")
        failed = text.replace("      ✓ should trim text input (386ms)", "      1) should trim text input")
        _, problems = self.judge("react", failed, 1)
        self.assertTrue(any("should trim text input" in p for p in problems), problems)

    def test_a_run_with_no_results_is_a_problem(self):
        _, problems = self.judge("svelte", "Cypress could not start\n", 1)
        self.assertTrue(any("no results table" in p for p in problems), problems)


def report(statuses):
    """A Playwright JSON report: {(stack, engine): {title: status}}."""
    specs = {}
    for (stack, engine), tests in statuses.items():
        for title, status in tests.items():
            specs.setdefault(title, []).append(
                {"projectName": f"{stack}-{engine}", "results": [{"status": status}]}
            )
    return {"suites": [{"specs": [{"title": t, "tests": ts} for t, ts in specs.items()]}]}


def expected():
    """Every stack as the judge expects it: its gaps failing, its other
    tests passing, twenty a stack, in each engine."""
    out = {}
    for stack in behaviour.STACKS:
        gaps = list(behaviour.GAPS[stack])
        titles = gaps + [f"behaviour {n}" for n in range(behaviour.TESTS - len(gaps))]
        for engine in behaviour.ENGINES:
            out[(stack, engine)] = {t: ("failed" if t in gaps else "passed") for t in titles}
    return out


class Behaviour(unittest.TestCase):
    def test_every_stack_as_written_passes(self):
        _, problems = behaviour.judge(report(expected()))
        self.assertEqual(problems, [])

    def test_a_gap_that_passes_in_one_engine_is_a_problem(self):
        statuses = expected()
        statuses[("react", "webkit")]["persistence: the todos and their states survive a reload"] = "passed"
        _, problems = behaviour.judge(report(statuses))
        self.assertTrue(any("react in webkit" in p and "known gap" in p for p in problems), problems)

    def test_a_new_failure_is_a_problem(self):
        statuses = expected()
        statuses[("pleris", "firefox")]["behaviour 3"] = "failed"
        _, problems = behaviour.judge(report(statuses))
        self.assertTrue(any("pleris in firefox" in p and "behaviour 3" in p for p in problems), problems)

    def test_an_engine_that_ran_nothing_is_a_problem(self):
        statuses = expected()
        del statuses[("svelte", "chromium")]
        _, problems = behaviour.judge(report(statuses))
        self.assertTrue(any("svelte in chromium: no test ran" in p for p in problems), problems)

    def test_cypress_counts_are_read_from_each_judges_report(self):
        self.assertEqual(behaviour.cypress_counts("acceptance: 29 of 29 passed\n"), (29, 29))
        self.assertEqual(behaviour.cypress_counts("x\nreact: 28 of 29 passed; 0 failed\n"), (28, 29))
        self.assertIsNone(behaviour.cypress_counts("the suite did not run\n"))


if __name__ == "__main__":
    unittest.main()
