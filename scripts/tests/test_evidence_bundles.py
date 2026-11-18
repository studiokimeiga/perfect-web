"""`evidence_bundles.py`: a campaign's bundle is kept while a record names
it (ADR-0245, amended 2026-11-18). Read on a tree of its own, with its
controls, and held of the repository.
"""

import importlib.util
import io
import pathlib
import shutil
import tempfile
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
        '    @bundle="$(mktemp -d docs/evidence/E14/receipts.XXXXXX)"\n'
    ),
}


class Bundles(unittest.TestCase):
    def tree(self, files: dict[str, str]) -> pathlib.Path:
        root = pathlib.Path(tempfile.mkdtemp(prefix="pw-bundles-"))
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        for rel, text in {**RECIPES, **files}.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return root

    def names(self, paths) -> list[str]:
        return sorted(p.name for p in paths)

    def test_the_templates_are_the_recipes_mktemp_directories(self):
        root = self.tree({})
        self.assertEqual(
            bundles.templates(root),
            ["docs/evidence/E14/receipts", "docs/evidence/E14/session-commands"],
        )

    def test_a_bundle_is_a_templates_directory_and_its_six_letters_alone(self):
        root = self.tree({
            "docs/evidence/E14/session-commands.ab12CD/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands-2026-10-27/report.json": "{}",
            "docs/evidence/E14/session-commands.ab1/report.json": "{}",
            "docs/evidence/E14/session-commands.ab12CD.txt": "a file, not a bundle",
        })
        self.assertEqual(self.names(bundles.bundles(root)), ["session-commands.ab12CD"])

    def test_a_bundle_its_evidence_names_is_kept_and_a_superseded_one_goes(self):
        root = self.tree({
            "docs/evidence/E14/session-command-authority.txt":
                "campaign: docs/evidence/E14/session-commands.NEW123/campaign/report.json\n",
            "docs/evidence/E14/session-commands.NEW123/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands.OLD456/campaign/report.json": "{}",
        })
        self.assertEqual(self.names(bundles.named(root)), ["session-commands.NEW123"])
        self.assertEqual(self.names(bundles.unnamed(root)), ["session-commands.OLD456"])

    def test_a_bundle_named_by_a_bundle_kept_is_kept_and_by_one_gone_goes(self):
        root = self.tree({
            "docs/evidence/E14/receipts.txt":
                "campaign: docs/evidence/E14/receipts.KEPT01/campaign/report.json\n",
            # The kept receipts' report names the session campaign it read,
            # which names one more: each is kept, transitively.
            "docs/evidence/E14/receipts.KEPT01/campaign/report.json":
                '{"sessions": "docs/evidence/E14/session-commands.READ02/campaign"}',
            "docs/evidence/E14/session-commands.READ02/campaign/report.json":
                '{"before": "session-commands.EARLY3"}',
            "docs/evidence/E14/session-commands.EARLY3/campaign/report.json": "{}",
            # A superseded receipts bundle names a session bundle: both go.
            "docs/evidence/E14/receipts.GONE04/campaign/report.json":
                '{"sessions": "docs/evidence/E14/session-commands.GONE05/campaign"}',
            "docs/evidence/E14/session-commands.GONE05/campaign/report.json": "{}",
        })
        self.assertEqual(
            self.names(bundles.named(root)),
            ["receipts.KEPT01", "session-commands.EARLY3", "session-commands.READ02"],
        )
        self.assertEqual(
            self.names(bundles.unnamed(root)),
            ["receipts.GONE04", "session-commands.GONE05"],
        )

    def test_a_record_outside_the_evidence_keeps_the_bundle_it_names(self):
        root = self.tree({
            "docs/DECISIONS/ADR-0001-a.md":
                "Its evidence is `docs/evidence/E14/session-commands.CITED1/`.\n",
            "docs/evidence/E14/session-commands.CITED1/campaign/report.json": "{}",
        })
        self.assertEqual(self.names(bundles.named(root)), ["session-commands.CITED1"])
        self.assertEqual(bundles.unnamed(root), [])

    def test_prune_removes_the_unnamed_alone_and_says_which(self):
        root = self.tree({
            "docs/evidence/E14/session-command-authority.txt":
                "campaign: docs/evidence/E14/session-commands.NEW123/campaign/report.json\n",
            "docs/evidence/E14/session-commands.NEW123/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands.OLD456/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands.OLD456/console.txt": "old\n",
        })
        gone = bundles.prune(root)
        self.assertEqual(self.names(gone), ["session-commands.OLD456"])
        self.assertFalse((root / "docs/evidence/E14/session-commands.OLD456").exists())
        self.assertTrue((root / "docs/evidence/E14/session-commands.NEW123/campaign/report.json").is_file())
        self.assertTrue((root / "docs/evidence/E14/session-command-authority.txt").is_file())

    def test_the_check_fails_on_an_unnamed_bundle_and_passes_without_one(self):
        root = self.tree({
            "docs/evidence/E14/session-command-authority.txt":
                "campaign: docs/evidence/E14/session-commands.NEW123/campaign/report.json\n",
            "docs/evidence/E14/session-commands.NEW123/campaign/report.json": "{}",
            "docs/evidence/E14/session-commands.OLD456/campaign/report.json": "{}",
        })
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(bundles, "ROOT", root), redirect_stdout(out), redirect_stderr(err):
            self.assertEqual(bundles.main(["--check"]), 1)
        self.assertIn("no record names docs/evidence/E14/session-commands.OLD456/", err.getvalue())
        with mock.patch.object(bundles, "ROOT", root), redirect_stdout(out), redirect_stderr(err):
            self.assertEqual(bundles.main(["--prune"]), 0)
            self.assertEqual(bundles.main(["--check"]), 0)
        self.assertIn("removed, no record names it: docs/evidence/E14/session-commands.OLD456/", out.getvalue())

    def test_the_repository_holds_no_unnamed_bundle(self):
        self.assertEqual(
            [str(b.relative_to(bundles.ROOT)) for b in bundles.unnamed()],
            [],
            "python3 scripts/evidence_bundles.py --prune removes them",
        )


