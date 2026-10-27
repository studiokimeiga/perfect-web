"""Run verify's Recipes shell block at the recipe-process boundary, without a DB."""

import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ADAPTER_URLS = ("PW_FEED_DATABASE_URL", "PW_STORE_DATABASE_URL")


def recipes_run() -> str:
    workflow = (ROOT / ".github/workflows/verify.yml").read_text(encoding="utf-8")
    steps = re.findall(
        r"(?ms)^      - name: Recipes\n(.*?)(?=^      - |\Z)", workflow,
    )
    if len(steps) != 1 or steps[0].count("        run: |\n") != 1:
        raise AssertionError("expected one Recipes step with a literal shell run block")
    return textwrap.dedent(steps[0].split("        run: |\n", 1)[1])


def clean_environment() -> dict[str, str]:
    env = dict(os.environ)
    for name in (*ADAPTER_URLS, "DATABASE", "RECIPES", "BASH_ENV", "ENV"):
        env.pop(name, None)
    return env


class VerifyRecipeDatabaseEnvironment(unittest.TestCase):
    def observe_recipe_process(self, database: str) -> dict:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            binaries = directory / "bin"
            binaries.mkdir()
            # Only the expensive consumer is replaced: the real workflow shell
            # must export its environment across an actual process boundary.
            probe = binaries / "python3"
            probe.write_text(
                f"#!{sys.executable}\n"
                "import json, os, pathlib, sys\n"
                "pathlib.Path('observed.json').write_text(json.dumps({\n"
                "    'argv': sys.argv[1:],\n"
                "    'feed': os.environ.get('PW_FEED_DATABASE_URL'),\n"
                "    'store': os.environ.get('PW_STORE_DATABASE_URL'),\n"
                "}))\n"
                "pathlib.Path('ci-out').mkdir()\n",
                encoding="utf-8",
            )
            probe.chmod(0o755)
            # The disk-free setup wait is unrelated to URL propagation. Keep
            # the real run block intact but make any such wait immediate.
            sleep = binaries / "sleep"
            sleep.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            sleep.chmod(0o755)
            env = clean_environment()
            env.update({
                "PATH": str(binaries) + os.pathsep + env.get("PATH", ""),
                "DATABASE": database,
                "RECIPES": "e14-session-command-authority e14-store-postgres",
            })
            result = subprocess.run(
                ["bash", "-e", "-c", recipes_run()], cwd=directory, env=env,
                capture_output=True, text=True, timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            observed = json.loads((directory / "observed.json").read_text())
            self.assertEqual(observed["argv"], [
                "scripts/ci_recipes.py", "--out", "ci-out",
                "e14-session-command-authority", "e14-store-postgres",
            ])
            self.assertTrue((directory / "ci-out/runner.txt").is_file())
            return observed

    def test_a_database_shard_exports_both_adapter_urls_to_its_recipes(self) -> None:
        database = "postgresql://postgres:ci%24pw@localhost:5432/pw_feed_test?application_name=recipe+env"
        observed = self.observe_recipe_process(database)
        self.assertEqual(observed["feed"], database)
        self.assertEqual(observed["store"], database)

    def test_a_shard_without_a_database_leaves_both_adapter_urls_unset(self) -> None:
        observed = self.observe_recipe_process("")
        self.assertIsNone(observed["feed"])
        self.assertIsNone(observed["store"])

    def test_the_command_gate_still_refuses_either_missing_adapter_url(self) -> None:
        recipe = (ROOT / "just/session-command-authority.just").read_text(encoding="utf-8")
        body = recipe.split("\ne14-session-command-authority:\n", 1)[1]
        first_command = body.splitlines()[0].strip().removeprefix("@")
        self.assertTrue(first_command.startswith("test "))
        for feed, store, wanted in (("", "", 1), ("feed", "", 1),
                                    ("", "store", 1), ("feed", "store", 0)):
            with self.subTest(feed=feed, store=store):
                env = clean_environment()
                if feed:
                    env["PW_FEED_DATABASE_URL"] = feed
                if store:
                    env["PW_STORE_DATABASE_URL"] = store
                result = subprocess.run(
                    ["bash", "-e", "-c", first_command], env=env,
                    capture_output=True, text=True, timeout=5,
                )
                self.assertEqual(result.returncode, wanted, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
