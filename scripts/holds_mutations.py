#!/usr/bin/env python3
"""Mutation controls for ADR-0307: a control is shown where its command's
predicates hold.

Each mutant undoes one piece:
- the checker: a name that holds no value read as one (PW0629), a predicate
  with parameters asked, a handler asking one; a control shown wherever, an
  `{#if}` or a `hidden` asserting nothing, an `{:else}` keeping what its
  condition asserted, a disjunction asserting its sides, a `|refusable`
  control held to the rule (PW5048); a page served to everyone asking its
  reader (PW5049); a predicate's answer no `Bool`;
- the lowering and the plan: a predicate read by no name of its own, an
  attribute computed from an answer reading nothing, a hole or a computed
  value reading an answer refused, the plan naming no predicate; a region a
  speculation renders again not holding the answers;
- the host: no answer given, an answer always true, the document not
  carrying the answer;
- the browser runtime: a region rendered again without the answers.

A checker, lowering or plan mutant must fail
`compiler/pw-core/tests/controls_where_they_hold.rs`; a host or speculation
mutant, the host's test of a control shown where its predicate holds; the
runtime's, the feed's speculated post in Chromium.

Run from the repository root; `just e14-holds` records the output. The source
is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
HOLDS = ROOT / "compiler/pw-core/src/holds.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a name that holds no value is read as one",
        "core",
        HOLDS,
        "                Named::NoValue(what) => out.push(refuse(\n",
        "                Named::NoValue(what) if what.is_empty() => out.push(refuse(\n",
    ),
    (
        "a predicate with parameters is asked",
        "core",
        HOLDS,
        "                Named::Parameterised(_) => out.push(refuse(\n",
        "                Named::Parameterised(_) if false => out.push(refuse(\n",
    ),
    (
        "a handler asks a predicate",
        "core",
        HOLDS,
        "                Named::Asked(_) if in_handler => out.push(refuse(\n",
        "                Named::Asked(_) if false => out.push(refuse(\n",
    ),
    (
        "a control is shown wherever",
        "core",
        HOLDS,
        "                if held.contains(&predicate) {\n",
        "                if true || held.contains(&predicate) {\n",
    ),
    (
        "an `{#if}` asserts nothing",
        "core",
        HOLDS,
        "                here.extend(asked(*s).when_true);\n",
        "                let _ = asked(*s);\n",
    ),
    (
        "a `hidden` asserts nothing",
        "core",
        HOLDS,
        "                    here.extend(asked(*e).when_false);\n",
        "                    let _ = asked(*e);\n",
    ),
    (
        "an `{:else}` keeps what its condition asserted",
        "core",
        HOLDS,
        "                    here = held.clone();\n",
        "",
    ),
    (
        "a disjunction asserts its sides",
        "core",
        HOLDS,
        "                op: BinOp::And,\n                lhs,\n                rhs,\n            } => conjuncts.extend([*lhs, *rhs]),\n",
        "                op: BinOp::Or,\n                lhs,\n                rhs,\n            } => conjuncts.extend([*lhs, *rhs]),\n",
    ),
    (
        "a `|refusable` control is held to the rule",
        "core",
        HOLDS,
        '                    .is_some_and(|(_, modifiers)| !modifiers.contains(&"refusable"))\n',
        "                    .is_some_and(|_| true)\n",
    ),
    (
        "a page served to everyone asks its reader",
        "core",
        HOLDS,
        '            && crate::resume::page_scope(hir, decl) == "public"\n',
        "            && false\n",
    ),
    (
        "a predicate's answer is no `Bool`",
        "core",
        VALUES,
        "            && self.sigs.by_def(d).is_some_and(|s| s.params.is_empty())\n",
        "            && false\n",
    ),
    (
        "a predicate is read by no name of its own",
        "core",
        TEMPLATE,
        "        names: asked_predicates(hirs, ws, sigs, unit),\n",
        "        names: BTreeMap::new(),\n",
    ),
    (
        "an attribute computed from an answer reads nothing",
        "core",
        TEMPLATE,
        '                ctx.signals.contains(n) || ctx.names.get(n).is_some_and(|to| to.ends_with("~holds"))\n',
        "                ctx.signals.contains(n)\n",
    ),
    (
        "a hole that reads an answer is refused",
        "core",
        PLAN,
        '        if let Some(predicate) = root.strip_suffix("~holds") {\n',
        '        if let Some(predicate) = root.strip_suffix("~holds").filter(|_| false) {\n',
    ),
    (
        "a value computed from an answer is refused",
        "core",
        PLAN,
        '    if root.ends_with("~holds") && reads.is_empty() {\n',
        '    if root.ends_with("~holds") && false {\n',
    ),
    (
        "the plan names no predicate",
        "core",
        PLAN,
        "            predicates: asked.into_iter().collect(),\n",
        "            predicates: { let _ = asked; Vec::new() },\n",
    ),
    (
        "a region a speculation renders again does not hold the answers",
        "host",
        SPECULATION,
        '                || root.ends_with("~holds")\n',
        "",
    ),
    (
        "the host gives no answer",
        "host",
        SERVER,
        "                out.insert(asked_name(&predicate), Val::Bool(holds));\n",
        "                let _ = (predicate, holds);\n",
    ),
    (
        "a document's answers are read again at each read",
        "host",
        SERVER,
        "                answers.get(&(session.to_string(), n)).cloned()\n",
        "                answers.get(&(session.to_string(), n)).cloned().filter(|_| false)\n",
    ),
    (
        "an answer is always true",
        "host",
        SERVER,
        "                            self.identity.answers(predicate, session)?,\n",
        "                            { let _ = self.identity.answers(predicate, session)?; true },\n",
    ),
    (
        "the document does not carry the answer",
        "host",
        SERVER,
        '            manifest["holds"] = serde_json::json!(holds);\n',
        "            let _ = holds;\n",
    ),
    (
        "a region the browser renders again does not read the answers",
        "browser",
        RUNTIME,
        "  return renderer.env([[binding, value], ...computed, ...params, ...holds, ...signals]);\n",
        "  return renderer.env([[binding, value], ...computed, ...params, ...signals]);\n",
    ),
]

CARGO = {
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "controls_where_they_hold"],
    "host": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "a_control_is_shown_where_its_commands_predicate_holds",
        "a_documents_answers_are_its_own_for_its_life",
    ],
}
SPECS = [
    ["e2e/feed.spec.mjs", "-g", "a post shown before the server answers waits", "--project=chromium"],
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
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def browser_tests(_suite="browser"):
    """(built, passed, failed), after a build: the store's pages and the
    feed's, each of which serves the runtime it was built with (the suite's
    servers start both), and the server the suite runs."""
    for script in ["run.sh", "feed.sh"]:
        built = subprocess.run(
            ["bash", f"spikes/own-renderer/{script}"],
            cwd=ROOT,
            env={**os.environ, "BUILD_ONLY": "1"},
            capture_output=True,
            text=True,
        )
        if built.returncode != 0:
            return False, 0, 0
    # The servers the suite starts: the development server and kiokun's.
    server = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
        return False, 0, 0
    passed = failed = 0
    for spec in SPECS:
        out, code = bounded(
            ["pnpm", "exec", "playwright", "test", *spec, "--reporter=line"],
            cwd=ROOT / "spikes/own-renderer",
        )
        if code is None:
            failed += 1
            continue
        out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
        passed += sum(int(n) for n in re.findall(r"(\d+) passed", out))
        failed += sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {
    "core": cargo_tests,
    "host": cargo_tests,
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
    # The pages and the server are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
