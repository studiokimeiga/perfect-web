#!/usr/bin/env python3
"""Mutation controls for ADR-0163: a page says when it is absent.

Each mutant undoes one piece:
- the checker (PW0342): the clause held to pages, its value read as
  `Type.Case` of a type, the case the type's, and a query the page reads
  answering it with a failure of that type;
- the plan: the case carried by its WIT name, on the bindings whose query
  can answer it, and written where a host reads it;
- the parser: the clause a policy it knows;
- the development server: a binding's declared case told from any other
  failure, carried up as its own kind, and answered 404, with its reason;
- the not-found page: a heading a reader can find.

A checker, plan or parser mutant must fail `compiler/pw-core/tests/
not_found.rs`; a server mutant, and the plan's written field, the server's
tests that see them (measured, below); the not-found page's,
`e2e/stores.spec.mjs` in Chromium.

Run from the repository root; `just e14-not-found` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
ROUTES = ROOT / "compiler/pw-core/src/routes.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "the clause is let be on a declaration that is no page",
        "compiler",
        CHECK,
        "        if decl.kind != DeclKind::Page {\n"
        "            out.push(refused(\n"
        "                format!(\n"
        "                    \"`not_found_on {value}` is a page's: `{}` is not one\",\n",
        "        if false && decl.kind != DeclKind::Page {\n"
        "            out.push(refused(\n"
        "                format!(\n"
        "                    \"`not_found_on {value}` is a page's: `{}` is not one\",\n",
    ),
    (
        "the type is looked for among terms",
        "compiler",
        ROUTES,
        "    let def = match ws.resolve_path_in(unit, Namespace::Type, ty) {\n",
        "    let def = match ws.resolve_path_in(unit, Namespace::Term, ty) {\n",
    ),
    (
        "a module's name is taken for the type",
        "compiler",
        ROUTES,
        "    let Some((ty, case)) = value.rsplit_once('.') else {\n",
        "    let Some((ty, case)) = value.split_once('.') else {\n",
    ),
    (
        "any case is the type's",
        "compiler",
        ROUTES,
        "        .is_some_and(|cases| cases.iter().any(|(c, _)| c == case));\n",
        "        .is_some_and(|_| true);\n",
    ),
    (
        "no query need answer it",
        "compiler",
        CHECK,
        "            .any(|(_, query, _)| crate::routes::error_of(sigs, *query) == Some(ty));\n",
        "            .any(|_| true);\n",
    ),
    (
        "a query's value is taken for its error",
        "compiler",
        ROUTES,
        "    result.args().get(1)?.def_id()\n",
        "    result.args().first()?.def_id()\n",
    ),
    (
        "a query that answers no Result is taken to fail",
        "compiler",
        ROUTES,
        "    if result.as_builtin() != Some(crate::resolved::Builtin::Result) {\n",
        "    if false {\n",
    ),
    (
        "the plan names the case as Pleris spells it",
        "compiler",
        PLAN,
        "                    vec![crate::wit::ident(case)]\n",
        "                    vec![case.clone()]\n",
    ),
    (
        "the plan marks every binding",
        "compiler",
        PLAN,
        "                Some((ty, case)) if crate::routes::error_of(sigs, *resource) == Some(*ty) => {\n",
        "                Some((_, case)) => {\n",
    ),
    (
        "the parser does not know the clause",
        "compiler",
        GRAMMAR,
        "    // ADR-0163: the declared error that means a page's address names\n"
        "    // nothing, answered 404 rather than 503.\n"
        "    \"not_found_on\",\n",
        "",
    ),
    (
        "the plan does not write the case",
        "server",
        PLAN,
        "    #[serde(default, skip_serializing_if = \"Vec::is_empty\")]\n"
        "    pub not_found: Vec<String>,\n",
        "    #[serde(skip)]\n"
        "    pub not_found: Vec<String>,\n",
    ),
    (
        "a declared case is a failure like any other",
        "server",
        SERVER,
        "                .is_some_and(|cases| cases.iter().any(|c| c == case.as_str()))\n",
        "                .is_some_and(|_| false)\n",
    ),
    (
        "any declared error is not found",
        "server",
        SERVER,
        "                .is_some_and(|cases| cases.iter().any(|c| c == case.as_str()))\n",
        "                .is_none_or(|_| true)\n",
    ),
    (
        "the failure loses its kind on the way up",
        "server",
        SERVER,
        # Re-anchored by ADR-0190: a failure names its page.
        "            .map_err(|e| e.of(&format!(\"`{}`'s queries\", template.name)))?;\n",
        "            .map_err(|e| Unread::Failed(e.to_string()))?;\n",
    ),
    (
        "a page that is not found is answered as one that failed",
        "server",
        SERVER,
        "            Unread::NotFound(_) => respond(\n"
        "                stream,\n"
        "                404,\n",
        "            Unread::NotFound(_) => respond(\n"
        "                stream,\n"
        "                503,\n",
    ),
    (
        "every response says OK",
        "server",
        SERVER,
        "        reason(code),\n        body.len()\n",
        "        \"OK\",\n        body.len()\n",
    ),
    (
        "the not-found page has no heading",
        "browser",
        SERVER,
        "<h1>Not found</h1>",
        "<p>Not found</p>",
    ),
]

SUITES_RUN = {
    "compiler": [["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "not_found"]],
    # The development server's tests that see its mutants' rules (W6's ruling
    # of 2026-10-09: a mutant that takes long is given the tests that can see
    # its rule), measured by `scripts/mutant_killers.py` on 2026-10-12 against
    # its whole suite: each of the six is failed by two of these, of the 5 to
    # 11 that fail under it. The whole suite ran for every server mutant, and
    # the recipe took 8,422 s of a verify.
    "server": [[
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_store_that_is_not_there_is_not_found",
        "tests::redirects::a_page_at_another_address_of_the_page_is_moved_there_for_good",
    ]],
}
BROWSER = ["e2e/stores.spec.mjs"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
# 1800 from 2026-10-12 (track store-accounts) while these mutants ran the
# server's whole suite, 941 to 961 s on CI's runner at 420 tests; they run
# the tests that see them since (above).
BOUND = 900


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
    """(built, passed, failed) over the suite's cargo test commands."""
    built, passed, failed = True, 0, 0
    for cmd in SUITES_RUN[suite]:
        out, code = bounded(cmd, cwd=ROOT)
        if code is None:
            return True, passed, failed + 1
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            built = False
            continue
        for p, f in found:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def browser_tests(_suite="browser"):
    """(built, passed, failed) for the browser tests, after a build: the page,
    and the server the suite runs, which `run.sh` does not build."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    server = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--project=chromium",
         "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"compiler": cargo_tests, "server": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed")
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
        print(f"{what} [{suite}]: {verdict}")
    # The page and the server are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
