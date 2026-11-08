"""The requires migration's hand cases, each by its exact head (run in
compiler/pw-core/tests). A literal backslash-n in the Rust source is ESC."""
import re

ESC = "\\n"
CASES = [
    ("principal.rs", "command noted() -> Int"),
    ("transition_values.rs", "command c(x: Int) -> Result<Int, String>"),
    ("value_relations.rs", "command add(k: Int) -> Result<Cart, E>"),
    ("value_relations.rs", "command add(k: Int) -> Result<C, E>"),
    ("generic_effects.rs", "command Put(id: Int) -> Result<Int, String>"),
    ("clauses_read_once.rs", "command search() -> Result<Int, String>"),
]
for path, head in CASES:
    t = open(path).read()
    pat = re.compile(re.escape(head) + "(?!" + re.escape(ESC + "    requires") + ")(?=" + re.escape(ESC) + ")")
    new, n = pat.subn(lambda m: m.group(0) + ESC + "    requires Anyone", t)
    open(path, "w").write(new)
    print(path, head, n)

p = "clause_places.rs"
t = open(p).read()
old = 'fn with(head: &str, clause: &str) -> String {\n    format!("{DECLS}{head} -> Int !{{}}' + ESC + '    {clause}' + ESC + '{{' + ESC + '    0' + ESC + '}}' + ESC + '")\n}'
assert t.count(old) == 1, "helper not found"
new = (
    'fn with(head: &str, clause: &str) -> String {\n'
    '    // A command says who may run it (the requires ADR).\n'
    '    let who = if head.starts_with("command ") { "' + ESC + '    requires Anyone" } else { "" };\n'
    '    format!("{DECLS}{head} -> Int !{{}}{who}' + ESC + '    {clause}' + ESC + '{{' + ESC + '    0' + ESC + '}}' + ESC + '")\n}'
)
open(p, "w").write(t.replace(old, new))
print(p, "helper")
