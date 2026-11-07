"""`unnumbered_adrs.py`: `master` holds no unnumbered ADR (track
evidence-integrity, finding 6). Read on a tree of its own, with its control;
held of the repository on `master`'s CI and a train's, where every ADR is
numbered.
"""

import importlib.util
import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


adrs = load("unnumbered_adrs")
# Spelled in parts, as the script spells it, so that this file names none.
X = "ADR-" + "XXXX"


class Unnumbered(unittest.TestCase):
    def tree(self, files: dict[str, str]) -> pathlib.Path:
        root = pathlib.Path(tempfile.mkdtemp(prefix="pw-adrs-"))
        for rel, text in files.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True)
        return root

    def test_a_mention_outside_code_and_an_unnumbered_file_are_named(self):
        root = self.tree(
            {
                # As statements_separated.rs held it, and an evidence header.
                "compiler/tests/a.rs": f"// the refusals (writing {X}'s refusals)\nfn a() {{}}\n",
                "docs/evidence/E14/x.txt": f"{X} - a header recorded on a track\n",
                f"docs/DECISIONS/{X}-a-track.md": "# A track's decision\n",
                # A header beside a recorded path is named all the same, and
                # a path to the file outside evidence names the decision.
                "docs/evidence/E14/y.txt": f"{X} - read docs/DECISIONS/{X}-a-track.md\n",
                "scripts/c.py": f"# docs/DECISIONS/{X}-a-track.md says why\n",
                # An elided diffstat path outside evidence names it too; and
                # in evidence `...` alone, with no `/`, is no path.
                "docs/d.md": f" .../{X}-a-track.md | 21 +++\n",
                "docs/evidence/E14/z.txt": f"see ...{X}-a-track.md\n",
            }
        )
        self.assertEqual(
            adrs.unnumbered(root),
            [
                ("compiler/tests/a.rs", 1, f"// the refusals (writing {X}'s refusals)"),
                (f"docs/DECISIONS/{X}-a-track.md", 0, f"{X}-a-track.md"),
                ("docs/d.md", 1, f".../{X}-a-track.md | 21 +++"),
                ("docs/evidence/E14/x.txt", 1, f"{X} - a header recorded on a track"),
                ("docs/evidence/E14/y.txt", 1, f"{X} - read docs/DECISIONS/{X}-a-track.md"),
                ("docs/evidence/E14/z.txt", 1, f"see ...{X}-a-track.md"),
                ("scripts/c.py", 1, f"# docs/DECISIONS/{X}-a-track.md says why"),
            ],
        )

    def test_the_control_a_mention_of_the_placeholder_in_code_and_a_numbered_adr_name_nothing(self):
        root = self.tree(
            {
                # As docs/PARALLEL.md describes the numbering.
                "docs/PARALLEL.md": f"- An ADR is unnumbered: `docs/DECISIONS/{X}-<slug>.md`, `# {X}: ..`.\n",
                "docs/DECISIONS/ADR-0281-a-merge.md": f"Numbering `{X}` rewrites lines (ADR-0281).\n",
                "scripts/b.py": "# ADR-0335: numbered\n",
                # A file that is no text is not read.
                "docs/evidence/E10/c.wasm": f"\0{X}\0",
                # A receipt's inputs, by the paths they had when it was
                # recorded, as train-9's record ADR-0352's (ADR-0344,
                # amended 2026-10-17).
                "docs/evidence/E14/receipt.json": (
                    '{\n  "inputs": {\n'
                    f'    "docs/DECISIONS/{X}-a-track.md": "754c7648",\n'
                    f'    "docs/DECISIONS/{X}-wasi-0.2-not-0.3.md": "9c4db5de"\n'
                    "  }\n}\n"
                ),
                # A diffstat in a record, whose long path `git diff --stat`
                # elides to `...` (train-14, the first press's record).
                "docs/evidence/E14/first-add.log": f" .../{X}-a-track.md     |  21 +++\n",
            }
        )
        self.assertEqual(adrs.unnumbered(root), [])

    def test_master_and_a_train_are_held_and_a_tracks_branch_is_not(self):
        self.assertTrue(adrs.held("refs/heads/master"))
        self.assertTrue(adrs.held("refs/heads/track/train-9"))
        self.assertFalse(adrs.held("refs/heads/track/evidence-integrity"))
        self.assertFalse(adrs.held(None))
        self.assertFalse(adrs.held("refs/heads/master-old"))

    @unittest.skipUnless(
        adrs.held(os.environ.get("GITHUB_REF")),
        "a track keeps its ADRs unnumbered until its merge; master's CI and a train's hold them",
    )
    def test_the_repository_holds_no_unnumbered_adr(self):
        self.assertEqual(adrs.unnumbered(), [])


if __name__ == "__main__":
    unittest.main()
