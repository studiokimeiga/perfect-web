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

    def test_the_device_runtimes_two_parts_hold_its_every_mutant(self):
        # Cut by cost, not count: each part about half the whole's seconds,
        # its `rows` mutants, the costliest, split between the two.
        parts = device_runtime_mutations.PARTS
        self.assertEqual(len(parts), 2)
        held = [i for start, end in parts for i in range(start, end)]
        self.assertEqual(held, list(range(len(device_runtime_mutations.MUTANTS))))
        rows = [
            sum(1 for m in device_runtime_mutations.MUTANTS[start:end] if m[1] == "rows")
            for start, end in parts
        ]
        self.assertLessEqual(max(rows) - min(rows), 5, rows)


if __name__ == "__main__":
    unittest.main()
