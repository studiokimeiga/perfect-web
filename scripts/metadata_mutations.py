#!/usr/bin/env python3
"""Mutation controls for ADR-0186: a page states its description.

Each mutant undoes one piece:
- the rule, PW5034: an item's property (microdata) taken as the page's
  metadata, or let name it too; metadata in a view, inside an element, the
  host's own (a charset, an `http-equiv`, the viewport in any case), one
  that names nothing, one named twice or by a computed value, one with an
  attribute the head does not write, one with no content or a blank one,
  one whose content or a hole of it is computed in place, and a second of a
  name HTML allows once, each let through; a name HTML lets repeat refused;
  names compared by their case;
- the template: metadata lowered with the rest of the view, or numbered
  before the title; the text after its last value dropped; an item's
  property taken into the head, or refused in the body;
- the plan: metadata that reads a signal let through;
- speculation: metadata that reads a speculated value refused as a title
  is;
- the renderer: metadata written into the body, unescaped, or from a value
  that is HTML; the binary's document written without it; a static page
  that states its title shipping the runtime (a correction to ADR-0183);
- the server: the store's page and a signal page written without it, and
  the store's rendered from no values.

A rule, template, plan or speculation mutant must fail `pw-core`'s
`tests/metadata.rs`; a renderer mutant, `pw-render`'s `tests/metadata.rs`
or `tests/titles.rs`; a server mutant, the development server's tests of a
page's head.

Run from the repository root; `just e14-metadata` records the output. The
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

INSIDE = (
    "                Some(format!(\n"
    "                    \"`<meta>` in `{}` is inside an element or a block, not at the top of its view\",\n"
)

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "an item's property is taken as the page's metadata",
        "core",
        CHECK,
        "            let message = if attr(\"itemprop\").is_some() {\n",
        "            let message = if false && attr(\"itemprop\").is_some() {\n",
    ),
    (
        "an item's property that names the page's metadata is let through",
        "core",
        CHECK,
        "                if keys.is_empty() && host.is_none() {\n",
        "                if true {\n",
    ),
    (
        "metadata in a view is let through",
        "core",
        CHECK,
        "            } else if !page {\n",
        "            } else if false {\n",
    ),
    (
        "metadata inside an element or a block is let through",
        "core",
        CHECK,
        "            } else if !roots.contains(&n) {\n" + INSIDE,
        "            } else if false {\n" + INSIDE,
    ),
    (
        "the host's charset, http-equiv or viewport is let through",
        "core",
        CHECK,
        "            } else if let Some(host) = host {\n",
        "            } else if let Some(host) = host.filter(|_| false) {\n",
    ),
    (
        "the viewport is the host's only as written in lower case",
        "core",
        CHECK,
        "            } else if static_attr(attrs, \"name\").is_some_and(|v| v.eq_ignore_ascii_case(\"viewport\"))\n",
        "            } else if static_attr(attrs, \"name\") == Some(\"viewport\")\n",
    ),
    (
        "metadata that names nothing is let through",
        "core",
        CHECK,
        "                    [] => Some(format!(\n"
        "                        \"`<meta>` in `{}` names nothing it describes\",\n"
        "                        decl.name\n"
        "                    )),\n",
        "                    [] => None,\n",
    ),
    (
        "metadata named by both name and property is let through",
        "core",
        CHECK,
        "                    [_, _] => Some(format!(\n",
        "                    [_, _] if false => Some(format!(\n",
    ),
    (
        "metadata named by a computed value is let through",
        "core",
        CHECK,
        "                    [(k, a)] if !matches!(a.value, AttrValue::Static(_)) => Some(format!(\n",
        "                    [(k, a)] if false && !matches!(a.value, AttrValue::Static(_)) => Some(format!(\n",
    ),
    (
        "an attribute the head does not write is let through",
        "core",
        CHECK,
        "                            _ if other.is_some() => Some(format!(\n",
        "                            _ if false && other.is_some() => Some(format!(\n",
    ),
    (
        "metadata with no content is let through",
        "core",
        CHECK,
        "                            None | Some(AttrValue::None) => Some(format!(\n",
        "                            None | Some(AttrValue::None) if false => Some(format!(\n",
    ),
    (
        "metadata with a blank content is let through",
        "core",
        CHECK,
        "                            Some(AttrValue::Static(_)) if !non_blank(attrs, \"content\") => Some(\n",
        "                            Some(AttrValue::Static(_)) if false => Some(\n",
    ),
    (
        "content computed in place is let through",
        "core",
        CHECK,
        "                            Some(AttrValue::Expr(e)) if !content_reads_values(body, *e) => {\n",
        "                            Some(AttrValue::Expr(e)) if false && !content_reads_values(body, *e) => {\n",
    ),
    (
        "a hole computed in place is read as a value",
        "core",
        CHECK,
        "            .all(|p| crate::template_ir::value_path_of(body, *p).is_some()),\n",
        "            .all(|_| true),\n",
    ),
    (
        "a content computed in place is read as a value",
        "core",
        CHECK,
        "        _ => crate::template_ir::value_path_of(body, e).is_some(),\n",
        "        _ => true,\n",
    ),
    (
        "a name HTML allows once, stated twice, is let through",
        "core",
        CHECK,
        "                                && !named.insert(v.to_ascii_lowercase()) =>\n",
        "                                && !named.insert(v.to_ascii_lowercase())\n"
        "                                && false =>\n",
    ),
    (
        "a name HTML lets repeat is refused",
        "core",
        CHECK,
        "                                && ONCE.iter().any(|o| o.eq_ignore_ascii_case(v))\n",
        "                                && true\n",
    ),
    (
        "names are compared by their case",
        "core",
        CHECK,
        "                                && !named.insert(v.to_ascii_lowercase()) =>\n",
        "                                && !named.insert(v.to_string()) =>\n",
    ),
    (
        "a name HTML allows once is known only in lower case",
        "core",
        CHECK,
        "                                && ONCE.iter().any(|o| o.eq_ignore_ascii_case(v))\n",
        "                                && ONCE.contains(&v)\n",
    ),
    (
        "metadata is lowered with the rest of the view",
        "core",
        TEMPLATE,
        "        .partition(|r| page && (is_title(body, *r) || is_meta(body, *r) || canonical == Some(*r)));\n",
        "        .partition(|r| page && (is_title(body, *r) || canonical == Some(*r)));\n",
    ),
    (
        "metadata is numbered before the title",
        "core",
        TEMPLATE,
        "    for title in titles {\n"
        "        chunks.push(Chunk::Dynamic(lower_title(body, title, &ctx, &mut ix)));\n"
        "    }\n"
        "    for meta in metas {\n"
        "        chunks.push(Chunk::Dynamic(lower_meta(body, meta, &ctx, &mut ix)));\n"
        "    }\n",
        "    for meta in metas {\n"
        "        chunks.push(Chunk::Dynamic(lower_meta(body, meta, &ctx, &mut ix)));\n"
        "    }\n"
        "    for title in titles {\n"
        "        chunks.push(Chunk::Dynamic(lower_title(body, title, &ctx, &mut ix)));\n"
        "    }\n",
    ),
    (
        "the text after metadata's last value is dropped",
        "core",
        TEMPLATE,
        "                if !rest.is_empty() {\n"
        "                    content.push(TitlePiece::Text(rest.to_string()));\n",
        "                if false {\n"
        "                    content.push(TitlePiece::Text(rest.to_string()));\n",
    ),
    (
        "an item's property at the top of a page is taken into its head",
        "core",
        TEMPLATE,
        "        if tag == \"meta\" && !attrs.iter().any(|a| a.name == \"itemprop\"))\n",
        "        if tag == \"meta\")\n",
    ),
    (
        "an item's property inside an element is refused",
        "core",
        TEMPLATE,
        "    if tag == \"meta\" && !attrs.iter().any(|a| a.name == \"itemprop\") {\n",
        "    if tag == \"meta\" {\n",
    ),
    (
        "metadata may read a signal",
        "core",
        PLAN,
        "            Part::Meta { content, .. } => title_reads(content),\n"
        "            Part::Canonical { args, .. } => canonical_reads(args),\n"
        "            // A stream's query's arguments",
        "            Part::Meta { .. } => Vec::new(),\n"
        "            Part::Canonical { args, .. } => canonical_reads(args),\n"
        "            // A stream's query's arguments",
    ),
    (
        "metadata that reads a speculated value is refused as a title is",
        "core",
        SPECULATION,
        "            crate::template_ir::ReadKind::Meta => continue,\n"
        "            // **A title that reads a speculated value** (ADR-0183): the\n"
        "            // browser renders no title again from a speculation, so the\n"
        "            // title would say what the value was while the page around it\n"
        "            // says what it is about to be.\n"
        "            crate::template_ir::ReadKind::Title => {\n",
        "            crate::template_ir::ReadKind::Title | crate::template_ir::ReadKind::Meta => {\n",
    ),
    (
        "metadata is written into the body",
        "render",
        RENDER,
        "        Part::Meta { .. } => Ok(()),\n",
        "        Part::Meta { .. } => {\n"
        "            out.push_str(\"<meta>\");\n"
        "            Ok(())\n"
        "        }\n",
    ),
    (
        "metadata is written unescaped",
        "render",
        RENDER,
        "            escape::attribute(&text)\n",
        "            text\n",
    ),
    (
        "metadata is written from a value that is HTML",
        "render",
        RENDER,
        "                    if let Value::Raw { .. } = v {\n"
        "                        return Err(Blocked::UnrepresentedConstruct {\n"
        "                            reason: \"metadata is text, and this value is HTML\".to_string(),\n",
        "                    if matches!(v, Value::Raw { .. }) && false {\n"
        "                        return Err(Blocked::UnrepresentedConstruct {\n"
        "                            reason: \"metadata is text, and this value is HTML\".to_string(),\n",
    ),
    (
        "the binary's document is written without its metadata",
        "render",
        BIN,
        "            Ok(metadata) => metadata,\n",
        "            Ok(_) => String::new(),\n",
    ),
    (
        "a static page that states its title ships the runtime",
        "render",
        BIN,
        "        .any(|e| e.anchor != pw_render::Anchor::Document);\n",
        "        .any(|_| true);\n",
    ),
    (
        # Re-anchored by ADR-0187: the store's style follows its metadata in
        # its head.
        "the store's page is written without its metadata",
        "server",
        SERVER,
        # Re-anchored by ADR-0220: the style is the data layer's; and by
        # track `device-store`: what the device decides hidden after it; and
        # by the first press's ADR: what the runtime boots with, last.
        "         <title>{title}</title>\\n{metadata}{style}{device}{BOOT}</head>",
        "         <title>{title}</title>\\n{style}{device}{BOOT}</head>",
    ),
    (
        "a signal page is written without its metadata",
        "server",
        SERVER,
        # Re-anchored by the first press's ADR: what the runtime boots with,
        # last in the head.
        "         <title>{}</title>\\n{metadata}{device}{BOOT}</head>",
        "         <title>{}</title>\\n{device}{BOOT}</head>",
    ),
    (
        "the store's metadata is rendered from no values",
        "server",
        SERVER,
        # Re-anchored by ADR-0190: by its page's template.
        "        let metadata = match pw_render::head_metadata(template, &env) {\n",
        "        let metadata = match pw_render::head_metadata(template, &Env::new()) {\n",
    ),
]

CARGO = {
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "metadata"],
    "render": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-render",
        "--test", "metadata", "--test", "titles",
    ],
    "server": [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "a_store_s_page_describes_itself_in_its_head",
        "a_signal_page_s_head_holds_its_title_and_metadata",
    ],
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    p = subprocess.Popen(CARGO[suite], cwd=ROOT, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(n) for n, _ in found), sum(int(f) for _, f in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite in CARGO:
        built, passed, failed = run_tests(suite)
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
            built, passed, failed = run_tests(suite)
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
