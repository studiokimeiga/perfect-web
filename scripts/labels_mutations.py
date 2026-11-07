#!/usr/bin/env python3
"""Mutation controls for ADR-0143: what names a form control.

Each mutant undoes one part of PW5014's rule:
- an `id` names nothing by itself;
- a `<label for>` names the first element with that `id`, when the label has
  text, and an `id` inside `{#each}` names no one row;
- a `<label>` without `for` names its first labelable descendant, when which
  one is first does not depend on what renders;
- `aria-labelledby` must reach an element with text, and a blank
  `aria-label` names nothing.

And each part ADR-0405 adds (2026-11-06):
- an `<input>` button names itself: a `submit` or `reset` by its `value`, or
  with none by the label HTML gives it; a `button` by its `value` alone; a
  blank `value` names nothing;
- an image button is named by its `alt`, PW5053's to hold, and PW5014 does
  not report it a second time; a `hidden` input is no control;
- `type` is an `<input>`'s alone;
- a button is refused with a button's message, note, reason and repair.

The tests in `labels.rs` and the generality witnesses must then fail. The
browser's half, that each accepted way gives the control its name, is
`e2e/parsed-tree.spec.mjs` in the own-renderer suite.

Run from the repository root; `just e14-labels` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "an `id` names a control again",
        CHECK,
        "                    if named.contains(&id) {\n",
        "                    if named.contains(&id) || attrs.iter().any(|a| a.name == \"id\") {\n",
    ),
    (
        "a label with `for` is read as one that wraps",
        CHECK,
        "    if attrs.iter().any(|a| a.name == \"for\") {\n        let target",
        "    if attrs.iter().any(|a| a.name == \"for\") && false {\n        let target",
    ),
    (
        "a label that wraps its control names nothing",
        CHECK,
        "    first_labelable(body, children)\n",
        "    first_labelable(body, children).filter(|_| false)\n",
    ),
    (
        "a label with no text names its control",
        CHECK,
        "            && has_text(body, n, control)\n",
        "            && (has_text(body, n, control) || true)\n",
    ),
    (
        "the last element with an `id` is the one a label names",
        CHECK,
        "            by_id.entry(id).or_insert(n);\n",
        "            by_id.insert(id, n);\n",
    ),
    (
        "an `id` inside `{#each}` names no row",
        CHECK,
        "            by_id.entry(id).or_insert(n);\n        }\n    }\n    by_id\n",
        "            by_id.entry(id).or_insert(n);\n        }\n    }\n"
        "    let rows = inside_each(body, order);\n"
        "    by_id.retain(|_, n| !rows.contains(n));\n    by_id\n",
    ),
    (
        "a control inside a block is a label's first",
        CHECK,
        "                Some(true) if !in_block => return Ok(Some(n)),\n",
        "                Some(true) => return Ok(Some(n)),\n",
    ),
    (
        "a composed view holds no control",
        CHECK,
        "        return None;\n    }\n    Some(match tag {\n",
        "        return Some(false);\n    }\n    Some(match tag {\n",
    ),
    (
        "a hidden input is labelable",
        CHECK,
        "        \"input\" => !static_attr(attrs, \"type\").is_some_and(|t| t.eq_ignore_ascii_case(\"hidden\")),\n",
        "        \"input\" => true,\n",
    ),
    (
        "a control's own options are its label",
        CHECK,
        "        c != skip\n",
        "        true\n",
    ),
    (
        "hidden text names a control",
        CHECK,
        "                    !hidden\n",
        "                    true\n",
    ),
    # Since ADR-0405 an `aria-label`, a button's `value` and an
    # image button's `alt` are read by one helper, `names_with`.
    (
        "a blank `aria-label` names a control",
        CHECK,
        "        let labelled = names_with(attrs, \"aria-label\") == Some(true);\n",
        "        let labelled = names_with(attrs, \"aria-label\").is_some();\n",
    ),
    (
        "a name the program computes is refused: an `aria-label`'s, a `value`'s",
        CHECK,
        "        AttrValue::Expr(_) => true,\n        AttrValue::None => false,\n    })\n",
        "        AttrValue::Expr(_) => false,\n        AttrValue::None => false,\n    })\n",
    ),
    (
        "an attribute written alone names: an `aria-label`, a `value`",
        CHECK,
        "        AttrValue::Expr(_) => true,\n        AttrValue::None => false,\n    })\n",
        "        AttrValue::Expr(_) => true,\n        AttrValue::None => true,\n    })\n",
    ),
    (
        "a blank attribute names: an `aria-label`, a `value`",
        CHECK,
        "        AttrValue::Static(_) => non_blank(attrs, name),\n",
        "        AttrValue::Static(_) => true,\n",
    ),
    (
        "`aria-labelledby` names a control whatever it reaches",
        CHECK,
        "        if labelled || labelled_by(body, n, &by_id).is_some() || names_itself(tag, attrs) {\n",
        "        if labelled || attrs.iter().any(|a| a.name == \"aria-labelledby\") || names_itself(tag, attrs) {\n",
    ),
    (
        "an IDREF to an element with no text names a control",
        CHECK,
        "                        non_blank(attrs, \"aria-label\") || has_text(body, t, n)\n",
        "                        non_blank(attrs, \"aria-label\") || true\n",
    ),
    (
        "an input's `type` is read case by case",
        CHECK,
        "    static_attr(attrs, \"type\").map(str::to_ascii_lowercase)\n",
        "    static_attr(attrs, \"type\").map(str::to_string)\n",
    ),
    # --- ADR-0405 ---
    (
        "an image button is PW5014's again, refused for having no `<label>`",
        CHECK,
        "                    if matches!(typ.as_deref(), Some(\"hidden\" | \"image\")) {\n",
        "                    if matches!(typ.as_deref(), Some(\"hidden\")) {\n",
    ),
    (
        "a hidden input is a control",
        CHECK,
        "                    if matches!(typ.as_deref(), Some(\"hidden\" | \"image\")) {\n",
        "                    if matches!(typ.as_deref(), Some(\"image\")) {\n",
    ),
    (
        "a `<select>`'s or a `<textarea>`'s `type` is read",
        CHECK,
        "                    let typ = (tag.as_str() == \"input\")\n"
        "                        .then(|| input_type(attrs))\n"
        "                        .flatten();\n",
        "                    let typ = input_type(attrs);\n",
    ),
    (
        "a button names itself whatever element it is",
        CHECK,
        "    let default = match (tag, input_type(attrs).as_deref()) {\n",
        "    let default = match (\"input\", input_type(attrs).as_deref()) {\n",
    ),
    (
        "a `submit` or `reset` with no `value` has no label",
        CHECK,
        "        (\"input\", Some(\"submit\" | \"reset\")) => true,\n",
        "        (\"input\", Some(\"submit\" | \"reset\")) => false,\n",
    ),
    (
        "a `button` input with no `value` has a label",
        CHECK,
        "        (\"input\", Some(\"button\")) => false,\n",
        "        (\"input\", Some(\"button\")) => true,\n",
    ),
    (
        "a button's `value` is not read",
        CHECK,
        "    names_with(attrs, \"value\").unwrap_or(default)\n",
        "    default\n",
    ),
    (
        "a blank `value` names a button",
        CHECK,
        "    names_with(attrs, \"value\").unwrap_or(default)\n",
        "    names_with(attrs, \"value\").is_some() || default\n",
    ),
    (
        "a button is refused with a field's message, reason and repair",
        CHECK,
        "                    let button = matches!(typ.as_deref(), Some(\"submit\" | \"reset\" | \"button\"));\n",
        "                    let button = false;\n",
    ),
    (
        "a button is refused with a field's message",
        CHECK,
        "                            Some(t) if button => format!(\"`<input type=\\\"{t}\\\">` has no label\"),\n",
        "                            Some(_) if false => String::new(),\n",
    ),
    (
        "the note does not say a button's `value` is blank",
        CHECK,
        "        if let Some(a) = attr(\"value\") {\n",
        "        if let Some(a) = attr(\"value\").filter(|_| false) {\n",
    ),
    (
        "the note does not say a `button` input has no `value`",
        CHECK,
        "        if let Some(a) = attr(\"type\") {\n",
        "        if let Some(a) = attr(\"type\").filter(|_| false) {\n",
    ),
    (
        "a field is given a button's note",
        CHECK,
        "        && matches!(t.as_str(), \"submit\" | \"reset\" | \"button\")\n    {\n",
        "        && !t.is_empty()\n    {\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "labels"],
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
