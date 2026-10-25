#!/usr/bin/env python3
"""Opt-in pw-core test-binary sharding with Cargo-owned complete inventory."""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
WEIGHTS = ROOT / "tools/perf_core/weights.json"
SHARD_COUNT = 4


def cargo_targets(metadata, files):
    pkgs = [p for p in metadata.get("packages", []) if p.get("name") == "pw-core"]
    if len(pkgs) != 1:
        raise ValueError("cannot identify unique pw-core package")
    targets = [t["name"] for t in pkgs[0]["targets"] if "test" in t.get("kind", [])]
    if not targets or len(set(targets)) != len(targets):
        raise ValueError("missing or duplicate Cargo integration targets")
    if set(targets) != set(files):
        raise ValueError(
            f"Cargo/test file mismatch, missing Cargo targets {sorted(set(files)-set(targets))}; "
            f"unexpected Cargo targets {sorted(set(targets)-set(files))}"
        )
    return sorted(targets)


def balanced(names, weights, num=SHARD_COUNT):
    if not names or len(set(names)) != len(names) or num < 1:
        raise ValueError("invalid target list or group count")
    if any(not isinstance(x, (int,float)) or not math.isfinite(x) or x < 0
           for x in weights.values()):
        raise ValueError("invalid measured test duration")
    groups, cost = [[] for _ in range(num)], [0.0] * num
    for name in sorted(names, key=lambda n:(-weights.get(n, 3.0),n)):
        i = min(range(num), key=lambda k:(cost[k],len(groups[k]),k))
        groups[i].append(name)
        cost[i] += weights.get(name, 3.0)
    if sorted(name for group in groups for name in group) != sorted(names):
        raise AssertionError("shard allocator omitted or duplicated targets")
    return [sorted(group) for group in groups]


def read_plan(weights_file=WEIGHTS):
    r = subprocess.run(
        ["cargo","metadata","--no-deps","--locked","--format-version","1"],
        cwd=ROOT,capture_output=True,text=True,check=True,timeout=90)
    source_files = {x.stem for x in (ROOT/"compiler/pw-core/tests").glob("*.rs")}
    targets = cargo_targets(json.loads(r.stdout), source_files)
    evidence = json.loads(weights_file.read_text())
    if evidence.get("schema") != 1 or not isinstance(evidence.get("seconds"),dict):
        raise ValueError("historical test durations are invalid")
    groups = balanced(targets,evidence["seconds"])
    return groups,evidence


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--check",action="store_true")
    p.add_argument("--shard",type=int)
    p.add_argument("--out",type=Path)
    p.add_argument("--weights-file",type=Path,default=WEIGHTS)
    a = p.parse_args()
    try:
        groups,evidence = read_plan(a.weights_file)
        if a.check:
            print(f"Validated {sum(map(len,groups))} distinct Cargo integration targets")
            for i,group in enumerate(groups):
                seconds=sum(evidence["seconds"].get(x,3) for x in group)
                print(f"shard {i}: {len(group)} targets, {seconds:.1f} historical seconds")
            return 0
        if a.shard is None or a.shard not in range(len(groups)):
            raise ValueError("select a valid --shard or --check")
        selected = groups[a.shard]
        cmd = ["cargo","test","--locked","-p","pw-core","--no-fail-fast"]
        cmd += [v for name in selected for v in ("--test",name)]
        print(f"TEST SHARD {a.shard}: {len(selected)} targets {selected}",flush=True)
        begin=time.monotonic()
        exit_code=subprocess.run(cmd,cwd=ROOT,check=False).returncode
        seconds=round(time.monotonic()-begin,3)
        out={"sha":os.getenv("GITHUB_SHA"),"shard":a.shard,
             "targets":selected,"seconds":seconds,"exit_code":exit_code,
             "expected_historical_seconds":sum(evidence["seconds"].get(x,3)
                                               for x in selected)}
        if a.out:
            a.out.mkdir(parents=True,exist_ok=True)
            (a.out/f"shard-{a.shard}.json").write_text(json.dumps(out,indent=2)+"\n")
        if os.getenv("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"],"a") as f:
                f.write(f"Shard {a.shard}: {len(selected)} binaries, {seconds}s, exit={exit_code}\n")
        print(json.dumps(out),flush=True)
        return exit_code
    except (OSError,ValueError,subprocess.CalledProcessError,subprocess.TimeoutExpired) as e:
        print(f"Invalid shard: {e}",file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
