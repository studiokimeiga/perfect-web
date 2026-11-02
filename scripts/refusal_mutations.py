#!/usr/bin/env python3
"""Mutation controls for ADR-0302: a refusal is told where the press was.

Each mutant undoes one piece:
- the checker (PW0351): a predicate that says nothing, words that are two
  strings, empty or with a hole, a `requires` not held to a declared
  predicate's count or types, a predicate declared twice, `says` on what is
  not a predicate;
- the build and the host: no words written; a refusal answered without its
  words, without the program's, or without the deployment's; a refusal not
  kept with its press; a predicate the deployment cannot evaluate, required
  or declared, served; a page, of either kind, served without its
  announcer;
- the browser runtime: a refusal's predicate not read; its words not told;
  the message not named by the control, or not beside it; the announcer not
  emptied first, saying nothing, or a second one added beside the served
  one; a press keeping what its last was told; a press no answer came for,
  a stale page's and a failed handler's each told wrong or nothing; a page
  a build renders served without its announcer;
- the program: the feed's composer shown to a reader signed out.

A checker mutant must fail `pw-core`'s tests, all of them run; the build's
page's, `pw-render`'s binary's (`tests/titles.rs`); a host mutant, the
host's identity and announcer tests; a runtime or program mutant, the
browser tests below, in Chromium.

Run from the repository root; `just e14-refusal` records the output of
part 1 of `PARTS`, `just e14-refusal-2` part 2 (`--part i/2`; with no part,
every mutant runs). Each completed attempt or handled SIGTERM restores its
source in finally; externally interrupted runs require byte-verified
recovery before another run.
"""

from collections import Counter
import os
import pathlib
import re
import signal
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
PREDICATES = ROOT / "compiler/pw-core/src/predicates.rs"
POLICY = ROOT / "compiler/pw-core/src/policy.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RENDER = ROOT / "runtime/pw-render/src/bin/pw-render.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
FEED = ROOT / "examples/feed/app.pw"

# (what, suite, file, anchor, replacement)
# The mutants the checker refuses, by the rule that is the protection each
# takes away (`scripts/mutant_programs.py`): every other mutant here
# leaves a program that checks.
CHECKED = {
    "the feed's composer is shown to a reader signed out": {'PW5048'},
}

