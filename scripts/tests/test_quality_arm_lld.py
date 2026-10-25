"""Keep accelerated linking target-specific and CI fail-closed."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


def block(workflow, name):
    prefix = f"  {name}:\n"
    assert workflow.count(prefix) == 1
    return re.split(r"(?m)^  [a-z0-9_]+:\n", workflow.split(prefix, 1)[1], maxsplit=1)[0]


class ArmLinkerControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        cls.script = (ROOT / "scripts/ci_arm_lld.sh").read_text()

    def test_nonunit_only_arm_loads_lld(self):
        job = block(self.workflow, "build")
        self.assertEqual(job.count("run: bash scripts/ci_arm_lld.sh"), 1)
        self.assertIn("if: matrix.runner == 'ubuntu-24.04-arm'", job)
        self.assertIn("if: matrix.runner != 'ubuntu-24.04-arm'", job)
        self.assertIn("shared-key: arm-ci-lld18", job)

    def test_arm_native_provisions_lld_before_cache(self):
        job = block(self.workflow, "arm_native_unit")
        self.assertEqual(job.count("run: bash scripts/ci_arm_lld.sh"), 1)
        self.assertLess(job.index("run: bash scripts/ci_arm_lld.sh"),
                        job.index("uses: Swatinem/rust-cache@"))
        self.assertIn("shared-key: arm-native-tests-lld18", job)
        self.assertIn("RUSTFLAGS: '-D warnings'", job)

    def test_cargo_probe_and_target_isolation(self):
        s = self.script
        for marker in (
            'if [[ "$(uname -m)" != "aarch64" ]]',
            "'lld=1:18.0-59~exp2'",
            "Ubuntu LLD 18.1.3",
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=",
            'exec cc -fuse-ld=lld "$@"',
            "cargo build --offline --locked",
            'test -s "$RUNNER_TEMP/pleris-arm-lld-probe-used"',
            'CARGO_TARGET_DIR="$probe/native-target" cargo build --offline --locked',
            '"$probe/native-target/debug/pleris-arm-lld-probe"',
            'rm "$RUNNER_TEMP/pleris-arm-lld-probe-used"',
        ):
            self.assertIn(marker, s)
        self.assertNotIn("RUSTFLAGS=", s)
        self.assertNotIn("CARGO_TARGET_WASM32", s)

    def test_original_partition_coverage(self):
        arm = block(self.workflow, "arm_native_unit")
        self.assertIn("group: [pw-core-libdocs, pw-core-int-0", arm)
        self.assertIn("cargo test --locked -p pw-core --lib --no-fail-fast", arm)
        self.assertIn("cargo test --locked -p pw-core --doc --no-fail-fast", arm)
        self.assertIn("python3 tools/perf_core/shard.py --shard", arm)
        self.assertIn("tools/perf_core/arm_weights.json", arm)
        x64 = block(self.workflow, "x64_native_unit")
        self.assertIn("group: [pw-core-libdocs, pw-core-int-0", x64)
        self.assertIn("cargo test --locked -p pw-core --lib --no-fail-fast", x64)
        self.assertIn("cargo test --locked -p pw-core --doc --no-fail-fast", x64)
        self.assertIn("python3 tools/perf_core/shard.py --shard", x64)
        for job in (arm, x64):
            self.assertIn("cargo test --locked -p pw-dev-server --no-fail-fast", job)
            self.assertIn("cargo test --locked --workspace --exclude pw-core --exclude pw-dev-server --no-fail-fast", job)
        self.assertNotIn("ci_arm_lld", x64)
        self.assertIn("needs: [build, arm_native_unit]", block(self.workflow, "arm_required"))
        self.assertIn("needs: [build, x64_native_unit]", block(self.workflow, "x64_required"))
