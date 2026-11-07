"""Give each `TableSchema { .. }` literal in the named files `owner: None`."""
import re
import sys

for path in sys.argv[1:]:
    lines = open(path).read().split("\n")
    out = []
    i = 0
    edited = 0
    while i < len(lines):
        line = lines[i]
        out.append(line)
        m = re.match(r"^(\s*)(.*)TableSchema \{\s*$", line)
        if m and "struct" not in line:
            indent = m.group(1)
            # the literal's closing brace: the first later line that is
            # `indent}` followed by `,` `;` `)` or nothing
            j = i + 1
            has_owner = False
            while j < len(lines):
                if re.match(rf"^{re.escape(indent)}\}}[,;)]*\s*$", lines[j]):
                    break
                if re.match(r"^\s*owner:", lines[j]):
                    has_owner = True
                j += 1
            for k in range(i + 1, j):
                out.append(lines[k])
            if not has_owner:
                out.append(f"{indent}    owner: None,")
                edited += 1
            i = j
            continue
        i += 1
    if edited:
        open(path, "w").write("\n".join(out))
        print(f"{path}: {edited}")
