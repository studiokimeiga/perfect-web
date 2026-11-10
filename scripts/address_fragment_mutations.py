#!/usr/bin/env python3
"""Mutation controls for track `todomvc`'s fragment binding (ADR-0367): a page
binds a signal to its address's fragment.

Each mutant undoes one piece:
- the checker: a clause that names no signal the page declares; a signal that
  is no `String`; a form of characters a fragment does not hold, one with an
  escape, one written twice, a comma that ends the forms; a first value no
  form gives; a handler that assigns the signal, or the refusal of any
  assignment; a link to a fragment no form names;
- the plan: the binding not carried;
- the host: the binding not carried into a document, of either builder;
- the browser: the fragment not read at start; not followed on a change; a
  fragment that is no form not taken as the default; a form compared with
  its `#`; the signal set from the first form where none is.

A checker mutant must fail `compiler/pw-core/tests/address_fragment.rs`; the
host's, `tests/address_fragment.rs` in the server; a browser mutant,
`e2e/fragment.spec.mjs` in Chromium.

Run from the repository root; `just e14-address-fragment` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECKER = ROOT / "compiler/pw-core/src/address_fragment.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a clause naming a signal the page does not declare is accepted",
        "core",
        CHECKER,
        "        let Some((_, signal)) = signals.iter().find(|(n, _)| *n == binding.signal) else {\n",
        "        let Some((_, signal)) = signals.iter().find(|(n, _)| *n == binding.signal || true) else {\n",
    ),
    (
        "a signal that is no String is accepted",
        "core",
        CHECKER,
        '        if written != "String" {\n',
        '        if false && written != "String" {\n',
    ),
    (
        "a form with a character a fragment does not hold is accepted",
        "core",
        CHECKER,
        "        if let Some(c) = form.chars().find(|c| !form_char(*c)) {\n",
        "        if let Some(c) = form.chars().find(|c| !form_char(*c) && false) {\n",
    ),
    (
        "a form with a percent sign is accepted",
        "core",
        CHECKER,
        "    c.is_ascii_alphanumeric() || \"-._~!$&'()*+,;=:@/?\".contains(c)\n",
        "    c.is_ascii_alphanumeric() || \"-._~!$&'()*+,;=:@/?%\".contains(c)\n",
    ),
    (
        "a form with an escape is accepted",
        "core",
        CHECKER,
        "    if text.contains('\\\\') {\n",
        "    if false && text.contains('\\\\') {\n",
    ),
    (
        "a form written twice is accepted",
        "core",
        CHECKER,
        "        if forms.iter().any(|f| f.form == form) {\n",
        "        if false && forms.iter().any(|f| f.form == form) {\n",
    ),
    (
        "a comma that ends the forms is accepted",
        "core",
        CHECKER,
        "            Some(_) => {\n                return Err((\n                    \"fragment_form_not_pair\",\n                    \"a comma ends the forms with none after it\".to_string(),\n                ));\n            }\n",
        "            Some(_) => break,\n",
    ),
    (
        "a form with no arrow is accepted",
        "core",
        CHECKER,
        '        let Some(after) = after.strip_prefix("=>") else {\n',
        '        let Some(after) = Some(after.strip_prefix("=>").unwrap_or(after)) else {\n',
    ),
    (
        "a first value no form gives is accepted",
        "core",
        CHECKER,
        "            Some(start) if binding.forms.iter().any(|f| f.value == start) => {}\n",
        "            Some(_) => {}\n",
    ),
    (
        "a handler's assignment of the bound signal is accepted",
        "core",
        CHECKER,
        "        if lexical.binder(*lhs) != Some(mine) {\n",
        "        if true || lexical.binder(*lhs) != Some(mine) {\n",
    ),
    (
        "an assignment of any signal is refused",
        "core",
        CHECKER,
        "        if lexical.binder(*lhs) != Some(mine) {\n",
        "        if lexical.binder(*lhs).is_none() {\n",
    ),
    (
        "a link to a fragment no form names is accepted",
        "core",
        CHECKER,
        "        if binding.forms.iter().any(|f| f.form == fragment) {\n",
        "        if true || binding.forms.iter().any(|f| f.form == fragment) {\n",
    ),
    (
        "a link to a form is refused",
        "core",
        CHECKER,
        "        if binding.forms.iter().any(|f| f.form == fragment) {\n",
        "        if binding.forms.iter().any(|f| f.form != fragment) {\n",
    ),
    (
        "a page binds nothing in its plan",
        "core",
        PLAN,
        "            fragment: crate::address_fragment::declared(decl),\n",
        "            fragment: None,\n",
    ),
    (
        "a page of signals alone does not say its binding",
        "host",
        SERVER,
        "    let mut manifest = manifest;\n    // The signal it binds to its address's fragment (track `todomvc`).\n    with_fragment(&mut manifest, plan);\n    // TRACK SEAM (device-store): what the device keeps that it holds.\n    device::with_device(&mut manifest, plan);\n    // What it computes from its signals (ADR-0227), and each row of a list\n",
        "    let mut manifest = manifest;\n    // TRACK SEAM (device-store): what the device keeps that it holds.\n    device::with_device(&mut manifest, plan);\n    // What it computes from its signals (ADR-0227), and each row of a list\n",
    ),
    (
        "a page that reads queries does not say its binding",
        "host",
        SERVER,
        "    // The signal it binds to its address's fragment (track `todomvc`).\n    with_fragment(&mut manifest, plan);\n    // TRACK SEAM (device-store): what the device keeps that it holds.\n    device::with_device(&mut manifest, plan);\n    // What it computes from them (ADR-0227), and each row of a list it\n",
        "    // TRACK SEAM (device-store): what the device keeps that it holds.\n    device::with_device(&mut manifest, plan);\n    // What it computes from them (ADR-0227), and each row of a list it\n",
    ),
    (
        "the fragment is not read at start",
        "browser",
        RUNTIME,
        "function bindFragment(binding) {\n  followFragment(binding);\n",
        "function bindFragment(binding) {\n",
    ),
    (
        "the fragment is not followed on a change",
        "browser",
        RUNTIME,
        '  addEventListener("hashchange", () => followFragment(binding), { signal: life.signal });\n',
        "",
    ),
    (
        "a fragment that is no form leaves the signal as it was",
        "browser",
        RUNTIME,
        "  return binding.forms.find((f) => f.form === form)?.value ?? firstValues.get(binding.signal);\n",
        "  return binding.forms.find((f) => f.form === form)?.value ?? signals.get(binding.signal);\n",
    ),
    (
        "the signal is set again to the value it holds",
        "browser",
        RUNTIME,
        "  if (signals.get(binding.signal) === value) return;\n",
        "",
    ),
    (
        "a form is compared with its hash sign",
        "browser",
        RUNTIME,
        "  const form = location.hash.slice(1);\n  return binding.forms.find",
        "  const form = location.hash;\n  return binding.forms.find",
    ),
    (
        "a form is compared without its case",
        "browser",
        RUNTIME,
        "  return binding.forms.find((f) => f.form === form)?.value",
        "  return binding.forms.find((f) => f.form.toLowerCase() === form.toLowerCase())?.value",
    ),
    (
        "a page that binds nothing follows the fragment",
        "browser",
        RUNTIME,
        "  if (PW_FRAGMENT && parts.fragment) bindFragment(parts.fragment);\n",
        '  bindFragment(parts.fragment ?? { signal: "name", forms: [{ form: "/specs", value: "Grace" }] });\n',
    ),
]

CARGO = {
    "core": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "address_fragment",
    ],
    "host": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "tests::address_fragment::",
    ],
}
SPEC = ["e2e/fragment.spec.mjs", "--project=chromium"]

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
    """(built, passed, failed), after a build: the pages the demo serves
    beside the store, and the servers the suite starts."""
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
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
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
