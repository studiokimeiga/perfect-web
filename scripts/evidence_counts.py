#!/usr/bin/env python3
"""**An evidence file's mutants are its script's** (track evidence-integrity,
finding 2, 2026-10-15).

`docs/evidence/E10/privacy-by-resolution.txt`, recorded at 334bf88 on
2026-09-26, said "4 of 4 mutants killed", one of them "the body's label is
read by spelling", which e0752896 (ADR-0256, 2026-10-07) retired from
`scripts/privacy_resolution_mutations.py`. Nothing compared an evidence
file with the script that wrote it: the record claimed a mutant its script
no longer plants, and a count its script no longer makes.

For each recipe that runs a mutation script, each `k of n mutants killed`
line of each evidence file the recipe writes is read with the verdict lines
above it, back to the count before it:
- each verdict names a mutant of one of the recipe's scripts, each mutant
  once: by its description, the longest of the words before its path,
  beside nothing but its other words, a suite's tag in brackets, and
  separators (`[core]`, `next-react: `, `V1  `);
- `n` is as many mutants as that script defines and as many verdicts as the
  block holds, and `k` as many as say KILLED;
- and each script the recipe runs is counted in one block.

A recipe whose evidence disagrees is **stale and planned** where a
verification run records it again: it runs on CI (not `LOCAL_ONLY`), every
file it writes names the commit it was recorded at (or is missing), and the
planner holds that evidence not current in the tree here, so a train's
push plans it (`ci_plan.py --stale`). Evidence of record comes only from a
run (ADR-0281), so such a recipe is named and fails nothing. Any other
disagreement fails: no plan would record it again.

Prints each disagreement, `path:line: what`, then each recipe's state, and
exits 1 where one fails.

    python3 scripts/evidence_counts.py
"""

import importlib.util
import pathlib
import re
import shlex
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

COUNT = re.compile(r"^(\d+) of (\d+) mutants killed")
# What a script says of each mutant (`mutation_anchors.py`'s shapes, and the
# verdicts the scripts print). A kill may say what killed it, as the guide's
# script says (`KILLED by the checker (PW5048)`, ADR-0343): until train-10's
# landing such a verdict was read as none, and the guide's evidence, once
# current, named none of its 74 mutants. A producer's script says `KILLED` and
# no more, as the session reader's does.
VERDICT = re.compile(
    r"^(.*): (KILLED(?: by [^()]+ \(.*\)| \(.*\))?|SURVIVED|DID NOT BUILD.*|ANCHOR NOT FOUND EXACTLY ONCE in .+)$"
)
# A suite's tag beside a mutant's description: `[core]`, `[server]`.
TAG = re.compile(r"\[[\w./ -]*\]")
SCRIPT = re.compile(r"scripts/([a-z0-9_]+_mutations\.py)")
# **A recipe that runs a part of a script's mutants** (the integrator's
# ruling 1 of 2026-10-17, train-10: `e14-refusal` no longer fit a CI job):
# `--part i/n`, the i-th of the script's `PARTS`, ranges of its `MUTANTS`.
# The recipe's count is of that part, and the parts together, each run by a
# recipe, hold every mutant exactly once (`split`).
PART = re.compile(r"scripts/([a-z0-9_]+_mutations\.py) --part (\d+)/(\d+)")


def executed_scripts(body: str) -> list[str]:
    """Literal Python campaign invocations, not hash inputs or quoted prose.
    Dynamic shell execution is outside this static-reference model."""
    lexer = shlex.shlex(body.replace("\\\n", " "), posix=True, punctuation_chars=";&|(){}\n")
    lexer.whitespace = " \t\r"
    lexer.whitespace_split = True
    tokens = list(lexer)
    found = set()
    command = True
    for at, token in enumerate(tokens):
        if token and all(ch in ";&|(){}\n" for ch in token):
            command = True
            continue
        if not command:
            continue
        word = token.lstrip("@")
        if not word or word in {"if", "then", "do", "!", "env"} or re.match(r"^[A-Za-z_]\w*=", word):
            continue
        command = False
        if not re.fullmatch(r"python(?:3(?:\.\d+)?)?", pathlib.PurePosixPath(word).name):
            continue
        at += 1
        while at < len(tokens):
            option = tokens[at]
            if option in {"-X", "-W", "--check-hash-based-pycs"}:
                at += 2
            elif option in {"-B", "-u", "-E", "-s", "-S", "-I", "-O", "-OO"} or option.startswith(("-X", "-W")):
                at += 1
            else:
                break
        if at >= len(tokens):
            continue
        if tokens[at] == "--":
            at += 1
        if at >= len(tokens):
            continue
        candidate = tokens[at].removeprefix("./")
        if candidate == "-m" and at + 1 < len(tokens):
            candidate = tokens[at + 1].replace(".", "/") + ".py"
        script = SCRIPT.fullmatch(candidate)
        if script:
            found.add(script[1])
    return sorted(found)


