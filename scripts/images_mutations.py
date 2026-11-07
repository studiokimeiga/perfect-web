#!/usr/bin/env python3
"""Mutation controls for ADR-0404: an `<img>` says what it shows.

Each mutant undoes one part of PW5053's rule:
- an `<img>` with no `alt` is refused, wherever its view renders it, a loop
  included, and reported at the element, beside the declaration that
  renders it;
- any `alt` stands for the image: text, `""`, the attribute alone, or a
  value's;
- a `title`, an `aria-label` or a `role` does not;
- only an `<img>` is held to it: a `<picture>`'s `<source>` is not.

And the image button ADR-0405 adds (2026-11-06):
- an `<input type="image">`, its `type` read whatever its case and on an
  `<input>` alone, is refused where its `alt` is missing, blank or written
  alone: its `alt` is its label, so it is never blank, as an `<img>`'s may
  be;
- an `alt` a value gives names it, and a `title`, an `aria-label` or a
  `<label>` does not;
- it is reported at the element, beside the declaration that renders it,
  with a note at an empty `alt`, and its own reason and repair.

The tests in `images.rs` and the generality witnesses must then fail.

Run from the repository root. The source is restored after every mutant,
whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"

HAS_ALT = "    attrs.iter().any(|a| a.name == \"alt\")\n"
IMAGE = "                if tag.as_str() == \"img\" && !has_alt(attrs) {\n"
# The image button (ADR-0405).
IMAGE_BUTTON = (
    "                if tag.as_str() == \"input\"\n"
    "                    && input_type(attrs).as_deref() == Some(\"image\")\n"
)
BUTTON_ALT = "                    && names_with(attrs, \"alt\") != Some(true)\n"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "an image with no `alt` is not refused",
        CHECK,
        IMAGE,
        "                if false && tag.as_str() == \"img\" && !has_alt(attrs) {\n",
    ),
    (
        "an image a loop renders is not read",
        CHECK,
        IMAGE,
        "                if tag.as_str() == \"img\" && !has_alt(attrs) && !inside_each(body, &order).contains(&id) {\n",
    ),
    (
        "a `<picture>`'s `<source>` is held to it",
        CHECK,
        IMAGE,
        "                if matches!(tag.as_str(), \"img\" | \"source\") && !has_alt(attrs) {\n",
    ),
    (
        "a `title` stands for an image",
        CHECK,
        HAS_ALT,
        "    attrs.iter().any(|a| matches!(a.name.as_str(), \"alt\" | \"title\"))\n",
    ),
    (
        "an `aria-label` stands for an image",
        CHECK,
        HAS_ALT,
        "    attrs.iter().any(|a| matches!(a.name.as_str(), \"alt\" | \"aria-label\"))\n",
    ),
    (
        "a `role` stands for an image",
        CHECK,
        HAS_ALT,
        "    attrs.iter().any(|a| matches!(a.name.as_str(), \"alt\" | \"role\"))\n",
    ),
    (
        "an empty `alt` is refused",
        CHECK,
        HAS_ALT,
        "    attrs.iter().any(|a| a.name == \"alt\" && !matches!(a.value, AttrValue::None))\n"
        "        && static_attr(attrs, \"alt\").is_none_or(|v| !v.is_empty())\n",
    ),
    (
        "an `alt` a value gives is refused",
        CHECK,
        HAS_ALT,
        "    attrs.iter().any(|a| a.name == \"alt\" && !matches!(a.value, AttrValue::Expr(_)))\n",
    ),
    (
        "the image is reported at its declaration, not at the element",
        CHECK,
        "        primary_span: body.node_span(id),\n"
        "        related: vec![Related::new(\n"
        "            hir.decl_span(decl_id_of(hir, decl)),\n"
        "            format!(\"`{}` renders this image\", decl.name),\n",
        "        primary_span: hir.decl_span(decl_id_of(hir, decl)),\n"
        "        related: vec![Related::new(\n"
        "            hir.decl_span(decl_id_of(hir, decl)),\n"
        "            format!(\"`{}` renders this image\", decl.name),\n",
    ),
    (
        "the declaration that renders the image is not named",
        CHECK,
        "            format!(\"`{}` renders this image\", decl.name),\n",
        "            String::new(),\n",
    ),
    # --- the image button (ADR-0405) ---
    (
        "an image button with no text in its `alt` is not refused",
        CHECK,
        IMAGE_BUTTON,
        "                if false\n"
        "                    && input_type(attrs).as_deref() == Some(\"image\")\n",
    ),
    (
        "an image button's `type` is read case by case",
        CHECK,
        IMAGE_BUTTON,
        "                if tag.as_str() == \"input\"\n"
        "                    && static_attr(attrs, \"type\") == Some(\"image\")\n",
    ),
    (
        "`type=\"image\"` makes an image button of any element",
        CHECK,
        IMAGE_BUTTON,
        "                if true\n"
        "                    && input_type(attrs).as_deref() == Some(\"image\")\n",
    ),
    (
        "an image button may have an empty `alt`, as an `<img>` may",
        CHECK,
        BUTTON_ALT,
        "                    && names_with(attrs, \"alt\").is_none()\n",
    ),
    (
        "an `alt` a value gives does not name an image button",
        CHECK,
        BUTTON_ALT,
        "                    && static_attr(attrs, \"alt\").is_none_or(|v| v.trim().is_empty())\n",
    ),
    (
        "an `aria-label` stands for an image button's `alt`",
        CHECK,
        BUTTON_ALT,
        "                    && names_with(attrs, \"alt\") != Some(true)\n"
        "                    && names_with(attrs, \"aria-label\") != Some(true)\n",
    ),
    (
        "a `title` stands for an image button's `alt`",
        CHECK,
        BUTTON_ALT,
        "                    && names_with(attrs, \"alt\") != Some(true)\n"
        "                    && names_with(attrs, \"title\") != Some(true)\n",
    ),
    (
        "an image button that something names as a control needs no `alt`",
        CHECK,
        BUTTON_ALT,
        "                    && names_with(attrs, \"alt\") != Some(true)\n"
        "                    && !named.contains(&id)\n",
    ),
    (
        "the image button is reported at its declaration, not at the element",
        CHECK,
        "        .to_string(),\n        primary_span: body.node_span(id),\n        related,\n",
        "        .to_string(),\n"
        "        primary_span: hir.decl_span(decl_id_of(hir, decl)),\n"
        "        related,\n",
    ),
    (
        "the declaration that renders the image button is not named",
        CHECK,
        "        format!(\"`{}` renders this button\", decl.name),\n",
        "        String::new(),\n",
    ),
    (
        "an empty `alt` on an image button is not pointed at",
        CHECK,
        "    related.extend(alt.map(|a| {\n",
        "    related.extend(alt.filter(|_| false).map(|a| {\n",
    ),
    (
        "an image button's blank `alt` is reported as a missing one",
        CHECK,
        '            Some(_) => "`<input type=\\"image\\">` has a blank `alt`",\n',
        '            Some(_) => "`<input type=\\"image\\">` has no `alt`",\n',
    ),
    (
        "an image button is refused for an image's reason",
        CHECK,
        "        reason: \"image_button_without_alt\",\n",
        "        reason: \"image_without_alt\",\n",
    ),
    (
        "an image button is given an image's repair",
        CHECK,
        '            description: "write `alt=\\"…\\"` saying what the button does".to_string(),\n',
        '            description: "write `alt=\\"…\\"` saying what the image shows".to_string(),\n',
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "images"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "generality"],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if m is None:
            built = False
            continue
        passed += int(m.group(1))
        failed += int(m.group(2))
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed")
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
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
            built, passed, failed = run_tests()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
