#!/usr/bin/env python3
"""Controls for changed-test reach (ADR-0372-ci-test-reach).

Only complete, inventory-matched unittest assertion failures earn kills;
imports, body errors, timeouts and failed cleanup fail the campaign.
"""
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile

import mutation_source
import unittest_producer
import unittest_protocol

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLAN = ROOT / "scripts/ci_plan.py"
COUNTS = ROOT / "scripts/evidence_counts.py"
MUTANTS = [
    ("hash inputs are counted as executed campaigns", COUNTS,
     "        run = executed_scripts(body)", "        run = sorted(set(SCRIPT.findall(body)))"),
    ("a long witness identifier replaces the mutant description", COUNTS,
     "    i = max(descriptions or range(len(said)), key=lambda k: len(said[k]))",
     "    i = max(range(len(said)), key=lambda k: len(said[k]))"),
    ("root tests reach no mutation script", PLAN,
     "        if names_package_tests(text, packages) or names_python_tests(text, changed):",
     "        if names_python_tests(text, changed):"),
    ("Python tests reach no mutation script", PLAN,
     "        if names_package_tests(text, packages) or names_python_tests(text, changed):",
     "        if names_package_tests(text, packages):"),
    ("root tests reach no direct recipe", PLAN,
     '        or names_package_tests(body.get(name, ""), packages)\n', ""),
    ("Python tests reach no direct recipe", PLAN,
     '        or names_python_tests(body.get(name, ""), changed)\n', ""),
    ("library root tests are ignored", PLAN,
     'if pathlib.Path(path).name in {"main.rs", "lib.rs"} and test_modules({path: lines})',
     'if pathlib.Path(path).name in {"main.rs"} and test_modules({path: lines})'),
    # An out-of-line test module (the nightly of 2026-11-15's finding).
    ("an out-of-line test module is no test module", PLAN,
     "            if declared.search((ROOT / declarer).read_text()):",
     "            if False:"),
    ("an out-of-line test module's own name is no filter", PLAN,
     "        for name in (module_name(p), module_name(declarer)):",
     "        for name in (module_name(declarer),):"),
    ("an out-of-line test module's declaring module is no filter", PLAN,
     "        for name in (module_name(p), module_name(declarer)):",
     "        for name in (module_name(p),):"),
    ("`tests` declared out of line is every crate's filter", PLAN,
     '            if name is not None and name != "tests":',
     "            if name is not None:"),
    ("a root's out-of-line test module reaches no package", PLAN,
     '        if pathlib.Path(declarer).name in {"main.rs", "lib.rs"}\n',
     "        if False\n"),
    ("bare Python module selectors are ignored", PLAN,
     '        if directory_named and re.search(rf"(?<![\\w.]){re.escape(name[:-3])}(?!\\w)", text):',
     '        if False:'),
    ("reading TodoMVC metadata starts its mutation runtime", ROOT / "scripts/todomvc_compat_mutations.py",
     "import guide_python\n", "import guide_python\nimport mutation_baseline\n"),
    ("workspace consumers omit changed root tests", PLAN,
     '    if re.search(r"(?<![\\w-])--workspace(?![\\w-])", text):',
     '    if False:'),
]
SELECTORS = [
    "scripts.tests.test_evidence_counts.Counts.test_hashed_or_quoted_script_names_are_not_executed_campaigns",
    "scripts.tests.test_evidence_counts.Counts.test_a_method_id_does_not_replace_the_mutant_description",
    "scripts.tests.test_ci_scripts.Plan.test_root_test_changes_reach_the_package_test_consumers",
    "scripts.tests.test_ci_scripts.Plan.test_an_out_of_line_test_module_reaches_what_runs_it",
    "scripts.tests.test_ci_scripts.Plan.test_python_test_changes_reach_scripts_and_direct_recipes",
    "scripts.tests.test_ci_scripts.Plan.test_bare_python_module_inventories_reach_their_campaigns",
    "scripts.tests.test_ci_scripts.Plan.test_a_todomvc_plan_writes_only_the_workflow_output",
]


def run(expected):
    directory = tempfile.TemporaryDirectory(prefix="pw-ci-test-reach-")
    with mutation_source.releasing(directory.cleanup):
        scratch = pathlib.Path(directory.name)
        frozen, report = scratch / "expected.json", scratch / "report.json"
        frozen.write_text(json.dumps(expected))
        child = subprocess.Popen(
            [sys.executable, "-B", "scripts/unittest_producer.py", "--expected", str(frozen),
             "--report", str(report), *SELECTORS], cwd=ROOT, start_new_session=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            out, _ = child.communicate(timeout=120)
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate()
            raise
        try:
            result = unittest_protocol.validate_result(child.returncode, report.read_text(), expected)
        except OSError:
            print(out, flush=True)
            return None
        if not result.valid or result.body_errors:
            print(out, flush=True)
            print(f"INVALID RESULT: {result.reason}; body errors: {result.body_errors}", flush=True)
            return None
        return result


def main():
    import mutation_baseline  # disable stale bytecode before changing source
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    expected = unittest_producer.load(SELECTORS)[2]
    baseline = run(expected)
    if baseline is None or baseline.failed or not baseline.passed:
        print("FAIL: baseline must execute and pass every selected method", flush=True)
        return 1
    print(f"baseline: {len(baseline.passed)} passed, 0 failed", flush=True)
    killed = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE", flush=True)
            return 1
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            result = run(expected)
        if result is None:
            print(f"{what}: INVALID RESULT (not a kill)", flush=True)
            return 1
        if result.assertion_failed:
            killed += 1
            print(f"{what}: KILLED ({len(result.assertion_failed)} assertion failures)", flush=True)
        else:
            print(f"{what}: SURVIVED", flush=True)
    print(f"{killed} of {len(MUTANTS)} mutants killed", flush=True)
    return int(killed != len(MUTANTS))


if __name__ == "__main__":
    sys.exit(main())
