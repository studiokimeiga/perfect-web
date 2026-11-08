#!/usr/bin/env python3
"""Mutation controls for ADR-0343: a guide whose examples compile.

The guide's mechanism is a test (`compiler/pw-cli/tests/guide.rs`), a
generator of the reference of codes (`scripts/guide_diagnostics.py`, held by
`scripts/tests/test_guide_diagnostics.py`) and a browser test of the
tutorial's app (`spikes/own-renderer/e2e/guestbook.spec.mjs`). Each mutant
undoes one thing they hold, and the test that holds it must fail, by name:

- the guide rots: an example that no longer type-checks, is not formatted as
  `pw fmt` writes it, checks and does not build, is refused no more or by
  another code; output a page shows that the compiler no longer prints; a
  fence with no info string, a language the guide is not written in, an
  indented fence; the tutorial differing from the app by a byte or a file;
  the reference of codes without a row, with another invariant, with a row
  too many; a gap in the checks that the compiler has closed, so that the
  page still says "not yet";
- the test's own logic: each check it makes, dropped or weakened, and the
  guards that stand between a block and its check (a page with no example,
  a ```pw block the extractor did not see, the program a page names);
- the generator of the reference: a code, a comment, a pipe, a retired code;
- the app the tutorial builds: its draft not cleared, a Remove on another
  reader's entry, its two halves swapped, the entry shown last, or removable
  while pending, a reader not told. Those the checker refuses by a rule are
  named in `CHECKED`: the checker, not the browser, is their protection.

A mutant is killed when a test it names fails (`MUST`): killed by another
test, or by a build that fails, is not what the mutant is for, and counts as
a survivor. The unmutated baseline must be green first.

Run from the repository root; `just e14-guide` records the output. The
source is restored after every mutant, whatever happens. A browser mutant
builds the app (`spikes/own-renderer/guestbook.sh`) and runs the spec in
Chromium with no retries and a complete assertion receipt, on the PORT the
environment gives (the base default 3141 where it gives none).
"""

import os
import json
import secrets
import signal
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))
import guide_python
import cargo_assertion_results
import browser_test_results
import mutation_source
import producer_process
TEST = ROOT / "compiler/pw-cli/tests/guide.rs"
GENERATOR = ROOT / "scripts/guide_diagnostics.py"
GUIDE = ROOT / "docs/guide"
TOUR = GUIDE / "tour.md"
TOUR_PAGES = GUIDE / "tour-pages.md"
ERRORS = GUIDE / "errors.md"
TUTORIAL = GUIDE / "tutorial.md"
REFERENCE = GUIDE / "diagnostics.md"
BOOK = ROOT / "examples/guestbook/book.pw"
PAGES = ROOT / "examples/guestbook/pages.pw"

GUIDE_TEST = "every_example_in_the_guide_compiles_as_it_says"
VERDICT = "a_wrong_example_is_reported_and_a_right_one_is_not"
FENCES = "a_fence_is_read_as_commonmark_reads_it"
UNCLOSED = "an_unclosed_or_indented_fence_is_refused"
INFO = "an_info_string_says_what_a_block_is"
PIECES = "a_pages_file_blocks_are_its_program_in_order"
SHOWN = "shown_output_belongs_to_the_refused_example_before_it"
BUILD_SHOWN = "a_gaps_build_output_belongs_to_the_gap_before_it"
SURVEY = "a_page_with_no_example_or_a_block_unseen_is_reported"
DIRECTORY = "a_pages_program_is_the_directory_it_names"
DIRECTORY_HELD = "a_page_is_held_to_the_directory_it_names"
DIFFERS = "a_program_differs_from_a_directory_by_a_file_or_a_byte"
REFERENCE_TEST = "the_diagnostic_reference_lists_every_registered_code"
REFERENCE_HELD = "the_reference_is_held_to_the_registry"
CURRENT = "test_the_committed_page_is_what_the_registry_writes"
READING = "test_each_entry_with_its_invariant_and_the_adrs_its_comment_cites"
REFUSED_LINE = "test_a_line_that_is_no_entry_is_refused_not_skipped"
PIPE = "test_a_pipe_in_a_cell_is_escaped"
RETIRED = "test_a_retired_code_with_its_reason_continued_across_lines"

# The mutants the checker refuses, by the rule that is the protection each
# takes away (`scripts/mutant_programs.py`): the checker, not a test, holds
# them, and the program they leave is not built. Every other mutant of a
# `.pw` source here leaves a program that checks and builds.
CHECKED = {
    "the composer is shown to a reader signed out": {"PW5048"},
    "Remove is shown to a reader signed out": {"PW5048"},
}

