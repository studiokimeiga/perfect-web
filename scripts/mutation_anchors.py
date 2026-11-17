#!/usr/bin/env python3
"""Every mutation control's anchor matches its file exactly once.

A mutation script (`scripts/*_mutations.py`) undoes one piece of the
compiler by replacing an exact anchor. When later work reformats or moves
that code, the anchor stops matching, and the script says so only when it
runs. Nothing ran a script after its own evidence was recorded, so on
2026-09-25 five anchors in four scripts had drifted unnoticed. This check
reads the anchors and builds nothing, so `just ci` runs it on every change.

A mutant is `(..., path, anchor, replacement)`. The check fails on an anchor
that does not match exactly once, on a replacement equal to its anchor, and
on a mutant of any other shape.

It fails too on a mutant left applied that keeps its anchor: one whose
replacement adds beside the anchor, found whole in the file. A script
stopped from outside does not always restore its source, and such a mutant
leaves the anchor matching once (2026-10-03, ADR-0161: a run killed while
its mutant hung). A mutant that replaces its anchor, left applied, takes
the anchor away, which the count above reports.

And it fails on a test the strict validator cannot read (2026-11-16): a
script whose `TESTS` filters the server's tests by module (`tests::owned_clock::`)
must list that module's file in its `SOURCES`, since a failure is accepted
only at an assertion of a file it lists. #116 added the filter alone, and its
verify found three mutants INVALID (40 minutes of CI) where this finds it at
once. So must a script that runs them by a word of their names (`uploads`,
`a_condition_`): each file holding a test whose name, as cargo reads it with
its module's, has that word (L5's second review, 2026-11-17:
`upload_metadata_mutations.py`'s `uploads blob` ran seven files its
`RUST_SOURCES` did not list, one of which made a kill INVALID in two full
verifies).
"""

import functools
import importlib.util
import pathlib
import re
import sys

sys.dont_write_bytecode = True

ROOT = pathlib.Path(__file__).resolve().parent.parent


