#!/usr/bin/env python3
"""**A campaign's bundle is kept while a record names it** (ADR-0245,
amended 2026-11-18).

A campaign recipe writes its run into a directory of its own under
`docs/evidence/`, named by `mktemp -d docs/evidence/<dir>/<name>.XXXXXX`, so
that a run never writes over the last one's files while it runs. Its
evidence file names the bundle (`campaign: …/report.json`), and a bundle's
own files may name another's (a receipt campaign's report names the session
campaign it read). Every run left its bundle behind: by 2026-11-18 the
repository held 22, 2,218 files, and four were named.

A bundle is kept where a record names it: any file under `docs/` outside
the bundles, or a file inside a bundle kept, transitively. Every other
bundle is a superseded run's, and goes. `evidence_fetch.py` prunes after it
copies a run's evidence; `--check` holds the repository to it.

    python3 scripts/evidence_bundles.py --check   # exit 1, naming each unnamed bundle
    python3 scripts/evidence_bundles.py --prune   # remove them, naming each
"""

import pathlib
import re
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
# Where a campaign recipe makes its bundle: `mktemp -d docs/evidence/E14/
# session-commands.XXXXXX`, its template's path before the dot and the X's.
MKTEMP = re.compile(r"mktemp -d (docs/evidence/[\w./-]+?)\.X{6,}")


def templates(root: pathlib.Path = ROOT) -> list[str]:
    """Each bundle's template, as the recipes write it (`docs/evidence/E14/
    session-commands`), from the justfile and every file it imports."""
    found = set()
    for recipe in [root / "justfile", *sorted((root / "just").glob("*.just"))]:
        if recipe.is_file():
            found.update(MKTEMP.findall(recipe.read_text()))
    return sorted(found)


def bundles(root: pathlib.Path = ROOT) -> list[pathlib.Path]:
    """Each bundle the tree holds: a directory named by a template and
    `mktemp`'s six or more letters and digits."""
    out = []
    for template in templates(root):
        parent = (root / template).parent
        if not parent.is_dir():
            continue
        name = re.escape(pathlib.Path(template).name)
        made = re.compile(rf"^{name}\.[A-Za-z0-9]{{6,}}$")
        out.extend(p for p in parent.iterdir() if p.is_dir() and made.match(p.name))
    return sorted(out)


def _names(path: pathlib.Path, wanted: dict[str, pathlib.Path]) -> set[str]:
    """The bundles of `wanted` (by directory name) that `path`'s text names."""
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return set()
    return {name for name in wanted if name in text}


def named(root: pathlib.Path = ROOT) -> set[pathlib.Path]:
    """Each bundle a record names: a file under `docs/` outside the bundles,
    or a file of a bundle so named, transitively."""
    held = bundles(root)
    wanted = {b.name: b for b in held}
    inside = {b.resolve() for b in held}
    if not wanted:
        return set()
    kept: set[str] = set()
    for path in sorted((root / "docs").rglob("*")):
        if not path.is_file():
            continue
        if any(parent in inside for parent in path.resolve().parents):
            continue
        kept |= _names(path, wanted)
    reading = sorted(kept)
    while reading:
        bundle = wanted[reading.pop()]
        for path in sorted(bundle.rglob("*")):
            if path.is_file():
                for name in _names(path, wanted) - kept:
                    kept.add(name)
                    reading.append(name)
    return {wanted[name] for name in kept}


def unnamed(root: pathlib.Path = ROOT) -> list[pathlib.Path]:
    """Each bundle no record names: a superseded run's."""
    kept = named(root)
    return [b for b in bundles(root) if b not in kept]


def prune(root: pathlib.Path = ROOT) -> list[pathlib.Path]:
    """Remove each unnamed bundle; the ones removed."""
    gone = unnamed(root)
    for bundle in gone:
        shutil.rmtree(bundle)
    return gone


def main(argv: list[str]) -> int:
    if argv not in (["--check"], ["--prune"]):
        print("usage: evidence_bundles.py --check | --prune", file=sys.stderr)
        return 2
    if argv == ["--prune"]:
        for bundle in prune(ROOT):
            print(f"  removed, no record names it: {bundle.relative_to(ROOT)}/")
        return 0
    left = unnamed(ROOT)
    for bundle in left:
        print(f"evidence-bundles: no record names {bundle.relative_to(ROOT)}/", file=sys.stderr)
    if left:
        print("evidence-bundles: `python3 scripts/evidence_bundles.py --prune` removes them", file=sys.stderr)
        return 1
    print(f"evidence-bundles: OK ({len(bundles(ROOT))} bundles, each named)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