# (what, where it is held, file, anchor, replacement). Where: "guide" is
# the guide's test and the generator's, "browser" the spec on a build of the
# app. MUST names, for each mutant, the tests that must fail.
MUTANTS = [
    # --- the guide rots: its pages ---------------------------------------
    (
        "an example no longer type-checks",
        "guide",
        TOUR,
        "fn total(prices: List<Int>) -> Int !{} {",
        "fn total(prices: List<Int>) -> String !{} {",
    ),
    (
        "an example is not as `pw fmt` writes it",
        "guide",
        TOUR,
        "    let count: Int = 5_000_000\n",
        "  let count: Int = 5_000_000\n",
    ),
    (
        "an example checks clean and `pw build` refuses it",
        "guide",
        TOUR_PAGES,
        "        <title>Counter</title>",
        "        <title>Pressed {count}</title>",
    ),
    (
        "a refused example is refused no more",
        "guide",
        TOUR,
        "    open & count > 0 | !open\n",
        "    (open & count > 0) | !open\n",
    ),
    (
        "a refused example is refused by another code",
        "guide",
        TOUR,
        "```pw fails=PW0031",
        "```pw fails=PW0032",
    ),
    (
        "output a page shows is not what `pw check` prints",
        "guide",
        ERRORS,
        "  = help: produce a `String` here, or change the declared result\n",
        "  = help: produce a `String` here\n",
    ),
    (
        "a block says nothing of what it is",
        "guide",
        ERRORS,
        "```pw fails=PW0009\n",
        "```\n",
    ),
    (
        "a block is in a language the guide is not written in",
        "guide",
        TOUR,
        "```sh\ntarget/debug/pw check packages",
        "```bash\ntarget/debug/pw check packages",
    ),
    (
        "a fence is indented",
        "guide",
        TOUR,
        "```pw fails=PW0031",
        " ```pw fails=PW0031",
    ),
    (
        "a fence is never closed",
        "guide",
        TOUR,
        "    open & count > 0 | !open\n}\n```\n",
        "    open & count > 0 | !open\n}\n``` x\n",
    ),
    (
        "a page's files no longer check together",
        "guide",
        TOUR,
        "import shop.money.{ Cents, cents, added, display }",
        "import shop.money.{ Cents, cents, added, shown }",
    ),
    (
        "the tutorial and the app differ by a byte",
        "guide",
        PAGES,
        "            <h1>Guestbook</h1>\n",
        "            <h1>Guest book</h1>\n",
    ),
    (
        "the tutorial builds a file the app does not have",
        "guide",
        TUTORIAL,
        "```pw file=guestbook/pages.pw\nmodule guestbook.pages\n",
        "```pw file=guestbook/page.pw\nmodule guestbook.pages\n",
    ),
    (
        'the closed title gap claims check still accepts it',
        'guide',
        TOUR_PAGES,
        '```pw fails=PW6601\nmodule shop.titled',
        '```pw gap=build\nmodule shop.titled',
    ),
    (
        "a build gap is closed: the build accepts it",
        "guide",
        TOUR_PAGES,
        "        <title>Pressed {count}</title>\n        <button",
        "        <title>Pressed</title>\n        <button",
    ),
    # Until ADR-0404 the guide's one `gap=check` was an `<img>` with no
    # `alt`, and this mutant closed it by making that `<img>` an unlabelled
    # `<input>`. PW5053 closed it for real; a page that calls a program the
    # check refuses a check gap is the same verdict.
    (
        "a check gap is closed: the check refuses it",
        "guide",
        TOUR_PAGES,
        '```pw fails=PW6601\nmodule shop.titled',
        '```pw gap=check\nmodule shop.titled',
    ),
    (
        'the closed two-signal gap is claimed to be refused still',
        'guide',
        TOUR_PAGES,
        '```pw\nmodule shop.sum',
        '```pw fails=PW6601\nmodule shop.sum',
    ),
    # --- the guide rots: the reference of codes --------------------------
    (
        "the reference has no row for a code",
        "guide",
        REFERENCE,
        "| PW0606 | `return_type` | a body must produce the result type its signature declares | — |\n",
        "",
    ),
    (
        "the reference gives a code another invariant",
        "guide",
        REFERENCE,
        "| PW0606 | `return_type` | a body must produce the result type its signature declares | — |\n",
        "| PW0606 | `return_type` | a body is whatever it produces | — |\n",
    ),
    (
        "the reference has a row for a code the compiler lacks",
        "guide",
        REFERENCE,
        "| PW0606 | `return_type` | a body must produce the result type its signature declares | — |\n",
        "| PW0606 | `return_type` | a body must produce the result type its signature declares | — |\n"
        "| PW9999 | `lacks` | a code the compiler lacks | — |\n",
    ),
    # --- the test: the checks of one example ----------------------------
    (
        "a clean example need not check clean",
        "guide",
        TEST,
        '            if code != Some(0) || !out.contains("no diagnostics") {\n'
        '                return Some(format!("expected no diagnostics:\\n{said}"));',
        '            if false {\n                return Some(format!("expected no diagnostics:\\n{said}"));',
    ),
    (
        "an example need not be as `pw fmt` writes it",
        "guide",
        TEST,
        "    if fmt_code != Some(0) && (parses || matches!(example.expect, Expect::Clean)) {",
        "    if false && (parses || matches!(example.expect, Expect::Clean)) {",
    ),
    (
        "an example that parses is held to the format only where it is clean",
        "guide",
        TEST,
        "    if fmt_code != Some(0) && (parses || matches!(example.expect, Expect::Clean)) {",
        "    if fmt_code != Some(0) && matches!(example.expect, Expect::Clean) {",
    ),
    (
        "a clean example need not build",
        "guide",
        TEST,
        "            let (built, build_out, build_err) = build(&names, &packages, scratch);\n"
        "            if built != Some(0) {",
        "            let (built, build_out, build_err) = build(&names, &packages, scratch);\n"
        "            if false && built != Some(0) {",
    ),
    (
        "a build gap may build",
        "guide",
        TEST,
        "                if built == Some(0) {",
        "                if false {",
    ),
    (
        "a gap's build output need not be what is shown",
        "guide",
        TEST,
        "                    if printed.trim_end() != shown.trim_end() {\n"
        "                        return Some(format!(\n"
        '                            "the page shows output `pw build`',
        "                    if false {\n"
        "                        return Some(format!(\n"
        '                            "the page shows output `pw build`',
    ),
    (
        "a refused example need not be refused",
        "guide",
        TEST,
        "            if code != Some(1) {",
        "            if false {",
    ),
    (
        "a refusal's codes are compared as a subset of what is reported",
        "guide",
        TEST,
        "            if &found != codes {",
        "            if !codes.is_subset(&found) {",
    ),
    (
        "a refusal's codes are compared as a superset of what is reported",
        "guide",
        TEST,
        "            if &found != codes {",
        "            if !found.is_subset(codes) {",
    ),
    (
        "output a page shows of a refusal need not be what is printed",
        "guide",
        TEST,
        "                if printed.trim_end() != shown.trim_end() {\n"
        "                    return Some(format!(\n"
        '                        "the page shows output `pw check`',
        "                if false {\n"
        "                    return Some(format!(\n"
        '                        "the page shows output `pw check`',
    ),
    (
        "output a page shows keeps the closing count `pw check` prints",
        "guide",
        TEST,
        '        .filter(|l| !l.starts_with("pw check: "))',
        "        .filter(|_| true)",
    ),
    # --- the test: fences and info strings ------------------------------
    (
        "a fence of another character closes a block",
        "guide",
        TEST,
        ".is_some_and(|(d, l)| d == *c && l >= *len && line[l..].trim().is_empty());",
        ".is_some_and(|(_d, l)| l >= *len && line[l..].trim().is_empty());",
    ),
    (
        "a closing fence may carry text",
        "guide",
        TEST,
        ".is_some_and(|(d, l)| d == *c && l >= *len && line[l..].trim().is_empty());",
        ".is_some_and(|(d, l)| d == *c && l >= *len);",
    ),
    (
        "a shorter fence closes a block",
        "guide",
        TEST,
        ".is_some_and(|(d, l)| d == *c && l >= *len && line[l..].trim().is_empty());",
        ".is_some_and(|(d, l)| d == *c && l >= 3 && line[l..].trim().is_empty());",
    ),
    (
        "an indented fence is accepted",
        "guide",
        TEST,
        "                    if trimmed.len() != line.len() {\n"
        '                        return Err(format!("line {n}: an indented fence"));\n'
        "                    }\n",
        "",
    ),
    (
        "a fence never closed is accepted",
        "guide",
        TEST,
        '        return Err(format!("line {at}: a fence never closed"));',
        "        return Ok(out);",
    ),
    (
        "two backticks are a fence",
        "guide",
        TEST,
        "    (len >= 3).then_some((c, len))",
        "    (len >= 2).then_some((c, len))",
    ),
    (
        "an attribute a block does not take is skipped",
        "guide",
        TEST,
        '                _ => return Err(format!("`{a}`: not an attribute a `pw` block takes")),',
        "                _ => {}",
    ),
    (
        "an attribute may be given twice",
        "guide",
        TEST,
        '                Some(("fails", codes)) if fails.is_none() => {',
        '                Some(("fails", codes)) => {',
    ),
    (
        "a file may be outside the page's tree",
        "guide",
        TEST,
        "                    if !name.ends_with(\".pw\") || name.starts_with('/') || name.contains(\"..\") {",
        '                    if !name.ends_with(".pw") {',
    ),
    (
        "a block may be a gap and a refusal",
        "guide",
        TEST,
        "            (None, None, Some(gap)) => Ok(Kind::Gap(gap)),",
        "            (_, _, Some(gap)) => Ok(Kind::Gap(gap)),",
    ),
    (
        "another language's block takes attributes",
        "guide",
        TEST,
        "    if !attributes.is_empty() {",
        "    if false {",
    ),
    (
        "a language the guide is not written in passes",
        "guide",
        TEST,
        "    if !OTHER_LANGUAGES.contains(&language) {",
        "    if false {",
    ),
    (
        "a page's pieces are joined with no blank line",
        "guide",
        TEST,
        "                if !piece.is_empty() {\n                    piece.push('\\n');\n                }\n",
        "",
    ),
    (
        "output belongs to any block before it",
        "guide",
        TEST,
        "                let follows = i > 0 && matches!(kind(&found[i - 1].info), Ok(Kind::Fails(_)));",
        "                let follows = i > 0;",
    ),
    (
        "a build's output belongs to any block before it",
        "guide",
        TEST,
        "                let follows =\n"
        "                    i > 0 && matches!(kind(&found[i - 1].info), Ok(Kind::Gap(Gap::Build)));",
        "                let follows = i > 0;",
    ),
    (
        "a build gap need not show what `pw build` prints",
        "guide",
        TEST,
        "        if matches!(e.expect, Expect::Gap(Gap::Build, None)) {",
        "        if false {",
    ),
    # --- the test: what stands between a block and its check ------------
    (
        "a page with no example is accepted",
        "guide",
        TEST,
        "        if opening == 0 && !UNEXAMPLED.contains(&page.as_str()) {",
        "        if false {",
    ),
    (
        "a Pleris block the reader of pages did not see goes unnoticed",
        "guide",
        TEST,
        "                if pieces != opening {",
        "                if false {",
    ),
    (
        "a page's pieces are not counted",
        "guide",
        TEST,
        "                    + if program { file_blocks(md) } else { 0 };",
        "                    + 0;",
    ),
    (
        "the index is held to show Pleris",
        "guide",
        TEST,
        'const UNEXAMPLED: &[&str] = &["README.md", "diagnostics.md"];',
        'const UNEXAMPLED: &[&str] = &["diagnostics.md"];',
    ),
    (
        "a page's program is not compared with the directory it names",
        "guide",
        TEST,
        '    differs(&assembled, on_disk).map(|why| format!("{page} and its directory: {why}"))',
        '    differs(&assembled, on_disk)\n        .filter(|_| false)\n        .map(|why| format!("{page} and its directory: {why}"))',
    ),
    (
        "a page that names a directory and builds nothing in it is accepted",
        "guide",
        TEST,
        '        return Some(format!("{page} names a directory and has no `file=` block"));',
        "        return None;",
    ),
    (
        "the directory a page names is read with its spaces",
        "guide",
        TEST,
        "            .map(|dir| dir.trim().to_string())",
        "            .map(|dir| dir.to_string())",
    ),
    (
        "a file the page lacks is not noticed",
        "guide",
        TEST,
        "    if page != dir {",
        "    if false {",
    ),
    (
        "trailing space is no difference",
        "guide",
        TEST,
        "**text != *other",
        "text.trim_end() != other.trim_end()",
    ),
    (
        "a row's invariant is not compared with the registry's",
        "guide",
        TEST,
        "            Some(row) if row.contains(&escaped(invariant)) => {}",
        "            Some(row) if row.contains(&escaped(invariant)) || true => {}",
    ),
    (
        "a code with no row is not reported",
        "guide",
        TEST,
        '            None => problems.push(format!("{id}: no row")),',
        "            None => {}",
    ),
    (
        "a row for a code the compiler lacks is not reported",
        "guide",
        TEST,
        "    if found != rows {",
        "    if false {",
    ),
    # --- the generator of the reference ---------------------------------
    (
        "an ADR a comment cites twice is listed twice",
        "guide",
        GENERATOR,
        "            if n not in adrs:",
        "            if True:",
    ),
    (
        "a comment is taken by the entry after the next",
        "guide",
        GENERATOR,
        "        out.append((code, symbol, int(revision), owner, invariant, adrs))\n        comment = []\n",
        "        out.append((code, symbol, int(revision), owner, invariant, adrs))\n",
    ),
    (
        "a line that is no entry is skipped",
        "guide",
        GENERATOR,
        '        if not m:\n            raise ValueError(f"codes.rs:{i + 1}: not an entry: {line!r}")\n',
        "        if not m:\n            i += 1\n            continue\n",
    ),
    (
        "a pipe in a cell ends it",
        "guide",
        GENERATOR,
        '    return text.replace("|", "\\\\|")',
        "    return text",
    ),
    (
        "a retired code's reason loses its continuation",
        "guide",
        GENERATOR,
        "        text = re.sub(r\"\\\\\\n\\s*\", \"\", text).replace('\\\\\"', '\"')",
        "        text = text.replace('\\\\\"', '\"')",
    ),
    (
        "the retired codes are left out of the page",
        "guide",
        GENERATOR,
        '    out += [f"| {code} | {cell(why)} |" for code, why in gone]',
        "    out += []",
    ),
    (
        "the last code of each owner is left out of the page",
        "guide",
        GENERATOR,
        "        for code, symbol, _rev, _owner, invariant, adrs in rows:",
        "        for code, symbol, _rev, _owner, invariant, adrs in rows[:-1]:",
    ),
    # --- the app the tutorial builds, in a browser ---------------------
    (
        "a signed entry does not clear the draft",
        "browser",
        PAGES,
        '                            draft = ""\n',
        "                            draft = draft\n",
    ),
    (
        "a refused signing is said nowhere",
        "browser",
        PAGES,
        "                        },\n                        Err(error) => said = refused(error),\n",
        "                        },\n                        Err(_) => (),\n",
    ),
    (
        "Remove is offered on another reader's entry and not on one's own",
        "browser",
        PAGES,
        "{#if entry.mine}",
        "{#if !entry.mine}",
    ),
    (
        "a reader signed out is shown the signed-in half",
        "browser",
        PAGES,
        "            {#if me.signed_in}\n",
        "            {#if !me.signed_in}\n",
    ),
    (
        "the entry is shown last, not first, before the server answers",
        "browser",
        BOOK,
        "    List.concat([entry], shown)\n",
        "    List.concat(shown, [entry])\n",
    ),
    (
        "the entry can be removed before the server has it",
        "browser",
        BOOK,
        "text: text_of(text), mine: false }",
        "text: text_of(text), mine: true }",
    ),
    (
        "the composer is shown to a reader signed out",
        "browser",
        PAGES,
        "<form hidden={!SignedIn} on:submit",
        "<form on:submit",
    ),
    (
        "Remove is shown to a reader signed out",
        "browser",
        PAGES,
        "{#if SignedIn}{#if entry.mine}",
        "{#if entry.mine}{#if entry.mine}",
    ),
    # A signing tells its own reader's entries and no one else's: well-typed
    # (the speculated `Latest` is still the command's to invalidate, so
    # PW5107 holds), and left to the browser to kill (the integrator's ruling
    # of 2026-11-02). Until then it took `Signed(_)` out of `invalidates_on`,
    # which PW5107 refuses: a checker's refusal of the speculation, not a
    # witness that the other pages are told.
    (
        "the entries are not told of a signed entry",
        "browser",
        BOOK,
        "    emits         Signed(guestbook())\n",
        "    invalidates   Latest(current_user(), _)\n",
    ),
]

