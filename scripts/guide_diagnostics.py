#!/usr/bin/env python3
"""Write `docs/guide/diagnostics.md` from the registry of diagnostic codes.

ADR-0343 (a guide whose examples compile): the guide's reference of codes is
generated from `compiler/pw-core/src/codes.rs` (ADR-0239's one registry), so
it cannot drift from what the compiler emits. Each code with its symbol, the
invariant the registry states for it, the analysis that owns it, and the
ADRs its entry's comment cites; then the retired codes, and the old numbers
the corpus still writes (`DEPRECATED_ALIASES` in `diagnostics.rs`).

`scripts/tests/test_guide_diagnostics.py` (in `just evidence-gates`) holds
the committed page to this script's output, and
`compiler/pw-cli/tests/guide.rs` holds it to `pw_core::codes::ALL`.

Run from anywhere: `python3 scripts/guide_diagnostics.py` rewrites the page;
`--check` says whether it is current and changes nothing.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CODES = ROOT / "compiler/pw-core/src/codes.rs"
ALIASES = ROOT / "compiler/pw-core/src/diagnostics.rs"
DECISIONS = ROOT / "docs/DECISIONS"
PAGE = ROOT / "docs/guide/diagnostics.md"

ENTRY = re.compile(
    r'^\s+([A-Z][A-Z0-9_]*) = "(PW\d{4})" / ([a-z0-9_]+) / (\d+), ([A-Za-z]+),(.*)$'
)
ADR = re.compile(r"ADR-(\d{4})")

# A heading for each owner, in words; an owner not named here is written
# from its own name, so a new one never stops the page being written.
TITLES = {
    "Syntax": "Syntax",
    "Resolution": "Names and imports",
    "Effects": "Effects",
    "DeclarationRules": "Declarations and their clauses",
    "Exhaustiveness": "Matching",
    "ScopeGraph": "Scopes, tasks and resources",
    "Placement": "Placement",
    "Privacy": "Privacy",
    "Markup": "Markup",
    "Types": "Types",
    "Layout": "Layout and frame phases",
    "ResourceGraph": "Caches, keys and invalidation",
    "Capability": "Capabilities",
    "UiState": "Signals and UI state",
    "Streaming": "Streamed regions",
    "Identity": "Accounts and sign-in",
    "Uploads": "Uploads",
    "Notifications": "The typed principal and notifications",
    "Messages": "Direct messages",
    "StorePg": "The store on PostgreSQL",
    "Kiokun": "kiokun",
    "StoreAccounts": "The store's accounts",
    "StoreSearch": "The store's search",
    "Payments": "Payments",
    "Schedules": "Scheduled commands",
}


def title(owner):
    if owner in TITLES:
        return TITLES[owner]
    words = re.findall(r"[A-Z][a-z0-9]*", owner) or [owner]
    return " ".join([words[0]] + [w.lower() for w in words[1:]])


def rust_string(text):
    """The value of the one Rust string literal `text` begins with."""
    if not text.startswith('"'):
        raise ValueError(f"not a string literal: {text[:40]!r}")
    out, i = [], 1
    while i < len(text):
        c = text[i]
        if c == "\\":
            nxt = text[i + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\"}.get(nxt, "\\" + nxt))
            i += 2
            continue
        if c == '"':
            return "".join(out)
        out.append(c)
        i += 1
    raise ValueError("an unclosed string literal")


def entries(source):
    """Each registered code, in the registry's order: (id, symbol, revision,
    owner, invariant, ADRs its entry's comment cites)."""
    lines = source.splitlines()
    start = next(i for i, l in enumerate(lines) if l.startswith("codes! {"))
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == "}")
    out, comment, i = [], [], start + 1
    while i < end:
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("//"):
            comment.append(stripped[2:].strip())
            i += 1
            continue
        if not stripped:
            comment = []
            i += 1
            continue
        m = ENTRY.match(line)
        if not m:
            raise ValueError(f"codes.rs:{i + 1}: not an entry: {line!r}")
        konst, code, symbol, revision, owner, rest = m.groups()
        rest = rest.strip()
        if not rest:
            i += 1
            rest = lines[i].strip()
        invariant = rust_string(rest)
        adrs = []
        for n in ADR.findall(" ".join(comment)):
            if n not in adrs:
                adrs.append(n)
        out.append((code, symbol, int(revision), owner, invariant, adrs))
        comment = []
        i += 1
    return out


