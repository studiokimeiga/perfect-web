"""Actual producer verdicts retain their canonical mutation-count owner."""
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from importlib import import_module
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import evidence_counts
import producer_ownership_mutations

guide_python = import_module('guide_python')

# The nine verdict lines from verify39526960864 at f15117de. Their native
# receipts were valid; eight were misread because bare method identifiers
# were longer than their descriptions. Keep this output independent of the
# current tuple representation and do not reinterpret the historical run.
VERDICTS = """launch cancellation is delivered before ownership: KILLED by test_producer_ownership.ProducerOwnership.test_launch_cancellation_reaps_before_all_four_restore (1 assertion methods fail)
cancellation omits owned cleanup: KILLED by test_producer_ownership.ProducerOwnership.test_running_cancellation_reaps_before_all_four_restore (1 assertion methods fail)
cleanup does not finish native reaping: KILLED by test_producer_ownership.ProducerOwnership.test_launch_cancellation_reaps_before_all_four_restore (1 assertion methods fail)
cleanup cancellation escapes before reaping: KILLED by test_producer_ownership.ProducerOwnership.test_repeated_cancellation_during_cleanup_cannot_skip_restoration (1 assertion methods fail)
default cancellation kills the parent without restoring source: KILLED by test_producer_ownership.ProducerOwnership.test_default_cancellation_raises_through_source_restoration (1 assertion methods fail)
ignored cancellation is captured: KILLED by test_producer_ownership.ProducerOwnership.test_ignored_parent_cancellation_stays_ignored (1 assertion methods fail)
native failed status is changed to success: KILLED by test_producer_ownership.ProducerOwnership.test_native_output_and_nonzero_status_are_unchanged (1 assertion methods fail)
timeout is reported as native completion: KILLED by test_producer_ownership.ProducerOwnership.test_timeout_reaps_and_preserves_native_partial_output (1 assertion methods fail)
original handlers are not restored: KILLED by test_producer_ownership.ProducerOwnership.test_launch_error_restores_original_handlers (1 assertion methods fail)
9 of 9 mutants killed
"""


class Metadata(unittest.TestCase):
    def test_actual_native_verdicts_have_exact_owner_and_catalog(self):
        blocks = evidence_counts.blocks(VERDICTS)
        self.assertEqual(len(blocks), 1)
        _, killed, total, verdicts = blocks[0]
        self.assertEqual((killed, total, len(verdicts)), (9, 9, 9))
        catalog = producer_ownership_mutations.MUTANTS
        self.assertEqual(
            evidence_counts.owner({'producer_ownership_mutations.py': catalog}, verdicts, total),
            'producer_ownership_mutations.py',
        )
        self.assertEqual(evidence_counts.disagree(catalog, killed, total, verdicts), [])

    def test_production_freeze_includes_the_pid_publication_witness(self):
        search = ROOT / 'scripts/tests'
        # Exercise main's real native/canonical inventory comparison, then stop
        # before any witness or mutation. Only the baseline result is synthetic.
        refused = SimpleNamespace(output='', result=SimpleNamespace(valid=False, failed=0))
        owner = SimpleNamespace(mutation_bound=SimpleNamespace(
            start=lambda: SimpleNamespace(stopped=False)))
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            with (
                mock.patch.object(producer_ownership_mutations, 'tempfile',
                                  SimpleNamespace(mkdtemp=lambda **_: directory)),
                mock.patch.dict(sys.modules, {'mutation_baseline': owner}),
                mock.patch.object(guide_python, 'freeze', wraps=guide_python.freeze) as frozen,
                mock.patch.object(guide_python, 'run', return_value=refused) as run,
                redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(producer_ownership_mutations.main(), 1)
            frozen.assert_called_once_with('test_producer_ownership', search,
                                           'test_producer_ownership.py', 11, evidence)
            run.assert_called_once()
            expected = run.call_args.args[1]
            self.assertEqual(len(expected), 11)
            self.assertIn(
                'test_producer_ownership.ProducerOwnership.'
                'test_pid_readiness_is_published_only_after_complete_record_for_all_four',
                expected,
            )
            self.assertEqual(expected, sorted(set(expected)))

    def test_native_freeze_still_refuses_the_stale_ten_test_count(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            self.assertRaisesRegex(guide_python.DiscoveryRefused, 'frozen inventory mismatch'),
        ):
            guide_python.freeze('test_producer_ownership', ROOT / 'scripts/tests',
                                'test_producer_ownership.py', 10, Path(directory))


if __name__ == '__main__':
    unittest.main()