# The test or tests that must fail for each mutant, by name. A test of the
# browser spec is named by a part of its title.
MUST = {
    "an example no longer type-checks": {GUIDE_TEST},
    "an example is not as `pw fmt` writes it": {GUIDE_TEST},
    "an example checks clean and `pw build` refuses it": {GUIDE_TEST},
    "a refused example is refused no more": {GUIDE_TEST},
    "a refused example is refused by another code": {GUIDE_TEST},
    "output a page shows is not what `pw check` prints": {GUIDE_TEST},
    "a block says nothing of what it is": {GUIDE_TEST},
    "a block is in a language the guide is not written in": {GUIDE_TEST},
    "a fence is indented": {GUIDE_TEST},
    "a fence is never closed": {GUIDE_TEST},
    "a page's files no longer check together": {GUIDE_TEST},
    "the tutorial and the app differ by a byte": {DIRECTORY},
    "the tutorial builds a file the app does not have": {DIRECTORY},
    'the closed two-signal gap is claimed to be refused still': {GUIDE_TEST},
    "a build gap is closed: the build accepts it": {GUIDE_TEST},
    "a check gap is closed: the check refuses it": {GUIDE_TEST},
    'the closed title gap claims check still accepts it': {GUIDE_TEST},
    "the reference has no row for a code": {REFERENCE_TEST, CURRENT},
    "the reference gives a code another invariant": {REFERENCE_TEST, CURRENT},
    "the reference has a row for a code the compiler lacks": {REFERENCE_TEST, CURRENT},
    "a clean example need not check clean": {VERDICT},
    "an example need not be as `pw fmt` writes it": {VERDICT},
    "an example that parses is held to the format only where it is clean": {VERDICT},
    "a clean example need not build": {VERDICT},
    "a build gap may build": {VERDICT},
    "a gap's build output need not be what is shown": {VERDICT},
    "a refused example need not be refused": {VERDICT},
    "a refusal's codes are compared as a subset of what is reported": {VERDICT},
    "a refusal's codes are compared as a superset of what is reported": {VERDICT},
    "output a page shows of a refusal need not be what is printed": {VERDICT},
    "output a page shows keeps the closing count `pw check` prints": {GUIDE_TEST},
    "a fence of another character closes a block": {UNCLOSED},
    "a closing fence may carry text": {UNCLOSED},
    "a shorter fence closes a block": {SURVEY},
    "an indented fence is accepted": {UNCLOSED},
    "a fence never closed is accepted": {UNCLOSED},
    "two backticks are a fence": {FENCES},
    "an attribute a block does not take is skipped": {INFO},
    "an attribute may be given twice": {INFO},
    "a file may be outside the page's tree": {INFO},
    "a block may be a gap and a refusal": {INFO},
    "another language's block takes attributes": {INFO},
    "a language the guide is not written in passes": {INFO},
    "a page's pieces are joined with no blank line": {PIECES},
    "output belongs to any block before it": {SHOWN},
    "a build's output belongs to any block before it": {BUILD_SHOWN},
    "a build gap need not show what `pw build` prints": {BUILD_SHOWN},
    "a page with no example is accepted": {SURVEY},
    "a Pleris block the reader of pages did not see goes unnoticed": {SURVEY},
    "a page's pieces are not counted": {SURVEY, GUIDE_TEST},
    "the index is held to show Pleris": {SURVEY, GUIDE_TEST},
    "a page's program is not compared with the directory it names": {DIRECTORY_HELD},
    "a page that names a directory and builds nothing in it is accepted": {DIRECTORY_HELD},
    "the directory a page names is read with its spaces": {DIRECTORY_HELD},
    "a file the page lacks is not noticed": {DIFFERS},
    "trailing space is no difference": {DIFFERS},
    "a row's invariant is not compared with the registry's": {REFERENCE_HELD},
    "a code with no row is not reported": {REFERENCE_HELD},
    "a row for a code the compiler lacks is not reported": {REFERENCE_HELD},
    "an ADR a comment cites twice is listed twice": {READING},
    "a comment is taken by the entry after the next": {READING},
    "a line that is no entry is skipped": {REFUSED_LINE},
    "a pipe in a cell ends it": {PIPE},
    "a retired code's reason loses its continuation": {RETIRED},
    "the retired codes are left out of the page": {CURRENT},
    "the last code of each owner is left out of the page": {CURRENT},
    "a signed entry does not clear the draft": {"signs, and the entry is theirs alone to remove"},
    "a refused signing is said nowhere": {"a signing the server refuses is said"},
    "Remove is offered on another reader's entry and not on one's own": {
        "signs, and the entry is theirs alone to remove"
    },
    "a reader signed out is shown the signed-in half": {"a signed-out reader reads the entries"},
    "the entry is shown last, not first, before the server answers": {
        "an entry is shown before the server answers"
    },
    "the entry can be removed before the server has it": {
        "an entry is shown before the server answers"
    },
    "the composer is shown to a reader signed out": set(),
    "Remove is shown to a reader signed out": set(),
    "the entries are not told of a signed entry": {"another reader's open page is told of an entry"},
}

