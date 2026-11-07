"""Give each command in a Rust test's program string `requires Anyone`.

The requires ADR's migration, for programs written inside Rust string
literals with `\\n` escapes (`"command f(x: Int) -> Int\\n    emits ..\\n{\\n"`),
`\\n\\` continuations, or `format!`'s `{{`. After the command's head, up to its
first escaped newline, `\\n    requires Anyone` goes in. A body opened on the
head's own line (`-> Cart {{\\n`) gets the clause between. A command whose
clauses already say `requires` or `invoked_by`, or come from a placeholder
(`{clause}`), is left as it is and listed. A head written with real newlines
is listed for a person to migrate.

Usage: python3 migrate_requires_rs.py FILE...
"""
import re
import sys

HEAD = re.compile(r"(?:(?<=\\n)|(?<![\w.]))command [A-Za-z_]\w*\(")
ESC_NL = "\\n"


def close_paren(t, i):
    depth = 0
    for k in range(i, len(t)):
        if t[k] == "(":
            depth += 1
        elif t[k] == ")":
            depth -= 1
            if depth == 0:
                return k
    return -1


def migrate(t, path):
    out, pos, changed, left = [], 0, [], []
    for m in HEAD.finditer(t):
        if m.start() < pos:
            continue
        close = close_paren(t, m.end() - 1)
        esc = t.find(ESC_NL, close)
        real = t.find("\n", close)
        line = t.count("\n", 0, m.start()) + 1
        name = t[m.start() + 8 : m.end() - 1]
        if close < 0 or esc < 0 or (0 <= real < esc):
            left.append(f"{path}:{line}: {name} (real newlines: by hand)")
            continue
        head_end = esc
        tail = t[close + 1 : head_end]
        # Where the body opens: on the head's line, or at the first escaped
        # newline followed by a brace.
        same_line = re.search(r"\s(\{\{?)\s*$", tail) and "!{" not in tail.rstrip()[-3:]
        body = [i for i in (t.find(ESC_NL + "{", head_end),) if i >= 0]
        region = "" if same_line else t[head_end : min(body) if body else head_end]
        if "requires" in region or "invoked_by" in region:
            continue
        if re.search(r"\{[a-z_]+\}", region):
            left.append(f"{path}:{line}: {name} (a placeholder clause: by hand)")
            continue
        if same_line:
            brace = re.search(r"\s(\{\{?)\s*$", tail)
            at = close + 1 + brace.start()
            out.append(t[pos:at])
            out.append(f"{ESC_NL}    requires Anyone{ESC_NL}{brace.group(1)}")
            pos = close + 1 + len(tail)
        else:
            out.append(t[pos:head_end])
            out.append(f"{ESC_NL}    requires Anyone")
            pos = head_end
        changed.append(f"{path}:{line}: {name}")
    out.append(t[pos:])
    return "".join(out), changed, left


def main(paths):
    for path in paths:
        t = open(path, encoding="utf-8").read()
        new, changed, left = migrate(t, path)
        if changed:
            open(path, "w", encoding="utf-8").write(new)
        for c in changed:
            print("changed", c)
        for entry in left:
            print("LEFT   ", entry)


if __name__ == "__main__":
    main(sys.argv[1:])