def retired(source):
    """(code, why) for each retired code."""
    block = source.split("pub const RETIRED: &[(&str, &str)] = &[", 1)[1].split("];", 1)[0]
    out = []
    # A trailing comma after the reason, as `rustfmt` writes a tuple split
    # across lines, is the tuple's.
    for m in re.finditer(r'\(\s*"(PW\d{4})",\s*((?:"(?:[^"\\]|\\.)*"\s*)+),?\s*\)', block, re.S):
        parts = re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(2), re.S)
        text = "".join(parts)
        # A `\` before a line's end continues the literal past the newline
        # and the next line's indentation.
        text = re.sub(r"\\\n\s*", "", text).replace('\\"', '"')
        out.append((m.group(1), " ".join(text.split())))
    return out


def aliases(source):
    """(old number, the code it means now), in the table's order."""
    block = source.split("pub const DEPRECATED_ALIASES", 1)[1].split("];", 1)[0]
    return re.findall(r'\("(PW\d{4})", "(PW\d{4})"\)', block)


def adr_link(number):
    found = sorted(DECISIONS.glob(f"ADR-{number}-*.md"))
    if not found:
        return number
    return f"[{number}](../DECISIONS/{found[0].name})"


def cell(text):
    """A table cell: a `|` would end it."""
    return text.replace("|", "\\|")


def page():
    codes = entries(CODES.read_text())
    gone = retired(CODES.read_text())
    old = aliases(ALIASES.read_text())
    symbols = {c[0]: c[1] for c in codes}
    cited = sum(1 for c in codes if c[5])
    out = [
        "# Diagnostic codes",
        "",
        "<!-- Written by scripts/guide_diagnostics.py from compiler/pw-core/src/codes.rs:",
        "     do not edit it by hand. Run `python3 scripts/guide_diagnostics.py` after a",
        "     change to the registry; `just ci` fails while this page is not current. -->",
        "",
        "Every diagnostic `pw check` prints carries a code, such as `[PW0606]`, and",
        "says in one sentence what the program must hold to. This page lists every",
        "code the compiler can emit, from its registry, `compiler/pw-core/src/codes.rs`.",
        "",
        "- **Code**: the number `pw check` prints. A number is never reused.",
        "- **Symbol**: the code's name in the compiler and in the corpus's",
        "  fixtures (`@rule`).",
        "- **What the program must hold to**: the registry's own sentence.",
        "- **ADR**: the decisions its registry entry cites, where it cites one.",
        "",
        f"{len(codes)} codes are registered; {cited} of them cite an ADR in the registry.",
        "A code whose entry cites none is marked `—`: search `docs/DECISIONS/` for its",
        "number or its symbol.",
        "",
    ]
    order = []
    for c in codes:
        if c[3] not in order:
            order.append(c[3])
    for owner in order:
        rows = [c for c in codes if c[3] == owner]
        out += [
            f"## {title(owner)}",
            "",
            "| Code | Symbol | What the program must hold to | ADR |",
            "|---|---|---|---|",
        ]
        for code, symbol, _rev, _owner, invariant, adrs in rows:
            links = ", ".join(adr_link(n) for n in adrs) or "—"
            out.append(f"| {code} | `{symbol}` | {cell(invariant)} | {links} |")
        out.append("")
    out += [
        "## Retired codes",
        "",
        "No longer emitted, and never reused.",
        "",
        "| Code | Why |",
        "|---|---|",
    ]
    out += [f"| {code} | {cell(why)} |" for code, why in gone]
    out += [
        "",
        "## Old numbers",
        "",
        "Numbers that named an invariant the code beside them names now: one invariant",
        "has one code. A diagnostic carries that code, never one of these, and a",
        "fixture's `@rule` must name a current registered code. An old number is never",
        "given to another invariant.",
        "",
        "| Old number | Now | Symbol |",
        "|---|---|---|",
    ]
    out += [f"| {a} | {b} | `{symbols.get(b, '?')}` |" for a, b in old]
    out.append("")
    return "\n".join(out)


def main(argv):
    text = page()
    if "--check" in argv:
        current = PAGE.read_text() if PAGE.exists() else ""
        if current != text:
            print(f"{PAGE.relative_to(ROOT)} is not current: run python3 scripts/guide_diagnostics.py")
            return 1
        print(f"{PAGE.relative_to(ROOT)} is current")
        return 0
    PAGE.parent.mkdir(parents=True, exist_ok=True)
    PAGE.write_text(text)
    print(f"wrote {PAGE.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
