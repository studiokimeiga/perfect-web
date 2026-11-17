"""Audit must prove a completed scan before it can accept documented advisories."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ALLOWED = "GHSA-vfj7-8cjw-p6xm"
NEW = "GHSA-2345-cfgh-jmpq"


def report(advisory=None):
    entries = {} if advisory is None else {"1": {
        "github_advisory_id": advisory, "severity": "high",
        "module_name": "fixture", "title": "Fixture advisory",
    }}
    return {"advisories": entries, "metadata": {"vulnerabilities": {
        "info": 0, "low": 0, "moderate": 0, "high": len(entries), "critical": 0,
    }, "totalDependencies": 1}}


class NodeAudit(unittest.TestCase):
    def run_audit(self, value, status=None, missing_allow=False, allow=ALLOWED):
        text = value if isinstance(value, str) else json.dumps(value)
        with tempfile.TemporaryDirectory(prefix="pw-node-audit-") as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            (root / "tools").mkdir()
            for name in ("audit-node.sh", "audit_node.py"):
                source = ROOT / "scripts" / name
                if source.exists():
                    shutil.copy(source, root / "scripts" / name)
            if not missing_allow:
                text_of_allow = f"{allow}\n  A documented fixture reason.\n" if allow else "# nothing accepted\n"
                (root / "tools/node-audit-allow.txt").write_text(text_of_allow)
            env = os.environ.copy()
            args = ["bash", str(root / "scripts/audit-node.sh")]
            if status is None:
                args.append("-")
            else:
                (root / "bin").mkdir()
                (root / "fixture.json").write_text(text)
                stub = root / "bin/pnpm"
                stub.write_text('#!/usr/bin/env bash\nprintf "%s" "$*" > "$AUDIT_ARGS"\ncat "$AUDIT_FIXTURE"\nexit "$AUDIT_STATUS"\n')
                stub.chmod(0o755)
                env.update(PATH=str(root / "bin") + os.pathsep + env["PATH"],
                           AUDIT_FIXTURE=str(root / "fixture.json"), AUDIT_STATUS=str(status),
                           AUDIT_ARGS=str(root / "args"))
            run = subprocess.run(args, input=text, text=True, capture_output=True, env=env, timeout=10)
            invoked = (root / "args").read_text() if (root / "args").exists() else ""
            return run, invoked

    def assert_refused(self, value, status=None, **kwargs):
        run, _ = self.run_audit(value, status, **kwargs)
        self.assertNotEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertNotIn("audit-node: OK", run.stdout + run.stderr)

    def test_clean_stdin_report_passes(self):
        run, _ = self.run_audit(report(), allow="")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("audit-node: OK", run.stdout)

    def test_an_allowance_no_advisory_matches_fails_and_is_named(self):
        # 2026-11-17: the baselines' patches left four allowances no audit
        # reported, and the OK line counted the list, not what it matched.
        run, _ = self.run_audit(report())
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertIn("allowances no reported advisory matches", run.stderr)
        self.assertIn(ALLOWED, run.stderr)
        self.assertNotIn("audit-node: OK", run.stdout)

    def test_documented_advisory_passes(self):
        run, _ = self.run_audit(report(ALLOWED))
        self.assertEqual(run.returncode, 0, run.stderr)

    def test_unreviewed_advisory_fails(self):
        self.assert_refused(report(NEW))

    def test_empty_response_fails(self):
        self.assert_refused("")

    def test_malformed_json_fails(self):
        self.assert_refused("{broken")

    def test_non_object_report_fails(self):
        self.assert_refused("[]")

    def test_missing_advisories_fails(self):
        value = report()
        del value["advisories"]
        self.assert_refused(value)

    def test_non_object_advisories_fails(self):
        value = report()
        value["advisories"] = []
        self.assert_refused(value)

    def test_error_response_fails_even_with_empty_advisories(self):
        value = report()
        value["error"] = {"code": "ECONNRESET", "message": "registry unavailable"}
        self.assert_refused(value)

    def test_missing_metadata_fails(self):
        self.assert_refused({"advisories": {}})

    def test_invalid_vulnerability_counts_fail(self):
        for count in (-1, True, "0", None):
            with self.subTest(count=count):
                value = report()
                value["metadata"]["vulnerabilities"]["high"] = count
                self.assert_refused(value)

    def test_missing_severity_count_fails(self):
        value = report()
        del value["metadata"]["vulnerabilities"]["critical"]
        self.assert_refused(value)

    def test_omitted_advisories_cannot_hide_reported_vulnerabilities(self):
        value = report()
        value["metadata"]["vulnerabilities"]["high"] = 1
        self.assert_refused(value)

    def test_one_allowed_advisory_cannot_hide_a_missing_subset(self):
        value = report(ALLOWED)
        value["metadata"]["vulnerabilities"]["high"] = 5
        self.assert_refused(value)

    def test_severity_distribution_must_match_even_when_totals_match(self):
        value = report(ALLOWED)
        value["metadata"]["vulnerabilities"]["high"] = 0
        value["metadata"]["vulnerabilities"]["moderate"] = 1
        self.assert_refused(value)

    def test_missing_advisory_identity_fails(self):
        value = report(ALLOWED)
        del value["advisories"]["1"]["github_advisory_id"]
        self.assert_refused(value)

    def test_advisory_shape_is_validated_before_allowance(self):
        for broken in ([], {"severity": "mystery"}, {"module_name": ""}, {"title": ""}):
            with self.subTest(broken=broken):
                value = report(ALLOWED)
                if isinstance(broken, dict):
                    value["advisories"]["1"].update(broken)
                else:
                    value["advisories"]["1"] = broken
                self.assert_refused(value)

    def test_duplicate_json_keys_fail(self):
        text = json.dumps(report())
        text = text.replace('"advisories": {}', '"advisories": {"hidden": {}}, "advisories": {}')
        self.assert_refused(text)

    def test_real_producer_clean_exit_and_exact_arguments(self):
        run, invoked = self.run_audit(report(), 0, allow="")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(invoked, "audit --json")

    def test_real_producer_advisory_exit_can_pass_documented_allowance(self):
        run, _ = self.run_audit(report(ALLOWED), 1)
        self.assertEqual(run.returncode, 0, run.stderr)

    def test_unexpected_producer_exit_fails_despite_valid_json(self):
        self.assert_refused(report(), 42)

    def test_failed_producer_cannot_pass_an_empty_advisory_report(self):
        self.assert_refused(report(), 1)

    def test_successful_producer_cannot_contradict_advisories(self):
        self.assert_refused(report(ALLOWED), 0)

    def test_missing_allowance_file_fails(self):
        self.assert_refused(report(), missing_allow=True)


if __name__ == "__main__":
    unittest.main()
