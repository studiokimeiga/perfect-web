"""Native process ownership, including cancellation between Popen and assignment."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import producer_process

PRODUCERS = ('case_and_delivery_mutations', 'template_mutations',
             'device_store_mutations', 'qualified_device_mutations')


class ProducerOwnership(unittest.TestCase):
    def probe(self, producer, phase, disposition='callable', pause_pid_publication=False):
        with tempfile.TemporaryDirectory() as directory:
            tmp = Path(directory)
            source, pidfile, receipt, reaped, pid_opened, pid_release, pid_complete = (
                tmp / name for name in ('source', 'pid', 'receipt', 'reaped', 'pid-opened', 'pid-release', 'pid-complete'))
            source.write_text('original')
            program = r"""
import json, os, signal, subprocess, sys, time
from pathlib import Path
sys.path.insert(0, SCRIPTS)
import importlib
producer = importlib.import_module(PRODUCER)
source, pidfile, receipt, reaped, pid_opened, pid_release, pid_complete = map(Path, PATHS)
if DISPOSITION == 'callable':
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(37))
elif DISPOSITION == 'default':
    signal.signal(signal.SIGTERM, signal.SIG_DFL)
native = subprocess.Popen
class Injected(native):
    def __init__(self, *args, **kwargs):
        super().__init__([sys.executable, '-c', 'import time; time.sleep(60)'], **kwargs)
        try:
            # Existence is the outer probe's readiness signal. Publish only a
            # complete, closed record, even when its writer is descheduled.
            pending_pid = pidfile.with_name(pidfile.name + '.pending')
            with pending_pid.open('w') as record:
                if PAUSE_PID_PUBLICATION:
                    pid_opened.write_text('writer held before content')
                    end = time.monotonic() + 5
                    while not pid_release.exists() and time.monotonic() < end:
                        time.sleep(.001)
                    if not pid_release.exists():
                        raise RuntimeError('PID publication seam was not released')
                record.write(str(self.pid))
            pending_pid.replace(pidfile)
            if PAUSE_PID_PUBLICATION:
                pid_complete.write_text('record complete')
        except BaseException:
            self.kill()
            self.communicate()
            raise
        if PHASE in ('launch', 'cleanup'):
            os.kill(os.getpid(), signal.SIGTERM)
    def communicate(self, *args, **kwargs):
        if PHASE == 'cleanup' and not args and 'timeout' not in kwargs:
            os.kill(os.getpid(), signal.SIGTERM)
        result = super().communicate(*args, **kwargs)
        reaped.write_text(str(self.returncode))
        return result
subprocess.Popen = Injected
source.write_text('mutant')
try:
    if PRODUCER in ('case_and_delivery_mutations', 'template_mutations'):
        producer.TESTS = [['unused']]
        producer.run_tests()
    elif PRODUCER == 'device_store_mutations':
        producer.bounded(['unused'])
    else:
        producer.run()
finally:
    pid = int(pidfile.read_text())
    try:
        os.kill(pid, 0)
        alive = True
    except ProcessLookupError:
        alive = False
    source.write_text('original')
    receipt.write_text(json.dumps({'alive_at_restore': alive, 'source': source.read_text(), 'reaped': reaped.exists()}))
