"""**Evidence keeps an input program as `.pw.txt`, never as `.pw`** (train-14).

Every tracked `.pw` file is a program source the check census
(`scripts/check_build_programs.py`, in `just e14-check-is-build`) must cover,
and a copy of an input kept beside a record is no program: some are refused
on purpose. #84's evidence kept two (`find.pw`, `find-refused.pw`), which
failed the census in train-14's `verify` (run 42390553154), as an evidence
copy had before in the same week. The census runs only in `verify`; this
runs in `just evidence-gates`, on every push.
"""

import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def evidence_programs(root: pathlib.Path) -> list[str]:
    """Each tracked `.pw` file under `docs/evidence/`, at any depth (a git
    pathspec's `*` matches `/`)."""
    out = subprocess.run(
        ["git", "ls-files", "--", "docs/evidence/*.pw"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return sorted(out.split())


class EvidenceKeepsNoProgram(unittest.TestCase):
    def test_the_repository_keeps_no_program_in_evidence(self):
        self.assertEqual(
            evidence_programs(ROOT),
            [],
            "keep an input program in docs/evidence/ as `<name>.pw.txt`: a `.pw` is a source the census must cover",
        )

    def test_a_program_at_any_depth_is_found_and_a_record_of_one_is_not(self):
        root = pathlib.Path(tempfile.mkdtemp(prefix="pw-evidence-"))
        files = {
            "docs/evidence/E14/x/find.pw": "module find\n",
            "docs/evidence/E14/x/kept.pw.txt": "module kept\n",
            "examples/a.pw": "module a\n",
        }
        for rel, text in files.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True)
        self.assertEqual(evidence_programs(root), ["docs/evidence/E14/x/find.pw"])


if __name__ == "__main__":
    unittest.main()
