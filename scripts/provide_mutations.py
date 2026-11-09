#!/usr/bin/env python3
"""Mutation controls for ADR-0144: a view's own signals, and a signal a page
provides to the views it composes.

Each mutant undoes one part:
- a module's `signal`, a `provide`, and what may assign the signal;
- each use of a view holding its own instance, the instance a provided
  signal is where a view is used, and the element saying which instance a
  handler's signal is;
- the blocks that own an instance, and the first values the plan holds;
- the rules: a provider for each need (PW5305), one `provide` (PW5306), no
  view holding a signal in a loop's row (PW5307), and a module signal read
  and changed where a page's is;
- the browser reading and setting each handler's instance, and starting a
  block's instances again when it shows another arm;
- the Marko adapter refusing what it does not model, rather than dropping it.

A compiler mutant must fail `labels.rs`'s neighbour, `provide.rs`. A browser
mutant must fail `e2e/provide.spec.mjs`, in Chromium, against the staged
build: run `BUILD_ONLY=1 bash spikes/own-renderer/run.sh` first.

Run from the repository root; `just e14-provide` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORE = ROOT / "compiler/pw-core/src"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
LOWER = CORE / "lower.rs"
NAMES = CORE / "names.rs"
IR = CORE / "template_ir.rs"
PLAN = CORE / "page_values.rs"
FIRST = CORE / "backend/signals.rs"
BACKEND = CORE / "backend/lower.rs"
SIGNATURES = CORE / "signatures.rs"
SIGNALS = CORE / "signals.rs"
MARKO = CORE / "marko.rs"
SPIKE = ROOT / "spikes/own-renderer"
RUNTIME = SPIKE / "public/pw-runtime.mjs"
STAGED = SPIKE / "dist/pw-runtime.mjs"

# (what, which tests, file, anchor, replacement)
MUTANTS = [
    (
        "a module's `signal` is not a declaration",
        "compiler",
        GRAMMAR,
        "        if self.at_kw(\"signal\") && self.nth_is(1, Kind::Ident) {\n            self.start(K::LetDecl);",
        "        if false && self.at_kw(\"signal\") && self.nth_is(1, Kind::Ident) {\n            self.start(K::LetDecl);",
    ),
    (
        "a module's `signal` lowers as a `let`",
        "compiler",
        LOWER,
        "                .is_some_and(|t| t.text() == \"signal\") =>",
        "                .is_some_and(|t| t.text() == \"signal \") =>",
    ),
    (
        "a `provide` is not marked",
        "compiler",
        LOWER,
        "            b.provides.insert(id);\n",
        "            let _ = &b.provides;\n",
    ),
    (
        "an imported signal is not assigned",
        "compiler",
        NAMES,
        "                declared.insert(name.clone());\n                module_mutable.insert(name.clone());\n",
        "                declared.insert(name.clone());\n",
    ),
    (
        "each use of a view shares one instance",
        "compiler",
        IR,
        "    for (name, init, declared) in own {\n        let Expr::Let { ty: Some(t), .. } = view.expr(declared) else {\n            continue;\n        };\n        let fresh = ix.rename(&name);",
        "    for (name, init, declared) in own {\n        let Expr::Let { ty: Some(t), .. } = view.expr(declared) else {\n            continue;\n        };\n        let fresh = name.clone();",
    ),
    (
        "a view's `provide` gives nothing to what it contains",
        "compiler",
        IR,
        "        names.insert(name.clone(), fresh.clone());\n"
        "        provided.insert(signal, fresh);\n"
        "        signals.insert(name);\n"
        "    }\n"
        "    // A signal provided around it is the nearest `provide`'s instance.",
        "        names.insert(name.clone(), fresh.clone());\n"
        "        let _ = (&mut provided, signal, fresh);\n"
        "        signals.insert(name);\n"
        "    }\n"
        "    // A signal provided around it is the nearest `provide`'s instance.",
    ),
    (
        "an element does not say which instance a handler's signal is",
        "compiler",
        IR,
        "    if !instances.is_empty() {\n        let json = serde_json::to_string(&instances)",
        "    if false && !instances.is_empty() {\n        let json = serde_json::to_string(&instances)",
    ),
    (
        "a view's instance is inside no block",
        "compiler",
        IR,
        "            ty: InstanceType::Written(*t),\n            within: ix.blocks.clone(),\n        });\n        names.insert(name.clone(), fresh);\n",
        "            ty: InstanceType::Written(*t),\n            within: Vec::new(),\n        });\n        names.insert(name.clone(), fresh);\n",
    ),
    (
        "an `{:else if}` arm is no block of its own",
        "compiler",
        IR,
        # Re-anchored by ADR-0229, which reads the arm's subject first.
        "                ix.blocks.push(nested);\n                let chunk = conditional(body, nested, v, run, more, ctx, ix);\n                ix.blocks.pop();\n",
        "                let chunk = conditional(body, nested, v, run, more, ctx, ix);\n",
    ),
    (
        "a block owns nothing",
        "compiler",
        PLAN,
        # Re-anchored by ADR-0229, whose computed subject's block owns too.
        # And by track `device-store`, whose block reads what a value computed
        # inside it reads.
        "                reads: block_reads(part, &signals, &computed_inputs),\n                attribute: String::new(),\n                owns: owned(entry.id.0),\n",
        "                reads: block_reads(part, &signals, &computed_inputs),\n                attribute: String::new(),\n                owns: Vec::new(),\n",
    ),
    (
        "the plan holds the page's own signals alone",
        "compiler",
        PLAN,
        "    let signals: Vec<String> = instances.iter().map(|i| i.name.clone()).collect();\n",
        "    let signals: Vec<String> = signals_of(body).into_iter().map(|(n, ..)| n).collect();\n",
    ),
    (
        "a view's instance has no first value",
        "compiler",
        FIRST,
        "            for instance in lowered.instances {\n",
        "            for instance in lowered.instances.into_iter().filter(|i| i.origin.1 == id) {\n",
    ),
    (
        "a handler does not reach a provided signal",
        "compiler",
        BACKEND,
        "        f.signals.insert(name, ty);\n    }\n    let (mut captured, mut paths) = (Vec::new(), Vec::new());\n",
        "        let _ = (name, ty);\n    }\n    let (mut captured, mut paths) = (Vec::new(), Vec::new());\n",
    ),
    (
        "a module signal has no type",
        "compiler",
        SIGNATURES,
        "                if decl.kind == DeclKind::Signal\n                    && let Some(written) = &decl.ret\n",
        "                if decl.kind == DeclKind::Opaque\n                    && let Some(written) = &decl.ret\n",
    ),
    (
        "a view's need reaches no page",
        "compiler",
        SIGNALS,
        "    for (node, view, tag, _) in &uses {\n        let Some(needs) = provision.needs.get(view) else {\n",
        "    for (node, view, tag, _) in uses.iter().take(0) {\n        let Some(needs) = provision.needs.get(view) else {\n",
    ),
    (
        "a need does not pass through a view",
        "compiler",
        SIGNALS,
        "        for (signal, through) in inner {\n            if gives.contains(&signal) {\n",
        "        for (signal, through) in inner.into_iter().take(0) {\n            if gives.contains(&signal) {\n",
    ),
    (
        "a view in between that provides it is not heard",
        "compiler",
        SIGNALS,
        "        for (signal, through) in inner {\n            if gives.contains(&signal) {\n                continue;\n            }\n",
        "        for (signal, through) in inner {\n            if gives.contains(&signal) && false {\n                continue;\n            }\n",
    ),
    (
        "a signal provided twice in one body is accepted",
        "compiler",
        SIGNALS,
        "        if let Some(first) = given.get(&signal) {\n",
        "        if let Some(first) = given.get(&signal).filter(|_| false) {\n",
    ),
    (
        "a view holding a signal is used in a row",
        "compiler",
        SIGNALS,
        "        if *in_row && provision.holds.contains(view) {\n",
        "        if *in_row && provision.holds.contains(view) && false {\n",
    ),
    (
        "a view holds what the views it composes hold",
        "compiler",
        SIGNALS,
        "        holds |= p.holds.contains(&view);\n",
        "        holds |= false && p.holds.contains(&view);\n",
    ),
    (
        "a module signal is changed outside a handler",
        "compiler",
        SIGNALS,
        "                    if !body.provides.contains(&id)\n                        && module(target).is_some()\n",
        "                    if !body.provides.contains(&id)\n                        && module(target).is_some()\n                        && false\n",
    ),
    (
        "a module signal is read where it cannot change",
        "compiler",
        SIGNALS,
        "                    None if module(id).is_some() => None,\n",
        "                    None if module(id).is_some() && false => None,\n",
    ),
    (
        "a module signal of any type is bound",
        "compiler",
        SIGNALS,
        "                if !declared.is_some_and(|t| t.as_primitive() == Some(Primitive::Str)) {\n",
        "                if !declared.is_some_and(|t| t.as_primitive() == Some(Primitive::Str)) && false {\n",
    ),
    (
        "a dialog a module signal shows is shown by nothing",
        "compiler",
        SIGNALS,
        "                Expr::Name(n) if module(root) => Some(n.clone()),\n",
        "                Expr::Name(n) if module(root) && false => Some(n.clone()),\n",
    ),
    (
        "the Marko adapter drops a `provide`",
        "compiler",
        MARKO,
        "    if !body.provides.is_empty() {\n        return Err(\"a `provide` (ADR-0144) is not modelled yet\".to_string());\n",
        "    if false && !body.provides.is_empty() {\n        return Err(\"a `provide` (ADR-0144) is not modelled yet\".to_string());\n",
    ),
    (
        "the Marko adapter renders a signal a page provides",
        "compiler",
        MARKO,
        "        Expr::Name(n) if signals.contains(n) => Some(n.clone()),\n",
        "        Expr::Name(n) if signals.contains(n) && false => Some(n.clone()),\n",
    ),
    (
        "the browser reads and sets a handler's signal by its own name",
        "browser",
        RUNTIME,
        "  const instance = (name) => instances[name] ?? name;\n",
        "  const instance = (name) => name;\n",
    ),
    (
        "a block's instances go on when it shows another arm",
        "browser",
        RUNTIME,
        "          for (const s of live.owns) signals.set(s, firstValues.get(s));\n",
        "          for (const s of []) signals.set(s, firstValues.get(s));\n",
    ),
    (
        "a block's instances start again at no value",
        "browser",
        RUNTIME,
        "          for (const s of live.owns) signals.set(s, firstValues.get(s));\n",
        "          for (const s of live.owns) signals.set(s, undefined);\n",
    ),
]

COMPILER = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "provide"],
]
BROWSER = [
    "pnpm", "exec", "playwright", "test", "e2e/provide.spec.mjs",
    "--project=chromium", "--reporter=line",
]


def run_compiler():
    """(built, passed, failed) over the compiler's tests."""
    built, passed, failed = True, 0, 0
    for cmd in COMPILER:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if m is None:
            built = False
            continue
        passed += int(m.group(1))
        failed += int(m.group(2))
    return built, passed, failed


def run_browser():
    """(ran, passed, failed) of the browser's spec, against the staged runtime."""
    shutil.copyfile(RUNTIME, STAGED)
    r = subprocess.run(BROWSER, cwd=SPIKE, capture_output=True, text=True)
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
    passed = re.search(r"^\s+(\d+) passed", out, re.M)
    failed = re.search(r"^\s+(\d+) failed", out, re.M)
    if passed is None and failed is None:
        return False, 0, 0
    return True, int(passed.group(1)) if passed else 0, int(failed.group(1)) if failed else 0


RUNS = {"compiler": run_compiler, "browser": run_browser}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    if not STAGED.exists():
        print("FAIL: no staged build; run `BUILD_ONLY=1 bash spikes/own-renderer/run.sh`")
        return 1
    for kind, run in RUNS.items():
        built, passed, failed = run()
        print(f"baseline ({kind}): {passed} passed, {failed} failed")
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, kind, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = RUNS[kind]()
        finally:
            path.write_text(original)
            if path == RUNTIME:
                shutil.copyfile(RUNTIME, STAGED)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{kind}]: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
