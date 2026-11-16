"""A mutant is put back when its script is told to stop (2026-11-16).

A default SIGTERM ends a Python process with no `finally`, no context
manager's exit: whatever mutant a mutation script had written stayed in the
tree. Since 2026-10 a TaskStop of a local run could leave one (the
integrator's notes say so), and on 2026-11-16 CI's
`test_guide_process_ownership.GuideOwnership.test_recursive_timeout_has_descending_cleanup_deadlines`
failed so: its nested fixture, told to stop between writing its mutant and
reaching `producer_process.run`, whose handler defers the signal, kept the
mutant. Each test here starts a child that holds a mutant, tells it to stop
once it says the mutant is written, and reads the source after it ends.
"""
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent


def held(body: str, tmp: pathlib.Path) -> subprocess.Popen:
    """A child running `body` with `scripts/` importable, its source at
    `tmp/source` holding `original`, and `tmp/ready` its word that the
    mutant is in place."""
    (tmp / "source").write_text("original")
    script = textwrap.dedent(
        f"""
        import pathlib, signal, sys, time
        sys.path.insert(0, {str(SCRIPTS)!r})
        source = pathlib.Path({str(tmp / "source")!r})
        ready = pathlib.Path({str(tmp / "ready")!r})
        """
    ) + textwrap.dedent(body)
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    for name in ("PW_MUTANT_SLOTS", "PW_MUTANT_INCREMENTAL"):
        env.pop(name, None)
    return subprocess.Popen([sys.executable, "-c", script], env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def stopped(child: subprocess.Popen, tmp: pathlib.Path) -> tuple[int, str]:
    """Tell `child` to stop once its mutant is in place; its status and
    what it said."""
    end = time.monotonic() + 20
    while not (tmp / "ready").exists():
        if child.poll() is not None or time.monotonic() > end:
            out, _ = child.communicate(timeout=10)
            raise AssertionError(f"the child never held its mutant: {out}")
        time.sleep(0.01)
    child.send_signal(signal.SIGTERM)
    out, _ = child.communicate(timeout=20)
    return child.returncode, out


@unittest.skipUnless(hasattr(signal, "SIGTERM") and os.name == "posix", "POSIX signals")
class TermRestores(unittest.TestCase):
    def test_a_sigterm_while_changed_holds_a_mutant_puts_the_source_back(self):
        with tempfile.TemporaryDirectory() as directory:
            tmp = pathlib.Path(directory)
            child = held(
                """
                import mutation_source
                with mutation_source.changed(source, "original", "mutant"):
                    ready.write_text("")
                    time.sleep(30)
                """,
                tmp,
            )
            status, out = stopped(child, tmp)
            self.assertEqual((tmp / "source").read_text(), "original", out)
            self.assertEqual(status, 128 + signal.SIGTERM, out)

    def test_a_sigterm_after_mutation_baseline_runs_a_scripts_own_finally(self):
        # Most mutation scripts write their mutants themselves, in a
        # `try`/`finally`, and each imports `mutation_baseline` first.
        with tempfile.TemporaryDirectory() as directory:
            tmp = pathlib.Path(directory)
            child = held(
                """
                import mutation_baseline
                try:
                    source.write_text("mutant")
                    ready.write_text("")
                    time.sleep(30)
                finally:
                    source.write_text("original")
                """,
                tmp,
            )
            status, out = stopped(child, tmp)
            self.assertEqual((tmp / "source").read_text(), "original", out)
            self.assertEqual(status, 128 + signal.SIGTERM, out)

    def test_a_disposition_the_script_chose_is_kept(self):
        # A script's own handler, or SIGTERM ignored, is the script's: only
        # the default is made to unwind.
        with tempfile.TemporaryDirectory() as directory:
            tmp = pathlib.Path(directory)
            child = held(
                """
                import mutation_source
                def own(number, frame):
                    source.write_text("own handler")
                    raise SystemExit(5)
                signal.signal(signal.SIGTERM, own)
                with mutation_source.changed(source, "original", "mutant"):
                    assert signal.getsignal(signal.SIGTERM) is own
                    ready.write_text("")
                    time.sleep(30)
                """,
                tmp,
            )
            status, out = stopped(child, tmp)
            self.assertEqual(status, 5, out)
            self.assertEqual((tmp / "source").read_text(), "original", out)
        with tempfile.TemporaryDirectory() as directory:
            tmp = pathlib.Path(directory)
            child = held(
                """
                import mutation_source
                signal.signal(signal.SIGTERM, signal.SIG_IGN)
                with mutation_source.changed(source, "original", "mutant"):
                    assert signal.getsignal(signal.SIGTERM) is signal.SIG_IGN
                    ready.write_text("")
                    time.sleep(1)
                print(signal.getsignal(signal.SIGTERM) is signal.SIG_IGN)
                """,
                tmp,
            )
            status, out = stopped(child, tmp)
            self.assertEqual((status, out.strip()), (0, "True"), out)
            self.assertEqual((tmp / "source").read_text(), "original", out)

    def test_changed_leaves_the_default_as_it_found_it(self):
        # Outside a mutant's life the script's disposition is what it was.
        with tempfile.TemporaryDirectory() as directory:
            tmp = pathlib.Path(directory)
            child = held(
                """
                import mutation_source
                with mutation_source.changed(source, "original", "mutant"):
                    inside = signal.getsignal(signal.SIGTERM)
                print(inside is not signal.SIG_DFL, signal.getsignal(signal.SIGTERM) is signal.SIG_DFL)
                """,
                tmp,
            )
            out, _ = child.communicate(timeout=20)
            self.assertEqual((child.returncode, out.strip()), (0, "True True"), out)
            self.assertEqual((tmp / "source").read_text(), "original", out)


if __name__ == "__main__":
    unittest.main()