def split(mutants: int, parts: list, run: dict[int, list[str]], n: int) -> list[str]:
    """What is wrong with a script's split of its `mutants` (how many) into
    `parts` (ranges of them), where the recipes `run` (by part, from 1) ask
    for parts of `n`: a part no recipe runs, or two do; a mutant in no part,
    or in two. Nothing, where the parts are a floor no mutant falls through."""
    out = []
    if n != len(parts):
        out.append(f"is asked for parts of {n}; its script defines {len(parts)}")
    for i in range(1, len(parts) + 1):
        if not run.get(i):
            out.append(f"part {i} of {len(parts)} is run by no recipe")
        elif len(run[i]) > 1:
            out.append(f"part {i} of {len(parts)} is run by {len(run[i])} recipes: {', '.join(run[i])}")
    held = [0] * mutants
    for start, end in parts:
        for k in range(max(start, 0), min(end, mutants)):
            held[k] += 1
    none = sum(1 for h in held if h == 0)
    if none:
        out.append(f"its parts leave {none} of its {mutants} mutants to none")
    twice = sum(1 for h in held if h > 1)
    if twice:
        out.append(f"its parts hold {twice} of its {mutants} mutants twice")
    return out


def load(path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def planner(root: pathlib.Path = ROOT):
    return load(root / "scripts/ci_plan.py")


def words(mutant: tuple) -> tuple[str, list[str]]:
    """A mutant's description, the longest of its words before its path,
    and its other words."""
    said = [w for w in mutant[:-3] if isinstance(w, str)]
    if not said:
        return "", []
    # A producer may carry its intended test method beside the description.
    # The method's identifier is metadata even when longer than the prose.
    descriptions = [i for i, word in enumerate(said) if not re.fullmatch(r"(?:\w+\.)+test_\w+", word)]
    i = max(descriptions or range(len(said)), key=lambda k: len(said[k]))
    return said[i], said[:i] + said[i + 1 :]


def names(said: str, mutant: tuple) -> bool:
    """Whether a verdict's `said` names `mutant`: its description, beside
    nothing but its other words, a suite's tag and separators."""
    description, others = words(mutant)
    if not description or description not in said:
        return False
    rest = said.replace(description, " ", 1)
    for w in others:
        rest = rest.replace(w, " ", 1)
    return TAG.sub(" ", rest).strip(" :") == ""


def blocks(text: str) -> list[tuple[int, int, int, list[tuple[int, str, str]]]]:
    """Each count in `text`: (its line, k, n, the verdicts above it back to
    the count before, each (line, what it names, its verdict))."""
    out, verdicts = [], []
    for at, line in enumerate(text.splitlines(), 1):
        v = VERDICT.match(line)
        if v:
            verdicts.append((at, v.group(1), v.group(2)))
            continue
        c = COUNT.match(line)
        if c:
            out.append((at, int(c.group(1)), int(c.group(2)), verdicts))
            verdicts = []
    return out


def disagree(
    mutants: list, k: int, n: int, verdicts: list, whose: str = "its script defines"
) -> list[tuple[int | None, str]]:
    """What disagrees between one count, the verdicts above it and the
    script's `mutants` (or its part's, `whose` says): (a line, or None for
    the count's, and what)."""
    out: list[tuple[int | None, str]] = []
    if n != len(mutants):
        out.append((None, f"counts {n} mutants; {whose} {len(mutants)}"))
    if len(verdicts) != n:
        out.append((None, f"counts {n} mutants above {len(verdicts)} verdicts"))
    killed = sum(1 for _, _, verdict in verdicts if verdict.startswith("KILLED"))
    if k != killed:
        out.append((None, f"counts {k} killed; {killed} verdicts say KILLED"))
    taken: set[int] = set()
    for at, said, _ in verdicts:
        found = next(
            (i for i, m in enumerate(mutants) if i not in taken and names(said, m)), None
        )
        if found is None:
            again = any(names(said, m) for m in mutants)
            out.append((at, f"{said!r} names {'a mutant named before' if again else 'no mutant of its script'}"))
        else:
            taken.add(found)
    return out


def owner(scripts: dict[str, list], verdicts: list, n: int) -> str | None:
    """The script of `scripts` whose mutants most of a block's verdicts name
    (of two that tie, one with as many mutants as counted), or none where
    they name none."""

    def score(s: str) -> tuple[int, bool]:
        named = sum(1 for _, said, _ in verdicts if any(names(said, m) for m in scripts[s]))
        return named, len(scripts[s]) == n

    best = max(scripts, key=score, default=None)
    return best if best is not None and score(best)[0] > 0 else None


def disagreements(
    root: pathlib.Path = ROOT, plan=None
) -> dict[str, list[tuple[str, int, str]]]:
    """Each recipe whose evidence disagrees with its scripts, with each
    disagreement: (a file, its line, what)."""
    plan = plan or planner(root)
    found: dict[str, list[tuple[str, int, str]]] = {}
    loaded: dict[str, list] = {}
    parts_of: dict[str, list] = {}
    # Each script some recipe runs a part of: by part, the recipes that run
    # it, and the parts they ask for (`split`).
    partly: dict[str, tuple[int, dict[int, list[str]], list[str]]] = {}
    for recipe, body in sorted(plan.bodies(root).items()):
        run = executed_scripts(body)
        files = plan.evidence_files(body)
        if not run or not files:
            continue
        asked = {s: (int(i), int(n)) for s, i, n in PART.findall(body)}
        scripts = {}
        whose = {}
        said: list[tuple[str, int, str]] = []
        for s in run:
            if s not in loaded:
                module = load(root / "scripts" / s)
                loaded[s] = list(getattr(module, "MUTANTS", []))
                parts_of[s] = list(getattr(module, "PARTS", []))
            scripts[s] = loaded[s]
            if s in asked:
                i, n = asked[s]
                _, by_part, recipes_of = partly.setdefault(s, (n, {}, []))
                by_part.setdefault(i, []).append(recipe)
                recipes_of.append(recipe)
                if 1 <= i <= len(parts_of[s]):
                    start, end = parts_of[s][i - 1]
                    scripts[s] = loaded[s][start:end]
                    whose[s] = f"its part {i} of {len(parts_of[s])} holds"
                else:
                    said.append((files[0], 0, f"{s}: is asked for part {i} of {n}; it defines {len(parts_of[s])}"))
        counted: set[str] = set()
        for rel in files:
            path = root / rel
            if not path.is_file():
                said.append((rel, 0, "no such file: the recipe's evidence is not recorded"))
                continue
            for at, k, n, verdicts in blocks(path.read_text(errors="replace")):
                s = owner(scripts, verdicts, n)
                if s is None:
                    said.append((rel, at, f"its verdicts name the mutants of none of {', '.join(run)}"))
                    continue
                if s in counted:
                    said.append((rel, at, f"{s}'s mutants are counted twice"))
                    continue
                counted.add(s)
                for line, what in disagree(scripts[s], k, n, verdicts, whose.get(s, "its script defines")):
                    said.append((rel, line or at, f"{s}: {what}"))
        if any(path.is_file() for path in (root / f for f in files)):
            for s in run:
                if s not in counted:
                    said.append((files[0], 0, f"counts none of {s}'s mutants, which the recipe runs"))
        if said:
            found[recipe] = said
    # A script's parts, held together: what is wrong with them is said of
    # every recipe that runs one of them.
    bodies = plan.bodies(root)
    for s, (n, by_part, recipes_of) in sorted(partly.items()):
        for what in split(len(loaded[s]), parts_of[s], by_part, n):
            for recipe in recipes_of:
                files = plan.evidence_files(bodies[recipe])
                found.setdefault(recipe, []).append((files[0], 0, f"{s}: {what}"))
    return found


def planned_again(recipes: list[str], root: pathlib.Path = ROOT, plan=None) -> set[str]:
    """The recipes of `recipes` a train's push plans again, as the planner
    judges the tree here once it is committed: run on CI, and stale
    (`ci_plan.stale`, read from the tree here, not the head)."""
    plan = plan or planner(root)
    names = [r for r in recipes if r not in plan.LOCAL_ONLY]
    if not names:
        return set()

    def show(head: str, path: str) -> str | None:
        try:
            return (root / path).read_text(errors="replace")
        except OSError:
            return None

    def read(head: str, path: str) -> str | None:
        m = plan.RECORDED_AT.search(show(head, path) or "")
        return m.group(1) if m else None

    return set(
        plan.stale(
            names,
            "HEAD",
            body=plan.bodies(root),
            show=show,
            read=read,
            descends_from=lambda sha, head: plan.descends(sha, "HEAD", root),
            reach=lambda sha, head: set(plan.recipes_for(plan.hunks(sha, None), plan.NEAR, names)),
        )
    )


def failing(found: dict, again: set[str]) -> list[str]:
    """The recipes of `found` whose disagreement no plan records again."""
    return sorted(r for r in found if r not in again)


def main() -> int:
    found = disagreements()
    again = planned_again(sorted(found))
    fails = failing(found, again)
    for recipe, said in sorted(found.items()):
        for rel, at, what in said:
            print(f"{rel}:{at}: {what}")
        if recipe in fails:
            print(f"  {recipe}: FAIL: no plan records it again")
        else:
            print(f"  {recipe}: stale, and planned at a train's push (ci_plan.py --stale)")
    print(
        f"evidence counts: {len(found)} recipes disagree with their scripts, "
        f"{len(found) - len(fails)} of them stale and planned, {len(fails)} failing"
    )
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
