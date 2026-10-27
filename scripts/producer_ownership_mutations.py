#!/usr/bin/env python3
"""The shared process owner is proved by named, native assertion witnesses."""
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
OWNER = ROOT / 'scripts/producer_process.py'
MUTANTS = [
    ('launch cancellation is delivered before ownership', 'test_launch_cancellation_reaps_before_all_four_restore',
     '                signal.signal(number, defer)\n', '                pass\n'),
    ('cancellation omits owned cleanup', 'test_running_cancellation_reaps_before_all_four_restore',
     '            if child is not None and not complete:\n', '            if False:\n'),
    ('cleanup does not finish native reaping', 'test_launch_cancellation_reaps_before_all_four_restore',
     '                out, err = child.communicate()\n', '                out, err = None, None\n'),
    ('cleanup cancellation escapes before reaping', 'test_repeated_cancellation_during_cleanup_cannot_skip_restoration',
     '                out, err = child.communicate()\n',
     '                signal.signal(signal.SIGTERM, previous[signal.SIGTERM])\n                out, err = child.communicate()\n'),
    ('default cancellation kills the parent without restoring source', 'test_default_cancellation_raises_through_source_restoration',
     '            raise SystemExit(128 + number)\n', '            signal.raise_signal(number)\n'),
    ('ignored cancellation is captured', 'test_ignored_parent_cancellation_stays_ignored',
     '            if previous[number] != signal.SIG_IGN:\n', '            if True:\n'),
    ('native failed status is changed to success', 'test_native_output_and_nonzero_status_are_unchanged',
     '    return subprocess.CompletedProcess(cmd, child.returncode, out, err)\n',
     '    return subprocess.CompletedProcess(cmd, 0, out, err)\n'),
    ('timeout is reported as native completion', 'test_timeout_reaps_and_preserves_native_partial_output',
     '                timed_out = True\n', '                timed_out = False\n'),
    ('original handlers are not restored', 'test_launch_error_restores_original_handlers',
     '                signal.signal(number, handler)\n', '                pass\n'),
]
# Every row exposes the actual changed authority for the planner/anchor checker.
MUTANTS = [(name, 'test_producer_ownership.ProducerOwnership.' + witness, OWNER, anchor, replacement)
           for name, witness, anchor, replacement in MUTANTS]


def main():
    import guide_python
    import mutation_baseline
    watch = mutation_baseline.mutation_bound.start()
    selector = 'test_producer_ownership'
    search = ROOT / 'scripts/tests'
    evidence = Path(tempfile.mkdtemp(prefix='pw-producer-ownership-'))
    expected = guide_python.freeze(selector, search, 'test_producer_ownership.py', 11, evidence)
    baseline = guide_python.run(selector, expected, search, evidence, timeout=60)
    print(baseline.output)
    if not baseline.result.valid or baseline.result.failed or watch.stopped:
        print('REFUSED: incomplete or failed baseline')
        return 1
    print(f'baseline: {len(expected)} passed, 0 failed')
    killed = 0
    for name, witness, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f'REFUSED: {name}: anchor is not unique')
            return 1
        if witness not in expected:
            print(f'REFUSED: {name}: witness not in frozen inventory')
            return 1
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            receipt = guide_python.run(witness, [witness], search, evidence, timeout=30)
        finally:
            path.write_text(original)
        print(receipt.output)
        result = receipt.result
        if not result.valid or result.body_errors or watch.stopped:
            print(f'REFUSED: {name}: invalid, erroneous or stopped producer; no kill')
            return 1
        if witness in result.assertion_failed:
            killed += 1
            print(f'{name}: KILLED by {witness} ({len(result.assertion_failed)} assertion methods fail)')
        else:
            print(f'{name}: SURVIVED')
    print(f'{killed} of {len(MUTANTS)} mutants killed')
    return int(killed != len(MUTANTS))


if __name__ == '__main__':
    sys.exit(main())
