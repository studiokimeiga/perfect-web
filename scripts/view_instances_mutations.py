#!/usr/bin/env python3
"""Mutation controls for ADR-0203: a view that contains itself is an instance
of its own template, made at run time.

Each mutant undoes one piece:
- the checker: which views contain themselves endlessly, the block on the
  way back that ends one, and that one holds no signal and shows no stream;
- the template: each use an instance, how deep it is used and how deep its
  view nests, and a schema over every view its instances reach;
- the plan: an instance a query gives, rendered again by the host, and one
  the page's signals give, by the browser, given nothing else;
- the renderer: each instance after the markup around it and not on its
  stack, inside its own frame, its own token, the page's values not its own,
  the first error in the document, and the depth a browser nests;
- the views a page sends, and the in-browser renderer reading them;
- the browser: an instance's parts read in its own template, its range, and
  a range paired by its path as well as its id.

A compiler or renderer mutant must fail `views_contain_themselves.rs`, the
renderer's `properties.rs` or the in-browser renderer's tests. A runtime
mutant must fail `e2e/thread.spec.mjs` in Chromium against the staged
build; a server or in-browser renderer mutant the same, after the page and
the server are built again.

Run from the repository root; `just e14-view-instances` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
IR = ROOT / "compiler/pw-core/src/template_ir.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
RENDER = ROOT / "runtime/pw-render/src/lib.rs"
WASM = ROOT / "runtime/pw-render-wasm/src/lib.rs"
SPIKE = ROOT / "spikes/own-renderer"
RUNTIME = SPIKE / "public/pw-runtime.mjs"
STAGED = SPIKE / "dist/pw-runtime.mjs"
SERVER = SPIKE / "server/src/main.rs"

# (what, which tests, file, anchor, replacement)
MUTANTS = [
    (
        "a view with no block on the way back to it is composed",
        "cargo",
        CHECK,
        "    if crate::template_ir::contains_itself_endlessly(workspace, hirs, def) {\n",
        "    if false && crate::template_ir::contains_itself_endlessly(workspace, hirs, def) {\n",
    ),
    (
        "a block on the way back to a view does not end it",
        "cargo",
        IR,
        "            Node::Block { children, .. } => children.iter().map(|c| (*c, true)).collect(),\n",
        "            Node::Block { children, .. } => children.iter().map(|c| (*c, in_block)).collect(),\n",
    ),
    (
        "a view that contains itself holds a signal",
        "cargo",
        CHECK,
        "        && !(crate::page_values::signals_of(body).is_empty()\n",
        "        && false && !(crate::page_values::signals_of(body).is_empty()\n",
    ),
    (
        "a view that contains itself shows a stream",
        "cargo",
        CHECK,
        "        && crate::template_ir::shows_a_stream(body)\n",
        "        && false && crate::template_ir::shows_a_stream(body)\n",
    ),
    (
        "a view that contains itself is composed in place",
        "cargo",
        IR,
        "    if contains_itself(ctx.ws, ctx.hirs, def) {\n",
        "    if false && contains_itself(ctx.ws, ctx.hirs, def) {\n",
    ),
    (
        "an instance is used at no depth",
        "cargo",
        IR,
        "            elements: ix.elements,\n            deepest: 0,\n",
        "            elements: 0,\n            deepest: 0,\n",
    ),
    (
        "a template nests one element less than it does",
        "cargo",
        IR,
        "    ix.deepest = ix.deepest.max(ix.elements + 1);\n",
        "    ix.deepest = ix.deepest.max(ix.elements);\n",
    ),
    (
        "an instance is not told how deep its view nests",
        "cargo",
        IR,
        "            *d = deepest.get(path.as_str()).copied().unwrap_or_default();\n",
        "            *d = 0;\n",
    ),
    (
        "a schema is its template's alone",
        "cargo",
        IR,
        "    close_schemas(&mut out);\n",
        "",
    ),
    (
        "a schema closes over the views used directly alone",
        "cargo",
        IR,
        "                stack.extend(reaches.get(p).into_iter().flatten());\n",
        "",
    ),
    (
        "an instance a query gives is not rendered again",
        "cargo",
        PLAN,
        "            })\n            && template.chunks.iter().any(\n"
        "                |c| matches!(c, crate::template_ir::Chunk::Dynamic(p) if p.id() == Some(entry.id)),\n"
        "            )\n        {\n            blocks.push(entry.id.0);\n",
        "            })\n            && template.chunks.iter().any(\n"
        "                |c| matches!(c, crate::template_ir::Chunk::Dynamic(p) if p.id() == Some(entry.id)),\n"
        "            )\n        {\n            let _ = entry.id.0;\n",
    ),
    (
        "an instance a signal gives is not rendered again",
        "cargo",
        PLAN,
        "            live.push(Live {\n                part: entry.id.0,\n"
        "                signal: root.to_string(),\n                path: String::new(),\n",
        "            let _ = Live {\n                part: entry.id.0,\n"
        "                signal: root.to_string(),\n                path: String::new(),\n",
    ),
    (
        "an instance given a signal and a query's value is planned",
        "cargo",
        PLAN,
        "                            if let Some(other) = own.iter().find(|v| !signal(v)) {\n",
        "                            if let Some(other) = own.iter().find(|_| false) {\n",
    ),
    (
        "an instance is rendered on the stack of the one around it",
        "cargo",
        RENDER,
        "            let at = out.len();\n            out.later.push(Later {\n"
        "                at,\n                chunks: &t.chunks,\n                env: child,\n            });\n",
        "            emit(&t.chunks, &child, others, out)?;\n",
    ),
    (
        "an instance's markup is written after its frame",
        "cargo",
        RENDER,
        "            let at = out.len();\n",
        "            let at = out.len() + format!(\"<!--pw:e{id}@{token}-->\").len();\n",
    ),
    (
        "an instance past what a browser nests is rendered",
        "cargo",
        RENDER,
        "            if root + deepest > NESTED_ELEMENTS {\n",
        "            if false && root + deepest > NESTED_ELEMENTS {\n",
    ),
    (
        "an instance reads the values around it",
        "cargo",
        RENDER,
        "            values: BTreeMap::new(),\n",
        "            values: self.values.clone(),\n",
    ),
    (
        "each instance of a part is one token",
        "cargo",
        RENDER,
        "            let token = env.domain.instance_token(&env.path, *id, path);\n",
        "            let token = env.domain.instance_token(&Default::default(), *id, path);\n",
    ),
    (
        "the markup around an instance fails first",
        "cargo",
        RENDER,
        "    let mut open = vec![Open::new(out, failed)];\n",
        "    if let Some(e) = failed {\n        return Err(e);\n    }\n"
        "    let mut open = vec![Open::new(out, None)];\n",
    ),
    (
        "a view reached through another is not sent",
        "cargo",
        RENDER,
        "            walk(&found.chunks, &mut paths);\n",
        "",
    ),
    (
        "the in-browser renderer reads no view's template",
        "cargo",
        WASM,
        "    let templates = match request.get(\"templates\") {\n",
        "    let templates = match None::<&serde_json::Value> {\n",
    ),
    (
        "an instance's parts are read in the page's template",
        "runtime",
        RUNTIME,
        "        within.push(view ? part.value : template);\n",
        "        within.push(template);\n",
    ),
    (
        "an instance's range is not indexed",
        "runtime",
        RUNTIME,
        "        if (view) open.set(",
        "        if (false) open.set(",
    ),
    (
        "a range is paired by its id alone",
        "runtime",
        RUNTIME,
        "    const key = `${at()}|${id}`;\n",
        "    const key = `|${id}`;\n",
    ),
    (
        "a request to the in-browser renderer carries no view",
        "runtime",
        RUNTIME,
        "const TEMPLATES_JSON = TEMPLATES.length ? JSON.stringify(TEMPLATES) : \"\";\n",
        "const TEMPLATES_JSON = \"\";\n",
    ),
    (
        "a page is sent without the views its instances reach",
        "build",
        SERVER,
        '    if !reached.is_empty() {\n        manifest["templates"] = serde_json::Value::Object(reached);\n',
        '    if false && !reached.is_empty() {\n        manifest["templates"] = serde_json::Value::Object(reached);\n',
    ),
    (
        "an instance the page's signals give is not sent to be rendered again",
        "build",
        SERVER,
        "            Some(\"conditional\" | \"match\" | \"instance\")\n",
        "            Some(\"conditional\" | \"match\")\n",
    ),
]

CARGO = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "views_contain_themselves"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-render", "--test", "properties"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-render-wasm"],
]

BROWSER = [
    "pnpm", "exec", "playwright", "test", "e2e/thread.spec.mjs",
    "--project=chromium", "--reporter=line",
]


def cargo_tests():
    """(built, passed, failed) over every cargo test command. A test binary
    that aborts, as one overflowing its stack does, is a failure."""
    built, passed, failed = True, 0, 0
    for cmd in CARGO:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            if "error[" in out or "could not compile" in out:
                built = False
            else:
                failed += 1
            continue
        for p, f in found:
            passed += int(p)
            failed += int(f)
        if r.returncode != 0 and not any(int(f) for _, f in found):
            failed += 1
    return built, passed, failed


def playwright():
    r = subprocess.run(BROWSER, cwd=SPIKE, capture_output=True, text=True)
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
    passed = re.search(r"^\s+(\d+) passed", out, re.M)
    failed = re.search(r"^\s+(\d+) failed", out, re.M)
    if passed is None and failed is None:
        return False, 0, 0
    return True, int(passed.group(1)) if passed else 0, int(failed.group(1)) if failed else 0


def runtime_tests():
    """(ran, passed, failed) of the browser's spec, against the staged runtime."""
    shutil.copyfile(RUNTIME, STAGED)
    return playwright()


def build_tests():
    """(built, passed, failed) of the browser's spec, after the page, the
    in-browser renderer and the server are built again."""
    env = {**os.environ, "BUILD_ONLY": "1"}
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"], cwd=ROOT, env=env, capture_output=True, text=True
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
    return playwright()


SUITES = {"cargo": cargo_tests, "runtime": runtime_tests, "build": build_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    if not STAGED.exists():
        print("FAIL: no staged build; run `BUILD_ONLY=1 bash spikes/own-renderer/run.sh`")
        return 1
    for suite in ("cargo", "build"):
        built, passed, failed = SUITES[suite]()
        print(f"baseline ({suite}): {passed} passed, {failed} failed")
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    rebuilt = False
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
            if path == RUNTIME:
                shutil.copyfile(RUNTIME, STAGED)
        rebuilt = rebuilt or suite == "build"
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}")
    # The page and the server, built again from the restored source.
    if rebuilt:
        build_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
