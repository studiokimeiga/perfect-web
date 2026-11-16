"""**What made a mutation script's baseline red** (2026-10-08).

A script whose unmutated baseline fails says so and stops: "no mutant can
mean anything". Until this it said no more, and a red baseline on CI or here
named neither the test that failed nor the build error: on 2026-10-08
`invariants_mutations.py` said "586 passed, 2 failed", and finding the two,
two committed components gone stale, took the suite run again by hand.

A script imports this module first in its `main()`. From then on what each
command it runs says is kept as the command ends, with how it ended: the
last `KEPT` commands, the end of what each said. Where the script reports a
red baseline, `explain()` prints what failed in what its commands said: each
failing test and the first line of its panic, a build's first errors, or,
from a browser suite, each failing test and its error. Nothing is run again:
a failure that comes and goes is named as it came, and the commands are the
script's own, however it holds them, in their own directories and
environments.

It keeps what `subprocess.Popen.communicate` returns, which
`subprocess.run` calls. A script imports it in `main()`, not at its top, so
that loading a script to read its mutants (`ci_plan.py`,
`mutation_anchors.py`) needs nothing beside it.

Imported, it first makes the script run its sources as they are (track
evidence-integrity, finding 1). Python takes a cached `.pyc` for its source
where the two agree in the source's size and its modification time in whole
seconds, so a mutant of a Python source the same size as its anchor,
restored within the second it was written in, left its bytecode to stand
for the restored source in the next run: another mutant's, a baseline's, or
`just ci`'s. From the import on, neither the script nor any process it
starts writes bytecode, and the bytecode cached for each Python source its
`MUTANTS` change is removed: writing none stops no reading, and bytecode
left by an earlier run would still be read.

Imported, it also bounds in memory every process the script starts from
then on (`mutation_bound.py`, 2026-10-09): a mutant's test that holds more
than the bound is stopped, the script's output says so, and its last line
says what the bound did, a kind of kill apart from a test's.

And first, where `PW_MUTANT_SLOTS` asks for more than one slot, it hands
the run to `mutation_slots.py`, which runs the script's own copy in each
slot, its mutants dealt among them, and ends this process with the run's
code: the script's loop never runs in this tree.

Imported, it also makes a SIGTERM unwind the script, where its disposition
is still the default (`mutation_source.unwinding`, 2026-11-16): a default
SIGTERM ends Python with no `finally`, and the mutant a script's `finally`
would put back stayed in the tree.
And where the run sets `PW_MUTANT_INCREMENTAL=1` (verify.yml's recipe step),
the script and every process it starts have no `CARGO_INCREMENTAL`, whatever
its recipe's line says, so that cargo's defaults apply (incremental for the dev
and test profiles), and the incremental caches are removed when they hold more
than a bound (`mutant_incremental.py`, 2026-10-20).
"""

import collections
import importlib.util
import os
import pathlib
import re
import subprocess
import sys
import time

# No bytecode written from here on, by the script (`dont_write_bytecode`) or
# by any process it starts, each of which inherits the environment's word.
sys.dont_write_bytecode = True
os.environ["PYTHONDONTWRITEBYTECODE"] = "1"


def forget_bytecode(mutants) -> list[pathlib.Path]:
    """Remove the bytecode cached for each Python source `mutants` change:
    each interpreter's in its `__pycache__`, and where this one would look
    for it. The files removed."""
    gone = []
    for mutant in mutants:
        path = mutant[-3] if isinstance(mutant, tuple) and len(mutant) >= 4 else None
        if not isinstance(path, pathlib.Path) or path.suffix != ".py":
            continue
        found = set((path.parent / "__pycache__").glob(f"{path.stem}.*.pyc"))
        found.add(pathlib.Path(importlib.util.cache_from_source(str(path))))
        for pyc in sorted(found):
            try:
                pyc.unlink()
            except FileNotFoundError:
                continue
            gone.append(pyc)
    return gone


# The script's own mutants: it imports this module first in its `main()`,
# so its `MUTANTS` are read before any of them is written.
forget_bytecode(getattr(sys.modules.get("__main__"), "MUTANTS", None) or [])

import mutation_bound
import mutation_slots

# Where `PW_MUTANT_SLOTS` asks for slots, the run is the slots' from here,
# and this process ends with its code: the script's loop never runs in this
# tree. In a slot, the script's `MUTANTS` are narrowed to those dealt to it
# (`mutation_slots.py`). Asked for none, nothing changes.
mutation_slots.engage()

