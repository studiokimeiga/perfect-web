"""Adversarial calibration of the bounded command producer's acceptance seams."""
from importlib import import_module
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
gate = import_module("session_command_mutations")


class SessionCommandProducer(unittest.TestCase):
    def test_generic_ci_catalogue_exports_every_canonical_control_and_edit(self):
        self.assertEqual({entry[0] for entry in gate.MUTANTS},
                         {control[0] for control in gate.CONTROLS})
        self.assertEqual(len(gate.MUTANTS),
                         sum(len(control[3]) for control in gate.CONTROLS))
        self.assertTrue({"private-trace-payload", "private-trace-error"}
                        <= {entry[0] for entry in gate.MUTANTS})

    def test_exact_head_requires_tracked_and_untracked_source_and_recipes_to_be_committed(self):
        from types import SimpleNamespace
        for status, untracked, expected in [(0, "", True), (1, "", False), (0, "runtime/extra.rs", False), (0, "packages/pw-std/extra.pw", False), (0, "just/extra.just", False)]:
            with patch.object(gate.subprocess, "run", return_value=SimpleNamespace(returncode=status)) as diff, patch.object(gate, "git", return_value=untracked) as listing:
                self.assertEqual(gate.committed_inputs(), expected)
                self.assertIn("packages", diff.call_args.args[0])
                self.assertIn("just", diff.call_args.args[0])
                self.assertIn("justfile", listing.call_args.args)

    def test_native_inventory_refuses_duplicates_missing_and_foreign_names(self):
        self.assertEqual(gate.native_list("scope::one: test\nscope::two: test\n", "scope::"),
                         ("scope::one", "scope::two"))
        for output in ["", "scope::one: test\nscope::one: test\n", "foreign::one: test\n"]:
            with self.assertRaises(ValueError):
                gate.native_list(output, "scope::")

    def test_macro_inventory_is_explicit_and_fails_unknown_forms(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "pg.rs"
            source.write_text("#[test]\nfn direct() {}\ncases! {\n    feed_case,Feed,checks;\n    store_case,Store,checks;\n}\n")
            with patch.object(gate, "POSTGRES", source):
                self.assertEqual(gate.declaration_names(source, "p::"),
                                 ("p::direct", "p::feed_case", "p::store_case"))
                source.write_text("cases! {\n mystery,Other,check;\n}\n")
                with self.assertRaises(ValueError):
                    gate.declaration_names(source, "p::")

    def test_mixed_resource_source_only_credits_the_unique_eof_test_module(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "lib.rs"
            source.write_text("fn product() {\n    assert!(false);\n}\n#[cfg(test)]\nmod fallible_command_tests {\n    #[test]\n    fn check() {\n        assert!(false);\n    }\n}\n")
            with patch.object(gate, "RESOURCE", source):
                self.assertTrue(gate.library_sites(f"panicked at {source}:8:9:\n"))
                self.assertFalse(gate.library_sites(f"panicked at {source}:2:5:\n"))
                self.assertEqual(gate.declaration_names(source, "fallible_command_tests::"),
                                 ("fallible_command_tests::check",))
                source.write_text(source.read_text() + "fn product_tail() {}\n")
                with self.assertRaises(ValueError):
                    gate.library_sites("")
                source.write_text("#[cfg(test)]\nmod fallible_command_tests {\n}\n#[cfg(test)]\nmod fallible_command_tests {\n}\n")
                with self.assertRaises(ValueError):
                    gate.test_module_bounds(source)

    def test_schema_receipts_reject_missing_duplicate_and_unowned_names(self):
        name = "pw_session_feed_0123456789abcdef"
        self.assertEqual(gate.schema_names(name + "\n", 1), {"feed":[name], "store":[]})
        for text, count in [("", 1), (name + "\n" + name + "\n", 2), ("public\n", 1),
                            ("pw_session_feed_../../x\n", 1), ("pw_session_store_1234\n", 1)]:
            with self.assertRaises(ValueError):
                gate.schema_names(text, count)

    def test_cleanup_observes_both_databases_and_never_drops_a_schema(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            receipt = output / "owned.txt"
            receipt.write_text("pw_session_feed_0123456789abcdef\npw_session_store_fedcba9876543210\n")
            commands = []
            def observed(command, log, **kwargs):
                commands.append(command)
                return {"status":0}, ""
            env = {"PW_FEED_DATABASE_URL":"feed-local-fixture", "PW_STORE_DATABASE_URL":"store-local-fixture"}
            with patch.object(gate, "process", side_effect=observed):
                result = gate.cleanup(receipt, 2, output, "control", env)
            self.assertTrue(result["valid"])
            self.assertEqual(len(commands), 2)
            self.assertEqual({c[c.index("--dbname")+1] for c in commands}, set(env.values()))
            self.assertTrue(all("SELECT nspname" in c[-1] and "DROP" not in c[-1] for c in commands))

    def test_cleanup_refuses_a_leftover_and_still_checks_partial_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            receipt = output / "owned.txt"
            name = "pw_session_feed_0123456789abcdef"
            receipt.write_text(name + "\n")
            env = {"PW_FEED_DATABASE_URL":"fixture"}
            with patch.object(gate, "process", return_value=({"status":0}, name + "\n")) as process:
                result = gate.cleanup(receipt, 1, output, "control", env)
                self.assertFalse(result["valid"])
                self.assertEqual(process.call_count, 1)
            with patch.object(gate, "process", return_value=({"status":0}, "")) as process:
                result = gate.cleanup(receipt, 2, output, "partial", env)
                self.assertFalse(result["valid"], "incomplete inventory never passes")
                self.assertTrue(result["checks"]["feed"]["valid"], "partial owned names are still independently inspected")
                self.assertEqual(process.call_count, 1)

    def test_cleanup_discards_child_stderr_before_retaining_logs(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            receipt = output / "owned.txt"
            receipt.write_text("pw_session_feed_0123456789abcdef\npw_session_store_fedcba9876543210\n")
            env = {"PW_FEED_DATABASE_URL": "postgresql://fixture:SYNTHETIC_PASSWORD%2@invalid/feed",
                   "PW_STORE_DATABASE_URL": "postgresql://fixture:SYNTHETIC_PASSWORD%2@invalid/store"}
            launch = gate.producer_process.run
            seen = []

            def diagnostic_child(command, **kwargs):
                seen.append(command)
                url = command[command.index("--dbname") + 1]
                return launch([sys.executable, "-c",
                               "import sys; sys.stderr.write(sys.argv[1]); sys.exit(2)", url], **kwargs)

            with patch.object(gate.producer_process, "run", side_effect=diagnostic_child):
                result = gate.cleanup(receipt, 2, output, "invalid-uri", env)
            self.assertFalse(result["valid"])
            self.assertEqual(len(seen), 2)
            self.assertNotIn("SYNTHETIC_PASSWORD", json.dumps(result))
            for kind in ("feed", "store"):
                self.assertEqual((output / f"invalid-uri.cleanup-{kind}.log").read_text(), "")
                self.assertEqual(result["checks"][kind]["status"], 2)
                self.assertEqual(result["checks"][kind]["stderr_policy"], "discarded at child launch")

    def test_cleanup_retains_stdout_and_native_exit_for_owned_name_queries(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            receipt = output / "owned.txt"
            name = "pw_session_feed_0123456789abcdef"
            receipt.write_text(name + "\n")
            env = {"PW_FEED_DATABASE_URL": "fixture-only"}
            launch = gate.producer_process.run
            for index, (stdout, status, valid) in enumerate([("", 0, True), (name + "\n", 0, False), ("", 2, False)]):
                def owned_query(_command, **kwargs):
                    return launch([sys.executable, "-c",
                                   "import sys; sys.stdout.write(sys.argv[1]); sys.exit(int(sys.argv[2]))",
                                   stdout, str(status)], **kwargs)
                with patch.object(gate.producer_process, "run", side_effect=owned_query):
                    result = gate.cleanup(receipt, 1, output, f"query-{index}", env)
                self.assertEqual(result["valid"], valid)
                self.assertEqual(result["checks"]["feed"]["status"], status)
                self.assertEqual((output / f"query-{index}.cleanup-feed.log").read_text(), stdout)

    def test_ordinary_native_process_still_retains_stderr_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "native.log"
            native, text = gate.process([sys.executable, "-c",
                                         "import sys; sys.stderr.write('safe native diagnostic\\n')"],
                                        log, timeout=5, env={})
            self.assertEqual(native["status"], 0)
            self.assertEqual(text, "safe native diagnostic\n")
            self.assertEqual(native["stderr_policy"], "raw merged stdout/stderr")
            self.assertEqual(log.read_text(), text)

    def test_cancellation_keeps_raw_log_and_is_not_converted_to_success(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "cancelled.log"
            with patch.object(gate.producer_process, "run", side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    gate.process(["unused"], log, timeout=1, env={})
            self.assertTrue(log.is_file())

    def test_final_restoration_failure_cannot_return_success(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.rs"
            source.write_text("anchor")
            output = base / "evidence"
            fake = [("one", "memory", "test", [(source, "anchor", "replacement")])]
            good = [{"valid":True, "native":{"failed":0}}]
            with patch.object(gate, "CONTROLS", fake), patch.object(gate, "inputs", side_effect=[{"source":"before"}, {"source":"after"}]), \
                 patch.object(gate, "git", side_effect=lambda *a: "fixed" if a == ("rev-parse", "HEAD") else ""), \
                 patch.object(gate, "inventory", return_value=[]), patch.object(gate, "run_native", return_value=good):
                self.assertEqual(gate.main(["--baseline-only", "--output", str(output)]), 1)
            import json
            report = json.loads((output / "report.json").read_text())
            self.assertFalse(report["valid"])
            self.assertFalse(report["restored"])

    def test_campaign_cancellation_restores_owned_source_and_keeps_invalid_report(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.rs"
            source.write_text("anchor")
            output = base / "evidence"
            fake = [("one", "memory", "test", [(source, "anchor", "replacement")])]
            good = [{"valid":True, "native":{"failed":0}}]
            with patch.object(gate, "ROOT", base), patch.object(gate, "CONTROLS", fake), \
                 patch.object(gate, "inputs", side_effect=lambda: {"source.rs":gate.digest(source)}), \
                 patch.object(gate, "git", side_effect=lambda *a: "fixed" if a == ("rev-parse", "HEAD") else ""), \
                 patch.object(gate, "inventory", return_value=[]), \
                 patch.object(gate, "run_native", side_effect=[good, KeyboardInterrupt]):
                with self.assertRaises(KeyboardInterrupt):
                    gate.main(["--output", str(output)])
            self.assertEqual(source.read_text(), "anchor")
            import json
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(report["abort"], "KeyboardInterrupt")
            self.assertFalse(report["valid"])
            self.assertTrue(report["restored"])

    def test_freezing_crosschecks_the_binary_list_against_source_declarations(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "test.rs"
            source.write_text("#[test]\nfn expected() {}\n")
            spec = [("memory", ["unused"], source, "prefix::")]
            with patch.object(gate, "process", return_value=({"status":0}, "prefix::other: test\n")), \
                 patch.object(gate, "drift", return_value=False):
                with self.assertRaisesRegex(ValueError, "inventories differ"):
                    gate.inventory(spec, base, {}, {}, 1)

    def test_cleanup_error_marker_invalidates_an_otherwise_green_native_run(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            source = output / "test.rs"
            source.write_text("#[test]\nfn one() {}\n")
            frozen = [{"name":"memory", "cargo":["unused"], "source":str(source), "names":["p::one"]}]
            green = "test p::one ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
            for marker in ["", "SESSION_SCHEMA_CLEANUP_ERROR owned: fixture failure\n"]:
                with patch.object(gate, "process", return_value=({"status":0}, green + marker)), \
                     patch.object(gate, "drift", return_value=False):
                    records = gate.run_native(frozen, output, "fixture", {}, {}, 1)
                self.assertEqual(records[0]["valid"], not bool(marker))
                self.assertEqual(records[0]["cleanup_error_marker"], bool(marker))

    def test_partial_or_invalid_native_controls_do_not_count_as_kills(self):
        self.assertFalse(gate.kill([]))
        self.assertFalse(gate.kill([{"valid":False,"native":{"failed":1}}]))
        self.assertFalse(gate.kill([{"valid":True,"native":{"failed":0}}]))
        self.assertFalse(gate.green([{"valid":True,"native":{"failed":1}}]))
        self.assertTrue(gate.kill([{"valid":True,"native":{"failed":1}}, {"valid":True,"native":{"failed":0}}]))


if __name__ == "__main__":
    unittest.main()
