#!/usr/bin/env python3
"""Mutation controls for ADR-0195's ruling 13, the host's half (the track
`page-csp`): every HTML answer carries a Content-Security-Policy generated
from the build and the deployment.

Each mutant undoes one piece:
- the build's refusal (PW5052): a scheme not another origin; a `//` naming
  no host, or its `\\` spelling not; a tab inside an address, or its
  leading spaces, kept; a srcset's later candidates not read; an image no
  load; a link that is an item's property taken as a load; a link's address
  a value chooses allowed; an address built with a value not read; a
  link's text before its value taken as known wherever it starts;
- the hashes: the script that starts the runtime not hashed, the data
  layer's style not hashed, a `<style>` the build writes not hashed, a text
  hashed with its CRs, the runtime's WebAssembly refused;
- what the build's markup asks: a style attribute a value gives, or one
  written, not found; markup a capability lets through taken as holding
  none; a block's markup not read; a comment's text read as markup; a
  quoted value read as attributes; a frame not found; a `<style>`'s text
  not read;
- the policy: a style attribute the build writes refused; a build that
  frames nothing framing the provider; the deployment's payment provider
  not framed; a sign-in at another origin refused; an address's origin
  keeping who it is from;
- the answers: a document written whole, or streamed, sent no policy; a
  document sent the bare policy; a page not found, a page the build
  rendered, a page moved, or the identity's page sent none; an answer that
  is not HTML sent one; an answer's type sniffed, of a document streamed,
  of the identity's; a stylesheet the build ships served as bytes; the
  announcer served styled;
- the runtime: the announcer, served unstyled, never hidden.

A build mutant must fail the compiler's tests of the rule, the link rule's,
the generality witnesses' and the corpus's (`tests/origins.rs`,
`links.rs`, `generality.rs`, `checking_source.rs`); a host mutant, the
server's policy tests (`csp::`, `tests::page_policy::`,
and the moved page's and the identity provider's page's in
`tests::redirects::` and `tests::sign_in::`); the runtime's, the
announcer's test in `e2e/page-policy.spec.mjs`, in Chromium, after the
store's page and the server are built again. The rest of that spec, a page
that runs with nothing refused in three engines, and its controls, is run
by `just e14-page-policy` beside this.

Run from the repository root; `just e14-page-policy` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CSP = ROOT / "spikes/own-renderer/server/src/csp.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
IDENTITY = ROOT / "spikes/own-renderer/server/src/identity.rs"
RENDER = ROOT / "runtime/pw-render/src/lib.rs"

RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
ORIGINS = ROOT / "compiler/pw-core/src/origins.rs"

CORE, SERVER_TESTS, BROWSER = "core", "server", "browser"

# (what is undone, the suite that kills it, file, anchor, replacement)
MUTANTS = [
    (
        "a scheme is not another origin",
        CORE,
        ORIGINS,
        "    if let Some(end) = scheme_end {\n",
        "    if let Some(end) = scheme_end.filter(|_| false) {\n",
    ),
    (
        "a `//` names no host",
        CORE,
        ORIGINS,
        "        (Some('/' | '\\\\'), Some('/' | '\\\\'))\n",
        "        (Some('#'), Some('#'))\n",
    ),
    (
        "a `\\` is not a `/`",
        CORE,
        ORIGINS,
        "        (Some('/' | '\\\\'), Some('/' | '\\\\'))\n",
        "        (Some('/'), Some('/'))\n",
    ),
    (
        "a tab inside an address is kept",
        CORE,
        ORIGINS,
        "        .filter(|c| !matches!(c, '\\t' | '\\n' | '\\r'))\n",
        "        .filter(|_| true)\n",
    ),
    (
        "an address's leading spaces are kept",
        CORE,
        ORIGINS,
        "    let trimmed = address.trim_matches(|c: char| c <= ' ');\n",
        "    let trimmed = address;\n",
    ),
    (
        "a srcset's candidates after its first are not read",
        CORE,
        ORIGINS,
        "            .filter_map(|candidate| candidate.split_ascii_whitespace().next())\n            .collect()\n",
        "            .filter_map(|candidate| candidate.split_ascii_whitespace().next())\n            .take(1)\n            .collect()\n",
    ),
    (
        "an image is no load",
        CORE,
        ORIGINS,
        "    (\"img\", \"src\"),\n",
        "",
    ),
    (
        "a link that is an item's property is taken as a load",
        CORE,
        ORIGINS,
        "                    && ((!has(\"rel\") && has(\"itemprop\"))\n",
        "                    && ((false && has(\"itemprop\"))\n",
    ),
    (
        "a link's address a value chooses is allowed",
        CORE,
        ORIGINS,
        "                        _ if element == \"link\" => Some(computed(tag, &a.name, &decl.name)),\n",
        "",
    ),
    (
        "an address built with a value is not read",
        CORE,
        ORIGINS,
        "                            match written(with_values_unread(unquoted(text)).as_str()) {\n",
        "                            match None::<String> {\n",
    ),
    (
        "an address built with a value is read only to its first value",
        CORE,
        ORIGINS,
        "                out.push('\\u{2026}');\n                rest = &rest[open + close + 1..];\n",
        "                rest = \"\";\n                let _ = close;\n",
    ),
    (
        "a link's text before its value is taken as known wherever it starts",
        CORE,
        ORIGINS,
        "                                None if element == \"link\" && !start.contains(['/', '?', '#']) => {\n",
        "                                None if element == \"link\" && start.is_empty() => {\n",
    ),
    (
        "the script that starts the runtime is not hashed",
        SERVER_TESTS,
        CSP,
        "            \"script-src 'self' 'wasm-unsafe-eval' {}\",\n            sha256_source(script)\n",
        "            \"script-src 'self' 'wasm-unsafe-eval' {}\",\n            sha256_source(\"\")\n",
    ),
    (
        "the runtime's WebAssembly is refused",
        SERVER_TESTS,
        CSP,
        "            \"script-src 'self' 'wasm-unsafe-eval' {}\",\n",
        "            \"script-src 'self' {}\",\n",
    ),
    (
        "the data layer's style is not hashed",
        SERVER_TESTS,
        CSP,
        "    if !style.is_empty() {\n        styles.push(sha256_source(style));\n    }\n",
        "    if false {\n        styles.push(sha256_source(style));\n    }\n",
    ),
    (
        "a `<style>` the build writes is not hashed",
        SERVER_TESTS,
        CSP,
        "    styles.extend(needs.styles.iter().map(|s| sha256_source(s)));\n",
        "",
    ),
    (
        "a text is hashed with its CRs",
        SERVER_TESTS,
        CSP,
        "    let text = text.replace(\"\\r\\n\", \"\\n\").replace('\\r', \"\\n\");\n",
        "    let text = text.to_string();\n",
    ),
    (
        "a style attribute a value gives is not found",
        SERVER_TESTS,
        CSP,
        "                            if name.eq_ignore_ascii_case(\"style\") =>\n",
        "                            if name.eq_ignore_ascii_case(\"no-style\") =>\n",
    ),
    (
        "markup a capability lets through is taken as holding no style attribute",
        SERVER_TESTS,
        CSP,
        "                        Part::RawHtml { .. } => self.style_attributes = true,\n",
        "",
    ),
    (
        "a block's markup is not read",
        SERVER_TESTS,
        CSP,
        "                    for inner in p.nested() {\n                        self.chunks(inner);\n                    }\n",
        "",
    ),
    (
        "a comment's text is read as markup",
        SERVER_TESTS,
        CSP,
        "            if let Some(comment) = rest.strip_prefix(\"!--\") {\n",
        "            if let Some(comment) = rest.strip_prefix(\"!--never\") {\n",
    ),
    (
        "a style attribute written is not found",
        SERVER_TESTS,
        CSP,
        "                if rest[..attribute_end].eq_ignore_ascii_case(\"style\") {\n",
        "                if &rest[..attribute_end] == \"style\" {\n",
    ),
    (
        "a quoted value is read as attributes",
        SERVER_TESTS,
        CSP,
        "                        Some(quote @ ('\"' | '\\'')) => {\n",
        "                        Some(quote @ '\\'') => {\n",
    ),
    (
        "a frame is not found",
        SERVER_TESTS,
        CSP,
        "                \"iframe\" => self.frames = true,\n",
        "",
    ),
    (
        "a `<style>`'s text is not read",
        SERVER_TESTS,
        CSP,
        "                    self.styles.insert(rest[..end].to_string());\n",
        "",
    ),
    (
        "a style attribute the build writes is refused",
        SERVER_TESTS,
        CSP,
        "    if needs.style_attributes {\n",
        "    if needs.style_attributes && false {\n",
    ),
    (
        "a build that frames nothing frames the provider",
        SERVER_TESTS,
        CSP,
        "    if let (true, Some(provider)) = (needs.frames, &deployment.frames) {\n",
        "    if let (_, Some(provider)) = (needs.frames, &deployment.frames) {\n",
    ),
    (
        "an address's origin keeps who it is from",
        SERVER_TESTS,
        CSP,
        "    let host = authority.rsplit('@').next().unwrap_or_default();\n",
        "    let host = authority;\n",
    ),
    (
        "the deployment's payment provider is not framed",
        SERVER_TESTS,
        SERVER,
        "            frames: self.payments.get().map(|p| p.provider.clone()),\n",
        "            frames: None,\n",
    ),
    (
        "a sign-in at another origin is refused",
        SERVER_TESTS,
        SERVER,
        "            forms: self.identity.sign_in_origin(),\n",
        "            forms: None,\n",
    ),
    (
        "a document written whole is sent no policy",
        SERVER_TESTS,
        SERVER,
        "                &format!(\"{PRIVATE}{cookie}{build}{delivery}{policy}\"),\n",
        "                &format!(\"{PRIVATE}{cookie}{build}{delivery}\"),\n",
    ),
    (
        "a streamed document is sent no policy",
        SERVER_TESTS,
        SERVER,
        "             {build}{delivery}{policy}connection: close\\r\\n\\r\\n\"\n",
        "             {build}{delivery}connection: close\\r\\n\\r\\n\"\n",
    ),
    (
        "a document is sent the bare policy",
        SERVER_TESTS,
        SERVER,
        "        let policy = format!(\"content-security-policy: {}\\r\\n\", self.policy());\n",
        "        let policy = format!(\"content-security-policy: {}\\r\\n\", csp::BARE);\n",
    ),
    (
        "an HTML answer that runs nothing is sent no policy",
        SERVER_TESTS,
        SERVER,
        "        &format!(\"{PRIVATE}{cookie}{}\", runs_nothing(mime)),\n",
        "        &format!(\"{PRIVATE}{cookie}\"),\n",
    ),
    (
        "a page the build rendered is sent no policy",
        SERVER_TESTS,
        SERVER,
        "    write_response(stream, code, mime, &runs_nothing(mime), body);\n",
        "    write_response(stream, code, mime, \"\", body);\n",
    ),
    (
        "an answer that is not HTML is sent the bare policy",
        SERVER_TESTS,
        SERVER,
        "    if mime.starts_with(\"text/html\") {\n        format!(\"content-security-policy: {}\\r\\n\", csp::BARE)\n",
        "    if !mime.is_empty() {\n        format!(\"content-security-policy: {}\\r\\n\", csp::BARE)\n",
    ),
    (
        "a page moved is sent no policy",
        SERVER_TESTS,
        SERVER,
        "            \"{PRIVATE}{cookie}location: {location}\\r\\n{}\",\n            runs_nothing(\"text/html\")\n",
        "            \"{PRIVATE}{cookie}location: {location}\\r\\n{}\",\n            runs_nothing(\"text/plain\")\n",
    ),
    (
        "the identity's page is sent no policy",
        SERVER_TESTS,
        IDENTITY,
        "        // An identity's page runs nothing (`csp::BARE`).\n        if self.mime.starts_with(\"text/html\") {\n",
        "        // An identity's page runs nothing (`csp::BARE`).\n        if self.mime.starts_with(\"text/never\") {\n",
    ),
    (
        "an answer's type may be sniffed",
        SERVER_TESTS,
        SERVER,
        # Re-anchored by ADR-0401: the connection line is the
        # answer's own (`Conn::connection`).
        "content-length: {}\\r\\n{NOSNIFF}{headers}{}\\r\\n\",\n",
        "content-length: {}\\r\\n{headers}{}\\r\\n\",\n",
    ),
    (
        "a streamed document's type may be sniffed",
        SERVER_TESTS,
        SERVER,
        "charset=utf-8\\r\\n{NOSNIFF}{PRIVATE}{cookie}\\\n",
        "charset=utf-8\\r\\n{PRIVATE}{cookie}\\\n",
    ),
    (
        "the identity's answers may be sniffed",
        SERVER_TESTS,
        IDENTITY,
        "            super::NOSNIFF,\n",
        "            \"\",\n",
    ),
    (
        "a stylesheet the build ships is served as bytes",
        SERVER_TESTS,
        SERVER,
        "        Some(\"css\") => \"text/css; charset=utf-8\",\n",
        "",
    ),
    (
        "the announcer is served styled",
        SERVER_TESTS,
        RENDER,
        "pub const ANNOUNCER: &str = \"<div role=\\\"status\\\" class=\\\"pw-announcer\\\"></div>\";\n",
        "pub const ANNOUNCER: &str = \"<div role=\\\"status\\\" class=\\\"pw-announcer\\\" style=\\\"position:absolute\\\"></div>\";\n",
    ),
    (
        "the announcer, served unstyled, is never hidden",
        BROWSER,
        RUNTIME,
        "  announcer.style.cssText =\n    \"position:absolute;width:1px;height:1px;margin:-1px;padding:0;border:0;\" +\n",
        "  void\n    \"position:absolute;width:1px;height:1px;margin:-1px;padding:0;border:0;\" +\n",
    ),
]

CORE_TESTS = [
    "cargo", "test", "--locked", "-p", "pw-core",
    "--test", "origins", "--test", "links", "--test", "generality", "--test", "checking_source",
]

CARGO = [
    "cargo", "test", "--locked", "-p", "pw-dev-server", "--",
    "csp::", "tests::page_policy::",
    "tests::redirects::a_page_at_another_address_of_the_page_is_moved_there_for_good",
    "tests::sign_in::the_development_provider_signs_up_and_in_through_the_flow",
]

# The browser's: the announcer's test, in Chromium, against the store's
# page, which serves the runtime it was built with.
SPEC = ["e2e/page-policy.spec.mjs", "-g", "announcer", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, **kw):
    """(output, exit code), or None for the code where it ran past BOUND."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(command):
    """(built, passed, failed) for a cargo test command."""
    out, code = bounded(command, cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(a) for a, _ in found), sum(int(b) for _, b in found)


def browser_tests():
    """(built, passed, failed), after a build: the store's page, which serves
    the runtime it was built with, and the server the suite runs."""
    built = subprocess.run(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                           env={**os.environ, "BUILD_ONLY": "1"},
                           capture_output=True, text=True)
    if built.returncode != 0:
        return False, 0, 0
    server = subprocess.run(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
                            cwd=ROOT, capture_output=True, text=True)
    if server.returncode != 0:
        return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer")
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


def core_tests():
    """(built, passed, failed) for the build's refusal and the corpus."""
    return cargo_tests(CORE_TESTS)


def server_tests():
    """(built, passed, failed) for the policy's tests."""
    return cargo_tests(CARGO)


SUITES = {CORE: core_tests, SERVER_TESTS: server_tests, BROWSER: browser_tests}


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
    try:
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
            print(f"{what}: {verdict}", flush=True)
    finally:
        # The page and the server built again from the restored source.
        browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