# Every process the script starts from here on, bounded in memory.
mutation_bound.start()
# And a SIGTERM unwinds the script from here on, where its disposition is
# still the default, so that the `finally` putting a mutant back runs: most
# scripts write their mutants themselves, in a `try`/`finally`, which a
# default SIGTERM skips (`mutation_source.unwinding`, 2026-11-16).
import mutation_source  # noqa: E402 - after the bytecode settings, as the imports beside it
mutation_source.unwinding()

import mutant_incremental

# And, where the run asks (`PW_MUTANT_INCREMENTAL=1`, which verify.yml's recipe
# step sets), its builds incremental, whatever its recipe's line says
# (track ci-diagnostics, 2026-10-20).
mutant_incremental.start()

# How many commands are kept: a baseline's, and whatever ran since.
KEPT = 64
# How much of what one command said is kept: its end, where a test runner
# and a compiler say what failed.
TAIL = 256 * 1024

# Each command kept: [its arguments, its exit status or None where it was
# stopped past its bound, what it said].
heard = collections.deque(maxlen=KEPT)

ESCAPES = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


def as_text(data):
    """What a stream said, as text."""
    if data is None:
        return ""
    if isinstance(data, bytes):
        return data.decode("utf-8", "replace")
    return data


def keep(popen, status, said):
    """Keep what one command said; said again, its later word stands."""
    said = said[-TAIL:]
    kept = getattr(popen, "_heard", None)
    if kept is None:
        popen._heard = kept = [popen.args, status, said]
        heard.append(kept)
    else:
        kept[1] = status
        kept[2] = said or kept[2]


def keeping(communicate):
    def communicated(self, *args, **kwargs):
        try:
            out, err = communicate(self, *args, **kwargs)
        except subprocess.TimeoutExpired as stopped:
            keep(self, None, as_text(stopped.stdout) + as_text(stopped.stderr))
            raise
        keep(self, self.returncode, as_text(out) + as_text(err))
        try:
            mutant_incremental.bounded(self.args)
        except Exception:  # noqa: BLE001 - a cache left standing is no failure
            pass
        return out, err

    communicated.keeps = True
    return communicated


if not getattr(subprocess.Popen.communicate, "keeps", False):
    subprocess.Popen.communicate = keeping(subprocess.Popen.communicate)


def failures(output, failed=True):
    """What a command's output says failed, at most twenty lines. Output
    with no result at all is shown as it ended, where the command failed."""
    text = ESCAPES.sub("", output)
    lines = text.splitlines()
    said = []
    # cargo: a failing test, and the first line of its panic.
    for i, line in enumerate(lines):
        m = re.match(r"^test (\S+) \.\.\. FAILED$", line)
        if m:
            said.append(f"failed: {m.group(1)}")
        m = re.match(r"^thread '([^']+)' .*panicked at (\S+):$", line)
        if m and i + 1 < len(lines):
            said.append(f"  {m.group(1)} at {m.group(2)}: {lines[i + 1].strip()[:160]}")
    # A build that failed: its first errors, where.
    for i, line in enumerate(lines):
        if re.match(r"^error(\[E\d+\])?: ", line):
            where = next(
                (l.strip() for l in lines[i + 1 : i + 4] if l.strip().startswith("-->")), ""
            )
            said.append(f"build: {line.strip()[:160]} {where}".rstrip())
    # Playwright: each failing test, and its error.
    for i, line in enumerate(lines):
        m = re.match(r"^\s+\d+\) (\[\w+\] › .+)$", line)
        if m:
            error = next((l.strip() for l in lines[i + 1 : i + 6] if "Error" in l), "")
            said.append(f"failed: {m.group(1).strip()[:160]} {error[:120]}".rstrip())
    # No result at all: how its output ended, which says why.
    if failed and not said and not result(text) and lines:
        said = ["no test result; its output ended:"] + [f"  {l[:160]}" for l in lines[-12:]]
    return said[:20]


# How much of one record a red baseline shows.
RECORD = 16 * 1024


def records(output):
    """What a failing browser test printed of its pages and its server, one
    line each beginning `pw-record`, the last four, each cut to RECORD
    characters. A recipe keeps no attachment, and a failure on CI alone
    (2026-10-09, "a follow reaches another reader") is read in these."""
    text = ESCAPES.sub("", output)
    found = [l.strip() for l in text.splitlines() if l.lstrip().startswith("pw-record")]
    return [l if len(l) <= RECORD else l[:RECORD] + " (cut)" for l in found[-4:]]


