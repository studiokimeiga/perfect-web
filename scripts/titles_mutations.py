#!/usr/bin/env python3
"""Mutation controls for ADR-0183: a page states its title.

Each mutant undoes one piece:
- the rules: a page served at a route that states no title, a blank title,
  a title in a view, one inside an element, a second one, and one holding
  an element, each let through;
- the template: a title numbered where it is written; the plan not naming
  it; a title that reads a speculated value let through;
- the renderer: a title written into the body; its whitespace kept as
  written; a page's static document titled by its name;
- the server: the store's page titled "Store"; a changed title not set;
- the runtime: the title's part not set as the document's title, or set
  again when it holds its text.

A rule, template or speculation mutant must fail `pw-core`'s
`tests/titles.rs`; a renderer mutant, `pw-render`'s; a server mutant, the
development server's tests; a runtime mutant, `e2e/accessibility.spec.mjs`
in Chromium, against a build of the mutated source.

Run from the repository root; `just e14-titles` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
RENDER = ROOT / "runtime/pw-render/src/lib.rs"
BIN = ROOT / "runtime/pw-render/src/bin/pw-render.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a page served at a route may state no title",
        "core",
        CHECK,
        "        if !page || !(stated.is_none() || blank) {\n",
        "        if true {\n",
    ),
    (
        "a title of whitespace states one",
        "core",
        CHECK,
        "        let blank = stated.is_some_and(|t| match body.node(t) {\n",
        "        let blank = false && stated.is_some_and(|t| match body.node(t) {\n",
    ),
    (
        "a title in a view is let through",
        "core",
        CHECK,
        "            let misplaced = if !page {\n",
        "            let misplaced = if false {\n",
    ),
    (
        # Re-anchored by ADR-0186: a page's metadata is refused inside an
        # element by a line of the same text.
        "a title inside an element is let through",
        "core",
        CHECK,
        "            } else if !roots.contains(&n) {\n"
        "                Some(format!(\n"
        "                    \"`<title>` in `{}` is inside an element or a block, not at the top of its view\",\n",
        "            } else if false {\n"
        "                Some(format!(\n"
        "                    \"`<title>` in `{}` is inside an element or a block, not at the top of its view\",\n",
    ),
    (
        "a second title is let through",
        "core",
        CHECK,
        "            } else if stated.is_some() {\n",
        "            } else if false {\n",
    ),
    (
        "a title holding an element is let through",
        "core",
        CHECK,
        "            } else if let Some(other) = other {\n",
        "            } else if let Some(other) = other.filter(|_| false) {\n",
    ),
    (
        "a title is numbered where it is written",
        "core",
        TEMPLATE,
        "    for root in rest {\n"
        "        lower_node(body, root, &ctx, &mut ix, &mut own);\n"
        "    }\n",
        # The page's title numbered before its own markup, the layout's
        # first (ADR-0303): the titles taken, lowered, and none left after.
        "    let titles: Vec<NodeId> = titles\n"
        "        .into_iter()\n"
        "        .inspect(|t| chunks.push(Chunk::Dynamic(lower_title(body, *t, &ctx, &mut ix))))\n"
        "        .filter(|_| false)\n"
        "        .collect();\n"
        "    for root in rest {\n"
        "        lower_node(body, root, &ctx, &mut ix, &mut own);\n"
        "    }\n",
    ),
    (
        "the plan does not name the title",
        "core",
        PLAN,
        # Re-anchored by ADR-0300, whose plan names its scope after the
        # title, and by the layouts' ADR, whose plan names its layout after it.
        "            title,\n            scope: crate::resume::page_scope(hir, decl).to_string(),\n",
        "            title: None,\n            scope: crate::resume::page_scope(hir, decl).to_string(),\n",
    ),
    (
        "a title that reads a speculated value is let through",
        "core",
        SPECULATION,
        "            crate::template_ir::ReadKind::Title => {\n"
        "                return Encoding::Unsupported {\n",
        "            crate::template_ir::ReadKind::Title => RegionKind::Attribute,\n"
        "            #[allow(unreachable_patterns)]\n"
        "            _ => {\n"
        "                return Encoding::Unsupported {\n",
    ),
    (
        "a title is written into the body",
        "render",
        RENDER,
        "        Part::Title { .. } => Ok(()),\n",
        "        Part::Title { .. } => {\n"
        "            out.push_str(\"<title></title>\");\n"
        "            Ok(())\n"
        "        }\n",
    ),
    (
        "a title keeps its whitespace as written",
        "render",
        RENDER,
        "    Ok(Some(\n"
        "        text.split_ascii_whitespace().collect::<Vec<_>>().join(\" \"),\n"
        "    ))\n",
        "    Ok(Some(text))\n",
    ),
    (
        "a page's static document is titled by its name",
        "render",
        BIN,
        "            Ok(Some(title)) => title,\n",
        "            Ok(Some(_)) => t.name.clone(),\n",
    ),
    (
        "the store's page is titled \"Store\"",
        "server",
        SERVER,
        # Re-anchored by ADR-0190: another page is titled by its name. And by
        # ADR-0191, which reads the store's page by its path.
        "                Ok(title) => title.unwrap_or_else(|| {\n"
        "                    if page == server.store_page() {\n"
        "                        \"Store\".to_string()\n"
        "                    } else {\n"
        "                        template.name.clone()\n"
        "                    }\n"
        "                }),\n",
        "                Ok(_) => \"Store\".to_string(),\n",
    ),
    (
        "a changed title is not set",
        "server",
        SERVER,
        "            && was.title.as_ref().map(|(_, t)| t) != Some(title)\n",
        "            && false\n",
    ),
    (
        "the runtime sets no title",
        "browser",
        RUNTIME,
        '    if (p.kind === "title") index.set(addressOf([], p.id), { title: true });\n',
        '    if (false) index.set(addressOf([], p.id), { title: true });\n',
    ),
    (
        "the runtime sets a title it shows already",
        "browser",
        RUNTIME,
        "    if (document.title !== text) document.title = text;\n",
        "    document.title = text;\n",
    ),
]

CARGO = {
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "titles"],
    "render": ["cargo", "test", "--quiet", "--locked", "-p", "pw-render", "--test", "titles"],
    # The tests that fail under its mutants, measured against the whole suite
    # by `scripts/mutant_killers.py` on 2026-10-12 and 2026-10-13 (W6's
    # ruling of 2026-10-09), two per mutant where two fail.
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_page_carries_its_data_layers_style_and_no_other",
        "tests::a_page_that_binds_a_query_is_rendered_with_its_parameters",
        "tests::a_title_that_changed_is_set_as_its_text",
    ],
}
BROWSER = ["e2e/accessibility.spec.mjs", "--project=chromium"]

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


def build():
    """The page and the server the suite runs, as the source says."""
    page = subprocess.run(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                          env={**os.environ, "BUILD_ONLY": "1"}, capture_output=True, text=True)
    server = subprocess.run(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
                            cwd=ROOT, capture_output=True, text=True)
    return page.returncode == 0 and server.returncode == 0


def browser_tests(_suite="browser"):
    """(built, passed, failed) for the spec, after a build."""
    if not build():
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


SUITES = {
    "core": cargo_tests,
    "render": cargo_tests,
    "server": cargo_tests,
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
    try:
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
    finally:
        # The page and the server as the source says, whatever a browser
        # mutant left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
