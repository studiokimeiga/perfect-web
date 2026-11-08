"""List each Pleris command written inside a Rust source (a test's program,
in a raw string or with escaped newlines) whose clauses say neither
`requires` nor `invoked_by`: what PW0367 refuses. Prints FILE:LINE NAME.

Usage: python3 find_unstated.py ROOT [EXCLUDE_PREFIX...]
"""
import os
import re
import sys

HEAD = re.compile(r"(?:(?<=\\n)|(?<![\w.\"]))command ([A-Za-z_]\w*)\(")


def scan(text):
    """Each (offset, name, region) of a command head and its clauses, up to
    the body's opening brace (a `{` opening a line, real or escaped, or ending
    the head's line)."""
    for m in HEAD.finditer(text):
        # Its parameters close where the parentheses balance.
        depth, k = 0, m.end() - 1
        while k < len(text):
            c = text[k]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        rest = text[k + 1 : k + 1 + 2000]
        # The body: a brace at a line's start, after a real or an escaped
        # newline, or `{`/`{{` ending the head's own line.
        ends = [i for i in (
            rest.find("\n{"),
            rest.find("\\n{"),
            rest.find("\\n\\\n{"),
        ) if i >= 0]
        same = re.match(r"[^\n]*?\s(\{\{?)\s*(\\n|\n|$)", rest)
        if same and "!{" not in rest[: same.end()].rstrip()[-4:]:
            region = ""
        else:
            region = rest[: min(ends)] if ends else rest[:400]
        yield m.start(), m.group(1), region


def main(root, excludes):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ("target", "node_modules", ".git")]
        for f in filenames:
            if not f.endswith(".rs"):
                continue
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, root)
            if any(rel.startswith(e) for e in excludes):
                continue
            text = open(path, encoding="utf-8").read()
            if "command " not in text:
                continue
            for at, name, region in scan(text):
                if "requires" in region or "invoked_by" in region:
                    continue
                line = text.count("\n", 0, at) + 1
                print(f"{rel}:{line} {name}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
