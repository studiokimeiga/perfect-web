"""Begin each memory-adapter test command for a test reader (owned-rows)."""
import glob
import re
import sys

HELPER = """
/// The reader every command here is begun for (the owned-rows ADR): one
/// request's, in a table no reader owns, where it changes any row.
fn a_reader() -> pw_host::tables::Writer {
    pw_host::tables::Writer::Reader(
        pw_host::tables::Reader::new("session-1", None).expect("a reader"),
    )
}"""

root = sys.argv[1]
crate = sys.argv[2]  # e.g. runtime/pw-table-memory
files = glob.glob(f"{root}/{crate}/src/*tests*.rs") + sorted(glob.glob(f"{root}/{crate}/tests/*.rs"))
for path in files:
    s = open(path).read()
    orig = s
    s = s.replace(".begin()", ".begin(a_reader())")
    s = re.sub(r"\.transaction\(\s*\|", ".transaction(a_reader(), |", s)
    s = re.sub(r"\.transaction\(move \|", ".transaction(a_reader(), move |", s)
    if s != orig and "fn reader()" not in s:
        lines = s.split("\n")
        last = -1
        for i, line in enumerate(lines):
            if line.startswith(("fn ", "pub fn ", "const ", "static ", "#[test]", "struct ", "///", "#[derive")):
                break
            if line.startswith(("use ", "mod ", "#[path", "pub use ")):
                last = i
        j = last
        while j >= 0 and j < len(lines) and not lines[j].rstrip().endswith(";"):
            j += 1
        lines.insert(j + 1, HELPER)
        s = "\n".join(lines)
    if s != orig:
        open(path, "w").write(s)
        print("edited", path)
