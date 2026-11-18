"""`evidence_bundles.py`: a campaign's bundle is kept while a record names it
(ADR-0245, amended 2026-11-18). Read on trees of their own, each a git
repository whose fixtures are tracked, with their controls; and held of the
repository (`Repository`, outside the mutation controls' suite: a campaign
run beside them would leave its bundle untracked, which this holds to no
record).
"""

import importlib.util
import io
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bundles = load("evidence_bundles")

RECIPES = {
    "justfile": "import 'just/campaign.just'\n",
    "just/campaign.just": (
        "e14-sessions:\n"
        '    @bundle="$(mktemp -d docs/evidence/E14/session-commands.XXXXXX)"\n'
        "e14-receipts:\n"
        '    @bundle="$(mktemp -d "docs/evidence/E14/receipts.XXXXXX")"\n'
    ),
}
NEW = "docs/evidence/E14/session-commands.NEW123"
OLD = "docs/evidence/E14/session-commands.OLD456"
NAMING_NEW = f"campaign: {NEW}/campaign/report.json\n"


def tree(case: unittest.TestCase, tracked: dict[str, str], untracked: dict[str, str] | None = None) -> pathlib.Path:
    """A git repository holding `tracked` in its index, and `untracked` beside."""
    root = pathlib.Path(tempfile.mkdtemp(prefix="pw-bundles-"))
    case.addCleanup(shutil.rmtree, root, ignore_errors=True)
    for rel, text in {**RECIPES, **tracked}.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    for rel, text in (untracked or {}).items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    return root


def names(paths) -> list[str]:
    return sorted(p.name for p in paths)


