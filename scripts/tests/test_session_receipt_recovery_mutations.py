"""Adversarial calibration of receipt recovery's strict native acceptance."""
from importlib import import_module
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
gate = import_module("session_receipt_recovery_mutations")


class ReceiptRecoveryProducer(unittest.TestCase):
    def test_native_inventory_is_exact_and_refuses_ignored_and_foreign_entries(self):
        expected = ("scope::one", "scope::two")
        self.assertEqual(gate.native_list("scope::one: test\nscope::two: test\n", ("scope::",)), expected)
        for raw in ["", "scope::one: test\nscope::one: test\n", "foreign::one: test\n",
                    "scope::one: benchmark\n", "scope::one: test (ignored)\n",
                    "scope::one: test\nscope::bench: benchmark\n",
                    "scope::one: test\nscope::other: unknown-kind\n"]:
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
                self.assertFalse(gate.evaluate(101, raw + 'SESSION_SCHEMA_CLEANUP_ERROR fixture\n', frozen, {}, sites)["valid"])
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
            source.write_text('#[test]\nfn one() {}\n#[test]\nfn neighbor() {}\n')
            spec = {"name":"unit","cargo":[],"source":source,"prefix":"p::","selected":("one",),"schema_count":0}
            for native, expected in [("p::one: test\n", True), ("p::other: test\n", False)]:
                with patch.object(gate, "ROOT", root), patch.object(gate, "SPECS", [spec]), \
                     patch.object(gate, "process", return_value=({"status":0,"reaped":True,"error":None}, native)), \
                     patch.object(gate, "drift", return_value=False):
                    if expected:
                        self.assertEqual(gate.inventory(root, {}, {}, 1)[0]["names"], ("p::one",))
                    else:
                        with self.assertRaisesRegex(ValueError, "inventories differ"):
                            gate.inventory(root, {}, {}, 1)

    def test_native_run_refuses_a_green_result_without_producer_reaping(self):
        good = "test p::one ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            frozen = {"name":"unit", "cargo":[], "schema_count":0, "names":["p::one"],"sources":[]}
            with patch.object(gate, "process", return_value=({"status":0,"reaped":False,"error":None}, good)), \
                 patch.object(gate, "drift", return_value=False):
                self.assertFalse(gate.run_native([frozen], output, "test", {}, {}, 1)[0]["valid"])

    def close_error_process(self, root, label, command, callback):
        """Real child, real reaping, then deterministic evidence-close failure."""
        original = Path.open
        target = root / f"{label}.unit.log"
        class FailClose:
            def __init__(self, stream):
                self.stream = stream
            def __enter__(self):
                return self.stream.__enter__()
            def __exit__(self, *args):
                self.stream.__exit__(*args)
                raise OSError("controlled log-close failure after owned child reaping")
        def opened(path, *args, **kwargs):
            stream = original(path, *args, **kwargs)
            return FailClose(stream) if path.name in (target.name, "inventory.unit.log") and args and args[0] == "w" else stream
        with patch.object(Path, "open", opened), patch.object(gate, "ROOT", root), \
             patch.object(gate, "SPECS", [{"name":"unit", "cargo":command, "source":root / "tests.rs", "prefix":"p::", "selected":None, "schema_count":0}]), patch.object(gate, "drift", return_value=False):
            return callback()

    def test_reaped_assertion_exit_followed_by_log_io_error_is_not_a_kill(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tests.rs"
            source.write_text('#[test]\nfn one() {\n    assert!(false, "privacy refusal");\n}\n')
            frozen = {"name":"unit", "cargo":[], "schema_count":0, "names":["p::one"], "sources":["tests.rs"]}
            raw = self.failed("p::one", "tests.rs", 3, 5)
            command = [sys.executable, "-c", f'import sys; sys.stdout.write({raw!r}); sys.exit(101)']
            record = self.close_error_process(root, "after-reap", command,
                lambda: gate.run_native([dict(frozen, cargo=command)], root, "after-reap", {}, {}, 5,
                    {"p::one":(str(source), 3, 5)}))[0]
            self.assertEqual(record["status"], 101)
            self.assertTrue(record["reaped"])
            self.assertEqual(record["error"], "OSError")
            self.assertTrue(record["native"]["valid"], "the synthetic native text itself is complete")
            self.assertFalse(record["valid"], "evidence I/O failure invalidates an otherwise attributed assertion exit")
            self.assertFalse(gate.killed([record]))

    def test_reaped_inventory_exit_followed_by_log_io_error_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tests.rs"
            source.write_text('#[test]\nfn one() {}\n')
            command = [sys.executable, "-c", 'print("p::one: test")']
            with patch.object(gate, "SOURCES", [(source, "p::")]):
                with self.assertRaises(ValueError):
                    self.close_error_process(root, "inventory", command,
                        lambda: gate.inventory(root, {}, {}, 5))
            receipt = json.loads((root / "inventory.unit.log.process.json").read_text())
            self.assertEqual(receipt["error"], "OSError")
            self.assertEqual(receipt["reaped"]["status"], 0)
            self.assertEqual((root / "inventory.unit.log").read_text(), "p::one: test\n")

    def test_actual_sigterm_before_launch_during_restore_and_final_report_is_invalid(self):
        script = r'''
import json, os, signal, sys
from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, sys.argv[1])
import session_receipt_recovery_mutations as gate
root, phase = Path(sys.argv[2]), sys.argv[3]
source = root / "source.rs"
source.write_text("anchor\n")
good = {"valid":True,"native":{"failed":0}}
kill = {"valid":True,"native":{"failed":1}}
original_changed, original_write = gate.mutation_source.changed, Path.write_text
report_signalled = False
@contextmanager
def changed(path, original, replacement):
    with original_changed(path, original, replacement):
        if phase == "before-launch":
            os.kill(os.getpid(), signal.SIGTERM)
        yield
def write(path, text, *args, **kwargs):
    global report_signalled
    if phase in ("before-launch", "restore") and path == source and text == "anchor\n" and path.read_text() == "replacement\n":
        # A first TERM during normal restore, or another TERM while already
        # unwinding cancellation, must both leave restoration uninterrupted.
        os.kill(os.getpid(), signal.SIGTERM)
        os.kill(os.getpid(), signal.SIGTERM)
    if phase == "finalize" and path == root / "evidence/report.json" and not report_signalled:
        report_signalled = True
        os.kill(os.getpid(), signal.SIGTERM)
    return original_write(path, text, *args, **kwargs)
def run(*args):
    if args[2] == "baseline":
        return [good]
    if args[2] == "one" and phase in ("restore", "finalize"):
        return [kill]
    if args[2] == "final-baseline" and phase == "finalize":
        return [good]
    (root / "unexpected-native-launch").write_text(args[2])
    raise AssertionError("no native launch is allowed after the controlled cancellation boundary")
control = [("one", "controlled source edit", [(source,"anchor","replacement")], [])]
with patch.object(gate,"ROOT",root), patch.object(gate,"CONTROLS",control), \
     patch.object(gate,"SOURCES",[]), patch.object(gate,"SPECS",[]), patch.object(gate,"witness_sites",return_value={}), \
     patch.object(gate,"inputs",side_effect=lambda:{"source.rs":gate.digest(source)}), \
     patch.object(gate,"git",side_effect=lambda *a:"fixed" if a == ("rev-parse","HEAD") else ""), \
     patch.object(gate,"inventory",return_value=[{"names":["p::one"]}]), \
     patch.object(gate,"run_native",side_effect=run), \
     patch.dict(gate.os.environ,{"PW_FEED_DATABASE_URL":"fake-feed","PW_STORE_DATABASE_URL":"fake-store"}), \
     patch.object(gate.mutation_source,"changed",changed), patch.object(Path,"write_text",write):
    raise SystemExit(gate.main(["--output",str(root / "evidence")]))
'''
        for phase in ("before-launch", "restore", "finalize"):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                child = subprocess.run([sys.executable, "-c", script, str(ROOT / "scripts"), str(root), phase],
                                       text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=10)
                self.assertEqual(child.returncode, 143, child.stdout)
                self.assertEqual((root / "source.rs").read_text(), "anchor\n", child.stdout)
                self.assertFalse((root / "unexpected-native-launch").exists(), child.stdout)
                report = json.loads((root / "evidence/report.json").read_text())
                self.assertEqual(report["abort"], "SystemExit")
                self.assertFalse(report["valid"])
                self.assertTrue(report["restored"])

    def test_main_restores_the_previous_term_handler_on_success_and_preflight_failure(self):
        import signal
        previous = signal.getsignal(signal.SIGTERM)
        with patch.object(sys, "stdout", io.StringIO()):
            self.assertEqual(gate.main(["--list"]), 0)
        self.assertIs(signal.getsignal(signal.SIGTERM), previous)
        with patch.object(gate, "SPECS", [{"source":Path("missing-fixture.rs"), "prefix":"p::", "selected":None}]), \
             patch.object(gate, "CONTROLS", [("one", "calibration", [], [])]), \
             patch.object(sys, "stdout", io.StringIO()):
            self.assertEqual(gate.main(["--check"]), 1)
        self.assertIs(signal.getsignal(signal.SIGTERM), previous)

    def fake_campaign(self, directory, native):
        root = Path(directory)
        source = root / "source.rs"
        source.write_text("anchor\n")
        output = root / "evidence"
        control = [("one", "test control", [(source, "anchor", "replacement")], [])]
        patches = [patch.object(gate, "ROOT", root), patch.object(gate, "CONTROLS", control),
                   patch.object(gate, "SOURCES", []), patch.object(gate, "SPECS", []), patch.object(gate, "witness_sites", return_value={}),
                   patch.object(gate, "inputs", side_effect=lambda: {"source.rs":gate.digest(source)}),
                   patch.object(gate, "git", side_effect=lambda *a: "fixed" if a == ("rev-parse", "HEAD") else ""),
                   patch.object(gate, "inventory", return_value=[{"names":["p::one"]}]),
                   patch.object(gate, "run_native", side_effect=native),
                   patch.dict(gate.os.environ, {"PW_FEED_DATABASE_URL":"fake-feed", "PW_STORE_DATABASE_URL":"fake-store"})]
        return source, output, patches

    def test_final_green_baseline_is_required_after_every_restored_kill(self):
        from contextlib import ExitStack
        good = {"valid":True,"native":{"failed":0}}
        killed = {"valid":True,"native":{"failed":1}}
        for final, expected in [(good, 0), ({"valid":False,"native":{"failed":0}}, 1), (killed, 1)]:
            with tempfile.TemporaryDirectory() as directory:
                source, output, patches = self.fake_campaign(directory, [[good], [killed], [final]])
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
            source, output, patches = self.fake_campaign(directory, [[good], KeyboardInterrupt])
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

    def test_baseexception_after_final_green_is_recorded_as_invalid(self):
        from contextlib import ExitStack
        good = {"valid":True,"native":{"failed":0}}
        killed = {"valid":True,"native":{"failed":1}}
        with tempfile.TemporaryDirectory() as directory:
            source, output, patches = self.fake_campaign(directory, [[good], [killed], [good]])
            heads = 0
            def head(*arguments):
                nonlocal heads
                if arguments == ("rev-parse", "HEAD"):
                    heads += 1
                    if heads == 2:
                        raise KeyboardInterrupt
                    return "fixed"
                return ""
            with ExitStack() as stack:
                for p in patches:
                    stack.enter_context(p)
                stack.enter_context(patch.object(gate, "git", side_effect=head))
                with self.assertRaises(KeyboardInterrupt):
                    gate.main(["--output", str(output)])
            report = json.loads((output / "report.json").read_text())
            self.assertFalse(report["valid"])
            self.assertEqual(report["abort"], "KeyboardInterrupt")
            self.assertTrue(report["restored"])
            self.assertEqual(source.read_text(), "anchor\n")

    def test_survivor_or_invalid_native_run_is_never_credited(self):
        self.assertFalse(gate.green([{"valid":False,"native":{"failed":0}}]))
        self.assertFalse(gate.killed([{"valid":False,"native":{"failed":1}}]))
        self.assertFalse(gate.killed([{"valid":True,"native":{"failed":0}}]))
        self.assertTrue(gate.killed([{"valid":True,"native":{"failed":1}}]))


    def test_selected_materializer_inventory_is_still_source_declared(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "tests.rs"
            source.write_text('#[test]\nfn one() {}\n#[test]\nfn neighbor() {}\n')
            self.assertEqual(gate.declarations(source, "p::", ("one",)), ("p::one",))
            for selected in [(), ("missing",), ("one", "one")]:
                with self.assertRaises(ValueError):
                    gate.declarations(source, "p::", selected)

    def test_receipt_schema_inventory_refuses_missing_duplicate_and_injected_names(self):
        name = "pw_session_receipt_0123456789abcdef"
        self.assertEqual(gate.schema_names(name + "\n", 1), [name])
        for raw in ["", name + "\n" + name + "\n", "public\n", name + "'; DROP SCHEMA public;--\n"]:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                gate.schema_names(raw, 1)

    def test_cleanup_proves_both_configured_sources_readonly_and_redacts_urls(self):
        name = "pw_session_receipt_0123456789abcdef"
        env = {"PW_FEED_DATABASE_URL":"private-feed-uri", "PW_STORE_DATABASE_URL":"private-store-uri"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / "schemas.txt"
            receipt.write_text(name + "\n")
            calls = []
            def queried(command, log, timeout, local, display_command=None, suppress_stderr=False):
                self.assertTrue(suppress_stderr)
                calls.append((command, display_command))
                self.assertEqual(local, env)
                self.assertIn("SELECT nspname FROM pg_catalog.pg_namespace", command[-1])
                self.assertNotIn("DROP", command[-1])
                self.assertIn(name, command[-1])
                return {"status":0,"error":None,"reaped":True}, ""
            with patch.object(gate, "process", side_effect=queried):
                result = gate.cleanup(receipt, 1, root, "actual", env)
            self.assertTrue(result["valid"])
            self.assertEqual(len(calls), 2)
            self.assertEqual({call[0][call[0].index("--dbname") + 1] for call in calls}, set(env.values()))
            for command, public in calls:
                self.assertNotIn(command[command.index("--dbname") + 1], public)
                self.assertTrue(public[public.index("--dbname") + 1].startswith("<configured-"))

    def test_cleanup_refuses_leftovers_io_missing_url_and_incomplete_receipts(self):
        name = "pw_session_receipt_0123456789abcdef"
        both = {"PW_FEED_DATABASE_URL":"feed", "PW_STORE_DATABASE_URL":"store"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / "schemas.txt"
            cases = [({"status":0,"error":None,"reaped":True}, name + "\n", both, name + "\n"),
                     ({"status":0,"error":"OSError","reaped":True}, "", both, name + "\n"),
                     ({"status":0,"error":None,"reaped":False}, "", both, name + "\n"),
                     ({"status":1,"error":None,"reaped":True}, "", both, name + "\n"),
                     ({"status":0,"error":None,"reaped":True}, "", {"PW_FEED_DATABASE_URL":"feed"}, name + "\n"),
                     ({"status":0,"error":None,"reaped":True}, "", both, "")]
            for record, raw, env, names in cases:
                receipt.write_text(names)
                with patch.object(gate, "process", return_value=(record, raw)):
                    self.assertFalse(gate.cleanup(receipt, 1, root, "invalid", env)["valid"])

    def test_native_cancellation_still_checks_the_owned_receipt_after_reaping(self):
        name = "pw_session_receipt_0123456789abcdef"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frozen = [{"name":"server", "cargo":[], "schema_count":1, "names":[gate.PG], "sources":[]}]
            def cancelled(command, log, timeout, env):
                Path(env["PW_SESSION_SCHEMA_RECEIPT"]).write_text(name + "\n")
                raise KeyboardInterrupt
            with patch.object(gate, "process", side_effect=cancelled), \
                 patch.object(gate, "cleanup", return_value={"valid":True}) as cleanup:
                with self.assertRaises(KeyboardInterrupt):
                    gate.run_native(frozen, root, "cancel", {}, {}, 1)
            cleanup.assert_called_once()
            self.assertEqual(cleanup.call_args.args[0].read_text(), name + "\n")

    def test_a_kill_requires_every_native_binary_complete_and_valid(self):
        good = {"valid":True,"native":{"failed":0}}
        domain = {"valid":True,"native":{"failed":1}}
        invalid = {"valid":False,"native":{"failed":0}}
        self.assertFalse(gate.green([]))
        self.assertFalse(gate.killed([]))
        self.assertTrue(gate.killed([good, domain]))
        self.assertFalse(gate.killed([invalid, domain]))
        self.assertFalse(gate.green([good, domain]))
        self.assertFalse(gate.killed([good, good]))


    def test_private_cleanup_diagnostics_are_discarded_before_any_artifact_write(self):
        marker = "PRIVATE_FAKE_URI_PASSWORD_TOKEN"
        raw_uri = "postgresql://audit:" + marker + "%XX@127.0.0.1/audit"
        child = [sys.executable, "-c", f'import sys; sys.stderr.write({raw_uri!r}); sys.exit(2)']
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record, text = gate.process(child, root / "cleanup.log", 5, {},
                                        display_command=["psql", "<configured-database>"], suppress_stderr=True)
            self.assertEqual(record["status"], 2)
            self.assertTrue(record["reaped"])
            self.assertEqual(text, "")
            receipt = json.loads((root / record["process_receipt"]).read_text())
            self.assertEqual(receipt["stderr_policy"], "discarded at child launch")
            (root / "results.json").write_text(json.dumps(record))
            for path in root.iterdir():
                self.assertNotIn(marker.encode(), path.read_bytes(), path.name)
                self.assertNotIn(raw_uri.encode(), path.read_bytes(), path.name)


if __name__ == "__main__":
    unittest.main()
