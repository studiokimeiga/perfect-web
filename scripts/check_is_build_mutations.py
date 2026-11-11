#!/usr/bin/env python3
"""Whole controls for the shared executable-program validation authority.

Every baseline command must build, execute tests and succeed. Source restores
on signals and failures. A bound stop fails the campaign; it never kills a
mutant by timeout. No filter is implemented: this runs every listed control.
"""
import os
import pathlib
import json
import signal
import subprocess
import sys
import tempfile

import cargo_test_results
import mutation_source
import unittest_producer
import unittest_protocol

ROOT = pathlib.Path(__file__).resolve().parent.parent
CLI = ROOT / "compiler/pw-cli/src/main.rs"
PROGRAM = ROOT / "compiler/pw-core/src/program.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
WIT = ROOT / "compiler/pw-core/src/wit.rs"
COMPONENT = ROOT / "compiler/pw-core/src/backend/component.rs"
JUST = ROOT / "justfile"

MUTANTS = [
    ("complete contracts discard their prepared privacy facts", ROOT / "compiler/pw-core/src/contract.rs", "checked.privacy()", "&crate::check::privacy_facts(&[], cx.sigs, inference)"),
    ('CLI check uses only partial-program semantics', CLI, 'pw_core::check::check_program_sources_with(&all, released.as_ref())', 'pw_core::check::check_sources_with(&all, released.as_ref())'),
    ('CLI build discards a complete validation failure', CLI, '    let build = match pw_core::program::validate_sources_with(&sources, released).into_build() {\n        Ok(b) => b,\n        Err(e) => {\n            eprintln!("pw: {e}");\n            return Err(ExitCode::FAILURE);\n        }\n    };\n', '    let build = match pw_core::program::validate_sources_with(&sources, released).into_build() {\n        Ok(b) => b,\n        Err(e) => {\n            eprintln!("pw: {e}");\n            return Ok(());\n        }\n    };\n'),
    ('support refusals disappear from check diagnostics', PROGRAM, '    for refusal in refusals {', '    for refusal in refusals.into_iter().take(0) {'),
    ('depended placeholders are accepted', BUILD, '                .filter(|target| placeholders.contains(target))', '                .filter(|_| false)'),
    ('page-plan refusals disappear', BUILD, '        for p in &self.pages {', '        for p in self.pages.iter().take(0) {'),
    ('blocked renderer parts are accepted', BUILD, '        for blocked in t.blocked() {', '        for blocked in t.blocked().into_iter().take(0) {'),
    ('WIT collision ownership points at the wrong file', PROGRAM, 'crate::wit::WitError::Collision { second, .. } => second.clone(),', 'crate::wit::WitError::Collision { .. } => String::new(),'),
    ('a proof applies to other HIR of equal length', LOWER, '                    .all(|(unit, hir)| std::ptr::eq(&unit.hir, *hir)),', '                    .all(|_| true),'),
    ('support diagnostics have no responsible source span', PROGRAM, 'primary_span: span', 'primary_span: 0..0'),
    ('placeholder stage metadata names a different stage', PROGRAM, 'Self::Placeholder => "backend_placeholder",', 'Self::Placeholder => "backend_component",'),
    ('a partial inspection plan is written', BUILD, '        if !refused.is_empty() {', '        if false && !refused.is_empty() {'),
    ('early WIT failure erases independent page refusal', BUILD, 'early.extend(build.support_refusals());', ''),
    ('an unmappable export loses source ownership', WIT, 'LocatedError::at(error, declaration)', 'LocatedError::from(error)'),
    ('public report data can mint a validated plan', PROGRAM, '(true, Ok(build)) => Plan::Validated(Executable { build }),', '(_, Ok(build)) => Plan::Validated(Executable { build }),'),
    ('a caller can mint a proof from a foreign context', LOWER, "impl<'a> Checked<'a> {", "impl<'a> Checked<'a> {\n    pub fn of(units: &'a [crate::check::Unit], cx: Context<'a>) -> Self {\n        let mut inference = crate::effects::Inference::new(cx.sigs, cx.ws); inference.run(cx.hirs); let privacy = std::rc::Rc::new(crate::check::privacy_facts(cx.hirs, cx.sigs, &inference)); Self { cx, units, pages: std::cell::OnceCell::new(), privacy }\n    }"),
    ('syntax refusal omits clean sibling diagnostics', PROGRAM, 'if parsed.iter().any(|file| !file.ok()) {', 'if false && parsed.iter().any(|file| !file.ok()) {'),
    ('a page failure hides its signal failure', BUILD, '(Err(why), Err(_)) => early.push(', '(Err(why), Err(_)) => Vec::<crate::program::Refusal>::new().push('),
    ('the complete accepted-corpus gate is not run', JUST, '    cargo test --locked -p pw-cli --test check_build_corpus accepted_corpus_has_exactly_its_known_backend_refusals', '    true'),
    ('the semantic accepted-corpus gate is not run', JUST, '    cargo test --locked -p pw-core --test checking_source no_accepted_corpus_file_reports_a_body_level_error', '    true'),
    ('a parsed WIT failure loses its generated source identity', COMPONENT, '.find(|w| w.exports == name || w.name == name)', '.find(|_| false)'),
    ('build_id restored browser failure is accepted', ROOT / "scripts/build_id_mutations.py", '    if not built or failed or not passed or (watch is not None and watch.stopped):\n        print("FAIL: the restored browser suite is not green")', '    if False:\n        print("FAIL: the restored browser suite is not green")'),
    ('build_id bypasses shared browser suite authority', ROOT / "scripts/build_id_mutations.py", '["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",', '["bash", "-c", "exit 0", "--",'),
    ('build_id producer failure is hidden by passed counts', ROOT / "scripts/build_id_mutations.py", 'if code != (1 if command_failed else 0):', 'if False and code != (1 if command_failed else 0):'),
    ('refusal restored browser failure is accepted', ROOT / "scripts/refusal_mutations.py", '        if not built or failed or not passed or (watch is not None and watch.stopped):\n            print("FAIL: the restored browser suite is not green")', '        if False:\n            print("FAIL: the restored browser suite is not green")'),
    ('refusal bypasses shared browser suite authority', ROOT / "scripts/refusal_mutations.py", '["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",', '["bash", "-c", "exit 0", "--",'),
    ('refusal producer failure is hidden by passed counts', ROOT / "scripts/refusal_mutations.py", 'if code != (1 if command_failed else 0):', 'if False and code != (1 if command_failed else 0):'),
    ('refusal batched test inventory drifts', ROOT / "scripts/refusal_mutations.py", 'if actual is None or Counter(expected) != Counter(actual):', 'if actual is None:'),
    ('refusal accepts a failed test inventory producer', ROOT / "scripts/refusal_mutations.py", '    if code != 0:\n        return None\n', '    if False and code != 0:\n        return None\n'),
    ('build_id invalid producer earns an assertion kill', ROOT / "scripts/build_id_mutations.py", 'not built or passed + failed == 0 or stopped', 'False'),
    ('build_id cargo producer status contradicts its counts', ROOT / "scripts/build_id_mutations.py", 'valid = code == (101 if failed else 0) and passed + failed > 0', 'valid = True'),
    ('build_id memory-stopped mutant earns a kill', ROOT / "scripts/build_id_mutations.py", 'stopped = watch is not None and bool(watch.stopped)', 'stopped = False'),
    ('refusal invalid producer earns an assertion kill', ROOT / "scripts/refusal_mutations.py", 'not built or passed + failed == 0 or stopped', 'False'),
    ('refusal cargo producer status contradicts its counts', ROOT / "scripts/refusal_mutations.py", 'valid = code == (101 if failed else 0) and passed + failed > 0', 'valid = True'),
    ('refusal memory-stopped mutant earns a kill', ROOT / "scripts/refusal_mutations.py", 'stopped = watch is not None and bool(watch.stopped)', 'stopped = False'),
    ('refusal named checker exception accepts extra codes', ROOT / "scripts/refusal_mutations.py", 'result.codes == named', 'named <= result.codes'),
    ('CIB Cargo invalid result is accepted', ROOT / "scripts/check_is_build_mutations.py", '\n            valid, p, f = result.valid and not stopped, result.passed, result.failed', '\n            valid, p, f = True, result.passed, result.failed'),
    ('held-label Cargo invalid result is accepted', ROOT / "scripts/held_labels_mutations.py", '    valid = not stopped and result.valid', '    valid = not stopped'),
    ('CIB Python ordinary body error earns a kill', ROOT / "scripts/check_is_build_mutations.py", '\n    if not result.valid or result.body_errors:', '\n    if not result.valid:'),
    ('CIB Python foreign inventory is accepted', ROOT / "scripts/check_is_build_mutations.py", '\n    result = unittest_protocol.validate_result(status, text, expected)', '\n    result = unittest_protocol.validate_result(status, text, expected)\n    if result.reason == "expected inventory mismatch":\n        return True, 1, 0'),
    ('CIB invalid producer reaches the next control', ROOT / "scripts/check_is_build_mutations.py", '\n        if not built or stopped:', '\n        if False:'),
    ('held-label invalid producer reaches the next control', ROOT / "scripts/held_labels_mutations.py", '        if not built:', '        if False:'),
    ('secondary source restore failure replaces the primary', ROOT / "scripts/mutation_source.py", '        if primary is None or (not cancellation(primary) and not isinstance(secondary, Exception)):\n            raise', '        raise'),
    ('CIB child receives an empty expected method inventory', ROOT / "scripts/check_is_build_mutations.py", '\n        frozen.write_text(json.dumps(expected))', '\n        frozen.write_text(json.dumps([]))'),
    ('consumer tests execute cached source instead of the mutant', ROOT / "scripts/tests/test_check_build_gate.py", '        exec(compile(path.read_bytes(), str(path), "exec"), module.__dict__)', '        spec.loader.exec_module(module)'),
    ('cleanup tests execute cached source instead of the mutant', ROOT / "scripts/tests/test_mutation_cleanup.py", '    exec(compile(path.read_bytes(), str(path), "exec"), module.__dict__)', '    spec.loader.exec_module(module)'),
    ('held-label known memory stop is ignored', ROOT / "scripts/held_labels_mutations.py", '    return built and not (watch is not None and watch.stopped), passed, failed', '    return built, passed, failed'),
    ('first source-release cancellation is discarded', ROOT / "scripts/mutation_source.py", '        if primary is None or (not cancellation(primary) and not isinstance(secondary, Exception)):', '        if primary is None:'),
    ('first source-diagnostic cancellation is discarded', ROOT / "scripts/mutation_source.py", '            if not cancellation(primary) and not isinstance(diagnostic, Exception):', '            if False:'),
    ('report directory context exit masks the original cancellation', ROOT / "scripts/check_is_build_mutations.py", '\n    with mutation_source.releasing(directory.cleanup):', '\n    with directory:'),
    # Train-12: track `device-store` (ADR-0366, decision 7) made a part computed from two signals build, so the
    # witness that a part cannot compute from several values is a signal and a query's value together, and
    # `a_part_may_compute_from_two_signals` holds the lift.
    ('a part computed from two signals is refused again', ROOT / "compiler/pw-core/src/page_values.rs", '    if browsers && (inputs.len() > 1 || nested) && !path.contains(".#") {\n', '    if browsers && false && !path.contains(".#") {\n'),
 ]
