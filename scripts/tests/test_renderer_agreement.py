"""The browser's renderer and the server's held to one answer, in ordinary CI.

`spikes/own-renderer/renderer-agreement.test.mjs` asks both the same requests:
the server's renderer through the `oracle` binary, built here.
"""
import os
import pathlib
import shutil
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class RendererAgreement(unittest.TestCase):
    def test_the_two_renderers_give_one_answer(self) -> None:
        node = shutil.which("node")
        self.assertIsNotNone(node, "the renderers' agreement requires Node")
        built = subprocess.run(
            ["cargo", "build", "--quiet", "--locked", "-p", "pw-render-oracle", "--bin", "oracle"],
            cwd=ROOT, capture_output=True, text=True, timeout=1200,
        )
        self.assertEqual(built.returncode, 0, built.stderr)
        result = subprocess.run(
            [node, "--test", "--test-reporter=spec", "spikes/own-renderer/renderer-agreement.test.mjs"],
            cwd=ROOT, capture_output=True, text=True, timeout=600, env=os.environ,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertRegex(result.stdout, r"(?m)^ℹ fail 0$")
        for label, count in (("cancelled", 0), ("skipped", 0), ("todo", 0)):
            self.assertRegex(result.stdout, rf"(?m)^ℹ {label} {count}$")


if __name__ == "__main__":
    unittest.main()