def problems_in(script):
    """(mutants read, problems found) for one script."""
    spec = importlib.util.spec_from_file_location(script.stem, script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    mutants = getattr(module, "MUTANTS", None)
    if not mutants:
        return 0, [f"{script.name}: no MUTANTS"]
    problems = []
    for mutant in mutants:
        shaped = (
            isinstance(mutant, tuple)
            and len(mutant) >= 4
            and isinstance(mutant[-3], pathlib.Path)
            and isinstance(mutant[-2], str)
            and isinstance(mutant[-1], str)
        )
        if not shaped:
            problems.append(f"{script.name}: a mutant of another shape: {mutant!r:.80}")
            continue
        what = " ".join(str(m) for m in mutant[:-3])
        path, anchor, replacement = mutant[-3:]
        if anchor == replacement:
            problems.append(f"{script.name}: {what}: the replacement is its anchor")
            continue
        text = path.read_text()
        found = text.count(anchor)
        if found != 1:
            problems.append(
                f"{script.name}: {what}: {found} matches in {path.relative_to(ROOT)}"
            )
        elif anchor in replacement and replacement in text:
            problems.append(
                f"{script.name}: {what}: applied in {path.relative_to(ROOT)}, and not restored"
            )
    problems.extend(unread_tests(script, module))
    problems.extend(unread_word_tests(script, module))
    return len(mutants), problems


SERVER = ROOT / "spikes/own-renderer/server/src"
MODULE_FILTER = re.compile(r"^[a-z_][a-z0-9_]*(::[a-z_][a-z0-9_]*)*::$")


def module_file(filter_text):
    """The file of the server's module a test filter names (`a::b::`):
    the deepest of `src/a/b.rs`, `src/a/b/mod.rs`, `src/a.rs` that is
    there, as Rust finds a module; none where no such file is."""
    parts = filter_text.rstrip(":").split("::")
    for depth in range(len(parts), 0, -1):
        stem = SERVER.joinpath(*parts[:depth])
        for candidate in (stem.with_suffix(".rs"), stem / "mod.rs"):
            if candidate.is_file():
                return candidate
    return None


def flat(value):
    """Every leaf of nested lists, tuples and a dict's values, in order."""
    if isinstance(value, dict):
        return flat(list(value.values()))
    if isinstance(value, (list, tuple)):
        return [leaf for item in value for leaf in flat(item)]
    return [value]


TEST_FN = re.compile(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)")
INLINE_MOD = re.compile(r"^[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{", re.M)


@functools.lru_cache(maxsize=None)
def server_tests():
    """Each of the server's tests as cargo names it, with its file: the
    file's module (`uploads::sniff`), a module inline in it where the test
    follows one (`tests`), and the test's own name. Read from the source, so
    a module nested deeper than one inline level is read as its first."""
    found = []
    for path in sorted(SERVER.rglob("*.rs")):
        text = path.read_text()
        if "#[test]" not in text:
            continue
        parts = path.relative_to(SERVER).with_suffix("").parts
        base = [p for p in parts if p not in ("main", "mod")]
        mods = [(m.start(), m.group(1)) for m in INLINE_MOD.finditer(text)]
        for test in TEST_FN.finditer(text):
            inline = [name for at, name in mods if at < test.start()][-1:]
            found.append(("::".join(base + inline + [test.group(1)]), path))
    return tuple(found)


def reached(word):
    """The files of the server's tests a filter word runs: cargo runs each
    test whose name, with its module's, holds the word."""
    return {path for name, path in server_tests() if word in name}


def commands(value):
    """Each `cargo test` command naming the server, wherever a script's
    value holds one: a list, or a suite's (command, environment) tuple."""
    if isinstance(value, dict):
        return [c for v in value.values() for c in commands(v)]
    if isinstance(value, (list, tuple)):
        if value[:2] in (["cargo", "test"], ("cargo", "test")) and all(isinstance(w, str) for w in value):
            return [list(value)] if "pw-dev-server" in value else []
        return [c for v in value for c in commands(v)]
    return []


def unread_word_tests(script, module):
    """A server test a script runs by a word of its name whose file none of
    its `*SOURCES` lists: its failures would be INVALID, never kills. Only a
    script whose sources name a server file reads the server's kills by
    them; another reads them some other way, and is left."""
    lists = [v for k, v in vars(module).items() if k.endswith("SOURCES")]
    listed = {pathlib.Path(s).resolve() for s in flat(lists) if isinstance(s, (str, pathlib.Path))}
    server = SERVER.resolve()
    if not any(server in p.parents for p in listed):
        return []
    words = set()
    for value in vars(module).values():
        for command in commands(value):
            if "--" in command:
                after = command[command.index("--") + 1:]
                words.update(w for w in after if not w.startswith("-") and not MODULE_FILTER.match(w))
    unlisted = {}
    for word in sorted(words):
        for path in reached(word):
            if path.resolve() not in listed:
                unlisted.setdefault(path, []).append(f"`{word}`")
    return [
        f"{script.name}: a command runs the server's tests by {' and '.join(by)}, and its "
        f"sources list not {path.relative_to(ROOT)}: its failures would be INVALID"
        for path, by in sorted(unlisted.items())
    ]


def unread_tests(script, module):
    """A server test module a script runs by name whose file its `SOURCES`
    does not list: its failures would be INVALID, never kills."""
    tests = getattr(module, "TESTS", None)
    sources = getattr(module, "SOURCES", None)
    if not tests or not sources:
        return []
    listed = {pathlib.Path(s).resolve() for s in flat(sources) if isinstance(s, (str, pathlib.Path))}
    problems = []
    for entry in tests if isinstance(tests, (list, tuple)) else [tests]:
        words = [w for w in flat(entry) if isinstance(w, str)]
        if "pw-dev-server" not in words:
            continue
        for word in words:
            if not MODULE_FILTER.match(word):
                continue
            found = module_file(word)
            if found is not None and found.resolve() not in listed:
                problems.append(
                    f"{script.name}: TESTS runs `{word}`, and SOURCES lists not "
                    f"{found.relative_to(ROOT)}: its failures would be INVALID"
                )
    return problems


def main():
    scripts = sorted((ROOT / "scripts").glob("*_mutations.py"))
    total, problems = 0, []
    for script in scripts:
        read, found = problems_in(script)
        total += read
        problems.extend(found)
    for p in problems:
        print(p)
    if problems:
        print(f"FAIL: {len(problems)} of {total} mutants cannot run as written")
        return 1
    print(f"mutation anchors: {total} mutants in {len(scripts)} scripts, each matching once")
    return 0


if __name__ == "__main__":
    sys.exit(main())