"""
            settings = '\n'.join(f'{name} = {value!r}' for name, value in [
                ('SCRIPTS', str(ROOT / 'scripts')), ('PRODUCER', producer),
                ('PATHS', list(map(str, (source, pidfile, receipt, reaped, pid_opened, pid_release, pid_complete)))),
                ('PHASE', phase), ('DISPOSITION', disposition), ('PAUSE_PID_PUBLICATION', pause_pid_publication)])
            wrapper = subprocess.Popen([sys.executable, '-c', settings + '\n' + program],
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            pid = None
            try:
                exposed_before_content = None
                if pause_pid_publication:
                    end = time.monotonic() + 5
                    while not pid_opened.exists() and wrapper.poll() is None and time.monotonic() < end:
                        time.sleep(.001)
                    self.assertTrue(pid_opened.exists(), 'the controlled PID writer did not reach its seam')
                    if pidfile.exists():
                        exposed_before_content = pidfile.read_text()
                    pid_release.write_text('resume writer')
                    end = time.monotonic() + 5
                    while not pid_complete.exists() and wrapper.poll() is None and time.monotonic() < end:
                        time.sleep(.001)
                    self.assertTrue(pid_complete.exists(), 'the controlled PID writer did not finish')
                end = time.monotonic() + 5
                while not pidfile.exists() and wrapper.poll() is None and time.monotonic() < end:
                    time.sleep(.01)
                self.assertTrue(pidfile.exists(), 'the native owned child did not launch')
                pid = int(pidfile.read_text())
                if phase == 'running':
                    wrapper.send_signal(signal.SIGTERM)
                output, _ = wrapper.communicate(timeout=5)
                expected = 37 if disposition == 'callable' else 128 + signal.SIGTERM
                self.assertEqual(wrapper.returncode, expected, output)
                self.assertTrue(receipt.exists(), 'cancellation skipped source restoration: ' + output)
                self.assertEqual(json.loads(receipt.read_text()),
                                 {'alive_at_restore': False, 'source': 'original', 'reaped': True}, output)
                self.assertEqual(source.read_text(), 'original')
                if pause_pid_publication:
                    self.assertIsNone(exposed_before_content,
                                      'PID readiness exposed an incomplete record before its writer finished')
            finally:
                pid_release.touch()
                if wrapper.poll() is None:
                    wrapper.kill()
                    wrapper.communicate()
                if pid is not None:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_pid_readiness_is_published_only_after_complete_record_for_all_four(self):
        # Hold the actual PID writer after open but before write: exists()
        # alone must never authorize int parsing or signalling an owned child.
        # Complete every reap/restoration assertion before checking exposure.
        for producer in PRODUCERS:
            with self.subTest(producer=producer):
                self.probe(producer, 'running', pause_pid_publication=True)

    def test_launch_cancellation_reaps_before_all_four_restore(self):
        for producer in PRODUCERS:
            with self.subTest(producer=producer):
                self.probe(producer, 'launch')

    def test_running_cancellation_reaps_before_all_four_restore(self):
        for producer in PRODUCERS:
            with self.subTest(producer=producer):
                self.probe(producer, 'running')

    def test_repeated_cancellation_during_cleanup_cannot_skip_restoration(self):
        for producer in PRODUCERS:
            with self.subTest(producer=producer):
                self.probe(producer, 'cleanup')

    def test_default_cancellation_raises_through_source_restoration(self):
        for producer in PRODUCERS:
            with self.subTest(producer=producer):
                self.probe(producer, 'launch', 'default')


    def test_native_output_and_nonzero_status_are_unchanged(self):
        result = producer_process.run([sys.executable, '-c',
            "import sys; print('out', flush=True); print('err', file=sys.stderr); sys.exit(17)"], timeout=5)
        self.assertEqual(result.returncode, 17)
        self.assertEqual(result.stdout, 'out\nerr\n')

    def test_timeout_reaps_and_preserves_native_partial_output(self):
        with tempfile.TemporaryDirectory() as directory:
            pidfile = Path(directory) / 'pid'
            child = "import os,time; from pathlib import Path; Path(" + repr(str(pidfile)) + ").write_text(str(os.getpid())); print('partial', flush=True); time.sleep(60)"
            pid = None
            try:
                with self.assertRaises(subprocess.TimeoutExpired) as expired:
                    producer_process.run([sys.executable, '-c', child], timeout=.2)
                self.assertEqual(expired.exception.output, 'partial\n')
                self.assertTrue(pidfile.exists())
                pid = int(pidfile.read_text())
                with self.assertRaises(ProcessLookupError):
                    os.kill(pid, 0)
            finally:
                if pid is None and pidfile.exists():
                    pid = int(pidfile.read_text())
                if pid is not None:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_child_default_and_ignored_signals_keep_native_semantics(self):
        original = signal.getsignal(signal.SIGTERM)
        try:
            for handler, code, output in [(lambda *_: None, -signal.SIGTERM, ''),
                                           (signal.SIG_IGN, 0, 'continued\n')]:
                signal.signal(signal.SIGTERM, handler)
                result = producer_process.run([sys.executable, '-c',
                    "import os,signal; os.kill(os.getpid(),signal.SIGTERM); print('continued')"], timeout=5)
                self.assertEqual((result.returncode, result.stdout), (code, output))
                self.assertIs(signal.getsignal(signal.SIGTERM), handler)
        finally:
            signal.signal(signal.SIGTERM, original)

    def test_ignored_parent_cancellation_stays_ignored(self):
        original = signal.getsignal(signal.SIGTERM)
        try:
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            try:
                result = producer_process.run([sys.executable, '-c',
                    "import os,signal; os.kill(os.getppid(),signal.SIGTERM); print('completed')"], timeout=5)
            except SystemExit as stopped:
                self.fail(f'an ignored signal cancelled the parent: {stopped}')
            self.assertEqual((result.returncode, result.stdout), (0, 'completed\n'))
            self.assertEqual(signal.getsignal(signal.SIGTERM), signal.SIG_IGN)
        finally:
            signal.signal(signal.SIGTERM, original)

    def test_child_does_not_inherit_a_blocked_cancellation_signal(self):
        result = producer_process.run([sys.executable, '-c',
            "import signal; print(int(signal.SIGTERM in signal.pthread_sigmask(signal.SIG_BLOCK, [])))"], timeout=5)
        self.assertEqual((result.returncode, result.stdout), (0, '0\n'))

    def test_launch_error_restores_original_handlers(self):
        before = {number: signal.getsignal(number) for number in (signal.SIGINT, signal.SIGTERM)}
        try:
            with self.assertRaises(FileNotFoundError):
                producer_process.run(['/this/producer/does/not/exist'], timeout=5)
            self.assertEqual({number: signal.getsignal(number) for number in before}, before)
        finally:
            for number, handler in before.items():
                signal.signal(number, handler)


if __name__ == '__main__':
    unittest.main()