MUTANTS = [
    (
        "a predicate that says nothing is accepted",
        "core",
        PREDICATES,
        "        if decl.kind == DeclKind::Predicate {\n",
        '        if decl.kind == DeclKind::Predicate && decl.policy("says").is_some() {\n',
    ),
    (
        "two strings are one predicate's words",
        "core",
        PREDICATES,
        "        .filter(|inner| !inner.contains('\"'))\n",
        "",
    ),
    (
        "a hole is words",
        "core",
        PREDICATES,
        "    if inner.contains('{') || inner.contains('}') {\n",
        "    if false {\n",
    ),
    (
        "empty words are words",
        "core",
        PREDICATES,
        "    if inner.trim().is_empty() {\n",
        "    if false {\n",
    ),
    (
        "a `requires` is not held to a predicate's count",
        "core",
        PREDICATES,
        "            if declared.params.len() != predicate.arguments.len() {\n",
        "            if false {\n",
    ),
    (
        "a `requires` is not held to a predicate's types",
        "core",
        PREDICATES,
        "                    && !given.same_as(taken)\n",
        "                    && false\n",
    ),
    (
        "a predicate declared twice is accepted",
        "core",
        PREDICATES,
        # Re-anchored by ADR-0321, whose note names the first declaration.
        "                Some((at, module, span)) if u == unit => out.push(Diagnostic {\n",
        "                Some((at, module, span)) if u == unit && false => out.push(Diagnostic {\n",
    ),
    (
        "`says` is any declaration's",
        "core",
        POLICY,
        '        "says" => (&[K::Predicate], "a predicate"),\n',
        '        "says" => (EVERY, "a predicate"),\n',
    ),
    (
        "the build writes no words",
        "host",
        BUILD,
        '        predicates: crate::predicates::words_of(hirs),\n',
        "        predicates: Default::default(),\n",
    ),
    (
        "a refusal is answered without its words",
        "host",
        SERVER,
        '                    "says": says,\n',
        "",
    ),
    (
        "the program's words are not told",
        "host",
        SERVER,
        '                    let says = server\n                        .words\n                        .get(predicate)\n',
        '                    let says = server\n                        .words\n                        .get("")\n',
    ),
    (
        "the deployment's words are not told",
        "host",
        SERVER,
        "                    .or_else(|| identity::says(predicate))\n",
        "                    .or_else(|| None)\n",
    ),
    (
        "a refusal is not kept with its press",
        "host",
        SERVER,
        '            "refused": self.refused,\n',
        "",
    ),
    (
        "a predicate the deployment cannot evaluate is served",
        "host",
        SERVER,
        "        if !unknown.is_empty() {\n",
        "        if false {\n",
    ),
    (
        "a predicate the program declares is not asked of the deployment",
        "host",
        SERVER,
        "            .chain(words.keys().map(String::as_str))\n",
        "",
    ),
    (
        "a page that binds no query is served without its announcer",
        "host",
        SERVER,
        ".unwrap_or_default());\n"
        "    // Where a failed press is said (ADR-0302), from the first byte.\n"
        "    let announcer = pw_render::ANNOUNCER;\n",
        ".unwrap_or_default());\n"
        "    // Where a failed press is said (ADR-0302), from the first byte.\n"
        '    let announcer = "";\n',
    ),
    (
        "a page that binds a query is served without its announcer",
        "host",
        SERVER,
        "    };\n"
        "    // Where a failed press is said (ADR-0302), from the first byte.\n"
        "    let announcer = pw_render::ANNOUNCER;\n",
        "    };\n"
        "    // Where a failed press is said (ADR-0302), from the first byte.\n"
        '    let announcer = "";\n',
    ),
    (
        "a page a build renders holds no announcer",
        "render",
        RENDER,
        "        tail.push_str(pw_render::ANNOUNCER);\n",
        "",
    ),
    (
        "a refusal's predicate is not read",
        "browser",
        RUNTIME,
        '        if (refusal?.refused) throw new Refused(component, refusal.refused, refusal.says ?? "");\n',
        "",
    ),
    (
        "a refusal is not told in its predicate's words",
        "browser",
        RUNTIME,
        "          tell(el, error.says || PLATFORM_SAYS.failed, `refused:${error.predicate}`);\n",
        "          tell(el, PLATFORM_SAYS.failed, `refused:${error.predicate}`);\n",
    ),
    (
        "the message is not beside the control",
        "browser",
        RUNTIME,
        "    el.after(message);\n",
        "    document.body.append(message);\n",
    ),
    (
        "the message is not named by the control",
        "browser",
        RUNTIME,
        '    el.setAttribute("aria-describedby", [...described, message.id].join(" "));\n',
        "",
    ),
    (
        "the announcer is not emptied first",
        "browser",
        RUNTIME,
        '  status.textContent = "";\n  clearTimeout(saying);\n',
        "  clearTimeout(saying);\n",
    ),
    (
        "the announcer says nothing",
        "browser",
        RUNTIME,
        "    status.textContent = words;\n",
        "",
    ),
    (
        "a second announcer is added beside the served one",
        "browser",
        RUNTIME,
        '  announcer = document.querySelector(".pw-announcer[role=status]");\n',
        "",
    ),
    (
        "a press keeps what its last was told",
        "browser",
        RUNTIME,
        "      untell(el);\n",
        "",
    ),
    (
        "a press no answer came for is told it failed",
        "browser",
        RUNTIME,
        '          tell(el, PLATFORM_SAYS.unreachable, "unreachable");\n',
        '          tell(el, PLATFORM_SAYS.failed, "unreachable");\n',
    ),
    (
        "a stale page's press is told nothing",
        "browser",
        RUNTIME,
        '        tell(el, PLATFORM_SAYS.stale, "reload-loop");\n',
        '        el.dataset.pwHandlerError = "reload-loop";\n',
    ),
    (
        "a failed handler is told nothing",
        "browser",
        RUNTIME,
        '          tell(el, PLATFORM_SAYS.failed, "failed");\n',
        '          el.dataset.pwHandlerError = "failed";\n',
    ),
    (
        "the feed's composer is shown to a reader signed out",
        "browser",
        FEED,
        # Re-anchored by ADR-0307, whose composer asks the deployment's `SignedIn`:
        # without its guard the build now refuses it (PW5048).
        "            <form hidden={!SignedIn} on:submit|prevent={() => match post_text(draft) {\n",
        "            <form on:submit|prevent={() => match post_text(draft) {\n",
    ),
]

