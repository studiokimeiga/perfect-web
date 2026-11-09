#!/usr/bin/env python3
"""The longer-holds ADR: a subscription is held until something happens, up
to `HOLD` (25 s), a stream's and a long poll's alike; held, its page is live;
and a page gone ends its hold at the next pass.

Each control undoes one rule (`main.rs`):
- a held stream, or a held long poll, never looks for its page gone;
- a quiet connection is taken for a page gone, so every hold ends at once;
- a held long poll, or a held stream, keeps its page live no longer (its
  pass no longer renews the document's `seen`);
- the hold is two seconds again.

The tests: `tests::longer_holds::`, and the idle subscription's
(`tests::an_idle_subscription_waits_on_the_table_rather_than_asking_it`), in
one frozen inventory.

Strict, as `session_overlap_mutations.py` is: the shared native assertion validator
(each failing test fails at one `assert` in the named sources), the exact
test inventory on the baseline and every control, every source restored and
checked by digest. A build, timeout or other panic is an invalid run, never
a kill; the first invalid or surviving control stops it. Run from the
repository root, whole.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
LONGER_HOLDS = SERVER / "tests/longer_holds.rs"
# The files a failing test may fail in: its assertion's own.
SOURCES = [LONGER_HOLDS, MAIN]
TEST_NAMES = [
    "tests::an_idle_subscription_waits_on_the_table_rather_than_asking_it",
    "tests::longer_holds::a_held_poll_keeps_its_page_live",
    "tests::longer_holds::a_held_stream_keeps_its_page_live",
    "tests::longer_holds::a_host_holds_a_subscription_twenty_five_seconds",
    "tests::longer_holds::a_polls_page_gone_ends_its_hold",
    "tests::longer_holds::a_streams_page_gone_ends_its_hold",
    "tests::longer_holds::an_idle_poll_is_held_its_whole_hold",
    "tests::longer_holds::a_poll_that_does_not_ask_is_not_held_long",
    "tests::longer_holds::a_stream_that_does_not_ask_is_not_held_long",
    "tests::longer_holds::a_stream_that_asks_is_held_long",
    "tests::longer_holds::a_regeneration_that_failed_is_tried_again_within_a_hold",
    "tests::longer_holds::a_regeneration_that_failed_is_tried_again_within_a_held_stream",
    "tests::longer_holds::a_stream_held_long_ends_before_its_queue_fills",
    "tests::longer_holds::a_poll_past_the_limit_is_held_as_any_page_is",
    "tests::longer_holds::a_long_hold_gives_its_place_back",
]
COMMAND = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
           "--", *TEST_NAMES, "--exact", "--test-threads=1", "--format=pretty"]

# (what, file, anchor, replacement)
MUTANTS = [
    ("a held stream never looks for its page gone", MAIN,
     "        if gone(stream) {\n"
     "            ended(format_args!(\"stream's page gone at pass {pass}\"));\n"
     "            return;\n"
     "        }\n",
     ""),
    ("a held long poll never looks for its page gone", MAIN,
     "        if gone(stream) {\n"
     "            return;\n"
     "        }\n",
     ""),
    ("a quiet connection is taken for a page gone", MAIN,
     "            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted\n",
     "            std::io::ErrorKind::Interrupted\n"),
    ("a held long poll keeps its page live no longer", MAIN,
     "            waiting.seen = std::time::Instant::now();\n"
     "            waiting.acknowledge(since);\n",
     "            waiting.acknowledge(since);\n"),
    ("a held stream keeps its page live no longer", MAIN,
     "            waiting.seen = std::time::Instant::now();\n"
     "            let filling = waiting.frames.len() >= MAX_WAITING / 2;\n",
     "            let filling = waiting.frames.len() >= MAX_WAITING / 2;\n"),
    ("the hold is two seconds again", MAIN,
     "const HOLD: std::time::Duration = std::time::Duration::from_secs(25);\n",
     "const HOLD: std::time::Duration = std::time::Duration::from_secs(2);\n"),
    ("a poll is held long whether it asks or not", MAIN,
     "    let until = opened + if long { server.long_hold } else { server.poll_hold };\n",
     "    let until = opened + server.long_hold;\n"),
    ("a poll is never held long", MAIN,
     "    let until = opened + if long { server.long_hold } else { server.poll_hold };\n",
     "    let until = opened + server.poll_hold;\n"),
    ("a stream is held long whether it asks or not", MAIN,
     "        let hold = if long { server.long_hold } else { server.stream_hold };\n",
     "        let hold = server.long_hold;\n"),
    ("a stream is never held long", MAIN,
     "        let hold = if long { server.long_hold } else { server.stream_hold };\n",
     "        let hold = server.stream_hold;\n"),
    ("a held long poll does not try again", MAIN,
     "        if gone(stream) {\n"
     "            return;\n"
     "        }\n"
     "        server.try_again_within(session, &mut tried);\n",
     "        if gone(stream) {\n"
     "            return;\n"
     "        }\n"),
    ("a held stream does not try again", MAIN,
     "            ended(format_args!(\"stream's page gone at pass {pass}\"));\n"
     "            return;\n"
     "        }\n"
     "        server.try_again_within(session, &mut tried);\n",
     "            ended(format_args!(\"stream's page gone at pass {pass}\"));\n"
     "            return;\n"
     "        }\n"),
    ("a hold never tries again", MAIN,
     "        if tried.elapsed() < self.retried_every {\n            return;\n        }\n",
     "        if true {\n            return;\n        }\n"),
    ("a hold tries again whatever is current", MAIN,
     "                .is_some_and(|entry| entry.stale);\n",
     "                .is_some_and(|_| false);\n"),
    ("no limit on how many are held long", MAIN,
     "        if self.long_holds.fetch_add(1, SeqCst) >= self.long_hold_limit {\n",
     "        if self.long_holds.fetch_add(1, SeqCst) >= usize::MAX {\n"),
    ("the limit is one fewer than it says", MAIN,
     "        if self.long_holds.fetch_add(1, SeqCst) >= self.long_hold_limit {\n",
     "        if self.long_holds.fetch_add(1, SeqCst) + 1 >= self.long_hold_limit {\n"),
    ("a place refused is kept", MAIN,
     "            self.long_holds.fetch_sub(1, SeqCst);\n            return None;\n",
     "            return None;\n"),
    ("a place held is never given back", MAIN,
     "impl Drop for HeldLong<'_> {\n    fn drop(&mut self) {\n        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);\n",
     "impl Drop for HeldLong<'_> {\n    fn drop(&mut self) {\n        let _ = self.0;\n"),
    ("a stream's queue fills", MAIN,
     "            if filling {\n                break;\n            }\n", ""),
    ("a stream's queue is full before it ends", MAIN,
     "            let filling = waiting.frames.len() >= MAX_WAITING / 2;\n",
     "            let filling = waiting.frames.len() >= MAX_WAITING;\n"),
]

# How long one run of the tests may take. Past it the run is invalid.
TIMEOUT = 1200


def run_tests():
    try:
        child = producer_process.run(COMMAND, cwd=ROOT, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"INVALID test producer: {type(error).__name__}", flush=True)
        return None
    print(child.stdout, flush=True)
    return cargo_assertion_results.validate(child.returncode, child.stdout,
                                            root=ROOT, sources=SOURCES, expected=TEST_NAMES)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    files = sorted({path for _, path, _, _ in MUTANTS} | set(SOURCES))
    before = {str(path.relative_to(ROOT)): digest(path) for path in files}
    report = {"scope": "a subscription held until something happens (the longer-holds ADR)",
              "command": COMMAND, "producer_sha256": digest(Path(__file__)),
              "sources": before, "baseline": None, "controls": [], "restored": False,
              "valid": False}
    try:
        baseline = run_tests()
        report["baseline"] = baseline._asdict() if baseline else None
        if (not baseline or not baseline.valid or baseline.failed
                or baseline.passed != len(TEST_NAMES)):
            print("FAIL: the unmutated baseline is not green; no control can mean anything")
            mutation_baseline.explain()
            return 1
        for name, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1 or anchor == replacement:
                print(f"INVALID anchor: {name}")
                report["error"] = f"anchor: {name}"
                return 1
            with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
                result = run_tests()
            restored = digest(path) == before[str(path.relative_to(ROOT))]
            killed = bool(restored and result and result.valid and result.failed > 0)
            report["controls"].append({"name": name, "killed": killed, "restored": restored,
                                       "native": result._asdict() if result else None})
            print(f"{name}: {'KILLED' if killed else 'INVALID OR SURVIVED'}"
                  + (f" ({result.failed} of {result.passed + result.failed} tests fail)"
                     if killed else ""), flush=True)
            if not killed:
                return 1
        report["valid"] = True
    finally:
        report["restored"] = all(digest(path) == before[str(path.relative_to(ROOT))]
                                 for path in files)
        report["valid"] = report["valid"] and report["restored"]
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        killed = sum(control["killed"] for control in report["controls"])
        print(f"{killed} of {len(MUTANTS)} mutants killed; valid={report['valid']}, "
              f"restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
