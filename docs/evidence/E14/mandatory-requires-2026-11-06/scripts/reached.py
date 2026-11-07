"""Which mutation scripts the change BASE..HEAD reaches: a script one of
whose anchors lies within (or within CONTEXT lines of) the lines the change
added or replaced in that anchor's file. Run from the repository root:
python3 reached.py BASE [CONTEXT]."""
import importlib.util
import pathlib
import re
import subprocess
import sys

base = sys.argv[1]
context = int(sys.argv[2]) if len(sys.argv) > 2 else 3
root = pathlib.Path.cwd()
sys.path.insert(0, str(root / "scripts"))

changed = {}
diff = subprocess.run(["git", "diff", "-U0", base, "HEAD"], capture_output=True, text=True, check=True).stdout
path = None
for line in diff.splitlines():
    if line.startswith("+++ "):
        path = line[6:] if line.startswith("+++ b/") else None
    m = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
    if m and path:
        start, count = int(m.group(1)), int(m.group(2) or 1)
        changed.setdefault(path, []).append((start - context, start + max(count, 1) + context))

reached = {}
for script in sorted((root / "scripts").glob("*_mutations.py")):
    spec = importlib.util.spec_from_file_location(script.stem, script)
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
    except Exception as e:  # a script that cannot load is reported, not hidden
        print(f"{script.name}: cannot load: {e}")
        continue
    for mutant in getattr(module, "MUTANTS", []):
        target, anchor = None, None
        for i, part in enumerate(mutant):
            if isinstance(part, pathlib.Path):
                target, anchor = part, mutant[i + 1]
                break
        if target is None or not isinstance(anchor, str):
            continue
        rel = str(pathlib.Path(target).resolve().relative_to(root))
        if rel not in changed:
            continue
        text = pathlib.Path(target).read_text()
        at = text.find(anchor)
        if at < 0:
            continue
        first = text.count("\n", 0, at) + 1
        last = first + anchor.count("\n")
        if any(a <= last and first <= b for a, b in changed[rel]):
            reached.setdefault(script.name, []).append(mutant[0])
for name, mutants in reached.items():
    print(f"{name}: {len(mutants)} anchor(s) in changed code: " + "; ".join(mutants[:4]))
