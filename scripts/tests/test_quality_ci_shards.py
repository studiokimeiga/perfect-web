"""Keep architecture-sharded CI exhaustive, fail-closed, and immutable."""

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
    def setUp(self) -> None:
        self.workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")

    def test_sharded_ci_omits_only_native_tests(self) -> None:
        full = recipe_dependencies("ci")
        without_unit = recipe_dependencies("ci-without-unit")
        self.assertEqual(full.count("test-unit"), 1)
        self.assertEqual(
            [dependency for dependency in full if dependency != "test-unit"],
            without_unit,
        )
        for required in ("lint", "test-compile", "mutation-anchors", "mutant-programs"):
            self.assertIn(required, without_unit)
        self.assertEqual(self.workflow.count("just ci-without-unit\n"), 1)

    def test_both_required_checks_fail_closed(self) -> None:
        for arch, native in (("ubuntu-24.04", "x64_native_unit"),
                             ("ubuntu-24.04-arm", "arm_native_unit")):
            self.assertEqual(self.workflow.count(f"    name: {arch}\n"), 1)
            self.assertIn(
                f"name: {arch}\n    needs: [build, {native}]\n    if: always()",
                self.workflow,
            )
        # A matrix failure propagates to its needs result. All partitions
        # must finish successfully; never convert cancelled/skipped into pass.
        self.assertEqual(self.workflow.count('test "$BUILD_RESULT" = success'), 2)
        self.assertEqual(
            self.workflow.count('test "$NATIVE_TESTS_RESULT" = success'), 2,
        )
        self.assertIn("permissions:\n  contents: read", self.workflow)

    def test_all_native_partitions_are_exhaustive(self) -> None:
        common = (
            "cargo test --locked -p pw-dev-server --no-fail-fast",
            "cargo test --locked --workspace --exclude pw-core --exclude pw-dev-server --no-fail-fast",
        )
        arm = self.workflow.split("  arm_native_unit:", 1)[1].split("  x64_native_unit:", 1)[0]
        self.assertIn(
            "group: [pw-core-libdocs, pw-core-int-0, pw-core-int-1, "
            "pw-core-int-2, pw-core-int-3, pw-dev-server, rest]", arm,
        )
        self.assertIn("cargo test --locked -p pw-core --lib --no-fail-fast", arm)
        self.assertIn("cargo test --locked -p pw-core --doc --no-fail-fast", arm)
        self.assertIn("pw-core-int-0|pw-core-int-1|pw-core-int-2|pw-core-int-3)", arm)
        self.assertIn('python3 tools/perf_core/shard.py --shard "$shard" --weights-file tools/perf_core/arm_weights.json', arm)
        self.assertIn("shared-key: arm-native-tests-lld18", arm)
        self.assertIn("matrix.group == 'pw-core-int-0'", arm)

        x64 = self.workflow.split("  x64_native_unit:", 1)[1].split("  x64_required:", 1)[0]
        self.assertIn(
            "group: [pw-core-libdocs, pw-core-int-0, pw-core-int-1, "
            "pw-core-int-2, pw-core-int-3, pw-dev-server, rest]", x64,
        )
        self.assertIn("cargo test --locked -p pw-core --lib --no-fail-fast", x64)
        self.assertIn("cargo test --locked -p pw-core --doc --no-fail-fast", x64)
        self.assertIn("pw-core-int-0|pw-core-int-1|pw-core-int-2|pw-core-int-3)", x64)
        self.assertIn('python3 tools/perf_core/shard.py --shard "$shard"', x64)
        self.assertIn("shared-key: x64-native-tests", x64)
        self.assertIn("matrix.group == 'pw-core-int-0'", x64)

        for job in (arm, x64):
            for command in common:
                self.assertIn(command, job)

        self.assertIn("tools/perf_core/shard.py --check", self.workflow)
        self.assertIn("tools/perf_core/shard.py --check --weights-file tools/perf_core/arm_weights.json", self.workflow)
        self.assertIn("unittest discover -s tools/perf_core", self.workflow)

    def test_new_native_actions_are_immutable(self) -> None:
        for native in ("arm_native_unit", "x64_native_unit"):
            tail = self.workflow.split(f"  {native}:", 1)[1]
            block = re.split(r"(?m)^  [a-z_]+:", tail, maxsplit=1)[0]
            refs = re.findall(r"(?m)^\s+- uses: [^@\n]+@([^\s]+)", block)
            self.assertGreaterEqual(len(refs), 6)
            for ref in refs:
                self.assertRegex(ref, r"^[0-9a-f]{40}$")


if __name__ == "__main__":
    unittest.main()
