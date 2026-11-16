"""Release owned source/report resources without losing cancellation identity.

Process execution, signals, bounds and reaping remain with each caller.
"""
from contextlib import contextmanager
import signal
import sys
import threading


def cancellation(error):
    return isinstance(error, (SystemExit, KeyboardInterrupt))


def release(cleanup, primary):
    try:
        cleanup()
    except BaseException as secondary:
        if primary is None or (not cancellation(primary) and not isinstance(secondary, Exception)):
            raise
        try:
            print(f"cleanup also failed: {secondary!r}", file=sys.stderr)
        except BaseException as diagnostic:
            if not cancellation(primary) and not isinstance(diagnostic, Exception):
                raise


@contextmanager
def releasing(cleanup):
    primary = None
    try:
        yield
    except BaseException as error:
        primary = error
        raise
    finally:
        release(cleanup, primary)


def _unwind(number, frame):
    raise SystemExit(128 + number)


def unwinding():
    """**A SIGTERM unwinds** (2026-11-16): where SIGTERM's disposition is
    still the default, it becomes `SystemExit(128 + SIGTERM)`, so that the
    `finally` that puts a mutant back runs, as SIGINT's `KeyboardInterrupt`
    already lets it. A default SIGTERM ends Python with no `finally` and no
    context manager's exit, and the mutant written stays in the tree: a
    TaskStop of a local run could leave one, and CI's guide-ownership test
    did (`scripts/tests/test_mutation_source.py`). A disposition the script
    chose, a handler or SIGTERM ignored, is the script's and is kept; only
    the main thread may set one, and elsewhere nothing changes.
    `producer_process.run` defers the signal while its child runs, and
    calls this handler after its cleanup, as it calls any callable.
    Returns whether it set the handler."""
    if threading.current_thread() is not threading.main_thread():
        return False
    if signal.getsignal(signal.SIGTERM) is not signal.SIG_DFL:
        return False
    signal.signal(signal.SIGTERM, _unwind)
    return True


@contextmanager
def changed(path, original, replacement):
    """`path` holds `replacement` for the block, and `original` after it,
    whatever ends the block: an error, SIGINT, or a SIGTERM ([`unwinding`]),
    which unwinds from before the mutant is written until it is put back. A
    default disposition is the default again after it."""
    set_here = unwinding()
    try:
        with releasing(lambda: path.write_text(original)):
            path.write_text(replacement)
            yield
    finally:
        if set_here and signal.getsignal(signal.SIGTERM) is _unwind:
            signal.signal(signal.SIGTERM, signal.SIG_DFL)
