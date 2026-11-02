#!/usr/bin/env python3
"""Mutation controls for ADR-0157: what a command answers the handler that
called it, and an item sold out since the page was rendered.

Each mutant undoes one piece:
- in the compiler: a command's call typed as its declared result, by the
  checker's typer or by the value relations, a handler answered nothing, an answer that keeps `Ok`'s value, the command sent
  where its answer is decoded (so a decoder that reads nothing drops it), a
  handler that matches on its call named nothing, and each of PW0339's and
  PW0620's three parts;
- in the server: `Ok` sent with its value, a refusal sent without its
  error, every item available, and a kept answer that forgets `null`;
- in the store: the availability check;
- in the browser runtime: the handler given the whole answer.

A compiler, server or store mutant must fail `handlers.rs`, `answers.rs`,
the oracle or the server's tests that see its rule (measured, below). The
runtime's must fail the availability and resource-path browser tests, in
Chromium.

Run from the repository root; `just e14-command-answers` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
INFER = ROOT / "compiler/pw-core/src/infer.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
RESOLVED = ROOT / "compiler/pw-core/src/resolved.rs"
JS = ROOT / "compiler/pw-core/src/backend/js_pure.rs"
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
STORE_DATA = ROOT / "spikes/own-renderer/server/src/store.rs"
STORE = ROOT / "examples/store/app.pw"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a command's call is typed as its declared result",
        "cargo",
        INFER,
        "                if self.sigs.kind_of(sig.definition) == Some(DeclKind::Command) {\n",
        "                if self.sigs.kind_of(sig.definition) == Some(DeclKind::Command) && false {\n",
    ),
    (
        "the value relations type a command's call as its declared result",
        "cargo",
        VALUES,
        "                        if self.sigs.kind_of(sig.definition) == Some(DeclKind::Command) =>\n",
        "                        if self.sigs.kind_of(sig.definition) == Some(DeclKind::Command) && false =>\n",
    ),
    (
        "a handler is answered nothing",
        "cargo",
        LOWER,
        "                .map(crate::resolved::ResolvedType::answered)\n",
        "                .filter(|_| false)\n                .map(crate::resolved::ResolvedType::answered)\n",
    ),
    (
        "the answer keeps Ok's value",
        "cargo",
        RESOLVED,
        "                        vec![unit, args[1].clone()],\n",
        "                        vec![args[0].clone(), args[1].clone()],\n",
    ),
    (
        "the command is sent where its answer is decoded",
        "cargo",
        JS,
        # Re-anchored by ADR-0173: how it is sent again follows its arguments.
        "                let answered = format!(\"{r}_answer\");\n"
        "                self.line(&format!(\n"
        "                    \"const {answered} = await context.command({}, [{}]{how});\",\n",
        "                let answered = format!(\n"
        "                    \"(await context.command({}, [{}]{how}))\",\n"
        "                    json(command),\n"
        "                    sent.join(\", \")\n"
        "                );\n"
        "                self.line(&format!(\n"
        "                    \"// {}, {}: sent where it is decoded\",\n",
    ),
    (
        "a handler that matches on its call is named nothing",
        "cargo",
        TEMPLATE,
        # Re-anchored by ADR-0159: one walk names a match, a binding and a
        # block's call.
        "        Expr::Match { scrutinee, .. } => named_call(body, *scrutinee),\n",
        "        Expr::Match { .. } => None,\n",
    ),
    (
        "PW0339: a command may be called from anywhere",
        "cargo",
        CHECK,
        "            if sent.contains(&e) {\n                continue;\n            }\n",
        "            if sent.contains(&e) || true {\n                continue;\n            }\n",
    ),
    (
        "PW0620: Ok's payload is not looked at",
        "cargo",
        CHECK,
        "        HPat::Ctor { path, args } if path == \"Ok\" || path == \"Result.Ok\" => args\n",
        "        HPat::Ctor { path, args } if (path == \"Ok\" || path == \"Result.Ok\") && false => args\n",
    ),
    (
        "PW0620: an answer bound to a name is not followed",
        "cargo",
        CHECK,
        "                    Some(Binder::Pattern(p)) => bound.get(&p).cloned(),\n",
        "                    Some(Binder::Pattern(p)) => bound.get(&p).cloned().filter(|_| false),\n",
    ),
    (
        "PW0620: a command declaring no Result may be bound",
        "cargo",
        CHECK,
        "                if !declares_result(def) {\n",
        "                if !declares_result(def) && false {\n",
    ),
    (
        "the server sends Ok with its value",
        "cargo",
        SERVER,
        "        Val::Result(Ok(_)) => serde_json::json!({ \"$case\": \"ok\" }),\n",
        "        Val::Result(Ok(_)) => val_to_json(v),\n",
    ),
    (
        "the server sends a refusal without its error",
        "cargo",
        SERVER,
        "                let answered = Answered {\n"
        "                    committed: false,\n"
        "                    result,\n",
        "                let answered = Answered {\n"
        "                    committed: false,\n"
        "                    result: result.filter(|_| false),\n",
    ),
    (
        "every item can be ordered",
        "cargo",
        STORE_DATA,
        # Re-anchored by ADR-0172: an item can be ordered when a store has it
        # and it is not sold out.
        # Re-anchored by track store-pg: the store's operations, built once
        # over either layer's rows.
        "                    r.catalog_item(item)?.is_some_and(|(_, _, can)| can),\n",
        "                    true || r.catalog_item(item)?.is_some_and(|(_, _, can)| can),\n",
    ),
    (
        "a kept answer forgets null",
        "cargo",
        SERVER,
        "            result: object.get(\"result\").cloned(),\n",
        "            result: object.get(\"result\").cloned().filter(|r| !r.is_null()),\n",
    ),
    (
        "the store adds whatever the item",
        "cargo",
        STORE,
        "    if !Menus.is_available(item) {\n",
        "    if false & !Menus.is_available(item) {\n",
    ),
    (
        "the handler is given the whole answer",
        "browser",
        RUNTIME,
        "            return answer.result;\n",
        "            return answer;\n",
    ),
]

CARGO = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "handlers",
     "--test", "answers"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "oracle"],
    # The development server's tests that see the cargo mutants' rules (W6's
    # ruling of 2026-10-09: a mutant that takes long is given the tests that
    # can see its rule), measured by `scripts/mutant_killers.py` on
    # 2026-10-12 against its whole suite. Each of the seven it sees is failed
    # by two of these where two fail at all; the other eight, the compiler's,
    # it does not see, and `handlers`, `answers` and the oracle do. Its whole
    # suite ran for every mutant, and the recipe took 13,000 s of a verify.
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
     "tests::a_build_that_names_no_build_is_not_served",
     "tests::a_kept_answer_is_the_answer_it_kept",
     "tests::a_line_records_the_stores_name_and_price_not_the_requests",
     "tests::a_page_carries_its_data_layers_style_and_no_other",
     "tests::an_item_sold_out_since_the_page_is_refused_by_name"],
]

BROWSER = ["e2e/availability.spec.mjs", "e2e/resource-path.spec.mjs"]


def cargo_tests():
    """(built, passed, failed) over every cargo test command."""
    built, passed, failed = True, 0, 0
    for cmd in CARGO:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", r.stdout + r.stderr)
        if not found:
            built = False
            continue
        for p, f in found:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def browser_tests():
    """(built, passed, failed) for the browser tests, after a build."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    r = subprocess.run(
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--project=chromium",
         "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
        capture_output=True,
        text=True,
    )
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"cargo": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    for suite, run in SUITES.items():
        built, passed, failed = run()
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
        print(f"{what}: {verdict}")
    # The page is built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