def result(output):
    """A test runner's last result line: cargo's, or Playwright's count."""
    found = re.findall(
        r"^(test result: .*|\s+\d+ (?:passed|failed|flaky|skipped).*)$",
        ESCAPES.sub("", output),
        re.M,
    )
    return found[-1].strip() if found else ""


def shown(args):
    """A command as it was run, shortened."""
    line = " ".join(str(a) for a in args) if isinstance(args, (list, tuple)) else str(args)
    return line if len(line) <= 160 else line[:157] + "..."


def explain(out=None):
    """Print what failed in what the commands kept said. Nothing is run
    again."""
    out = out or sys.stdout
    print("what failed, as its commands said it:", file=out)
    told = False
    for args, status, said in heard:
        lines = failures(said, failed=status != 0)
        if status == 0 and not lines:
            continue
        ended = "stopped past its bound" if status is None else f"exit {status}"
        print(f"  {shown(args)} ({ended}):", file=out)
        for line in lines or ["(what it said was not captured)"]:
            print(f"    {line}", file=out)
        for line in records(said):
            print(f"    {line}", file=out)
        told = True
    if not told and heard:
        # Each command ended well, and the script counted its baseline red:
        # no test passed, or none ran. Each one's last result says which.
        print("  each command ended well; what each counted:", file=out)
        for args, _, said in heard:
            print(f"  {shown(args)}: {result(said) or '(no test result)'}", file=out)
    if not heard:
        print("  (no command it ran was kept)", file=out)
    keep_browser_output(out)
    out.flush()


# **A red baseline keeps its browser run's output** (track ci-diagnostics,
# 2026-10-19). Playwright clears `test-results/` at the start of each run, and
# every run after this script's in a CI shard is another's: the baseline's
# failure was lost with the shard's later runs. Its trace is kept, apart from
# those, in the suite's `kept-traces/<recipe>/<time>/<script>-baseline/`
# (`keep_traces.py`: bounded, uploaded by `verify.yml`).
#
# **Only a baseline's.** A mutant's browser run fails by design, once per
# mutant and hundreds to a shard, and its trace says nothing that the kill
# does not: they would swamp the artifact. `explain()` is called where the
# baseline is red, before any mutant has run, and where a script stops at a
# mutant's producer it refused (292 calls on train-11; the fourteen after no
# red baseline's usual words are named in `tests/test_mutation_baseline.py`),
# so what is kept is the output of the LAST command the script ran, and only
# where that was a browser run that failed: a refused producer's call finds a
# cargo command last, and keeps nothing.
SUITES = ("spikes/own-renderer", "benchmarks/harness")
ROOT = pathlib.Path(__file__).resolve().parent.parent
# When this script began: a `test-results/` older than that is an earlier
# recipe's, and is not this baseline's to keep.
STARTED = time.time()


def runs_a_suite(args):
    """Whether a kept command is a Playwright run: `... playwright test ...`,
    as a list of words or as one command line."""
    words = [str(a) for a in args] if isinstance(args, (list, tuple)) else str(args).split()
    return "test" in words and any("playwright" in w for w in words)


def keep_browser_output(out):
    """Keep the output of the script's last command where it was a browser run
    that failed, in each suite directory it left some in since the script
    began. Says where, or why not; never raises, since it runs where a
    baseline has failed already."""
    last = heard[-1] if heard else None
    if last is None or last[1] == 0 or not runs_a_suite(last[0]):
        return
    try:
        import keep_traces

        script = pathlib.Path(getattr(sys.modules.get("__main__"), "__file__", None) or "script").stem
        tag = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        for suite in SUITES:
            source = ROOT / suite / "test-results"
            if not source.is_dir() or source.stat().st_mtime < STARTED:
                continue
            root = ROOT / suite / "kept-traces"
            dest = keep_traces.destination(root, os.environ.get("PW_RECIPE"), tag, f"{script}-baseline")
            print(f"  its browser run's output: {keep_traces.keep(source, dest, root).says()}", file=out)
    except Exception as error:  # noqa: BLE001 - a failed baseline is told anyway
        print(f"  its browser run's output could not be kept: {error}", file=out)
