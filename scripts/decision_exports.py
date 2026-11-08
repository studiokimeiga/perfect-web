#!/usr/bin/env python3
"""**Every export a page's script asks of the decision's Wasm, the Wasm has.**

ADR-0400 took the refusal's trace out of the browser's decision:
`pw-resume-wasm` no longer exports `last_trace_ptr` or `last_trace_len`. The
Marko spike's runtime still called both on every refusal, so a rebuilt
decision threw a `TypeError` there, and no CI job runs that spike's browser
suite (train-14's review, 2026-11-07). A script asks the decision for an
export in one of two ways, and this reads both:

- a member of the instance's exports, beside its `decide_manifest`:
  `w.decide_manifest(...)`, then `w.last_recovery()`;
- a name destructured with `decide_manifest`:
  `const { memory, alloc, decide_manifest } = instance.exports`.

The decision's exports are the `#[unsafe(no_mangle)]` functions of
`runtime/pw-resume-wasm/src/lib.rs`, and its `memory`.

Prints each export asked for and missing, `path: name`, and exits 1 if one
is.

    python3 scripts/decision_exports.py
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WASM_SOURCE = ROOT / "runtime/pw-resume-wasm/src/lib.rs"
EXPORTED = re.compile(r'#\[(?:unsafe\()?no_mangle\)?\]\s*pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(\w+)')
DESTRUCTURED = re.compile(r"\b(?:const|let|var)\s*\{([^{}]*)\}\s*=")
MEMBER_CALL = re.compile(r"\b([A-Za-z_$][\w$]*)\s*\.\s*decide_manifest\s*\(")


def exports(source: str) -> set[str]:
    """The decision's exports: its exported functions, and its memory."""
    return set(EXPORTED.findall(source)) | {"memory"}


def asked(script: str) -> set[str]:
    """Each export `script` asks of the decision's Wasm."""
    names: set[str] = set()
    for m in DESTRUCTURED.finditer(script):
        keys = [part.split(":")[0].split("=")[0].strip() for part in m.group(1).split(",")]
        if "decide_manifest" in keys:
            names.update(k for k in keys if k and not k.startswith("..."))
    for var in set(MEMBER_CALL.findall(script)):
        names.update(re.findall(rf"(?<![\w$.]){re.escape(var)}\s*\.\s*([A-Za-z_$][\w$]*)", script))
    return names


def scripts(root: pathlib.Path = ROOT) -> list[str]:
    """Every tracked script that names the decision's `decide_manifest`."""
    out = subprocess.run(["git", "ls-files", "-z", "*.mjs", "*.js"], cwd=root, capture_output=True, check=True)
    paths = [p for p in out.stdout.decode().split("\0") if p and "node_modules/" not in p]
    return sorted(p for p in paths if "decide_manifest" in (root / p).read_text(encoding="utf-8", errors="replace"))


def missing(root: pathlib.Path = ROOT) -> tuple[list[tuple[str, str]], dict[str, set[str]]]:
    """Each (script, export) asked for and not exported, and what each
    script asks for."""
    have = exports((root / WASM_SOURCE.relative_to(ROOT)).read_text(encoding="utf-8"))
    each = {p: asked((root / p).read_text(encoding="utf-8")) for p in scripts(root)}
    return [(p, n) for p, names in each.items() for n in sorted(names - have)], each


def main() -> int:
    gone, each = missing()
    for path, name in gone:
        print(f"{path}: {name}")
    print(f"decision exports asked for: {sum(len(v) for v in each.values())} in "
          f"{sum(1 for v in each.values() if v)} script(s); missing: {len(gone)}")
    return 1 if gone else 0


if __name__ == "__main__":
    sys.exit(main())
