"""A mutation script's mutants in parts, each part a recipe of its own.

A verify's wall-clock is its slowest shard's, and a recipe runs whole on one
shard: the slowest recipe sets every full verify's length. `d3_postgres`
was split first (ADR-0393, amended 2026-11-17), its `chosen` written in the
script; `row_ownership`, about three hours whole (11,009 s in L5's repairs'
verify), runs in three parts since the owner's grant of 2026-11-17 to make CI
faster with no gate given up (ADR-0359, amended). Each part runs the
unmutated baseline first, as a whole run does; the parts together hold
every mutant once, which `evidence_counts.py` checks of every recipe that
asks for one (`split`).
"""

from __future__ import annotations

import re


def even(count: int, n: int) -> list[tuple[int, int]]:
    """`n` half-open ranges of `count` mutants, in order, as even as whole
    mutants allow: the first parts take the one left over."""
    if n < 1 or count < n:
        raise ValueError(f"{count} mutants cannot make {n} parts")
    size, extra = divmod(count, n)
    out, start = [], 0
    for i in range(n):
        end = start + size + (1 if i < extra else 0)
        out.append((start, end))
        start = end
    return out


def chosen(argv: list[str], mutants: list, parts: list[tuple[int, int]], script: str) -> tuple[list, str]:
    """The mutants a run asks for, and how its output says which: `--part
    i/n`, the i-th of `parts`, where n is how many there are; those whose
    description holds one of the words given; every mutant where nothing is
    asked. Words that name no mutant (a part mistyped, `--part=1/3`, among
    them) refuse to run: the baseline alone would pass."""
    if argv[:1] == ["--part"]:
        asked = re.fullmatch(r"(\d+)/(\d+)", argv[1]) if len(argv) == 2 else None
        if asked is None or int(asked.group(2)) != len(parts) or not 1 <= int(asked.group(1)) <= len(parts):
            raise SystemExit(f"{script}: ask for every mutant, words, or --part i/{len(parts)}")
        i = int(asked.group(1))
        start, end = parts[i - 1]
        return mutants[start:end], f"part {i} of {len(parts)}: mutants {start + 1} to {end} of {len(mutants)}"
    picked = [m for m in mutants if not argv or any(w in m[0] for w in argv)]
    if not picked:
        raise SystemExit(f"{script}: no mutant's description holds {' or '.join(map(repr, argv))}")
    return picked, ""
