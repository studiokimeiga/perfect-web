#!/usr/bin/env python3
"""Mutation controls for item 7 of "a press changes what it changes": a
template's layout is not the page.

Each mutant undoes one piece of `as_shown` in the compiler's template lowering.
Every one must fail `compiler/pw-core/tests/template_layout.rs`.

Run from the repository root; `just e14-rows` records the output. The source is
restored after every mutant, whatever happens.
"""

import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "compiler/pw-core/src/template_ir.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    ("a run with a line break is left", SOURCE,
     "        if run.contains('\\n') {\n            out.push(' ');\n",
     "        if run.contains('\\n') {\n            out.push_str(run);\n"),
    ("every run is one space", SOURCE,
     "        if run.contains('\\n') {\n            out.push(' ');\n",
     "        if !run.is_empty() {\n            out.push(' ');\n"),
    ("a preformatted element's text is laid out", SOURCE,
     '    const IS_CONTENT: [&str; 4] = ["pre", "textarea", "script", "style"];\n',
     '    const IS_CONTENT: [&str; 2] = ["script", "style"];\n'),
    ("only the element a text is directly in is read", SOURCE,
     "    if parents.iter().any(|p| IS_CONTENT.contains(&p.as_str())) || !text.contains('\\n') {\n",
     "    if parents.last().is_some_and(|p| IS_CONTENT.contains(&p.as_str())) || !text.contains('\\n') {\n"),
    ("text that can never be shown is one space", SOURCE,
     '    if text.chars().all(space) && parents.last().is_some_and(|p| SHOWS_NO_TEXT.contains(&p.as_str())) {\n',
     '    if false && text.chars().all(space) && parents.last().is_some_and(|p| SHOWS_NO_TEXT.contains(&p.as_str())) {\n'),
    ("a text with words where none can be shown is nothing", SOURCE,
     '    if text.chars().all(space) && parents.last().is_some_and(|p| SHOWS_NO_TEXT.contains(&p.as_str())) {\n',
     '    if parents.last().is_some_and(|p| SHOWS_NO_TEXT.contains(&p.as_str())) {\n'),
    ("a cell is taken for its row", SOURCE,
     '        "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "datalist", "head",\n',
     '        "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "td", "head",\n'),
    ("a list is taken for a table", SOURCE,
     '        "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "datalist", "head",\n',
     '        "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "ul", "head",\n'),
    ("a non-breaking space is white space", SOURCE,
     "    let space = |c: char| matches!(c, ' ' | '\\t' | '\\n' | '\\r' | '\\u{c}');\n",
     "    let space = |c: char| c.is_whitespace();\n"),
    ("a carriage return is no white space", SOURCE,
     "    let space = |c: char| matches!(c, ' ' | '\\t' | '\\n' | '\\r' | '\\u{c}');\n",
     "    let space = |c: char| matches!(c, ' ' | '\\t' | '\\n' | '\\u{c}');\n"),
    ("the element is not noted", SOURCE,
     "    ix.parents.push(tag.to_string());\n", "    ix.parents.push(String::new());\n"),
    ("an element's end is not noted", SOURCE,
     "    ix.parents.pop();\n    ix.elements -= 1;\n", "    ix.elements -= 1;\n"),
]


def suite():
    """(built, passed, failed) of the template layout's tests."""
    run = subprocess.run(
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "template_layout"],
        cwd=ROOT, capture_output=True, text=True,
    )
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", run.stdout + run.stderr)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def main():
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = suite()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        return 1
    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = suite()
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
    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