CARGO_TEST = ["cargo", "test", "--locked", "-p", "pw-cli", "--test", "guide", "--", "--format=pretty"]
CARGO_NAMES = {GUIDE_TEST, VERDICT, FENCES, UNCLOSED, INFO, PIECES, SHOWN,
               BUILD_SHOWN, SURVEY, DIRECTORY, DIRECTORY_HELD, DIFFERS,
               REFERENCE_TEST, REFERENCE_HELD,
               "a_refused_examples_codes_are_read_from_its_output"}
BOUND = 1200
_BROWSER_TITLES = None
PYTHON_TEST = [
    "python3",
    "-m",
    "unittest",
    "discover",
    "-s",
    "scripts/tests",
    "-p",
    "test_guide_diagnostics.py",
]
APP_BUILD = ["bash", "spikes/own-renderer/guestbook.sh"]
SPEC = [
    "pnpm",
    "exec",
    "playwright",
    "test",
    "e2e/guestbook.spec.mjs",
    "--project=chromium",
    "--retries=0",
    "--reporter=line,json,./assertion-result-reporter.mjs",
]


def run(cmd, cwd=ROOT, env=None):
    try:
        result = producer_process.run(cmd, cwd=cwd, env=env, timeout=BOUND)
        return result.returncode, re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", result.stdout)
    except subprocess.TimeoutExpired as expired:
        return None, expired.output or "producer timed out"


