"""Follow the InteractionId help as a reader would, with the built `pw`.

For each program in help-demo/: `pw check --plain` it as written, then with
only the clause the help names first, then with the type as well. The help
says both steps are needed: the clause alone must be refused (PW0335), and
both must check clean. Each program is checked with the standard packages,
as the guide's test checks its examples. Usage, from the repository root:
python3 help_demo.py PW_BINARY DEMO_DIR."""
import pathlib
import re
import subprocess
import sys
import tempfile

pw, demo = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
STD = sorted(str(p) for d in ["packages/pw-std", "packages/pw-platform-web"] for p in pathlib.Path(d).glob("*.pw"))
assert STD, "run from the repository root: no standard package found"
NEW = "and `opaque type InteractionId = String` if the program has none"
CASES = [
    # file, the code it is refused with, where the clause goes, where the type goes
    ("twice.pw", "PW0338",
     ("command post(text: String) -> Result<Post, PostError> {\n",
      "command post(text: String) -> Result<Post, PostError>\n    idempotent_by InteractionId\n{\n"),
     ("type Post =", "opaque type InteractionId = String\n\ntype Post =")),
    ("retry.pw", "PW0312",
     ("    retry bounded_exponential(max = 3, jitter = true)\n",
      "    retry bounded_exponential(max = 3, jitter = true)\n    idempotent_by InteractionId\n"),
     ("type Post =", "opaque type InteractionId = String\n\ntype Post =")),
    ("clock.pw", "PW6405",
     ("    invoked_by Clock\n", "    invoked_by Clock\n    idempotent_by InteractionId\n"),
     ("command remind", "opaque type InteractionId = String\n\ncommand remind")),
]


def check(name, src):
    with tempfile.TemporaryDirectory() as d:
        path = pathlib.Path(d) / name
        path.write_text(src)
        r = subprocess.run([str(pw), "check", "--plain", *STD, str(path)], capture_output=True, text=True)
    out = r.stdout + r.stderr
    return r.returncode, sorted(set(re.findall(r"^(?:error|warning): \[(PW\d{4})\]", out, re.M))), out


failed = 0
for name, code, clause, typ in CASES:
    src = (demo / name).read_text()
    assert src.count(clause[0]) == 1 and src.count(typ[0]) == 1, name
    status, codes, out = check(name, src)
    helps = [l.strip() for l in out.splitlines() if l.strip().startswith("= help:") and "InteractionId" in l]
    ok = code in codes and any(NEW in h for h in helps)
    print(f"{name} as written: exit {status}, codes {codes}; {'OK' if ok else 'FAIL'}")
    for h in helps:
        print(f"    {h}")
    failed += not ok
    only = src.replace(*clause)
    status, codes, out = check(name, only)
    ok = "PW0335" in codes and code not in codes
    print(f"{name} with the clause alone: exit {status}, codes {codes}; {'OK' if ok else 'FAIL'}")
    for l in out.splitlines():
        if l.startswith("error: [PW0335]"):
            print(f"    {l}")
    failed += not ok
    both = only.replace(*typ)
    status, codes, out = check(name, both)
    ok = status == 0 and not codes
    print(f"{name} with the clause and the type: exit {status}, codes {codes}; {'OK' if ok else 'FAIL'}")
    if not ok:
        print(out)
    failed += not ok
print(f"{9 - failed} of 9 steps as the help says")
sys.exit(1 if failed else 0)
