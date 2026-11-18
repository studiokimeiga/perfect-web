#!/usr/bin/env python3
"""**A campaign's bundle is kept while a record names it** (ADR-0245,
amended 2026-11-18).

A campaign recipe writes its run into a directory of its own under
`docs/evidence/`, made by `mktemp -d docs/evidence/<dir>/<name>.XXXXXX`, so
that a run never writes over the last one's files while it runs. Its
evidence file names the bundle (`campaign: …/report.json`). Every run left
its bundle behind: on 2026-11-18 master held 22, 2,218 files, and records
named three.

- **A bundle** is a directory named by a recipe's template and as many
  letters and digits as its `X`s. A directory under that name is the
  campaign's.
- **A record names a bundle** in two ways. An evidence file's `campaign:`
  line names its own. A document (a `.md` under `docs/`: an ADR, the
  status) may cite one.
- **Nothing else names a bundle**. A file inside a bundle is no record: a
  campaign's report lists the untracked files it started beside, other
  bundles among them, and reads none of them. Neither is another line of a
  text file, which could list a failed check's bundles.
- **Only a bundle git tracks** can go. A bundle not yet committed is a run's
  here, still writing or not yet fetched, and stays.

`evidence_fetch.py` prunes once it has copied a run's evidence;
`--check` holds the repository to it.

    python3 scripts/evidence_bundles.py --check   # exit 1, naming each tracked bundle no record names
    python3 scripts/evidence_bundles.py --prune   # remove them, naming each
"""

import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
# Where a campaign recipe makes its bundle: `mktemp -d docs/evidence/E14/
# session-commands.XXXXXX`, quoted or not, its template's path before the
# dot, and its X's.
MKTEMP = re.compile(r"""mktemp -d ["']?(docs/evidence/[\w./-]+?)\.(X{6,})""")
# An evidence file's line naming its campaign's bundle.
CAMPAIGN = re.compile(r"^campaign: (\S+)", re.M)


def templates(root: pathlib.Path = ROOT) -> list[tuple[str, int]]:
    """Each bundle's template, as the recipes write it (`docs/evidence/E14/
    session-commands`), and its X's, from the justfile and every file it
    imports."""
    found = set()
    for recipe in [root / "justfile", *sorted((root / "just").glob("*.just"))]:
        if recipe.is_file():
            found.update((path, len(xs)) for path, xs in MKTEMP.findall(recipe.read_text()))
    return sorted(found)


def bundles(root: pathlib.Path = ROOT) -> list[pathlib.Path]:
    """Each bundle the tree holds: a directory named by a template and as
    many letters and digits as its X's."""
    out = []
    for template, width in templates(root):
        parent = (root / template).parent
        if not parent.is_dir():
            continue
        name = re.escape(pathlib.Path(template).name)
        made = re.compile(rf"^{name}\.[A-Za-z0-9]{{{width}}}$")
        out.extend(p for p in parent.iterdir() if p.is_dir() and made.match(p.name))
    return sorted(out)


def tracked(root: pathlib.Path = ROOT) -> set[pathlib.Path]:
    """Each bundle holding a file git tracks; none where git cannot say, so a
    failure prunes less, never more. `Popen` itself: a caller's test that
    answers `subprocess.run` for its own commands answers none of this."""
    try:
        git = subprocess.Popen(["git", "ls-files", "-z", "--", "docs/evidence"], cwd=root,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        listed, _ = git.communicate(timeout=60)
    except (OSError, subprocess.SubprocessError):
        return set()
    if git.returncode != 0 or not isinstance(listed, bytes):
        return set()
    files = {(root / f).resolve() for f in listed.decode(errors="replace").split("\0") if f}
    return {b for b in bundles(root) if any(b.resolve() in f.parents for f in files)}


def named(root: pathlib.Path = ROOT) -> set[pathlib.Path]:
    """Each bundle a record names: an evidence file's `campaign:` line, or a
    document's citation, outside every bundle."""
    held = bundles(root)
    wanted = {b.name: b for b in held}
    inside = {b.resolve() for b in held}
    kept: set[str] = set()
    for path in sorted((root / "docs").rglob("*")):
        if not path.is_file() or path.suffix not in (".txt", ".md"):
            continue
        if any(parent in inside for parent in path.resolve().parents):
            continue
        try:
            text = path.read_text(errors="replace")
        except OSError:
            continue
        if path.suffix == ".txt":
            text = "\n".join(CAMPAIGN.findall(text))
        kept |= {name for name in wanted if name in text}
    return {wanted[name] for name in kept}


def unnamed(root: pathlib.Path = ROOT) -> list[pathlib.Path]:
    """Each tracked bundle no record names: a superseded run's."""
    kept = named(root)
    return sorted(b for b in tracked(root) if b not in kept)


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
    print(f"evidence-bundles: OK ({len(bundles(ROOT))} bundles, each named or a run's here)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