def _stopped(watch):
    return watch is not None and bool(watch.stopped)


class CompilerRefusal(tuple):
    """The actual app compiler exited1 with its complete error-code set."""
    def __new__(cls, codes, output):
        value = super().__new__(cls, (False, 0, 0, set(), output))
        value.codes = frozenset(codes)
        return value


PYTHON_SELECTOR = pathlib.Path(PYTHON_TEST[-1]).stem
PYTHON_SEARCH_DIR = ROOT / "scripts/tests"
PYTHON_PATTERN = PYTHON_TEST[-1]
PYTHON_EXPECTED_COUNT = 7
_PYTHON_EXPECTED = None
_PYTHON_EVIDENCE_ROOT = None


class GuideResult(tuple):
    def __new__(cls, ran, failed, names, output, cargo_failed=(), python_result=None):
        value = super().__new__(cls, (ran, failed, names, output))
        value.cargo_failed = frozenset(cargo_failed)
        value.python_result = python_result
        return value


def freeze_python():
    global _PYTHON_EVIDENCE_ROOT
    _PYTHON_EVIDENCE_ROOT = pathlib.Path(tempfile.mkdtemp(prefix="pw-guide-python-evidence-"))
    expected = guide_python.freeze(PYTHON_SELECTOR, PYTHON_SEARCH_DIR, PYTHON_PATTERN,
                                   PYTHON_EXPECTED_COUNT, _PYTHON_EVIDENCE_ROOT)
    if len({identity.rsplit(".", 1)[-1] for identity in expected}) != len(expected):
        raise guide_python.DiscoveryRefused("bare MUST names would be ambiguous")
    print(f"Python frozen full IDs: {expected}; evidence {_PYTHON_EVIDENCE_ROOT}", flush=True)
    return expected


