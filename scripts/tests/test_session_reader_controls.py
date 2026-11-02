"""Producer contract: invalid runs and failed restoration cannot earn success."""
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
spec = importlib.util.spec_from_file_location("session_reader_controls_under_test", SCRIPTS / "session_reader_mutations.py")
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
Native = namedtuple("Native", "valid passed failed")


class ProducerTests(unittest.TestCase):
    def run_probe(self, results, *, changed_test=False, anchor="guard", error=None):
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
                "ROOT": root, "SOURCE": source,
                "MUTANTS": [("fixture guard", product, anchor, "unguarded")],
                "run_tests": run,
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

    def test_complete_native_assertion_and_restoration_are_required(self):
        status, report, product, _ = self.run_probe([Native(True, 4, 0), Native(True, 3, 1)])
        self.assertEqual(status, 0)
        self.assertTrue(report["valid"])
        self.assertEqual(product, "guard")

    def test_invalid_baseline_never_runs_controls(self):
        status, report, _, count = self.run_probe([Native(False, 0, 0)])
        self.assertEqual(status, 1)
        self.assertEqual(count, 1)
        self.assertEqual(report["controls"], [])

    def test_invalid_native_failure_never_counts_as_a_kill(self):
        status, report, product, _ = self.run_probe([Native(True, 4, 0), Native(False, 3, 1)])
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertFalse(report["controls"][0]["killed"])
        self.assertEqual(product, "guard")

    def test_final_source_mismatch_forces_nonzero_exit(self):
        status, report, product, _ = self.run_probe([Native(True, 4, 0), Native(True, 3, 1)], changed_test=True)
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertFalse(report["restored"])
        self.assertEqual(product, "guard")

    def test_missing_anchor_is_invalid(self):
        status, report, product, _ = self.run_probe([Native(True, 4, 0)], anchor="absent")
        self.assertEqual(status, 1)
        self.assertFalse(report["valid"])
        self.assertEqual(product, "guard")

    def test_cancellation_restores_source_and_propagates(self):
        _, report, product, _ = self.run_probe([Native(True, 4, 0)], error=KeyboardInterrupt())
        self.assertTrue(report["restored"])
        self.assertFalse(report["valid"])
        self.assertEqual(product, "guard")


if __name__ == "__main__":
    unittest.main()
