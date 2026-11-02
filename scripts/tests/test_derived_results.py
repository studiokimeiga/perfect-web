"""Derived controls must retain their inventory and refuse broken producers."""
import contextlib
import importlib.util
import io
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location("derived_controls", Path(__file__).resolve().parents[1] / "derived_mutations.py")
controls = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controls)


def green(name):
    return SimpleNamespace(returncode=0, stdout=f"running 1 test\ntest {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")


class Results(unittest.TestCase):
    def test_equal_counts_do_not_replace_the_frozen_named_inventory(self):
        with patch.object(controls, "TESTS", [["first"], ["second"]]), patch.object(controls, "INVENTORY", {}), patch.object(controls.producer_process, "run", side_effect=[
                green("first"), green("second"), green("first"), green("renamed")]), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(controls.run_tests(), (True, 2, 0))
            self.assertFalse(controls.run_tests()[0])

    def test_a_failed_build_or_contradictory_status_is_not_a_kill(self):
        for child in [SimpleNamespace(returncode=101, stdout="error: failed to compile"),
                      SimpleNamespace(returncode=101, stdout=green("first").stdout)]:
            with patch.object(controls, "INVENTORY", {}), patch.object(controls.producer_process, "run", return_value=child), contextlib.redirect_stdout(io.StringIO()):
                self.assertFalse(controls.run_tests()[0])


if __name__ == "__main__":
    unittest.main()