def python_tests():
    if _PYTHON_EXPECTED is None:
        result = guide_python.unittest_protocol.Result(False, "Python inventory was not frozen")
        return guide_python.PythonRun(result, result.reason, ROOT, None)
    return guide_python.run(PYTHON_SELECTOR, _PYTHON_EXPECTED, PYTHON_SEARCH_DIR, _PYTHON_EVIDENCE_ROOT)


def guide_tests():
    """Cargo tests plus one canonical, provenance-validated Python result."""
    code, out = run(CARGO_TEST)
    outputs = [out]
    cargo = cargo_assertion_results.validate(code, out, root=ROOT, sources=[TEST], expected=CARGO_NAMES)
    if not cargo.valid:
        outputs.append("Cargo result refused: " + (cargo.reason or "incomplete inventory or non-assertion failure"))
        return GuideResult(None, 0, set(), "\n".join(outputs))
    ran = cargo.passed + cargo.failed
    failed = cargo.failed
    cargo_failed = set(cargo.failure_names)
    python = python_tests()
    outputs.append(python.output)
    outcome = python.result
    if not outcome.valid or outcome.body_errors:
        return GuideResult(None, 0, set(), "\n".join(outputs), cargo_failed, outcome)
    # Full expected IDs and actual body provenance were validated by the shared
    # authority before any bare MUST name is considered here.
    names = cargo_failed | {identity.rsplit(".", 1)[-1] for identity in outcome.assertion_failed}
    return GuideResult(ran + outcome.executed, failed + len(outcome.assertion_failed), names,
                       "\n".join(outputs), cargo_failed, outcome)


