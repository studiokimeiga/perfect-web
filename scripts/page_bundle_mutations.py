#!/usr/bin/env python3
"""Mutation controls for the page bundle's ADR: a page's code is one module.

Each mutant undoes one piece:
- the packaging: a handler in a block or a view left out, a page's computed
  values or speculations left out, a handler with no module passed over, a
  bundle not minified or not compressed, a build that does not say so;
- the host: the module not named on the table, a file it did not write served,
  a build that does not say so naming one, a delivery that does not digest it;
- the browser: a page with a refused handler asking for it, a failed bundle not
  fallen back on or asked for again, a retry of a piece asking for it again.

A mutant of the packaging must fail `optimize-assets.test.mjs`, of the host
`tests::page_bundle::`, and of the browser `e2e/page-bundle.spec.mjs` in
Chromium.

Run from the repository root; `just e14-page-bundle` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGING = ROOT / "spikes/own-renderer/optimize-assets.mjs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a handler inside a block is left out",
        "packaging",
        PACKAGING,
        "    for (const inner of [part.then, part.otherwise, part.body, part.placeholder, part.ready?.body, part.failed?.body, ...(part.arms ?? []).map((a) => a.body)]) {\n",
        "    for (const inner of []) {\n",
    ),
    (
        "a view's handlers are left out",
        "packaging",
        PACKAGING,
        "      if (view) handlersIn(view.chunks, templates, out, seen);\n",
        "",
    ),
    (
        "a handler named twice is bundled twice",
        "packaging",
        PACKAGING,
        "    if (part.part === \"event\") out.add(part.handler);\n",
        "    if (part.part === \"event\") out.add(`${part.handler}${out.size}`);\n",
    ),
    (
        "a handler with no module is passed over",
        "packaging",
        PACKAGING,
        "        throw new Error(`${page} names handler ${id}, and no module was compiled for it`);\n",
        "        continue;\n",
    ),
    (
        "a page's computed values are left out",
        "packaging",
        PACKAGING,
        "      lines.push(`export * as computed from \"./computed/${page}.mjs\";`);\n",
        "",
    ),
    (
        "a page's speculations are left out",
        "packaging",
        PACKAGING,
        "      lines.push(`export * as speculation from \"./speculations/${page}.mjs\";`);\n",
        "",
    ),
    (
        "a page with no code has a bundle",
        "packaging",
        PACKAGING,
        "    if (lines.length === 0) continue;\n",
        "",
    ),
    (
        "a bundle is not minified",
        "packaging",
        PACKAGING,
        "      minify: true,\n      charset: \"utf8\",\n      legalComments: \"none\",\n",
        "      charset: \"utf8\",\n      legalComments: \"inline\",\n",
    ),
    (
        "a bundle is not compressed with Brotli",
        "packaging",
        PACKAGING,
        "    await writeFile(file, body);\n    await writeFile(file + \".br\", brotliCompressSync(body, BR));\n",
        "    await writeFile(file, body);\n",
    ),
    (
        "a build does not say it is bundled",
        "packaging",
        PACKAGING,
        "export const DELIVERY = { warm: true, versioned: true, bundled: true };\n",
        "export const DELIVERY = { warm: true, versioned: true };\n",
    ),
    (
        "a packaging run bundles nothing",
        "packaging",
        PACKAGING,
        "  await bundlePages(directory);\n",
        "",
    ),
    (
        "the table names no module",
        "host",
        SERVER,
        "                table.push_str(&format!(\"#bundle|{page}|/bundle/{page}.mjs\\n\"));\n",
        "",
    ),
    (
        "a build that does not say so names its modules",
        "host",
        SERVER,
        "    if !delivery_says(dist, \"bundled\") {\n        return Default::default();\n    }\n",
        "",
    ),
    (
        "a file no packaging wrote is served",
        "host",
        SERVER,
        "                .is_some_and(|page| server.bundles.contains(page)) =>\n",
        "                .is_some_and(|_| true) =>\n",
    ),
    (
        "a delivery does not digest its bundles",
        "host",
        SERVER,
        "[\"handlers\", \"computed\", \"speculations\", \"bundles\"]\n",
        "[\"handlers\", \"computed\", \"speculations\"]\n",
    ),
    (
        "a bundle is kept a year at any delivery",
        "host",
        SERVER,
        "                    bytes,\n                    server.delivery.cache_policy(query),\n                ),\n                Err(_) => respond_build(\n                    &mut stream,\n                    404,\n                    \"text/plain; charset=utf-8\",\n                    b\"this module was not bundled\",\n",
        "                    bytes,\n                    \"public, max-age=31536000, immutable\",\n                ),\n                Err(_) => respond_build(\n                    &mut stream,\n                    404,\n                    \"text/plain; charset=utf-8\",\n                    b\"this module was not bundled\",\n",
    ),
    (
        "the table's word is not read",
        "browser",
        RUNTIME,
        "  pageBundleAt = lines.find((l) => l.startsWith(`#bundle|${PAGE}|`))?.split(\"|\")[2] ?? null;\n",
        "",
    ),
    (
        "a page with a refused handler asks for its bundle",
        "browser",
        RUNTIME,
        "        list.every((p) => p.kind !== \"event\" || !p.value || verdictOf(nameOf(template, p), p).attach),\n",
        "        list.every(() => true),\n",
    ),
    (
        "a bundle that fails fails the press",
        "browser",
        RUNTIME,
        "    (all) => all[piece],\n    () => import(url),\n",
        "    (all) => all[piece],\n",
    ),
    (
        "a bundle that failed is asked for again",
        "browser",
        RUNTIME,
        "    pageBundleAt = null;\n    log.push(`the page's module was not loaded: ${error.message ?? error}`);\n",
        "    pageBundleLoading = null;\n    log.push(`the page's module was not loaded: ${error.message ?? error}`);\n",
    ),
    (
        "a press asks for its own module beside the bundle",
        "browser",
        RUNTIME,
        "  if (attempt > 0 || !usesBundle()) return import(url);\n",
        "  return import(url);\n",
    ),
    (
        "a piece the wrong name is asked of the bundle",
        "browser",
        RUNTIME,
        "  const pending = pageCode(`h_${identity}`, url, attempt).then((module) => {\n",
        "  const pending = pageCode(identity, url, attempt).then((module) => {\n",
    ),
]

SPEC = ["e2e/page-bundle.spec.mjs", "--project=chromium"]


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


def packaging():
    out, code = bounded(["node", "--test", "--test-reporter=spec", "spikes/own-renderer/optimize-assets.test.mjs"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    passed = sum(int(n) for n in re.findall(r"(?m)^ℹ pass (\d+)$", out))
    failed = sum(int(n) for n in re.findall(r"(?m)^ℹ fail (\d+)$", out))
    return passed + failed > 0, passed, failed


def host():
    out, code = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "tests::page_bundle::"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def browser():
    built, code = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT, env={**os.environ, "BUILD_ONLY": "1"})
    if code != 0:
        return False, 0, 0
    _, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"], cwd=ROOT / "spikes/own-renderer")
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"packaging": packaging, "host": host, "browser": browser}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run()
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
    # What was built from a mutant is built again from the restored source.
    browser()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
