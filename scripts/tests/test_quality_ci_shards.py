"""Fail if ARM64 CI sharding silently stops checking an original CI gate."""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def recipe_dependencies(name: str) -> list[str]:
    text = (ROOT / "justfile").read_text(encoding="utf-8")
    matches = re.findall(rf"(?m)^{re.escape(name)}: ([^\n]+)$", text)
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one {name} recipe, got {len(matches)}")
    return matches[0].split()


class CiShardingContract(unittest.TestCase):
    def test_sharded_arm_ci_omits_only_unit_tests(self) -> None:
        full = recipe_dependencies("ci")
        without_unit = recipe_dependencies("ci-without-unit")
        self.assertEqual(full.count("test-unit"), 1)
        self.assertEqual(
            [dependency for dependency in full if dependency != "test-unit"],
            without_unit,
        )
        for required in ("lint", "test-compile", "mutation-anchors", "mutant-programs"):
            self.assertIn(required, without_unit)

    def test_required_ci_aggregates_every_arm_native_shard(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("just ci-without-unit", workflow)
        self.assertIn("just ci\n", workflow)
        for command in (
            "cargo test --locked -p pw-core --no-fail-fast",
            "cargo test --locked -p pw-dev-server --no-fail-fast",
            "cargo test --locked --workspace --exclude pw-core --exclude pw-dev-server --no-fail-fast",
        ):
            self.assertIn(command, workflow)
        self.assertIn("name: ubuntu-24.04-arm\n    needs: [build, arm_native_unit]\n    if: always()", workflow)
        self.assertIn('test "$BUILD_RESULT" = success', workflow)
        self.assertIn('test "$NATIVE_TESTS_RESULT" = success', workflow)

    def test_new_arm_native_actions_are_commit_pinned(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        section = workflow.split("  arm_native_unit:", 1)[1].split("  arm_required:", 1)[0]
        refs = re.findall(r"(?m)^\s+- uses: [^@\n]+@([^\s]+)", section)
        self.assertGreaterEqual(len(refs), 6)
        for ref in refs:
            self.assertRegex(ref, r"^[0-9a-f]{40}$")


if __name__ == "__main__":
    unittest.main()
