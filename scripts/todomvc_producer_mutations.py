#!/usr/bin/env python3
"""Controls for TodoMVC's shared native-result and source-ownership gates."""
from pathlib import Path
import signal
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import guide_python
import mutation_source

PRODUCER = ROOT / "scripts/todomvc_mutations.py"
SELECTOR = "test_todomvc_producers"
SEARCH = ROOT / "scripts/tests"
EXPECTED = 15
MUTANTS = [
    ("invalid native Cargo results earn credit", "test_todomvc_producers.Producers.test_server_refuses_timeout_build_error_and_native_disagreement", PRODUCER,
     'return (True, result.passed, result.failed) if result.valid else (False, 0, 0)',
     'return (True, result.passed, result.failed)'),
    ("foreign Cargo test identities are accepted", "test_todomvc_producers.Producers.test_server_requires_exact_four_ids_and_assertion_site", PRODUCER,
     'sources=[SERVER_SOURCE], expected=SERVER_NAMES)', 'sources=[SERVER_SOURCE], expected=None)'),
    ("a failed build launches the browser", "test_todomvc_producers.Producers.test_build_failure_or_timeout_cannot_launch_browser", PRODUCER,
     '        if code != 0:\n', '        if False:\n'),
    ("browser inventory changes after baseline", "test_todomvc_producers.Producers.test_browser_requires_browser_count_complete_frozen_ids", PRODUCER,
     'nonce, INVENTORIES.get("browser"))', 'nonce, None)'),
    ("short baseline browser inventory is accepted", "test_todomvc_producers.Producers.test_browser_requires_browser_count_complete_frozen_ids", PRODUCER,
     'not result.valid or result.passed + result.failed != BROWSER_COUNT:', 'not result.valid:'),
    ("invalid browser provenance earns credit", "test_todomvc_producers.Producers.test_browser_requires_fresh_receipt_and_native_exit", PRODUCER,
     'if not result.valid or result.passed + result.failed != BROWSER_COUNT:',
     'if result.passed + result.failed not in (0, BROWSER_COUNT):'),
    ("checker exit can be an infrastructure failure", "test_todomvc_producers.Producers.test_checker_accepts_only_complete_native_diagnostics", PRODUCER,
     'if type(code) is not int or code not in (0, 1) or not isinstance(output, str):',
     'if not isinstance(output, str):'),
    ("checker summary need not account for diagnostics", "test_todomvc_producers.Producers.test_checker_accepts_only_complete_native_diagnostics", PRODUCER,
     'int(summary[2]) != len(errors)', 'False'),
    ("a subset of named checker codes is enough", "test_todomvc_producers.Producers.test_checker_kill_requires_exact_named_codes", PRODUCER,
     'if named and codes == named:', 'if named and codes <= named:'),
    ("an invalid producer earns a kill", "test_todomvc_producers.Producers.test_invalid_or_stopped_producer_never_earns_kill", PRODUCER,
     'if not valid or not passed + failed or watch.stopped:',
     'if not passed + failed or watch.stopped:'),
    ("a stopped producer prints a kill before refusing", "test_todomvc_producers.Producers.test_invalid_or_stopped_producer_never_earns_kill", PRODUCER,
     'if not valid or not passed + failed or watch.stopped:',
     'if not valid or not passed + failed:'),
    ("an invalid result reaches another control", "test_todomvc_producers.Producers.test_invalid_result_stops_before_the_next_control", PRODUCER,
     '        if refused or watch.stopped:\n            break\n',
     '        if watch.stopped:\n            break\n'),
    ("failed restored source is ignored", "test_todomvc_producers.Producers.test_restored_source_result_is_required", PRODUCER,
     '        if not valid or failed or not passed or watch.stopped:\n            print(f"REFUSED: restored',
     '        if False:\n            print(f"REFUSED: restored'),
    ("cancellation restores the planted source", "test_todomvc_producers.Producers.test_cancellation_restores_owned_source", PRODUCER,
     'with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):',
     'with mutation_source.changed(path, original.replace(anchor, replacement, 1), original.replace(anchor, replacement, 1)):'),
    ("timeout claims a native test failure status", "test_todomvc_producers.Producers.test_timeout_uses_shared_owner_and_remains_invalid", PRODUCER,
     'return expired.output, None', 'return expired.output, 101'),
    ("producer cancellation bypasses source restoration", "test_todomvc_producers.Producers.test_outer_sigterm_restores_real_owned_source", ROOT / "scripts/todomvc_producer_mutations.py",
     '\n    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))\n', '\n    None  # no cancellation unwind\n'),
    ("producer restored suite is not required", "test_todomvc_producers.Producers.test_outer_requires_restored_full_suite", ROOT / "scripts/todomvc_producer_mutations.py",
     '\n    if not restored.result.valid or restored.result.failed or restored.result.body_errors or watch.stopped:\n',
     '\n    if False:\n'),
]


def main():
    import mutation_baseline
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
    evidence = Path(tempfile.mkdtemp(prefix="pw-todomvc-producers-"))
    expected = guide_python.freeze(SELECTOR, SEARCH, SELECTOR + ".py", EXPECTED, evidence)
    baseline = guide_python.run(SELECTOR, expected, SEARCH, evidence, timeout=60)
    if not baseline.result.valid or baseline.result.failed or watch.stopped:
        print(baseline.output, flush=True)
        return 1
    killed = refused = survived = 0
    for what, intended, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique)", flush=True)
            refused += 1
            break
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            try:
                compile(path.read_text(), str(path), "exec")
            except SyntaxError:
                print(f"{what}: REFUSED (invalid producer syntax)", flush=True)
                refused += 1
                break
            run = guide_python.run(SELECTOR, expected, SEARCH, evidence, timeout=60)
        result = run.result
        if not result.valid or result.body_errors or watch.stopped:
            verdict = "REFUSED (invalid or stopped producer; no kill)"
            refused += 1
        elif intended in result.assertion_failed:
            verdict = f"KILLED by {intended} ({len(result.assertion_failed)} of {len(expected)} tests fail)"
            killed += 1
        else:
            verdict = "SURVIVED"
            survived += 1
        print(f"{what}: {verdict}", flush=True)
        if verdict.startswith(("REFUSED", "SURVIVED")):
            print(run.output, flush=True)
        if watch.stopped:
            return 1
    restored = guide_python.run(SELECTOR, expected, SEARCH, evidence, timeout=60)
    if not restored.result.valid or restored.result.failed or restored.result.body_errors or watch.stopped:
        print("REFUSED: restored producer suite is not green", flush=True)
        print(restored.output, flush=True)
        refused += 1
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survived} survived", flush=True)
    return int(bool(refused or survived or killed != len(MUTANTS)))


if __name__ == '__main__':
    sys.exit(main())
