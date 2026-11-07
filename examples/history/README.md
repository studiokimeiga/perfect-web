# Corpus history

The text of every fixture **as it was** before the change that let the compiler
enforce it. See `docs/CORPUS.md` for the per-fixture record.

These are not fixtures. They are not counted, not category-checked, and not run
by `pw check` in CI. They exist so that
`compiler/pw-core/tests/corpus_history.rs` can ask one question:

> Does the *older* text still fail?

Nine of the ten changes added an import or a declaration the fixture called and
the program lacked. That repair is legitimate — a fixture that calls something
which exists nowhere is silent because the program is incomplete, not because
the checker is — but it is exactly the shape of change that could also, by
accident, remove the defect instead of the obstruction. If it did, the old text
would stop being rejected, and this test is what notices.

A fixture whose old text is EXPECTED to pass now (because the change corrected
a genuinely wrong specification) belongs in `EXPECTED_TO_PASS` in that test,
with the reason written next to it. There are six, each with its reason
there; R-023's left it in C9, when A-025 gave the program a route table its
old link is dead relative to.

`C8/` holds the texts before the value relations' first run, `C9/` the
routed pages' before each stated its title (ADR-0183), and `C10/` two
witnesses' before a reference to nothing was its own defect (ADR-0185).
`C30/` holds R-027's and a witness's before a key's stale-work policy was
asked where the key changes (ADR-0403): both now check clean, which
is the change. Each version is recorded in `docs/CORPUS.md`.