def failure_categories(result, intended):
    categories = []
    if result.cargo_failed & intended:
        categories.append("Rust body assertion")
    python = result.python_result
    if python is not None:
        if {name.rsplit(".", 1)[-1] for name in python.assertion_failed} & intended:
            categories.append("Python body assertion")
    return ", ".join(categories)


def browser_titles(metadata, inventory):
    """Only identity/title metadata from the same invocation's built-in JSON.

    Status and assertion provenance come exclusively from the nonce receipt.
    The pinned JSON reporter assigns each spec the same TestCase.id used by
    our reporter. Chromium-only selection requires exactly one project test.
    """
    titles = {}
    def visit(suites, parents=()):
        if not isinstance(suites, list):
            raise ValueError("browser metadata suites are missing")
        for suite in suites:
            if not isinstance(suite, dict) or not isinstance(suite.get("title"), str) or not suite["title"]:
                raise ValueError("browser metadata suite title is missing")
            path = (*parents, suite["title"])
            specs = suite.get("specs", [])
            if not isinstance(specs, list):
                raise ValueError("browser metadata specs are missing")
            for spec in specs:
                if not isinstance(spec, dict):
                    raise ValueError("browser metadata spec is invalid")
                identity, title, tests = spec.get("id"), spec.get("title"), spec.get("tests")
                if (not isinstance(identity, str) or not identity or identity in titles
                        or not isinstance(title, str) or not title
                        or not isinstance(tests, list) or len(tests) != 1
                        or not isinstance(tests[0], dict) or tests[0].get("projectName") != "chromium"):
                    raise ValueError("browser metadata identity/title/project is invalid or duplicate")
                titles[identity] = " › ".join((*path, title))
            visit(suite.get("suites", []), path)
    if not isinstance(metadata, dict):
        raise ValueError("browser metadata is missing")
    visit(metadata.get("suites"))
    if set(titles) != set(inventory) or len(titles) != 5:
        raise ValueError("browser metadata differs from complete five-test receipt")
    return titles


def compiler_codes(code, output):
    """A complete plain `pw check` refusal, before guestbook.sh can build."""
    if type(code) is not int or code != 1:
        return set()
    lines = output.splitlines()
    if not lines:
        return set()
    summary = re.fullmatch(r"pw check: (\d+) file\(s\), (\d+) error\(s\), 0 warning\(s\)", lines[-1])
    files = sum(len(list((ROOT / directory).glob("*.pw"))) for directory in
                ("packages/pw-std", "packages/pw-platform-web", "examples/guestbook"))
    errors = [line for line in lines if line.startswith("error:")]
    codes = [re.fullmatch(r"error: \[(PW\d{4})\] .+", line) for line in errors]
    started = unrecognized = False
    # A diagnostic's gutter is as wide as the widest line number it shows:
    # its `-->` and `= note:` lines are indented by that width, one more
    # space for each digit past two (`   --> book.pw:99:19` beside a line
    # 100); until 2026-11-02 only a two-digit gutter was read.
    for line in lines[:-1]:
        if line.startswith("error:"):
            started = True
        elif line and (not started or re.fullmatch(
                r"(?:\s*(?:\d+|\.\.\.)?\s*\|.*| {2,}--> .+:\d+:\d+| {3,}= (?:note|help):.*)", line) is None):
            unrecognized = True
    if (summary is None or int(summary[1]) != files or int(summary[2]) != len(errors)
            or not errors or not all(codes) or unrecognized
            or sum(line.startswith("pw check:") for line in lines) != 1
            or any(re.match(r"(?:warning:|error\[|thread |fatal:|Traceback|panicked)", line) for line in lines)):
        return set()
    return {match[1] for match in codes}


