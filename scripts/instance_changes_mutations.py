#!/usr/bin/env python3
"""Mutation controls for ADR-0168: a change reaches every part that reads
it.

Each mutant undoes one piece:
- the renderer: a changed attribute, or a changed text, not set; a block
  that changed set in place; an attribute's value not the one the document
  writes;
- the development server: a rename that sets the name's text alone; the menu
  control decoding only `%20`;
- the browser runtime: an attribute patch not applied, or applied as
  written rather than as parsed; an instance already in place moved.

A renderer mutant must fail `pw-render`'s tests, all of them run; a server
mutant, the server's tests or the browser's; a runtime mutant,
`e2e/stores.spec.mjs` or `e2e/keyed-list.spec.mjs` in Chromium.

Run from the repository root; `just e14-instance-changes` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RENDER = ROOT / "runtime/pw-render/src/lib.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a changed attribute is not set",
        "render",
        RENDER,
        "                if x != y\n                    && let Some((name, value)) = y\n",
        "                if false\n                    && x != y\n                    && let Some((name, value)) = y\n",
    ),
    (
        "a changed text is not set",
        "render",
        RENDER,
        "                    if x != y {\n"
        "                        out.push((*id, InstanceChange::Text(y)));\n",
        "                    if false && x != y {\n"
        "                        out.push((*id, InstanceChange::Text(y)));\n",
    ),
    (
        "a block that changed is set in place",
        "render",
        RENDER,
        # Re-anchored by ADR-0178: a block that decides is looked into, and
        # any other, a list inside the row, is what this one is now.
        "            _ => {\n"
        "                if rendered(c, before)? != rendered(c, after)? {\n"
        "                    return Ok(false);\n",
        "            _ => {\n"
        "                if false && rendered(c, before)? != rendered(c, after)? {\n"
        "                    return Ok(false);\n",
    ),
    (
        "an attribute's value is not the one the document writes",
        "render",
        RENDER,
        "            (name.clone(), Some(escaped(&s, *context)))\n",
        "            (name.clone(), Some(s))\n",
    ),
    (
        "a rename sets the name's text alone",
        "server",
        SERVER,
        # Re-anchored by ADR-0178: a rename's patches are its row's, as the
        # whole menu's difference is derived, which a session's list's are;
        # and by ADR-0181, where an attribute's change becomes its patch.
        "            pw_render::InstanceChange::Attribute {\n"
        "                name,\n"
        "                value: Some(value),\n"
        "            } => out.push(Targeted {\n"
        "                target,\n"
        "                operation: PatchOp::SetAttribute { name, value },\n"
        "            }),\n",
        "            pw_render::InstanceChange::Attribute { value: Some(_), .. } => {}\n",
    ),
    (
        "the menu control decodes only spaces",
        "browser",
        SERVER,
        "            // Until 2026-10-03 only `%20` was, and `%26` stayed in a name.\n"
        "            let q = |k: &str| {\n"
        "                query\n"
        "                    .split('&')\n"
        "                    .find_map(|p| p.strip_prefix(&format!(\"{k}=\")))\n"
        "                    .and_then(percent_decoded)\n",
        "            // Until 2026-10-03 only `%20` was, and `%26` stayed in a name.\n"
        "            let q = |k: &str| {\n"
        "                query\n"
        "                    .split('&')\n"
        "                    .find_map(|p| p.strip_prefix(&format!(\"{k}=\")))\n"
        "                    .map(|v| v.replace(\"%20\", \" \"))\n",
    ),
    (
        "an attribute patch is not applied",
        "browser",
        RUNTIME,
        "function setAttributeAt(address, op) {\n",
        "function setAttributeAt(address, op) {\n  if (address) return true;\n",
    ),
    (
        "an attribute is set as written, not as parsed",
        "browser",
        RUNTIME,
        # Re-anchored by ADR-0182, which sets it only when it changes.
        "  const value = unescaped(op.value, \"\\ufffd\");\n",
        "  const value = op.value;\n",
    ),
    (
        "an instance already in place is moved",
        "browser",
        RUNTIME,
        "      if (nodes.includes(anchor)) return true;\n",
        "",
    ),
]

CARGO = {
    "render": ["cargo", "test", "--quiet", "--locked", "-p", "pw-render"],
    # The tests that fail under its mutants, measured against the whole suite
    # by `scripts/mutant_killers.py` on 2026-10-12 and 2026-10-13 (W6's
    # ruling of 2026-10-09), two per mutant where two fail.
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_rename_sets_every_part_that_reads_the_name",
    ],
}
BROWSER = ["e2e/stores.spec.mjs", "e2e/keyed-list.spec.mjs", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
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
    """(built, passed, failed) over the suite's tests."""
    out, code = bounded(CARGO[suite], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


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
        ["pnpm", "exec", "playwright", "test", *BROWSER, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"render": cargo_tests, "server": cargo_tests, "browser": browser_tests}


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
