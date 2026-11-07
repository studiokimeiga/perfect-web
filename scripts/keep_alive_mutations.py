#!/usr/bin/env python3
"""The keep-alive ADR: a host keeps a connection for its next request where
HTTP/1.1 keeps it, frames each request's body strictly, and tells each
commit to its readers after its answer.

Each control undoes one rule of `framed_length`, `answer_connection`,
`Conn` or `handle` (`main.rs`):
- a `transfer-encoding` read as no body; two `content-length`s, or one that
  is no number, taken as none; a folded header, or whitespace before a
  header's colon, read as a header (each would let a request be read out of
  another's body);
- a request that asks to close kept; an HTTP/1.0 request kept;
- a connection kept before its request's body is read whole;
- the host's cap on connections not held;
- a kept connection waiting without end for its next request;
- a connection kept after an answer not written whole by `write_response`,
  or after none (a stream, a subscription cut off);
- a commit told only when its connection closes, not after its answer;
- arming a dropped command (ADR-0175) leaving the kept connections waiting,
  so a browser sends the command again by itself and the page never sees
  the drop.

The tests: `tests::keep_alive::`, and the upload's own
(`tests::uploads::an_uploads_connection_is_closed_after_its_answer`), in one
frozen inventory.

Strict, as the other controls of this track's kind: the shared native
assertion validator (each failing test fails at one `assert` in the named
sources), the exact test inventory on the baseline and every control, every
source restored and checked by digest. A build, timeout or other panic is an
invalid run, never a kill; the first invalid or surviving control stops it.
Run from the repository root, whole.
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
KEEP_ALIVE = SERVER / "tests/keep_alive.rs"
UPLOADS = SERVER / "tests/uploads.rs"
# The files a failing test may fail in: its assertion's own.
SOURCES = [KEEP_ALIVE, UPLOADS]
TEST_NAMES = [
    "tests::keep_alive::a_commands_body_read_whole_keeps_its_connection",
    "tests::keep_alive::a_commit_is_told_after_its_answer_while_its_connection_is_kept",
    "tests::keep_alive::a_connection_is_kept_for_its_next_request",
    "tests::keep_alive::a_content_length_that_is_no_number_is_refused_and_closed",
    "tests::keep_alive::a_folded_header_is_refused_and_closed",
    "tests::keep_alive::a_kept_connection_is_closed_after_its_idle_wait",
    "tests::keep_alive::a_request_answered_with_nothing_closes_its_connection",
    "tests::keep_alive::a_request_that_asks_to_close_is_answered_and_closed",
    "tests::keep_alive::a_streams_connection_is_closed_with_it",
    "tests::keep_alive::an_answer_before_its_body_is_read_closes_the_connection",
    "tests::keep_alive::an_http_1_0_request_is_answered_and_closed",
    "tests::keep_alive::arming_a_drop_closes_the_kept_connections_waiting",
    "tests::keep_alive::past_the_cap_an_answer_closes_its_connection",
    "tests::keep_alive::requests_sent_together_are_answered_in_turn",
    "tests::keep_alive::the_keep_alive_header_says_the_idle_wait",
    "tests::keep_alive::transfer_encoding_is_refused_and_its_connection_closed",
    "tests::keep_alive::two_content_lengths_are_refused_and_closed",
    "tests::keep_alive::whitespace_before_a_headers_colon_is_refused_and_closed",
    "tests::uploads::an_uploads_connection_is_closed_after_its_answer",
    # The head read whole or refused (train-14's review).
    "tests::keep_alive::a_head_cut_short_by_a_header_that_is_no_text_is_refused_and_closed",
    "tests::keep_alive::a_head_that_trickles_past_its_deadline_is_refused_and_closed",
    "tests::keep_alive::a_head_past_its_size_is_refused_and_closed",
    "tests::keep_alive::a_head_request_is_answered_with_no_body_and_closed",
    "tests::keep_alive::a_body_that_trickles_past_its_deadline_is_not_read_on",
    "tests::keep_alive::the_next_request_waits_on_no_telling",
    "tests::keep_alive::the_accept_loop_backs_off_after_a_failure",
    "tests::keep_alive::a_second_answer_to_one_request_closes_its_connection",
]
COMMAND = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
           "--", *TEST_NAMES, "--exact", "--test-threads=1", "--format=pretty"]

# (what, file, anchor, replacement)
MUTANTS = [
    ("a transfer-encoding is read as no body", MAIN,
     '        if name.eq_ignore_ascii_case("transfer-encoding") {\n',
     '        if false && name.eq_ignore_ascii_case("transfer-encoding") {\n'),
    ("two content-lengths are taken as none", MAIN,
     '        _ => Err((400, "more than one content-length")),\n',
     '        _ => Ok(0),\n'),
    ("a content-length that is no number is taken as none", MAIN,
     '        [_] => Err((400, "content-length is not a number")),\n',
     '        [_] => Ok(0),\n'),
    ("a folded header is read as a header", MAIN,
     "        if line.starts_with([' ', '\\t']) {\n",
     "        if false {\n"),
    ("whitespace before a header's colon is read as a header", MAIN,
     "        if name != name.trim_end() {\n",
     "        if false {\n"),
    ("a request that asks to close is kept", MAIN,
     "        && !asks_to_close(&headers)\n",
     ""),
    ("an HTTP/1.0 request is kept", MAIN,
     '    let persists = version == "HTTP/1.1"\n',
     "    let persists = !version.is_empty()\n"),
    ("a connection is kept before its request's body is read", MAIN,
     "    stream.keep(persists && length == 0);\n",
     "    stream.keep(persists);\n"),
    ("the host's cap on connections is not held", MAIN,
     "        && server.open.load(std::sync::atomic::Ordering::SeqCst) <= KEPT_AT_MOST;\n",
     "        && true;\n"),
    ("a kept connection waits without end for its next request", MAIN,
     "    conn.set_read_timeout(Some(idle)).is_ok()\n",
     "    conn.set_read_timeout(None).is_ok()\n"),
    ("a connection is kept after an answer not written whole, or none", MAIN,
     "        self.keeps()\n"
     "            && self\n"
     "                .keeping\n"
     "                .answered\n"
     "                .load(std::sync::atomic::Ordering::SeqCst)\n"
     "    }\n",
     "        self.keeps()\n"
     "    }\n"),
    ("arming a dropped command leaves the kept connections waiting", MAIN,
     '            for (_, idle) in std::mem::take(&mut *server.idle.lock().expect("idle")) {\n',
     "            for (_, idle) in BTreeMap::<std::net::SocketAddr, TcpStream>::new() {\n"),
    ("a commit is told only when its connection closes", MAIN,
     "            if !server.telling.lock().expect(\"telling\").is_empty() {\n"
     "                scope.spawn(|| server.tell_waiting());\n"
     "            }\n"
     "            if !next_request(&conn, &mut reader, server.keep_alive) {\n"
     "                return;\n"
     "            }\n",
     "            if !next_request(&conn, &mut reader, server.keep_alive) {\n"
     "                server.tell_waiting();\n"
     "                return;\n"
     "            }\n"),
    # --- the head read whole or refused (train-14's review) ---
    ("a header that is no text ends the head", MAIN,
     '        let Ok(text) = String::from_utf8(line) else {\n'
     '            return Head::Refused(400, "a header is not text");\n'
     '        };\n',
     '        let Ok(text) = String::from_utf8(line) else {\n'
     '            let request = lines.first().cloned().unwrap_or_default();\n'
     '            return Head::Read { request, headers: lines.iter().skip(1).cloned().collect() };\n'
     '        };\n'),
    ("a stall mid-head is read as the head's end", MAIN,
     '        Head::Refused(408, "the request was not sent in time")\n',
     '        Head::Read { request: lines[0].clone(), headers: lines[1..].concat() }\n'),
    ("the deadline bounds each read of the head alone", MAIN,
     '        let _ = stream.set_read_timeout(Some(left));\n'
     '        let mut line = Vec::new();\n',
     '        let _ = stream.set_read_timeout(Some(REQUEST_READ_AT_MOST));\n'
     '        let mut line = Vec::new();\n'),
    ("a head has no size", MAIN,
     '        if size > HEAD_AT_MOST {\n'
     '            return Head::Refused(431, "the request\'s head is too large");\n'
     '        }\n',
     ''),
    ("a HEAD's answer carries its body", MAIN,
     '    let body = if stream.is_head_only() { &[][..] } else { body };\n',
     '    let body = body;\n'),
    ("a HEAD keeps its connection", MAIN,
     '        && !head_only\n',
     ''),
    ("the deadline bounds each read of the body alone", MAIN,
     '        let left = deadline.saturating_duration_since(std::time::Instant::now());\n'
     '        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {\n'
     '            return false;\n'
     '        }\n',
     '        let _ = stream.set_read_timeout(Some(REQUEST_READ_AT_MOST));\n'),
    # --- the connections (train-14's review) ---
    ("the telling runs before the next request", MAIN,
     '                scope.spawn(|| server.tell_waiting());\n',
     '                server.tell_waiting();\n'),
    ("the accept loop asks again at once", MAIN,
     '    std::time::Duration::from_millis(10u64.saturating_mul(1 << failures.min(7)).min(1000))\n',
     '    std::time::Duration::from_millis(0 * failures as u64)\n'),
    ("a second answer keeps its connection", MAIN,
     '    if stream.was_answered() {\n        stream.keep(false);\n    }\n',
     ''),
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
    report = {"scope": "a connection kept for its next request (the keep-alive ADR)",
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