CARGO = {
    # The tests that can see a predicate's rules (W6's ruling of 2026-10-09:
    # a mutant that takes long is given the tests that can see its rule).
    # Each core mutant ran pw-core's tests whole, linking every test target
    # again, and the script took 3.5 h of CI (ADR-0302's verify); each is
    # killed by these alone (8 of 8, 103 s, 2026-10-10).
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "predicates"],
    "render": ["cargo", "test", "--quiet", "--locked", "-p", "pw-render", "--test", "titles"],
    "host": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "sign_in::", "a_signal_page_holds_its_announcer",
    ],
}
SPECS = [
    ["e2e/identity.spec.mjs", "--project=chromium"],
    ["e2e/recovery.spec.mjs", "-g", "still stale", "--project=chromium"],
    ["e2e/lazy-handler.spec.mjs", "-g", "fails to load", "--project=chromium"],
    ["e2e/accessibility.spec.mjs", "-g", "announcer|same node from the start", "--project=chromium"],
]

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
        return False, 0, 0
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    passed = sum(int(p) for p, _ in found)
    failed = sum(int(f) for _, f in found)
    valid = code == (101 if failed else 0) and passed + failed > 0
    return valid, passed, failed


class CompilerRefusal(tuple):
    """Actual check diagnostics from a failed source-program build."""
    def __new__(cls, codes):
        result = super().__new__(cls, (False, 0, 0))
        result.codes = frozenset(codes)
        return result


def browser_tests(_suite="browser"):
    """(built, passed, failed), after a build: the store's page and the
    feed's, each of which serves the runtime it was built with, and the
    server the suite runs, which neither script builds."""
    for script in ["run.sh", "feed.sh"]:
        built = subprocess.run(
            ["bash", f"spikes/own-renderer/{script}"],
            cwd=ROOT,
            env={**os.environ, "BUILD_ONLY": "1"},
            capture_output=True,
            text=True,
        )
        if built.returncode != 0:
            # feed.sh stops at its actual CLI check; only the already named
            # diagnostic set can earn this program mutant a checker kill.
            output = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", built.stdout + built.stderr)
            codes = set(re.findall(r"(?m)^error: \[(PW\d{4})\]", output))
            if script == "feed.sh" and built.returncode == 1 and codes:
                return CompilerRefusal(codes)
            return False, 0, 0
    server = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
        return False, 0, 0
    # Preserve each original file/grep/project selection through Playwright's
    # own --list/--test-list interface. Verify its exact selected multiset
    # before sharing one startup; new/overlapping selections cannot disappear.
    expected = []
    for spec in SPECS:
        listed = listed_tests(spec)
        if listed is None:
            return False, 0, 0
        expected.extend(listed)
    with tempfile.TemporaryDirectory(prefix="pw-refusal-selection-") as directory:
        selection = pathlib.Path(directory) / "tests.txt"
        selection.write_text("\n".join(expected) + "\n")
        batch = ["--test-list", str(selection), "--project=chromium"]
        actual = listed_tests(batch)
        if actual is None or Counter(expected) != Counter(actual):
            print("FAIL: batched refusal selection differs from the original test multiset", flush=True)
            return False, 0, 0
        out, code = bounded(
            ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",
             "pnpm", "exec", "playwright", "test", *batch, "--reporter=line"],
            cwd=ROOT / "spikes/own-renderer",
        )
    if code is None:
        return False, 0, 0
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    command_failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    failed = command_failed
    complete = True
    # Counts describe tests; run_suite owns producer status/no-test refusal.
    if code != (1 if command_failed else 0):
        complete = False
    return complete and passed + failed > 0, passed, failed


