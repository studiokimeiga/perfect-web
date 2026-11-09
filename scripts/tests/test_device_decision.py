"""A page that talks to no server decides as `pw-resume.wasm` decides, in
ordinary CI.

`spikes/own-renderer/device-decision.test.mjs` asks both the same questions:
the real module, built here for the browser, and the runtime's own lines.
"""
import os
import pathlib
import shutil
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class DeviceDecision(unittest.TestCase):
    def test_the_two_decisions_give_one_answer(self) -> None:
        node = shutil.which("node")
        self.assertIsNotNone(node, "the decisions' agreement requires Node")
        built = subprocess.run(
            [
                "cargo", "build", "--quiet", "--locked", "-p", "pw-resume-wasm",
                "--target", "wasm32-unknown-unknown", "--profile", "browser",
            ],
            cwd=ROOT, capture_output=True, text=True, timeout=1200,
        )
        self.assertEqual(built.returncode, 0, built.stderr)
        result = subprocess.run(
            [node, "--test", "--test-reporter=spec", "spikes/own-renderer/device-decision.test.mjs"],
            cwd=ROOT, capture_output=True, text=True, timeout=600, env=os.environ,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertRegex(result.stdout, r"(?m)^ℹ fail 0$")
        for label, count in (("cancelled", 0), ("skipped", 0), ("todo", 0)):
            self.assertRegex(result.stdout, rf"(?m)^ℹ {label} {count}$")


if __name__ == "__main__":
    unittest.main()