class Bundles(unittest.TestCase):
    def test_the_templates_are_the_recipes_mktemp_directories_quoted_or_not(self):
        self.assertEqual(
            bundles.templates(tree(self, {})),
            [("docs/evidence/E14/receipts", 6), ("docs/evidence/E14/session-commands", 6)],
        )

    def test_a_bundle_is_a_templates_directory_of_as_many_letters_as_its_xs(self):
        root = tree(self, {
            "docs/evidence/E14/session-commands.ab12CD/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands-2026-10-27/report.json": "{}",
            "docs/evidence/E14/session-commands.ab1/report.json": "{}",
            "docs/evidence/E14/session-commands.ab12CDe/report.json": "{}",
            "docs/evidence/E14/session-commands.ab12CD.txt": "a file, not a bundle",
            "docs/evidence/E14/session-commands.abcdef": "a file named as a bundle",
        })
        self.assertEqual(names(bundles.bundles(root)), ["session-commands.ab12CD"])
        self.assertEqual(names(bundles.prune(root)), ["session-commands.ab12CD"])
        self.assertTrue((root / "docs/evidence/E14/session-commands.abcdef").is_file())

    def test_a_bundle_its_evidence_names_is_kept_and_a_superseded_one_goes(self):
        root = tree(self, {
            "docs/evidence/E14/session-command-authority.txt": "produced by: just e14-sessions\n" + NAMING_NEW,
            f"{NEW}/campaign/report.json": "{}",
            f"{OLD}/campaign/report.json": "{}",
        })
        self.assertEqual(names(bundles.named(root)), ["session-commands.NEW123"])
        self.assertEqual(names(bundles.unnamed(root)), ["session-commands.OLD456"])

    def test_a_bundles_own_files_name_no_bundle(self):
        # A campaign's report lists the untracked files it started beside
        # (its `working_tree`), another campaign's bundle among them, and
        # reads none of them: that keeps nothing.
        root = tree(self, {
            "docs/evidence/E14/receipts.txt": "campaign: docs/evidence/E14/receipts.KEPT01/campaign/report.json\n",
            "docs/evidence/E14/receipts.KEPT01/campaign/report.json":
                json.dumps({"working_tree": f"?? {OLD}/\n"}),
            "docs/evidence/E14/receipts.KEPT01/console.txt": f"campaign: {OLD}/campaign/report.json\n",
            f"{OLD}/campaign/report.json": "{}",
        })
        self.assertEqual(names(bundles.named(root)), ["receipts.KEPT01"])
        self.assertEqual(names(bundles.unnamed(root)), ["session-commands.OLD456"])

    def test_a_text_records_other_lines_name_no_bundle(self):
        # A failed check's report lists the bundles it found unnamed: a
        # line that is not a campaign's keeps none of them.
        root = tree(self, {
            "docs/evidence/E14/evidence-bundles.txt": f"AssertionError: ['{OLD}'] != []\n",
            f"{OLD}/campaign/report.json": "{}",
        })
        self.assertEqual(bundles.named(root), set())
        self.assertEqual(names(bundles.unnamed(root)), ["session-commands.OLD456"])

    def test_a_document_outside_the_evidence_keeps_the_bundle_it_cites(self):
        root = tree(self, {
            "docs/DECISIONS/ADR-0001-a.md": "Its evidence is `docs/evidence/E14/session-commands.CITED1/`.\n",
            "docs/evidence/E14/session-commands.CITED1/campaign/report.json": "{}",
        })
        self.assertEqual(names(bundles.named(root)), ["session-commands.CITED1"])
        self.assertEqual(bundles.unnamed(root), [])

    def test_a_bundle_git_does_not_track_is_a_run_here_and_stays(self):
        # A campaign run here makes its bundle first, and names it in its
        # evidence file last: until then no record names it.
        root = tree(self, {
            "docs/evidence/E14/session-command-authority.txt": NAMING_NEW,
            f"{NEW}/campaign/report.json": "{}",
        }, untracked={f"{OLD}/console.txt": "a run still writing\n"})
        self.assertEqual(names(bundles.bundles(root)), ["session-commands.NEW123", "session-commands.OLD456"])
        self.assertEqual(bundles.unnamed(root), [])
        self.assertEqual(bundles.prune(root), [])
        self.assertTrue((root / OLD / "console.txt").is_file())

    def test_prune_removes_the_unnamed_alone_and_says_which(self):
        root = tree(self, {
            "docs/evidence/E14/session-command-authority.txt": NAMING_NEW,
            f"{NEW}/campaign/report.json": "{}",
            f"{OLD}/campaign/report.json": "{}",
            f"{OLD}/console.txt": "old\n",
        })
        self.assertEqual(names(bundles.prune(root)), ["session-commands.OLD456"])
        self.assertFalse((root / OLD).exists())
        self.assertTrue((root / NEW / "campaign/report.json").is_file())
        self.assertTrue((root / "docs/evidence/E14/session-command-authority.txt").is_file())

    def test_the_check_fails_on_an_unnamed_bundle_and_passes_once_pruned(self):
        root = tree(self, {
            "docs/evidence/E14/session-command-authority.txt": NAMING_NEW,
            f"{NEW}/campaign/report.json": "{}",
            f"{OLD}/campaign/report.json": "{}",
        })
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(bundles, "ROOT", root), redirect_stdout(out), redirect_stderr(err):
            self.assertEqual(bundles.main(["--check"]), 1)
        self.assertIn(f"no record names {OLD}/", err.getvalue())
        with mock.patch.object(bundles, "ROOT", root), redirect_stdout(out), redirect_stderr(err):
            self.assertEqual(bundles.main(["--prune"]), 0)
            self.assertEqual(bundles.main(["--check"]), 0)
        self.assertIn(f"removed, no record names it: {OLD}/", out.getvalue())


