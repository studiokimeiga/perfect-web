#!/usr/bin/env python3
"""Mutation controls for the inline styles' ADR: a page's stylesheets, small
together, are part of its document.

Each mutant undoes one piece:
- which build and which page: a build that does not say so, a page rendered
  for each reader;
- the bound: none, or one byte short of what it says;
- what cannot move and keeps them all: a stylesheet the build does not hold, a
  link that says more than its address, a style of the page's own, a sheet with
  nothing said of how it is sent, and each thing a sheet may hold that would
  mean something else inside a document;
- where they go: their order, and the head;
- the policy: not told;
- the packaging: not saying so.

A mutant of the host must fail `tests::inline_styles::`, of the packaging
`optimize-assets.test.mjs`, and of the page `e2e/device-runtime.spec.mjs` in
Chromium against a production build.

Run from the repository root; `just e14-inline-styles` records the output. The
source is restored after every mutant, whatever happens.
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

# How long one command may run; past it, the run counts as failing.
BOUND = 900

# (what, suite, file, anchor, replacement)
MUTANTS = [
    ("a build that does not say so inlines", "host", SERVER,
     '        let inline = delivery_says(dist, "inline");\n', "        let inline = true;\n"),
    ("a build that says so inlines nothing", "host", SERVER,
     '        let inline = delivery_says(dist, "inline");\n', "        let inline = false;\n"),
    ("a page rendered for each reader inlines", "host", SERVER,
     "            let (document, style) = self.delivery.styles_inline(document);\n",
     "            let (document, style) = (document, None::<String>);\n"),
    ("any amount is inlined", "host", SERVER,
     'if sent > INLINE_AT_MOST || kept.contains("rel=\\"stylesheet\\"") {',
     'if kept.contains("rel=\\"stylesheet\\"") {'),
    ("the bound itself is too much", "host", SERVER,
     'if sent > INLINE_AT_MOST || kept.contains("rel=\\"stylesheet\\"") {',
     'if sent >= INLINE_AT_MOST || kept.contains("rel=\\"stylesheet\\"") {'),
    ("a page with no stylesheet is given an empty one", "host", SERVER,
     """        let Some(first) = first else {
            return (document, None);
        };
""",
     """        let first = first.unwrap_or(0);
"""),
    ("a link the walk did not read stays beside them", "host", SERVER,
     'if sent > INLINE_AT_MOST || kept.contains("rel=\\"stylesheet\\"") {',
     "if sent > INLINE_AT_MOST {"),
    ("a style between them is passed", "host", SERVER,
     """        let passes = document[first..last_ends].contains("<style")
            || (first > head_ends && document[head_ends..first].contains("<style"));
""",
     """        let passes = first > head_ends && document[head_ends..first].contains("<style");
"""),
    ("a head's link goes to the head's end", "host", SERVER,
     """        let at = if first < head_ends {
            first
        } else {
            kept.find("</head>").expect("the head, as above")
        };
""",
     """        let at = kept.find("</head>").expect("the head, as above");
"""),
    ("every style in the body keeps them where they are", "host", SERVER,
     """        let passes = document[first..last_ends].contains("<style")
            || (first > head_ends && document[head_ends..first].contains("<style"));
""",
     """        let passes = document[first..last_ends].contains("<style")
            || (first > head_ends && document[head_ends..].contains("<style"));
"""),
    ("a stylesheet the build does not hold is lost", "host", SERVER,
     "            let Some(style) = self.styles.get(href.split('?').next().unwrap_or(href)) else {\n"
     "                return (document, None);\n"
     "            };\n",
     "            let Some(style) = self.styles.get(href.split('?').next().unwrap_or(href)) else {\n"
     "                rest = &after[closes + END.len()..];\n"
     "                continue;\n"
     "            };\n"),
    ("a stylesheet is looked for with its version", "host", SERVER,
     "self.styles.get(href.split('?').next().unwrap_or(href))", "self.styles.get(href)"),
    ("a link that says more is read as if it did not", "host", SERVER,
     "            if !after[closes..].starts_with(END) {\n                return (document, None);\n            }\n", ""),
    ("a style of the page's own before them is passed over", "host", SERVER,
     """        let passes = document[first..last_ends].contains("<style")
            || (first > head_ends && document[head_ends..first].contains("<style"));
""",
     """        let passes = document[first..last_ends].contains("<style");
"""),
    ("the order is not kept", "host", SERVER,
     '        let style = texts.join("\\n");\n',
     '        texts.reverse();\n        let style = texts.join("\\n");\n'),
    ("they stay where the links were", "host", SERVER,
     """        let at = if first < head_ends {
            first
        } else {
            kept.find("</head>").expect("the head, as above")
        };
""",
     """        let at = first;
"""),
    ("an import moves", "host", SERVER,
     '    !["</style", "@import", "@namespace"]\n', '    !["</style", "@namespace"]\n'),
    ("a namespace moves", "host", SERVER,
     '    !["</style", "@import", "@namespace"]\n', '    !["</style", "@import"]\n'),
    ("what would end the element moves", "host", SERVER,
     '    !["</style", "@import", "@namespace"]\n', '    !["@import", "@namespace"]\n'),
    ("a sheet is read as written", "host", SERVER,
     "    let text = text.to_ascii_lowercase();\n", ""),
    ("a relative address moves", "host", SERVER,
     '            ["data:", "https:", "http:", "/", "#"]\n                .iter()\n                .any(|from| address.starts_with(from))\n',
     "            !address.is_empty() || true\n"),
    ("no address moves", "host", SERVER,
     '            ["data:", "https:", "http:", "/", "#"]\n                .iter()\n                .any(|from| address.starts_with(from))\n',
     "            address.is_empty()\n"),
    ("a space before an address is its first character", "host", SERVER,
     '                .trim_start()\n                .trim_start_matches(', '                .trim_start_matches('),
    ("a quote is part of an address", "host", SERVER,
     """                .trim_start()\n                .trim_start_matches(['"', '\\'']);\n""",
     "                .trim_start();\n"),
    ("a stylesheet with no record of its size has none", "host", SERVER,
     "                        sent: sent.len() as usize,\n", "                        sent: 0,\n"),
    ("the policy is not told", "host", SERVER,
     "        needs.styles.extend(style);\n", "        let _ = style;\n"),
    ("a stylesheet is not minified", "packaging", PACKAGING,
     '  if (entry.name.endsWith(".css")) {\n', '  if (entry.name.endsWith(".nothing")) {\n'),
    ("the packaging does not say so", "packaging", PACKAGING,
     "bundled: true, device: true, inline: true }", "bundled: true, device: true }"),
    ("the page does not ask for less", "browser", SERVER,
     "            let (document, style) = self.delivery.styles_inline(document);\n",
     "            let (document, style) = (document, None::<String>);\n"),
]


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
    out, code = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "tests::inline_styles::"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def browser():
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1"}
    _, code = bounded(["bash", "spikes/own-renderer/todomvc.sh"], cwd=ROOT, env=env)
    if code != 0:
        return False, 0, 0
    _, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/device-runtime.spec.mjs", "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer", env=env)
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
