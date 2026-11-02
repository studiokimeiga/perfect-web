#!/usr/bin/env python3
"""Mutation controls for kiokun.com's word page on the development server
(track `kiokun`, W6).

A test that passes with the mechanism removed is not evidence for it. Each
mutant undoes one rule of the word page, in the program
(`examples/kiokun-site/app.pw`), in the read-only kiokun layer
(`spikes/own-renderer/server/src/kiokun.rs`), or at the host's seam that
chooses it, and at least one of the server's kiokun tests
(`server/src/tests/kiokun.rs`) must then fail or not build.

Each mutant runs the tests its suite is given (SEEN_BY): the cover of its
suite's measured killers, each mutant seen by two tests where two kill it
(the integrator's ruling of 2026-10-09; `scripts/mutant_killers.py`). Run
whole, every mutant ran every kiokun test, and the script outran a verify's
six hours (verify 38038009339).

Run from the repository root; `just e14-kiokun-word` records the output.
The sources are restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP = ROOT / "examples/kiokun-site/app.pw"
LAYER = ROOT / "spikes/own-renderer/server/src/kiokun.rs"
HOST = ROOT / "spikes/own-renderer/server/src/main.rs"
SEARCH = ROOT / "spikes/own-renderer/server/src/kiokun/search.rs"

# (what is undone, its suite, file, anchor, replacement). The suite is the
# page's area the mutant is in, which names the tests it is given (SEEN_BY).
MUTANTS = [
    (
        "a file answers for any word whose name it has",
        "files",
        APP,
        "        Some(e) => if e.key == word { Some(e) } else { None },",
        "        Some(e) => Some(e),",
    ),
    (
        "a stub's redirect is read and not shown",
        "stubs",
        APP,
        "                Some(t) => Some(followed_from(t, f)),",
        "                Some(_) => Some(f),",
    ),
    (
        "kiokun's file names escape control characters alone",
        "files",
        APP,
        "    if unsafe_mark | control { 95 } else { c }",
        "    if control { 95 } else { c }",
    ),
    (
        "a Chinese reading with no senses is shown",
        "words",
        APP,
        "    List.filter(all, s => List.length(s.definitions) > 0)",
        "    all",
    ),
    (
        "a search-only form is shown",
        "words",
        APP,
        "    let shown = List.filter(items, h => !List.any(h.tags, t => t == hidden))",
        "    let shown = items",
    ),
    (
        "kiokun's placeholder definition is shown",
        "words",
        APP,
        '    let kept = List.filter(w.senses, s => s.text != "" & s.text != "Sentence")',
        '    let kept = List.filter(w.senses, s => s.text != "")',
    ),
    (
        "one sense is numbered as many are",
        "words",
        APP,
        "        many: List.length(w.senses) > 1,",
        "        many: List.length(w.senses) > 0,",
    ),
    (
        "a name's kind is shown as its code",
        "words",
        APP,
        '        "given" => "given name",',
        '        "given" => "given",',
    ),
    (
        "a reading's Jyutping is its pinyin",
        "words",
        APP,
        "[{c.jyutping}]",
        "[{c.pinyin}]",
    ),
    (
        "any subdirectory is read",
        "files",
        LAYER,
        "    let hex = subdirectory.len() == 2\n        && subdirectory",
        "    let hex = true\n        || subdirectory",
    ),
    (
        "a file name may hold a separator",
        "files",
        LAYER,
        " && !file.contains(['/', '\\\\', '\\0']);",
        ";",
    ),
    (
        "a name too long for the file system is read",
        "files",
        LAYER,
        "    let fits = name.len() <= NAME_MAX;",
        "    let fits = true;",
    ),
    (
        "a repeated id keys two records",
        "files",
        LAYER,
        '        } else {\n            format!("{id}:{n}")\n        };',
        "        } else {\n            id.clone()\n        };",
    ),
    (
        "the layer promises no strong reads, which the program states",
        "files",
        LAYER,
        '            reads: BTreeSet::from(["strong".to_string()]),',
        "            reads: BTreeSet::new(),",
    ),
    (
        "a Chinese word with empty forms shows no headword",
        "words",
        APP,
        "{ if w.simp != \"\" { w.simp } else { word } }",
        "{ w.simp }",
    ),
    (
        "a label of kiokun's table is not read",
        "labels",
        APP,
        '        Some(l) => if l.text == "" { None } else { Some(l.text) },',
        "        Some(_) => None,",
    ),
    (
        "a code is looked up as written alone, as kiokun.com does",
        "labels",
        APP,
        "        None => match labelled(labels, kind, underscored(code)) {",
        "        None => match labelled(labels, kind, code) {",
    ),
    (
        "a form's note is looked up by its short code",
        "labels",
        APP,
        '        "rK" => "rkanji",',
        '        "rK" => "rK",',
    ),
    (
        "a part of speech is labelled from another table",
        "labels",
        APP,
        '    let pos = List.map(s.pos, c => label(labels, "pos", c))',
        '    let pos = List.map(s.pos, c => label(labels, "misc", c))',
    ),
    (
        "Mandarin keeps the readings the words do not read",
        "header",
        APP,
        "    let read = List.filter(frequencies, p => List.any(words, w => w == p))",
        "    let read = frequencies",
    ),
    (
        "Mandarin skips the older sources",
        "header",
        APP,
        "    if List.length(shown) > 0 { shown } else { if List.length(old) > 0 { old } else { words } }",
        "    if List.length(shown) > 0 { shown } else { words }",
    ),
    (
        "the gloss is the meaning before the keyword",
        "header",
        APP,
        '    learner(if keyword != "" { keyword } else { if meaning != "" { meaning } else { gloss } })',
        '    learner(if meaning != "" { meaning } else { if keyword != "" { keyword } else { gloss } })',
    ),
    (
        "the gloss keeps its source markers",
        "header",
        APP,
        '    String.trim(without(without(without(without(gloss, "(trad/jp)"), "(trad)"), "(simp)"), "(jp)"))',
        "    String.trim(gloss)",
    ),
    (
        "a marker is matched in its case alone",
        "header",
        APP,
        "    match found_at(String.to_lower_ascii(text), marker, 0) {",
        "    match found_at(text, marker, 0) {",
    ),
    (
        "every entry has a character header",
        "header",
        APP,
        "        shown: has_chinese | has_japanese,",
        "        shown: true,",
    ),
    (
        "an HSK level of 0 is shown",
        "header",
        APP,
        '            Some(n) => if n != 0 { "HSK {n}" } else { "" },',
        '            Some(n) => "HSK {n}",',
    ),
    (
        "dictionary meanings are shown with no keyword",
        "header",
        APP,
        '        Some(m) => if m.keyword != "" & m.lexical != "" { m.lexical } else { "" },',
        "        Some(m) => m.lexical,",
    ),
    (
        "a reading the file repeats is shown twice",
        "header",
        APP,
        '(out, v) => if v == "" | List.any(out, s => s == v) { out }',
        '(out, v) => if v == "" { out }',
    ),
    (
        "KANJIDIC's flat readings are never read",
        "header",
        LAYER,
        '        ("grouped", Val::Bool(groups.is_some())),',
        '        ("grouped", Val::Bool(true)),',
    ),
    (
        "a hanja reading is its romanization",
        "header",
        LAYER,
        '        ("readings", texts(k, "readings", "hangul")),',
        '        ("readings", texts(k, "readings", "romanization")),',
    ),
    (
        "the forms' notes are not read from the table",
        "labels",
        LAYER,
        'const LABEL_KINDS: [&str; 5] = ["pos", "misc", "field", "dial", "head_info"];',
        'const LABEL_KINDS: [&str; 4] = ["pos", "misc", "field", "dial"];',
    ),
    (
        "an app without its label table is served with none",
        "labels",
        LAYER,
        '    let path = app.join("src/lib/japanese-labels.json");\n    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;',
        '    let path = app.join("src/lib/japanese-labels.json");\n    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());',
    ),
    (
        "a stub's card yields to its target's",
        "stubs",
        APP,
        "    let primary = match stub.mnemonic {",
        "    let primary = match target.mnemonic {",
    ),
    (
        "a stub's traditional variants are not read",
        "stubs",
        APP,
        "        Some(s) => if List.length(traditional_of(s)) > 1 { traditional_of(s) } else { none },",
        "        Some(s) => if List.length(traditional_of(s)) > 1 { none } else { none },",
    ),
    (
        "a Chinese word read twice is shown twice",
        "stubs",
        APP,
        "fn more_chinese(into: List<ChineseWord>, more: List<ChineseWord>) -> List<ChineseWord> {\n    List.fold(more, into, (out, w) => if List.any(out, s => s.id == w.id) { out } else { List.concat(out, [w]) })",
        "fn more_chinese(into: List<ChineseWord>, more: List<ChineseWord>) -> List<ChineseWord> {\n    List.concat(into, more)",
    ),
    (
        "a simplified form is equivalent whatever its card teaches",
        "stubs",
        APP,
        '            if source_concept != "" & source_concept == target_concept { Some(target) } else { None }',
        '            if source_concept != "" { Some(target) } else { None }',
    ),
    (
        "a concept keeps its case and spaces",
        "header",
        APP,
        "    spaced(String.to_lower(learner(gloss)))",
        "    learner(gloss)",
    ),
    (
        "an equivalent form's words are not merged",
        "stubs",
        APP,
        "        Some(t) => if t == canonical { merged_form(into, r) } else { into },",
        "        Some(_) => into,",
    ),
    (
        "a traditional form is not Hong Kong's",
        "header",
        APP,
        '        if x.character != "" & traditional_role { add_role(t3, x.character, "hong-kong") } else { t3 }',
        "        t3",
    ),
    (
        "a simplified form is written as a traditional one",
        "header",
        APP,
        '            if has_role(roles, "hong-kong") & List.length(roles) == 1 { "zh-HK" } else { if has_role(roles, "simplified") & !has_role(roles, "traditional") { "zh-Hans" } else { "zh-Hant" } }',
        '            if has_role(roles, "hong-kong") & List.length(roles) == 1 { "zh-HK" } else { "zh-Hant" }',
    ),
    (
        "a form without a card has no meaning",
        "header",
        APP,
        "        meaning: if carded != \"\" { carded } else { gloss_of(glosses, r.character) },",
        "        meaning: carded,",
    ),
    (
        "a component gloss keeps its source marker",
        "header",
        APP,
        "        Some(g) => learner(g.gloss),",
        "        Some(g) => g.gloss,",
    ),
    (
        "the batched read reads nothing",
        "files",
        LAYER,
        "                    out.push(read_one(&r, &subdirectory, &file)?);",
        "                    out.push(Val::Option(None));",
    ),
    (
        "the app's glosses are read from another file",
        "files",
        LAYER,
        '    let path = app.join("static/game_data/component_glosses.json");',
        '    let path = app.join("static/game_data/component_uses.json");',
    ),
    (
        "a length is counted in code points, not UTF-16 units",
        "head",
        APP,
        "(n, c) => n + if c > 65535 { 2 } else { 1 })",
        "(n, c) => n + 1)",
    ),
    (
        "a definition keeps its markup",
        "head",
        APP,
        "    String.trim(spaced(String.from_codepoints(untagged(String.codepoints(value)))))",
        "    String.trim(spaced(value))",
    ),
    (
        "a long title is not cut",
        "head",
        APP,
        '    if units(text) <= length { text } else { "{kept}…" }',
        "    text",
    ),
    (
        "a definition is not split at the full-width semicolon",
        "head",
        APP,
        "(s, c) => if c == 59 | c == 65307 {",
        "(s, c) => if c == 59 {",
    ),
    (
        "a numbered definition keeps its number",
        "head",
        APP,
        "    if digits > 0 & after {",
        "    if false {",
    ),
    (
        "a classifier and a one-letter piece are named",
        "head",
        APP,
        "    List.filter(unique_clean(all), v => units(v) > 1 & units(v) <= 96 & !classifier(v))",
        "    List.filter(unique_clean(all), v => units(v) > 0 & units(v) <= 96)",
    ),
    (
        "the learner gloss does not lead the meanings",
        "head",
        APP,
        "    let shown = List.take(unique_clean(List.concat([gloss], definitions_of(e))), 5)",
        "    let shown = List.take(unique_clean(definitions_of(e)), 5)",
    ),
    (
        "the title names no other form",
        "head",
        APP,
        '    let suffix = String.join(List.take(List.filter(forms, f => f != word), 3), "・")',
        '    let suffix = String.join(List.take(List.filter(forms, f => f != word), 0), "・")',
    ),
    (
        "a pitch shard hashes code points",
        "pitch",
        APP,
        "    if c > 65535 { [55296 + (c - 65536) / 1024, 56320 + (c - 65536) % 1024] } else { [c] }",
        "    [c]",
    ),
    (
        "a reading's own accent is never read",
        "pitch",
        APP,
        "    match List.find(readings, r => r.reading == reading) {",
        "    match List.find(readings, r => false) {",
    ),
    (
        "a small kana is a mora of its own",
        "pitch",
        APP,
        "            Some(next) => if small_kana(next) {",
        "            Some(next) => if false {",
    ),
    (
        "a fall after the last mora is 中高",
        "pitch",
        APP,
        '    if accent == 0 { "平板" } else { if accent == 1 { "頭高" } else { if accent == count { "尾高" } else { "中高" } } }',
        '    if accent == 0 { "平板" } else { if accent == 1 { "頭高" } else { "中高" } }',
    ),
    (
        "the morae after the fall stay high",
        "pitch",
        APP,
        "(a > 1 & List.length(out) > 0 & List.length(out) < a)",
        "(a > 1 & List.length(out) > 0)",
    ),
    (
        "a word's readings are read in sorted order",
        "pitch",
        LAYER,
        "                Ok(Readings(out))",
        "                out.sort_by(|a, b| a.0.cmp(&b.0));\n                Ok(Readings(out))",
    ),
    (
        "a null accent is 0",
        "pitch",
        LAYER,
        "                    out.push((reading, accent.and_then(|a| a.as_i64())));",
        "                    out.push((reading, accent.map(|_| 0)));",
    ),
    (
        "a Chinese example is written in its traditional form",
        "examples",
        APP,
        'Example { text: if e.simp != "" { e.simp } else { e.trad }, translation: e.en }',
        "Example { text: e.trad, translation: e.en }",
    ),
    (
        "two examples are shown before the disclosure",
        "examples",
        APP,
        "        first: List.take(all, 1),",
        "        first: List.take(all, 2),",
    ),
    (
        "an example without a text is shown",
        "examples",
        APP,
        '    let usable = List.filter(items, e => e.text != "")',
        "    let usable = items",
    ),
    (
        "a Japanese sentence's `land` is not read",
        "examples",
        APP,
        'List.find(e.sentences, t => t.land == "jpn" | t.lang == "jpn")',
        'List.find(e.sentences, t => t.lang == "jpn")',
    ),
    (
        "the host does not choose the kiokun layer",
        "files",
        HOST,
        '    } else if imports("kiokun:") {',
        '    } else if imports("kiokun-never:") {',
    ),
    (
        "a word's own file never moves it",
        "moves",
        APP,
        "    let moved = moved_from(first, shown_word)\n",
        "    let moved = moved_from(None, shown_word)\n",
    ),
    (
        "a word is moved to itself",
        "moves",
        APP,
        "            Some(t) => if t != word { Some(t) } else { None },\n",
        "            Some(t) => Some(t),\n",
    ),
    (
        "a move is temporary",
        "moves",
        APP,
        "    redirect_on  KiokunError.Moved permanent\n",
        "    redirect_on  KiokunError.Moved temporary\n",
    ),
    (
        "the page names no move",
        "moves",
        APP,
        "    redirect_on  KiokunError.Moved permanent\n",
        "",
    ),
    (
        "a character's own form counts among its traditional forms",
        "stubs",
        APP,
        '    let traditional = List.filter(traditional_of(e), v => v != "" & v != source)\n',
        '    let traditional = List.filter(traditional_of(e), v => v != "")\n',
    ),
    (
        "Contains keeps the file's order",
        "contains",
        APP,
        "    let sorted = List.sort_by(e.contains, (a, b) => by_place(word, a, b))\n",
        "    let sorted = e.contains\n",
    ),
    (
        "Contains is never broken down",
        "contains",
        APP,
        "    List.concat(placed, List.sort_by(rest, (a, b) => by_remaining(chosen.form, a, b)))\n",
        "    List.sort_by(candidates, (a, b) => by_remaining(chosen.form, a, b))\n",
    ),
    (
        "a breakdown of more pieces is better",
        "contains",
        APP,
        "            List.length(candidate) < List.length(now)\n",
        "            List.length(candidate) > List.length(now)\n",
    ),
    (
        "a breakdown of worse words is better",
        "contains",
        APP,
        "                quality_of(candidate) > quality_of(now)\n",
        "                quality_of(candidate) < quality_of(now)\n",
    ),
    (
        "the whole word may be one piece of itself",
        "contains",
        APP,
        "    if stop > end | (cursor == start & stop == end) | !matches_at(form, c.chars, cursor) {\n",
        "    if stop > end | !matches_at(form, c.chars, cursor) {\n",
    ),
    (
        "the rest are not ordered longest first",
        "contains",
        APP,
        "        List.length(b.chars) - List.length(a.chars)\n",
        "        List.length(a.chars) - List.length(b.chars)\n",
    ),
    (
        "a word's forms are never carded together",
        "contains",
        APP,
        "    if List.length(forms) < 2 | length == 0 | List.any(forms, f => List.length(f) != length) {\n",
        "    if true {\n",
    ),
    (
        "a card's definition is never a component gloss",
        "contains",
        APP,
        "        definition: contains_definition(card, glosses),\n",
        "        definition: card.candidate.item.d,\n",
    ),
    (
        "a gloss keeps its source marker",
        "contains",
        APP,
        '    if marker == "" { t } else { String.trim(String.slice(t, 0, String.length(t) - String.length(marker))) }\n',
        "    t\n",
    ),
    (
        "an on'yomi the same as the reading is shown",
        "contains",
        APP,
        '    let jo = if p.jo != p.jp { p.jo } else { "" }\n',
        "    let jo = p.jo\n",
    ),
    (
        "glosses are not read for Contains' previews",
        "contains",
        APP,
        "List.map(l.data.contains, p => p.w)",
        "List.map(List.take(l.data.contains, 0), p => p.w)",
    ),
    (
        "Chinese names are not put last",
        "contains",
        APP,
        "    let first = if chinese { proper } else { common }\n",
        "    let first = if chinese { 0 } else { common }\n",
    ),
    (
        "Japanese common words are not put first",
        "contains",
        APP,
        "    let first = if chinese { proper } else { common }\n",
        "    let first = if chinese { proper } else { 0 }\n",
    ),
    (
        "words are not ranked by frequency",
        "contains",
        APP,
        "        if rank_of(a.item) != rank_of(b.item) { if rank_of(a.item) < rank_of(b.item) { -1 } else { 1 } } else { a.index - b.index }\n",
        "        a.index - b.index\n",
    ),
    (
        "a rank of 0 is the first",
        "contains",
        APP,
        "        Some(n) => if n > 0 { n } else { 1000000000 },\n",
        "        Some(n) => n,\n",
    ),
    (
        "a word read and defined as another is shown twice",
        "contains",
        APP,
        "List.any(out, q => q.w == p.w & reading_of(q) == reading_of(p) & String.trim(q.d) == String.trim(p.d))",
        "false",
    ),
    (
        "a zero-width space is a letter",
        "contains",
        APP,
        "c != 8203 & c != 8204",
        "c != 8204",
    ),
    (
        "Korean words are ranked",
        "contains",
        APP,
        '    if language == "korean" {\n        unique\n',
        '    if language == "none" {\n        unique\n',
    ),
    (
        "every Appears in word is shown at once",
        "contains",
        APP,
        '        first: List.take(shown, 10),\n        rest: List.drop(shown, 10),\n        more: "{List.length(shown) - 10} more items",\n',
        '        first: shown,\n        rest: List.drop(shown, 1000),\n        more: "{List.length(shown) - 10} more items",\n',
    ),
    (
        "an equivalent form's Appears in is not merged",
        "contains",
        APP,
        "        in_chinese: more_previews(into.in_chinese, related.in_chinese),\n",
        "        in_chinese: into.in_chinese,\n",
    ),
    (
        "an equivalent form's Contains is not merged",
        "contains",
        APP,
        "        contains: more_contains(into.contains, related.contains),\n",
        "        contains: into.contains,\n",
    ),
    (
        "a merged word is shown twice",
        "contains",
        APP,
        '    List.fold(more, into, (out, p) => if p.w == "" | List.any(out, s => s.w == p.w) { out } else { List.concat(out, [p]) })\n',
        '    List.fold(more, into, (out, p) => if p.w == "" { out } else { List.concat(out, [p]) })\n',
    ),
    (
        "a preview's common flag is not read",
        "contains",
        LAYER,
        '                        Val::Bool(p.get("c").and_then(|c| c.as_bool()) == Some(true)),\n',
        "                        Val::Bool(false),\n",
    ),
    (
        "a preview's rank is not read",
        "contains",
        LAYER,
        '                    ("fr", maybe_int(p.get("fr"))),\n',
        '                    ("fr", maybe_int(None)),\n',
    ),
    (
        "a page with EDRDG's data does not acknowledge the Group",
        "edrdg",
        APP,
        "        edrdg: edrdg_shown(e),\n",
        "        edrdg: false,\n",
    ),
    (
        "every page acknowledges EDRDG, its data shown or not",
        "edrdg",
        APP,
        "        edrdg: edrdg_shown(e),\n",
        "        edrdg: true,\n",
    ),
    (
        "a CJK query is searched as typed alone",
        "search",
        APP,
        "add_terms(add_terms(add_term(none, trimmed), values_of(aliases, trimmed)), spellings(trimmed, variants))",
        "add_term(none, trimmed)",
    ),
    (
        "a katakana query is not read as hiragana",
        "search",
        APP,
        "if c >= 12449 & c <= 12534 { c - 96 } else { c }",
        "c",
    ),
    (
        "a Japanese form names its own page",
        "search",
        APP,
        '    if row.language == "japanese" {\n        match List.get(values_of(canonical, row.word), 0) {',
        '    if false {\n        match List.get(values_of(canonical, row.word), 0) {',
    ),
    (
        "the query's own row never names its group",
        "search",
        APP,
        "    if row.word == asked { 1000 } else {",
        "    if false { 1000 } else {",
    ),
    (
        "pronunciations join Chinese first",
        "search",
        APP,
        "    let parts = List.filter([g.japanese, g.chinese, g.korean], p => p != \"\")",
        "    let parts = List.filter([g.chinese, g.japanese, g.korean], p => p != \"\")",
    ),
    (
        "a hit shows every definition",
        "search",
        APP,
        "    let lines = List.fold(List.take(g.definitions, 3), none,",
        "    let lines = List.fold(g.definitions, none,",
    ),
    (
        "every query is searched as Latin",
        "search",
        APP,
        "fn is_cjk(asked: String) -> Bool {\n    List.any(",
        "fn is_cjk(asked: String) -> Bool {\n    false & List.any(",
    ),
    (
        "a column shows every hit at once",
        "search",
        APP,
        "        first: List.take(mine, 8),\n        rest: List.drop(mine, 8),",
        "        first: mine,\n        rest: List.drop(mine, 100000),",
    ),
    (
        "the CJK statement ranks the query's word as any prefix",
        "search",
        SEARCH,
        '.map(|i| format!("WHEN word = ? THEN {}", if i == 0 { 1000 } else { 900 }))',
        '.map(|i| format!("WHEN word = ? THEN {}", if i == 0 { 400 } else { 900 }))',
    ),
    (
        "a Latin query is bound as FTS5's query language",
        "search",
        SEARCH,
        "        binds.push(Sql::Text(as_words(query)));",
        "        binds.push(Sql::Text(query.to_string()));",
    ),
    (
        "a term's inner quote is not doubled",
        "search",
        SEARCH,
        """        .map(|t| format!("\\"{}\\"", t.replace('"', "\\"\\"")))""",
        """        .map(|t| format!("\\"{}\\"", t))""",
    ),
    (
        "a kana query's reading is never searched",
        "search",
        SEARCH,
        "        if !reading.is_empty() {\n            clauses.push(",
        "        if false {\n            clauses.push(",
    ),
    (
        "the word page states no canonical address",
        "head",
        APP,
        "        <link rel=\"canonical\" href={WordPage(page.word)} />\n",
        "",
    ),
    (
        "the canonical address is the one asked",
        "head",
        APP,
        "        <link rel=\"canonical\" href={WordPage(page.word)} />\n",
        "        <link rel=\"canonical\" href={WordPage(word)} />\n",
    ),
    (
        "a blank query is a search like any other",
        "search",
        APP,
        '        blank: String.trim(asked) == "",',
        "        blank: false,",
    ),
    (
        "a word with its own entry leads nowhere",
        "search",
        APP,
        '        Some(_) => [WordLead { id: asked, asked: asked, shown: asked, how: "" }],',
        "        Some(_) => none,",
    ),
    (
        "an inflected query leads nowhere",
        "search",
        APP,
        "            Some(r) => [WordLead { id: asked, asked: asked, shown: r.word, how: r.how }],",
        "            Some(_) => none,",
    ),
    (
        "a Latin query with an entry leads to it",
        "search",
        APP,
        "    let own_files = read_entries(if cjk { [place_of(trimmed)] } else { no_places })",
        "    let own_files = read_entries(if String.trim(q) != \"\" { [place_of(trimmed)] } else { no_places })",
    ),
    (
        "a godan く verb's te form is って",
        "deinflection",
        APP,
        'godan("v5k", "く", "か", "き", "け", "こ", "いて", "いた"),',
        'godan("v5k", "く", "か", "き", "け", "こ", "って", "った"),',
    ),
    (
        "行く's te form is as any く verb's",
        "deinflection",
        APP,
        'godan("v5k-s", "く", "か", "き", "け", "こ", "って", "った"),',
        'godan("v5k-s", "く", "か", "き", "け", "こ", "いて", "いた"),',
    ),
    (
        "a one-row verb's hypothetical base is る",
        "deinflection",
        APP,
        '        Base { name: "hypothetical", text: "{stem}れ" },',
        '        Base { name: "hypothetical", text: "{stem}る" },',
    ),
    (
        "来る's irrealis base is 来る's continuative",
        "deinflection",
        APP,
        '                Base { name: "irrealis", text: "来" },',
        '                Base { name: "irrealis", text: "来た" },',
    ),
    (
        "an adjective's past base is った",
        "deinflection",
        APP,
        '        Base { name: "ta", text: "{stem}かった" },',
        '        Base { name: "ta", text: "{stem}った" },',
    ),
    (
        "いい's forms are built on い",
        "deinflection",
        APP,
        'WordClass { code: "adj-ix", group: "adj-i", ending: "いい", bases: adjective_bases("よ") },',
        'WordClass { code: "adj-ix", group: "adj-i", ending: "いい", bases: adjective_bases("い") },',
    ),
    (
        "する after one kanji has no sa-row forms",
        "deinflection",
        APP,
        '                Base { name: "irrealis", text: "さ" },\n                Base { name: "volitional", text: "そう" },',
        '                Base { name: "irrealis", text: "し" },\n                Base { name: "volitional", text: "そう" },',
    ),
    (
        "the negative is not an auxiliary",
        "deinflection",
        APP,
        'Auxiliary { groups: verbs(), base: "irrealis", text: "ない", makes: "adj-i", reason: "negative" },',
        'Auxiliary { groups: verbs(), base: "irrealis", text: "ない", makes: "none", reason: "negative" },',
    ),
    (
        "the polite ます is not an auxiliary",
        "deinflection",
        APP,
        'Auxiliary { groups: verbs(), base: "continuative", text: "ます", makes: "masu", reason: "polite" },',
        'Auxiliary { groups: verbs(), base: "continuative", text: "ます", makes: "none", reason: "polite" },',
    ),
    (
        "an auxiliary's word is read back once",
        "deinflection",
        APP,
        "fn deinflect_rounds() -> Int {\n    6\n}",
        "fn deinflect_rounds() -> Int {\n    0\n}",
    ),
    (
        "a katakana word is read only as written",
        "deinflection",
        APP,
        "    let spellings = each_once([hiragana(asked), asked])",
        "    let spellings = each_once([asked])",
    ),
    (
        "the candidates are not ordered by their steps",
        "deinflection",
        APP,
        "    List.sort_by(kept, (a, b) => List.length(a.reasons) - List.length(b.reasons))",
        "    List.reverse(kept)",
    ),
    (
        "any entry fits a candidate, its part of speech unread",
        "deinflection",
        APP,
        "        Some(x) => List.any(x.japanese, w => List.any(w.senses, s => List.any(s.pos, p => pos_group(p) == group))),",
        "        Some(_) => true,",
    ),
    (
        "a hiragana candidate is never looked up by reading",
        "deinflection",
        APP,
        "    } else if hiragana_only(d.word) {\n        List.fold(written_forms(readings, d.word),",
        "    } else if false {\n        List.fold(written_forms(readings, d.word),",
    ),
    (
        "a candidate's redirect is not followed",
        "deinflection",
        APP,
        "            Some(t) => entry_of(hopped, t),\n            None => Some(e),",
        "            Some(_) => Some(e),\n            None => Some(e),",
    ),
    (
        "an inflected word resolves to one form",
        "deinflection",
        APP,
        "fn resolved_limit() -> Int {\n    5\n}",
        "fn resolved_limit() -> Int {\n    1\n}",
    ),
    (
        "a bare stem takes two steps, after the forms it can also be",
        "deinflection",
        APP,
        'Closing { groups: ["ichidan"], base: "continuative", text: "", reasons: ["continuative stem"] },',
        'Closing { groups: ["ichidan"], base: "continuative", text: "", reasons: ["continuative stem", "stem"] },',
    ),
    (
        "the page shows an inflected word's own address as not found",
        "deinflection",
        APP,
        "    let first = match List.get(resolutions, 0) {\n        Some(r) => recorded(",
        "    let first = match List.get(List.drop(resolutions, List.length(resolutions)), 0) {\n        Some(r) => recorded(",
    ),
    (
        "the page's own forms are among its homophones",
        "homophones",
        APP,
        "    let kept = List.filter(f.rows, r => !List.any(forms, x => x == r.word))",
        "    let kept = List.filter(f.rows, r => r.word != f.reading)",
    ),
    (
        "a reading two words share is asked twice",
        "homophones",
        APP,
        '(o, k) => if k.text == "" | List.any(o, r => r == k.text) { o }',
        '(o, k) => if k.text == "" { o }',
    ),
    (
        "one group shows its reading",
        "homophones",
        APP,
        "labelled: List.length(groups) > 1,",
        "labelled: true,",
    ),
    (
        "every homophone is marked common",
        "homophones",
        APP,
        'chip: if r.is_common != 0 { "homophone-chip common" }',
        'chip: if true { "homophone-chip common" }',
    ),
    (
        "a reading with no words left is a group",
        "homophones",
        APP,
        "    let groups = List.fold(found, none, (out, f) => match List.length(homophone_words(f, forms)) {",
        "    let groups = List.fold(found, none, (out, f) => match 1 + List.length(homophone_words(f, forms)) {",
    ),
    (
        "the page asks for fewer words than the endpoint's most",
        "homophones",
        APP,
        "fn homophone_limit() -> Int {\n    20\n}",
        "fn homophone_limit() -> Int {\n    3\n}",
    ),
    (
        "the reading statement orders shorter words before common ones",
        "homophones",
        SEARCH,
        "             ORDER BY is_common DESC, LENGTH(word) ASC \\\n             LIMIT ?2\";",
        "             ORDER BY LENGTH(word) ASC, is_common DESC \\\n             LIMIT ?2\";",
    ),
]

# Four test threads, CI's runner's cores, for the baseline and for each
# mutant's cover. Each distinct program is compiled once per test process
# and its files written into each test's own TempDir (the integrator's
# ruling of 2026-10-09; ADR-0158 holds), and each component once a process
# (ADR-0317).
#
# The test binary's peak footprint as ADR-0292's bound reads it (macOS
# phys_footprint, measured 2026-10-12; the bound 3.9 GiB on CI):
# - before ADR-0317, when every test's server compiled the program's
#   components again: every kiokun test 5.21 GiB on four threads (verify
#   38235212974 stopped that baseline), 4.19 on three, 3.47 on two, 2.11 on
#   one; the largest cover, files' eight tests, 2.98 GiB on two threads;
# - after it: every kiokun test 1.28 GiB on four threads, in 46 s; files'
#   cover 1.10 GiB on four, in 55 s.
CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server"]
TESTS = [CARGO + ["--", "kiokun", "--test-threads=4"]]

K = "tests::kiokun::"

# **Where each suite's killers were looked for**: the tests of its mutants'
# area. Measured with `scripts/mutant_killers.py scripts/kiokun_word_mutations.py
# --suite S -- cargo test --locked -p pw-dev-server -- --exact <the group>
# --test-threads=4`; a mutant its group does not kill is looked for among
# every kiokun test. Whole runs cost about 550 CPU seconds each, the program
# compiled once and every test's own server about 14 more, so measuring 107
# mutants whole would take about 19 hours on the shared 12-core machine.
GROUPS = {
    "files": [
        K + "a_file_answers_only_for_the_word_it_records",
        K + "a_word_kiokun_lacks_is_not_found",
        K + "a_word_too_long_to_be_a_file_name_is_not_found",
        K + "kiokun_is_served_by_the_host_its_entries_read_from_kiokuns_files",
        K + "the_layer_reads_no_path_but_one_of_kiokuns_files",
        K + "a_record_listed_twice_is_shown_twice_each_keyed_once",
        K + "an_app_without_its_label_table_is_refused",
        K + "the_word_page_shows_each_languages_words_as_kiokun_does",
    ],
    "stubs": [
        K + "a_stubs_card_heads_the_page_it_names",
        K + "a_stubs_page_shows_its_targets_forms_and_its_own",
        K + "a_stubs_redirect_is_followed_to_the_entry_it_names",
        K + "a_stub_of_several_traditional_forms_shows_each_ones_words",
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
        K + "the_header_shows_each_written_form_with_its_roles",
    ],
    "words": [
        K + "the_word_page_shows_each_languages_words_as_kiokun_does",
        K + "one_japanese_sense_is_not_numbered_and_many_are",
        K + "what_kiokun_com_does_not_show_is_not_shown",
        K + "what_kiokun_com_leaves_out_is_left_out",
    ],
    "labels": [
        K + "a_code_is_shown_by_kiokuns_label_for_it",
        K + "an_app_without_its_label_table_is_refused",
        K + "the_header_shows_each_written_form_with_its_roles",
        K + "the_word_page_shows_each_languages_words_as_kiokun_does",
    ],
    "header": [
        K + "the_character_header_shows_its_gloss_levels_and_readings",
        K + "the_character_headers_rules_hold",
        K + "the_header_shows_each_written_form_with_its_roles",
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
    ],
    "head": [
        K + "the_description_takes_kiokuns_fragments_and_length",
        K + "the_page_head_matches_kiokuns_answers_for_the_sample",
        K + "the_page_describes_itself_as_kiokun_com_does",
        K + "the_word_page_names_its_entrys_page_canonical",
    ],
    "pitch": [
        K + "a_japanese_word_shows_its_pitch_accent",
        K + "a_pitch_shard_is_its_words_hash_over_utf16_units",
    ],
    "examples": [
        K + "a_japanese_senses_examples_are_its_japanese_sentences",
        K + "a_senses_examples_are_shown_first_and_the_rest_disclosed",
        K + "the_japanese_examples_match_kiokuns_answers_for_the_sample",
    ],
    "moves": [
        K + "an_equivalent_simplified_form_is_moved_to_its_traditional_page",
        K + "the_moves_match_kiokuns_answers_for_the_sample",
        K + "a_stubs_redirect_is_followed_to_the_entry_it_names",
    ],
    "contains": [
        K + "contains_breaks_the_word_down_and_cards_its_forms",
        K + "contains_and_appears_in_match_kiokuns_answers_for_the_sample",
        K + "appears_in_ranks_each_languages_words_ten_at_a_time",
    ],
    "edrdg": [
        K + "a_page_with_edrdg_data_acknowledges_the_group",
        K + "what_kiokun_com_leaves_out_is_left_out",
    ],
    "search": [
        K + "search_reads_kana_variants_and_canonical_pages",
        K + "search_matches_kiokuns_answers_for_the_sample",
        "kiokun::search::tests::a_query_is_words_whatever_it_holds",
        "kiokun::search::tests::a_query_fts5_cannot_read_raw_is_answered",
        "kiokun::search::tests::a_csv_record_holds_quotes_commas_and_line_breaks",
        K + "the_search_box_sends_its_query_to_the_search_page",
    ],
    "homophones": [
        K + "homophones_are_each_readings_words_but_the_pages_own",
        K + "homophones_match_kiokuns_answers_for_the_sample",
    ],
    "deinflection": [
        K + "an_inflected_word_shows_the_dictionary_form_it_is",
        K + "deinflection_matches_kiokuns_answers_for_the_sample",
    ],
}

# **The tests each suite's mutants are given** (the integrator's ruling of
# 2026-10-09: a mutant that takes long is given the tests that can see its
# rule): the cover `mutant_killers.py` prints over the measured killers, each
# mutant seen by two tests where two kill it. A suite not named here runs
# every kiokun test.
SEEN_BY = {
    "files": [
        K + "a_code_is_shown_by_kiokuns_label_for_it",
        K + "a_file_answers_only_for_the_word_it_records",
        K + "a_japanese_word_shows_its_pitch_accent",
        K + "a_record_listed_twice_is_shown_twice_each_keyed_once",
        K + "a_stub_of_several_traditional_forms_shows_each_ones_words",
        K + "a_word_too_long_to_be_a_file_name_is_not_found",
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
        K + "the_layer_reads_no_path_but_one_of_kiokuns_files",
    ],
    "stubs": [
        K + "a_stub_of_several_traditional_forms_shows_each_ones_words",
        K + "a_stubs_card_heads_the_page_it_names",
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
        K + "an_equivalent_simplified_form_is_moved_to_its_traditional_page",
    ],
    "words": [
        K + "one_japanese_sense_is_not_numbered_and_many_are",
        K + "the_character_headers_rules_hold",
        K + "the_word_page_shows_each_languages_words_as_kiokun_does",
        K + "what_kiokun_com_leaves_out_is_left_out",
    ],
    "labels": [
        K + "a_code_is_shown_by_kiokuns_label_for_it",
        K + "an_app_without_its_label_table_is_refused",
    ],
    "header": [
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
        K + "the_character_header_shows_its_gloss_levels_and_readings",
        K + "the_character_headers_rules_hold",
        K + "the_header_shows_each_written_form_with_its_roles",
    ],
    "head": [
        K + "the_description_takes_kiokuns_fragments_and_length",
        K + "the_page_describes_itself_as_kiokun_com_does",
        K + "the_page_head_matches_kiokuns_answers_for_the_sample",
        K + "the_word_page_names_its_entrys_page_canonical",
    ],
    "pitch": [
        K + "a_japanese_word_shows_its_pitch_accent",
        K + "a_pitch_shard_is_its_words_hash_over_utf16_units",
    ],
    "examples": [
        K + "a_japanese_senses_examples_are_its_japanese_sentences",
        K + "a_senses_examples_are_shown_first_and_the_rest_disclosed",
        K + "the_japanese_examples_match_kiokuns_answers_for_the_sample",
    ],
    "moves": [
        K + "an_equivalent_simplified_form_is_moved_to_its_traditional_page",
    ],
    "contains": [
        K + "an_equivalent_forms_words_are_shown_on_the_canonical_page",
        K + "appears_in_ranks_each_languages_words_ten_at_a_time",
        K + "contains_and_appears_in_match_kiokuns_answers_for_the_sample",
        K + "contains_breaks_the_word_down_and_cards_its_forms",
    ],
    "edrdg": [
        K + "a_page_with_edrdg_data_acknowledges_the_group",
    ],
    "search": [
        K + "search_matches_kiokuns_answers_for_the_sample",
        K + "search_reads_kana_variants_and_canonical_pages",
        K + "the_search_box_sends_its_query_to_the_search_page",
    ],
    "homophones": [
        K + "homophones_are_each_readings_words_but_the_pages_own",
        K + "homophones_match_kiokuns_answers_for_the_sample",
    ],
    "deinflection": [
        K + "an_inflected_word_shows_the_dictionary_form_it_is",
        K + "deinflection_matches_kiokuns_answers_for_the_sample",
    ],
}


def command_for(suite):
    """The test command a suite's mutant is given."""
    names = SEEN_BY.get(suite)
    if names is None:
        return TESTS[0]
    return CARGO + ["--", "--exact", *names, "--test-threads=4"]


def run_tests(cmd):
    """(built, passed, failed) of a test command."""
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    out = r.stdout + r.stderr
    m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if m is None:
        return False, 0, 0
    return True, int(m.group(1)), int(m.group(2))


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run_tests(TESTS[0])
    print(f"baseline: {passed} passed, {failed} failed")
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
            built, passed, failed = run_tests(command_for(suite))
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
