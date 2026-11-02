#!/usr/bin/env python3
"""Mutation controls for ADR-0382: the modules a press needs are loaded
within the deadline, and a load that failed is not kept.

Each mutant undoes one piece: a handler's module, or the page's speculation
module, waited for without a deadline; a failed speculation load kept, or
asked for again under the same specifier, which the browser keeps failed; a
failed renderer load kept; a press whose speculation cannot load failed
rather than sent unspeculated; a renderer load a press gave up on kept; a
module's load timed by a send's deadline; a page's computed values kept
failed, or asked for again under the same specifier.

`e2e/press-modules.spec.mjs` and `e2e/computed-modules.spec.mjs`, in
Chromium, must fail for each. Only complete
native results earn a kill: a build that fails, a producer that times out, a
receipt that is missing or forged, or an inventory that differs from the
baseline's is REFUSED, never a kill.

Run from the repository root; `just e14-press-modules` records the output.
The source is restored after every mutant, whatever happens.
"""

import json
import os
import pathlib
import secrets
import signal
import subprocess
import sys
import tempfile

import browser_test_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a handler's module is waited for without a deadline",
        RUNTIME,
        "  const pending = within(import(url), `the handler ${identity}`).then((module) => {\n",
        "  const pending = import(url).then((module) => {\n",
    ),
    (
        "the speculation module is waited for without a deadline",
        RUNTIME,
        "  const loading = within(\n"
        "    Promise.all([speculationLoaded ?? import(url), trackedRenderer]),\n"
        '    "the speculation module",\n'
        "  ).then(\n",
        "  const loading = Promise.all([speculationLoaded ?? import(url), trackedRenderer]).then(\n",
    ),
    (
        "a failed speculation load is kept",
        RUNTIME,
        "      if (speculationModule !== loading) throw error;\n"
        "      speculationModule = null;\n",
        "      if (speculationModule !== loading) throw error;\n",
    ),
    (
        "a failed speculation load is asked for again under the same specifier",
        RUNTIME,
        "      speculationAttempts += 1;\n",
        "",
    ),
    (
        "a failed renderer load is kept",
        RUNTIME,
        "        if (renderer === loading) renderer = null;\n",
        "",
    ),
    (
        "a renderer load a press gave up on is kept",
        RUNTIME,
        "      forgetRenderer(rendering);\n",
        "",
    ),
    (
        "a module's load is timed by a send's deadline",
        RUNTIME,
        "    timer = setTimeout(() => reject(new Error(`${what}: none within ${loadDeadline} ms`)), loadDeadline);\n",
        "    timer = setTimeout(() => reject(new Error(`${what}: none within ${sendDeadline} ms`)), sendDeadline);\n",
    ),
    (
        "a failed computed values load is kept",
        RUNTIME,
        "      computedModule = null;\n",
        "",
    ),
    (
        "a failed computed values load is asked for again under the same specifier",
        RUNTIME,
        "      computedAttempts += 1;\n",
        "",
    ),
    (
        "a press whose speculation cannot load fails",
        RUNTIME,
        "  let module;\n"
        "  try {\n"
        "    module = await speculations();\n"
        "  } catch (error) {\n"
        "    if (error instanceof RetiredRenderer) throw error;\n"
        "    log.push(`${component} sent unspeculated: ${error.message ?? error}`);\n"
        "    return [];\n"
        "  }\n",
        "  const module = await speculations();\n",
    ),
]

SPEC = ["e2e/press-modules.spec.mjs", "e2e/computed-modules.spec.mjs"]
BOUND = 1200
INVENTORIES = {}


def bounded(cmd, **kw):
    """Shared native process ownership; timeout is invalid, never a test failure."""
    try:
        result = producer_process.run(cmd, timeout=BOUND, **kw)
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired as expired:
        return expired.output, None


def build():
    """Build the actual source; refusal cannot earn a mutation kill."""
    for command, env in (
        (["bash", "spikes/own-renderer/run.sh"], {"BUILD_ONLY": "1"}),
        (["bash", "spikes/own-renderer/feed.sh"], {}),
        (["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], {}),
    ):
        out, code = bounded(command, cwd=ROOT, env={**os.environ, **env})
        if code != 0:
            print(out, flush=True)
            return False
    return True


def browser_tests():
    if not build():
        return False, 0, 0
    with tempfile.TemporaryDirectory(prefix="pw-press-modules-") as directory:
        receipt = pathlib.Path(directory) / "result.json"
        nonce = secrets.token_hex(32)
        out, code = bounded(
            ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",
             "pnpm", "exec", "playwright", "test", *SPEC, "--project=chromium", "--retries=0",
             "--reporter=line,./assertion-result-reporter.mjs"],
            cwd=ROOT / "spikes/own-renderer",
            env={**os.environ, "PW_ASSERTION_REPORT": str(receipt), "PW_ASSERTION_NONCE": nonce},
        )
        try:
            report = json.loads(receipt.read_text())
        except (OSError, ValueError):
            report = None
        result = browser_test_results.parse(code, report, nonce, INVENTORIES.get("browser"))
        if not result.valid or result.failed:
            print(out, flush=True)
        if not result.valid:
            print(f"REFUSED browser producer: {result.reason}", flush=True)
            return False, 0, 0
        INVENTORIES.setdefault("browser", result.inventory)
        return True, result.passed, result.failed


def main():
    import mutation_baseline  # retain native baseline output before starting producers
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
    INVENTORIES.clear()
    valid, passed, failed = browser_tests()
    print(f"baseline (browser): {passed} passed, {failed} failed", flush=True)
    if not valid or failed or not passed or watch.stopped:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    killed = refused = survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})")
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            valid, passed, failed = browser_tests()
        if not valid or watch.stopped:
            verdict = "REFUSED (invalid or stopped producer; no kill)"
            refused += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            killed += 1
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
        if watch.stopped:
            print("REFUSED: memory guard stopped the campaign")
            return 1
    if not build() or watch.stopped:
        print("REFUSED: restored source did not build")
        refused += 1
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())
