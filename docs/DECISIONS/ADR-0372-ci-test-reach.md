# ADR-0372: changed tests plan the checks that consume them

Status: accepted under the owner's delegation, 2026-10-20.

## Finding

Train-11 recorded two silent verification gaps. A change inside a crate
root's test module was treated as `main::` or `lib::`, neither of which
is a Rust test namespace. A changed Python test was never an input to the
mutation-script reach rule. Both gaps reproduced with failing planner
tests before the implementation. They also let old evidence appear current,
because planning and evidence freshness use the same reach functions.

## Decision

For an inline test-module change in `src/main.rs` or `src/lib.rs`, find the
owning package from its Cargo manifest and conservatively select consumers
that name that package's tests or workspace tests. This includes consumers
using bare test-name filters and avoids inventing a `main::` namespace.
Selecting more of that package is preferable to silently missing a witness.
Ordinary source lines still use the existing anchor reach rule.

Python test files under `scripts/tests/` reach mutation scripts and direct
recipe bodies that name the path, its dotted unittest module (including
class/method selectors), or its directory and filename separately as an
explicit discovery command does. Name boundaries prevent prefix neighbors
from counting. This is the existing static-reference model, not execution
of arbitrary test discovery or dynamic command construction.

## Validation

The regression tests cover both Rust root filenames, shell and argv package
selectors, workspace consumers, unrelated packages and package prefixes,
Python paths, dotted selectors, explicit discovery, and unknown tests.
Mutation controls remove each new script/direct-recipe reach edge.
Commands and results are recorded after execution; no remote gate is claimed
by the presence of this note.

## Limits

Package selection is conservative and may run more Rust recipes than strictly
necessary. Existing inline-module recognition is unchanged; custom Rust
module layouts and dynamically constructed Python test commands remain
outside this static-reference rule. No application language semantics change.

## Evidence consumer correction found during validation

The untouched master at 115895de fails the evidence-count gate once the
train-11 evidence is current. Its guide recipe hashes both mutation scripts
but runs only the producer campaign. The gate mistook the hash input for
execution. It also chose an intended test-method ID as the description
because that identifier was longer than the actual mutant description.
Both have red-before/green-after regression tests and independent controls.

Campaign discovery now tokenizes shell text and recognizes literal Python
invocations (including supported interpreter flags), preserving quoted
strings. Hashing or printing a filename is not running it. Method IDs beside
a mutation description are metadata; they cannot replace its description.
Actual second campaigns still require their own complete evidence blocks.
Dynamic shell execution remains outside this static-reference model.

## Bare module selectors in native producer inventories, 2026-10-20

The train-12 follow-up plan omitted `e14-mutation-bound` after a change to
`test_ci_scripts.py`. That campaign names `scripts/tests` and the exact bare
module `test_ci_scripts` in a tuple; it constructs the `.py` discovery pattern
later for the shared native producer. The existing rule sees only a filename
with its suffix or a fully qualified module and therefore misses this reader.

Extend the same static directory/name rule to exact bare module selectors,
including class/method suffixes, while retaining name boundaries. An unrelated
module with the same prefix or the same basename in another directory remains
unselected. A regression must require the actual campaign and its recipe, plus
synthetic bare/class/neighbor consumers. A control removes just the new edge.
No separate recipe allowlist is introduced: freshness and planning continue
using the same canonical function. Whole affected verification remains due.

[Native before/after and whole nine-control output](../evidence/tooling/ci-bare-module-reach.txt):
the real consumer was omitted before the correction; both focused cases and
all nine strict controls pass afterward, with no producer errors or memory
stops. Exact hosted follow-up and final combined gates remain pending.


## Landing (2026-10-21)

Landed on `master` with train-12 at `e7664d2eb01aa8c6e91df594bd7270a84a8bf8de`
after exact-commit CI ([39584219053](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219053))
and automatic track verification ([39575516650](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516650)).
Dated statements above that call landing or combined gates pending describe
the candidate before it landed and are kept as written. The
[acceptance record](../diagnostics/train-12/acceptance-record-2026-10-21.md)
states what the landing's evidence does and does not prove.

## Amended 2026-11-16: a whole test module declared out of line

The integrator's, under the owner's delegation of 2026-10-02.

**Finding.** The Limits above left "custom Rust module layouts" outside the
rule, and one such layout is ordinary Rust: a whole test module in a file of
its own, declared under `#[cfg(test)]` by the module above it (`#[cfg(test)]
mod failure_tests;` in `pw-table-memory`'s `lib.rs`). It is under no
`tests/` directory and holds no inline `mod tests {`, so a change to it
planned nothing. The nightly of 2026-11-15 (verify 43721856914) found
`e14-d2-writes` INVALID by a probe's `unwrap` in `failure_tests.rs`, and the
push of its fix planned no recipe (`ci_plan.py --changed` gave `shards=[]`):
the fix's verify would not have run the recipe it fixes, and the evidence
fetched after it would have been taken for current. Six such files are in
the repository: `failure_tests.rs` (`pw-table-memory`), `test_assertions.rs`
(`pw-host`), `tests.rs` (`pw-outbound`, and `tools/fuzz`), `queue_tests.rs`
(`pw-core`'s `effects`), and `privacy_tests.rs` (the server's `uploads`).

**Decision.** `ci_plan.test_module_files` finds a changed `.rs` file that its
declaring file (where Rust looks for it: `lib.rs`, `main.rs` or `mod.rs`
beside it, or the file named for its directory) declares under
`#[cfg(test)]`, other attributes and a visibility allowed between them. Such
a file reaches what an inline test module reaches: its own name and its
declaring module's, as filters (`test_modules`); and, declared by a crate's
root, its package's tests, run whole (`root_test_packages`), as a root's
inline module does. A module named `tests` is no filter, which would name
every crate's; its package reaches it. A module no `#[cfg(test)]` declares
is untouched by the rule, whatever it holds.

**Validation.** `scripts/tests/test_ci_scripts.py`'s
`test_an_out_of_line_test_module_reaches_what_runs_it`: the real
`failure_tests.rs` plans `d2_writes_mutations.py` and `e14-d2-writes` (it
failed before the change), and a fixture crate holds each case: a root's
module (its package and filter), a root's `tests` (its package only), a
nested module (its filter and its parent's, not the package) and an
undeclared module (nothing). `ci_test_reach_mutations.py` gains five
mutants, one for each edge, and the old root mutant is re-anchored: 15 of 15
killed. Each of the six files now plans the recipes that run it (1 to 21
each).

**Limits.** A nested module's change does not reach a consumer that runs its
package's tests whole by `-p` alone, as a nested inline module's does not;
`#[path]` attributes and modules declared by a macro stay outside the rule.
