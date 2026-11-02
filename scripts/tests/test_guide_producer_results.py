#!/usr/bin/env python3
"""Guide producer/main regressions; real Python children, no compiler/browser."""
import contextlib
import importlib.util
import io
import json
import pathlib
import re
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]


def load_guide():
    spec = importlib.util.spec_from_file_location("private_guide_mutations", ROOT / "scripts/guide_mutations.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def cargo_output(failed=False, name="intended_private_witness"):
    tests = f'test {name} ... {"FAILED" if failed else "ok"}\n'
    if failed:
        lines = (ROOT / "compiler/pw-cli/tests/guide.rs").read_text().splitlines()
        line = next(i for i, text in enumerate(lines, 1) if text.lstrip().startswith("assert!("))
        column = len(lines[line-1]) - len(lines[line-1].lstrip()) + 1
        tests += (f"\nfailures:\n\n---- {name} stdout ----\nthread '{name}' panicked at "
                  f"compiler/pw-cli/tests/guide.rs:{line}:{column}:\nassertion failed\n\nfailures:\n    {name}\n")
    return tests + f'test result: {"FAILED" if failed else "ok"}. {0 if failed else 1} passed; {1 if failed else 0} failed; 0 ignored; 0 measured; 0 filtered out\n'


def browser_receipts(env, failed=False):
    inventory = [f"private-{i}" for i in range(5)]
    titles = ["intended private witness", "second", "third", "fourth", "fifth"]
    receipt = dict(schema=1, complete=True, nonce=env["PW_ASSERTION_NONCE"], inventory=inventory,
                   tests=[[identity, "failed" if failed and i == 0 else "passed"] for i, identity in enumerate(inventory)])
    metadata = {"suites": [{"title": "guestbook.spec.mjs", "specs": [
        {"id": identity, "title": title, "tests": [{"projectName": "chromium"}]}
        for identity, title in zip(inventory, titles)]}]}
    pathlib.Path(env["PW_ASSERTION_REPORT"]).write_text(json.dumps(receipt))
    pathlib.Path(env["PLAYWRIGHT_JSON_OUTPUT_NAME"]).write_text(json.dumps(metadata))
    return {identity: "guestbook.spec.mjs › " + title for identity, title in zip(inventory, titles)}


def compiler_output(codes=("PW5048",)):
    files = sum(len(list((ROOT / directory).glob("*.pw"))) for directory in
                ("packages/pw-std", "packages/pw-platform-web", "examples/guestbook"))
    return "\n".join([*(f"error: [{code}] private named diagnostic" for code in codes),
                       f"pw check: {files} file(s), {len(codes)} error(s), 0 warning(s)"])


def python_output(failed=False, name="intended_private_witness"):
    if failed:
        return f"FAIL: {name} (private.fixture)\n\nRan 1 test in 0.001s\n\nFAILED (failures=1)\n"
    return "Ran 7 tests in 0.001s\n\nOK\n"


def browser_output(failed=False):
    if failed:
        return "  1) [chromium] › e2e/guestbook.spec.mjs:1:1 › intended private witness ───\n  1 failed\n"
    return "5 passed (1s)\n"


class GuideProducers(unittest.TestCase):
    receipts = {}

    def real_receipt(self, kind):
        if kind not in self.receipts:
            module = load_guide()
            with tempfile.TemporaryDirectory(prefix="pw-guide-real-receipt-") as folder:
                directory = pathlib.Path(folder)
                source = "import unittest\nclass Body(unittest.TestCase):\n"
                if kind != "empty":
                    source += " def test_intended_private_witness(self):\n"
                    source += "  self.fail('real body assertion')\n" if kind == "assertion" else "  pass\n"
                    for number in range(6):
                        source += f" def test_extra_{number}(self): pass\n"
                else:
                    source += " pass\n"
                (directory / "private_receipt.py").write_text(source)
                expected = sorted([f"private_receipt.Body.test_extra_{number}" for number in range(6)] + ["private_receipt.Body.test_intended_private_witness"]) if kind != "empty" else []
                produced = module.guide_python.run("private_receipt", expected, directory, directory / "evidence")
                text = (produced.directory / "result.json").read_text() if (produced.directory / "result.json").exists() else ""
                self.assertEqual(produced.result.valid, kind != "empty")
                self.receipts[kind] = (text, expected, produced.output)
        return self.receipts[kind]

    def guide_result(self, cargo=(0, None), python=(0, "green")):
        module = load_guide()
        cargo = (cargo[0], cargo[1] if cargo[1] is not None else cargo_output())
        kind = python[1]
        text, expected, output = self.real_receipt("green" if kind == "missing" else kind)
        if kind == "missing":
            text = ""
        outcome = module.guide_python.unittest_protocol.validate_result(python[0], text, expected)
        produced = module.guide_python.PythonRun(outcome, output, pathlib.Path("private-fixture-evidence"), python[0])
        with mock.patch.object(module, "CARGO_NAMES", {"intended_private_witness"}), mock.patch.object(module, "run", return_value=cargo), mock.patch.object(module, "python_tests", return_value=produced):
            return module.guide_tests()

    def browser_result(self, status, output, alter=None, frozen=True):
        module = load_guide()
        def produce(command, **kw):
            if command == module.APP_BUILD:
                return 0, "built\n"
            env = kw["env"]
            if output in (browser_output(), browser_output(True)):
                titles = browser_receipts(env, output == browser_output(True))
                if frozen:
                    module._BROWSER_TITLES = titles
                if alter:
                    alter(env, module)
            return status, output
        with mock.patch.object(module, "run", side_effect=produce) as run:
            result = module.browser_tests()
        return result, run.call_args_list, module

    def test_cargo_refuses_invalid_failure_status(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assertIsNone(self.guide_result(cargo=(status, cargo_output(True)))[0])

    def test_cargo_refuses_invalid_green_status(self):
        for status in (-9, 137, 67, 101):
            with self.subTest(status=status):
                self.assertIsNone(self.guide_result(cargo=(status, cargo_output()))[0])

    def test_cargo_refuses_empty_or_missing_summary(self):
        for text in ("", "test result: ok. 0 passed; 0 failed\n"):
            with self.subTest(text=text):
                self.assertIsNone(self.guide_result(cargo=(0, text))[0])

    def test_cargo_accepts_actual_assertion_failure(self):
        ran, failed, names, _ = self.guide_result(cargo=(101, cargo_output(True)))
        self.assertEqual((ran, failed), (8, 1))
        self.assertIn("intended_private_witness", names)

    def test_cargo_all_binary_summaries_must_agree(self):
        mixed = cargo_output() + cargo_output(True)
        ran, failed, names, _ = self.guide_result(cargo=(101, mixed))
        self.assertIsNone(ran, "one selected binary must report exactly one full inventory")
        self.assertIsNone(self.guide_result(cargo=(0, mixed))[0])

    def test_cargo_refuses_build_or_stop_diagnostics_after_partial_results(self):
        for extra in ("error[E0001]: private build error\n", "error: could not compile private\n", "signal: 9, SIGKILL\n"):
            with self.subTest(extra=extra):
                self.assertIsNone(self.guide_result(cargo=(101, cargo_output(True) + extra))[0])

    def test_cargo_quiet_failure_without_inventory_is_refused(self):
        output = "---- intended_private_witness stdout ----\n" + cargo_output(True).split("\n", 1)[1]
        ran, failed, names, _ = self.guide_result(cargo=(101, output))
        self.assertIsNone(ran, "failure prose alone does not establish executed inventory")

    def test_cargo_additional_empty_binary_is_refused(self):
        empty = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        self.assertIsNone(self.guide_result(cargo=(0, cargo_output() + empty))[0])
        self.assertIsNone(self.guide_result(cargo=(0, empty + empty))[0])

    def test_python_refuses_invalid_failure_status(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assertIsNone(self.guide_result(python=(status, "assertion"))[0])

    def test_python_refuses_invalid_green_status(self):
        for status in (-9, 137, 67, 1):
            with self.subTest(status=status):
                self.assertIsNone(self.guide_result(python=(status, "green"))[0])

    def test_python_refuses_empty_or_missing_summary(self):
        for kind in ("missing", "empty"):
            with self.subTest(kind=kind):
                self.assertIsNone(self.guide_result(python=(0, kind))[0])

    def test_python_accepts_actual_assertion_failure(self):
        ran, failed, names, _ = self.guide_result(python=(1, "assertion"))
        self.assertEqual((ran, failed), (8, 1))
        self.assertIn("test_intended_private_witness", names)

    def test_guide_accepts_green_producers(self):
        ran, failed, names, _ = self.guide_result()
        self.assertEqual((ran, failed, names), (8, 0, set()))

    def test_guide_retains_earlier_producer_output(self):
        out = self.guide_result(cargo=(101, "earlier Cargo detail\n" + cargo_output(True)))[-1]
        self.assertIn("earlier Cargo detail", out)
        self.assertIn("Ran 7 tests", out)

    def test_browser_refuses_invalid_failure_status(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assertFalse(self.browser_result(status, browser_output(True))[0][0])

    def test_browser_refuses_invalid_green_status(self):
        for status in (-9, 137, 67, 1):
            with self.subTest(status=status):
                self.assertFalse(self.browser_result(status, browser_output())[0][0])

    def test_browser_refuses_empty_or_missing_summary(self):
        for text in ("", "0 passed\n"):
            with self.subTest(text=text):
                self.assertFalse(self.browser_result(0, text)[0][0])

    def test_browser_accepts_actual_assertion_failure(self):
        result, _, _ = self.browser_result(1, browser_output(True))
        self.assertEqual(result[:3], (True, 5, 1))
        self.assertIn("guestbook.spec.mjs › intended private witness", result[3])

    def test_browser_accepts_green_producer(self):
        result, _, _ = self.browser_result(0, browser_output())
        self.assertEqual(result[:3], (True, 5, 0))

    def test_browser_uses_canonical_authority_and_exact_selection(self):
        result, calls, module = self.browser_result(0, browser_output())
        self.assertTrue(result[0])
        expected = ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--", *module.SPEC]
        self.assertEqual(calls[1].args[0], expected)
        self.assertEqual(calls[1].kwargs["cwd"], ROOT / "spikes/own-renderer")
        env = calls[1].kwargs["env"]
        self.assertEqual(env["PLAYWRIGHT_JSON_OUTPUT_FILE"], env["PLAYWRIGHT_JSON_OUTPUT_NAME"])

    def test_cargo_requires_exact_inventory_and_assertion_site(self):
        for output in (cargo_output().replace("intended_private_witness", "foreign"),
                       cargo_output(True).replace("panicked at compiler/", "panicked at foreign/"),
                       re.sub(r":(\d+):\nassertion failed", ":999:\noperand panic", cargo_output(True)),
                       cargo_output().replace("0 ignored", "1 ignored")):
            status = 101 if "FAILED" in output else 0
            self.assertIsNone(self.guide_result(cargo=(status, output))[0])

    def test_original_rust_inventory_is_frozen_exactly(self):
        module = load_guide()
        source = module.TEST.read_text()
        actual = set(re.findall(r"#\[test\]\s*fn (\w+)\(", source))
        self.assertEqual(len(actual), 15)
        self.assertEqual(module.CARGO_NAMES, actual)

    def test_browser_refuses_partial_stale_and_duplicate_receipts(self):
        def alter(env, module, kind):
            path = pathlib.Path(env["PW_ASSERTION_REPORT"])
            report = json.loads(path.read_text())
            if kind == "partial": report["tests"].pop()
            if kind == "stale": report["nonce"] = "older invocation"
            if kind == "duplicate": report["inventory"][1] = report["inventory"][0]
            if kind == "runtime": report["complete"] = False
            path.write_text(json.dumps(report))
        for kind in ("partial", "stale", "duplicate", "runtime"):
            self.assertFalse(self.browser_result(1, browser_output(True), lambda e, m: alter(e, m, kind))[0][0])

    def test_browser_refuses_missing_and_disagreeing_metadata(self):
        def alter(env, module, kind):
            path = pathlib.Path(env["PLAYWRIGHT_JSON_OUTPUT_NAME"])
            if kind == "missing":
                path.unlink()
                return
            metadata = json.loads(path.read_text())
            specs = metadata["suites"][0]["specs"]
            if kind == "partial": specs.pop()
            if kind == "extra": specs.append({**specs[0], "id": "foreign"})
            if kind == "duplicate": specs.append(dict(specs[0]))
            if kind == "foreign": specs[1]["id"] = "foreign"
            if kind == "title": specs[0]["title"] = "different intended witness"
            if kind == "project": specs[0]["tests"][0]["projectName"] = "firefox"
            if kind == "invalid": metadata = None
            path.write_text(json.dumps(metadata))
        for kind in ("missing", "partial", "extra", "duplicate", "foreign", "title", "project", "invalid"):
            with self.subTest(kind=kind):
                self.assertFalse(self.browser_result(1, browser_output(True), lambda e, m: alter(e, m, kind))[0][0])
                if kind != "title":
                    self.assertFalse(self.browser_result(0, browser_output(), lambda e, m: alter(e, m, kind), frozen=False)[0][0])

    def test_browser_freezes_mapping_only_from_green_complete_baseline(self):
        self.assertFalse(self.browser_result(1, browser_output(True), frozen=False)[0][0])
        result, calls, module = self.browser_result(0, browser_output(), frozen=False)
        self.assertTrue(result[0])
        self.assertEqual(len(module._BROWSER_TITLES), 5)
        self.assertIn("--retries=0", calls[1].args[0])
        self.assertIn("--reporter=line,json,./assertion-result-reporter.mjs", calls[1].args[0])

    def test_browser_json_and_prose_do_not_authorize_outcomes(self):
        def alter(env, module):
            path = pathlib.Path(env["PLAYWRIGHT_JSON_OUTPUT_NAME"])
            metadata = json.loads(path.read_text())
            metadata["stats"] = {"unexpected": 999}
            for spec in metadata["suites"][0]["specs"]:
                spec["ok"] = False
                spec["tests"][0]["status"] = "unexpected"
            path.write_text(json.dumps(metadata))
        self.assertEqual(self.browser_result(0, browser_output(), alter)[0][:3], (True, 5, 0))
        self.assertFalse(self.browser_result(1, "999 failed with intended private witness")[0][0])

    def test_checked_refusal_requires_complete_error_inventory(self):
        module = load_guide()
        native = compiler_output()
        self.assertEqual(module.compiler_codes(1, native), {"PW5048"})
        for status, output in ((True, native), (1, native + "\nerror: unrelated runtime failure"),
                               (1, native.replace("1 error(s)", "2 error(s)")),
                               (1, native.replace("0 warning(s)", "1 warning(s)")),
                               (1, native.replace("error: [PW5048]", "error: untyped")),
                               (1, native + "\npw check: 1 file(s), 1 error(s), 0 warning(s)"),
                               (1, "error[E0001]: build failed\n" + native),
                               (1, "internal compiler error: compiler defect\n" + native),
                               (1, "bash: line 2: unexpected failure\n" + native),
                               (1, "unrecognized output\n" + native),
                               (1, "  unexpected indented failure\n" + native),
                               (1, "   | stray gutter before diagnostic\n" + native)):
            self.assertEqual(module.compiler_codes(status, output), set())

    def test_checked_plain_diagnostic_continuations_are_supported(self):
        module = load_guide()
        lines = compiler_output().splitlines()
        diagnostic = "\n".join([lines[0], "  --> pages.pw:30:13", "   |",
            "10 | | source snippet", "...  |", "   | |__^ diagnostic", "   = note: explanation",
            "   = help: repair", lines[-1]])
        self.assertEqual(module.compiler_codes(1, diagnostic), {"PW5048"})
        # A diagnostic showing a line past 99 widens its gutter by a space.
        wide = "\n".join([lines[0], "   --> book.pw:99:19", "    |",
            " 99 | | source snippet", "100 | | more", "    | |__^ diagnostic",
            "    = note: explanation", "    = help: repair", lines[-1]])
        self.assertEqual(module.compiler_codes(1, wide), {"PW5048"})

    def test_main_cancellation_restores_source_and_preserves_exit(self):
        module = load_guide()
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "source"
            path.write_text("original")
            watch = types.SimpleNamespace(stopped=[])
            baseline = types.SimpleNamespace(mutation_bound=types.SimpleNamespace(start=lambda: watch))
            with mock.patch.dict(sys.modules, {"mutation_baseline": baseline}), \
                 mock.patch.object(module, "MUTANTS", [("own", "guide", path, "original", "mutated")]), \
                 mock.patch.object(module, "MUST", {"own": {"test_own"}}), \
                 mock.patch.object(module, "CHECKED", {}), \
                 mock.patch.object(module, "freeze_python", return_value=["test_own"]), \
                 mock.patch.object(module, "guide_tests", side_effect=[module.GuideResult(1, 0, set(), ""), SystemExit(37)]):
                with self.assertRaises(SystemExit) as caught:
                    module.main()
            self.assertEqual(caught.exception.code, 37)
            self.assertEqual(path.read_text(), "original")

    def run_main(self, phase, status=0, mode="invalid", codes=("PW5048",), named=False):
        module = load_guide()
        held = "browser" if phase in ("browser", "build") else "guide"
        witness = "intended private witness" if held == "browser" else ("test_intended_private_witness" if phase == "python" else "intended_private_witness")
        what = "private intended mutation"
        watch = types.SimpleNamespace(stopped=[])
        baseline = types.SimpleNamespace(explain=mock.Mock(), mutation_bound=types.SimpleNamespace(start=lambda: watch))
        output = io.StringIO()
        with tempfile.TemporaryDirectory(prefix="pw-guide-producer-") as directory:
            source = pathlib.Path(directory) / "owned-source"
            original = b"one original anchor\n"
            source.write_bytes(original)
            build_calls = 0
            cargo_calls = 0
            def produce(command, **kw):
                nonlocal build_calls, cargo_calls
                mutated = source.read_bytes() != original
                if command == module.CARGO_TEST:
                    cargo_calls += 1
                    if mode == "bad_baseline" and cargo_calls == 1:
                        return 101, cargo_output()
                    if mutated and phase == "cargo":
                        if mode == "stopped":
                            watch.stopped.append(("private-owned-child", 1))
                        return (0, cargo_output()) if mode == "green" else (status, cargo_output(True, witness))
                    return 0, cargo_output()
                if command == module.PYTHON_TEST:
                    if mutated and phase == "python":
                        return (0, python_output()) if mode == "green" else (status, python_output(True, witness))
                    return 0, python_output()
                if command == module.APP_BUILD:
                    build_calls += 1
                    if mode == "cleanup_failure" and build_calls == 3:
                        return 67, "private cleanup build failure\n"
                    if mutated and phase == "build":
                        return status, compiler_output(codes)
                    return 0, "built\n"
                if command[-len(module.SPEC):] == module.SPEC:
                    browser_receipts(kw["env"], mutated and phase == "browser" and mode != "green")
                    if mutated and phase == "browser":
                        return (0, browser_output()) if mode == "green" else (status, browser_output(True))
                    return 0, browser_output()
                raise AssertionError(f"unexpected command: {command}")
            expected_python = sorted([f"private.Body.test_extra_{number}" for number in range(6)] + ["private.Body.test_intended_private_witness"])
            def produce_python():
                protocol = module.guide_python.unittest_protocol
                if source.read_bytes() != original and phase == "python" and mode != "green":
                    if status == 1:
                        failed_id = "private.Body.test_intended_private_witness"
                        result = protocol.Result(True, "mocked validated body assertion", tuple(identity for identity in expected_python if identity != failed_id), (failed_id,), (), 7)
                    else:
                        result = protocol.Result(False, "mocked invalid native child status")
                else:
                    result = protocol.Result(True, "mocked validated green bodies", tuple(expected_python), (), (), 7)
                return module.guide_python.PythonRun(result, "mocked structured Python domain outcome", pathlib.Path(directory), status)
            with contextlib.ExitStack() as stack:
                stack.enter_context(mock.patch.object(module, "CARGO_NAMES", {"intended_private_witness"}))
                stack.enter_context(mock.patch.object(module, "MUTANTS", [(what, held, source, "original", "mutated")]))
                stack.enter_context(mock.patch.object(module, "MUST", {what: set() if named else {witness}}))
                stack.enter_context(mock.patch.object(module, "CHECKED", {what: {"PW5048"}} if named else {}))
                stack.enter_context(mock.patch.object(module, "run", side_effect=produce))
                stack.enter_context(mock.patch.object(module, "freeze_python", return_value=expected_python))
                stack.enter_context(mock.patch.object(module, "python_tests", side_effect=produce_python))
                stack.enter_context(mock.patch.dict(sys.modules, {"mutation_baseline": baseline}))
                stack.enter_context(contextlib.redirect_stdout(output))
                code = module.main()
            self.assertEqual(source.read_bytes(), original, "actual main finally must restore exact fixture bytes")
        return code, output.getvalue()

    def assert_refused_without_kill(self, result):
        code, output = result
        self.assertNotEqual(code, 0)
        self.assertIn("REFUSED", output)
        self.assertNotIn("KILLED", output)
        self.assertNotIn("1 of 1 mutants killed", output)

    def test_main_refuses_cargo_invalid_status_without_kill(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assert_refused_without_kill(self.run_main("cargo", status))

    def test_main_refuses_python_invalid_status_without_kill(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assert_refused_without_kill(self.run_main("python", status))

    def test_main_refuses_browser_invalid_status_without_kill(self):
        for status in (-9, 137, 67, 0):
            with self.subTest(status=status):
                self.assert_refused_without_kill(self.run_main("browser", status))

    def test_main_refuses_named_codes_after_invalid_build_status(self):
        for status in (-9, 137, 67):
            with self.subTest(status=status):
                self.assert_refused_without_kill(self.run_main("build", status, named=True))

    def test_main_refuses_additional_named_codes(self):
        self.assert_refused_without_kill(self.run_main("build", 1, codes=("PW5048", "PW6601"), named=True))

    def test_main_refuses_unlisted_compiler_refusal(self):
        self.assert_refused_without_kill(self.run_main("build", 1))

    def test_main_accepts_exact_named_compiler_refusal(self):
        code, output = self.run_main("build", 1, named=True)
        self.assertEqual(code, 0)
        self.assertIn("KILLED by the checker (PW5048)", output)

    def test_main_accepts_valid_intended_assertion_neighbours(self):
        for phase, status in (("cargo", 101), ("python", 1), ("browser", 1)):
            with self.subTest(phase=phase):
                code, output = self.run_main(phase, status)
                self.assertEqual(code, 0)
                self.assertIn("KILLED", output)
                self.assertIn("1 of 1 mutants killed", output)

    def test_main_green_neighbours_are_survivors(self):
        for phase in ("cargo", "python", "browser"):
            with self.subTest(phase=phase):
                code, output = self.run_main(phase, mode="green")
                self.assertNotEqual(code, 0)
                self.assertIn("SURVIVED", output)
                self.assertNotIn("KILLED", output)

    def test_main_refuses_bound_stop_with_valid_partial_summary(self):
        self.assert_refused_without_kill(self.run_main("cargo", 101, mode="stopped"))

    def test_main_refuses_invalid_green_baseline(self):
        code, output = self.run_main("cargo", 101, mode="bad_baseline")
        self.assertNotEqual(code, 0)
        self.assertNotIn("KILLED", output)

    def test_final_restored_build_failure_fails_campaign(self):
        code, output = self.run_main("browser", 1, mode="cleanup_failure")
        self.assertNotEqual(code, 0)
        self.assertIn("does not build again from the restored sources", output)


class GuideNativeProtocol(unittest.TestCase):
    """Actual native child/fixture provenance through the real guide main."""

    def run_native_main(self, mode):
        module = load_guide()
        watch = types.SimpleNamespace(stopped=[])
        baseline = types.SimpleNamespace(explain=mock.Mock(), mutation_bound=types.SimpleNamespace(start=lambda: watch))
        with tempfile.TemporaryDirectory(prefix="pw-guide-native-main-") as folder:
            directory = pathlib.Path(folder)
            source = directory / "private_native.py"
            marker = directory / "entered"
            original = '''import pathlib, unittest
MODE = "green"
MARKER = pathlib.Path(__file__).with_name("entered")
class Native(unittest.TestCase):
 def setUp(self):
  if MODE == "setup_error": raise OSError("actual setUp error")
  if MODE == "setup_assertion": raise AssertionError("actual setUp assertion")
 def tearDown(self):
  if MODE == "teardown_error": raise OSError("actual tearDown error")
  if MODE == "teardown_assertion": raise AssertionError("actual tearDown assertion")
 if MODE == "async":
  async def test_required(self):
   MARKER.write_text("entered")
   self.fail("deferred body must not earn a kill")
 elif MODE == "generator":
  def test_required(self):
   MARKER.write_text("entered")
   yield "deferred"
 elif MODE == "expected_failure":
  @unittest.expectedFailure
  def test_required(self):
   MARKER.write_text("entered")
   self.fail("expected failure")
 else:
  def test_required(self):
   MARKER.write_text("entered")
   if MODE == "assertion": self.fail("actual body assertion")
   if MODE == "body_error": raise OSError("actual body error")
   if MODE == "skip": self.skipTest("actual skip")
   if MODE == "returned_coroutine":
    async def later(): pass
    return later()
'''.encode()
            source.write_bytes(original)
            produced = []
            real_run = module.guide_python.run
            mutated_marker_cleared = False
            def cargo(command, **kwargs):
                nonlocal mutated_marker_cleared
                self.assertEqual(command, module.CARGO_TEST)
                if source.read_bytes() != original and not mutated_marker_cleared:
                    marker.unlink(missing_ok=True)
                    mutated_marker_cleared = True
                return 0, cargo_output()
            def python_run(*args, **kwargs):
                result = real_run(*args, **kwargs)
                produced.append(result)
                return result
            output = io.StringIO()
            what = "native intended mutation"
            with contextlib.ExitStack() as stack:
                for name, value in {
                    "CARGO_NAMES": {"intended_private_witness"},
                    "MUTANTS": [(what, "guide", source, 'MODE = "green"', f'MODE = "{mode}"')],
                    "MUST": {what: {"test_required"}}, "CHECKED": {},
                    "PYTHON_SELECTOR": "private_native", "PYTHON_SEARCH_DIR": directory,
                    "PYTHON_PATTERN": "private_native.py", "PYTHON_EXPECTED_COUNT": 1,
                }.items():
                    stack.enter_context(mock.patch.object(module, name, value))
                stack.enter_context(mock.patch.object(module, "run", side_effect=cargo))
                stack.enter_context(mock.patch.object(module.guide_python, "run", side_effect=python_run))
                stack.enter_context(mock.patch.dict(sys.modules, {"mutation_baseline": baseline}))
                stack.enter_context(contextlib.redirect_stdout(output))
                code = module.main()
            self.assertEqual(source.read_bytes(), original, "real guide main must restore exact private source bytes")
            self.assertEqual(len(produced), 2, "baseline and mutant each launch a real canonical child")
            self.assertTrue(produced[0].result.valid)
            self.assertEqual(produced[0].result.executed, 1)
            return code, output.getvalue(), produced[-1], marker.exists()

    def test_native_setup_failures_earn_no_kill_and_never_enter_body(self):
        for mode in ("setup_error", "setup_assertion"):
            with self.subTest(mode=mode):
                code, output, produced, entered = self.run_native_main(mode)
                self.assertNotEqual(code, 0)
                self.assertIn("REFUSED", output)
                self.assertNotIn("KILLED", output)
                self.assertFalse(produced.result.valid)
                self.assertEqual(produced.result.executed, 0)
                self.assertFalse(entered)

    def test_native_teardown_failures_earn_no_kill_after_body_entry(self):
        for mode in ("teardown_error", "teardown_assertion"):
            with self.subTest(mode=mode):
                code, output, produced, entered = self.run_native_main(mode)
                self.assertNotEqual(code, 0)
                self.assertIn("REFUSED", output)
                self.assertNotIn("KILLED", output)
                self.assertFalse(produced.result.valid)
                self.assertEqual(produced.result.executed, 0)
                self.assertTrue(entered)

    def test_native_body_assertion_is_a_named_assertion_kill(self):
        code, output, produced, entered = self.run_native_main("assertion")
        self.assertEqual(code, 0)
        self.assertIn("KILLED by test_required", output)
        self.assertIn("Python body assertion", output)
        self.assertNotIn("Python body error", output)
        self.assertEqual(produced.result.assertion_failed, ("private_native.Native.test_required",))
        self.assertEqual(produced.result.body_errors, ())
        self.assertTrue(entered)

    def test_native_body_error_earns_no_assertion_kill(self):
        code, output, produced, entered = self.run_native_main("body_error")
        self.assertNotEqual(code, 0)
        self.assertIn("REFUSED", output)
        self.assertNotIn("KILLED", output)
        self.assertEqual(produced.result.body_errors, ("private_native.Native.test_required",))
        self.assertEqual(produced.result.assertion_failed, ())
        self.assertTrue(entered)

    def test_native_skip_expected_and_deferred_bodies_earn_no_kill(self):
        for mode in ("skip", "expected_failure", "async", "generator", "returned_coroutine"):
            with self.subTest(mode=mode):
                code, output, produced, _ = self.run_native_main(mode)
                self.assertNotEqual(code, 0)
                self.assertIn("REFUSED", output)
                self.assertNotIn("KILLED", output)
                self.assertFalse(produced.result.valid)
                self.assertEqual(produced.result.executed, 0)

    def test_native_guide_discovery_preserves_exact_original_seven_ids(self):
        module = load_guide()
        expected = [
            "test_guide_diagnostics.Current.test_every_registered_code_has_a_row",
            "test_guide_diagnostics.Current.test_the_committed_page_is_what_the_registry_writes",
            "test_guide_diagnostics.Reading.test_a_line_that_is_no_entry_is_refused_not_skipped",
            "test_guide_diagnostics.Reading.test_a_pipe_in_a_cell_is_escaped",
            "test_guide_diagnostics.Reading.test_a_retired_code_with_its_reason_continued_across_lines",
            "test_guide_diagnostics.Reading.test_an_owner_without_a_title_is_written_from_its_name",
            "test_guide_diagnostics.Reading.test_each_entry_with_its_invariant_and_the_adrs_its_comment_cites",
        ]
        actual = module.freeze_python()
        self.assertEqual(actual, expected)
        produced = module.guide_python.run(module.PYTHON_SELECTOR, actual, module.PYTHON_SEARCH_DIR)
        self.assertTrue(produced.result.valid, produced.output)
        self.assertEqual(produced.result.passed, tuple(expected))
        self.assertEqual(produced.result.executed, 7)

    def test_native_timeout_refuses_and_reaps_its_owned_child(self):
        module = load_guide()
        with tempfile.TemporaryDirectory(prefix="pw-guide-timeout-") as folder:
            directory = pathlib.Path(folder)
            (directory / "private_timeout.py").write_text("import time, unittest\nclass Native(unittest.TestCase):\n def test_required(self): time.sleep(30)\n")
            produced = module.guide_python.run("private_timeout", ["private_timeout.Native.test_required"], directory, directory / "evidence", timeout=0.3)
            self.assertFalse(produced.result.valid)
            self.assertEqual(produced.result.executed, 0)
            self.assertIsNone(produced.status)
            metadata = json.loads((produced.directory / "process.json").read_text())
            self.assertTrue(metadata["timed_out"])
            self.assertTrue(metadata["child_reaped"])
            self.assertTrue(metadata["deployment_env_absent"])

    def test_native_fresh_cache_does_not_read_same_timestamp_prior_mutant(self):
        import os
        import py_compile
        module = load_guide()
        with tempfile.TemporaryDirectory(prefix="pw-guide-bytecode-") as folder:
            directory = pathlib.Path(folder)
            source = directory / "private_cached.py"
            old = 'import unittest\nclass Native(unittest.TestCase):\n def test_required(self): self.assertEqual("old", "old")\n'
            new = old.replace('("old", "old")', '("new", "old")')
            self.assertEqual(len(old), len(new))
            source.write_text(old)
            stamp = source.stat()
            cached = pathlib.Path(py_compile.compile(str(source), doraise=True))
            cached_bytes = cached.read_bytes()
            source.write_text(new)
            os.utime(source, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
            produced = module.guide_python.run("private_cached", ["private_cached.Native.test_required"], directory, directory / "evidence")
            self.assertTrue(produced.result.valid, produced.output)
            self.assertEqual(produced.result.assertion_failed, ("private_cached.Native.test_required",))
            self.assertEqual(cached.read_bytes(), cached_bytes, "existing cache must remain untouched")

if __name__ == "__main__":
    unittest.main()