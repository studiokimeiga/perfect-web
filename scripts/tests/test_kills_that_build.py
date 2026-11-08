"""**A mutant counted killed because it does not build controls nothing**
(NEXT's item 8, train-16), unless what refuses it is the protection it takes
away: Pleris's checker, named by its codes in its script's `CHECKED` and
held to them by `scripts/mutant_programs.py`.

Every other "KILLED (does not build)" was a replacement gone stale, written
against code that had since changed: six were counted killed in the records
for weeks (template-values, document-reads, menu-changed twice, orders,
store-postgres), and cross-session's until train-15. This holds every record
under `docs/evidence` to it. A record kept from before a repair, to be read
beside the record after it, is history (`BEFORE`).

    python3 -m unittest scripts/tests/test_kills_that_build.py
"""

import pathlib
import re
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import mutant_programs  # noqa: E402

EVIDENCE = ROOT / "docs/evidence"

# A mutant's verdict line, its suite's name (`[server]`) left out.
LINE = re.compile(r"^(?P<what>.*?)(?: \[[a-z]+\])?: KILLED \(does not build\)$")

# Records of a state before a repair, kept to be compared with the record
# after it, by their paths from the repository.
BEFORE = {
    "docs/evidence/E14/train10-d1-result-parser-before.txt": "train-10's result parser, before its repair",
    "docs/evidence/E14/train10-each-producer-before.txt": "train-10's each producer, before its repair",
}


def refused_by_the_checker() -> set[str]:
    """Each mutant whose script names the checker's codes for it."""
    return {what for _, what, _, _, _, codes in mutant_programs.mutants() if codes}


def kills_that_do_not_build(declared: set[str]) -> list[str]:
    """Each record's line that counts a mutant killed because it does not
    build, where nothing declares the checker refuses it."""
    found = []
    for path in sorted(EVIDENCE.rglob("*.txt")):
        rel = path.relative_to(ROOT).as_posix()
        if rel in BEFORE:
            continue
        for n, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
            m = LINE.match(line)
            if m and m.group("what") not in declared:
                found.append(f"{rel}:{n}: {m.group('what')}")
    return found


class KillsThatBuild(unittest.TestCase):
    def test_no_record_counts_a_mutant_killed_because_it_does_not_build(self):
        self.assertEqual(kills_that_do_not_build(refused_by_the_checker()), [])

    def test_a_line_names_its_mutant_whatever_its_suite(self):
        m = LINE.match("a menu change's route tells no page reading a search [server]: KILLED (does not build)")
        self.assertEqual(m.group("what"), "a menu change's route tells no page reading a search")
        self.assertIsNone(LINE.match("a mutant: KILLED (1 of 4 tests fail)"))

    def test_each_record_kept_from_before_a_repair_is_there(self):
        for rel in BEFORE:
            self.assertTrue((ROOT / rel).is_file(), rel)


if __name__ == "__main__":
    unittest.main()
