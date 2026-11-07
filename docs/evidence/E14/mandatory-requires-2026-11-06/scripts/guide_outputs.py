"""Rewrite each `text pw-check` block of the guide from the built `pw`.

For each ```pw block followed by a ```text pw-check block, the program is
written as the guide's test writes it (`<module's last segment>.pw`, with the
standard packages beside it) and checked with `pw check --plain`; the block
becomes what it printed, minus its closing count line, as the test compares.

Usage, from the repository root: python3 guide_outputs.py PW_BINARY PAGE...
Prints each block it changed.
"""
import pathlib
import re
import subprocess
import sys
import tempfile

pw = pathlib.Path(sys.argv[1]).resolve()
root = pathlib.Path.cwd()
STD = sorted(str(p) for d in ["packages/pw-std", "packages/pw-platform-web"] for p in (root / d).glob("*.pw"))


def file_name_of(src):
    m = re.search(r"^\s*module\s+(\S+)", src, re.M)
    return f"{m.group(1).rsplit('.', 1)[-1]}.pw" if m else "example.pw"


def printed(src):
    with tempfile.TemporaryDirectory() as d:
        name = file_name_of(src)
        (pathlib.Path(d) / name).write_text(src)
        r = subprocess.run([str(pw), "check", "--plain", *STD, name], cwd=d, capture_output=True, text=True)
    lines = (r.stdout + r.stderr).rstrip("\n").split("\n")
    if lines and lines[-1].startswith("pw check:"):
        lines = lines[:-1]
    return "\n".join(lines).rstrip("\n") + "\n"


for page in sys.argv[2:]:
    text = open(page, encoding="utf-8").read()
    pattern = re.compile(r"(```pw[^\n]*\n)(.*?)(```\n\n```text pw-check\n)(.*?)(```)", re.S)
    changed = []

    def redo(m):
        new = printed(m.group(2))
        if new != m.group(4):
            changed.append(file_name_of(m.group(2)))
        return m.group(1) + m.group(2) + m.group(3) + new + m.group(5)

    text = pattern.sub(redo, text)
    if changed:
        open(page, "w", encoding="utf-8").write(text)
    for c in changed:
        print(f"{page}: {c}")
