"""A broken producer must never be evidence that an image-privacy fault died."""
import contextlib
import importlib.util
import io
from pathlib import Path
from types import SimpleNamespace
import tempfile
import sys
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    "upload_metadata_controls", Path(__file__).resolve().parents[1] / "upload_metadata_mutations.py"
)
controls = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controls)


class Results(unittest.TestCase):
    def rust_result(self, status, output):
        with patch.object(controls.subprocess, "run", return_value=SimpleNamespace(
            returncode=status, stdout=output, stderr=""
        )):
            return controls.rust()

    def test_missing_zero_and_inconsistent_results_are_invalid(self):
        for status, output in [
            (101, "error: failed to compile"),
            (0, "test result: ok. 0 passed; 0 failed"),
            (1, "test result: ok. 85 passed; 0 failed"),
            (0, "test result: FAILED. 84 passed; 1 failed"),
        ]:
            with self.subTest(status=status, output=output):
                self.assertFalse(self.rust_result(status, output)[0])

    def test_actual_assertion_failure_and_green_neighbour_are_valid(self):
        self.assertEqual(self.rust_result(101, "test result: FAILED. 84 passed; 1 failed"), (True, 84, 1))
        self.assertEqual(self.rust_result(0, "test result: ok. 85 passed; 0 failed"), (True, 85, 0))

    def test_browser_infrastructure_is_not_an_assertion_kill(self):
        for message in ["worker process exited unexpectedly", "browserType.launch: missing binary",
                        "Test timeout of 30000ms exceeded", "ECONNREFUSED", "1 did not run"]:
            self.assertFalse(controls.browser_result(f"{message}\n  1 failed\n", 1)[0])
        self.assertEqual(controls.browser_result("Error: expect(received).toEqual(expected)\n  1 failed\n", 1), (True, 0, 1))
        self.assertEqual(controls.browser_result("  36 passed\n", 0), (True, 36, 0))
        self.assertFalse(controls.browser_result("  36 passed\n", 1)[0])

    def test_invalid_mutant_fails_campaign_and_restores_source(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / "source.rs"
            source.write_text("original")
            runner = Mock(side_effect=[(True, 85, 0), (False, 0, 0)])
            with patch.object(controls, "MUTANTS", [("fault", controls.RUST, source, "original", "changed")]), \
                 patch.object(controls, "SUITES", {controls.RUST: runner}), \
                 patch.object(controls.subprocess, "run", return_value=SimpleNamespace(returncode=0)), \
                 patch.object(controls.signal, "signal"), \
                 patch.dict(sys.modules, {"mutation_baseline": Mock()}), \
                 contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(controls.main(), 1)
            self.assertEqual(source.read_text(), "original")
            self.assertIn("INVALID", output.getvalue())
            self.assertIn("0 of 1 mutants killed", output.getvalue())


if __name__ == "__main__":
    unittest.main()
