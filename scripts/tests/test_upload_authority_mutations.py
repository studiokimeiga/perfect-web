"""Adversarial calibration of upload authority's strict native acceptance."""
from importlib import import_module
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
gate = import_module("upload_authority_mutations")


class UploadAuthorityProducer(unittest.TestCase):
    def test_native_inventory_is_exact_and_refuses_ignored_and_foreign_entries(self):
        expected = ("scope::one", "scope::two")
        self.assertEqual(gate.native_list("scope::one: test\nscope::two: test\n", ("scope::",)), expected)
        for raw in ["", "scope::one: test\nscope::one: test\n", "foreign::one: test\n",
                    "scope::one: benchmark\n", "scope::one: test (ignored)\n"]:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                gate.native_list(raw, ("scope::",))

    def test_source_inventory_refuses_missing_duplicate_and_macro_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tests.rs"
            path.write_text('#[test]\nfn one() {}\n#[test]\nfn two() {}\n')
            self.assertEqual(gate.declarations(path, "p::"), ("p::one", "p::two"))
            for source in ["", '#[test]\nfn one() {}\n#[test]\nfn one() {}\n',
                           '#[test]\nfn one() {}\ncases! { two; }\n']:
                path.write_text(source)
                with self.assertRaises(ValueError):
                    gate.declarations(path, "p::")

    def test_catalogue_exposes_all_exact_source_edits_to_preflight(self):
        self.assertEqual({control[0] for control in gate.CONTROLS}, {entry[0] for entry in gate.MUTANTS})
        self.assertEqual(len(gate.MUTANTS), sum(len(control[2]) for control in gate.CONTROLS))
        self.assertEqual(len(gate.CONTROLS), len({control[0] for control in gate.CONTROLS}))
        self.assertTrue(all(control[3] for control in gate.CONTROLS))

    def test_domain_assertion_site_excludes_helper_and_setup_assertions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tests.rs"
            source.write_text('fn helper() {\n    assert!(false, "fixture setup");\n}\n'
                              '#[test]\nfn one() {\n    assert_eq!(\n        actual(), 404,\n'
                              '        "private bytes are refused"\n    );\n}\n')
            specs = [(source, "p::")]
            witness = [("p::one", "private bytes are refused")]
            with patch.object(gate, "ROOT", root):
                sites = gate.witness_sites(specs, witness)
                self.assertEqual(sites, {"p::one": (str(source), 6, 5)})
                self.assertTrue(gate.failure_sites(self.failed("p::one", "tests.rs", 6, 5), sites))
                self.assertFalse(gate.failure_sites(self.failed("p::one", "tests.rs", 2, 5), sites))
                self.assertFalse(gate.failure_sites(self.failed("p::other", "tests.rs", 6, 5), sites))
                self.assertFalse(gate.failure_sites(self.failed("p::one", "tests.rs", 6, 1), sites))
                self.assertFalse(gate.failure_sites("", sites, ["p::one"]))
                self.assertTrue(gate.failure_sites("\x1b[31m" + self.failed("p::one", "tests.rs", 6, 5) + "\x1b[0m", sites, ["p::one"]))
                self.assertFalse(gate.failure_sites("\x1b[31m" + self.failed("p::one", "tests.rs", 2, 5) + "\x1b[0m", sites, ["p::one"]))
            source.write_text(source.read_text().replace('actual(), 404,', 'actual(), "private bytes are refused",'))
            with self.assertRaises(ValueError):
                gate.witness_sites(specs, witness)

    def test_shared_domain_helper_requires_its_explicit_function_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "tests.rs"
            source.write_text('fn privacy() {\n    assert!(false, "refuse private bytes");\n}\n'
                              '#[test]\nfn one() { privacy(); }\n')
            with self.assertRaises(ValueError):
                gate.witness_sites([(source, "p::")], [("p::one", "refuse private bytes")])
            sites = gate.witness_sites([(source, "p::")], [("p::one", "refuse private bytes", "privacy")])
            self.assertEqual(sites, {"p::one":(str(source), 2, 5)})

    @staticmethod
    def failed(name, path, line, column):
        return (f'test {name} ... FAILED\n\nfailures:\n\n---- {name} stdout ----\n'
                f"thread '{name}' panicked at {path}:{line}:{column}:\nassertion failed\n\n"
                f'failures:\n    {name}\n\ntest result: FAILED. 0 passed; 1 failed; '
                '0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n')

    def test_native_assertion_validation_rejects_build_setup_timeout_and_partial_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tests.rs"
            source.write_text('#[test]\nfn one() {\n    assert!(false, "privacy witness");\n}\n')
            frozen = {"names": ["p::one"], "sources": ["tests.rs"]}
            sites = {"p::one": (str(source), 3, 5)}
            raw = self.failed("p::one", "tests.rs", 3, 5)
            with patch.object(gate, "ROOT", root), patch.object(gate, "drift", return_value=False):
                self.assertTrue(gate.evaluate(101, raw, frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(1, raw, frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(None, raw, frozen, {}, sites)["valid"])
                setup = self.failed("p::one", "tests.rs", 1, 1)
                self.assertFalse(gate.evaluate(101, setup, frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(101, raw.replace('0 ignored;', '1 ignored;'), frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(101, raw.replace('p::one', 'p::foreign'), frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(101, 'error: could not compile\n', frozen, {}, sites)["valid"])
                self.assertFalse(gate.evaluate(101, raw + 'UPLOAD_AUTHORITY_CLEANUP_ERROR fixture\n', frozen, {}, sites)["valid"])
                with patch.object(gate, "drift", return_value=True):
                    self.assertFalse(gate.evaluate(101, raw, frozen, {}, sites)["valid"])

    def test_exact_head_requires_all_source_and_recipe_inputs_committed(self):
        from types import SimpleNamespace
        for status, extra, expected in [(0, "", True), (1, "", False), (0, "scripts/new.py", False),
                                        (0, "just/new.just", False)]:
            with patch.object(gate.subprocess, "run", return_value=SimpleNamespace(returncode=status)) as diff, \
                 patch.object(gate, "git", return_value=extra) as listing:
                self.assertEqual(gate.committed_inputs(), expected)
                self.assertIn("packages", diff.call_args.args[0])
                self.assertIn("justfile", listing.call_args.args)

    def test_process_errors_keep_raw_output_and_never_acquire_native_status(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            for error in [OSError("launch"), subprocess.TimeoutExpired(["child"], 1, output="partial")]:
                log = output / type(error).__name__
                with patch.object(gate.producer_process, "run", side_effect=error):
                    record, text = gate.process(["unused"], log, 1, {})
                self.assertIsNone(record["status"])
                self.assertTrue(log.is_file())
                self.assertEqual(record["log_sha256"], gate.digest(log))
                self.assertEqual(text, "partial" if isinstance(error, subprocess.TimeoutExpired) else "")

    def test_process_cancellation_keeps_receipt_and_propagates(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "cancelled.log"
            with patch.object(gate.producer_process, "run", side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    gate.process(["unused"], log, 1, {})
            self.assertTrue(log.is_file())

    def test_actual_child_completion_and_timeout_retain_raw_logs_and_reap_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            record, text = gate.process([sys.executable, "-c", 'print("complete native output", flush=True)'],
                                        output / "complete.log", 5, {})
            self.assertEqual(record["status"], 0)
            self.assertTrue(record["reaped"])
            self.assertEqual(text, "complete native output\n")
            receipt = json.loads((output / record["process_receipt"]).read_text())
            self.assertEqual(receipt["reaped"]["status"], 0)
            self.assertFalse(receipt["reaped"]["timed_out"])
            record, text = gate.process([sys.executable, "-c", 'import time; print("partial output", flush=True); time.sleep(30)'],
                                        output / "timeout.log", .3, {})
            self.assertIsNone(record["status"])
            self.assertFalse(record["reaped"], "timed-out native output is never acceptance")
            self.assertEqual(text, "partial output\n")
            receipt = json.loads((output / record["process_receipt"]).read_text())
            self.assertTrue(receipt["reaped"]["timed_out"])
            self.assertIsInstance(receipt["reaped"]["status"], int)
            self.assertEqual(record["error"], "TimeoutExpired")

    def test_inventory_crosschecks_native_and_declared_identities_before_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tests.rs"
            source.write_text('#[test]\nfn one() {}\n')
            for native, expected in [("p::one: test\n", True), ("p::other: test\n", False)]:
                with patch.object(gate, "ROOT", root), patch.object(gate, "SOURCES", [(source, "p::")]), \
                     patch.object(gate, "process", return_value=({"status":0,"reaped":True}, native)), \
                     patch.object(gate, "drift", return_value=False):
                    if expected:
                        self.assertEqual(gate.inventory(root, {}, {}, 1)["names"], ("p::one",))
                    else:
                        with self.assertRaisesRegex(ValueError, "inventories differ"):
                            gate.inventory(root, {}, {}, 1)

    def test_native_run_refuses_a_green_result_without_producer_reaping(self):
        good = "test p::one ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            frozen = {"names":["p::one"],"sources":[]}
            with patch.object(gate, "process", return_value=({"status":0,"reaped":False}, good)), \
                 patch.object(gate, "drift", return_value=False):
                self.assertFalse(gate.run_native(frozen, output, "test", {}, {}, 1)["valid"])

    def fake_campaign(self, directory, native):
        root = Path(directory)
        source = root / "source.rs"
        source.write_text("anchor\n")
        output = root / "evidence"
        control = [("one", "test control", [(source, "anchor", "replacement")], [])]
        patches = [patch.object(gate, "ROOT", root), patch.object(gate, "CONTROLS", control),
                   patch.object(gate, "SOURCES", []), patch.object(gate, "witness_sites", return_value={}),
                   patch.object(gate, "inputs", side_effect=lambda: {"source.rs":gate.digest(source)}),
                   patch.object(gate, "git", side_effect=lambda *a: "fixed" if a == ("rev-parse", "HEAD") else ""),
                   patch.object(gate, "inventory", return_value={"names":["p::one"]}),
                   patch.object(gate, "run_native", side_effect=native)]
        return source, output, patches

    def test_final_green_baseline_is_required_after_every_restored_kill(self):
        from contextlib import ExitStack
        good = {"valid":True,"native":{"failed":0}}
        killed = {"valid":True,"native":{"failed":1}}
        for final, expected in [(good, 0), ({"valid":False,"native":{"failed":0}}, 1), (killed, 1)]:
            with tempfile.TemporaryDirectory() as directory:
                source, output, patches = self.fake_campaign(directory, [good, killed, final])
                with ExitStack() as stack:
                    for p in patches:
                        stack.enter_context(p)
                    self.assertEqual(gate.main(["--output", str(output)]), expected)
                report = json.loads((output / "report.json").read_text())
                self.assertEqual(source.read_text(), "anchor\n")
                self.assertTrue(report["controls"][0]["killed"])
                self.assertEqual(report["valid"], expected == 0)
                self.assertIsNotNone(report["final_baseline"])

    def test_cancellation_restores_source_and_keeps_invalid_evidence(self):
        from contextlib import ExitStack
        good = {"valid":True,"native":{"failed":0}}
        with tempfile.TemporaryDirectory() as directory:
            source, output, patches = self.fake_campaign(directory, [good, KeyboardInterrupt])
            with ExitStack() as stack:
                for p in patches:
                    stack.enter_context(p)
                with self.assertRaises(KeyboardInterrupt):
                    gate.main(["--output", str(output)])
            self.assertEqual(source.read_text(), "anchor\n")
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(report["abort"], "KeyboardInterrupt")
            self.assertFalse(report["valid"])
            self.assertTrue(report["restored"])

    def test_survivor_or_invalid_native_run_is_never_credited(self):
        self.assertFalse(gate.green({"valid":False,"native":{"failed":0}}))
        self.assertFalse(gate.killed({"valid":False,"native":{"failed":1}}))
        self.assertFalse(gate.killed({"valid":True,"native":{"failed":0}}))
        self.assertTrue(gate.killed({"valid":True,"native":{"failed":1}}))


if __name__ == "__main__":
    unittest.main()
