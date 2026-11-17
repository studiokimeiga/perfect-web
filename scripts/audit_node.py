#!/usr/bin/env python3
"""Validate the pinned pnpm audit protocol before applying allowances (ADR-0371)."""
import argparse
import json
from pathlib import Path
import re
import sys

SEVERITIES = {"info", "low", "moderate", "high", "critical"}
GHSA = re.compile(r"GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4}\Z")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON member")
        result[key] = value
    return result


def findings_of(advisory):
    """**How many of the registry's counts an advisory is** (ADR-0371,
    amended 2026-11-17): one for each version of its package it was found at
    (`findings`), each by the one path pnpm's audit tree gives a package. A
    finding with any other number of paths is a report this validator has
    not seen, and is refused until it is read."""
    findings = advisory.get("findings")
    if not isinstance(findings, list) or not findings:
        raise ValueError("advisory has no findings")
    for finding in findings:
        version = finding.get("version") if isinstance(finding, dict) else None
        if not isinstance(version, str) or not version.strip():
            raise ValueError("finding has no version")
        paths = finding.get("paths")
        if not isinstance(paths, list) or len(paths) != 1 or not isinstance(paths[0], str) or not paths[0].strip():
            raise ValueError("finding has not exactly one path")
    return len(findings)


def validate(report, producer_exit):
    if not isinstance(report, dict):
        raise ValueError("report must be an object")
    if "error" in report:
        raise ValueError("the audit producer reported an error")
    advisories = report.get("advisories")
    if not isinstance(advisories, dict):
        raise ValueError("report has no advisory object")
    metadata = report.get("metadata")
    if not isinstance(metadata, dict):
        raise ValueError("report has no metadata object")
    counts = metadata.get("vulnerabilities")
    if not isinstance(counts, dict) or set(counts) != SEVERITIES:
        raise ValueError("report has no complete severity counts")
    if any(type(value) is not int or value < 0 for value in counts.values()):
        raise ValueError("severity counts must be nonnegative integers")
    observed = {severity: 0 for severity in SEVERITIES}
    for advisory in advisories.values():
        if not isinstance(advisory, dict):
            raise ValueError("advisory must be an object")
        identity = advisory.get("github_advisory_id")
        if not isinstance(identity, str) or not GHSA.fullmatch(identity):
            raise ValueError("advisory has no GHSA identity")
        if advisory.get("severity") not in SEVERITIES:
            raise ValueError("advisory has an unknown severity")
        for field in ("module_name", "title"):
            if not isinstance(advisory.get(field), str) or not advisory[field].strip():
                raise ValueError("advisory has no " + field)
        observed[advisory["severity"]] += findings_of(advisory)
    if observed != counts:
        raise ValueError("advisories and severity counts disagree")
    if producer_exit is not None and producer_exit != (1 if advisories else 0):
        raise ValueError("audit exit status disagrees with its report")
    return list(advisories.values())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow", type=Path, required=True)
    parser.add_argument("--producer-exit", type=int)
    args = parser.parse_args()
    try:
        report = json.load(sys.stdin, object_pairs_hook=unique_object)
        advisories = validate(report, args.producer_exit)
        allowed = {line for line in args.allow.read_text().splitlines() if GHSA.fullmatch(line)}
    except (ValueError, TypeError, OSError) as error:
        print(f"audit-node: REFUSED (no completed audit): {error}", file=sys.stderr)
        return 2
    unexpected = [advisory for advisory in advisories if advisory["github_advisory_id"] not in allowed]
    if unexpected:
        print("audit-node: advisories not on the allow list:", file=sys.stderr)
        for advisory in unexpected:
            print(f'{advisory["github_advisory_id"]}  {advisory["severity"]:8} '
                  f'{advisory["module_name"]}  {advisory["title"]}', file=sys.stderr)
        return 1
    # An allowance the audit no longer reports is gone from the tree, or its
    # advisory's record changed: it goes with it, as an advisory comes with
    # its reason (2026-11-17: #124 removed two such allowances by hand, and
    # the baselines' patches four; the count printed below was the list's,
    # so nothing said which matched).
    reported = {advisory["github_advisory_id"] for advisory in advisories}
    stale = sorted(allowed - reported)
    if stale:
        print("audit-node: allowances no reported advisory matches (remove each):", file=sys.stderr)
        for identity in stale:
            print(identity, file=sys.stderr)
        return 1
    print(f"audit-node: OK (no new advisories; {len(allowed)} accepted and documented, each reported)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
