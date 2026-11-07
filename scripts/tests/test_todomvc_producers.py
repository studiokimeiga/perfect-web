"""TodoMVC mutation evidence requires native complete assertion results."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import todomvc_mutations as todo
import todomvc_producer_mutations as guards


class Producers(unittest.TestCase):
    def setUp(self):
        todo.INVENTORIES.clear()

    def cargo(self, failed=False, foreign=False, operand=False):
        names = sorted(todo.SERVER_NAMES)
        if foreign:
            names[-1] = "tests::foreign"
        failing = names[0]
        tests = ''.join(f'test {n} ... {"FAILED" if failed and n == failing else "ok"}\n' for n in names)
        if not failed:
            return tests + f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'
        lines = todo.SERVER_SOURCE.read_text().splitlines()
        line = next(i for i, s in enumerate(lines, 1) if s.lstrip().startswith('assert!('))
        column = len(lines[line - 1]) - len(lines[line - 1].lstrip()) + 1
        if operand:
            column += 10
        site = f'spikes/own-renderer/server/src/tests/todomvc.rs:{line}:{column}'
        return (tests + f"\nfailures:\n\n---- {failing} stdout ----\nthread '{failing}' panicked at {site}:\n"
                f'assertion failed\n\nfailures:\n    {failing}\n'
                f'test result: FAILED. {len(names)-1} passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n')

    def server(self, status, output):
        with mock.patch.object(todo, 'bounded', return_value=(output, status)), contextlib.redirect_stdout(io.StringIO()):
            return todo.server_tests()

    def test_server_accepts_complete_native_pass_and_assertion(self):
        self.assertEqual(self.server(0, self.cargo()), (True, 4, 0))
        self.assertEqual(self.server(101, self.cargo(True)), (True, 3, 1))

    def test_server_refuses_timeout_build_error_and_native_disagreement(self):
        for status in (None, -9, 0, 1, True):
            self.assertFalse(self.server(status, self.cargo(True))[0])
        self.assertFalse(self.server(101, self.cargo(True) + 'error[E1234]: build failed')[0])

    def test_server_requires_exact_four_ids_and_assertion_site(self):
        self.assertFalse(self.server(0, self.cargo(foreign=True))[0])
        self.assertFalse(self.server(0, self.cargo().replace('0 ignored', '1 ignored'))[0])
        self.assertFalse(self.server(101, self.cargo(True, operand=True))[0])

    def test_build_failure_or_timeout_cannot_launch_browser(self):
        for stage in range(3):
            for status in (1, None, -9):
                calls = 0
                def native(*args, **kwargs):
                    nonlocal calls
                    calls += 1
                    return ('build failed', status) if calls == stage + 1 else ('ok', 0)
                with mock.patch.object(todo, 'bounded', side_effect=native) as run, contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(todo.browser_tests(pages=True), (False, 0, 0))
                    self.assertEqual(run.call_count, stage + 1)

    def browser(self, status=0, patch=None, present=True, expected=None):
        if expected is not None:
            todo.INVENTORIES['browser'] = expected
        def native(command, **kwargs):
            env = kwargs['env']
            ids = [f'case-{n}' for n in range(todo.BROWSER_COUNT)]
            report = dict(schema=1, complete=True, nonce=env['PW_ASSERTION_NONCE'],
                          inventory=ids, tests=[[n, 'passed'] for n in ids])
            if patch:
                report.update(patch)
            if present:
                Path(env['PW_ASSERTION_REPORT']).write_text(json.dumps(report))
            self.assertIn('--retries=0', command)
            self.assertIn('--project=chromium', command)
            self.assertIn('--reporter=line,./assertion-result-reporter.mjs', command)
            return '999 failed', status
        with mock.patch.object(todo, 'build', return_value=True), mock.patch.object(todo, 'bounded', side_effect=native), \
                contextlib.redirect_stdout(io.StringIO()):
            return todo.browser_tests()

    def test_browser_requires_fresh_receipt_and_native_exit(self):
        count = todo.BROWSER_COUNT
        self.assertEqual(self.browser(), (True, count, 0))
        failing = [[f'case-{n}', 'failed' if n == 0 else 'passed'] for n in range(count)]
        self.assertEqual(self.browser(1, {'tests': failing}), (True, count - 1, 1))
        for status, patch, present in ((1, {}, False), (0, {'nonce': 'old'}, True),
                                       (0, {'tests': failing}, True), (None, {}, True)):
            self.assertFalse(self.browser(status, patch, present)[0])

    def test_browser_requires_browser_count_complete_frozen_ids(self):
        self.assertFalse(self.browser(expected=('foreign',))[0])
        for patch in ({'tests': []}, {'complete': False},
                      {'tests': [[f'case-{n}', 'skipped'] for n in range(todo.BROWSER_COUNT)]},
                      {'inventory': ['one'], 'tests': [['one', 'passed']]}):
            todo.INVENTORIES.clear()
            self.assertFalse(self.browser(patch=patch)[0])

    def checker(self, code=1, suffix='', errors=None, summary=None):
        errors = errors or ['error: [PW6805] device data names its privacy']
        output = '\n'.join(errors + ['  --> app.pw:1:1', '   | source',
                         summary or f'pw check: 3 file(s), {len(errors)} error(s), 0 warning(s)']) + '\n' + suffix
        return todo.checker_result(code, output, 3)

    def test_checker_accepts_only_complete_native_diagnostics(self):
        self.assertEqual(todo.checker_result(0, 'pw check: 3 file(s), no diagnostics\n', 3), (True, True, set()))
        self.assertEqual(self.checker(), (True, False, {'PW6805'}))
        for code in (0, -9, None, 101, True):
            self.assertFalse(self.checker(code)[0])
        for suffix in ('error[E1234]: compiler failed', 'thread panicked', 'Traceback', 'arbitrary output'):
            self.assertFalse(self.checker(suffix=suffix)[0])
        self.assertFalse(self.checker(summary='pw check: 2 file(s), 1 error(s), 0 warning(s)')[0])
        self.assertFalse(self.checker(summary='pw check: 3 file(s), 2 error(s), 0 warning(s)')[0])
        self.assertFalse(todo.checker_result(1, 'error: [PW6805] truncated', 3)[0])

    def campaign(self, result=(True, 3, 1), *, stopped=False, restored=True, cancel=False,
                 checker=None, named=None, second=False):
        import mutation_baseline
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'owned'; source.write_text('original')
            watch = types.SimpleNamespace(stopped=False)
            output = io.StringIO()
            calls = []
            def mutant():
                calls.append('mutant')
                watch.stopped = stopped
                if cancel:
                    raise SystemExit(37)
                return result
            kind = todo.PROGRAM if checker is not None else todo.RUNTIME
            mutants = [('fault', kind, source, 'original', 'changed')]
            if second:
                mutants.append(('must not run after invalid', kind, source, 'original', 'changed'))
            with mock.patch.object(todo, 'MUTANTS', mutants), \
                 mock.patch.object(todo, 'CHECKED', {'fault': named} if named else {}), \
                 mock.patch.object(todo, 'RUNS', {kind: [('server', mutant)]}), \
                 mock.patch.object(todo, 'server_tests', return_value=(True, 4, 0)), \
                 mock.patch.object(todo, 'browser_tests', return_value=(True, 50, 0)), \
                 mock.patch.object(todo, 'runtime_browser_tests', return_value=(restored, 50, 0)), \
                 mock.patch.object(todo, 'checks', return_value=checker), \
                 mock.patch.object(todo.signal, 'signal'), \
                 mock.patch.object(mutation_baseline.mutation_bound, 'start', return_value=watch), \
                 contextlib.redirect_stdout(output):
                try:
                    if cancel:
                        with self.assertRaises(SystemExit) as error:
                            todo.main()
                        self.assertEqual(error.exception.code, 37)
                        code = 1
                    else:
                        code = todo.main()
                finally:
                    self.assertEqual(source.read_text(), 'original')
            return code, output.getvalue(), calls

    def test_invalid_or_stopped_producer_never_earns_kill(self):
        self.assertEqual(self.campaign()[0], 0)
        for result, stop in (((False, 0, 1), False), ((True, 0, 0), False), ((True, 0, 1), True)):
            code, output, _ = self.campaign(result, stopped=stop)
            self.assertEqual(code, 1)
            self.assertNotIn(': KILLED', output)
        self.assertEqual(self.campaign((True, 4, 0))[0], 1)

    def test_invalid_result_stops_before_the_next_control(self):
        code, output, calls = self.campaign((False, 0, 1), second=True)
        self.assertEqual(code, 1)
        self.assertEqual(calls, ['mutant'])
        self.assertNotIn(': KILLED', output)

    def test_checker_kill_requires_exact_named_codes(self):
        self.assertEqual(self.campaign(checker=(True, False, {'PW6805'}), named={'PW6805'})[0], 0)
        for checker, named in (((False, False, {'PW6805'}), {'PW6805'}),
                               ((True, False, {'PW6805', 'PW0001'}), {'PW6805'}),
                               ((True, False, {'PW6805'}), {'PW6805', 'PW0001'}),
                               ((True, False, {'PW6805'}), None),
                               ((True, True, set()), {'PW6805'})):
            code, output, _ = self.campaign(checker=checker, named=named)
            self.assertEqual(code, 1)
            self.assertNotIn(': KILLED', output)

    def test_restored_source_result_is_required(self):
        code, output, _ = self.campaign(restored=False)
        self.assertEqual(code, 1)
        self.assertIn('restored browser producer is not green', output)

    def test_cancellation_restores_owned_source(self):
        self.assertEqual(self.campaign(cancel=True)[0], 1)

    def test_timeout_uses_shared_owner_and_remains_invalid(self):
        with mock.patch.object(todo.producer_process, 'run', side_effect=subprocess.TimeoutExpired(['native'], 1, output='partial')) as run:
            self.assertEqual(todo.bounded(['native'], cwd=ROOT), ('partial', None))
            self.assertEqual(run.call_args.kwargs['timeout'], todo.BOUND)
            self.assertEqual(run.call_args.kwargs['cwd'], ROOT)

    def test_outer_requires_restored_full_suite(self):
        import mutation_baseline
        good = types.SimpleNamespace(result=types.SimpleNamespace(valid=True, failed=0, body_errors=()), output='')
        bad = types.SimpleNamespace(result=types.SimpleNamespace(valid=False, failed=0, body_errors=()), output='invalid')
        with mock.patch.object(guards, 'MUTANTS', []), \
             mock.patch.object(guards.guide_python, 'freeze', return_value=('witness',)), \
             mock.patch.object(guards.guide_python, 'run', side_effect=[good, bad]) as run, \
             mock.patch.object(guards.signal, 'signal'), \
             mock.patch.object(mutation_baseline.mutation_bound, 'start', return_value=types.SimpleNamespace(stopped=False)), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(guards.main(), 1)
            self.assertEqual(run.call_count, 2)

    def test_outer_sigterm_restores_real_owned_source(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'owned.py'
            path.write_text('value = 1\n')
            child = textwrap.dedent('''\
                import os,pathlib,signal,sys,types
                sys.path.insert(0, sys.argv[1])
                import todomvc_producer_mutations as campaign
                import mutation_baseline
                path = pathlib.Path(sys.argv[2])
                campaign.MUTANTS = [('fault', 'witness', path, 'value = 1', 'value = 2')]
                campaign.guide_python.freeze = lambda *args: ('witness',)
                mutation_baseline.mutation_bound.start = lambda: types.SimpleNamespace(stopped=False)
                calls = 0
                def native(*args, **kwargs):
                    global calls
                    calls += 1
                    if calls == 2:
                        assert path.read_text() == 'value = 2\\n'
                        print('owned source planted; sending SIGTERM', flush=True)
                        os.kill(os.getpid(), signal.SIGTERM)
                        raise RuntimeError('SIGTERM did not interrupt')
                    return types.SimpleNamespace(result=types.SimpleNamespace(valid=True, failed=0, body_errors=()), output='')
                campaign.guide_python.run = native
                sys.exit(campaign.main())
                ''')
            result = todo.producer_process.run([sys.executable, '-c', child, str(ROOT / 'scripts'), str(path)],
                                               timeout=10, grace=2, cwd=ROOT)
            self.assertIn('owned source planted; sending SIGTERM', result.stdout)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(path.read_text(), 'value = 1\n')


if __name__ == '__main__':
    unittest.main()