def listed_tests(spec):
    """Playwright's own replayable selection rows, never an assertion result."""
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *spec, "--list", "--reporter=list"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code != 0:
        return None
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    rows = [line.strip() for line in out.splitlines()
            if line.strip().startswith("[chromium]") and " › " in line]
    return rows or None


SUITES = {
    "core": cargo_tests,
    "render": cargo_tests,
    "host": cargo_tests,
    "browser": browser_tests,
}

# **The parts a run of these mutants is split into** (the integrator's
# ruling 1 of 2026-10-17, train-10): the recipe no longer fit a CI job. On
# train-10's verify (39054436657) each browser mutant took about 1,800 s,
# where train-9's took about 1,110 s, and the whole, with the recipe's tests,
# browsers and baselines, some 26,600 s, past the 20,700 s a job runs: its
# shard was stopped at the limit after 20,507 s. Ranges of `MUTANTS`, by
# index, half open: part 1 the checker's, the host's, the render's and five
# browser mutants, part 2 the other seven, each some four and a half hours on
# that run's times. `e14-refusal` runs part 1 and `e14-refusal-2` part 2, each
# with its own evidence and its own count; `evidence_counts.py` holds the two
# to every mutant once. No mutant is left out: a run with no part runs all.
PARTS = [(0, 23), (23, 30)]


def chosen(argv: list[str]) -> tuple[list, str]:
    """The mutants a run asks for, and how its output says which: `--part
    i/n`, the i-th of `PARTS`, where n is how many there are; every mutant
    where nothing is asked."""
    if not argv:
        return list(MUTANTS), ""
    asked = re.fullmatch(r"(\d+)/(\d+)", argv[1]) if len(argv) == 2 and argv[0] == "--part" else None
    if asked is None or int(asked.group(2)) != len(PARTS) or not 1 <= int(asked.group(1)) <= len(PARTS):
        raise SystemExit(f"refusal_mutations.py: ask for every mutant, or --part i/{len(PARTS)}")
    i = int(asked.group(1))
    start, end = PARTS[i - 1]
    return MUTANTS[start:end], f"part {i} of {len(PARTS)}: mutants {start + 1} to {end} of {len(MUTANTS)}"


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    picked, which = chosen(sys.argv[1:])
    if which:
        print(which, flush=True)
    for suite, run in SUITES.items():
        # A part's baselines are its suites' alone: a suite none of its
        # mutants runs means nothing to it.
        if not any(m[1] == suite for m in picked):
            continue
        built, passed, failed = run(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or (watch is not None and watch.stopped):
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = refused = killed = 0
    for what, suite, path, anchor, replacement in picked:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            refused += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            result = SUITES[suite](suite)
            built, passed, failed = result
        finally:
            path.write_text(original)
        stopped = watch is not None and bool(watch.stopped)
        named = CHECKED.get(what, set())
        if isinstance(result, CompilerRefusal) and result.codes and result.codes == named and not stopped:
            verdict = f"KILLED (named checker refusal: {','.join(sorted(result.codes))})"
            killed += 1
        elif not built or passed + failed == 0 or stopped:
            verdict = "REFUSED (invalid build, status, empty result or stopped producer)"
            refused += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            killed += 1
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
    # The pages and the server are built again from the restored source.
    if any(m[1] == "browser" for m in picked):
        built, passed, failed = browser_tests()
        print(f"restored (browser): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or (watch is not None and watch.stopped):
            print("FAIL: the restored browser suite is not green")
            mutation_baseline.explain()
            return 1

    print(f"{killed} of {len(picked)} mutants killed; {refused} refused")
    return 1 if survivors or refused else 0


if __name__ == "__main__":
    sys.exit(main())
