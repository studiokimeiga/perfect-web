"""`scripts/mutation_parts.py`: a mutation script's mutants in parts, each a
recipe of its own (ADR-0359, amended 2026-11-17), and `row_ownership`'s
three and `device_runtime`'s two, held to every mutant once."""

from __future__ import annotations

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
import mutation_parts  # noqa: E402 - after the path, as the tests beside it
import device_runtime_mutations  # noqa: E402
import row_ownership_mutations  # noqa: E402


class Parts(unittest.TestCase):
    def test_even_parts_hold_every_mutant_once_in_order(self):
        self.assertEqual(mutation_parts.even(241, 3), [(0, 81), (81, 161), (161, 241)])
        self.assertEqual(mutation_parts.even(9, 3), [(0, 3), (3, 6), (6, 9)])
        for count, n in ((1, 1), (7, 2), (100, 7)):
            parts = mutation_parts.even(count, n)
            held = [i for start, end in parts for i in range(start, end)]
            self.assertEqual(held, list(range(count)))
            sizes = [end - start for start, end in parts]
            self.assertLessEqual(max(sizes) - min(sizes), 1)
        with self.assertRaises(ValueError):
            mutation_parts.even(2, 3)

    def test_a_part_is_its_range_and_says_so(self):
        mutants = [(f"m{i}", None, "", "") for i in range(9)]
        parts = mutation_parts.even(9, 3)
        picked, which = mutation_parts.chosen(["--part", "2/3"], mutants, parts, "x.py")
        self.assertEqual([m[0] for m in picked], ["m3", "m4", "m5"])
        self.assertEqual(which, "part 2 of 3: mutants 4 to 6 of 9")
        picked, which = mutation_parts.chosen([], mutants, parts, "x.py")
        self.assertEqual((len(picked), which), (9, ""))
        picked, which = mutation_parts.chosen(["m8"], mutants, parts, "x.py")
        self.assertEqual([m[0] for m in picked], ["m8"])

    def test_a_part_asked_wrongly_or_a_word_that_names_no_mutant_refuses(self):
        mutants = [(f"m{i}", None, "", "") for i in range(9)]
        parts = mutation_parts.even(9, 3)
        for argv in (["--part", "4/3"], ["--part", "1/2"], ["--part", "0/3"], ["--part"],
                     ["--part=1/3"], ["no such mutant"]):
            with self.subTest(argv), self.assertRaises(SystemExit):
                mutation_parts.chosen(argv, mutants, parts, "x.py")

    def test_row_ownerships_three_parts_hold_its_every_mutant(self):
        parts = row_ownership_mutations.PARTS
        self.assertEqual(len(parts), 3)
        held = [i for start, end in parts for i in range(start, end)]
        self.assertEqual(held, list(range(len(row_ownership_mutations.MUTANTS))))

    def test_the_device_runtimes_three_parts_hold_its_every_mutant(self):
        # Part 1 the decision's alone; parts 2 and 3 cut by cost, not count,
        # their `rows` mutants, the costliest, split between the two.
        parts = device_runtime_mutations.PARTS
        mutants = device_runtime_mutations.MUTANTS
        self.assertEqual(len(parts), 3)
        held = [i for start, end in parts for i in range(start, end)]
        self.assertEqual(held, list(range(len(mutants))))
        start, end = parts[0]
        self.assertEqual({m[1] for m in mutants[start:end]}, {"decision"})
        rows = [sum(1 for m in mutants[a:b] if m[1] == "rows") for a, b in parts[1:]]
        self.assertLessEqual(max(rows) - min(rows), 5, rows)

    def test_a_change_near_a_device_runtime_mutant_plans_each_of_its_recipes(self):
        # Each recipe names the script (the split's review: the main recipe
        # ran no part, and a change near 169 of its 225 mutants no longer
        # planned it, nor its tests in three engines). One mutant of each part
        # and of each suite, as the planner reads a change near it.
        import ci_plan

        mutants = device_runtime_mutations.MUTANTS
        sample = {}
        for i, (_, suite, path, anchor, _) in enumerate(mutants):
            part = next(n for n, (a, b) in enumerate(device_runtime_mutations.PARTS) if a <= i < b)
            sample.setdefault((part, suite), (path, anchor))
        names = list(ci_plan.bodies())
        recipes = {"e14-device-runtime", "e14-device-runtime-2", "e14-device-runtime-3"}
        for (part, suite), (path, anchor) in sorted(sample.items(), key=lambda kv: (kv[0][0], kv[0][1])):
            text = path.read_text()
            line = text[: text.index(anchor)].count("\n") + 1
            rel = str(path.resolve().relative_to(ci_plan.ROOT))
            with self.subTest(part=part + 1, suite=suite):
                planned = set(ci_plan.recipes_for({rel: [(line, line)]}, ci_plan.NEAR, names))
                self.assertLessEqual(recipes, planned)

if __name__ == "__main__":
    unittest.main()
