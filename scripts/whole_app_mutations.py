#!/usr/bin/env python3
"""Mutation controls for ADR-0429: `pw build --dist DIR` builds a whole app.

Each mutant undoes one piece of what the command does that an app's build
script did by hand: the standard packages added, a directory read for its
`.pw` files, a source named twice built once, the runtime and each asset
written beside the artifacts, the record of the sources and their digests,
a program in error building nothing, a build before replaced whole and
nothing else replaced, and what only a whole app's build takes refused
elsewhere. The tests in pw-cli's `whole_app.rs` must then fail.

Run from the repository root; `just e14-whole-app` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CLI = ROOT / "compiler/pw-cli/src/main.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    ("the standard packages are not added", CLI,
     '    for package in ["pw-std", "pw-platform-web"] {\n        pw_files_under(&packages.join(package), &mut sources)\n',
     '    for package in Vec::<&str>::new() {\n        pw_files_under(&packages.join(package), &mut sources)\n'),
    ("a source named twice is built twice", CLI,
     "        .filter(|p| seen.insert(p.clone()))\n",
     "        .filter(|p| {\n            seen.insert(p.clone());\n            true\n        })\n"),
    ("a directory is read as a file", CLI,
     "        if path.is_dir() {\n            let before = given.len();\n",
     "        if false {\n            let before = given.len();\n"),
    ("an app of no sources is built", CLI,
     "    if paths.is_empty() {\n        return Err(\"give the app's sources: its .pw files, or a directory of them\".to_string());\n    }\n",
     ""),
    ("the runtime is not written", CLI,
     '    if let Err(e) = std::fs::copy(\n        spike.join("public/pw-runtime.mjs"),\n        dir.join("pw-runtime.mjs"),\n    ) {\n',
     '    if let Err(e) = Ok::<(), std::io::Error>(()) {\n'),
    ("an asset is not written", CLI,
     "        if let Err(e) = std::fs::copy(file, dir.join(name)) {\n",
     "        if let Err(e) = Ok::<(), std::io::Error>(()) {\n            let _ = (file, name);\n"),
    ("an asset's name may leave the build", CLI,
     "        let plain = !name.is_empty()\n            && name != \".\"\n            && name != \"..\"\n            && !name.contains(['/', '\\\\'])\n",
     "        let plain = !name.is_empty()\n"),
    ("a directory that is no build is replaced", CLI,
     "        if !empty && !built {\n", "        if false {\n"),
    ("a build before is not replaced", CLI,
     "        if let Err(e) = std::fs::remove_dir_all(dir) {\n",
     "        if let Err(e) = Ok::<(), std::io::Error>(()) {\n"),
    ("a source's digest is not its own", CLI,
     "        let sha256: String = sha2::Sha256::digest(&bytes)\n",
     "        let sha256: String = sha2::Sha256::digest(b\"\")\n"),
    ("the record names no source", CLI,
     '        files.push(serde_json::json!({ "path": source.as_str(), "sha256": sha256 }));\n',
     '        if false {\n            files.push(serde_json::json!({ "path": source.as_str(), "sha256": sha256 }));\n        }\n'),
    ("a program in error is built", CLI,
     "    if let Some(dist) = &dist {\n        if errors > 0 {\n",
     "    if let Some(dist) = &dist {\n        if false {\n"),
    ("what builds a whole app is taken without one", CLI,
     "    if cmd == \"build\" && dist.is_none() && (production || !assets.is_empty()) {\n",
     "    if false {\n"),
    ("a whole app and its artifacts alone are asked together", CLI,
     "    if cmd == \"build\" && dist.is_some() && flag_value(&args, \"--out\").is_some() {\n",
     "    if false {\n"),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-cli", "--test", "whole_app"],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if m is None:
            built = False
            continue
        passed += int(m.group(1))
        failed += int(m.group(2))
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed")
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run_tests()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
