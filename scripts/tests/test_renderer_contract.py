"""Run the browser runtime's lifecycle seam regressions in ordinary CI."""
import pathlib
import shutil
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class RendererContract(unittest.TestCase):
    def test_renderer_lifecycle(self) -> None:
        node = shutil.which("node")
        self.assertIsNotNone(node, "renderer lifecycle tests require Node")
        result = subprocess.run(
            [node, "--test", "--test-reporter=spec", "spikes/own-renderer/renderer-contract.test.mjs"],
            cwd=ROOT, capture_output=True, text=True, timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertRegex(result.stdout, r"(?m)^ℹ pass 13$")
        self.assertRegex(result.stdout, r"(?m)^ℹ fail 0$")
        for label, count in (("tests", 13), ("cancelled", 0), ("skipped", 0), ("todo", 0)):
            self.assertRegex(result.stdout, rf"(?m)^ℹ {label} {count}$")


if __name__ == "__main__":
    unittest.main()