COMMANDS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--doc", "backend::lower::Checked"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--lib", "backend::lower::checked_input_tests"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "check_is_build"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-cli", "--test", "check_is_build"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-cli", "--test", "check_build_corpus", "accepted_corpus"],
    ["python3", "scripts/tests/test_check_build_gate.py"],
    ["python3", "scripts/tests/test_mutation_cleanup.py"],
    ["python3", "scripts/tests/test_run_suite.py"],
]
BOUND = 180

def python_selector(command):
    return command[1].removesuffix(".py").replace("/", ".")


def freeze_python():
    # Freeze the actual selectors' IDs on pristine source, before any mutant.
    return {tuple(command): unittest_producer.load([python_selector(command)])[2]
            for command in COMMANDS if command[0] == "python3"}


def bounded(command):
    child = subprocess.Popen(command, cwd=ROOT, start_new_session=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    stopped = False
    try:
        out, _ = child.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        out, _ = child.communicate()
        stopped = True
    return out, child.returncode, stopped


def python_result(status, text, expected):
    result = unittest_protocol.validate_result(status, text, expected)
    if not result.valid or result.body_errors:
        return False, 0, 0
    return True, len(result.passed), len(result.assertion_failed)


def python_tests(command, expected):
    directory = tempfile.TemporaryDirectory(prefix="pw-cib-result-")
    with mutation_source.releasing(directory.cleanup):
        scratch = pathlib.Path(directory.name)
        frozen, report = scratch / "expected.json", scratch / "report.json"
        frozen.write_text(json.dumps(expected))
        argv = [sys.executable, str(ROOT / "scripts/unittest_producer.py"),
                "--expected", str(frozen), "--report", str(report), python_selector(command)]
        out, status, stopped = bounded(argv)
        try:
            text = report.read_text()
        except OSError as error:
            return False, 0, 0, stopped, out + f"\nmissing producer report: {error}"
        valid, passed, failed = python_result(status, text, expected)
        return valid and not stopped, passed, failed, stopped, out


def run_tests(inventories=None):
    inventories = freeze_python() if inventories is None else inventories
    passed = failed = 0
    outputs = []
    for command in COMMANDS:
        if command[0] == "python3":
            valid, p, f, stopped, out = python_tests(command, inventories[tuple(command)])
        else:
            out, status, stopped = bounded(command)
            result = cargo_test_results.parse(status, out)
            valid, p, f = result.valid and not stopped, result.passed, result.failed
        outputs.append(out)
        if not valid:
            return False, passed, failed, stopped, outputs
        passed += p
        failed += f
    return True, passed, failed, False, outputs

def main():
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    inventories = freeze_python()
    built, passed, failed, stopped, outputs = run_tests(inventories)
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or not passed or failed or stopped:
        print("FAIL: baseline did not build, execute tests and succeed")
        print("\n".join(outputs))
        return 1
    survivors = stops = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}", flush=True)
            survivors += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed, stopped, outputs = run_tests(inventories)
        if not built or stopped:
            print(f"{what}: INVALID RESULT (campaign fails; not a kill)", flush=True)
            print("\n".join(outputs))
            return 1
        if failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{len(MUTANTS) - survivors - stops} of {len(MUTANTS)} mutants killed; {stops} bound stops", flush=True)
    return 1 if survivors or stops else 0

if __name__ == "__main__":
    sys.exit(main())
