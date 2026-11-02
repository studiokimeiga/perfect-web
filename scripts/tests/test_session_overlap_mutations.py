"""Producer contract of scripts/session_overlap_mutations.py (ADR-0389, amended
2026-11-02): no database, an invalid run, a survivor or a source left changed
never earns success, and the source is restored whatever happens."""
from collections import namedtuple
from contextlib import ExitStack, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("session_overlap_controls_under_test",
                                              SCRIPTS / "session_overlap_mutations.py")
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
Native = namedtuple("Native", "valid passed failed")
TESTS = len(probe.TEST_NAMES)


class ProducerTests(unittest.TestCase):
    def run_probe(self, results, *, database=True, changed_test=False, anchor="guard", error=None):
        with tempfile.TemporaryDirectory() as folder, ExitStack() as stack:
            root = Path(folder)
            product = root / "product.rs"
            source = root / "witness.rs"
            report = root / "report.json"
            product.write_text("guard")
            source.write_text("assert!(true);")
            calls = iter(results)
            count = 0

            def run():
                nonlocal count
                count += 1
                if count == 2 and changed_test:
                    source.write_text("a different test source")
                if count == 2 and error:
                    raise error
                return next(calls)

            for name, value in {
                "ROOT": root, "SOURCES": [source],
                "MUTANTS": [("fixture guard", product, anchor, "unguarded")],
                "run_tests": run, "database_named": lambda: database,
            }.items():
                stack.enter_context(patch.object(probe, name, value))
            stack.enter_context(patch.object(sys, "argv", ["probe", "--report", str(report)]))
            stack.enter_context(redirect_stdout(io.StringIO()))
            if error:
                with self.assertRaises(type(error)):
                    probe.main()
                status = None
            else:
                status = probe.main()
            return status, json.loads(report.read_text()), product.read_text(), count

    def test_a_complete_native_assertion_kill_and_restoration_succeed(self):
        status, report, product, _ = self.run_probe([Native(True, TESTS, 0), Native(True, TESTS - 1, 1)])
        self.assertEqual(status, 0)
        self.assertTrue(report["valid"])
        self.assertEqual(product, "guard")

    def test_no_database_runs_nothing(self):
        status, report, _, count = self.run_probe([Native(True, TESTS, 0)], database=False)
        self.assertEqual(status, 1)
        self.assertEqual(count, 0)
        self.assertFalse(report["valid"])

    def test_an_incomplete_baseline_never_runs_controls(self):
        status, report, _, count = self.run_probe([Native(True, TESTS - 1, 0)])
        self.assertEqual(status, 1)
        self.assertEqual(count, 1)
        self.assertEqual(report["controls"], [])

    def test_an_invalid_native_failure_never_counts_as_a_kill(self):
        status, report, product, _ = self.run_probe([Native(True, TESTS, 0), Native(False, TESTS - 1, 1)])
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertFalse(report["controls"][0]["killed"])
        self.assertEqual(product, "guard")

    def test_a_survivor_fails_the_run(self):
        status, report, product, _ = self.run_probe([Native(True, TESTS, 0), Native(True, TESTS, 0)])
        self.assertEqual(status, 1)
        self.assertFalse(report["controls"][0]["killed"])
        self.assertEqual(product, "guard")

    def test_a_test_source_changed_during_the_run_fails_it(self):
        status, report, product, _ = self.run_probe([Native(True, TESTS, 0), Native(True, TESTS - 1, 1)],
                                                    changed_test=True)
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertFalse(report["restored"])
        self.assertEqual(product, "guard")

    def test_a_missing_anchor_is_invalid(self):
        status, report, product, _ = self.run_probe([Native(True, TESTS, 0)], anchor="absent")
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertEqual(product, "guard")

    def test_cancellation_restores_the_source_and_propagates(self):
        _, report, product, _ = self.run_probe([Native(True, TESTS, 0)], error=KeyboardInterrupt())
        self.assertTrue(report["restored"])
        self.assertFalse(report["valid"])
        self.assertEqual(product, "guard")


if __name__ == "__main__":
    unittest.main()
