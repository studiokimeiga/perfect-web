"""PW0367's migration, for the programs inside other crates' tests that the
requires ADR's first pass (pw-core's tests) did not reach: each command a
reader can run says `requires Anyone`, as anyone could run it before. Exact
edits, each matched once. Run from #83's worktree root."""
import pathlib
import sys

EDITS = {
    "compiler/pw-conformance/tests/case_arguments.rs": [
        (f"command {name}({params}) -> Result<Int, String> {{\n",
         f"command {name}({params}) -> Result<Int, String>\n    requires Anyone\n{{\n")
        for name, params in [
            ("tipped", "t: Tip"),
            ("sized", "s: Size"),
            ("maybe", "n: Option<Int>"),
            ("either", "r: Result<Int, String>"),
        ]
    ] + [
        (f"command {name}({params}) -> Result<Int, String>\n    idempotent_by InteractionId\n",
         f"command {name}({params}) -> Result<Int, String>\n    requires      Anyone\n    idempotent_by InteractionId\n")
        for name, params in [
            ("choose", "t: Tip"),
            ("maybe_pressed", "n: Option<Int>"),
            ("either_pressed", "r: Result<Int, String>"),
            ("planted", "t: Tree"),
        ]
    ],
    "spikes/own-renderer/server/src/tests/store_scheduled.rs": [
        ("command add_in_five(item: MenuItemId, refuse: Bool)\n    -> Result<Int, CartError>\n    idempotent_by InteractionId\n",
         "command add_in_five(item: MenuItemId, refuse: Bool)\n    -> Result<Int, CartError>\n    requires      Anyone\n    idempotent_by InteractionId\n"),
        ("command sweep_in_five(order: String)\n    -> Result<Int, CartError>\n    idempotent_by InteractionId\n",
         "command sweep_in_five(order: String)\n    -> Result<Int, CartError>\n    requires      Anyone\n    idempotent_by InteractionId\n"),
    ],
    "spikes/own-renderer/server/src/tests/scheduled_replay.rs": [
        (f"command {head} -> Result<Int, CartError>\n    idempotent_by InteractionId\n",
         f"command {head} -> Result<Int, CartError>\n    requires      Anyone\n    idempotent_by InteractionId\n")
        for head in [
            "queue_pure()",
            "queue_notice()",
            "queue_failure(item: MenuItemId)",
            "queue_repeat(item: MenuItemId)",
        ]
    ],
}

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
for rel, edits in EDITS.items():
    path = root / rel
    text = path.read_text()
    for old, new in edits:
        n = text.count(old)
        if n != 1:
            raise SystemExit(f"{rel}: {n} matches for {old!r}")
        text = text.replace(old, new)
    path.write_text(text)
    print(f"{rel}: {len(edits)} commands")
