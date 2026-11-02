"""Scheduler evidence refuses false kills and preserves owned sources."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location('scheduler_controls', Path(__file__).resolve().parents[1] / 'scheduled_replay_mutations.py')
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)

def green(name='case'):
    return SimpleNamespace(returncode=0, stdout=f'running 1 test\ntest {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n')

def red(path='case.rs', name='case', column=1):
    return SimpleNamespace(returncode=101, stdout=f"running 1 test\ntest {name} ... FAILED\n\nfailures:\n\n---- {name} stdout ----\nthread '{name}' panicked at {path}:1:{column}:\nassertion failed\n\nfailures:\n    {name}\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n")

class Results(unittest.TestCase):

    def run_child(self, child, source='assert!(false);\n', clean=True):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            p = root / 'case.rs'
            p.write_text(source)
            with patch.object(c, 'ROOT', root), patch.object(c, 'SOURCES', [p]), patch.object(c, 'TESTS', [['fake']]), patch.object(c, 'INVENTORY', {}), patch.object(c, 'cleanup_verified', return_value=clean), patch.object(c.producer_process, 'run', return_value=child), patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), contextlib.redirect_stdout(io.StringIO()):
                return c.run_tests()

    def test_real_native_assertion_and_green_neighbor(self):
        self.assertEqual(self.run_child(green())[:3], (True, 1, 0))
        self.assertEqual(self.run_child(red())[:3], (True, 0, 1))

    def test_setup_product_worker_and_io_panics_are_invalid(self):
        for (source, child) in [('panic!("setup");\n', red()), ('assert!(false);\n', red('product.rs')), ('assert!(false);\n', red(column=2)), ('assert!(false);\n', SimpleNamespace(returncode=-9, stdout='worker killed'))]:
            self.assertFalse(self.run_child(child, source)[0])

    def test_build_missing_zero_duplicate_skipped_and_contradictory_results_are_invalid(self):
        for child in [SimpleNamespace(returncode=101, stdout='error: failed to compile'), SimpleNamespace(returncode=0, stdout=''), SimpleNamespace(returncode=0, stdout='running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'), SimpleNamespace(returncode=101, stdout=green().stdout), SimpleNamespace(returncode=0, stdout=red().stdout), SimpleNamespace(returncode=0, stdout='running 2 tests\ntest case ... ok\ntest case ... ok\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'), SimpleNamespace(returncode=0, stdout='running 2 tests\ntest case ... ok\ntest other ... ignored\ntest result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n')]:
            self.assertFalse(self.run_child(child)[0])

    def test_leftover_owned_schema_invalidates_even_a_real_assertion_kill(self):
        self.assertFalse(self.run_child(red(), clean=False)[0])

    def test_equal_counts_cannot_replace_either_binarys_named_inventory(self):
        with patch.object(c, 'TESTS', [['first'], ['second']]), patch.object(c, 'INVENTORY', {}), patch.object(c, 'cleanup_verified', return_value=True), patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), patch.object(c.producer_process, 'run', side_effect=[green('a'), green('b'), green('a'), green('renamed')]), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(c.run_tests()[:3], (True, 2, 0))
            self.assertFalse(c.run_tests()[0])

    def test_missing_database_and_timeout_have_no_kill_credit(self):
        with patch.dict(os.environ, {}, clear=True), contextlib.redirect_stdout(io.StringIO()):
            self.assertFalse(c.run_tests()[0])
        with patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), patch.object(c.producer_process, 'run', side_effect=subprocess.TimeoutExpired('fake', 1)), contextlib.redirect_stdout(io.StringIO()):
            self.assertFalse(c.run_tests()[0])

    def test_cleanup_requires_fresh_owned_names_and_successful_empty_database_answer(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'receipt'
            for text in ['', 'public\n', 'pw_scheduler_store_0123456789abcdef\npw_scheduler_store_0123456789abcdef\n', "pw_scheduler_store_x'; DROP SCHEMA public;--\n"]:
                p.write_text(text)
                with patch.object(c.producer_process, 'run') as run:
                    self.assertFalse(c.cleanup_verified(p, 'test-db', True))
                    run.assert_not_called()
            p.write_text('pw_scheduler_store_0123456789abcdef\n')
            for (status, out, want) in [(0, '', True), (1, '', False), (0, 'pw_scheduler_store_0123456789abcdef\n', False)]:
                with patch.object(c.producer_process, 'run', return_value=SimpleNamespace(returncode=status, stdout=out)):
                    self.assertEqual(c.cleanup_verified(p, 'test-db', True), want)

    def test_invalid_mutant_fails_campaign_and_restores_source(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'source.rs'
            p.write_text('original')
            fake = SimpleNamespace(mutation_bound=SimpleNamespace(start=lambda : SimpleNamespace(stopped=False)), explain=lambda : None)
            with patch.object(c, 'MUTANTS', [('fault', p, 'original', 'mutated')]), patch.object(c, 'run_tests', side_effect=[(True, 1, 0, ()), (False, 0, 0, ())]), patch.dict(sys.modules, {'mutation_baseline': fake}), patch.object(sys, 'argv', ['producer']), contextlib.redirect_stdout(io.StringIO()) as out:
                self.assertEqual(c.main(), 1)
            self.assertEqual(p.read_text(), 'original')
            self.assertIn('INVALID', out.getvalue())
            self.assertIn('0 of 1 mutants killed', out.getvalue())

    def test_cancellation_restores_the_owned_source_and_propagates(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'source.rs'
            p.write_text('original')
            fake = SimpleNamespace(mutation_bound=SimpleNamespace(start=lambda : SimpleNamespace(stopped=False)), explain=lambda : None)
            with patch.object(c, 'MUTANTS', [('fault', p, 'original', 'mutated')]), patch.object(c, 'run_tests', side_effect=[(True, 1, 0, ()), SystemExit(143)]), patch.dict(sys.modules, {'mutation_baseline': fake}), patch.object(sys, 'argv', ['producer']), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    c.main()
                self.assertEqual(error.exception.code, 143)
            self.assertEqual(p.read_text(), 'original')

    def test_a_product_assertion_in_the_mixed_runtime_module_is_invalid(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            p = root / 'runtime/pw-materialize/src/scheduled.rs'
            p.parent.mkdir(parents=True)
            p.write_text('assert!(false);\n#[cfg(test)]\nmod tests {\nassert!(false);\n}\n')
            with patch.object(c, 'ROOT', root):
                self.assertFalse(c.assertions_are_in_tests('panicked at runtime/pw-materialize/src/scheduled.rs:1:1:'))
                self.assertTrue(c.assertions_are_in_tests('panicked at runtime/pw-materialize/src/scheduled.rs:4:1:'))
            p.write_text('assert!(false);\n')
            with patch.object(c, 'ROOT', root):
                self.assertFalse(c.assertions_are_in_tests('panicked at runtime/pw-materialize/src/scheduled.rs:1:1:'))

    def test_timeout_and_cancellation_preserve_owned_schema_names_in_the_log(self):

        def abort(error):

            def run(command, **kwargs):
                Path(kwargs['env']['PW_SCHEDULER_SCHEMA_RECEIPT']).write_text('pw_scheduler_store_0123456789abcdef\n')
                raise error
            return run
        for error in [subprocess.TimeoutExpired('fake', 1, output='partial native output'), SystemExit(143)]:
            with patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), patch.object(c.producer_process, 'run', side_effect=abort(error)), contextlib.redirect_stdout(io.StringIO()) as out:
                if isinstance(error, SystemExit):
                    with self.assertRaises(SystemExit):
                        c.run_tests()
                else:
                    self.assertFalse(c.run_tests()[0])
            self.assertIn('OWNED_SCHEDULER_SCHEMAS_ON_ABORT', out.getvalue())
            self.assertIn('pw_scheduler_store_0123456789abcdef', out.getvalue())

    def test_mixed_module_tail_or_duplicate_boundary_cannot_attribute_product_assertions(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            p = root / 'runtime/pw-materialize/src/scheduled.rs'
            p.parent.mkdir(parents=True)
            for text in ['#[cfg(test)]\nmod tests {\n    assert!(false);\n}\nfn product() {\n    assert!(false);\n}\n', '#[cfg(test)]\nmod tests {\n}\n#[cfg(test)]\nmod tests {\n    assert!(false);\n}\n']:
                p.write_text(text)
                with patch.object(c, 'ROOT', root):
                    self.assertFalse(c.assertions_are_in_tests('panicked at runtime/pw-materialize/src/scheduled.rs:6:5:'))

    def test_complete_native_run_with_unverified_cleanup_preserves_receipt_names(self):

        def child(command, **kwargs):
            Path(kwargs['env']['PW_SCHEDULER_SCHEMA_RECEIPT']).write_text('pw_scheduler_store_0123456789abcdef\n')
            return green()
        with patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), patch.object(c.producer_process, 'run', side_effect=child), patch.object(c, 'cleanup_verified', return_value=False), contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertFalse(c.run_tests()[0])
        self.assertIn('pw_scheduler_store_0123456789abcdef', out.getvalue())

    def test_failed_abort_reporting_does_not_replace_cancellation(self):
        with patch.dict(os.environ, {'PW_STORE_DATABASE_URL': 'test-db'}), patch.object(c.producer_process, 'run', side_effect=SystemExit(143)), patch.object(c, 'report_abort', side_effect=OSError('report unavailable')), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                c.run_tests()
        self.assertEqual(error.exception.code, 143)
if __name__ == '__main__':
    unittest.main()
