#!/usr/bin/env python3
"""**A mutant of a program's source is still a program** (the integrator's
finding, 2026-10-14), or the checker's refusal of it is what it undoes.

A mutation script's mutant of a `.pw` source runs the tests on the program
it leaves. Where the checker refuses that program, every test that builds
it fails, and the script reads "KILLED (13 of 13 tests fail)": a kill by
the build, whatever the tests hold. `store_hours_mutations.py`'s "an order
is placed whatever the hours" was one from the day order-total gave
`UserOrders.place` three arguments, and its mutant still called it with
one; two more were (a `&&` the language does not have, and a list of
another type). Where the checker's rule is the protection a mutant takes
away (a user's data cached `shared` is refused, PW5001), the script says so,
by that rule: `CHECKED = {"what": {"PW5001"}}` beside its `MUTANTS`.

**Every program a mutation script mutates** (the integrator's ruling of
2026-10-15, track evidence-integrity-2), found from the mutants' own paths.
Until then a list named four programs (the store, the feed, kiokun and its
site), and a mutant of any other source was skipped unread: the benchmark's
store, a rejected fixture, and `examples/demo`, whose mutants track
argument-invariants checked in its own script.
- A mutant's **root** is its source's directory; or the source alone where
  the directory's sources declare a module twice (a corpus of fixtures, each
  its own program, E2B).
- A root's **program** is the standard packages (`packages/*/*.pw`), the
  root's sources, and every source they import, transitively: an import is
  the source that declares its module nearest the importing source, by the
  steps between their directories (the same directory, `examples/domain.pw`
  above it, `examples/lib/` beside it); a tie, or none, fails. A fixture
  root's own directory is no source of its imports.
- A mutant is checked in **every program that holds its source**: a
  library's in each program that imports it, a standard package's in all.

For each, the program is checked in a scratch copy before and after the
mutant. It passes where the mutant adds no error, or only errors of the
codes its script names for it; a script that names codes for a mutant that
adds none in any program that holds its source is told so too. A standard
package's mutant is refused only where a program uses what it takes away
(track `device-store`, 2026-10-20: the platform's `on:blur` taken away
refuses TodoMVC's program, PW5022, and leaves every other one checking), so
its names are met by the programs that refuse it. Non-vacuity: every mutant
of a `.pw` source is judged in at least one program, or fails.

    python3 scripts/mutant_programs.py PW

where PW is a built `pw` (`target/debug/pw`). Exits 1 on any failure.
"""

from __future__ import annotations

import concurrent.futures
import fnmatch
import importlib.util
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The standard packages, which every program holds.
STD = "packages/*/*.pw"

MODULE = re.compile(r"^[ \t]*module[ \t]+([A-Za-z_][\w.]*)", re.M)
IMPORT = re.compile(r"^[ \t]*import[ \t]+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)", re.M)

# Full 344-mutant / 560-check census matched exactly at 2, 4 and 6 workers;
# in the 2026-10-24 Linux CI benchmark, four was the quickest observed.
# CPU contention may outweigh additional threads beyond four.
WORKERS = 4


def load(script: pathlib.Path):
    spec = importlib.util.spec_from_file_location(script.stem, script)
    module = importlib.util.module_from_spec(spec)
    sys.path.insert(0, str(script.parent))
    try:
        spec.loader.exec_module(module)
    finally:
        sys.path.pop(0)
    return module


def mutants():
    """Each mutant of a `.pw` source: (script, what, path from the root,
    anchor, replacement, the codes its script names for it). A path outside
    the repository is kept as written, and fails: it names no program."""
    out = []
    for script in sorted((ROOT / "scripts").glob("*_mutations.py")):
        module = load(script)
        checked = getattr(module, "CHECKED", {})
        for m in getattr(module, "MUTANTS", []):
            path = next((x for x in m if isinstance(x, pathlib.Path)), None)
            strs = [x for x in m if isinstance(x, str)]
            if path is None or path.suffix != ".pw" or len(strs) < 3:
                continue
            try:
                rel = path.resolve().relative_to(ROOT).as_posix()
            except ValueError:
                rel = str(path)
            what = strs[0]
            out.append((script.name, what, rel, strs[-2], strs[-1], set(checked.get(what, ()))))
    return out