class Fetch(unittest.TestCase):
    """`evidence_fetch.py` prunes, once it has copied a run's evidence, the
    bundles that evidence superseded, and keeps the one it names."""

    def test_a_fetch_removes_the_bundle_its_evidence_superseded(self):
        import json
        import sys
        import types
        fetch = load("evidence_fetch")
        sha = "a" * 40
        root = pathlib.Path(tempfile.mkdtemp(prefix="pw-bundles-fetch-"))
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        artifact, checkout = root / "artifact", root / "checkout"
        new_txt = (
            "produced by: just e14-sessions\n"
            f"commit: {sha}\n"
            "campaign: docs/evidence/E14/session-commands.NEW123/campaign/report.json\n"
        )
        wrote = {
            "docs/evidence/E14/session-command-authority.txt": new_txt,
            "docs/evidence/E14/session-commands.NEW123/campaign/report.json": "{}",
        }
        for rel, text in wrote.items():
            target = artifact / "evidence" / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        (artifact / "results.json").write_text(json.dumps(
            [{"recipe": "e14-sessions", "status": 0, "seconds": 5, "wrote": sorted(wrote)}]))
        for rel, text in {
            **RECIPES,
            "docs/evidence/E14/session-command-authority.txt":
                "campaign: docs/evidence/E14/session-commands.OLD456/campaign/report.json\n",
            "docs/evidence/E14/session-commands.OLD456/campaign/report.json": "{}",
        }.items():
            path = checkout / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        (checkout / "scripts").mkdir()
        run = {"headSha": sha, "url": "https://example.invalid/run/1", "status": "completed",
               "conclusion": "success", "jobs": []}

        def gh(*args):
            if args[:2] == ("run", "view"):
                return json.dumps(run)
            shutil.copytree(artifact, pathlib.Path(args[args.index("--dir") + 1]), dirs_exist_ok=True)
            return ""

        out = io.StringIO()
        with mock.patch.object(fetch, "ROOT", checkout), \
             mock.patch.object(fetch, "SECONDS", checkout / "scripts/ci_seconds.json"), \
             mock.patch.object(fetch, "gh", side_effect=gh), \
             mock.patch.object(fetch, "moved_on", return_value=set()), \
             mock.patch.object(fetch.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout=sha)), \
             mock.patch.object(sys, "argv", ["evidence_fetch.py", "1"]), redirect_stdout(out):
            self.assertEqual(fetch.main(), 0)
        self.assertTrue((checkout / "docs/evidence/E14/session-commands.NEW123/campaign/report.json").is_file())
        self.assertFalse((checkout / "docs/evidence/E14/session-commands.OLD456").exists(), out.getvalue())
        self.assertIn("removed, no record names it: docs/evidence/E14/session-commands.OLD456/", out.getvalue())


if __name__ == "__main__":
    unittest.main()
