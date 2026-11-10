#!/usr/bin/env python3
"""Mutation controls for ADR-0311: a page states its canonical address, and a
navigation takes the next page's head whole.

Each mutant undoes one piece:
- the checker: a page's own canonical link refused as any other; a canonical
  link anywhere taken as the page's; one naming more relations than
  `canonical`, or with another attribute, passing (PW5035); an address naming
  anything, a page served at no route, or an argument computed in place
  passing (PW5051); `og:url` a page's to state (PW5034); and the page an
  address names called (PW0027);
- the value relations: an address's arguments not related to the page's
  parameters (PW0604, PW0605);
- the HIR: an address lowered as a call of its page, which every analysis
  that reads a call then reads, its privacy label among them;
- the template IR: the link lowered in the body, and an argument given by
  its place only;
- the renderer: the address written without the origin, a value unescaped,
  `og:url` not written, an address written with no origin, and a value no
  segment carries written;
- the host: an origin with a path taken, and a production deployment with
  no origin started;
- the browser: the server not given its origin, and a navigation taking
  only the next page's description.

A checker, HIR, relations or IR mutant must fail `compiler/pw-core/tests/
canonical.rs` or `links.rs`; a renderer mutant, `runtime/pw-render/tests/
canonical.rs`; a host mutant, the host's tests of a canonical address and of
an origin; the browser's, `e2e/canonical.spec.mjs` in Chromium.

Run from the repository root; `just e14-canonical` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
CANONICAL = ROOT / "compiler/pw-core/src/canonical.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
LOWER = ROOT / "compiler/pw-core/src/lower.rs"
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
RENDER = ROOT / "runtime/pw-render/src/lib.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
IDENTITY = ROOT / "spikes/own-renderer/server/src/identity.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a page's own canonical link is refused as any other",
        "core",
        CHECK,
        "                (Some(_), None) if canonical && own == Some(n) => {\n",
        "                (Some(_), None) if false && canonical && own == Some(n) => {\n",
    ),
    (
        "a canonical link anywhere in a page is its own",
        "core",
        CANONICAL,
        "    crate::template_ir::roots_of(body)\n        .into_iter()\n"
        "        .find(|r| is_canonical_link(body, *r))\n",
        "    body.walk_markup(&crate::template_ir::roots_of(body))\n        .into_iter()\n"
        "        .find(|r| is_canonical_link(body, *r))\n",
    ),
    (
        "a canonical link names other relations too",
        "core",
        CHECK,
        "                    if written.split_ascii_whitespace().count() > 1 {\n",
        "                    if written.split_ascii_whitespace().count() > 2 {\n",
    ),
    (
        "a canonical link carries another attribute",
        "core",
        CHECK,
        '                        .find(|a| !["rel", "href"].contains(&a.name.as_str()));\n',
        '                        .find(|a| !["rel", "href", "hreflang"].contains(&a.name.as_str()));\n',
    ),
    (
        "a canonical address names anything",
        "core",
        CHECK,
        "        if kind != Some(DeclKind::Page) {\n",
        "        if false && kind != Some(DeclKind::Page) {\n",
    ),
    (
        "a page served at no route is an address",
        "core",
        CHECK,
        "        match route {\n",
        "        match route.or(Some(String::new())) {\n",
    ),
    (
        "an argument computed in place is given",
        "core",
        CHECK,
        "            if !text && crate::template_ir::value_path_of(body, a.value).is_none() {\n",
        "            if false && !text && crate::template_ir::value_path_of(body, a.value).is_none() {\n",
    ),
    (
        "a page states its own og:url",
        "core",
        CHECK,
        "                .is_some_and(|v| v.eq_ignore_ascii_case(\"og:url\"))\n",
        "                .is_some_and(|v| v.eq_ignore_ascii_case(\"og:url\") && false)\n",
    ),
    (
        "an address is lowered as a call of its page",
        "core",
        LOWER,
        "                    Some(e) if canonical && name == \"href\" => match self.address(b, &e) {\n",
        "                    Some(e) if false && canonical && name == \"href\" => match self.address(b, &e) {\n",
    ),
    (
        "an address's arguments are not the page's parameters",
        "core",
        VALUES,
        "        if let Expr::Address { page, args } = self.body.expr(id) {\n",
        "        if let Expr::Address { page, args } = self.body.expr(id)\n            && false\n        {\n",
    ),
    (
        "the link is lowered in the body",
        "core",
        TEMPLATE,
        "        .partition(|r| page && (is_title(body, *r) || is_meta(body, *r) || canonical == Some(*r)));\n",
        "        .partition(|r| page && (is_title(body, *r) || is_meta(body, *r)));\n",
    ),
    (
        "an argument is given by its place only",
        "core",
        TEMPLATE,
        "        let param = a.name.clone().unwrap_or_else(|| params[i].to_string());\n",
        "        let param = params[i].to_string();\n",
    ),
    (
        "an address is written without the origin",
        "render",
        RENDER,
        "        let absolute = format!(\"{}{address}\", origin.trim_end_matches('/'));\n",
        "        let absolute = format!(\"{address}\");\n",
    ),
    (
        "a value is written unescaped",
        "render",
        RENDER,
        "            address = address.replace(&format!(\"{{{name}}}\"), &escape::url_component(&text));\n",
        "            address = address.replace(&format!(\"{{{name}}}\"), &text);\n",
    ),
    (
        "og:url is not written",
        "render",
        RENDER,
        "            \"<link rel=\\\"canonical\\\" href=\\\"{}\\\">\\n<meta property=\\\"og:url\\\" content=\\\"{}\\\">\\n\",\n",
        "            \"<link rel=\\\"canonical\\\" href=\\\"{}\\\">\\n<!-- {} -->\\n\",\n",
    ),
    (
        "an address is written with no origin",
        "render",
        RENDER,
        "        let Some(origin) = origin else {\n",
        "        let Some(origin) = origin.or(Some(\"\")) else {\n",
    ),
    (
        "a value no segment carries is written",
        "render",
        RENDER,
        "            if text.is_empty() || text == \".\" || text == \"..\" {\n",
        "            if false {\n",
    ),
    (
        "an origin may carry a path",
        "host",
        IDENTITY,
        "        if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {\n",
        "        if authority.is_empty() || authority.contains(['?', '#', '@']) {\n",
    ),
    (
        "a production deployment with no origin starts",
        "host",
        SERVER,
        "    if server.states_canonical() && !deployment.development && !stated {\n",
        "    if server.states_canonical() && !deployment.development && !stated && false {\n",
    ),
    (
        "the server is given an origin not its deployment's",
        "browser",
        SERVER,
        "            let _ = server.origin.set(origin);\n",
        "            let _ = (origin, server.origin.set(\"https://elsewhere.invalid\".to_string()));\n",
    ),
    (
        "a navigation takes only the next page's description",
        "browser",
        RUNTIME,
        "  const stated = 'meta[name]:not([name=\"viewport\" i]), meta[property], link[rel=\"canonical\"], style';\n",
        "  const stated = 'meta[name=\"description\"], style';\n",
    ),
]

CARGO = {
    "core": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core",
        "--test", "canonical", "--test", "links",
    ],
    "render": ["cargo", "test", "--quiet", "--locked", "-p", "pw-render", "--test", "canonical"],
    "host": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "a_page_s_canonical_address_is_written_with_the_deployment_s_origin",
        "an_origin_is_a_scheme_a_host_and_a_port",
    ],
}
SPEC = ["e2e/canonical.spec.mjs", "--project=chromium"]

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
    """(built, passed, failed), after a build: the pages, and the servers the
    suite starts."""
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
    "render": cargo_tests,
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