class Fetch(unittest.TestCase):
    """`evidence_fetch.py` prunes once it has copied a run's evidence, and
    never where it copied nothing."""

    SHA = "a" * 40

    def checkout(self) -> tuple[pathlib.Path, pathlib.Path]:
        """An artifact whose recipe names the new bundle, and a checkout
        whose tracked evidence names the old one."""
        checkout = tree(self, {
            "docs/evidence/E14/session-command-authority.txt": f"campaign: {OLD}/campaign/report.json\n",
            f"{OLD}/campaign/report.json": "{}",
        })
        (checkout / "scripts").mkdir()
        artifact = checkout.parent / (checkout.name + "-artifact")
        self.addCleanup(shutil.rmtree, artifact, ignore_errors=True)
        wrote = {
            "docs/evidence/E14/session-command-authority.txt":
                f"produced by: just e14-sessions\ncommit: {self.SHA}\n" + NAMING_NEW,
            f"{NEW}/campaign/report.json": "{}",
        }
        for rel, text in wrote.items():
            target = artifact / "evidence" / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        (artifact / "results.json").write_text(json.dumps(
            [{"recipe": "e14-sessions", "status": 0, "seconds": 5, "wrote": sorted(wrote)}]))
        return checkout, artifact

    def fetched(self, checkout, artifact, argv, head=SHA) -> tuple[int, str]:
        fetch = load("evidence_fetch")
        run = {"headSha": self.SHA, "url": "https://example.invalid/run/1", "status": "completed",
               "conclusion": "success", "jobs": []}

        def gh(*args):
            if args[:2] == ("run", "view"):
                return json.dumps(run)
            shutil.copytree(artifact, pathlib.Path(args[args.index("--dir") + 1]), dirs_exist_ok=True)
            return ""

        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(fetch, "ROOT", checkout), \
             mock.patch.object(fetch, "SECONDS", checkout / "scripts/ci_seconds.json"), \
             mock.patch.object(fetch, "gh", side_effect=gh), \
             mock.patch.object(fetch, "moved_on", return_value=set()), \
             mock.patch.object(fetch.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout=head)), \
             mock.patch.object(sys, "argv", ["evidence_fetch.py", "1", *argv]), \
             redirect_stdout(out), redirect_stderr(err):
            status = fetch.main()
        return status, out.getvalue() + err.getvalue()

    def test_a_fetch_removes_the_bundle_its_evidence_superseded(self):
        checkout, artifact = self.checkout()
        status, said = self.fetched(checkout, artifact, [])
        self.assertEqual(status, 0, said)
        self.assertTrue((checkout / NEW / "campaign/report.json").is_file())
        self.assertFalse((checkout / OLD).exists(), said)
        self.assertIn(f"removed, no record names it: {OLD}/", said)

    def test_a_refused_fetch_prunes_nothing(self):
        checkout, artifact = self.checkout()
        (artifact / "evidence/docs/evidence/E14/session-command-authority.txt").write_text(
            f"commit: {'b' * 40}\n" + NAMING_NEW)
        status, said = self.fetched(checkout, artifact, [])
        self.assertEqual(status, 1, said)
        self.assertTrue((checkout / OLD / "campaign/report.json").is_file(), said)

    def test_a_fetch_that_copied_nothing_prunes_nothing(self):
        checkout, artifact = self.checkout()
        # Its one recipe skipped, nothing is copied; and its evidence there
        # named no bundle, so a prune would take the old one.
        (checkout / "docs/evidence/E14/session-command-authority.txt").write_text("no campaign\n")
        status, said = self.fetched(checkout, artifact, ["--skip", "e14-sessions"])
        self.assertEqual(status, 0, said)
        self.assertTrue((checkout / OLD / "campaign/report.json").is_file(), said)

    def test_a_times_only_fetch_prunes_nothing(self):
        checkout, artifact = self.checkout()
        (checkout / "docs/evidence/E14/session-command-authority.txt").write_text("no campaign\n")
        status, said = self.fetched(checkout, artifact, ["--times-only"])
        self.assertEqual(status, 0, said)
        self.assertTrue((checkout / OLD / "campaign/report.json").is_file(), said)


class Repository(unittest.TestCase):
    def test_the_repository_holds_no_tracked_bundle_no_record_names(self):
        self.assertEqual(
            [str(b.relative_to(bundles.ROOT)) for b in bundles.unnamed()],
            [],
            "python3 scripts/evidence_bundles.py --prune removes them",
        )


if __name__ == "__main__":
    unittest.main()
