#!/usr/bin/env python3
"""Bounded ADR-0389 immutable-reader controls, not a full session gate.

Use the shared strict native assertion validator and restoration/process helpers.
Build/setup/timeout/IO failures are invalid, never kills. The four selected test
identities must complete on the baseline and every control. Run with an explicit
memory data layer; PostgreSQL, browsers and command fences are separate gates.
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
SOURCE = SERVER / "tests/session_revocation.rs"
TEST_NAMES = [
    "tests::session_revocation::an_anonymous_render_never_borrows_a_transient_authenticated_actor",
    "tests::session_revocation::a_stream_worker_keeps_its_render_actor_across_a_concurrent_account_change",
    "tests::session_revocation::an_anonymous_stream_worker_cannot_borrow_the_live_account",
    "tests::session_revocation::a_private_initial_document_cannot_finish_after_its_session_is_revoked",
]
COMMAND = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
           "--", *TEST_NAMES, "--exact", "--test-threads=1", "--format=pretty"]
MUTANTS = [
    ("principal reads ignore the invocation actor", SERVER / "identity.rs",
     "    pub fn of(&self, session: &str) -> Option<Principal> {\n"
     "        if session.is_empty() {\n            return None;\n        }\n"
     "        if let Some(reader) = self.invocation_reader(session) {\n",
     "    pub fn of(&self, session: &str) -> Option<Principal> {\n"
     "        if session.is_empty() {\n            return None;\n        }\n"
     "        if let Some(reader) = self.invocation_reader(session).filter(|_| false) {\n"),
    ("stream workers discard the captured reader", SERVER / "main.rs",
     "                let answer = principals.with_reader(session, &reader, || {\n"
     "                    server.fetch_answer(session, &run.stream, &run.args)\n"
     "                });\n",
     "                let _discarded = (principals, reader);\n"
     "                let answer = server.fetch_answer(session, &run.stream, &run.args);\n"),
    ("live delivery inspection borrows the frozen reader", SERVER / "identity.rs",
     "        Ok(self.cached_reader(session))\n", "        Ok(self.bound_reader(session))\n"),
]


def run_tests():
    try:
        child = producer_process.run(COMMAND, cwd=ROOT, timeout=180)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"INVALID test producer: {type(error).__name__}", flush=True)
        return None
    print(child.stdout, flush=True)
    return cargo_assertion_results.validate(child.returncode, child.stdout,
        root=ROOT, sources=[SOURCE], expected=TEST_NAMES)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    files = sorted({path for _, path, _, _ in MUTANTS} | {SOURCE})
    before = {str(path.relative_to(ROOT)): digest(path) for path in files}
    report = {"scope": "immutable synchronous readers only", "command": COMMAND,
              "producer_sha256": digest(Path(__file__)),
              "sources": before, "baseline": None, "controls": [], "restored": False,
              "valid": False}
    try:
        baseline = run_tests()
        report["baseline"] = baseline._asdict() if baseline else None
        if not baseline or not baseline.valid or baseline.failed or baseline.passed != len(TEST_NAMES):
            print("FAIL: baseline is not complete and green; no controls credited")
            return 1
        for name, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1:
                print(f"INVALID anchor: {name}")
                return 1
            with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
                result = run_tests()
            restored = digest(path) == before[str(path.relative_to(ROOT))]
            killed = bool(restored and result and result.valid and result.failed > 0)
            report["controls"].append({"name": name, "killed": killed, "restored": restored,
                "native": result._asdict() if result else None})
            print(f"{name}: {'KILLED' if killed else 'INVALID OR SURVIVED'}", flush=True)
            if not killed:
                return 1
        report["valid"] = True
    finally:
        report["restored"] = all(digest(path) == before[str(path.relative_to(ROOT))] for path in files)
        report["valid"] = report["valid"] and report["restored"]
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(f"controls: {len(report['controls'])}/{len(MUTANTS)}, valid={report['valid']}, restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
