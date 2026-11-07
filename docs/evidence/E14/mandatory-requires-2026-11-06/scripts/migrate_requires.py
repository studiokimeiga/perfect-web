"""Give each command that says nothing of who may run it `requires Anyone`.

The requires ADR's migration: a command with neither `requires` nor
`invoked_by` was anyone's to run, so `requires Anyone` keeps what it does and
says so. The clause goes first among the command's clauses, aligned as
`pw fmt` aligns them (each value one space past the longest clause name).

Usage, from the repository root: python3 migrate_requires.py FILE...
Prints each command it changed as `file:line: name`.
"""
import re
import sys

COMMAND = re.compile(r"^(?P<indent>[ ]*)(?:public |private )?command\s+(?P<name>\w+)\s*\(")
CLAUSE = re.compile(r"^(?P<indent>[ ]+)(?P<key>[a-z_]+)(?P<gap>\s+)(?P<value>\S.*)$")


def migrate(lines):
    """The lines with each unstated command given `requires Anyone`, and the
    (line number, name) of each."""
    out, changed, i = [], [], 0
    while i < len(lines):
        m = COMMAND.match(lines[i])
        if not m:
            out.append(lines[i])
            i += 1
            continue
        start = i
        # The signature runs to the line that opens the body, `{`.
        j = i
        while j < len(lines) and not lines[j].rstrip().endswith("{"):
            j += 1
        if j == len(lines):
            out.append(lines[i])
            i += 1
            continue
        block = lines[start : j + 1]
        clauses = [k for k, text in enumerate(block) if k > 0 and CLAUSE.match(text) and not text.strip().startswith("->")]
        keys = [CLAUSE.match(block[k]).group("key") for k in clauses]
        if "requires" in keys or "invoked_by" in keys:
            out.extend(block)
            i = j + 1
            continue
        changed.append((start + 1, m.group("name")))
        clause_indent = m.group("indent") + "    "
        if clauses:
            width = max(len(k) for k in keys + ["requires"])
            first = clauses[0]
            new = [CLAUSE.match(block[k]) for k in clauses]
            for k, c in zip(clauses, new):
                block[k] = f"{c.group('indent')}{c.group('key').ljust(width)} {c.group('value')}"
            block.insert(first, f"{clause_indent}{'requires'.ljust(width)} Anyone")
        elif block[-1].strip() == "{":
            # The body's `{` on a line of its own: the clause goes before it.
            block.insert(len(block) - 1, f"{clause_indent}requires Anyone")
        else:
            # `command f(..) -> T {` on one line: the clause goes between the
            # signature and a `{` of its own.
            head = block[-1].rstrip()
            assert head.endswith("{"), head
            block[-1] = head[:-1].rstrip()
            block.append(f"{clause_indent}requires Anyone")
            block.append(f"{m.group('indent')}{{")
        out.extend(block)
        i = j + 1
    return out, changed


def main(paths):
    for path in paths:
        text = open(path, encoding="utf-8").read()
        lines = text.split("\n")
        if path.endswith(".md"):
            new, changed = migrate_markdown(lines)
        else:
            new, changed = migrate(lines)
        if changed:
            open(path, "w", encoding="utf-8").write("\n".join(new))
            for line, name in changed:
                print(f"{path}:{line}: {name}")


def migrate_markdown(lines):
    """Each ```pw block of a page, migrated as a file is."""
    out, changed, i = [], [], 0
    while i < len(lines):
        if lines[i].startswith("```pw"):
            j = i + 1
            while lines[j] != "```":
                j += 1
            new, c = migrate(lines[i + 1 : j])
            changed += [(i + 1 + n, name) for n, name in c]
            out += [lines[i]] + new + ["```"]
            i = j + 1
        else:
            out.append(lines[i])
            i += 1
    return out, changed


if __name__ == "__main__":
    main(sys.argv[1:])
