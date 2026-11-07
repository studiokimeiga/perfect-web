#!/usr/bin/env python3
"""Mutation controls for ADR-0184: what a cache may keep holds nothing of a
session's (charter §15.6 tests 2 and 13).

Each mutant undoes one piece:
- the wire: a session's response, whole or streamed, kept by any cache; a
  file of the build naming a session;
- the server's shared caches: a private query's value kept for every
  reader; a session's cart materialized in the public partition.

A server mutant must fail the development server's tests; a browser
mutant, `e2e/shared-output.spec.mjs` in Chromium, against a build of the
mutated source.

Run from the repository root; `just e14-shared-output` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

PRIVATE = 'const PRIVATE: &str = "cache-control: private, no-store\\r\\n";\n'

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a session's response may be kept by any cache",
        "server",
        SERVER,
        PRIVATE,
        'const PRIVATE: &str = "";\n',
    ),
    (
        "a streamed page may be kept by any cache",
        "server",
        SERVER,
        # Re-anchored by ADR-0300, which names the build after it, and by
        # the page-csp track, which says its type before it.
        '            "HTTP/1.1 200 OK\\r\\ncontent-type: text/html; charset=utf-8\\r\\n{NOSNIFF}{PRIVATE}{cookie}\\\n',
        '            "HTTP/1.1 200 OK\\r\\ncontent-type: text/html; charset=utf-8\\r\\n{NOSNIFF}{cookie}\\\n',
    ),
    (
        "a file of the build names a session",
        "server",
        SERVER,
        '    write_response(stream, code, mime, &runs_nothing(mime), body);\n',
        '    write_response(stream, code, mime, &format!("set-cookie: pw-session=leaked; Path=/\\r\\n{}", runs_nothing(mime)), body);\n',
    ),
    (
        "a private query's value is kept for every reader",
        "server",
        SERVER,
        "    if policy[\"cache\"] == \"private\" || policy[\"privacy\"] != \"public\" {\n"
        "        m = m.private();\n",
        "    if false {\n"
        "        m = m.private();\n",
    ),
    (
        "a session's cart is materialized in the public partition",
        "server",
        SERVER,
        "    EntryIdentity::new(\"store.page.Cart\", &[owner], session_partition(session)).generation(BUILD)\n",
        "    EntryIdentity::new(\"store.page.Cart\", &[owner], Partition::Public).generation(BUILD)\n",
    ),
    (
        "the store's page may be kept by any cache, as the browser is told",
        "browser",
        SERVER,
        PRIVATE,
        'const PRIVATE: &str = "";\n',
    ),
]

CARGO = {
    # The server tests that fail under its mutants, measured against the
    # whole suite by `scripts/mutant_killers.py` on 2026-10-12 (W6's ruling
    # of 2026-10-09), two per mutant where two fail.
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_session_s_response_is_kept_by_no_cache_and_a_build_s_names_no_session",
        "tests::messages::a_third_user_signed_in_and_not_sees_none_of_it",
        "tests::redirects::a_page_at_another_address_of_the_page_is_moved_there_for_good",
        "tests::what_a_shared_cache_keeps_holds_nothing_a_session_put_in_it",
    ],
}
BROWSER = ["e2e/shared-output.spec.mjs", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, **kw):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    out, code = bounded(CARGO[suite], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def build():
    """The page and the server the suite runs, as the source says."""
    page = subprocess.run(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                          env={**os.environ, "BUILD_ONLY": "1"}, capture_output=True, text=True)
    server = subprocess.run(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
                            cwd=ROOT, capture_output=True, text=True)
    return page.returncode == 0 and server.returncode == 0


def browser_tests(_suite="browser"):
    """(built, passed, failed) for the spec, after a build."""
    if not build():
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {
    "server": cargo_tests,
    "browser": browser_tests,
}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    try:
        for what, suite, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1:
                print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
                survivors += 1
                continue
            try:
                path.write_text(original.replace(anchor, replacement, 1))
                built, passed, failed = SUITES[suite](suite)
            finally:
                path.write_text(original)
            if not built:
                verdict = "KILLED (does not build)"
            elif failed:
                verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            else:
                verdict = "SURVIVED"
                survivors += 1
            print(f"{what} [{suite}]: {verdict}", flush=True)
    finally:
        # The page and the server as the source says, whatever a browser
        # mutant left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
