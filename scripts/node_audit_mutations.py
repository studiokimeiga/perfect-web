#!/usr/bin/env python3
"""Node audit controls require exact named body assertions (ADR-0371)."""
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))
import guide_python

AUDIT = ROOT / "scripts/audit_node.py"
SHELL = ROOT / "scripts/audit-node.sh"
MUTANTS = [
    ('non-object report passes', 'NodeAudit.test_non_object_report_fails', AUDIT,
     '        raise ValueError("report must be an object")\n', '        return []\n'),
    ('producer error object passes', 'NodeAudit.test_error_response_fails_even_with_empty_advisories', AUDIT,
     '        raise ValueError("the audit producer reported an error")\n', '        return []\n'),
    ('missing advisory object passes', 'NodeAudit.test_missing_advisories_fails', AUDIT,
     '        raise ValueError("report has no advisory object")\n', '        return []\n'),
    ('missing metadata passes', 'NodeAudit.test_missing_metadata_fails', AUDIT,
     '        raise ValueError("report has no metadata object")\n', '        return []\n'),
    ('incomplete severity counts pass', 'NodeAudit.test_missing_severity_count_fails', AUDIT,
     '        raise ValueError("report has no complete severity counts")\n', '        return []\n'),
    ('invalid count values pass', 'NodeAudit.test_invalid_vulnerability_counts_fail', AUDIT,
     '        raise ValueError("severity counts must be nonnegative integers")\n', '        return []\n'),
    ('omitted advisories pass', 'NodeAudit.test_omitted_advisories_cannot_hide_reported_vulnerabilities', AUDIT,
     '        raise ValueError("advisories and severity counts disagree")\n', '        return []\n'),
    ('non-object advisory passes', 'NodeAudit.test_advisory_shape_is_validated_before_allowance', AUDIT,
     '            raise ValueError("advisory must be an object")\n', '            return []\n'),
    ('advisory identity can be absent', 'NodeAudit.test_missing_advisory_identity_fails', AUDIT,
     '            raise ValueError("advisory has no GHSA identity")\n', '            return []\n'),
    ('allowed advisory skips severity validation', 'NodeAudit.test_advisory_shape_is_validated_before_allowance', AUDIT,
     '            raise ValueError("advisory has an unknown severity")\n', '            return []\n'),
    ('producer status mismatch passes', 'NodeAudit.test_unexpected_producer_exit_fails_despite_valid_json', AUDIT,
     '        raise ValueError("audit exit status disagrees with its report")\n', '        return []\n'),
    ('duplicate keys hide earlier members', 'NodeAudit.test_duplicate_json_keys_fail', AUDIT,
     '        if key in result:\n', '        if False and key in result:\n'),
    ('invalid report earns a successful exit', 'NodeAudit.test_malformed_json_fails', AUDIT,
     '        return 2\n', '        return 0\n'),
    ('unknown advisories are silently accepted', 'NodeAudit.test_unreviewed_advisory_fails', AUDIT,
     '    if unexpected:\n', '    if False and unexpected:\n'),
    ('missing advisory descriptive fields pass', 'NodeAudit.test_advisory_shape_is_validated_before_allowance', AUDIT,
     '                raise ValueError("advisory has no " + field)\n', '                return []\n'),
    ('shell loses the actual producer exit', 'NodeAudit.test_unexpected_producer_exit_fails_despite_valid_json', SHELL,
     ' --producer-exit "$status"\n', '\n'),
    ('nonempty incomplete reports pass their count check', 'NodeAudit.test_one_allowed_advisory_cannot_hide_a_missing_subset', AUDIT,
     '    if observed != counts:\n', '    if bool(advisories) != any(counts.values()):\n'),
    ('total-only counts hide severity redistribution', 'NodeAudit.test_severity_distribution_must_match_even_when_totals_match', AUDIT,
     '    if observed != counts:\n', '    if sum(observed.values()) != sum(counts.values()):\n'),
    # 2026-11-17: an allowance no reported advisory matches is refused
    # (ADR-0371, amended), so each goes with its advisory.
    ('an allowance no advisory matches passes', 'NodeAudit.test_an_allowance_no_advisory_matches_fails_and_is_named', AUDIT,
     '    if stale:\n', '    if False and stale:\n'),
]
SELECTOR = "test_node_audit"
SEARCH = ROOT / "scripts/tests"
PATTERN = "test_node_audit.py"
EXPECTED_COUNT = 25
TIMEOUT = 120


def main():
    import mutation_baseline
    watch = mutation_baseline.mutation_bound.start()
    evidence = Path(tempfile.mkdtemp(prefix="pw-node-audit-controls-"))
    print(f"structured instrument receipts: {evidence}", flush=True)
    try:
        expected = guide_python.freeze(SELECTOR, SEARCH, PATTERN, EXPECTED_COUNT, evidence)
    except guide_python.DiscoveryRefused as error:
        print(f"REFUSED: {error}")
        return 1
    witnesses = {"test_node_audit." + mutant[1] for mutant in MUTANTS}
    if not witnesses <= set(expected):
        print("REFUSED: intended full IDs are not in the frozen inventory")
        return 1
    baseline = guide_python.run(SELECTOR, expected, SEARCH, evidence, TIMEOUT)
    result = baseline.result
    print(f"baseline: {result.executed} executed; {len(result.failed)} failed; valid={result.valid}", flush=True)
    if not result.valid or result.failed or result.executed != EXPECTED_COUNT or watch.stopped:
        print(baseline.output)
        mutation_baseline.explain()
        return 1
    killed = refused = survived = 0
    originals = {path: path.read_bytes() for *_, path, anchor, replacement in MUTANTS}
    try:
        for what, witness, path, anchor, replacement in MUTANTS:
            original = originals[path]
            assert path.read_bytes() == original, "source changed between owned controls"
            text = original.decode()
            if text.count(anchor) != 1:
                print(f"REFUSED {what}: anchor did not match exactly once", flush=True)
                refused += 1
                continue
            try:
                path.write_bytes(text.replace(anchor, replacement, 1).encode())
                run = guide_python.run(SELECTOR, expected, SEARCH, evidence, TIMEOUT)
                result = run.result
                intended = "test_node_audit." + witness
                if not result.valid or watch.stopped or result.body_errors:
                    verdict = "REFUSED (invalid, stopped or erroneous instrument producer; no kill)"
                    refused += 1
                    print(run.output)
                elif intended in result.assertion_failed:
                    verdict = f"KILLED by {intended} ({len(result.assertion_failed)} actual body assertions)"
                    killed += 1
                else:
                    verdict = "SURVIVED (no intended body assertion)"
                    survived += 1
                    print(run.output)
            finally:
                path.write_bytes(original)
            print(f"{what}: {verdict}", flush=True)
    finally:
        for path, original in originals.items():
            path.write_bytes(original)
        assert all(path.read_bytes() == original for path, original in originals.items())
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survived} survived", flush=True)
    return int(bool(refused or survived or watch.stopped))


if __name__ == "__main__":
    sys.exit(main())