def sources(root: pathlib.Path) -> dict[str, str]:
    """Every `.pw` source, by its path from `root`, with its text: those git
    tracks, or, outside a repository, every one under it."""
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.pw"], cwd=root, capture_output=True, text=True
    )
    if listed.returncode == 0 and (root / ".git").exists():
        paths = [p for p in listed.stdout.split("\0") if p]
    else:
        paths = sorted(
            p.relative_to(root).as_posix()
            for p in root.rglob("*.pw")
            if not {"node_modules", "target", ".git"} & set(p.relative_to(root).parts)
        )
    return {p: (root / p).read_text(errors="replace") for p in paths if (root / p).is_file()}


def steps(a: str, b: str) -> int:
    """The steps between two directories, up to what they share and down."""
    pa, pb = pathlib.PurePosixPath(a).parts, pathlib.PurePosixPath(b).parts
    shared = 0
    while shared < min(len(pa), len(pb)) and pa[shared] == pb[shared]:
        shared += 1
    return (len(pa) - shared) + (len(pb) - shared)


def parent(rel: str) -> str:
    return pathlib.PurePosixPath(rel).parent.as_posix()


def is_std(rel: str) -> bool:
    """Whether a source is a standard package's (`STD`), segment by segment."""
    parts, pattern = rel.split("/"), STD.split("/")
    return len(parts) == len(pattern) and all(fnmatch.fnmatchcase(a, b) for a, b in zip(parts, pattern))


class Programs:
    """The repository's sources, by the module each declares, and each
    root's program."""

    def __init__(self, root: pathlib.Path):
        self.text = sources(root)
        self.module_of: dict[str, str] = {}
        self.declaring: dict[str, list[str]] = {}
        self.names_in: dict[str, list[str]] = {}
        for rel, text in self.text.items():
            m = MODULE.search(text)
            if m:
                self.module_of[rel] = m.group(1)
                self.declaring.setdefault(m.group(1), []).append(rel)
                self.names_in.setdefault(parent(rel), []).append(m.group(1))
        self.std = sorted(rel for rel in self.text if is_std(rel))

    def root_of(self, rel: str) -> str:
        """A source's root: its directory, or itself where its directory's
        sources declare a module twice."""
        names = self.names_in.get(parent(rel), [])
        return rel if len(names) != len(set(names)) else parent(rel)

    def members(self, root: str) -> list[str]:
        """A root's own sources."""
        if root in self.text:
            return [root]
        return sorted(rel for rel in self.text if parent(rel) == root)

    def resolve(self, module: str, importer: str, excluded: set[str]) -> tuple[str | None, str]:
        """The source of `module` nearest `importer`, and why none where none
        is: (source, "") or (None, why)."""
        candidates = [c for c in self.declaring.get(module, []) if c not in excluded and c != importer]
        if not candidates:
            return None, "no source declares it"
        here = parent(importer)
        best = min(steps(here, parent(c)) for c in candidates)
        nearest = sorted(c for c in candidates if steps(here, parent(c)) == best)
        if len(nearest) > 1:
            return None, f"{len(nearest)} sources as near declare it: {', '.join(nearest[:4])}"
        return nearest[0], ""

    def program(self, root: str) -> tuple[list[str], list[str]]:
        """A root's program, as its sources, and each import it could not
        resolve."""
        own = self.members(root)
        excluded = set()
        if root in self.text:
            # A fixture: its directory's other sources are other programs.
            excluded = {rel for rel in self.text if parent(rel) == parent(root) and rel != root}
        held = set(self.std) | set(own)
        modules = {self.module_of.get(rel) for rel in held}
        queue = list(own)
        problems = []
        while queue:
            rel = queue.pop()
            for module in IMPORT.findall(self.text[rel]):
                if module in modules:
                    continue
                found, why = self.resolve(module, rel, excluded)
                if found is None:
                    problems.append(f"{rel} imports `{module}`: {why}")
                else:
                    held.add(found)
                    modules.add(module)
                    queue.append(found)
        return sorted(held), sorted(set(problems))


