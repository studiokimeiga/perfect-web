#!/usr/bin/env python3
"""Mutation controls for ADR-0228: a value computed in a row is the row's.

Each mutant undoes one piece: the path a row's value is named by, from the
innermost loop's item; the host's row read, its function and its path; the
speculation module's value for each row and for a part, and what it refuses;
and what the feed's rows and likes found: a field of a value charged a
same-named function's effects, and an opaque value that did not compare. The
feed's like shown before the server answers must fail too.

The compiler's and the server's tests must fail for a mutant of the compiler
("cargo"). A mutant of the feed must fail `e2e/feed.spec.mjs` in three
engines, against a build of the mutated source ("browser").

Run from the repository root; `just e14-computed-rows` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
VALUES = ROOT / "compiler/pw-core/src/page_values.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
EFFECTS = ROOT / "compiler/pw-core/src/effects.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
FEED = ROOT / "examples/feed/app.pw"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a row's value is named by no item",
        "cargo",
        TEMPLATE,
        "            Some(item)\n                if inputs\n",
        "            Some(item)\n                if false && inputs\n",
    ),
    (
        "a row's value is named by the outermost loop's item",
        "cargo",
        TEMPLATE,
        "        match self.loops.last() {\n",
        "        match self.loops.first() {\n",
    ),
    (
        "a row's read does not run the function",
        "cargo",
        VALUES,
        "        row.steps.push(Step::Derived(component_id));\n",
        "",
    ),
    (
        "a row's value is set at its input's path",
        "cargo",
        VALUES,
        "        row.path = path.to_string();\n",
        "",
    ),
    (
        "a speculated row's value is not computed",
        "cargo",
        SPECULATION,
        "                computed.push((rest.to_string(), functions.len() - 1));\n",
        "",
    ),
    (
        "a speculated row's value from a field of its item is built",
        "cargo",
        SPECULATION,
        "                if read != binding {\n",
        "                if false && read != binding {\n",
    ),
    (
        "a part computed from a speculated value is not computed again",
        "cargo",
        SPECULATION,
        "            reads.push((read.clone(), hole.part.0, functions.len() - 1));\n",
        "",
    ),
    (
        "an attribute computed from a speculated value is built",
        "cargo",
        SPECULATION,
        # Re-anchored by ADR-0235: a block's subject from the value whole is
        # computed too, and an attribute's alone is refused by name.
        '            if speculates(root) && (what == "an attribute\'s value" || read != root) {\n',
        "            if speculates(root) && read != root {\n",
    ),
    (
        "a field of a value is a function of the same name",
        "cargo",
        EFFECTS,
        "                    if matches!(body.expr(head), Expr::Name(_))\n",
        "                    if false && matches!(body.expr(head), Expr::Name(_))\n",
    ),
    (
        "an opaque value does not compare as its representation",
        "cargo",
        LOWER,
        "            (Type::Nominal(def, instance), true) if rt.as_ref() == Some(&lt) => {\n",
        "            (Type::Nominal(def, instance), true) if false && rt.as_ref() == Some(&lt) => {\n",
    ),
    (
        "a like is not shown before the server answers",
        "browser",
        FEED,
        # Re-anchored by ADR-0276: the feed formatted, its continued value
        # one level in from its clause.
        "    optimistic    Timeline(current_session(), _) as feed => liked(feed, post),\n"
        "        FollowingTimeline(current_session(), _) as feed => liked(feed, post),\n"
        "        Thread(post) as thread => liked_thread(thread, post)\n",
        "",
    ),
    (
        "a like shown before the server answers counts nothing",
        "browser",
        FEED,
        "        Item { id: i.id, author: i.author, text: i.text, likes: i.likes + 1, mine: i.mine, images: i.images }\n",
        "        Item { id: i.id, author: i.author, text: i.text, likes: i.likes, mine: i.mine, images: i.images }\n",
    ),
]

CARGO = [
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core",
        "--test", "computed_rows", "--test", "computed_holes", "--test", "computed_signals",
        "--test", "effects_through_values", "--test", "optimistic_keys",
    ],
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "each_rows_and_sent", "compares_as_its_representation",
    ],
]

SPEC = ["e2e/feed.spec.mjs"]

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


def cargo_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in CARGO:
        out, code = bounded(cmd, cwd=ROOT)
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out or "")
        if code is None:
            failed += 1
            continue
        if not results:
            built = False
            continue
        for p, f in results:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def build():
    """The store's and the feed's pages, with the runtime they load, and the
    server, as the source says."""
    _, page = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                      env={**os.environ, "BUILD_ONLY": "1"})
    _, feed = bounded(["bash", "spikes/own-renderer/feed.sh"], cwd=ROOT)
    _, server = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    return page == 0 and feed == 0 and server == 0


def browser_tests():
    """(built, passed, failed) for the spec in three engines, after a build.
    A spec skipped, as one whose build is stale is, ran nothing: not built."""
    if not build():
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"cargo": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite in sorted({m[1] for m in MUTANTS}):
        built, passed, failed = SUITES[suite]()
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
                built, passed, failed = SUITES[suite]()
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
        # The pages and the server as the source says, whatever a mutant
        # left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