def browser_tests():
    """(valid, ran, failed, failed titles, output), or typed compiler refusal."""
    global _BROWSER_TITLES
    code, out = run(APP_BUILD)
    if code != 0:
        codes = compiler_codes(code, out)
        if codes:
            return CompilerRefusal(codes, out)
        return False, 0, 0, set(), out
    directory = pathlib.Path(tempfile.mkdtemp(prefix="pw-guide-browser-"))
    receipt, metadata = directory / "assertions.json", directory / "metadata.json"
    nonce = secrets.token_hex(32)
    code, out = run(
        ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--", *SPEC],
        cwd=ROOT / "spikes/own-renderer",
        env={**os.environ, "PW_ASSERTION_REPORT": str(receipt), "PW_ASSERTION_NONCE": nonce,
             "PLAYWRIGHT_JSON_OUTPUT_NAME": str(metadata), "PLAYWRIGHT_JSON_OUTPUT_FILE": str(metadata)},
    )
    (directory / "output.txt").write_text(out)
    try:
        report = json.loads(receipt.read_text())
    except (OSError, ValueError):
        report = None
    result = browser_test_results.parse(code, report, nonce, _BROWSER_TITLES)
    if not result.valid:
        return False, 0, 0, set(), out + "\nBrowser receipt refused: " + result.reason
    try:
        titles = browser_titles(json.loads(metadata.read_text()), result.inventory)
        if _BROWSER_TITLES is not None and titles != _BROWSER_TITLES:
            raise ValueError("browser title mapping changed after baseline")
        if _BROWSER_TITLES is None:
            if result.failed:
                raise ValueError("browser title mapping requires a green baseline")
            _BROWSER_TITLES = titles
    except (OSError, ValueError) as error:
        return False, 0, 0, set(), out + "\nBrowser metadata refused: " + str(error)
    failed_titles = {titles[identity] for identity, status in report["tests"] if status == "failed"}
    return True, result.passed + result.failed, result.failed, failed_titles, out


def judge(what, held, names, failed):
    """Whether the tests that failed are the ones `what` is for."""
    must = MUST[what]
    if held == "browser":
        hit = any(any(want in t for t in names) for want in must)
    else:
        hit = bool(must & names)
    return hit


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    global _PYTHON_EXPECTED, _BROWSER_TITLES
    _PYTHON_EXPECTED = None
    _BROWSER_TITLES = None
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))

    seen = set()
    for what, *_ in MUTANTS:
        assert what not in seen and what in MUST, f"{what}: not listed once in MUST"
        seen.add(what)
    assert set(MUST) == seen, "MUST names a mutant that is not one"
    assert set(CHECKED) <= seen, "CHECKED names a mutant that is not one"

    try:
        _PYTHON_EXPECTED = freeze_python()
    except guide_python.DiscoveryRefused as error:
        print(f"REFUSED: Python discovery did not establish a frozen inventory: {error}")
        return 1
    result = guide_tests()
    ran, failed, names, out = result
    print(f"baseline: the guide's tests, {ran} ran, {failed} failed")
    if ran is None or failed or not ran or _stopped(watch):
        print(out[-2000:])
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    browser = any(held == "browser" for _, held, *_ in MUTANTS)
    if browser:
        result = browser_tests()
        valid, ran, failed, titles, out = result
        print(f"baseline: the guestbook in Chromium, {ran} ran, {failed} failed")
        if not valid or failed or not ran or _stopped(watch):
            print(out[-3000:])
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = refused = killed = 0
    for what, held, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            if held == "guide":
                result = guide_tests()
                ran, failed, names, out = result
                if ran is None or not ran or _stopped(watch):
                    verdict = "REFUSED (invalid or stopped guide producer; no kill)"
                    refused += 1
                elif not failed:
                    verdict = "SURVIVED"
                    survivors += 1
                elif judge(what, held, names, failed):
                    shown = ", ".join(sorted(MUST[what] & names))
                    category = failure_categories(result, MUST[what] & names)
                    verdict = f"KILLED by {shown} ({failed} of {ran} tests fail; {category})"
                    killed += 1
                else:
                    verdict = f"SURVIVED (no intended witness failed: {sorted(names)})"
                    survivors += 1
            else:
                result = browser_tests()
                valid, ran, failed, names, out = result
                if _stopped(watch):
                    verdict = "REFUSED (stopped browser/build producer; no kill)"
                    refused += 1
                elif isinstance(result, CompilerRefusal):
                    if what in CHECKED and result.codes == CHECKED[what]:
                        verdict = f"KILLED by the checker ({', '.join(sorted(result.codes))})"
                        killed += 1
                    else:
                        verdict = f"REFUSED by {sorted(result.codes)}, and the rule named is {sorted(CHECKED.get(what, set()))}"
                        refused += 1
                elif not valid or not ran:
                    verdict = "REFUSED (invalid browser/build producer; no kill)"
                    refused += 1
                elif what in CHECKED:
                    verdict = "SURVIVED (the checker takes it)"
                    survivors += 1
                elif not failed:
                    verdict = "SURVIVED"
                    survivors += 1
                elif judge(what, held, names, failed):
                    verdict = f"KILLED by the spec ({failed} of {ran} tests fail)"
                    killed += 1
                else:
                    verdict = f"SURVIVED (no intended witness failed: {sorted(names)})"
                    survivors += 1
        print(f"{what}: {verdict}", flush=True)
        if verdict.startswith("REFUSED"):
            print(out[-3000:], flush=True)
        if _stopped(watch):
            print("REFUSED: memory guard stopped the campaign")
            return 1

    if browser:
        # Leave the app built from the sources as they are.
        code, out = run(APP_BUILD)
        if code != 0 or _stopped(watch):
            print("FAIL: the app does not build again from the restored sources")
            refused += 1

    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return 1 if survivors or refused else 0


if __name__ == "__main__":
    sys.exit(main())