def errors(pw: str, root: pathlib.Path, files: list[str]) -> list[tuple[str, str]]:
    """A program's errors, as (code, message), each once."""
    out = subprocess.run([pw, "check", *files], cwd=root, capture_output=True, text=True)
    text = re.sub(r"\x1b\[[0-9;]*m", "", out.stdout + out.stderr)
    found = re.findall(r"error: \[(PW\d{4})\] (.*)", text)
    return sorted(set(found))


# A mutant its script names codes for, which adds no error in one program:
# wrong only where it is so in every program that holds its source (`main`).
CLEAN = "clean"


def judge(pw: str, root: pathlib.Path, files: list[str], base: list, m) -> str | None:
    """Why the mutant `m` fails the rule in the program of `files`, `CLEAN`
    where its script names codes it adds none of here, or none where it
    passes."""
    script, what, rel, anchor, replacement, named = m
    target = root / rel
    original = target.read_text()
    if original.count(anchor) != 1:
        return None  # `mutation_anchors.py`'s to say
    try:
        target.write_text(original.replace(anchor, replacement, 1))
        after = errors(pw, root, files)
    finally:
        target.write_text(original)
    added = [e for e in after if e not in base]
    codes = {code for code, _ in added}
    if not added and named:
        return CLEAN
    if added and not codes <= named:
        shown = "; ".join(f"[{c}] {msg}" for c, msg in added[:3])
        return f"{script}: {what}: leaves a program the checker refuses: {shown}"
    return None


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    pw = str(pathlib.Path(sys.argv[1]).resolve())
    found = mutants()
    programs = Programs(ROOT)
    failures: list[str] = []
    roots = sorted({programs.root_of(m[2]) for m in found if m[2] in programs.text})
    held: dict[str, list[str]] = {}
    for root in roots:
        files, problems = programs.program(root)
        held[root] = files
        failures += [f"{root}'s program: {p}" for p in problems]
    # Each mutant, in every program that holds its source.
    pairs = []
    for m in found:
        within = [r for r in roots if m[2] in held[r]]
        if not within:
            failures.append(f"{m[0]}: {m[1]}: its source {m[2]} is in no program found")
        pairs += [(r, m) for r in within]
    scratch = pathlib.Path(tempfile.mkdtemp(prefix="pw-mutant-programs-"))
    trees = []
    # Each mutant's verdict in each program, by whether it added nothing
    # where its script names codes.
    clean: dict[tuple[str, str], list[bool]] = {}
    try:
        needed = sorted({f for r in roots for f in held[r]})
        for i in range(WORKERS):
            tree = scratch / str(i)
            for rel in needed:
                (tree / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / rel, tree / rel)
            trees.append(tree)
        base = {r: errors(pw, trees[0], held[r]) for r in roots}

        def worker(i: int) -> list[tuple[tuple[str, str], str | None]]:
            return [((m[0], m[1]), judge(pw, trees[i], held[r], base[r], m)) for r, m in pairs[i::WORKERS]]

        with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
            for out in pool.map(worker, range(WORKERS)):
                for key, why in out:
                    clean.setdefault(key, []).append(why == CLEAN)
                    if why is not None and why != CLEAN:
                        failures.append(why)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    # A mutant whose script names codes for it, which every program that
    # holds its source checks: the names are met nowhere.
    for m in found:
        if m[5] and clean.get((m[0], m[1])) and all(clean[(m[0], m[1])]):
            failures.append(f"{m[0]}: {m[1]}: names {sorted(m[5])}, and the program it leaves checks")
    judged = {(m[0], m[1]) for _, m in pairs}
    if not found:
        failures.append("no mutant of a `.pw` source found: the gate read nothing")
    for f in sorted(set(failures)):
        print(f"FAIL: {f}")
    named = sum(1 for m in found if m[5])
    print(
        f"mutant programs: {len(found)} mutants of a program's source, {len(judged)} judged, in "
        f"{len(roots)} programs ({', '.join(roots)}); {len(pairs)} checks; {named} refused by the rule "
        f"their script names, {len(set(failures))} failing"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
